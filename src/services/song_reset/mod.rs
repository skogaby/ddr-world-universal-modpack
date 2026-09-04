//! Song Reset Service — in-place rewind of the live gameplay run.
//!
//! Rewinds the current song to 0:00 **without any teardown**: the
//! `DancePlaySequence` (DPS), both `GamePlayActor`s and every HUD child
//! stay alive; only their *state* is reset. The engine hands us the hard
//! part: msg `0x1044` (the timing anchor DPS state 6 broadcasts once at
//! song start) is handled by each GamePlayActor as a full in-place rewind
//! — it re-anchors the music clock, **discards and rebuilds the entire
//! judge-record vector from the pristine note list at playhead 0**,
//! rebuilds the density array and re-enters the in-song step. What the
//! engine does not reset is a small enumerated surface this service owns:
//! the score/combo/judge-count accumulator block, the gauge child, and
//! the HUD refresh.
//!
//! Sequence (design: `.agents/planning/20260812-inplace-restart/design.md`,
//! offsets: `research/run_state_re.md` in the same dir):
//!
//! 1. **Gates** — scene GAMEPLAY, live DPS at step 7 (in-song), every
//!    GamePlayActor at step 4, non-course, per-song gauge snapshot
//!    captured, all resolutions present. Any failure ⇒ `Refused` and the
//!    caller falls back to its own restart path.
//! 2. **Phase 1 (synchronous)** — stop the song cue (`DPS+0x128`), replay
//!    it from the still-registered bank slot 5 (zero disk I/O), store the
//!    new handle back to `DPS+0x128`.
//! 3. **Phase 2 (per-frame driver)** — poll the cue-prepared byte; when
//!    ready, in ONE synchronous frame callback: broadcast `0x1043` then
//!    `0x1044 {fresh tick}` to the DPS subtree (the engine's own
//!    protocol, delivered with the engine's own broadcast primitive),
//!    THEN zero the accumulators, restore the gauges (values latched at
//!    song start), refresh the HUD (`0x1033`/`0x1045`/`0x103F`), and
//!    notify `on_song_reset` subscribers.
//!
//! Threading: `request_reset` must be called on the frame thread (the
//! input-poll/gesture context qualifies). The driver and the snapshot
//! probe run through `widget_renderer::run_on_render_thread`. No lock is
//! held across any engine call.
//!
//! The `t_ms` parameter exists for Training Mode (seek-to-T uses the same
//! judge-record rebuild with a nonzero playhead); this phase implements
//! `t_ms == 0` only.

pub mod seek;
#[cfg(test)]
mod seek_tests;

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use once_cell::sync::Lazy;

use crate::core::signatures::{GamePlayActorLayout, SignatureStore};
use crate::core::{memory, module_resolver};
use crate::services::{bm2d_api, scene_manager, song_rate, widget_renderer};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

// ── Actor-tree offsets (shared layout facts, same as quick_restart) ──
/// `*(TS + 0x58)` = the active gosub child (the DPS during GAMEPLAY).
const ACTIVE_CHILD_OFFSET: usize = 0x58;
const FIRST_CHILD_OFFSET: usize = 0x18;
const NEXT_SIBLING_OFFSET: usize = 0x10;
/// Actor tree flags; 0x20 = dispatch suppressed, 0x24 = dead-or-dying.
const TREE_FLAGS_OFFSET: usize = 0x20;
const TREE_FLAGS_DISPATCH_SUPPRESSED: u32 = 0x20;
const TREE_FLAGS_DEAD_MASK: u32 = 0x24;
/// vtable slot of `agcs::Actor::onMessage(this, msg, param)`.
const VTBL_ON_MESSAGE_OFFSET: usize = 0x18;

// ── DancePlaySequence fields ─────────────────────────────────────────
/// DPS `agcs::StackStep`: values at +0x68, depth index (u16) at +0x92.
const DPS_STEP_BASE: usize = 0x68;
const DPS_STEP_INDEX: usize = 0x92;
/// DPS in-song step (state 7 of `DancePlaySequence::update`).
const DPS_STEP_IN_SONG: i32 = 7;
/// Song basename `std::string` (MSVC, 0x20 bytes: buf/ptr, len +0x10,
/// cap +0x18) and the variant-suffix string right after it. Their
/// concatenation is the bank name DPS state 4 plays (slot 5).
const DPS_BASENAME_OFFSET: usize = 0xA0;
const DPS_SUFFIX_OFFSET: usize = 0xC8;
/// The song cue handle (i32) DPS state 4 stores and states 8/leave stop.
const DPS_CUE_HANDLE_OFFSET: usize = 0x128;
/// m_courseMaxStage — values > 1 mean a multi-song course (blocked).
const DPS_COURSE_MAX_OFFSET: usize = 0x98;

// ── GamePlayActor fields (run_state_re.md §2.1) ──────────────────────
/// GamePlayActor `agcs::StackStep`: values at +0x58, index at +0x82.
const GPA_STEP_BASE: usize = 0x58;
const GPA_STEP_INDEX: usize = 0x82;
/// The in-song actor step — the ONLY step the 0x1044 rewind is valid in
/// (the handler's own gate is {3,4}; we require 4).
const GPA_STEP_IN_SONG: i32 = 4;
/// Play side (0/1).
const GPA_SIDE_OFFSET: usize = 0x84;
/// Music clock anchor tick (i64 — research §2.2/§6: `music_count =
/// vt+0x248() + frameTick − SOUND_OFFSET − anchor@+0x160`). Zero until the
/// run's first `0x1044` lands; nonzero = the clock is anchored (the
/// Step-3 driver's first-anchored-frame input).
const GPA_ANCHOR_OFFSET: usize = 0x160;
/// Judge counts by grade: 8 × i32 at +0x1A0 + grade*4.
const GPA_JUDGE_COUNTS_OFFSET: usize = 0x1A0;
const GPA_JUDGE_COUNT_SLOTS: usize = 8;
/// Freeze-OK count.
const GPA_FREEZE_OK_OFFSET: usize = 0x1C0;
/// FAST / SLOW counts.
const GPA_FAST_OFFSET: usize = 0x1C4;
const GPA_SLOW_OFFSET: usize = 0x1C8;
/// Judged-event count (jumps may count double).
const GPA_JUDGED_EVENTS_OFFSET: usize = 0x1CC;
/// Money score / EX score / combo / max combo.
const GPA_SCORE_OFFSET: usize = 0x1D4;
const GPA_EX_SCORE_OFFSET: usize = 0x1D8;
const GPA_COMBO_OFFSET: usize = 0x1DC;
const GPA_MAX_COMBO_OFFSET: usize = 0x1E0;
/// Consecutive-miss streak (no-play detection reads ≥ 0x32).
const GPA_MISS_STREAK_OFFSET: usize = 0x1E4;
/// m_isDead / song-finished / risky death-result flags.
const GPA_IS_DEAD_OFFSET: usize = 0x1E8;
const GPA_SONG_FINISHED_OFFSET: usize = 0x1E9;
// The death-result flag, the instant-death gauge gate
// (`m_canInstantDeath`-equivalent, quick restart's death-flag anatomy + the
// 20260721 decompile: `0x103C`'s STEP_GAME_OVER advance and the DPS
// finish-poll's death arm are BOTH conditioned on this byte being 0), and
// the gauge-percent tracking cluster (min ctor 1.0, max, last, accumulated
// loss/gain ctor 0.0 — restored to the ctor values; the gauge's own
// post-reset 0x103F broadcast re-seeds them exactly like song start) all
// live in the GamePlayActor region that sits 8 bytes LOWER on 20250805 /
// 20260224 than on 20260324+ (`+0x2B8/+0x2B7/+0x2A0..` vs
// `+0x2B0/+0x2AF/+0x298..`). They come from
// `SignatureStore::gameplay_actor_layout()` (derived from the actor ctor's
// seed block); the service is unavailable without it.
static GPA_LAYOUT: std::sync::OnceLock<GamePlayActorLayout> = std::sync::OnceLock::new();

/// The derived GamePlayActor layout. `init` refuses without it, so every
/// reset path that reaches here has it; the fallback keeps the callers
/// honest if one ever runs before `init`.
fn gpa_layout() -> Option<GamePlayActorLayout> {
    GPA_LAYOUT.get().copied()
}

// ── Gauge actor fields (run_state_re.md §5) ──────────────────────────
/// Percent-family (`GaugeActor` base): live value, fixed-point 0..10000.
const GAUGE_VALUE_OFFSET: usize = 0x90;
// DISPLAY LATCHES — deliberately NOT touched by the reset (v6,
// 2026-08-16 cabinet findings): +0x94 displayed percent, +0x98 display
// velocity, +0x9C last display-state enum. The per-frame update
// (FUN_180073f90) EARLY-OUTS while |displayed − value/10000| < ε, and
// the gauge_usr color re-label AND the 0x1037/0x1038 danger on/off
// transitions all live BEHIND that gate, keyed on old-state@+0x9C vs
// the fresh classify. The v5 reset snapped +0x94 to the restored value
// and zeroed +0x9C — which (a) made the update skip forever, freezing
// the gauge COLOR at its pre-reset label until the first judge moved
// the value, and (b) destroyed the "was in danger" evidence, so the
// engine never emitted the 0x1038 danger-off and the lane kept flashing
// red. Leaving all three latches STALE makes the very next update tick
// take the full path: it animates toward the restored value,
// re-classifies from the true old state, re-labels the color, and
// emits the exact danger-off broadcast through the engine's own
// plumbing.
// NOTE: +0xB0 is the gauge's AFP layer object POINTER (8 bytes; the
// per-frame update dereferences it as `afp_layer_mc_refer(*(layer+8),
// "gauge_usr")`). The ctor zeroes it only because the layer does not
// exist yet — mid-song it is live and MUST NOT be touched (nulling it
// crashed the first cabinet deploy, 2026-08-12).
/// Emptied latch: once set the gauge stays dead (all deltas forced 0).
const GAUGE_EMPTIED_OFFSET: usize = 0xB8;
/// Cached combo/score display values (msgs 0x1045/0x1033), +0xBC..+0xC8.
const GAUGE_CACHE_FIRST_OFFSET: usize = 0xBC;
const GAUGE_CACHE_SLOTS: usize = 4;
/// Internal miss-streak + ctor-zeroed tail (+0xCC/+0xD0/+0xD4).
const GAUGE_MISS_STREAK_OFFSET: usize = 0xCC;
const GAUGE_D0_OFFSET: usize = 0xD0;
const GAUGE_D4_OFFSET: usize = 0xD4;
/// Risky-depleted latch.
const GAUGE_RISKY_DEPLETED_OFFSET: usize = 0xD9;
/// Fixed-point scale for the percent family (10000.0f in the binary).
const GAUGE_VALUE_SCALE: f32 = 10000.0;
/// Sanity bound for a snapshotted percent-gauge value.
const GAUGE_VALUE_MAX: i32 = 10000;

/// LifeGaugeActor (LIFE4/RISKY — NOT a GaugeActor subclass): lives
/// remaining at +0x90; +0x94 is the update's LAST-DISPLAYED lives latch
/// (the diff driver — the update runs its damage-frame/danger-message
/// block only while `+0x94 != +0x90`, then latches `+0x94 = +0x90`;
/// v6 correction — the original table called it "starting lives"
/// because the ctor seeds both from the lives param). Grade threshold
/// +0x98, max at +0x9C, display-mode latch at +0xA0 (0 normal / 1 ...
/// / 2 danger — its transitions emit 0x1037/0x1038 exactly like the
/// percent family). +0xA8 is a qword slot the ctor nulls (this actor's
/// own layer/timer — never touched mid-song). Dead latch is the BYTE at
/// +0xB0 (`start <= 0` at ctor); +0xB4 is a display substate dword
/// (NOT touched — same stale-latch principle as the percent family).
const LIFE_GAUGE_LIVES_OFFSET: usize = 0x90;
const LIFE_GAUGE_DEAD_LATCH_OFFSET: usize = 0xB0;
/// Sanity bound for a lives count.
const LIFE_GAUGE_MAX_LIVES: i32 = 16;

// ── FlareGaugeActor extension fields (2026 layout — attested by the
// `flare_gauge_ctor_layout` AOB, which pins every one of these offsets
// as a literal disp32; Ghidra-verified on 20260324/20260421/20260526/
// 20260616/20260721). The REAL FlareGaugeActor is the 0x138-byte class
// built for gauge options 1..0xB (1 = FLOATING, 2..10 = FLARE I..IX,
// 0xB = FLARE EX) — run_state_re.md §5's table had it swapped with
// GradeGaugeActor (option 0xE). ─────────────────────────────────────
/// Consecutive good-judge streak (course-carry pressure term; ctor 0).
const FLARE_STREAK_OFFSET: usize = 0xE4;
/// Per-grade judge-history counters (8 × i32, ctor 0) — the FLOATING
/// demotion input: `calcJudgePoint` recomputes `Σ counter[g] ×
/// weight(g, level)` on EVERY judge and demotes (multi-level loop,
/// written to Option+0x7C) when the lifetime total hits -10000. Stale
/// counters after an in-place reset are THE floating-flare bug: each
/// aborted attempt's misses (plus the prepare-window phantom misses)
/// accumulate until an early miss on a fresh attempt cascades a
/// multi-level demotion.
const FLARE_HISTORY_FIRST_OFFSET: usize = 0xEC;
const FLARE_HISTORY_SLOTS: usize = 8;
/// Per-level gauge array (11 × i32, ctor 10000 each; course carry-in
/// overwrites) — the course-mode floating walk's state. Snapshot-
/// restored for shape fidelity even though course play is gate-refused.
const FLARE_LEVELS_FIRST_OFFSET: usize = 0x10C;
const FLARE_LEVEL_SLOTS: usize = 11;

// ── FlareGaugeActor pre-20260324 layout (attested by the
// `flare_gauge_ctor_layout_v1` AOB; 20250805 / 20260224): side +0xE0, the
// same 8 per-grade history counters at +0xE4..+0x100, and NO streak or
// per-level array (no course carry on those builds). The FLOATING demote
// loop is otherwise identical, so the restore only zeroes the counters. ──
const FLARE_V1_HISTORY_FIRST_OFFSET: usize = 0xE4;
/// True when the v1 attestation matched (and the 2026 one did not).
static FLARE_LAYOUT_V1: AtomicBool = AtomicBool::new(false);

// ── ddr::player::Option (flare level home) ──────────────────────────
/// Each side's Option is `*(player_option_table[side]) + 0xE0` (the
/// game's own accessor shape — same chain assist_tick uses for
/// JUDGMENT TIMING).
// (build-dependent: 0xE0 on 20260324+, 0xF0 on 20250805 / 20260224 —
// resolved via `stage_records::player_option_offset()`).
/// The CURRENT flare level (1..=10, 10 = EX) — Option vt+0x1A0 setter /
/// vt+0x310 getter are plain accessors of this field (Ghidra-verified on
/// 20260324 and 20260721). GamePlayActor onSetup seeds it from the gauge
/// option (FLOATING → 10); the FLOATING demote loop lowers it mid-song.
/// A full teardown restart re-runs onSetup (which is why the old
/// scene-jump restart restored Floating Flare); the in-place reset must
/// restore the song-start snapshot itself.
const OPTION_FLARE_LEVEL_OFFSET: usize = 0x7C;
/// Sanity bounds for a live flare level.
const FLARE_LEVEL_MIN: i32 = 1;
const FLARE_LEVEL_MAX: i32 = 10;

// ── GradeGaugeActor extension field (2026 layout — attested by the
// `grade_gauge_ctor_layout` AOB). The 0xE8-byte class built for gauge
// option 0xE. ────────────────────────────────────────────────────────
/// Best-EX-score watermark: `calcJudgePoint` (FUN_180075360) multiplies
/// the miss penalty while the current EX score has not grown past it,
/// and rewrites it on every judge (good judge ⇒ current EX; miss ⇒
/// clamped to EX+1000). Survives an in-place reset — the EX score
/// restarts at 0 against a fat pre-reset watermark, over-penalizing
/// early misses on the replayed run until the first good judge rewrites
/// it. Reset to the ctor's sentinel.
const GRADE_WATERMARK_OFFSET: usize = 0xE0;
/// The ctor's watermark seed (INT_MIN — "no EX score seen yet").
const GRADE_WATERMARK_SENTINEL: i32 = i32::MIN;

/// Frame-clock struct: the current tick (ms domain) lives at +0x1268 —
/// the exact value DPS state 6 broadcasts as the 0x1044 anchor.
const FRAME_TICK_FIELD_OFFSET: usize = 0x1268;

// ── Seek-to-T machinery (Training Mode, design §4.4) ─────────────────
/// GamePlayActor note vector begin/end pointers (0x60-stride notes —
/// `seek::NOTE_STRIDE`; rewind-worker decompile, 2026-08-13).
const GPA_NOTES_BEGIN_OFFSET: usize = 0x90;
const GPA_NOTES_END_OFFSET: usize = 0x98;
/// Judge-record vector begin/end pointers (0x40 stride —
/// `seek::RECORD_STRIDE`).
const GPA_RECORDS_BEGIN_OFFSET: usize = 0xB0;
const GPA_RECORDS_END_OFFSET: usize = 0xB8;
/// The three note-population counts the rewind worker sums for the
/// reserve call (+0x194 + 0x198 + 0x19C).
const GPA_NOTE_COUNT_OFFSETS: [usize; 3] = [0x194, 0x198, 0x19C];
/// The trio's third member (`+0x19C`) is NOT a static population: it is
/// a per-run DYNAMIC counter the engine increments once per freeze-head
/// "arm" conversion (each such conversion also awards a grade-6
/// judgment — numerator and denominator of the money score grow in
/// lockstep, keeping a full pass at exactly 1,000,000). It is never
/// zeroed by the engine's 0x1044 rewind, while the conversions' one-shot
/// state lives in the NOTE vector (also never rebuilt) — so a replayed
/// pass keeps the fat denominator but can never re-earn the numerator:
/// the 2026-08-14 loop score caps solved exactly as this skew (e.g.
/// blli: 456×100000/490 = 93061 → 930,610). The per-song snapshot
/// latches its song-start baseline and [`reset_side_state`] restores it,
/// making every in-place reset score-identical to a natural song start
/// (the engine's own start-of-song reserve undershoots the record count
/// the same way — the record vector's append growth is its everyday
/// path).
const GPA_DYNAMIC_POP_OFFSET: usize = 0x19C;
/// The per-frame RAW music count (i32 ms, judge domain — research §3.1;
/// the rewind worker zeroes it). The training gestures' "current
/// position" source.
const GPA_RAW_COUNT_OFFSET: usize = 0x178;
/// Sanity range for a live raw music count (the pre-song approach runs
/// negative; an hour bounds any chart).
const RAW_COUNT_SANE_MIN_MS: i32 = -60_000;
/// Sanity bound for the summed record count (a chart holds thousands).
const RECORD_COUNT_SANE_MAX: i64 = 200_000;
/// Sanity bound for the note vector's byte length.
const NOTE_VECTOR_MAX_BYTES: usize = 4 << 20;

/// ControlMessageActor's embedded StackStep (same shape as DPS/GPA):
/// values at +0x58, depth index u16 at +0x82.
const CMA_STEP_BASE: usize = 0x58;
const CMA_STEP_INDEX: usize = 0x82;
/// The end cascade's DISPLAY-domain last-note-end threshold (fires
/// 0x104A at StackStep 3→4 — chart content over; research §4.1). The
/// display domain is beat-proportional at a raw-ms-comparable scale, so
/// the raw sanity range applies to it too.
const CMA_CHART_END_DISPLAY_OFFSET: usize = 0x94;
/// The end cascade's raw-domain song-over threshold (fires 0x104B at
/// StackStep 4→5) — the seek clamp bound (research §4.3).
const CMA_CHART_END_RAW_OFFSET: usize = 0x98;
/// Cascade steps ≥ this have fired 0x104A (chart content over): the run
/// is inside the one-way end machinery and seeks refuse.
const CMA_STEP_CONTENT_OVER: i32 = 4;
/// Seek targets stay this far below `chart_end_raw` — a seek past the
/// thresholds hard-ends the song unresettably (the cascade is one-way).
const SEEK_END_MARGIN_MS: i32 = 1_000;
/// Sanity range for a chart end (raw ms; an hour dwarfs any chart).
/// Public: also the training loop's raised-`+0x94` parking value — the
/// largest threshold [`set_chart_end_thresholds`] accepts, unreachable
/// by any real pass's display count (each loop iteration re-anchors the
/// clock, so counts never approach an hour).
pub const CHART_END_SANE_MAX_MS: i32 = 3_600_000;

/// The rebuild worker's context arg (`&{actor, playhead}` in R9): the
/// rewind worker builds it as actor qword + playhead dword (high dword
/// unused) at [RSP+0x20].
#[repr(C)]
struct RebuildContext {
    actor: *mut u8,
    playhead: i32,
    _pad: i32,
}

/// `clear(records_vec)` — resets the vector's end to its begin.
type RebuildClearFn = unsafe extern "C" fn(*mut u8);
/// `reserve(records_vec, count)` — game-heap capacity for `count` records.
type RebuildReserveFn = unsafe extern "C" fn(*mut u8, i64);
/// `rebuild(out², notes_begin, notes_end, ctx)` — appends one record per
/// non-control note at the context's playhead; writes 2 qwords to `out`.
type RebuildRebuildFn =
    unsafe extern "C" fn(*mut u8, *const u8, *const u8, *const RebuildContext) -> *mut u8;

// ── ScoreActor fields (run_state_re.md §5a) ──────────────────────────
/// Score display target (raised by msg 0x1036 and the rival-sync
/// update).
const SCORE_ACTOR_TARGET_OFFSET: usize = 0x68;
/// Displayed (tweened) score. The ctor seeds **-1**, which forces the
/// render pass to repaint every digit (its per-digit diff
/// `displayed%10 != target%10` skips unchanged digits — resetting the
/// score 1,020,300 → 0 without the sentinel leaves the mid-number
/// significant zeros lit with stale bright bitmaps; observed on the
/// 2026-08-12 v2 cabinet run as score reading "0, 0" after resets).
const SCORE_ACTOR_DISPLAYED_OFFSET: usize = 0x6C;
const SCORE_ACTOR_DISPLAYED_SENTINEL: i32 = -1;

// ── NoteResultActor / pacemaker clip (0x18007a450 / 0x18007b300 on
// 20260721; layout attested by the ctor's `88 99 C0 00 00 00` byte write
// on 20260526/20260616/20260721) ─────────────────────────────────────
/// `dance_score_compare` CMovieClip wrapper — the pacemaker readout the
/// PUS ms-error swap rides. Its msg-0x1036 update refuses once the
/// clip's current frame reaches the frame of label "out" (msg 0x103A,
/// the pacemaker outro, jumps it there whenever a gauge empties / LIFE4
/// lives hit 0). Natural song flow destroys the actor with the scene, so
/// stock never notices the latch — the in-place reset reuses the actor
/// and must rewind the clip to its song-start state (frame 0, paused:
/// exactly what the actor's onSetup produces).
const NOTE_RESULT_PACEMAKER_CLIP_OFFSET: usize = 0xB0;
/// CMovieClip wrapper: AFP layer id (the onSetup `afp_layer_play(id, 0)`
/// target).
const CLIP_LAYER_ID_OFFSET: usize = 0x08;
/// CMovieClip wrapper: MovieClip id (the SetFrame/SetFrameLabel target —
/// the engine's own case-0x1032 SetFrame reads it here).
const CLIP_MC_ID_OFFSET: usize = 0x110;
/// `afp_mc_op` SetFrame opcode (BM2D::CMovieClip::SetFrame).
const MC_OP_SET_FRAME: i32 = 0xF08;

// ── Engine messages (run_state_re.md §9) ─────────────────────────────
/// Pre-start arm (input/start protocol) — DPS state 5.
const MSG_PRE_START_ARM: i32 = 0x1043;
/// Timing anchor {i64 tick} — DPS state 6; the per-actor rewind.
const MSG_TIMING_ANCHOR: i32 = 0x1044;
/// Combo/score HUD update {side, combo, maxCombo, grade, flag:u8}.
const MSG_COMBO_UPDATE: i32 = 0x1033;
/// Gauge percent report {side, pct:f32}.
const MSG_GAUGE_PERCENT: i32 = 0x103F;

/// How long Phase 2 waits for the replayed cue to prepare before giving
/// up (the cabinet-measured prepare is ~0.1 s; 5 s means something is
/// genuinely wrong).
const PREPARE_TIMEOUT_SECS: u64 = 5;

/// Delayed-restart countdown: how far before the countdown's end the cue
/// replay is issued. The prepare completed in ≤16 ms on the cabinet;
/// 250 ms is generous margin so "prepared" always lands before the
/// countdown expires (audio start is tied to prepare completion, so the
/// replay must NOT be issued at gesture time — that would run the audio
/// the full delay ahead of the future-dated clock).
const REPLAY_LEAD_MS: u64 = 250;
/// Upper clamp for the configured restart delay.
const MAX_DELAY_MS: i32 = 10_000;

// ── Resolved engine entry points ─────────────────────────────────────
/// `song_play_by_bank(slot, name) -> i32 handle` (slot 5 = per-song bank).
type SongPlayFn = unsafe extern "C" fn(i32, *const u8) -> i32;
/// `song_stop_by_handle(handle)`.
type SongStopFn = unsafe extern "C" fn(i32);
/// `song_is_prepared(handle) -> bool`.
type SongPreparedFn = unsafe extern "C" fn(i32) -> u8;
/// `update_broadcast(actor, msg, param, depth)` — the engine's own
/// subtree message delivery.
type BroadcastFn = unsafe extern "C" fn(*mut u8, i32, *mut u8, i32);
/// `agcs::Actor::onMessage(this, msg, param) -> handled`.
type OnMessageFn = unsafe extern "C" fn(*mut u8, i32, *mut u8) -> i32;

static SONG_PLAY: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static SONG_STOP: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static SONG_PREPARED: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static BROADCAST: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static FRAME_TICK_GLOBAL: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static GAMEPLAY_ACTOR_VTABLE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
/// Percent-family gauge vtables WITHOUT run state beyond the shared
/// base-class block (Normal, Immortal).
static PERCENT_GAUGE_VTABLES: [AtomicPtr<u8>; 2] = [
    AtomicPtr::new(std::ptr::null_mut()),
    AtomicPtr::new(std::ptr::null_mut()),
];
/// FlareGaugeActor — a percent-family gauge whose subclass carries the
/// floating-flare run state (see the FLARE_* offsets).
static FLARE_GAUGE_VTABLE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
/// GradeGaugeActor — a percent-family gauge whose subclass carries the
/// best-EX-score watermark (see GRADE_WATERMARK_OFFSET).
static GRADE_GAUGE_VTABLE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static LIFE_GAUGE_VTABLE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static SCORE_ACTOR_VTABLE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
/// NoteResultActor — the judge-display child owning the pacemaker
/// (`dance_score_compare`) clip. Resolved OPTIONALLY (fail-open): missing
/// only skips the pacemaker-clip rewind, never refuses resets.
static NOTE_RESULT_ACTOR_VTABLE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
/// The derived per-side context table (`player_option_table`) — the
/// flare-level restore's route to each side's ddr::player::Option.
static PLAYER_OPTION_TABLE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
/// Whether the 2026 FlareGaugeActor layout was attested by the
/// `flare_gauge_ctor_layout` AOB AND the option table resolved. False ⇒
/// any live FlareGaugeActor poisons the snapshot and `request_reset`
/// refuses (the caller's scene-jump fallback re-runs onSetup, restoring
/// flare state the slow-but-correct way).
static FLARE_RESTORE_AVAILABLE: AtomicBool = AtomicBool::new(false);
/// Whether the GradeGaugeActor watermark layout was attested by the
/// `grade_gauge_ctor_layout` AOB. Same refuse-if-live rule as flare.
static GRADE_RESTORE_AVAILABLE: AtomicBool = AtomicBool::new(false);

// ── Seek machinery (optional — missing pieces only disable seeks) ────
static REBUILD_CLEAR: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static REBUILD_RESERVE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static REBUILD_REBUILD: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static CMA_VTABLE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
/// One-shot latch for the "seek machinery unresolved" WARN.
static SEEK_MISSING_WARNED: AtomicBool = AtomicBool::new(false);

static AVAILABLE: AtomicBool = AtomicBool::new(false);
/// Generation for the snapshot probe AND the reset driver: a scene
/// change or a newer reset supersedes whatever is in flight.
static GENERATION: AtomicUsize = AtomicUsize::new(0);

/// Whether a reset driver is currently waiting on cue prepare.
static RESET_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// What kind of gauge child a snapshot entry describes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum GaugeClass {
    /// GaugeActor family — value is fixed-point 0..10000 at +0x90.
    Percent,
    /// FlareGaugeActor — percent family PLUS the floating-flare run
    /// state (judge-history counters, per-level array, Option+0x7C).
    Flare,
    /// GradeGaugeActor — percent family PLUS the best-EX-score
    /// watermark at +0xE0.
    Grade,
    /// LifeGaugeActor — lives count at +0x90, start at +0x94.
    Lives,
}

/// The flare-specific song-start state a [`GaugeClass::Flare`] entry
/// carries (captured by the probe right after onSetup, before the first
/// judge — the counters are ctor-zero and the level is the fresh seed).
#[derive(Clone, Copy)]
struct FlareSnapshot {
    /// Option+0x7C at song start (FLOATING ⇒ 10 = EX).
    level: i32,
    /// Per-level gauge array (+0x10C..+0x134) at song start.
    level_gauges: [i32; FLARE_LEVEL_SLOTS],
}

#[derive(Clone, Copy)]
struct GaugeSnapshot {
    gauge: usize,
    class: GaugeClass,
    start_value: i32,
    /// Present iff `class == Flare`.
    flare: Option<FlareSnapshot>,
}

#[derive(Clone)]
struct SideSnapshot {
    actor: usize,
    gauges: Vec<GaugeSnapshot>,
    /// The dynamic population counter's song-start baseline
    /// ([`GPA_DYNAMIC_POP_OFFSET`]) — restored by every reset so replayed
    /// passes score like fresh ones.
    note_pop_baseline: i32,
}

#[derive(Default)]
struct Snapshot {
    sides: Vec<SideSnapshot>,
    captured: bool,
    /// Set when the live run contains state the reset cannot restore
    /// (a FlareGaugeActor without the attested layout / readable
    /// level). `request_reset` refuses so the caller's scene-jump
    /// fallback — which re-runs onSetup — restarts correctly instead.
    blocked: Option<&'static str>,
}

static SNAPSHOT: Lazy<Mutex<Snapshot>> = Lazy::new(|| Mutex::new(Snapshot::default()));

/// Boot-latched dry-run switch (`DDR_INPLACE_RESET_DRY` env): the gates
/// and the snapshot run and log, but nothing is mutated — `request_reset`
/// returns `Refused` so the caller's shipped fast path still restarts.
/// First-deploy validation aid (plan Step 6).
static DRY_RUN: Lazy<bool> = Lazy::new(|| std::env::var("DDR_INPLACE_RESET_DRY").is_ok());

/// Subscribers to "the run was reset to T ms" (T = 0 this phase).
/// `Arc` so the notify path can snapshot the list and call outside the
/// registry lock.
type ResetCallback = std::sync::Arc<dyn Fn(i32) + Send + Sync>;
static SUBSCRIBERS: Lazy<Mutex<Vec<(usize, ResetCallback)>>> = Lazy::new(|| Mutex::new(Vec::new()));
static NEXT_SUBSCRIBER_ID: AtomicUsize = AtomicUsize::new(1);

/// Outcome of a `request_reset` call.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResetOutcome {
    /// Phase 1 completed; the driver will finish the rewind when the cue
    /// prepares (or invoke the recovery callback on timeout/failure).
    Started,
    /// A gate failed before any state was touched — the caller should
    /// fall back to its own restart path.
    Refused,
    /// Retained for API stability; no longer returned (seek-to-T is
    /// implemented — nonzero `t_ms` gates through `Refused` instead).
    Unsupported,
}

/// What happens to the score/combo/gauge accumulators when a reset or
/// seek completes (design §4.4 step 5).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccumulatorPolicy {
    /// The shipped zeroing + gauge restore (restarts and v1 seeks).
    Zero,
    /// Preserve the accumulators across the jump — reserved for v2
    /// FF/RW; refused in v1.
    Keep,
}

// ── Init / availability ──────────────────────────────────────────────

/// Resolve everything the reset needs and register the per-song snapshot
/// probe. Fail-open: missing resolutions leave the service unavailable
/// (`request_reset` returns `Refused`) without affecting anything else.
pub fn init(signatures: &SignatureStore) -> bool {
    let required: [(&str, &AtomicPtr<u8>); 6] = [
        ("song_play_by_bank", &SONG_PLAY),
        ("song_stop_by_handle", &SONG_STOP),
        ("song_is_prepared", &SONG_PREPARED),
        ("update_broadcast", &BROADCAST),
        ("frame_tick_global", &FRAME_TICK_GLOBAL),
        ("gameplay_actor_vtable", &GAMEPLAY_ACTOR_VTABLE),
    ];
    let mut missing = Vec::new();
    for (name, slot) in required {
        match signatures.get_address(name) {
            Some(addr) => slot.store(addr as *mut u8, Ordering::Release),
            None => missing.push(name),
        }
    }
    for (name, slot) in [
        ("normal_gauge_vtable", &PERCENT_GAUGE_VTABLES[0]),
        ("immortal_gauge_vtable", &PERCENT_GAUGE_VTABLES[1]),
        ("flare_gauge_vtable", &FLARE_GAUGE_VTABLE),
        ("grade_gauge_vtable", &GRADE_GAUGE_VTABLE),
    ] {
        match signatures.get_address(name) {
            Some(addr) => slot.store(addr as *mut u8, Ordering::Release),
            None => missing.push(name),
        }
    }
    match signatures.get_address("life_gauge_vtable") {
        Some(addr) => LIFE_GAUGE_VTABLE.store(addr as *mut u8, Ordering::Release),
        None => missing.push("life_gauge_vtable"),
    }
    match signatures.get_address("score_actor_vtable") {
        Some(addr) => SCORE_ACTOR_VTABLE.store(addr as *mut u8, Ordering::Release),
        None => missing.push("score_actor_vtable"),
    }
    // The build's GamePlayActor death-flag / gauge-cluster offsets (the
    // region shifted by 8 bytes between 20260224 and 20260324). Required:
    // a reset that restored the cluster at the wrong offsets would corrupt
    // unrelated actor state on the old builds.
    match signatures.gameplay_actor_layout() {
        Some(layout) => {
            let _ = GPA_LAYOUT.set(layout);
        }
        None => missing.push("gameplay_actor_layout"),
    }
    // Optional (fail-open): identifies the NoteResultActor child so the
    // reset can rewind the pacemaker (dance_score_compare) clip out of
    // its msg-0x103A "out" outro — the one-way latch that otherwise
    // freezes the pacemaker readout across every in-place loop/restart
    // after a gauge-empty in an earlier pass. Missing only skips that
    // rewind.
    match signatures.get_address("note_result_actor_vtable") {
        Some(addr) => NOTE_RESULT_ACTOR_VTABLE.store(addr as *mut u8, Ordering::Release),
        None => log_warn!(
            "SongReset: note_result_actor_vtable unresolved -- pacemaker clip rewind disabled (readout may stay frozen after a death + in-place reset)"
        ),
    }

    if !missing.is_empty() {
        log_warn!(
            "SongReset: unavailable -- missing resolutions: {}",
            missing.join(", ")
        );
        return false;
    }
    if !scene_manager::is_available() {
        log_warn!("SongReset: unavailable -- scene_manager not available");
        return false;
    }

    // Floating-flare restore surface (fail-open per gauge family): the
    // layout attestation AOB plus the derived per-side Option table.
    // Missing pieces leave FLARE_RESTORE_AVAILABLE false — resets refuse
    // whenever a FlareGaugeActor is live (the caller's scene-jump
    // fallback restores flare state correctly), everything else works.
    let flare_2026 = signatures.get_address("flare_gauge_ctor_layout").is_some();
    let flare_v1 = signatures
        .get_address("flare_gauge_ctor_layout_v1")
        .is_some();
    let flare_attested = flare_2026 || flare_v1;
    match signatures.get_address("player_option_table") {
        Some(addr) if flare_attested => {
            PLAYER_OPTION_TABLE.store(addr as *mut u8, Ordering::Release);
            FLARE_LAYOUT_V1.store(!flare_2026 && flare_v1, Ordering::Release);
            FLARE_RESTORE_AVAILABLE.store(true, Ordering::Release);
            if !flare_2026 {
                log_info!(
                    "SongReset: flare restore using the pre-20260324 FlareGaugeActor layout (v1)"
                );
            }
        }
        table => {
            log_warn!(
                "SongReset: flare restore unavailable (layout attested: {}, option table: {}) -- resets will refuse while a FLARE gauge is live",
                flare_attested,
                table.is_some()
            );
        }
    }

    // Grade-watermark reset surface: attestation AOB only (the
    // watermark is an actor field; no Option access needed). Same
    // refuse-if-live rule as flare.
    if signatures.get_address("grade_gauge_ctor_layout").is_some() {
        GRADE_RESTORE_AVAILABLE.store(true, Ordering::Release);
    } else {
        log_warn!(
            "SongReset: grade-watermark reset unavailable (layout unattested) -- resets will refuse while a GRADE gauge is live"
        );
    }

    // Optional seek machinery (Training Mode seek-to-T): unresolved
    // pieces leave `seek_available()` false — nonzero-T requests refuse,
    // the shipped t=0 reset is unaffected (design §6 fail-open ladder).
    let mut seek_missing = Vec::new();
    for (name, slot) in [
        ("judge_rebuild_clear", &REBUILD_CLEAR),
        ("judge_rebuild_reserve", &REBUILD_RESERVE),
        ("judge_rebuild_rebuild", &REBUILD_REBUILD),
        ("control_message_actor_vtable", &CMA_VTABLE),
    ] {
        match signatures.get_address(name) {
            Some(addr) => slot.store(addr as *mut u8, Ordering::Release),
            None => seek_missing.push(name),
        }
    }
    if !seek_missing.is_empty() {
        log_warn!(
            "SongReset: seek-to-T unavailable -- missing: {}",
            seek_missing.join(", ")
        );
    }

    // Permanent scene callback: arm the gauge snapshot on every gameplay
    // entry, invalidate it (and any in-flight probe/driver) on exit.
    scene_manager::on_scene_change(Box::new(|prev, next| {
        if next == scene::GAMEPLAY && prev != scene::GAMEPLAY {
            let generation = GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
            if let Ok(mut snap) = SNAPSHOT.lock() {
                *snap = Snapshot::default();
            }
            RESET_IN_FLIGHT.store(false, Ordering::Release);
            arm_snapshot_probe(generation);
        } else if prev == scene::GAMEPLAY && next != scene::GAMEPLAY {
            GENERATION.fetch_add(1, Ordering::AcqRel);
            if let Ok(mut snap) = SNAPSHOT.lock() {
                *snap = Snapshot::default();
            }
            RESET_IN_FLIGHT.store(false, Ordering::Release);
        }
    }));

    AVAILABLE.store(true, Ordering::Release);
    log_info!("SongReset: available (in-place rewind armed)");
    true
}

pub fn is_available() -> bool {
    AVAILABLE.load(Ordering::Acquire)
}

/// Whether seek-to-T is available: the base service plus the rebuild trio
/// and the ControlMessageActor vtable (all derived at init). False ⇒
/// nonzero-T `request_reset` calls return `Refused`.
pub fn seek_available() -> bool {
    is_available()
        && !REBUILD_CLEAR.load(Ordering::Acquire).is_null()
        && !REBUILD_RESERVE.load(Ordering::Acquire).is_null()
        && !REBUILD_REBUILD.load(Ordering::Acquire).is_null()
        && !CMA_VTABLE.load(Ordering::Acquire).is_null()
}

/// Whether a reset/seek transaction is currently in flight (`Started`
/// returned, completion/recovery not yet run). Set on every path that
/// hands off to a driver; cleared at completion, on every recovery, and
/// at gameplay entry/exit. Callers dispatching their own resets (the
/// training scrub) consult it to keep ONE transaction in flight instead
/// of racing the drivers' generation tokens.
pub fn reset_in_flight() -> bool {
    RESET_IN_FLIGHT.load(Ordering::Acquire)
}

/// The live run's chart end (raw ms — ControlMessageActor `+0x98`, the
/// 0x104B song-over threshold) for `side`, or `None` when no live run /
/// no matching actor / insane value. The training mod's marker clamp
/// input (Steps 2–4).
pub fn chart_end_raw(side: i32) -> Option<i32> {
    let dps = live_dps()?;
    for actor in gameplay_actors(dps) {
        let actor_side = unsafe { memory::read_i32(actor.add(GPA_SIDE_OFFSET)) };
        if actor_side != side {
            continue;
        }
        let cma = control_message_child(actor)?;
        let end = unsafe { memory::read_i32(cma.add(CMA_CHART_END_RAW_OFFSET)) };
        return (0..=CHART_END_SANE_MAX_MS).contains(&end).then_some(end);
    }
    None
}

/// Both live end-cascade thresholds for `side` — `(+0x94 display-domain
/// last-note-end, +0x98 raw-ms song-over)` off the side's
/// ControlMessageActor — or `None` when no live run / no matching actor
/// / either value insane. Step 4's threshold surface: the LOOP OFF
/// apply stashes the stock pair through this, and the loop driver's
/// fire bound reads the live pair (design §4.2/§4.3). Quiet like
/// [`chart_end_raw`] — the caller owns the WARN-once ladder.
pub fn chart_end_thresholds(side: i32) -> Option<(i32, i32)> {
    let dps = live_dps()?;
    for actor in gameplay_actors(dps) {
        let actor_side = unsafe { memory::read_i32(actor.add(GPA_SIDE_OFFSET)) };
        if actor_side != side {
            continue;
        }
        let cma = control_message_child(actor)?;
        let display = unsafe { memory::read_i32(cma.add(CMA_CHART_END_DISPLAY_OFFSET)) };
        let raw = unsafe { memory::read_i32(cma.add(CMA_CHART_END_RAW_OFFSET)) };
        return ((0..=CHART_END_SANE_MAX_MS).contains(&display)
            && (0..=CHART_END_SANE_MAX_MS).contains(&raw))
        .then_some((display, raw));
    }
    None
}

/// Write both end-cascade thresholds (`+0x94 = display_ms`,
/// `+0x98 = raw_ms`) on EVERY live GamePlayActor's ControlMessageActor
/// — the LOOP OFF early natural end (design §4.2: the truncated end
/// applies to both sides). Fail-closed: `false` with NOTHING written
/// when either value is out of the sane range, no live run exists, or
/// ANY actor's CMA is unresolvable — the section end is applied whole
/// or not at all (§6 ladder: the caller WARNs once and the song plays
/// to its natural end).
pub fn set_chart_end_thresholds(display_ms: i32, raw_ms: i32) -> bool {
    if !(0..=CHART_END_SANE_MAX_MS).contains(&display_ms)
        || !(0..=CHART_END_SANE_MAX_MS).contains(&raw_ms)
    {
        return false;
    }
    let Some(dps) = live_dps() else {
        return false;
    };
    let actors = gameplay_actors(dps);
    if actors.is_empty() {
        return false;
    }
    // Resolve every CMA BEFORE the first write (refuse-before-write).
    let mut cmas = Vec::with_capacity(actors.len());
    for actor in &actors {
        let Some(cma) = control_message_child(*actor) else {
            return false;
        };
        cmas.push(cma);
    }
    for cma in cmas {
        unsafe {
            memory::write_i32(cma.add(CMA_CHART_END_DISPLAY_OFFSET) as *mut u8, display_ms);
            memory::write_i32(cma.add(CMA_CHART_END_RAW_OFFSET) as *mut u8, raw_ms);
        }
    }
    true
}

/// Write per-side end-cascade threshold pairs: each entry is
/// `(side, display_ms, raw_ms)` for that side's ControlMessageActor —
/// the versus-capable sibling of [`set_chart_end_thresholds`] (2026-08-31
/// versus-training lift: in versus each side plays its OWN chart, so a
/// single value pair sampled from one side must never be written onto the
/// other side's CMA). Fail-closed all-or-nothing across the WHOLE list:
/// `false` with NOTHING written when the list is empty, any value is out
/// of the sane range, no live run exists, or ANY listed side's actor/CMA
/// is unresolvable.
pub fn set_chart_end_thresholds_per_side(writes: &[(i32, i32, i32)]) -> bool {
    if writes.is_empty() {
        return false;
    }
    for &(_, display_ms, raw_ms) in writes {
        if !(0..=CHART_END_SANE_MAX_MS).contains(&display_ms)
            || !(0..=CHART_END_SANE_MAX_MS).contains(&raw_ms)
        {
            return false;
        }
    }
    let Some(dps) = live_dps() else {
        return false;
    };
    let actors = gameplay_actors(dps);
    if actors.is_empty() {
        return false;
    }
    // Resolve every side's CMA BEFORE the first write (refuse-before-write).
    let mut resolved = Vec::with_capacity(writes.len());
    for &(side, display_ms, raw_ms) in writes {
        let mut cma_for_side = None;
        for actor in &actors {
            let actor_side = unsafe { memory::read_i32(actor.add(GPA_SIDE_OFFSET)) };
            if actor_side != side {
                continue;
            }
            cma_for_side = control_message_child(*actor);
            break;
        }
        let Some(cma) = cma_for_side else {
            return false;
        };
        resolved.push((cma, display_ms, raw_ms));
    }
    for (cma, display_ms, raw_ms) in resolved {
        unsafe {
            memory::write_i32(cma.add(CMA_CHART_END_DISPLAY_OFFSET) as *mut u8, display_ms);
            memory::write_i32(cma.add(CMA_CHART_END_RAW_OFFSET) as *mut u8, raw_ms);
        }
    }
    true
}

/// The instant-death gauge gate byte (`GamePlayActor+0x2B7`) of the FIRST
/// live actor, or `None` when no live run. RE (20260721): the `gauge::DEAD`
/// chain's `0x103C` handler advances the actor to STEP_GAME_OVER only when
/// this byte is 0, and the DPS finish-poll (`FUN_18005bde0`) returns "not
/// finished" unconditionally while it is nonzero and the actor is below
/// STEP_GAME_OVER — with the gate set, a death latches `m_isDead` but can
/// never end the run. The training loop's death-bypass stash source.
pub fn death_gate() -> Option<bool> {
    let dps = live_dps()?;
    let gate = gpa_layout()?.death_gate;
    let actor = *gameplay_actors(dps).first()?;
    Some(unsafe { memory::read_u8(actor.add(gate)) } != 0)
}

/// One side's instant-death gauge gate, or `None` when no live run / no
/// matching actor. The training loop's PER-SIDE death-bypass stash source
/// (2026-08-31 versus-training lift: sides can run different gauge
/// classes — one immortal-class with a nonzero stock gate — so a single
/// first-actor stash restored to both would corrupt the other side).
pub fn death_gate_for_side(side: i32) -> Option<bool> {
    let dps = live_dps()?;
    let gate = gpa_layout()?.death_gate;
    for actor in gameplay_actors(dps) {
        let actor_side = unsafe { memory::read_i32(actor.add(GPA_SIDE_OFFSET)) };
        if actor_side != side {
            continue;
        }
        return Some(unsafe { memory::read_u8(actor.add(gate)) } != 0);
    }
    None
}

/// Write one side's instant-death gauge gate (the per-side restore half of
/// the training loop's death bypass). `false` = no live run / no matching
/// actor, nothing written.
pub fn set_death_gate_for_side(side: i32, on: bool) -> bool {
    let Some(dps) = live_dps() else {
        return false;
    };
    let Some(gate) = gpa_layout().map(|l| l.death_gate) else {
        return false;
    };
    for actor in gameplay_actors(dps) {
        let actor_side = unsafe { memory::read_i32(actor.add(GPA_SIDE_OFFSET)) };
        if actor_side != side {
            continue;
        }
        unsafe {
            memory::write_u8(actor.add(gate) as *mut u8, u8::from(on));
        }
        return true;
    }
    false
}

/// Write the instant-death gauge gate on every live actor (the training
/// loop's death-bypass: set while LOOP governs the song, restored to the
/// stashed stock value at disarm/song end). `false` = no live run, nothing
/// written.
pub fn set_death_gate(on: bool) -> bool {
    let Some(dps) = live_dps() else {
        return false;
    };
    let Some(gate) = gpa_layout().map(|l| l.death_gate) else {
        return false;
    };
    let actors = gameplay_actors(dps);
    if actors.is_empty() {
        return false;
    }
    for actor in actors {
        unsafe {
            memory::write_u8(actor.add(gate) as *mut u8, u8::from(on));
        }
    }
    true
}

/// Whether any live actor has died (`m_isDead@+0x1E8`). With the death
/// gate set the flag latches without ending the run — this is the training
/// loop's revive trigger. Quiet `false` when no live run.
pub fn any_actor_dead() -> bool {
    let Some(dps) = live_dps() else {
        return false;
    };
    gameplay_actors(dps)
        .iter()
        .any(|actor| unsafe { memory::read_u8(actor.add(GPA_IS_DEAD_OFFSET)) } != 0)
}

/// The decoded note vector of `side`'s live GamePlayActor, or `None`
/// when no live run / no matching actor / the vector fails validation.
/// The Step-4 display⇄raw converters' input (`seek::display_for_raw` /
/// `seek::raw_for_display`). Mirrors `plan_side_rebuilds`' bounds/stride
/// validation (kept separate — that path is cabinet-validated shipped
/// code); quiet like [`chart_end_raw`].
pub fn decoded_notes(side: i32) -> Option<Vec<seek::NoteView>> {
    let dps = live_dps()?;
    for actor in gameplay_actors(dps) {
        let actor_side = unsafe { memory::read_i32(actor.add(GPA_SIDE_OFFSET)) };
        if actor_side != side {
            continue;
        }
        unsafe {
            let begin = memory::read_ptr(actor.add(GPA_NOTES_BEGIN_OFFSET));
            let end = memory::read_ptr(actor.add(GPA_NOTES_END_OFFSET));
            if begin.is_null() || end.is_null() || (end as usize) < (begin as usize) {
                return None;
            }
            let notes_len = end as usize - begin as usize;
            if notes_len > NOTE_VECTOR_MAX_BYTES || notes_len % seek::NOTE_STRIDE != 0 {
                return None;
            }
            let bytes = std::slice::from_raw_parts(begin, notes_len);
            return seek::decode_notes(bytes);
        }
    }
    None
}

/// One side's live scoring counters — a read-only diagnostic surface
/// (built for the Step-4 loop score-cap investigation, 2026-08-14, and
/// retained: it pinned the `+0x19C` denominator skew in one cabinet
/// run). The money-score formula (judge_submit, `FUN_18005fd30` on
/// 20260721) divides the judged weights by the SUM of the three
/// note-population counts at `+0x194/+0x198/+0x19C`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JudgeDiag {
    /// The three population counts (`+0x194`, `+0x198`, `+0x19C`) — the
    /// score denominator's terms.
    pub populations: [i32; 3],
    /// Per-grade judge counts (`+0x1A0 + grade*4`, 8 slots).
    pub grades: [i32; 8],
    /// Freeze-OK count (`+0x1C0`).
    pub freeze_ok: i32,
    /// Judged-event count (`+0x1CC`).
    pub judged_events: i32,
    /// Money score (`+0x1D4`).
    pub score: i32,
    /// Combo (`+0x1DC`).
    pub combo: i32,
}

/// Read [`JudgeDiag`] off `side`'s live GamePlayActor (quiet, like
/// [`chart_end_raw`]).
pub fn judge_diag(side: i32) -> Option<JudgeDiag> {
    let dps = live_dps()?;
    for actor in gameplay_actors(dps) {
        let actor_side = unsafe { memory::read_i32(actor.add(GPA_SIDE_OFFSET)) };
        if actor_side != side {
            continue;
        }
        unsafe {
            let mut populations = [0i32; 3];
            for (slot, offset) in GPA_NOTE_COUNT_OFFSETS.iter().enumerate() {
                populations[slot] = memory::read_i32(actor.add(*offset));
            }
            let mut grades = [0i32; 8];
            for (grade, value) in grades.iter_mut().enumerate() {
                *value = memory::read_i32(actor.add(GPA_JUDGE_COUNTS_OFFSET + grade * 4));
            }
            return Some(JudgeDiag {
                populations,
                grades,
                freeze_ok: memory::read_i32(actor.add(GPA_FREEZE_OK_OFFSET)),
                judged_events: memory::read_i32(actor.add(GPA_JUDGED_EVENTS_OFFSET)),
                score: memory::read_i32(actor.add(GPA_SCORE_OFFSET)),
                combo: memory::read_i32(actor.add(GPA_COMBO_OFFSET)),
            });
        }
    }
    None
}

/// The live run's raw-ms music count (GamePlayActor `+0x178` — the judge
/// domain), or `None` when no live run / insane value. The clock anchor
/// is shared across sides, so the first actor's count is THE count. The
/// training gestures' "current position" source.
pub fn current_raw_music_count() -> Option<i32> {
    let dps = live_dps()?;
    let actor = *gameplay_actors(dps).first()?;
    let count = unsafe { memory::read_i32(actor.add(GPA_RAW_COUNT_OFFSET)) };
    (RAW_COUNT_SANE_MIN_MS..=CHART_END_SANE_MAX_MS)
        .contains(&count)
        .then_some(count)
}

/// Whether the current run has reached its FIRST anchored frame (the
/// Step-3 silent-start driver's detection predicate, design §4.3): a live
/// DPS at the in-song step, at least one GamePlayActor, every actor at
/// step 4 with a nonzero clock anchor (`+0x160` — set by the game's own
/// `0x1044 {now}` from DPS state 6). Requiring the DPS step keeps the
/// one-shot adjust out of the 6→7 transition frame, where its own gates
/// would transiently refuse.
///
/// Despite the name this is a STATE predicate, not an edge: it stays
/// true for the whole in-song phase (an in-place reset re-writes the
/// anchor) and reads false during the pre-song "READY?" init states and
/// the song-end tail (DPS 8/9). Callers that also need a trustworthy
/// music count should use [`run_in_song`].
pub fn first_anchored_frame() -> bool {
    let Some(dps) = live_dps() else {
        return false;
    };
    unsafe {
        if read_step(dps, DPS_STEP_BASE, DPS_STEP_INDEX) != Some(DPS_STEP_IN_SONG) {
            return false;
        }
    }
    let actors = gameplay_actors(dps);
    if actors.is_empty() {
        return false;
    }
    actors.iter().all(|actor| unsafe {
        read_step(*actor, GPA_STEP_BASE, GPA_STEP_INDEX) == Some(GPA_STEP_IN_SONG)
            && memory::read_u64(actor.add(GPA_ANCHOR_OFFSET)) != 0
    })
}

/// The run is LIVE and its music count is TRUSTWORTHY: [`first_anchored_frame`]
/// AND the `+0x178` raw count reads strictly below every live side's raw
/// chart end. The second half matters because `+0x178` is a per-frame
/// CACHED value — until the anchor lands it holds the raw frame tick
/// (minutes-since-boot scale, cabinet finding 2026-08-14) and can still
/// hold that stale tick for one frame after the anchor. A live
/// pre-cascade run can never legitimately read at/past its song-over
/// threshold, so `count < chart_end` is the credibility test.
///
/// The ONE definition of "gameplay has actually begun" (READY banner
/// gone, arrows scrolling) shared by the training gestures and the loop
/// driver's initial bound compute. Any unreadable input ⇒ `false`
/// (conservative — callers drop the action; the song is never disturbed).
pub fn run_in_song() -> bool {
    if !first_anchored_frame() {
        return false;
    }
    matches!(
        (
            current_raw_music_count(),
            (0..2).filter_map(chart_end_raw).min(),
        ),
        (Some(count), Some(end)) if count < end
    )
}

/// The Step-3 silent-start adjust (design §4.3): re-run the anchor +
/// record-rebuild + freeze-neutralization block at `t_q_ms` on an
/// ALREADY-RUNNING song — no cue stop/replay, no accumulator/gauge block
/// (the run just started; its accumulators are already zero). The caller
/// (the training driver, on the first anchored frame) derives `t_q_ms`
/// and `lead_wall_ms` from the live binding's applied content mapping, so
/// clock, audio, and claps land on the same served block.
///
/// Gated and fail-closed exactly like the seek's completion: seek
/// machinery resolved, GAMEPLAY, live DPS in-song, non-course, all actors
/// at step 4, end cascade unfired with `t_q` clamped below every side's
/// chart end. `false` ⇒ nothing was mutated (the caller falls back to a
/// stop/replay seek per the design §6 ladder). Ends in
/// `notify_subscribers(t_q)` — assist_tick and the other subscribers
/// re-sync exactly as they do for a seek.
pub fn adjust_run_to(t_q_ms: i32, lead_wall_ms: u64) -> bool {
    if t_q_ms < 0 || !seek_available() || !is_available() {
        return false;
    }
    if scene_manager::current_scene() != scene::GAMEPLAY {
        return false;
    }
    let Some(dps) = live_dps() else {
        return false;
    };
    unsafe {
        if read_step(dps, DPS_STEP_BASE, DPS_STEP_INDEX) != Some(DPS_STEP_IN_SONG) {
            return false;
        }
        if memory::read_i32(dps.add(DPS_COURSE_MAX_OFFSET)) > 1 {
            return false;
        }
    }
    let actors = gameplay_actors(dps);
    if actors.is_empty() {
        return false;
    }
    for actor in &actors {
        if unsafe { read_step(*actor, GPA_STEP_BASE, GPA_STEP_INDEX) } != Some(GPA_STEP_IN_SONG) {
            return false;
        }
    }
    // End-cascade clamp (research §4.3) — identical to the seek's gate:
    // every side below the content-over step, sane chart ends, and the
    // target below the MIN end minus the margin.
    let mut min_end = i32::MAX;
    for actor in &actors {
        let Some(cma) = control_message_child(*actor) else {
            return false;
        };
        match unsafe { read_step(cma, CMA_STEP_BASE, CMA_STEP_INDEX) } {
            Some(step) if step < CMA_STEP_CONTENT_OVER => {}
            _ => return false,
        }
        let end = unsafe { memory::read_i32(cma.add(CMA_CHART_END_RAW_OFFSET)) };
        if !(0..=CHART_END_SANE_MAX_MS).contains(&end) {
            return false;
        }
        min_end = min_end.min(end);
    }
    if t_q_ms >= min_end.saturating_sub(SEEK_END_MARGIN_MS) {
        return false;
    }
    let plan = SeekPlan {
        t_q: t_q_ms,
        delay_wall_ms: lead_wall_ms,
        rate: song_rate::clock_patch::snapshot(),
    };
    if !perform_adjust(dps, &actors, &plan) {
        return false;
    }
    log_info!(
        "SongReset: run adjusted in place -- t_q {} ms, lead {} ms ({} actor(s), no stop/replay)",
        t_q_ms,
        lead_wall_ms,
        actors.len()
    );
    notify_subscribers(t_q_ms);
    true
}

/// Register a callback fired after every completed in-place reset, with
/// the content time the run was reset to (0 this phase). Fired on the
/// frame thread, after game state is fully reset.
pub fn on_song_reset(callback: impl Fn(i32) + Send + Sync + 'static) -> usize {
    let id = NEXT_SUBSCRIBER_ID.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut subs) = SUBSCRIBERS.lock() {
        subs.push((id, std::sync::Arc::new(callback)));
    }
    id
}

pub fn remove_callback(id: usize) {
    if let Ok(mut subs) = SUBSCRIBERS.lock() {
        subs.retain(|(sub_id, _)| *sub_id != id);
    }
}

// ── Actor-tree walking ───────────────────────────────────────────────

/// The live DPS, gated exactly like the shipped fast paths: a non-null,
/// non-dying active child on the TransitionSequence.
fn live_dps() -> Option<*mut u8> {
    let ts = scene_manager::current_transition_sequence()?;
    unsafe {
        let child = memory::read_ptr(ts.add(ACTIVE_CHILD_OFFSET)) as *mut u8;
        if child.is_null() {
            return None;
        }
        let flags = memory::read_u32(child.add(TREE_FLAGS_OFFSET));
        if flags & TREE_FLAGS_DEAD_MASK != 0 {
            return None;
        }
        Some(child)
    }
}

/// Every GamePlayActor child of `dps` (vtable match).
fn gameplay_actors(dps: *mut u8) -> Vec<*mut u8> {
    let target = GAMEPLAY_ACTOR_VTABLE.load(Ordering::Acquire);
    let mut out = Vec::new();
    if target.is_null() {
        return out;
    }
    unsafe {
        let mut child = memory::read_ptr(dps.add(FIRST_CHILD_OFFSET)) as *mut u8;
        while !child.is_null() {
            if memory::read_ptr(child) == target {
                out.push(child);
            }
            child = memory::read_ptr(child.add(NEXT_SIBLING_OFFSET)) as *mut u8;
        }
    }
    out
}

/// Classify a child of a GamePlayActor against the resolved gauge
/// vtable set.
fn classify_gauge(child: *mut u8) -> Option<GaugeClass> {
    unsafe {
        let vtable = memory::read_ptr(child);
        for slot in &PERCENT_GAUGE_VTABLES {
            if vtable == slot.load(Ordering::Acquire) {
                return Some(GaugeClass::Percent);
            }
        }
        if vtable == FLARE_GAUGE_VTABLE.load(Ordering::Acquire) {
            return Some(GaugeClass::Flare);
        }
        if vtable == GRADE_GAUGE_VTABLE.load(Ordering::Acquire) {
            return Some(GaugeClass::Grade);
        }
        if vtable == LIFE_GAUGE_VTABLE.load(Ordering::Acquire) {
            return Some(GaugeClass::Lives);
        }
    }
    None
}

/// Resolve `side`'s live ddr::player::Option object through the derived
/// per-side context table (`*(table[side]) + 0xE0` — the same chain
/// assist_tick reads JUDGMENT TIMING through). None on any null link.
fn player_option_ptr(side: i32) -> Option<*mut u8> {
    if !(0..=1).contains(&side) {
        return None;
    }
    let table = PLAYER_OPTION_TABLE.load(Ordering::Acquire);
    if table.is_null() {
        return None;
    }
    unsafe {
        let holder = memory::read_ptr(table.add(side as usize * 8));
        if holder.is_null() {
            return None;
        }
        let ctx = memory::read_ptr(holder);
        if ctx.is_null() {
            return None;
        }
        let option_off = crate::services::stage_records::player_option_offset()?;
        Some((ctx as *mut u8).add(option_off))
    }
}

/// The ControlMessageActor child of a GamePlayActor (vtable match) —
/// each side's end-cascade owner (seek clamp source).
fn control_message_child(actor: *mut u8) -> Option<*mut u8> {
    let target = CMA_VTABLE.load(Ordering::Acquire);
    if target.is_null() {
        return None;
    }
    unsafe {
        let mut child = memory::read_ptr(actor.add(FIRST_CHILD_OFFSET)) as *mut u8;
        while !child.is_null() {
            if memory::read_ptr(child) == target {
                return Some(child);
            }
            child = memory::read_ptr(child.add(NEXT_SIBLING_OFFSET)) as *mut u8;
        }
    }
    None
}

/// Read the active StackStep value of an embedded step machine.
/// Returns None when the depth index is out of range (layout drift).
unsafe fn read_step(object: *mut u8, base: usize, index: usize) -> Option<i32> {
    let idx = *(object.add(index) as *const u16) as usize;
    if idx >= 5 {
        return None;
    }
    Some(*(object.add(base + idx * 8) as *const i32))
}

// ── Per-song gauge snapshot ──────────────────────────────────────────

/// Self-requeueing render-thread probe: waits until every GamePlayActor
/// has at least one recognizable gauge child (onSetup runs on the actors'
/// first update tick, well before the music starts), then latches each
/// gauge's ctor-produced start value. The gauge cannot have moved yet —
/// deltas only happen on judge events, and the first judge is minutes of
/// frames away behind the song-start protocol.
fn arm_snapshot_probe(generation: usize) {
    if !widget_renderer::is_available() {
        log_warn!("SongReset: widget_renderer unavailable -- no gauge snapshot this song");
        return;
    }
    probe_step(generation, Instant::now());
}

fn probe_step(generation: usize, started: Instant) {
    widget_renderer::run_on_render_thread(move || {
        if GENERATION.load(Ordering::Acquire) != generation {
            return;
        }
        if scene_manager::current_scene() != scene::GAMEPLAY {
            return;
        }
        if started.elapsed().as_secs() >= 30 {
            log_warn!("SongReset: gauge snapshot probe timed out -- reset unavailable this song");
            return;
        }

        let Some(dps) = live_dps() else {
            probe_step(generation, started);
            return;
        };
        let actors = gameplay_actors(dps);
        if actors.is_empty() {
            probe_step(generation, started);
            return;
        }

        let mut sides = Vec::with_capacity(actors.len());
        let mut blocked: Option<&'static str> = None;
        for actor in &actors {
            let actor_side = unsafe { memory::read_i32(actor.add(GPA_SIDE_OFFSET)) };
            let mut gauges = Vec::new();
            unsafe {
                let mut child = memory::read_ptr(actor.add(FIRST_CHILD_OFFSET)) as *mut u8;
                while !child.is_null() {
                    if let Some(class) = classify_gauge(child) {
                        let start_value =
                            memory::read_i32(child.add(GAUGE_VALUE_OFFSET) as *const u8);
                        let sane = match class {
                            GaugeClass::Percent | GaugeClass::Flare | GaugeClass::Grade => {
                                (0..=GAUGE_VALUE_MAX).contains(&start_value)
                            }
                            GaugeClass::Lives => (1..=LIFE_GAUGE_MAX_LIVES).contains(&start_value),
                        };
                        let flare = if sane && class == GaugeClass::Flare {
                            match snapshot_flare_state(child, actor_side) {
                                Ok(flare) => Some(flare),
                                Err(why) => {
                                    blocked = Some(why);
                                    None
                                }
                            }
                        } else {
                            None
                        };
                        if class == GaugeClass::Grade
                            && !GRADE_RESTORE_AVAILABLE.load(Ordering::Acquire)
                        {
                            blocked = Some("GRADE gauge live but watermark layout unattested");
                        }
                        if sane {
                            gauges.push(GaugeSnapshot {
                                gauge: child as usize,
                                class,
                                start_value,
                                flare,
                            });
                        } else {
                            if class == GaugeClass::Flare {
                                blocked = Some("FLARE gauge start value out of range");
                            }
                            log_warn!(
                                "SongReset: gauge snapshot value {} out of range ({:?}) -- skipping gauge",
                                start_value,
                                class
                            );
                        }
                    }
                    child = memory::read_ptr(child.add(NEXT_SIBLING_OFFSET)) as *mut u8;
                }
            }
            if gauges.is_empty() {
                // onSetup hasn't run yet — try again next frame.
                probe_step(generation, started);
                return;
            }
            // The dynamic population counter's baseline: the probe runs
            // before the music starts (freeze-arm conversions are the
            // run's earliest possible increments and need a hit head),
            // so this is the chart loader's start-of-song value.
            let note_pop_baseline = unsafe { memory::read_i32(actor.add(GPA_DYNAMIC_POP_OFFSET)) };
            sides.push(SideSnapshot {
                actor: *actor as usize,
                gauges,
                note_pop_baseline,
            });
        }

        if let Ok(mut snap) = SNAPSHOT.lock() {
            snap.sides = sides;
            snap.captured = true;
            snap.blocked = blocked;
            match blocked {
                Some(why) => log_warn!(
                    "SongReset: snapshot BLOCKED this song ({}) -- in-place resets will refuse (scene-jump fallback covers restarts)",
                    why
                ),
                None => log_info!(
                    "SongReset: gauge snapshot captured ({} side(s)) at {:.2}s",
                    snap.sides.len(),
                    started.elapsed().as_secs_f32()
                ),
            }
        }
    });
}

/// Latch a FlareGaugeActor's song-start flare state: the side's current
/// flare level (Option+0x7C — the fresh onSetup seed; FLOATING ⇒ 10) and
/// the per-level gauge array. `Err` poisons the snapshot: the run holds
/// flare state the reset cannot restore, so `request_reset` must refuse
/// and let the scene-jump fallback (which re-runs onSetup) restart.
///
/// # Safety
/// `gauge` must be a live FlareGaugeActor (caller vtable-matched it).
unsafe fn snapshot_flare_state(
    gauge: *mut u8,
    actor_side: i32,
) -> Result<FlareSnapshot, &'static str> {
    if !FLARE_RESTORE_AVAILABLE.load(Ordering::Acquire) {
        return Err("FLARE gauge live but flare layout/table unresolved");
    }
    let Some(option) = player_option_ptr(actor_side) else {
        return Err("FLARE gauge live but side Option unreachable");
    };
    let level = memory::read_i32(option.add(OPTION_FLARE_LEVEL_OFFSET));
    if !(FLARE_LEVEL_MIN..=FLARE_LEVEL_MAX).contains(&level) {
        return Err("FLARE level out of range at song start");
    }
    let mut level_gauges = [0i32; FLARE_LEVEL_SLOTS];
    // The v1 layout has no per-level array (nothing to snapshot/restore).
    if !FLARE_LAYOUT_V1.load(Ordering::Acquire) {
        for (slot, value) in level_gauges.iter_mut().enumerate() {
            let read = memory::read_i32(gauge.add(FLARE_LEVELS_FIRST_OFFSET + slot * 4));
            if !(0..=GAUGE_VALUE_MAX).contains(&read) {
                return Err("FLARE per-level gauge value out of range");
            }
            *value = read;
        }
    }
    Ok(FlareSnapshot {
        level,
        level_gauges,
    })
}

// ── Message delivery (the engine's own shape) ────────────────────────

/// Deliver `msg` to `root` and its whole subtree exactly the way DPS
/// states 5/6 do: flag-guarded self onMessage (vt+0x18, pointer verified
/// in-module) then the engine broadcast per child. Returns false when the
/// self-dispatch could not be trusted (bad vtable pointer).
fn broadcast_to_subtree(root: *mut u8, msg: i32, param: *mut u8) -> bool {
    let broadcast_addr = BROADCAST.load(Ordering::Acquire);
    if broadcast_addr.is_null() {
        return false;
    }
    unsafe {
        let flags = memory::read_u32(root.add(TREE_FLAGS_OFFSET));
        if flags & TREE_FLAGS_DISPATCH_SUPPRESSED != 0 {
            return false;
        }
        let vtable = memory::read_ptr(root);
        let on_message_addr = memory::read_ptr(vtable.add(VTBL_ON_MESSAGE_OFFSET));
        let in_module = module_resolver::get_game_module().is_some_and(|m| {
            let base = m.base as usize;
            let addr = on_message_addr as usize;
            addr > base && addr < base + m.size
        });
        if !in_module {
            log_warn!(
                "SongReset: onMessage ptr {:?} outside gamemdx -- broadcast refused",
                on_message_addr
            );
            return false;
        }
        let on_message: OnMessageFn = std::mem::transmute(on_message_addr);
        if on_message(root, msg, param) == 0 {
            let broadcast: BroadcastFn = std::mem::transmute(broadcast_addr);
            let mut child = memory::read_ptr(root.add(FIRST_CHILD_OFFSET)) as *mut u8;
            while !child.is_null() {
                // Capture the sibling first: handlers may mutate links.
                let next = memory::read_ptr(child.add(NEXT_SIBLING_OFFSET)) as *mut u8;
                broadcast(child, msg, param, 0);
                child = next;
            }
        }
    }
    true
}

// ── std::string reading (MSVC, SSO) ──────────────────────────────────

/// Read an MSVC `std::string` at `addr` (buf/ptr at +0, len at +0x10,
/// cap at +0x18; heap pointer when cap >= 0x10). None on insane fields.
unsafe fn read_msvc_string(addr: *const u8) -> Option<Vec<u8>> {
    let len = memory::read_u64(addr.add(0x10)) as usize;
    let cap = memory::read_u64(addr.add(0x18)) as usize;
    if len > cap || cap > 0x1000 {
        return None;
    }
    let buf = if cap >= 0x10 {
        let p = memory::read_ptr(addr);
        if p.is_null() {
            return None;
        }
        p
    } else {
        addr
    };
    Some(std::slice::from_raw_parts(buf, len).to_vec())
}

// ── The reset itself ─────────────────────────────────────────────────

/// Request an in-place rewind of the live run to `t_ms`. MUST be called
/// on the frame thread. `on_recovery` is invoked (on the frame thread) if
/// the reset started but could not complete — the song is stopped at that
/// point, so the caller should force its natural-death restart to recover.
///
/// `t_ms == 0` is the shipped restart (instant or countdown-delayed).
/// `t_ms > 0` is seek-to-T (Training Mode, design §4.4): the audio's
/// content mapping shifts to the block-quantized target between cue stop
/// and replay, the timing anchor back-dates by `wall(T_q)`, and the
/// judge records rebuild at playhead `T_q` (pre-T notes consumed-neutral,
/// spanning freezes neutralized). Requires `seek_available()` and a live
/// song-rate binding; any gate failure returns `Refused` untouched.
///
/// `delay_ms` (clamped 0..=10000): countdown before the (re)started song
/// begins. For `t_ms == 0`: the field resets IMMEDIATELY with the clock
/// anchored `delay_ms` into the future (the engine's natural pre-song
/// approach), the cue replay scheduled near the countdown's end, and an
/// idempotent re-anchor at prepared — the shipped v4 protocol. For seeks
/// the mapping's silent LEAD serves the approach instead: the cue replays
/// immediately (silence for `delay_ms`, then content at T_q) and the
/// anchor composes `now + delay − wall(T_q)` in one prepared→anchor
/// adjacency.
///
/// `policy`: what happens to the accumulators at completion —
/// [`AccumulatorPolicy::Zero`] is the shipped zeroing; `Keep` is reserved
/// for v2 FF/RW and refused in v1.
pub fn request_reset(
    t_ms: i32,
    delay_ms: i32,
    policy: AccumulatorPolicy,
    on_recovery: Option<fn()>,
) -> ResetOutcome {
    if policy == AccumulatorPolicy::Keep {
        log_warn!("SongReset: refused -- AccumulatorPolicy::Keep is reserved for v2");
        return ResetOutcome::Refused;
    }
    if t_ms < 0 {
        log_info!("SongReset: refused -- negative seek target {}", t_ms);
        return ResetOutcome::Refused;
    }
    let delay_ms = delay_ms.clamp(0, MAX_DELAY_MS) as u64;
    if !is_available() || !widget_renderer::is_available() {
        return ResetOutcome::Refused;
    }
    if scene_manager::current_scene() != scene::GAMEPLAY {
        return ResetOutcome::Refused;
    }

    // ── Gates (design §3 Phase 0) ──
    let Some(dps) = live_dps() else {
        log_info!("SongReset: refused -- no live DPS");
        return ResetOutcome::Refused;
    };
    unsafe {
        match read_step(dps, DPS_STEP_BASE, DPS_STEP_INDEX) {
            Some(step) if step == DPS_STEP_IN_SONG => {}
            step => {
                log_info!("SongReset: refused -- DPS step {:?} (need 7)", step);
                return ResetOutcome::Refused;
            }
        }
        let course_max = memory::read_i32(dps.add(DPS_COURSE_MAX_OFFSET));
        if course_max > 1 {
            log_info!("SongReset: refused -- course mode ({} stages)", course_max);
            return ResetOutcome::Refused;
        }
    }
    let actors = gameplay_actors(dps);
    if actors.is_empty() {
        log_info!("SongReset: refused -- no GamePlayActor");
        return ResetOutcome::Refused;
    }
    for actor in &actors {
        match unsafe { read_step(*actor, GPA_STEP_BASE, GPA_STEP_INDEX) } {
            Some(step) if step == GPA_STEP_IN_SONG => {}
            step => {
                log_info!("SongReset: refused -- actor step {:?} (need 4)", step);
                return ResetOutcome::Refused;
            }
        }
    }
    let snapshot = match SNAPSHOT.lock() {
        Ok(snap) if snap.captured => {
            if let Some(why) = snap.blocked {
                log_info!("SongReset: refused -- snapshot blocked ({})", why);
                return ResetOutcome::Refused;
            }
            snap.sides.clone()
        }
        _ => {
            log_info!("SongReset: refused -- gauge snapshot not captured");
            return ResetOutcome::Refused;
        }
    };
    // The snapshot must describe exactly the live actors (a stale snapshot
    // after some unexpected actor churn must refuse, not corrupt).
    if snapshot.len() != actors.len()
        || !actors
            .iter()
            .all(|a| snapshot.iter().any(|s| s.actor == *a as usize))
    {
        log_info!("SongReset: refused -- snapshot does not match live actors");
        return ResetOutcome::Refused;
    }

    let old_handle = unsafe { memory::read_i32(dps.add(DPS_CUE_HANDLE_OFFSET)) };
    if !(0..0x1000).contains(&old_handle) {
        log_info!(
            "SongReset: refused -- cue handle {} out of range",
            old_handle
        );
        return ResetOutcome::Refused;
    }

    if *DRY_RUN {
        for side in &snapshot {
            for g in &side.gauges {
                log_info!(
                    "SongReset[dry]: actor {:#x} gauge {:#x} {:?} start {} flare {:?}",
                    side.actor,
                    g.gauge,
                    g.class,
                    g.start_value,
                    g.flare.map(|f| f.level)
                );
            }
        }
        log_info!(
            "SongReset[dry]: all gates pass (cue handle {}, {} actor(s), delay {} ms) -- refusing so the scene-jump path restarts",
            old_handle,
            actors.len(),
            delay_ms
        );
        return ResetOutcome::Refused;
    }

    // Read the bank name up front (both paths need it; the delayed path
    // also re-reads it at replay time — same strings, same song).
    let bank_name = unsafe {
        let base = read_msvc_string(dps.add(DPS_BASENAME_OFFSET));
        let suffix = read_msvc_string(dps.add(DPS_SUFFIX_OFFSET));
        match (base, suffix) {
            (Some(mut b), Some(s)) if !b.is_empty() => {
                b.extend_from_slice(&s);
                b.push(0);
                b
            }
            _ => {
                log_info!("SongReset: refused -- could not read song bank name");
                return ResetOutcome::Refused;
            }
        }
    };

    // ── Seek-to-T (Training Mode, design §4.4) ──
    if t_ms != 0 {
        return request_seek(
            t_ms,
            delay_ms,
            dps,
            &actors,
            snapshot,
            old_handle,
            bank_name,
            on_recovery,
        );
    }

    // ── Delayed path: reset the field NOW with a future-dated anchor,
    // replay the audio near the countdown's end. ──
    if delay_ms > 0 {
        unsafe {
            let stop: SongStopFn = std::mem::transmute(SONG_STOP.load(Ordering::Acquire));
            stop(old_handle);
        }
        clear_content_mapping_if_shifted();
        if !perform_reset(dps, &actors, &snapshot, delay_ms) {
            // Song stopped, nothing else changed — recover via the
            // caller's natural-death restart.
            log_warn!("SongReset: future anchor refused -- invoking recovery");
            if let Some(recover) = on_recovery {
                recover();
            }
            return ResetOutcome::Started;
        }
        let generation = GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
        RESET_IN_FLIGHT.store(true, Ordering::Release);
        log_info!(
            "SongReset: field reset -- cue {} stopped, song starts in {} ms ({} actor(s))",
            old_handle,
            delay_ms,
            actors.len()
        );
        notify_subscribers(0);
        countdown_step(
            generation,
            Instant::now(),
            delay_ms,
            bank_name,
            None,
            actors.len(),
            on_recovery,
        );
        return ResetOutcome::Started;
    }

    // ── Instant path (Phase 1): cut audio + replay (synchronous) ──
    let new_handle = unsafe {
        let stop: SongStopFn = std::mem::transmute(SONG_STOP.load(Ordering::Acquire));
        let play: SongPlayFn = std::mem::transmute(SONG_PLAY.load(Ordering::Acquire));
        stop(old_handle);
        clear_content_mapping_if_shifted();
        play(5, bank_name.as_ptr())
    };
    if new_handle == -1 {
        // The song is stopped and nothing else changed — recover via the
        // caller's natural-death restart.
        log_warn!("SongReset: replay failed (handle -1) -- invoking recovery");
        if let Some(recover) = on_recovery {
            recover();
        }
        return ResetOutcome::Started;
    }
    unsafe {
        memory::write_i32(dps.add(DPS_CUE_HANDLE_OFFSET) as *mut u8, new_handle);
    }

    let generation = GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    RESET_IN_FLIGHT.store(true, Ordering::Release);
    log_info!(
        "SongReset: started -- cue {} stopped, cue {} preparing ({} actor(s))",
        old_handle,
        new_handle,
        actors.len()
    );
    driver_step(
        generation,
        Instant::now(),
        new_handle,
        snapshot,
        on_recovery,
    );
    ResetOutcome::Started
}

// ── Seek-to-T internals (Training Mode, design §4.4) ─────────────────

/// Clear a leftover non-`{0,0}` content mapping before a t=0 restart: a
/// prior seek on this song left the audio shifted, and a restart to 0
/// must serve content from the true beginning. Deliberately a no-op (no
/// seqlock churn, no production restart) when no binding is live or the
/// mapping is already `{0,0}` — the shipped no-seek behavior is
/// bit-identical.
fn clear_content_mapping_if_shifted() {
    if let Some(mapping) = song_rate::runtime::active_content_mapping() {
        if mapping != (0, 0) && !song_rate::runtime::set_content_mapping(0, 0) {
            log_warn!("SongReset: could not clear the seek mapping -- audio may start shifted");
        }
    }
}

/// The completion parameters a seek driver carries to `perform_seek`.
#[derive(Clone, Copy)]
struct SeekPlan {
    /// Content-domain playhead (derived from the served block grid).
    t_q: i32,
    /// The silent approach lead's wall-domain length (block-quantized —
    /// the same length the mapping's lead blocks serve as silence).
    delay_wall_ms: u64,
    /// The rate snapshot latched at request time (a committed rate cannot
    /// change mid-song).
    rate: song_rate::clock_patch::RateSnapshot,
}

/// The nonzero-T arm of `request_reset`: seek gates, then the §5.4
/// transaction — stop → publish the content mapping → replay → (driver)
/// anchor + rebuild at T_q. Shares the caller's already-passed Phase-0
/// gates; every additional gate refuses BEFORE any state is touched.
#[allow(clippy::too_many_arguments)]
fn request_seek(
    t_ms: i32,
    delay_ms: u64,
    dps: *mut u8,
    actors: &[*mut u8],
    snapshot: Vec<SideSnapshot>,
    old_handle: i32,
    bank_name: Vec<u8>,
    on_recovery: Option<fn()>,
) -> ResetOutcome {
    if !seek_available() {
        if !SEEK_MISSING_WARNED.swap(true, Ordering::AcqRel) {
            log_warn!("SongReset: seek refused -- rebuild trio / CMA vtable unresolved");
        }
        return ResetOutcome::Refused;
    }
    // Audio preflight: a live binding IS the seek's audio half (identity
    // sessions bind via the training arm — design §4.5); its main-entry
    // grid is the quantizer.
    let Some(grid) = song_rate::runtime::active_content_grid() else {
        log_info!("SongReset: seek refused -- no live song-rate binding");
        return ResetOutcome::Refused;
    };
    // End-cascade clamp (research §4.3): the cascade is one-way and a
    // seek past its thresholds hard-ends the song unresettably. Every
    // side must be below the content-over step with a sane chart end;
    // the clamp bound is the MIN across sides.
    let mut min_end = i32::MAX;
    for actor in actors {
        let Some(cma) = control_message_child(*actor) else {
            log_info!("SongReset: seek refused -- ControlMessageActor child not found");
            return ResetOutcome::Refused;
        };
        match unsafe { read_step(cma, CMA_STEP_BASE, CMA_STEP_INDEX) } {
            Some(step) if step < CMA_STEP_CONTENT_OVER => {}
            step => {
                log_info!("SongReset: seek refused -- end cascade at step {:?}", step);
                return ResetOutcome::Refused;
            }
        }
        let end = unsafe { memory::read_i32(cma.add(CMA_CHART_END_RAW_OFFSET)) };
        if !(0..=CHART_END_SANE_MAX_MS).contains(&end) {
            log_info!("SongReset: seek refused -- chart end {} out of range", end);
            return ResetOutcome::Refused;
        }
        min_end = min_end.min(end);
    }
    // Quantize on the SERVED-stream grid (wall domain; identity ⇒ the
    // source grid — the design's B(T) letter), then derive the
    // content-domain playhead and clamp it.
    let rate = song_rate::clock_patch::snapshot();
    let wall_target = seek::wall_ms(t_ms, &rate);
    let Some(quantized) = seek::quantize_seek(
        wall_target,
        grid.samples_per_block,
        grid.sample_rate,
        grid.stream_blocks,
    ) else {
        log_info!("SongReset: seek refused -- degenerate block grid");
        return ResetOutcome::Refused;
    };
    let t_q = seek::content_ms(quantized.t_q_ms, &rate);
    if t_q >= min_end.saturating_sub(SEEK_END_MARGIN_MS) {
        log_info!(
            "SongReset: seek refused -- target {} ms within {} ms of chart end {}",
            t_q,
            SEEK_END_MARGIN_MS,
            min_end
        );
        return ResetOutcome::Refused;
    }
    // The silent approach lead, block-quantized on the same grid. The
    // anchor's delay term is the lead's exact ms, so the served silence
    // and the clock's future-dating agree to the block.
    let (lead_blocks, delay_wall_ms) = if delay_ms > 0 {
        match seek::quantize_seek(
            delay_ms as i32,
            grid.samples_per_block,
            grid.sample_rate,
            grid.stream_blocks,
        ) {
            Some(lead) => (lead.blocks, lead.t_q_ms.max(0) as u64),
            None => (0, 0),
        }
    } else {
        (0, 0)
    };

    // ── Transaction (research §5.4): stop → mapping → replay ──
    let new_handle = unsafe {
        let stop: SongStopFn = std::mem::transmute(SONG_STOP.load(Ordering::Acquire));
        stop(old_handle);
        if !song_rate::runtime::set_content_mapping(quantized.blocks, lead_blocks) {
            // The preflight saw a live binding, so this is a same-frame
            // race; the song is stopped — recover via the caller's
            // natural-death restart.
            log_warn!("SongReset: seek mapping refused post-stop -- invoking recovery");
            if let Some(recover) = on_recovery {
                recover();
            }
            return ResetOutcome::Started;
        }
        let play: SongPlayFn = std::mem::transmute(SONG_PLAY.load(Ordering::Acquire));
        play(5, bank_name.as_ptr())
    };
    if new_handle == -1 {
        log_warn!("SongReset: seek replay failed (handle -1) -- invoking recovery");
        if let Some(recover) = on_recovery {
            recover();
        }
        return ResetOutcome::Started;
    }
    unsafe {
        memory::write_i32(dps.add(DPS_CUE_HANDLE_OFFSET) as *mut u8, new_handle);
    }

    let generation = GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    RESET_IN_FLIGHT.store(true, Ordering::Release);
    log_info!(
        "SongReset: seek started -- t_q {} ms (shift {} + lead {} blocks), cue {} -> {} ({} actor(s))",
        t_q,
        quantized.blocks,
        lead_blocks,
        old_handle,
        new_handle,
        actors.len()
    );
    seek_driver_step(
        generation,
        Instant::now(),
        new_handle,
        snapshot,
        SeekPlan {
            t_q,
            delay_wall_ms,
            rate,
        },
        on_recovery,
    );
    ResetOutcome::Started
}

/// Per-frame seek driver: the reset driver's protocol (prepare poll →
/// re-validate → one synchronous completion block) carrying a
/// [`SeekPlan`] into `perform_seek`.
fn seek_driver_step(
    generation: usize,
    started: Instant,
    handle: i32,
    snapshot: Vec<SideSnapshot>,
    plan: SeekPlan,
    on_recovery: Option<fn()>,
) {
    widget_renderer::run_on_render_thread(move || {
        if GENERATION.load(Ordering::Acquire) != generation {
            return; // superseded by a newer reset or a scene change
        }
        if scene_manager::current_scene() != scene::GAMEPLAY {
            RESET_IN_FLIGHT.store(false, Ordering::Release);
            return;
        }
        let recover_with = |why: &str| {
            RESET_IN_FLIGHT.store(false, Ordering::Release);
            log_warn!("SongReset: {} -- invoking recovery", why);
            if let Some(recover) = on_recovery {
                recover();
            }
        };
        if started.elapsed().as_secs() >= PREPARE_TIMEOUT_SECS {
            recover_with("seek cue never prepared");
            return;
        }
        let prepared = unsafe {
            let is_prepared: SongPreparedFn =
                std::mem::transmute(SONG_PREPARED.load(Ordering::Acquire));
            is_prepared(handle) != 0
        };
        if !prepared {
            seek_driver_step(generation, started, handle, snapshot, plan, on_recovery);
            return;
        }
        let Some((dps, actors)) = revalidated_world(&snapshot) else {
            recover_with("world changed during seek prepare window");
            return;
        };
        if !perform_seek(dps, &actors, &snapshot, &plan) {
            recover_with("seek completion refused");
            return;
        }
        RESET_IN_FLIGHT.store(false, Ordering::Release);
        log_info!(
            "SongReset: seek complete -- t_q {} ms, stop->anchor {:.0} ms",
            plan.t_q,
            started.elapsed().as_secs_f32() * 1000.0
        );
        notify_subscribers(plan.t_q);
    });
}

/// Re-validate the world before a completion block: live DPS in-song,
/// exactly the snapshot's actor count, all at the in-song step (the
/// player may have died during the silence window; the DPS may have
/// moved on). Any surprise ⇒ recovery, never a partial completion.
fn revalidated_world(snapshot: &[SideSnapshot]) -> Option<(*mut u8, Vec<*mut u8>)> {
    let dps = live_dps()?;
    unsafe {
        if read_step(dps, DPS_STEP_BASE, DPS_STEP_INDEX)? != DPS_STEP_IN_SONG {
            return None;
        }
    }
    let actors = gameplay_actors(dps);
    if actors.is_empty() || actors.len() != snapshot.len() {
        return None;
    }
    for actor in &actors {
        if unsafe { read_step(*actor, GPA_STEP_BASE, GPA_STEP_INDEX) } != Some(GPA_STEP_IN_SONG) {
            return None;
        }
    }
    Some((dps, actors))
}

/// One side's pre-validated rebuild inputs, read and planned BEFORE any
/// engine state is touched — a validation failure refuses the whole
/// completion with the run still intact.
struct SideRebuild {
    actor: *mut u8,
    record_count: i64,
    notes_begin: *const u8,
    notes_len: usize,
    writes: Vec<seek::RecordWrite>,
}

/// Validate every side's note vector + counts and plan the R14
/// neutralization writes (pure — `seek::neutralization_writes`).
fn plan_side_rebuilds(actors: &[*mut u8], t_q: i32) -> Option<Vec<SideRebuild>> {
    let mut sides = Vec::with_capacity(actors.len());
    for actor in actors {
        let actor = *actor;
        unsafe {
            let mut record_count: i64 = 0;
            for offset in GPA_NOTE_COUNT_OFFSETS {
                let value = memory::read_i32(actor.add(offset));
                if value < 0 || i64::from(value) > RECORD_COUNT_SANE_MAX {
                    log_warn!(
                        "SongReset: seek refused -- note count {} at +{:#x} out of range",
                        value,
                        offset
                    );
                    return None;
                }
                record_count += i64::from(value);
            }
            if record_count > RECORD_COUNT_SANE_MAX {
                log_warn!(
                    "SongReset: seek refused -- summed record count {} out of range",
                    record_count
                );
                return None;
            }
            let begin = memory::read_ptr(actor.add(GPA_NOTES_BEGIN_OFFSET));
            let end = memory::read_ptr(actor.add(GPA_NOTES_END_OFFSET));
            if begin.is_null() || end.is_null() || (end as usize) < (begin as usize) {
                log_warn!("SongReset: seek refused -- note vector pointers insane");
                return None;
            }
            let notes_len = end as usize - begin as usize;
            if notes_len > NOTE_VECTOR_MAX_BYTES || notes_len % seek::NOTE_STRIDE != 0 {
                log_warn!(
                    "SongReset: seek refused -- note vector length {} insane",
                    notes_len
                );
                return None;
            }
            let bytes = std::slice::from_raw_parts(begin, notes_len);
            let Some(notes) = seek::decode_notes(bytes) else {
                log_warn!("SongReset: seek refused -- note vector failed to decode");
                return None;
            };
            sides.push(SideRebuild {
                actor,
                record_count,
                notes_begin: begin,
                notes_len,
                writes: seek::neutralization_writes(&notes, t_q),
            });
        }
    }
    Some(sides)
}

/// The synchronous heart of the seek (design §4.4 steps 3–6): after all
/// inputs pre-validate, deliver the engine protocol (0x1043, then 0x1044
/// with the BACK-DATED anchor — the handler re-anchors and rebuilds at
/// playhead 0), re-run the rebuild trio per side at playhead T_q, apply
/// the spanning-freeze neutralization writes, then the accumulator
/// policy (Zero — the shipped block) and HUD refresh. Returns false with
/// NOTHING mutated when a pre-validation fails.
/// The shared anchor+rebuild core (research §5.4's transaction tail):
/// pre-validated side plans → `0x1043` (pre-start arm) → back-dated
/// `0x1044` anchor → per-side record-rebuild trio at `plan.t_q` + R14
/// spanning-freeze neutralization. Everything the seek's completion does
/// EXCEPT the accumulator/gauge/HUD block (`reset_side_state`) — the
/// Step-3 silent-start adjust runs this block alone (the run just
/// started; its accumulators are already zero). `false` before the first
/// broadcast ⇒ nothing was mutated.
fn perform_adjust(dps: *mut u8, actors: &[*mut u8], plan: &SeekPlan) -> bool {
    let Some(rebuilds) = plan_side_rebuilds(actors, plan.t_q) else {
        return false;
    };
    let clear_ptr = REBUILD_CLEAR.load(Ordering::Acquire);
    let reserve_ptr = REBUILD_RESERVE.load(Ordering::Acquire);
    let rebuild_ptr = REBUILD_REBUILD.load(Ordering::Acquire);
    if clear_ptr.is_null() || reserve_ptr.is_null() || rebuild_ptr.is_null() {
        return false;
    }
    unsafe {
        let tick_global = FRAME_TICK_GLOBAL.load(Ordering::Acquire);
        let frame_struct = memory::read_ptr(tick_global);
        if frame_struct.is_null() {
            log_warn!("SongReset: frame clock struct is null -- aborting before seek anchor");
            return false;
        }
        let mut arm_payload = [0u8; 16];
        if !broadcast_to_subtree(dps, MSG_PRE_START_ARM, arm_payload.as_mut_ptr()) {
            return false;
        }
        let now = memory::read_u64(frame_struct.add(FRAME_TICK_FIELD_OFFSET));
        let mut anchor = seek::anchor_tick(now, plan.delay_wall_ms, plan.t_q, &plan.rate);
        if !broadcast_to_subtree(dps, MSG_TIMING_ANCHOR, &mut anchor as *mut u64 as *mut u8) {
            return false;
        }
        let clear: RebuildClearFn = std::mem::transmute(clear_ptr);
        let reserve: RebuildReserveFn = std::mem::transmute(reserve_ptr);
        let rebuild: RebuildRebuildFn = std::mem::transmute(rebuild_ptr);
        for side in &rebuilds {
            // The trio the 0x1044 handler runs at playhead 0, re-run
            // directly at T_q (research §2.1/§3.2).
            let vec_ptr = side.actor.add(GPA_RECORDS_BEGIN_OFFSET);
            clear(vec_ptr);
            reserve(vec_ptr, side.record_count);
            let context = RebuildContext {
                actor: side.actor,
                playhead: plan.t_q,
                _pad: 0,
            };
            let mut out = [0u64; 2];
            rebuild(
                out.as_mut_ptr() as *mut u8,
                side.notes_begin,
                side.notes_begin.add(side.notes_len),
                &context,
            );
            // Spanning-freeze neutralization (R14), bounds-checked
            // against the LIVE rebuilt vector.
            let records_begin = memory::read_ptr(side.actor.add(GPA_RECORDS_BEGIN_OFFSET)) as usize;
            let records_end = memory::read_ptr(side.actor.add(GPA_RECORDS_END_OFFSET)) as usize;
            let records_len = records_end.saturating_sub(records_begin);
            for write in &side.writes {
                if records_begin != 0 && write.byte_offset + 4 <= records_len {
                    memory::write_i32((records_begin + write.byte_offset) as *mut u8, write.value);
                } else {
                    log_warn!(
                        "SongReset: neutralization write at +{:#x} out of bounds -- skipped",
                        write.byte_offset
                    );
                }
            }
        }
    }
    true
}

fn perform_seek(
    dps: *mut u8,
    actors: &[*mut u8],
    snapshot: &[SideSnapshot],
    plan: &SeekPlan,
) -> bool {
    if !perform_adjust(dps, actors, plan) {
        return false;
    }
    // Accumulator policy (Zero — the only v1 policy) + gauge restore +
    // HUD refresh: the shipped block, shared with the t=0 reset.
    reset_side_state(dps, actors, snapshot);
    true
}

/// Phase 2: per-frame driver. Polls the cue-prepared byte, then performs
/// the whole re-anchor + zero + refresh in one synchronous callback.
fn driver_step(
    generation: usize,
    started: Instant,
    handle: i32,
    snapshot: Vec<SideSnapshot>,
    on_recovery: Option<fn()>,
) {
    widget_renderer::run_on_render_thread(move || {
        if GENERATION.load(Ordering::Acquire) != generation {
            return; // superseded by a newer reset or a scene change
        }
        if scene_manager::current_scene() != scene::GAMEPLAY {
            RESET_IN_FLIGHT.store(false, Ordering::Release);
            return;
        }
        if started.elapsed().as_secs() >= PREPARE_TIMEOUT_SECS {
            RESET_IN_FLIGHT.store(false, Ordering::Release);
            log_warn!(
                "SongReset: cue {} never prepared ({}s) -- invoking recovery",
                handle,
                PREPARE_TIMEOUT_SECS
            );
            if let Some(recover) = on_recovery {
                recover();
            }
            return;
        }

        let prepared = unsafe {
            let is_prepared: SongPreparedFn =
                std::mem::transmute(SONG_PREPARED.load(Ordering::Acquire));
            is_prepared(handle) != 0
        };
        if !prepared {
            driver_step(generation, started, handle, snapshot, on_recovery);
            return;
        }

        // Re-validate the world before touching it: the player may have
        // died (actor step 5) during the silence window, the DPS may have
        // moved on. Any surprise ⇒ recovery, never a partial reset.
        let Some((dps, actors)) = revalidated_world(&snapshot) else {
            RESET_IN_FLIGHT.store(false, Ordering::Release);
            log_warn!("SongReset: world changed during prepare window -- invoking recovery");
            if let Some(recover) = on_recovery {
                recover();
            }
            return;
        };

        if !perform_reset(dps, &actors, &snapshot, 0) {
            RESET_IN_FLIGHT.store(false, Ordering::Release);
            log_warn!("SongReset: anchor broadcast refused -- invoking recovery");
            if let Some(recover) = on_recovery {
                recover();
            }
            return;
        }
        RESET_IN_FLIGHT.store(false, Ordering::Release);
        log_info!(
            "SongReset: complete -- stop->anchor {:.0} ms",
            started.elapsed().as_secs_f32() * 1000.0
        );

        notify_subscribers(0);
    });
}

/// Countdown driver for the delayed restart. The field is already reset
/// with a future-dated anchor (music count negative, notes approaching in
/// silence); this driver replays the cue `REPLAY_LEAD_MS` before the
/// countdown ends and re-anchors (`0x1044 {now}`) the moment it is both
/// prepared and due — the same prepared→anchor adjacency as the instant
/// path. The re-anchor is idempotent: at negative music count nothing is
/// judgeable, so the judge-record rebuild reproduces identical state.
fn countdown_step(
    generation: usize,
    started: Instant,
    delay_ms: u64,
    bank_name: Vec<u8>,
    handle: Option<i32>,
    expected_actors: usize,
    on_recovery: Option<fn()>,
) {
    widget_renderer::run_on_render_thread(move || {
        if GENERATION.load(Ordering::Acquire) != generation {
            return; // superseded by a newer reset or a scene change
        }
        if scene_manager::current_scene() != scene::GAMEPLAY {
            RESET_IN_FLIGHT.store(false, Ordering::Release);
            return;
        }
        let recover_with = |why: &str| {
            RESET_IN_FLIGHT.store(false, Ordering::Release);
            log_warn!("SongReset: {} -- invoking recovery", why);
            if let Some(recover) = on_recovery {
                recover();
            }
        };
        let elapsed_ms = started.elapsed().as_millis() as u64;
        if elapsed_ms >= delay_ms + PREPARE_TIMEOUT_SECS * 1000 {
            recover_with("delayed replay never prepared");
            return;
        }

        // The world must still look reset-shaped every step: DPS in-song,
        // the expected actors at the in-song step (nothing is judgeable at
        // negative music count, so any drift means outside interference).
        let ready = (|| -> Option<*mut u8> {
            let dps = live_dps()?;
            unsafe {
                if read_step(dps, DPS_STEP_BASE, DPS_STEP_INDEX)? != DPS_STEP_IN_SONG {
                    return None;
                }
            }
            let actors = gameplay_actors(dps);
            if actors.len() != expected_actors {
                return None;
            }
            for actor in &actors {
                if unsafe { read_step(*actor, GPA_STEP_BASE, GPA_STEP_INDEX) }
                    != Some(GPA_STEP_IN_SONG)
                {
                    return None;
                }
            }
            Some(dps)
        })();
        let Some(dps) = ready else {
            recover_with("world changed during countdown");
            return;
        };

        match handle {
            None => {
                // Waiting to issue the replay.
                if elapsed_ms + REPLAY_LEAD_MS < delay_ms {
                    countdown_step(
                        generation,
                        started,
                        delay_ms,
                        bank_name,
                        None,
                        expected_actors,
                        on_recovery,
                    );
                    return;
                }
                let new_handle = unsafe {
                    let play: SongPlayFn = std::mem::transmute(SONG_PLAY.load(Ordering::Acquire));
                    play(5, bank_name.as_ptr())
                };
                if new_handle == -1 {
                    recover_with("delayed replay failed (handle -1)");
                    return;
                }
                unsafe {
                    memory::write_i32(dps.add(DPS_CUE_HANDLE_OFFSET) as *mut u8, new_handle);
                }
                countdown_step(
                    generation,
                    started,
                    delay_ms,
                    bank_name,
                    Some(new_handle),
                    expected_actors,
                    on_recovery,
                );
            }
            Some(h) => {
                let prepared = unsafe {
                    let is_prepared: SongPreparedFn =
                        std::mem::transmute(SONG_PREPARED.load(Ordering::Acquire));
                    is_prepared(h) != 0
                };
                if !prepared || elapsed_ms < delay_ms {
                    countdown_step(
                        generation,
                        started,
                        delay_ms,
                        bank_name,
                        Some(h),
                        expected_actors,
                        on_recovery,
                    );
                    return;
                }
                if !broadcast_anchor_now(dps) {
                    recover_with("countdown re-anchor refused");
                    return;
                }
                RESET_IN_FLIGHT.store(false, Ordering::Release);
                log_info!(
                    "SongReset: complete -- delayed start at {} ms (target {} ms)",
                    elapsed_ms,
                    delay_ms
                );
            }
        }
    });
}

/// Broadcast `0x1044 {now}` to the DPS subtree — the countdown's final
/// re-anchor. Idempotent per-actor rewind (see `FUN_18005bac0`).
fn broadcast_anchor_now(dps: *mut u8) -> bool {
    unsafe {
        let tick_global = FRAME_TICK_GLOBAL.load(Ordering::Acquire);
        let frame_struct = memory::read_ptr(tick_global);
        if frame_struct.is_null() {
            return false;
        }
        let mut tick = memory::read_u64(frame_struct.add(FRAME_TICK_FIELD_OFFSET));
        broadcast_to_subtree(dps, MSG_TIMING_ANCHOR, &mut tick as *mut u64 as *mut u8)
    }
}

/// Notify `on_song_reset` subscribers (outside the registry lock, on the
/// frame thread, after game state is fully reset) with the content time
/// the run was reset to — 0 for restarts, T_q for seeks.
fn notify_subscribers(t_ms: i32) {
    let callbacks: Vec<ResetCallback> = SUBSCRIBERS
        .lock()
        .map(|subs| subs.iter().map(|(_, cb)| cb.clone()).collect())
        .unwrap_or_default();
    for cb in callbacks {
        cb(t_ms);
    }
}

/// The synchronous heart of the reset: engine re-anchor first, then the
/// accumulator zeroing, gauge restore and HUD refresh. Runs in one frame
/// callback so no judge tick can interleave. Returns false when the
/// engine protocol could not be delivered — in that case NOTHING was
/// zeroed (the run keeps playing over the replayed cue and the caller
/// must recover): zeroed accumulators without the judge-record rebuild
/// would corrupt the run.
///
/// `anchor_delay_ms` future-dates the timing anchor (the delayed-restart
/// countdown): the music count runs negative until the delay elapses,
/// which is the engine's own pre-song lead-in state.
fn perform_reset(
    dps: *mut u8,
    actors: &[*mut u8],
    snapshot: &[SideSnapshot],
    anchor_delay_ms: u64,
) -> bool {
    unsafe {
        // 1. The engine's own song-start protocol: 0x1043 (pre-start arm)
        //    then 0x1044 (timing anchor → per-actor judge-record rebuild
        //    at playhead 0 + clock re-anchor). Payloads mirror the DPS
        //    state 5/6 sites.
        let tick_global = FRAME_TICK_GLOBAL.load(Ordering::Acquire);
        let frame_struct = memory::read_ptr(tick_global);
        if frame_struct.is_null() {
            log_warn!("SongReset: frame clock struct is null -- aborting before anchor");
            return false;
        }
        let mut arm_payload = [0u8; 16];
        if !broadcast_to_subtree(dps, MSG_PRE_START_ARM, arm_payload.as_mut_ptr()) {
            return false;
        }
        let mut tick =
            memory::read_u64(frame_struct.add(FRAME_TICK_FIELD_OFFSET)) + anchor_delay_ms;
        if !broadcast_to_subtree(dps, MSG_TIMING_ANCHOR, &mut tick as *mut u64 as *mut u8) {
            return false;
        }
    }
    // 2. Accumulator zero + gauge restore + HUD refresh, per side.
    reset_side_state(dps, actors, snapshot);
    true
}

/// The accumulator zero + gauge restore + HUD refresh block
/// ([`AccumulatorPolicy::Zero`]) — shared verbatim by the t=0 reset and
/// the seek completion, so the shipped restart semantics ARE the seek's
/// policy application.
fn reset_side_state(dps: *mut u8, actors: &[*mut u8], snapshot: &[SideSnapshot]) {
    unsafe {
        for actor in actors {
            let actor = *actor;
            let side = memory::read_i32(actor.add(GPA_SIDE_OFFSET));

            for slot in 0..GPA_JUDGE_COUNT_SLOTS {
                memory::write_i32(actor.add(GPA_JUDGE_COUNTS_OFFSET + slot * 4) as *mut u8, 0);
            }
            for offset in [
                GPA_FREEZE_OK_OFFSET,
                GPA_FAST_OFFSET,
                GPA_SLOW_OFFSET,
                GPA_JUDGED_EVENTS_OFFSET,
                GPA_SCORE_OFFSET,
                GPA_EX_SCORE_OFFSET,
                GPA_COMBO_OFFSET,
                GPA_MAX_COMBO_OFFSET,
                GPA_MISS_STREAK_OFFSET,
            ] {
                memory::write_i32(actor.add(offset) as *mut u8, 0);
            }
            memory::write_u8(actor.add(GPA_IS_DEAD_OFFSET) as *mut u8, 0);
            memory::write_u8(actor.add(GPA_SONG_FINISHED_OFFSET) as *mut u8, 0);
            // Death result + gauge tracking cluster back to ctor values (the
            // gauge's own 0x103F below re-seeds the cluster exactly like the
            // first real delta). Build-dependent offsets; `init` refused
            // without the layout, so this is only a belt-and-braces guard.
            if let Some(layout) = gpa_layout() {
                memory::write_u8(actor.add(layout.death_result) as *mut u8, 0);
                memory::write_f32(actor.add(layout.gauge_min) as *mut u8, 1.0);
                memory::write_f32(actor.add(layout.gauge_max) as *mut u8, 0.0);
                memory::write_f32(actor.add(layout.gauge_last) as *mut u8, 0.0);
                memory::write_f32(actor.add(layout.gauge_loss) as *mut u8, 0.0);
                memory::write_f32(actor.add(layout.gauge_gain) as *mut u8, 0.0);
            }

            // Gauge children: ctor-mirror restore from the snapshot.
            let Some(side_snap) = snapshot.iter().find(|s| s.actor == actor as usize) else {
                continue; // validated by the caller; defensive only
            };
            // The dynamic population counter back to its song-start
            // baseline (see GPA_DYNAMIC_POP_OFFSET): the engine never
            // rewinds it, and a replayed pass cannot re-earn its
            // numerator twins — leaving it fat caps the money score
            // below 1MM on every loop/restart replay.
            memory::write_i32(
                actor.add(GPA_DYNAMIC_POP_OFFSET) as *mut u8,
                side_snap.note_pop_baseline,
            );
            for g in &side_snap.gauges {
                let gauge = g.gauge as *mut u8;
                match g.class {
                    GaugeClass::Percent | GaugeClass::Flare | GaugeClass::Grade => {
                        // Grade-specific state FIRST: the best-EX-score
                        // watermark back to the ctor's INT_MIN sentinel
                        // (EX score restarts at 0 — a fat pre-reset
                        // watermark over-penalizes early misses on the
                        // replayed run until the first good judge
                        // rewrites it). Attested layout — request_reset
                        // refuses when a GRADE gauge is live without it.
                        if g.class == GaugeClass::Grade {
                            memory::write_i32(
                                gauge.add(GRADE_WATERMARK_OFFSET) as *mut u8,
                                GRADE_WATERMARK_SENTINEL,
                            );
                        }
                        // Flare-specific state FIRST (attested layout —
                        // request_reset refuses when a FLARE gauge is
                        // live without a FlareSnapshot):
                        //   - zero the per-grade judge-history counters
                        //     and the good-judge streak (the FLOATING
                        //     demotion input — stale counters compound
                        //     across restarts until an early miss
                        //     cascades a multi-level demotion; the
                        //     reported "Floating Flare became FLARE 5/7"
                        //     bug),
                        //   - restore the per-level gauge array,
                        //   - restore the side's CURRENT flare level
                        //     (Option+0x7C) to its song-start seed
                        //     (FLOATING ⇒ 10 = EX). The HUD flare badge
                        //     re-derives from Option+0x7C on the next
                        //     diff-driven update (stale +0x9C, v6 rule).
                        if let Some(flare) = &g.flare {
                            let v1 = FLARE_LAYOUT_V1.load(Ordering::Acquire);
                            let history_first = if v1 {
                                FLARE_V1_HISTORY_FIRST_OFFSET
                            } else {
                                FLARE_HISTORY_FIRST_OFFSET
                            };
                            if !v1 {
                                memory::write_i32(gauge.add(FLARE_STREAK_OFFSET) as *mut u8, 0);
                            }
                            for slot in 0..FLARE_HISTORY_SLOTS {
                                memory::write_i32(
                                    gauge.add(history_first + slot * 4) as *mut u8,
                                    0,
                                );
                            }
                            if !v1 {
                                for (slot, value) in flare.level_gauges.iter().enumerate() {
                                    memory::write_i32(
                                        gauge.add(FLARE_LEVELS_FIRST_OFFSET + slot * 4) as *mut u8,
                                        *value,
                                    );
                                }
                            }
                            if let Some(option) = player_option_ptr(side) {
                                memory::write_i32(
                                    option.add(OPTION_FLARE_LEVEL_OFFSET) as *mut u8,
                                    flare.level,
                                );
                            } else {
                                log_warn!(
                                    "SongReset: side {} Option unreachable at restore -- flare level NOT restored",
                                    side
                                );
                            }
                        }
                        memory::write_i32(gauge.add(GAUGE_VALUE_OFFSET) as *mut u8, g.start_value);
                        // The display latches (+0x94/+0x98/+0x9C) stay
                        // STALE on purpose (v6): the per-frame update's
                        // diff gate then takes the full path next tick —
                        // it animates the bar to the restored value,
                        // re-classifies the gauge_usr color label from
                        // the true old state, and emits the engine's own
                        // 0x1038 danger-off when leaving state 3/0x10.
                        // (v5 snapped the display and zeroed the state:
                        // the update early-outed forever — stale COLOR
                        // until the first judge — and the destroyed old
                        // state suppressed the danger-off, leaving the
                        // lane flashing red. Cabinet-observed
                        // 2026-08-16.)
                        for offset in [GAUGE_MISS_STREAK_OFFSET, GAUGE_D0_OFFSET, GAUGE_D4_OFFSET] {
                            memory::write_i32(gauge.add(offset) as *mut u8, 0);
                        }
                        for slot in 0..GAUGE_CACHE_SLOTS {
                            memory::write_i32(
                                gauge.add(GAUGE_CACHE_FIRST_OFFSET + slot * 4) as *mut u8,
                                0,
                            );
                        }
                        memory::write_u8(
                            gauge.add(GAUGE_EMPTIED_OFFSET) as *mut u8,
                            (g.start_value <= 0) as u8,
                        );
                        memory::write_u8(gauge.add(GAUGE_RISKY_DEPLETED_OFFSET) as *mut u8, 0);

                        // Tracking resync (GamePlayActor +0x2A0 cluster +
                        // any HUD gauge boards) via the gauge's own report
                        // message shape.
                        #[repr(C)]
                        struct GaugePercentPayload {
                            side: i32,
                            pct: f32,
                        }
                        let mut payload = GaugePercentPayload {
                            side,
                            pct: g.start_value as f32 / GAUGE_VALUE_SCALE,
                        };
                        broadcast_to_subtree(
                            dps,
                            MSG_GAUGE_PERCENT,
                            &mut payload as *mut GaugePercentPayload as *mut u8,
                        );
                    }
                    GaugeClass::Lives => {
                        memory::write_i32(
                            gauge.add(LIFE_GAUGE_LIVES_OFFSET) as *mut u8,
                            g.start_value,
                        );
                        // Dead latch (BYTE — +0xB1..+0xB3 is padding but the
                        // ctor writes a byte, so we do too). The display
                        // latches (+0x94 last-displayed lives, +0xA0
                        // display mode, +0xB4 substate) stay STALE like
                        // the percent family's (v6): the update's
                        // `+0x94 != +0x90` diff then runs the full block
                        // next tick and its mode transition emits the
                        // engine's own 0x1038 danger-off. The qword at
                        // +0xA8 is this actor's own pointer/timer slot —
                        // never touched.
                        memory::write_u8(
                            gauge.add(LIFE_GAUGE_DEAD_LATCH_OFFSET) as *mut u8,
                            (g.start_value <= 0) as u8,
                        );
                    }
                }
            }

            // ScoreActor (a direct child): zero the display target and
            // seed the displayed value with the ctor's -1 sentinel — the
            // render pass diffs digits against the displayed value and
            // only repaints changes, so without the sentinel any
            // significant zeros of the pre-reset score stay lit with
            // stale bright bitmaps (2026-08-12 v2 cabinet observation).
            // -1 forces the same full repaint a fresh song start gets.
            let score_vtable = SCORE_ACTOR_VTABLE.load(Ordering::Acquire);
            let note_result_vtable = NOTE_RESULT_ACTOR_VTABLE.load(Ordering::Acquire);
            let mut child = memory::read_ptr(actor.add(FIRST_CHILD_OFFSET)) as *mut u8;
            while !child.is_null() {
                let child_vtable = memory::read_ptr(child) as *mut u8;
                if child_vtable == score_vtable {
                    memory::write_i32(child.add(SCORE_ACTOR_TARGET_OFFSET) as *mut u8, 0);
                    memory::write_i32(
                        child.add(SCORE_ACTOR_DISPLAYED_OFFSET) as *mut u8,
                        SCORE_ACTOR_DISPLAYED_SENTINEL,
                    );
                } else if !note_result_vtable.is_null() && child_vtable == note_result_vtable {
                    // NoteResultActor: rewind the pacemaker
                    // (dance_score_compare) clip to the song-start state
                    // its onSetup produces (frame 0, paused). Undoes the
                    // msg-0x103A "out" outro — a one-way latch that
                    // otherwise freezes the readout (stock score-delta OR
                    // the PUS ms-error swap) for every in-place
                    // loop/restart after a gauge-empty in an earlier
                    // pass. The next judged step's 0x1036 replays it
                    // exactly like the first judge of a fresh song.
                    let clip = memory::read_ptr(child.add(NOTE_RESULT_PACEMAKER_CLIP_OFFSET));
                    if !clip.is_null() {
                        // Signed validity checks mirror the engine's own
                        // `id < 1 => invalid` guards.
                        let mc_id = memory::read_i32(clip.add(CLIP_MC_ID_OFFSET));
                        if mc_id > 0 {
                            bm2d_api::mc_op(mc_id as u32, MC_OP_SET_FRAME, 0);
                        }
                        let layer_id = memory::read_i32(clip.add(CLIP_LAYER_ID_OFFSET));
                        if layer_id > 0 {
                            bm2d_api::layer_play_raw(layer_id as u32, 0.0);
                        }
                    }
                }
                child = memory::read_ptr(child.add(NEXT_SIBLING_OFFSET)) as *mut u8;
            }

            // Combo display refresh (the judge dispatcher's own payload
            // shape; consumers that ignore it self-correct at the first
            // judge event). NOTE: msg 0x1045 is deliberately NOT sent —
            // its only consumer is the GaugeActor score cache, which the
            // gauge restore above already zeroes directly; the score
            // DISPLAY listens to 0x1036/rival-sync, handled by the
            // sentinel write above.
            #[repr(C)]
            struct ComboPayload {
                side: i32,
                combo: i32,
                max_combo: i32,
                grade: i32,
                flag: u8,
            }
            let mut combo = ComboPayload {
                side,
                combo: 0,
                max_combo: 0,
                grade: 0,
                flag: 0,
            };
            broadcast_to_subtree(
                dps,
                MSG_COMBO_UPDATE,
                &mut combo as *mut ComboPayload as *mut u8,
            );
        }
    }
}
