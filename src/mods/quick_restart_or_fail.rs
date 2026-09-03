//! Quick Restart / Quick Fail Mod — pinpad gestures during gameplay.
//!
//! Listens for two single-press gestures on either pinpad during scene 28
//! (GAMEPLAY), the FF/RW scrub model (one press = one action, no
//! GestureBuffer):
//!   - **3** → Quick Fail. Cut straight back to song select — no fade,
//!     no STAGE FAILED banner, no results screen. The Mods-tab option
//!     `skip_results_fast_exit` ("SKIP RESULTS ON FAST EXIT", default ON)
//!     lets a player opt out of the skip: with the PRESSING side's value
//!     OFF, the fail takes the natural flow instead (fade + FAILED banner
//!     + the stage results screen showing the score up to the drop-out +
//!     the game's own natural tail). See §13 of the research doc for why
//!     a direct results-loader `finish` was rejected (the play record is
//!     only committed by the natural song-end machinery).
//!   - **1** → Quick Restart. Cut straight into a fresh play of the same
//!     song — no fade, no banner, no READY panel. A press while a
//!     previous restart's in-place reset is still in flight is dropped
//!     (the scrub's `reset_in_flight` precedent), so a double-tap can't
//!     escalate into the fresh-DPS reload path.
//!
//! ## Fast path (primary, 2026-08-12): dismiss the stage shutter, then `finish`
//!
//! The two earlier `finish`-based attempts limbo'd, and the cabinet
//! diagnostic pass found the real, single root cause: mid-song the **stage
//! shutter** (the kind-3 jacket "READY?" panel) is **parked at state 6** — a
//! state that only advances when a new shutter request arrives. Both the
//! loader's mask-apply gate (`state ∈ {0,4}`) and a fresh
//! `DancePlaySequence`'s state-1 gate (`state==4 || (no-pending &&
//! state==0)`) reject state 6 forever, so any `finish`-installed successor
//! ticks but never proceeds. (The 2026-08-12 diagnostic explicitly refuted
//! the earlier "background/movie exit gate" hypothesis: `FUN_180031af0`
//! reads TRUE mid-song; `shutter=6` was the smoking gun.) The natural flow
//! never hits this because DPS state 8's banner request is itself what
//! un-parks the shutter — the banner IS the drain mechanism.
//!
//! The ShutterActor has a purpose-built bannerless dismiss: **msg `0x100c`**
//! → "if the stage shutter (kind 3) is active, force state 7" → out-tail →
//! state 8 releases the layer → **state 0 idle with no pending kind — no
//! banner art ever loads**. So the fast path is:
//!
//!   1. Gate on the shutter being in a known-good state: idle
//!      (`0`, nothing active or pending) needs no dismiss; parked-revealed
//!      (`6`, active kind 3, no pending) gets `0x100c` sent via the actor's
//!      own `onMessage` (vt+0x18), verified by reading state 7 back.
//!      Any other state (transitional, READY panel, banner mid-close) ⇒
//!      natural-death fallback.
//!   2. `finish(DPS, target_1idx)` — restart target `0x1C` (0-idx 27 stage
//!      loader, `getNextID → 0x1D` fresh gameplay; packages already
//!      resident), fail target `0x19` (0-idx 24 select loader, `getNextID →
//!      0x1A` song select). The loader ticks, waits the sub-second shutter
//!      drain (7→8→0), applies its masks, passes the (cabinet-confirmed
//!      already-true) background exit gate, and finishes into the target the
//!      natural way. The fresh DPS explicitly supports an idle shutter — it
//!      simply skips the READY panel, which is the instant restart we want.
//!
//! Fail additionally requires the session-continues predicate (`!course &&
//! event == 0 && override == -1 && stage < max_stage`): jumping to song
//! select skips `ResultSequence`, the game's only session-over decision, so
//! a final/extra-stage fail must keep the natural flow (which ends the
//! session properly). Restart needs no predicate — the shipped 29→28
//! redirect has skipped results with unchanged stage-counter semantics
//! since 2026-05.
//!
//! Resolution: `sequence_finish` (AOB) + `shutter_actor_global` (derived
//! from the `shutter_close_request` wrapper AOB — the msg-0x1007 imm pins
//! it). Struct offsets on the ShutterActor (StackStep `+0x58`/`+0x82`,
//! active kind `+0x310`, pending `+0x314`) are validated by range checks at
//! use; any surprise ⇒ fallback. `finish` is synchronous and re-enters our
//! scene hook — NO lock may be held across it. The `0x100c` send is a
//! synchronous state write on the same frame thread the game dispatches
//! messages on.
//!
//! ## Fallback: natural death + scene redirects
//!
//! Force every active GamePlayActor to `STEP_GAME_OVER` + death flags and
//! let DPS states 8/9 run (fade, stop song, FAILED banner), then cut the
//! tail with a one-shot redirect: restart 29→28 (fresh DPS, proven since
//! 2026-05), fail 29→24 (skip results; predicate + repair gated, else the
//! full natural tail). Cabinet-validated 2026-08-12.
//!
//! ## The pre-song READY window (2026-08-31 soft-lock fix)
//!
//! Before the song starts (DPS pre-song init states 0..=6 — the "READY?"
//! banner period), the natural-death fallback is UNSAFE: forcing
//! `STEP_GAME_OVER` into mid-init GamePlayActors leaves DPS parked in a
//! pre-song state that never consults the death flags, so its state-8
//! banner request never fires and nothing drains the shutter — a soft
//! lock (cabinet-observed for quick-fail; restart took the identical
//! path). The window is detected by reading the DPS step (`dps_pre_song`)
//! and handled explicitly:
//!
//!   - **Fail (3)**: fast path only. The `0x100c` dismiss works from the
//!     covering/revealing panel states (4/5) exactly as from the mid-song
//!     park (the handler only checks `active kind == 3`), so the fast
//!     `finish(DPS, 0x19)` exit to song select is available for the whole
//!     READY window. Any gate refusal ⇒ the gesture is IGNORED (never the
//!     fallback), and the quick-fail taint is only set on success.
//!   - **Restart (1)**: ignored — the song hasn't started, so a restart is
//!     semantically a no-op, and no restart shape is safe pre-song.
//!   - `fail_song` itself refuses pre-song as a structural backstop.
//!
//! ## Early-press drain stall (2026-08-31, the limbo actually observed)
//!
//! On shutter art whose `shutter_play` clip carries no frame labels, the
//! drain's state-7 `"out"` play silently fails (the clip never advances)
//! and state 8's wait (`current >= max(frame("out_end"), frame("end"))`)
//! never passes while the clip is still near frame 0 — i.e. any dismiss in
//! the first seconds of the song limbos, while mid-song dismisses are
//! masked (the clip already sits at its final frame). Fixed by
//! `unblock_shutter_drain` after every verified dismiss: SetFrame the clip
//! to the wait's own target. No-op on healthy art (state 7's `"out"` play
//! re-seeks the playhead anyway); fail-open. A silent post-`finish`
//! watchdog remains permanently: destination scene within 20 s, or ONE
//! gate sample + LIMBO WARN.
//!
//! See `docs/quick_restart_fail_speedup_research.md` for the full RE record
//! (§4a the corrected root cause, §4b the fast path) and
//! `.agents/planning/20260523-bulk-hack-porting/research/quick-restart-pivot.md`
//! for the original investigation history.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::core::signatures::{GamePlayActorLayout, ShutterActorLayout};
use crate::core::{memory, module_resolver};
use crate::mods::config;
use crate::mods::mod_menu;
use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::custom_options::{self, RegisterSpec};
use crate::services::{
    bm2d_api, input_manager, scene_manager, score_guard, song_reset, stage_records, widget_renderer,
};
use crate::types::buttons::*;
use crate::types::scenes::scene;
use crate::{log_debug, log_info, log_warn};

// ── Restart-delay knob (`quick_restart.restart_delay_ms`) ───────────
/// Live restart-delay value in ms (config-seeded at init, adjusted from
/// the overlay row, read at gesture time). The in-place reset still
/// resets the FIELD instantly; this delays the restarted song's start
/// (a countdown to get back in position).
static RESTART_DELAY_MS: AtomicI32 = AtomicI32::new(0);
/// Overlay enum-row key for the delay knob.
const DELAY_ROW_KEY: &str = "restart-delay";
/// Delay clamp bounds (ms). 10 s is far beyond any useful countdown.
const DELAY_MIN_MS: i32 = 0;
const DELAY_MAX_MS: i32 = 10_000;

/// Preset delay steps offered by the overlay row: 0 (INSTANT) through
/// 10.0 s in 0.5 s steps. An operator-edited config value outside this
/// list is auto-added (fps_unlock's normalization pattern) so the row
/// always shows the active value.
fn delay_presets() -> Vec<i32> {
    (0..=DELAY_MAX_MS).step_by(500).collect()
}

// ── "SKIP RESULTS ON FAST EXIT" player option ────────────────────────
/// Per-side choice for the quick-fail gesture (press 3), written by the
/// `skip_results_fast_exit` options-menu row and read at gesture time.
/// `true` (the default, ON) = today's behavior: cut straight to song
/// select, skipping the results screen. `false` (OFF) = take the natural
/// fail flow instead — fade + FAILED banner + the stage results screen
/// showing the score breakdown up to the drop-out, then the game's own
/// natural tail. The side that PRESSED 3 governs (the fail itself is
/// cabinet-wide either way).
///
/// OFF deliberately reuses the proven natural-death fallback rather than
/// a `finish` jump into the 0-idx 29 result loader: the per-stage record
/// ResultSequence displays is only written by the result commit
/// (GamePlayActor vtable +0x28) during the natural song-end machinery —
/// a mid-song `finish` would render an all-zero results screen. See
/// `docs/quick_restart_fail_speedup_research.md` §10.
static SKIP_RESULTS: [AtomicBool; 2] = [AtomicBool::new(true), AtomicBool::new(true)];

/// Options-menu row callback for `skip_results_fast_exit`. Runs on the
/// game's render thread — atomics only, non-blocking.
fn skip_results_on_change(player_side: u8, new_value: i32) {
    if player_side < 2 {
        let skip = new_value != 0;
        SKIP_RESULTS[player_side as usize].store(skip, Ordering::Release);
        log_info!(
            "QuickRestartOrFail: skip-results-on-fast-exit side={} {}",
            player_side,
            if skip { "ON" } else { "OFF" }
        );
    }
}

/// Register the SKIP RESULTS ON FAST EXIT row on the Mods tab (default
/// ON = today's instant cut). Registration failure degrades to the
/// default: every quick fail skips results.
fn register_skip_results_option() {
    if !custom_options::is_available() {
        log_warn!(
            "QuickRestartOrFail: custom_options unavailable -- skip-results option row will not render (defaulting to skip)"
        );
        return;
    }
    let spec = RegisterSpec::bool_toggle("skip_results_fast_exit")
        .display_name("Skip Results on Fast Exit")
        .description(
            "Skip the results screen when failing out of a song with the quick-exit gesture",
        )
        .default_value(1)
        .on_change(skip_results_on_change);
    match custom_options::register_option(spec) {
        Ok(_handle) => {
            log_info!("QuickRestartOrFail: registered skip-results option on Mods tab");
        }
        Err(e) => {
            log_warn!(
                "QuickRestartOrFail: skip-results option registration failed: {e} -- defaulting to skip"
            );
        }
    }
}

fn delay_label(ms: i32) -> String {
    if ms == 0 {
        "INSTANT".to_string()
    } else {
        format!("{:.1}s", ms as f32 / 1000.0)
    }
}

/// Offset of the active gosub-child slot on a sequence.
/// `*(seq + ACTIVE_CHILD_OFFSET)` is the running DancePlaySequence
/// when the parent is the TransitionSequence and the scene is GAMEPLAY.
const ACTIVE_CHILD_OFFSET: usize = 0x58;

/// Offset of the first-child pointer on an actor.
const FIRST_CHILD_OFFSET: usize = 0x18;

/// Offset of the next-sibling pointer on an actor.
const NEXT_SIBLING_OFFSET: usize = 0x10;

/// Offset of `agcs::StackStep`'s active step slot on `GamePlayActor`.
/// Writing `STEP_GAME_OVER` here drops the actor into the natural
/// game-over fade-out, which advances through STEP_END and bubbles up
/// to DPS's STEP_FINISH on subsequent update ticks. (Fallback path only.)
const GAMEPLAY_ACTOR_STEP_OFFSET: usize = 0x58;

/// `GamePlayActor::STEP_GAME_OVER`. Triggers the per-actor 0.25s
/// fade-out before STEP_END. (Fallback path only.)
const STEP_GAME_OVER: u32 = 5;

/// `GamePlayActor::m_isDead`. The natural fail-out path (DPS's
/// STEP_FINISH selecting CLEARED vs FAILED shutter, score
/// suppression, lamp lighting) reads this byte; without it the
/// player is treated as having cleared the song even with step
/// advanced past STEP_GAME_OVER. (Fallback path only.)
const GAMEPLAY_ACTOR_IS_DEAD_OFFSET: usize = 0x1E8;

// `GamePlayActor::m_canInstantDeath`-equivalent gate (`gauge::DEAD`'s case
// body in `GamePlayActor::onReceiveMessage` is guarded on this byte; writing
// 1 mirrors the hard-gauge configuration the natural flow expects when the
// player dies) and the death-result flag that case body sets. `+0x2B7/+0x2B8`
// on 20260324+ but `+0x2AF/+0x2B0` on 20250805 / 20260224 — read from
// `SignatureStore::gameplay_actor_layout()` at init. (Fallback path only;
// without the layout the fallback still forces `m_isDead` + STEP_GAME_OVER.)
static GPA_LAYOUT: std::sync::OnceLock<GamePlayActorLayout> = std::sync::OnceLock::new();

/// Sanity bound for the stage counter / max-stage values read by the
/// skip-results predicate; anything outside means a decode went wrong.
const MAX_SANE_STAGE: i32 = 9;

/// **1-indexed** `finish` targets (the ONLY 1-indexed scene ids in this
/// module; everything else is 0-indexed). Restart: 0x1C = the 0-idx 27
/// stage `LoadingSequence` (`getNextID → 0x1D` fresh gameplay). Fail:
/// 0x19 = the 0-idx 24 select `LoadingSequence` (`getNextID → 0x1A` song
/// select). Only safe AFTER the stage shutter is idle/dismissed — see the
/// module doc.
const STAGE_LOADER_1IDX: i32 = 0x1C;
const SELECT_LOADER_1IDX: i32 = 0x19;

// ── ShutterActor layout / protocol (validated by range checks at use) ──
/// `agcs::StackStep` state base on the ShutterActor (active slot at
/// `+0x58 + idx*8`, depth index `idx` at `+0x82`) — the same embedded
/// StackStep shape as `GAMEPLAY_ACTOR_STEP_OFFSET`.
const SHUTTER_STEP_BASE: usize = 0x58;
const SHUTTER_STEP_INDEX: usize = 0x82;
// Active shutter kind (`-1` = none), pending requested kind (written by msg
// 0x1007; `-1` = none) and the stage-jacket panel's kind id. `+0x310/+0x314`
// and kind 3 on 20260324+, but `+0x2E0/+0x2E4` and kind 1 on 20250805 /
// 20260224 (whose shutter has 6 kinds, not 9) — derived per build by
// `SignatureStore::derive_shutter_actor_layout` from the onUpdate kind/layer
// lookup + stage-kind compare. Without it every shutter read fails and the
// fast paths fall back (the pre-2026-09 behavior on the old builds).
static SHUTTER_LAYOUT: std::sync::OnceLock<ShutterActorLayout> = std::sync::OnceLock::new();
/// Shutter states the fast path understands: 0 = idle (layer released),
/// 4 = closed/covering (the READY-window park — the jacket panel fully
/// displayed, waiting for DPS state 5's `stage_out` send), 5 = the
/// `stage_out` reveal anim in flight, 6 = the stage panel parked after its
/// reveal (the mid-song state), 7 = the drain tail entered by the 0x100c
/// dismiss.
const SHUTTER_STATE_IDLE: i32 = 0;
const SHUTTER_STATE_COVERED: i32 = 4;
const SHUTTER_STATE_REVEALING: i32 = 5;
const SHUTTER_STATE_PARKED_REVEALED: i32 = 6;
const SHUTTER_STATE_DRAIN_TAIL: i32 = 7;
const SHUTTER_STATE_MAX: i32 = 8;
/// The ShutterActor's bannerless stage-panel dismiss: its custom message
/// handler forces state 7 iff the active kind is 3 (it does NOT check the
/// current state — the state-7 drain replays `stage_out` if it never ran,
/// so a dismiss from 4/5/6 all land in the same 7→8→0 idle-park), leaving
/// the pending kind untouched — the drain ends parked at idle with NO new
/// banner.
const MSG_SHUTTER_DISMISS_STAGE: i32 = 0x100C;
/// Actor tree-flags offset, the "destruction in progress, dispatch
/// suppressed" bit (the same guard the game's own message wrappers use),
/// and the composite "dead or dying" mask (a child with either bit set
/// must not be `finish`ed — post-transition window).
const TREE_FLAGS_OFFSET: usize = 0x20;
const TREE_FLAGS_DISPATCH_SUPPRESSED: u32 = 0x20;
const TREE_FLAGS_DEAD_MASK: u32 = 0x24;
/// vtable slot of `agcs::Actor::onMessage(this, msg, param)`.
const VTBL_ON_MESSAGE_OFFSET: usize = 0x18;

// ── Shutter drain unblock (2026-08-31 early-dismiss limbo fix) ────────
/// Per-kind layer table on the ShutterActor: the layer OBJECT pointer of a
/// `shared_ptr<Layer>` pair sits at `+0x88 + kind*0x10` (the control block
/// at `+0x90 + kind*0x10`) — the `local_200` the update's state waits read.
const SHUTTER_LAYER_TABLE_OFFSET: usize = 0x88;
/// AFP MovieClip id on the layer object (`*(u32*)(layer + 0x110)` — the id
/// every `afp_mc_get_param` in the shutter update targets).
const SHUTTER_LAYER_MC_ID_OFFSET: usize = 0x110;
/// `afp_mc_op` SetFrame opcode (BM2D::CMovieClip::SetFrame — same opcode
/// song_reset uses for the pacemaker clip rewind).
const MC_OP_SET_FRAME: i32 = 0xF08;

/// `agcs::Sequence::finish(this, nextSceneId_1INDEXED)`.
type SequenceFinishFn = unsafe extern "C" fn(*mut u8, i32);
/// `agcs::Actor::onMessage(this, msg, param) -> handled`.
type OnMessageFn = unsafe extern "C" fn(*mut u8, i32, *mut u8) -> i32;

/// Resolved `sequence_finish` fn ptr (null = fast paths unavailable; the
/// natural-death fallback still works). Also doubles as the diagnostic
/// sampler's 20260721 build guard.
static SEQUENCE_FINISH: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());

/// Resolved `shutter_actor_global` (the POINTER TO the ShutterActor
/// singleton pointer; null = fast paths unavailable).
static SHUTTER_GLOBAL: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());

// ── Select-residency patch ──────────────────────────────────────────
/// Resolved `gameplay_loader_masks` site (createNextSequence case 0x1c:
/// `MOV EDX,0x8000; MOV R8D,0x32000` — the stage loader's ctor masks).
static LOADER_MASKS_SITE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
/// One-shot latch for the residency patch (enable() can run again after a
/// mod-menu toggle; the patch is one-way).
static RESIDENCY_PATCHED: AtomicBool = AtomicBool::new(false);
/// The unload imm32 sits at this offset from the AOB match (after
/// `BA 00 80 00 00 41 B8`).
const LOADER_UNLOAD_IMM_OFFSET: usize = 7;
/// Stock unload mask 0x32000 (evicts select-music 0x2000 + scene_result
/// 0x10000 + 0x20000) → patched 0x30000 (keep select-music resident).
const LOADER_UNLOAD_STOCK: [u8; 4] = [0x00, 0x20, 0x03, 0x00];
const LOADER_UNLOAD_PATCHED: [u8; 4] = [0x00, 0x00, 0x03, 0x00];

// ── Restart ready-dwell skip + init-phase sampler ───────────────────
/// `DancePlaySequence`'s `agcs::StackStep`: values at `+0x68`, depth index
/// at `+0x92` (same embedded shape as the shutter's `+0x58`/`+0x82`).
const DPS_STEP_BASE: usize = 0x68;
const DPS_STEP_INDEX: usize = 0x92;
/// DPS elapsed-since-creation timer (`+0x130`, accumulated every frame).
/// State 5 gates song start on `DAT_18035a8b4 (= 5.0s) <= this` — the
/// "READY?" ready-dwell. On a fast restart the jacket panel is already
/// dismissed, so those ~3.9 s are dead air; seeding the timer past the
/// threshold clears the gate (the bank-prepared condition still holds).
const DPS_READY_TIMER_OFFSET: usize = 0x130;
/// Value seeded into the ready timer — comfortably above any plausible
/// dwell threshold (stock is 5.0s), written every frame until the song
/// starts so it can never clamp below a higher threshold and deadlock.
const DPS_READY_TIMER_SEED: f32 = 1000.0;
/// Generation counter — a new restart invalidates any driver chain still
/// re-queueing from a previous restart.
static SAMPLER_GEN: AtomicUsize = AtomicUsize::new(0);

/// First in-song DPS step. Steps 0..=6 are the pre-song init phase (layout,
/// actors, bank register/prepare, the "READY?" dwell, timing anchor); 7 =
/// in-song; 8/9 = the natural song-end tail. See the state table in the
/// `start_restart_init_sampler` doc.
const DPS_STEP_IN_SONG: i32 = 7;

// --- TEMPORARY limbo-root-cause diagnostics (2026-08-12) -------------------
// File-relative RVAs on the 20260721 cabinet build ONLY, guarded at runtime
// by the resolved `sequence_finish` address. First deploy confirmed the
// loader exit gate reads TRUE mid-song (bg system ready) and `shutter=6` —
// the shutter park is the root cause. Kept for one more deploy to observe
// the fast path's shutter fields; remove after the fast path is validated.

/// `agcs::Sequence::finish` RVA on 20260721 — the build guard.
const RVA_SEQUENCE_FINISH_20260721: usize = 0x21df70;
/// `DAT_1806f2d30` — the `BgMovieActor` singleton pointer global.
const RVA_BG_MOVIE_ACTOR_GLOBAL: usize = 0x6f2d30;
/// `DAT_1806f2d40` — the `ShutterActor` singleton pointer global.
const RVA_SHUTTER_GLOBAL: usize = 0x6f2d40;
/// `DAT_1806f2d68` — the scene resource/package manager pointer global
/// (`+0x24` = async-load-in-progress byte).
const RVA_SCENE_RES_MGR_GLOBAL: usize = 0x6f2d68;
/// `FUN_180031af0` — the loader exit gate's "background/movie system ready"
/// predicate. Reads `BgMovieActor` unguarded: only call when the singleton
/// is non-null (the loader guards the same way).
const RVA_BG_SYSTEM_READY_FN: usize = 0x31af0;
/// `FUN_18003f590(bgObj+0x150)` — "pending background switch settled" term.
const RVA_BG_SWITCH_READY_FN: usize = 0x3f590;
/// `FUN_18003fa20(map)` — "async movie map idle" term (maps at
/// `bgObj+0x348` and `bgObj+0x3f0`).
const RVA_BG_MAP_READY_FN: usize = 0x3fa20;

static GAMEPLAY_ACTOR_VTABLE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());

pub struct QuickRestartOrFailMod {
    input_cb_id: Option<usize>,
    scene_cb_id: Option<usize>,
    delay_row_registered: bool,
}

impl QuickRestartOrFailMod {
    pub fn new() -> Self {
        Self {
            input_cb_id: None,
            scene_cb_id: None,
            delay_row_registered: false,
        }
    }
}

/// Seed the live restart-delay from `quick_restart.restart_delay_ms`
/// (mod-config.json). Clamped; absent = 0 (instant).
fn load_delay_from_config() {
    let ms = config::get()
        .and_then(|c| c.quick_restart.as_ref())
        .and_then(|q| q.restart_delay_ms)
        .unwrap_or(0)
        .clamp(DELAY_MIN_MS, DELAY_MAX_MS);
    RESTART_DELAY_MS.store(ms, Ordering::Release);
}

/// Persist the live delay back to `mod-config.json` under
/// `quick_restart`. Whole-section replace (the section has one key).
fn persist_delay() {
    let ms = RESTART_DELAY_MS.load(Ordering::Acquire);
    config::save_json_key(
        "quick_restart",
        serde_json::json!({ "restart_delay_ms": ms }),
    );
}

/// Overlay enum-row callback: record + persist the new delay. Effective
/// on the very next restart gesture (read at gesture time). Runs on the
/// render/input thread — non-blocking, no game calls.
fn set_delay(value: i32) {
    RESTART_DELAY_MS.store(value.clamp(DELAY_MIN_MS, DELAY_MAX_MS), Ordering::Release);
    persist_delay();
    log_info!(
        "QuickRestartOrFail: restart delay set to {} (effective immediately)",
        delay_label(value)
    );
}

/// Register the `RESTART DELAY` enum row under the mod's master toggle
/// (optional tier — config-file control works without the overlay). An
/// operator-edited value outside the preset list is auto-added so the
/// row always displays the active selection.
fn register_delay_row() {
    let selected = RESTART_DELAY_MS.load(Ordering::Acquire);
    let mut values = delay_presets();
    if !values.contains(&selected) {
        values.push(selected);
        values.sort_unstable();
        values.dedup();
    }
    let labels = values.iter().map(|v| delay_label(*v)).collect();
    mod_menu::register_enum_row(mod_menu::EnumRowSpec {
        key: DELAY_ROW_KEY.to_string(),
        label: "Restart Delay".to_string(),
        hint: "Countdown before a quick-restarted song begins.".to_string(),
        parent_row_key: Some("quick-restart-or-fail".to_string()),
        values,
        labels,
        initial_value: selected,
        on_change: Arc::new(set_delay),
    });
    log_info!("QuickRestartOrFail: registered restart-delay overlay row");
}

impl Mod for QuickRestartOrFailMod {
    fn id(&self) -> &str {
        "quick-restart-or-fail"
    }
    fn name(&self) -> &str {
        "Quick Restart / Fail"
    }
    fn description(&self) -> &str {
        "Press 1 to restart song, press 3 to fail-out (during gameplay)"
    }
    fn required_signatures(&self) -> &[&str] {
        &["gameplay_actor_vtable"]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        let vtable = ctx.signatures.require_address("gameplay_actor_vtable");
        GAMEPLAY_ACTOR_VTABLE.store(vtable as *mut u8, Ordering::Release);

        // Restart-delay knob: seed the live value from config.
        load_delay_from_config();

        // Optional: the bannerless fast paths need both. Missing just means
        // every trigger takes the natural-death fallback below.
        match ctx.signatures.get_address("sequence_finish") {
            Some(addr) => SEQUENCE_FINISH.store(addr as *mut u8, Ordering::Release),
            None => log_warn!(
                "QuickRestartOrFail: sequence_finish unresolved -- fast paths unavailable (natural fail flow only)"
            ),
        }
        match ctx.signatures.get_address("shutter_actor_global") {
            Some(addr) => SHUTTER_GLOBAL.store(addr as *mut u8, Ordering::Release),
            None => log_warn!(
                "QuickRestartOrFail: shutter_actor_global unresolved -- fast paths unavailable (natural fail flow only)"
            ),
        }
        match ctx.signatures.shutter_actor_layout() {
            Some(layout) => {
                let _ = SHUTTER_LAYOUT.set(layout);
            }
            None => log_warn!(
                "QuickRestartOrFail: ShutterActor layout underived -- fast paths unavailable (natural fail flow only)"
            ),
        }
        match ctx.signatures.gameplay_actor_layout() {
            Some(layout) => {
                let _ = GPA_LAYOUT.set(layout);
            }
            None => log_warn!(
                "QuickRestartOrFail: GamePlayActor layout underived -- fallback death simulation skips the death-gate/result bytes"
            ),
        }
        // Optional: the select-residency patch site (applied at enable()).
        match ctx.signatures.get_address("gameplay_loader_masks") {
            Some(addr) => LOADER_MASKS_SITE.store(addr as *mut u8, Ordering::Release),
            None => log_warn!(
                "QuickRestartOrFail: gameplay_loader_masks unresolved -- select-residency patch unavailable (quick fail will reload the select packages, ~5s)"
            ),
        }
        true
    }

    fn enable(&mut self) {
        if !input_manager::is_available() {
            log_warn!("QuickRestartOrFail: input_manager unavailable -- gestures inactive");
            return;
        }

        apply_select_residency_patch();

        // Overlay knob (optional tier — config-file control works
        // regardless): the RESTART DELAY row nests under this mod's
        // master toggle, like FPS TARGET under FPS Unlock.
        register_delay_row();
        self.delay_row_registered = true;

        // Player-facing "SKIP RESULTS ON FAST EXIT" row on the Mods tab
        // (default ON = today's instant cut to song select). Optional
        // tier: a registration failure just keeps the default.
        register_skip_results_option();

        let id = input_manager::on_input_event(Arc::new(|event: &InputEvent| {
            on_input_event(event);
        }));
        self.input_cb_id = Some(id);

        // Each fresh gameplay starts a clean song: clear the per-song
        // quick-fail taint so a stale fail from the previous song can't
        // suppress this song's score submission.
        if scene_manager::is_available() {
            let id = scene_manager::on_scene_change(Box::new(|prev, next| {
                if next == scene::GAMEPLAY && prev != scene::GAMEPLAY {
                    score_guard::reset_song_taint();
                }
            }));
            self.scene_cb_id = Some(id);
        }

        let fast = if SEQUENCE_FINISH.load(Ordering::Acquire).is_null()
            || SHUTTER_GLOBAL.load(Ordering::Acquire).is_null()
        {
            "natural fail flow only"
        } else {
            "bannerless fast paths armed"
        };
        log_info!(
            "QuickRestartOrFail: enabled (press 1 = restart, press 3 = fail; {})",
            fast
        );
    }

    fn disable(&mut self) {
        if let Some(id) = self.input_cb_id.take() {
            input_manager::remove_callback(id);
        }
        if let Some(id) = self.scene_cb_id.take() {
            scene_manager::remove_callback(id);
        }
        if self.delay_row_registered {
            mod_menu::remove_rows_for(&[DELAY_ROW_KEY]);
            self.delay_row_registered = false;
        }
        log_info!("QuickRestartOrFail: disabled");
    }
}

fn on_input_event(event: &InputEvent) {
    if event.event_type != InputEventType::Pressed {
        return;
    }

    // Only fire during gameplay.
    if scene_manager::current_scene() != scene::GAMEPLAY {
        return;
    }

    // One press = one action (the FF/RW scrub model — no GestureBuffer).
    // Either side's pinpad triggers; both actions are cabinet-wide. The
    // fail passes the pressing side so their SKIP RESULTS preference
    // governs.
    if event.button == button::NUM_1 {
        trigger_restart();
    } else if event.button == button::NUM_3 {
        trigger_fail(event.player as usize);
    }
}

/// Read the active `DancePlaySequence`'s current `agcs::StackStep` value
/// (the same read the restart init sampler uses: `DPS+0x68` indexed by the
/// u16 at `DPS+0x92`, range-validated). `None` when not in GAMEPLAY, no
/// active child, or any read is out of range — callers must treat `None`
/// conservatively for the decision they're gating.
fn dps_step() -> Option<i32> {
    if scene_manager::current_scene() != scene::GAMEPLAY {
        return None;
    }
    let ts = scene_manager::current_transition_sequence()?;
    unsafe {
        let child = memory::read_ptr(ts.add(ACTIVE_CHILD_OFFSET));
        if child.is_null() {
            return None;
        }
        let idx = *(child.add(DPS_STEP_INDEX) as *const u16) as usize;
        if idx >= 5 {
            return None;
        }
        let s = *(child.add(DPS_STEP_BASE + idx * 8) as *const i32);
        if (0..=15).contains(&s) {
            Some(s)
        } else {
            None
        }
    }
}

/// True when the song provably has NOT started yet (the READY-banner
/// window: DPS in its pre-song init states 0..=6). An unreadable step
/// returns `false` — the mid-song paths keep their long-shipped behavior
/// on any layout surprise; the pre-song special-casing only engages on a
/// positive identification.
fn dps_pre_song() -> bool {
    matches!(dps_step(), Some(s) if s < DPS_STEP_IN_SONG)
}

/// Walks the active TS → DPS → children chain and returns every child
/// whose vtable matches `gameplay_actor_vtable`. Empty when not in
/// gameplay or when the actor tree isn't yet captured.
fn find_gameplay_actors() -> Vec<*mut u8> {
    let mut out = Vec::new();

    let Some(transition_seq) = scene_manager::current_transition_sequence() else {
        return out;
    };
    let target_vtable = GAMEPLAY_ACTOR_VTABLE.load(Ordering::Acquire);
    if target_vtable.is_null() {
        return out;
    }

    unsafe {
        let dps_slot = transition_seq.add(ACTIVE_CHILD_OFFSET) as *const *mut u8;
        let dps = *dps_slot;
        if dps.is_null() {
            return out;
        }

        let mut child = *(dps.add(FIRST_CHILD_OFFSET) as *const *mut u8);
        while !child.is_null() {
            let vtable = *(child as *const *mut u8);
            if vtable == target_vtable {
                out.push(child);
            }
            child = *(child.add(NEXT_SIBLING_OFFSET) as *const *mut u8);
        }
    }

    out
}

/// Force-fail `actor` by mirroring the post-`gauge::DEAD` +
/// post-`gauge::GAME_OVER` state in one shot:
///
/// - Set the death-gate (`+0x2B7`) and death-result (`+0x2B8`) bytes
///   to 1, mimicking what the `gauge::DEAD` handler would write on a
///   hard-gauge configuration.
/// - Set `m_isDead` (`+0x1E8`) to 1 — DPS's STEP_FINISH reads this
///   to pick the FAILED shutter kind and suppress score submission.
/// - Advance `agcs::StackStep`'s active slot to `STEP_GAME_OVER`,
///   which kicks the per-actor fade-out and bubbles up through
///   STEP_END → DPS::STEP_FINISH → STEP_CLOSING → `returnToParent`.
unsafe fn force_game_over(actor: *mut u8) {
    match GPA_LAYOUT.get() {
        Some(layout) => {
            *(actor.add(layout.death_gate)) = 1;
            *(actor.add(layout.death_result)) = 1;
        }
        None => log_warn!(
            "QuickRestartOrFail: GamePlayActor layout underived -- forcing game over without the death-gate/result bytes"
        ),
    }
    *(actor.add(GAMEPLAY_ACTOR_IS_DEAD_OFFSET)) = 1;
    *(actor.add(GAMEPLAY_ACTOR_STEP_OFFSET) as *mut u32) = STEP_GAME_OVER;
}

/// Shared core of both gestures: optionally arm a one-shot scene redirect,
/// then force every active GamePlayActor to STEP_GAME_OVER and let the
/// framework's natural fail flow run.
///
/// REFUSES during the pre-song READY window: forcing STEP_GAME_OVER into
/// mid-init GamePlayActors while DPS is still in its pre-song states
/// (0..=6) soft-locks — DPS never reaches its state-8 banner request and
/// nothing else drains the parked shutter (cabinet-observed 2026-08-31).
/// The gesture triggers gate this themselves; this check is the structural
/// backstop so no future call path can reintroduce the lock.
fn fail_song(redirect_target: Option<i32>, label: &str) {
    if dps_pre_song() {
        log_warn!(
            "QuickRestartOrFail: {} refused -- natural-death fallback is unsafe pre-song (READY window)",
            label
        );
        return;
    }
    let actors = find_gameplay_actors();
    if actors.is_empty() {
        log_warn!(
            "QuickRestartOrFail: no GamePlayActor found -- skipping {}",
            label
        );
        return;
    }
    if let Some(target) = redirect_target {
        scene_manager::add_redirect_once(scene::STAGE_RESULT, target);
    }
    for actor in &actors {
        unsafe { force_game_over(*actor) };
    }
    log_info!(
        "QuickRestartOrFail: {} fired on {} GamePlayActor(s)",
        label,
        actors.len()
    );
}

/// Apply the select-residency patch: rewrite the stage loader's unload
/// imm32 (createNextSequence case 0x1c, `MOV R8D,0x32000`) to `0x30000` so
/// gameplay entry stops evicting the select-music packages (mask `0x2000`).
/// Cabinet-measured: reloading them is ~5 s of the quick-fail latency (and
/// of every natural post-song return to song select — at boot, when the
/// attract chain had left them resident, the same loader took < 1 s).
/// One-way (never unpatched — a mid-session toggle leaving residency
/// behavior changed is harmless), checked (stock bytes verified before the
/// write), fail-open (a miss just keeps the stock 5 s reload).
fn apply_select_residency_patch() {
    if RESIDENCY_PATCHED.load(Ordering::Acquire) {
        return;
    }
    let site = LOADER_MASKS_SITE.load(Ordering::Acquire);
    if site.is_null() {
        return;
    }
    unsafe {
        match memory::apply_checked_patch(
            site.add(LOADER_UNLOAD_IMM_OFFSET),
            &LOADER_UNLOAD_STOCK,
            &LOADER_UNLOAD_PATCHED,
        ) {
            Ok(()) => {
                RESIDENCY_PATCHED.store(true, Ordering::Release);
                log_info!(
                    "QuickRestartOrFail: select-residency patch applied (gameplay entry keeps the select-music packages resident)"
                );
            }
            Err(e) => log_warn!(
                "QuickRestartOrFail: select-residency patch failed ({:?}) -- quick fail will reload the select packages",
                e
            ),
        }
    }
}

/// After a fast restart, drive the incoming `DancePlaySequence` to an
/// instant start: each frame, while it is in its pre-song init states
/// (0..=5), seed the ready-dwell timer (`DPS+0x130`) past the 5.0 s
/// threshold so state 5's gate clears the moment the song bank is prepared
/// (~1.2 s) instead of after the full "READY?" dwell (~5 s). The jacket
/// panel was already dismissed, so there is no visual to preserve.
///
/// Also logs each init-state transition with elapsed time (TEMPORARY
/// instrumentation — kept to confirm the ready-skip lands; strip once
/// validated). DPS init states (from the `FUN_180057ec0` decompile):
/// 0 = wait layout actor, 1 = build note-field actors, 2 = dance_root
/// movie layer, 3 = readiness poll (msg 0x1001), 4 = register song bank
/// (slot 5), 5 = bank-prepare wait + ready-dwell + msg 0x1043, 6 = timing
/// anchor (msg 0x1044), 7 = in-song. `-1` = not in GAMEPLAY yet (loader
/// hop). Runs on the render thread via the `run_on_render_thread`
/// self-requeue; pure reads + the one guarded timer write, range
/// validated; stops at step ≥ 7, on timeout, or when a newer restart's
/// driver supersedes this one.
fn start_restart_init_sampler() {
    if !widget_renderer::is_available() {
        return;
    }
    let gen = SAMPLER_GEN.fetch_add(1, Ordering::AcqRel) + 1;
    log_info!("QuickRestartOrFail[init-sampler]: started (gen {})", gen);
    sample_restart_step(gen, Instant::now(), -100);
}

fn sample_restart_step(gen: usize, started: Instant, last_step: i32) {
    widget_renderer::run_on_render_thread(move || {
        if SAMPLER_GEN.load(Ordering::Acquire) != gen {
            return; // a newer restart's driver took over
        }
        if started.elapsed() > Duration::from_secs(30) {
            log_info!(
                "QuickRestartOrFail[init-sampler]: timeout at step {} after {:.2}s",
                last_step,
                started.elapsed().as_secs_f32()
            );
            return;
        }

        let mut step = -1i32;
        if scene_manager::current_scene() == scene::GAMEPLAY {
            if let Some(ts) = scene_manager::current_transition_sequence() {
                unsafe {
                    let child = memory::read_ptr(ts.add(ACTIVE_CHILD_OFFSET));
                    if !child.is_null() {
                        let idx = *(child.add(DPS_STEP_INDEX) as *const u16) as usize;
                        if idx < 5 {
                            let s = *(child.add(DPS_STEP_BASE + idx * 8) as *const i32);
                            if (0..=15).contains(&s) {
                                step = s;
                                // Ready-dwell skip: clear the state-5 timer
                                // gate while still in pre-song init.
                                if (0..=5).contains(&step) {
                                    memory::write_f32(
                                        child.add(DPS_READY_TIMER_OFFSET) as *mut u8,
                                        DPS_READY_TIMER_SEED,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        if step != last_step {
            log_info!(
                "QuickRestartOrFail[init-sampler]: step {} -> {} at {:.2}s",
                last_step,
                step,
                started.elapsed().as_secs_f32()
            );
        }
        if step >= 7 {
            log_info!(
                "QuickRestartOrFail[init-sampler]: song running at {:.2}s -- done",
                started.elapsed().as_secs_f32()
            );
            return;
        }
        sample_restart_step(gen, started, step);
    });
}

/// Snapshot of the ShutterActor's fast-path-relevant state.
struct ShutterSnapshot {
    actor: *mut u8,
    state: i32,
    active_kind: i32,
    pending_kind: i32,
}

/// Read the ShutterActor's state/kind fields, range-validating every value
/// (a layout drift on a future build must read as "unknown", never as a
/// plausible state). `Ok(None)` = no shutter actor exists (safe: both the
/// loader and the fresh DPS treat a missing shutter as passable).
fn read_shutter() -> Result<Option<ShutterSnapshot>, &'static str> {
    let global = SHUTTER_GLOBAL.load(Ordering::Acquire);
    if global.is_null() {
        return Err("shutter global unresolved");
    }
    unsafe {
        let actor = *(global as *const *mut u8);
        if actor.is_null() {
            return Ok(None);
        }
        let idx = *(actor.add(SHUTTER_STEP_INDEX) as *const u16) as usize;
        if idx >= 5 {
            return Err("step index out of range");
        }
        let state = *(actor.add(SHUTTER_STEP_BASE + idx * 8) as *const i32);
        let Some(layout) = SHUTTER_LAYOUT.get() else {
            return Err("shutter layout underived");
        };
        let active_kind = *(actor.add(layout.active_kind) as *const i32);
        let pending_kind = *(actor.add(layout.pending_kind) as *const i32);
        if !(0..=SHUTTER_STATE_MAX).contains(&state)
            || !(-1..=SHUTTER_STATE_MAX).contains(&active_kind)
            || !(-1..=SHUTTER_STATE_MAX).contains(&pending_kind)
        {
            return Err("state/kind fields out of range");
        }
        Ok(Some(ShutterSnapshot {
            actor,
            state,
            active_kind,
            pending_kind,
        }))
    }
}

/// Ensure the shutter cannot block a `finish`-installed successor:
/// - no shutter actor, or fully idle (state 0, nothing active or pending):
///   nothing to do;
/// - the stage panel active with no pending request — covering at state 4
///   (the READY window), revealing at 5, or parked-revealed at 6 (mid-song):
///   send the game's own bannerless dismiss (msg `0x100c` through the
///   actor's `onMessage`) and verify the synchronous state-7 write took
///   (the handler only checks `active kind == 3`; state 7's drain replays
///   or finishes the out label as needed, so all three entry states land
///   in the same 7→8→0 idle park);
/// - anything else (art-load transitional states 1–3, a banner request
///   already in flight): refuse — the caller falls back.
fn ensure_shutter_dismissed(label: &str) -> bool {
    let shutter = match read_shutter() {
        Ok(s) => s,
        Err(why) => {
            log_warn!("QuickRestartOrFail: shutter read failed ({why}) -- {label} falling back");
            return false;
        }
    };
    let Some(s) = shutter else {
        return true; // no shutter actor at all — nothing can block
    };

    if s.state == SHUTTER_STATE_IDLE && s.active_kind < 0 && s.pending_kind < 0 {
        return true;
    }

    if matches!(
        s.state,
        SHUTTER_STATE_COVERED | SHUTTER_STATE_REVEALING | SHUTTER_STATE_PARKED_REVEALED
    ) && Some(s.active_kind) == SHUTTER_LAYOUT.get().map(|l| l.stage_kind)
        && s.pending_kind < 0
    {
        unsafe {
            let flags = memory::read_u32(s.actor.add(TREE_FLAGS_OFFSET));
            if flags & TREE_FLAGS_DISPATCH_SUPPRESSED != 0 {
                log_warn!(
                    "QuickRestartOrFail: shutter dispatch suppressed (flags 0x{:X}) -- {} falling back",
                    flags,
                    label
                );
                return false;
            }
            // vtable slot +0x18 = agcs::Actor::onMessage. All pointer math in
            // BYTES (`*const u8`); sanity-check the fetched code pointer lies
            // inside gamemdx before calling it (a bad slot must degrade to
            // the fallback, never crash).
            let vtable = *(s.actor as *const *const u8);
            let on_message_addr = *(vtable.add(VTBL_ON_MESSAGE_OFFSET) as *const *const u8);
            let in_module = module_resolver::get_game_module().is_some_and(|m| {
                let base = m.base as usize;
                let addr = on_message_addr as usize;
                addr > base && addr < base + m.size
            });
            if !in_module {
                log_warn!(
                    "QuickRestartOrFail: shutter onMessage ptr {:?} outside gamemdx -- {} falling back",
                    on_message_addr,
                    label
                );
                return false;
            }
            let on_message: OnMessageFn = std::mem::transmute(on_message_addr);
            on_message(s.actor, MSG_SHUTTER_DISMISS_STAGE, std::ptr::null_mut());

            // The dismiss handler writes state 7 synchronously; read it back
            // as proof the build's handler actually took the message.
            let idx = *(s.actor.add(SHUTTER_STEP_INDEX) as *const u16) as usize;
            let state_after = *(s.actor.add(SHUTTER_STEP_BASE + idx * 8) as *const i32);
            if state_after != SHUTTER_STATE_DRAIN_TAIL {
                log_warn!(
                    "QuickRestartOrFail: shutter dismiss not taken (state {} -> {}) -- {} falling back",
                    s.state,
                    state_after,
                    label
                );
                return false;
            }
        }
        log_info!(
            "QuickRestartOrFail: stage shutter dismissed ({} -> 7 drain) for {}",
            s.state,
            label
        );
        unblock_shutter_drain(s.actor);
        return true;
    }

    log_info!(
        "QuickRestartOrFail: shutter not in a fast-path state (state {}, active {}, pending {}) -- {} falling back",
        s.state,
        s.active_kind,
        s.pending_kind,
        label
    );
    false
}

/// Fast-forward the shutter clip past the drain's wait target (the
/// 2026-08-31 early-dismiss limbo fix).
///
/// The drain's state 8 waits on `current_frame >= max(frame("out_end"),
/// frame("end"))`, and the only thing that advances the clip is state 7's
/// `"out"` label play. On shutter art whose `shutter_play` clip carries no
/// labels (observed on a stock-data CrossOver install: `in`, `stage_out`,
/// `ready_out`, `out`, `out_end` all missing — only `end` resolves), that
/// play silently fails, so the clip never moves. Mid-song that's masked —
/// the clip long since sits at its final frame, so the wait passes
/// instantly — but a dismiss in the first seconds of the song finds the
/// clip still near frame 0 and state 8 parks forever (the watchdog-caught
/// `shutter=8` limbo).
///
/// Fix: compute state 8's own target (same label queries the game makes)
/// and SetFrame the clip there. On label-less art this satisfies the wait
/// directly; on healthy art state 7's `"out"` play re-seeks the playhead
/// anyway, so the write is a harmless no-op visually and the natural
/// sub-second out animation still runs. Fail-open: any unresolved id or
/// failed lookup just leaves the drain to its stock behavior.
fn unblock_shutter_drain(actor: *mut u8) {
    unsafe {
        let Some(layout) = SHUTTER_LAYOUT.get() else {
            return;
        };
        let kind = *(actor.add(layout.active_kind) as *const i32);
        if kind != layout.stage_kind || kind < 0 {
            return;
        }
        let layer =
            *(actor.add(SHUTTER_LAYER_TABLE_OFFSET + kind as usize * 0x10) as *const *const u8);
        if layer.is_null() {
            // State 8 treats a missing layer as "drain complete" — nothing
            // to unblock.
            return;
        }
        let mc_id = *(layer.add(SHUTTER_LAYER_MC_ID_OFFSET) as *const u32);
        if mc_id == 0 {
            return;
        }
        let out_end = bm2d_api::mc_frame_by_label(mc_id, c"out_end");
        let end = bm2d_api::mc_frame_by_label(mc_id, c"end");
        let target = out_end.unwrap_or(0).max(end.unwrap_or(0));
        if target == 0 {
            // Both labels absent (or bm2d API unavailable): the wait's
            // threshold is 0, which any current frame satisfies — the
            // drain completes on its own.
            log_info!(
                "QuickRestartOrFail: shutter drain unblock skipped (out_end={:?} end={:?} -- wait target 0)",
                out_end,
                end
            );
            return;
        }
        let ok = bm2d_api::mc_op(mc_id, MC_OP_SET_FRAME, target as i32);
        log_info!(
            "QuickRestartOrFail: shutter clip fast-forwarded to drain target frame {} (out_end={:?} end={:?} ok={})",
            target,
            out_end,
            end,
            ok
        );
    }
}

/// The fast path's shared trigger: gate, dismiss the stage shutter, then/// `finish(DPS, target_1idx)`.
///
/// Gates (any failure returns `false` — caller falls back):
///   1. `sequence_finish` + `shutter_actor_global` resolved.
///   2. A live TransitionSequence with a live, not-dying active child at
///      `TS+0x58` (the `flags & 0x24` dead-mask, quick-logout style).
///   3. The child really is a live DancePlaySequence — proven by finding at
///      least one GamePlayActor among its children (vtable match).
///   4. The stage shutter is idle or successfully dismissed
///      (`ensure_shutter_dismissed`) — the root cause of both 2026-08 limbo
///      attempts (see the module doc).
///
/// `finish` is synchronous and re-enters our scene hook during the call, so
/// NO lock may be held here (score_guard calls completed before this).
fn try_fast_finish(target_1idx: i32, label: &str) -> bool {
    let finish_addr = SEQUENCE_FINISH.load(Ordering::Acquire);
    if finish_addr.is_null() || SHUTTER_GLOBAL.load(Ordering::Acquire).is_null() {
        return false;
    }

    let Some(ts) = scene_manager::current_transition_sequence() else {
        log_warn!("QuickRestartOrFail: no TransitionSequence captured -- {label} falling back");
        return false;
    };

    let (child, flags) = unsafe {
        let child = memory::read_ptr(ts.add(ACTIVE_CHILD_OFFSET)) as *mut u8;
        if child.is_null() {
            (child, 0u32)
        } else {
            (child, memory::read_u32(child.add(TREE_FLAGS_OFFSET)))
        }
    };
    if child.is_null() {
        log_warn!(
            "QuickRestartOrFail: TransitionSequence has no active child -- {label} falling back"
        );
        return false;
    }
    if flags & TREE_FLAGS_DEAD_MASK != 0 {
        log_warn!(
            "QuickRestartOrFail: active child is dying (flags 0x{:X}) -- {} falling back",
            flags,
            label
        );
        return false;
    }

    // Liveness proof: the child at TS+0x58 hosts GamePlayActors, so it is a
    // live DancePlaySequence (not some other sequence behind stale scene
    // tracking).
    let actor_count = find_gameplay_actors().len();
    if actor_count == 0 {
        log_warn!("QuickRestartOrFail: no GamePlayActor found -- {label} falling back");
        return false;
    }

    // The load-bearing step: without this, the finish-installed successor
    // waits forever on the parked stage shutter (both 2026-08 limbos).
    if !ensure_shutter_dismissed(label) {
        return false;
    }

    log_info!(
        "QuickRestartOrFail: {} -- finish({}₁ᵢₙdₑₓ) with {} GamePlayActor(s)",
        label,
        target_1idx,
        actor_count
    );
    let finish: SequenceFinishFn = unsafe { std::mem::transmute(finish_addr) };
    unsafe { finish(child, target_1idx) };
    // Permanent safety net: silently watch the finish-installed loader
    // reach its destination; a limbo (it happened twice: the parked-6
    // shutter in 2026-08, the label-less drain stall in 2026-08-31)
    // self-diagnoses with one gate sample + WARN instead of a mute hang.
    start_finish_watchdog(target_1idx);
    true
}

/// Post-`finish` watchdog: wait (silently) for the destination scene
/// (restart: back at GAMEPLAY with a live DPS; fail: SONG_SELECT). If it
/// has not arrived within 20 s, sample the loader's gate inputs once
/// (shutter state / mgr_loading / bg readiness terms — the RVA-guarded
/// `diag_sample_loader_exit_gate`) and WARN: the sample names the blocked
/// gate so a future limbo is diagnosable from a single log. Pure reads on
/// the render thread via the self-requeue pattern; a newer gesture's
/// watchdog supersedes an older one.
static WATCHDOG_GEN: AtomicUsize = AtomicUsize::new(0);

fn start_finish_watchdog(target_1idx: i32) {
    if !widget_renderer::is_available() {
        return;
    }
    let gen = WATCHDOG_GEN.fetch_add(1, Ordering::AcqRel) + 1;
    watchdog_tick(gen, Instant::now(), target_1idx);
}

fn watchdog_tick(gen: usize, started: Instant, target_1idx: i32) {
    widget_renderer::run_on_render_thread(move || {
        if WATCHDOG_GEN.load(Ordering::Acquire) != gen {
            return;
        }
        let scene = scene_manager::current_scene();
        let arrived = match target_1idx {
            STAGE_LOADER_1IDX => scene == scene::GAMEPLAY && dps_step().is_some(),
            _ => scene == scene::SONG_SELECT,
        };
        if arrived {
            log_info!(
                "QuickRestartOrFail[watchdog]: destination scene {} reached at {:.2}s",
                scene,
                started.elapsed().as_secs_f32()
            );
            return;
        }
        if started.elapsed() > Duration::from_secs(20) {
            diag_sample_loader_exit_gate("watchdog timeout");
            log_warn!(
                "QuickRestartOrFail[watchdog]: loader did NOT complete within 20s (scene {}) -- LIMBO (gate sample above)",
                scene
            );
            return;
        }
        watchdog_tick(gen, started, target_1idx);
    });
}

/// Sample the `LoadingSequence` gate inputs and log them — pure reads plus
/// calls into the game's own read-only readiness predicates, on the frame
/// thread (the same thread the loader evaluates them on). Now fired only
/// by the post-`finish` watchdog on a limbo timeout: the one line names
/// the blocked gate (it identified BOTH historical limbo causes — the
/// parked-6 shutter in 2026-08 and the state-8 drain stall in 2026-08-31).
/// Fires only on the 20260721 build (guarded by the resolved
/// `sequence_finish` RVA — the raw-RVA reads below are build-specific); no
/// game state modified.
fn diag_sample_loader_exit_gate(label: &str) {
    let finish_addr = SEQUENCE_FINISH.load(Ordering::Acquire) as usize;
    if finish_addr == 0 {
        return;
    }
    let Some(module) = module_resolver::get_game_module() else {
        return;
    };
    let base = module.base as usize;
    if finish_addr.wrapping_sub(base) != RVA_SEQUENCE_FINISH_20260721 {
        log_info!(
            "QuickRestartOrFail[diag]: {} -- sample skipped (not the 20260721 build)",
            label
        );
        return;
    }

    unsafe {
        // Shutter state + kinds: the loader's mask-apply gate needs
        // null/0/4; mid-song the stage panel (kind 3) parks at 6.
        let shutter = *((base + RVA_SHUTTER_GLOBAL) as *const *const u8);
        let (shutter_state, shutter_active, shutter_pending) = if shutter.is_null() {
            (-1, -1, -1)
        } else {
            let idx = *(shutter.add(0x82) as *const u16) as usize;
            (
                *(shutter.add(0x58 + idx * 8) as *const i32),
                *(shutter.add(0x310) as *const i32),
                *(shutter.add(0x314) as *const i32),
            )
        };

        // Scene resource manager async-load byte: the loader waits on == 0.
        let mgr = *((base + RVA_SCENE_RES_MGR_GLOBAL) as *const *const u8);
        let mgr_loading = if mgr.is_null() {
            -1i32
        } else {
            *mgr.add(0x24) as i32
        };

        // The exit gate proper. The loader's own guard: singleton null => pass.
        let bg_actor = *((base + RVA_BG_MOVIE_ACTOR_GLOBAL) as *const *const u8);
        if bg_actor.is_null() {
            log_info!(
                "QuickRestartOrFail[diag]: {} -- shutter={}/{}/{} mgr_loading={} bg_actor=null (exit gate passes)",
                label,
                shutter_state,
                shutter_active,
                shutter_pending,
                mgr_loading
            );
            return;
        }
        let bg_ready_fn: unsafe extern "C" fn() -> u8 =
            std::mem::transmute(base + RVA_BG_SYSTEM_READY_FN);
        let gate = bg_ready_fn();

        // Per-term breakdown (same subterms the gate evaluates).
        let bg_obj = *(bg_actor.add(0x58) as *const *mut u8);
        let frame = *(bg_actor.add(0x68) as *const *const u8);
        let frame_ready = if frame.is_null() {
            -1i32
        } else {
            *frame.add(0xc0) as i32
        };
        let (switch_ready, map1_ready, map2_ready) = if bg_obj.is_null() {
            (1u8, 1u8, 1u8)
        } else {
            let switch_fn: unsafe extern "C" fn(*mut u8) -> u8 =
                std::mem::transmute(base + RVA_BG_SWITCH_READY_FN);
            let map_fn: unsafe extern "C" fn(*mut u8) -> u8 =
                std::mem::transmute(base + RVA_BG_MAP_READY_FN);
            (
                switch_fn(bg_obj.add(0x150)),
                map_fn(bg_obj.add(0x348)),
                map_fn(bg_obj.add(0x3f0)),
            )
        };
        log_info!(
            "QuickRestartOrFail[diag]: {} -- loader exit gate={} (shutter={}/{}/{} mgr_loading={} bg_obj={:?} switch_ready={} maps_ready=({},{}) frame={:?} frame_ready={})",
            label,
            gate,
            shutter_state,
            shutter_active,
            shutter_pending,
            mgr_loading,
            bg_obj,
            switch_ready,
            map1_ready,
            map2_ready,
            frame,
            frame_ready
        );
    }
}

/// The skip-results session-over guard, gating BOTH fail shapes (the fast
/// select-loader `finish` and the fallback 29 → 24 redirect): each skips
/// `ResultSequence` — the game's only session-over decision (`+0xE8`,
/// docs/quick_logout_research.md §5.2) — so the skip must PROVE the
/// session would have continued. Conservative: every condition must hold and
/// every read must succeed, else the natural flow (which ends the session
/// properly, incl. the final-stage game-over) runs instead.
///
/// Session continues after this song iff (non-course, non-event):
///   - no final-stage override armed (`GameWork+0x10 == -1`; stock code only
///     ever writes -1, but a "make this my last stage" mod would set it), and
///   - the 0-based stage counter is strictly below the last normal stage
///     index (`max_stage` — normal stage count is `max_stage + 1`). The
///     final normal stage (where the extra-stage grant lives) and the extra
///     stage both fall back.
fn session_continues_after_results_skip() -> bool {
    let Some(event) = stage_records::event_mode() else {
        log_info!("QuickRestartOrFail: session state unavailable -- quick-fail taking the full natural tail");
        return false;
    };
    let (Some(override_stage), Some(stage), Some(max_stage)) = (
        stage_records::final_stage_override(),
        stage_records::stage_counter(),
        stage_records::max_stage_setting(),
    ) else {
        log_info!("QuickRestartOrFail: session state unavailable -- quick-fail taking the full natural tail");
        return false;
    };

    let continues = event == 0
        && override_stage == -1
        && (0..=MAX_SANE_STAGE).contains(&stage)
        && (0..=MAX_SANE_STAGE).contains(&max_stage)
        && stage < max_stage;
    if !continues {
        log_info!(
            "QuickRestartOrFail: session may end after this song (stage {}, max {}, override {}, event {}) -- quick-fail taking the full natural tail",
            stage,
            max_stage,
            override_stage,
            event
        );
    }
    continues
}

/// `presser_side` = the side (0/1) whose pinpad pressed 3; their
/// `skip_results_fast_exit` preference governs (the fail itself is
/// cabinet-wide either way).
fn trigger_fail(presser_side: usize) {
    // READY-banner window (DPS pre-song init, before the arrows scroll):
    // the natural-death fallback is UNSAFE here — forcing STEP_GAME_OVER
    // into mid-init GamePlayActors leaves DPS parked in a pre-song state
    // that never consults the death flags, a soft lock (cabinet-observed
    // 2026-08-31). Only the fast `finish` is taken: the shutter dismiss
    // handles the covering/revealing panel (states 4/5) the same way as
    // the mid-song park. Any refusal ⇒ the gesture is IGNORED (never the
    // fallback). The taint is set only on success — a refused pre-song
    // gesture must not suppress the score of the song about to play (and
    // `reset_song_taint` would collaterally clear training taints).
    // SKIP RESULTS is moot pre-song (there is no score to show), so the
    // fast exit is taken regardless of the pressing side's preference.
    if dps_pre_song() {
        let session_continues = !is_course() && session_continues_after_results_skip();
        if session_continues && try_fast_finish(SELECT_LOADER_1IDX, "quick-fail (pre-song fast)") {
            score_guard::set_quick_fail();
            return;
        }
        log_info!(
            "QuickRestartOrFail: quick-fail ignored during the pre-song READY window (no safe exit path)"
        );
        return;
    }

    // A quick failure fails out both sides at once, so the resulting play is
    // incomplete for everyone. Taint the score guard so neither side's score is
    // submitted for this song (and the session's logout save is sanitised too).
    score_guard::set_quick_fail();

    // SKIP RESULTS ON FAST EXIT = OFF for the pressing side: take the
    // natural fail flow with NO redirect — fade + FAILED banner + the
    // stage results screen (score breakdown up to the drop-out) + the
    // game's own natural tail (which decides session continuation, so no
    // predicate is needed). This is the proven predicate-fail fallback
    // path; see the module doc for why a direct results-loader `finish`
    // was rejected.
    if !SKIP_RESULTS
        .get(presser_side)
        .map(|s| s.load(Ordering::Acquire))
        .unwrap_or(true)
    {
        fail_song(None, "quick-fail (show results)");
        return;
    }

    // Both fail shapes skip ResultSequence — the game's only session-over
    // decision — so both require the session-continues predicate. Courses
    // and any session that might end after this song keep the full natural
    // tail (the game must run its own game-over/session-end there).
    let session_continues = !is_course() && session_continues_after_results_skip();

    // Fast path: dismiss the parked stage shutter, then finish(DPS, 0x19)
    // straight into the 0-idx 24 song-select loader. No fade, no FAILED
    // banner, no results screen; getNextID(0x19) = 0x1A lands on song
    // select with no redirect needed.
    if session_continues && try_fast_finish(SELECT_LOADER_1IDX, "quick-fail (fast)") {
        return;
    }

    // Fallback: natural fail flow (fade + FAILED banner), with a one-shot
    // 29 → 24 redirect to still skip the results screen when it's provably
    // safe AND the m_currentID repair is available (without it the tail
    // after the redirected scene runs the wrong successor).
    if session_continues && scene_manager::redirect_repair_available() {
        fail_song(
            Some(scene::CAUTION_TO_SONG_INTERSTITIAL),
            "quick-fail (skip results)",
        );
    } else {
        fail_song(None, "quick-fail (full natural tail)");
    }
}

fn trigger_restart() {
    if is_course() {
        log_info!("QuickRestartOrFail: restart blocked (course mode)");
        return;
    }
    // READY-banner window: the song hasn't started, so a "restart" is a
    // no-op semantically — and every restart shape is unsafe here (the
    // in-place reset refuses pre-song by its own gates, a mid-init fresh-DPS
    // `finish` reload is unvalidated, and the natural-death fallback
    // soft-locks — cabinet-observed for quick-fail 2026-08-31, same path).
    if dps_pre_song() {
        log_info!(
            "QuickRestartOrFail: restart ignored during the pre-song READY window (song not started)"
        );
        return;
    }
    // A press while a previous restart's in-place reset is still in
    // flight is DROPPED (the FF/RW scrub's `reset_in_flight` precedent):
    // `request_reset` would refuse and the press would otherwise escalate
    // into the fresh-DPS fast path — a full reload for a double-tap.
    if song_reset::reset_in_flight() {
        log_debug!("QuickRestartOrFail: restart dropped -- a reset is already in flight");
        return;
    }
    // Restarting discards the current attempt, so clear the per-song quick-fail
    // taint: an honest replay must be allowed to submit its score.
    score_guard::reset_song_taint();

    // Fastest path: in-place rewind — no scene transition, no loader, no
    // actor teardown. The service gates itself (DPS in-song, all actors at
    // the in-song step, non-course, gauge snapshot captured) and refuses
    // cleanly when any gate fails; a refusal falls through to the shipped
    // fresh-DPS fast path below. If the reset STARTS but cannot complete
    // (cue never prepared / player died during the silence window), the
    // recovery callback forces the natural-death restart — the song is
    // already stopped at that point, so scene-jumping is the safe recovery.
    //
    // `quick_restart.restart_delay_ms` (mod-config.json + the RESTART
    // DELAY overlay row, default 0 = instant): countdown between the
    // gesture and the restarted song's start — the field still resets
    // immediately (silence, notes back on their pre-song approach),
    // giving players a beat to get back in position.
    let delay_ms = RESTART_DELAY_MS.load(Ordering::Acquire);

    // Training Mode (Step 2): a set A marker turns the restart into
    // restart-from-A — the same in-place reset seeking to A behind the
    // silent approach lead (the operator's restart delay composes as the
    // lead's minimum: a delay ≥ the training lead keeps their calibrated
    // repositioning window exactly). A refused seek falls through to the
    // shipped restart-at-0 ladder below (R6/design §6).
    if let Some(a_ms) = crate::mods::training_mode::active_section_start() {
        let lead_ms =
            i32::try_from(crate::mods::training_mode::TRAINING_LEAD_MS).unwrap_or(i32::MAX);
        let lead_ms = lead_ms.max(delay_ms);
        match song_reset::request_reset(
            a_ms,
            lead_ms,
            song_reset::AccumulatorPolicy::Zero,
            Some(restart_reset_recovery),
        ) {
            song_reset::ResetOutcome::Started => {
                log_info!(
                    "QuickRestartOrFail: restart-from-A (seek to {} ms, lead {} ms)",
                    a_ms,
                    lead_ms
                );
                return;
            }
            song_reset::ResetOutcome::Refused | song_reset::ResetOutcome::Unsupported => {
                log_warn!(
                    "QuickRestartOrFail: restart-from-A refused (A = {} ms) -- falling back to restart at 0",
                    a_ms
                );
            }
        }
    }

    match song_reset::request_reset(
        0,
        delay_ms,
        song_reset::AccumulatorPolicy::Zero,
        Some(restart_reset_recovery),
    ) {
        song_reset::ResetOutcome::Started => {
            log_info!(
                "QuickRestartOrFail: quick-restart (in-place reset, delay {} ms)",
                delay_ms
            );
            return;
        }
        song_reset::ResetOutcome::Refused | song_reset::ResetOutcome::Unsupported => {}
    }

    // Fast path: dismiss the parked stage shutter, then finish(DPS, 0x1C)
    // into the 0-idx 27 stage loader; getNextID(0x1C) = 0x1D builds a fresh
    // DancePlaySequence — same song from the top, stage counter untouched,
    // no fade, no banner, no READY panel (the fresh DPS accepts the idle
    // shutter). No predicate needed: the shipped 29→28 redirect has skipped
    // results with identical semantics since 2026-05.
    if try_fast_finish(STAGE_LOADER_1IDX, "quick-restart (fast)") {
        // Drive the fresh DPS to an instant start (skip the ~5 s "READY?"
        // ready-dwell) + log the init-phase breakdown.
        start_restart_init_sampler();
        return;
    }

    // Fallback: natural fail flow (fade + FAILED banner) with the one-shot
    // 29 → 28 redirect. Cabinet-proven.
    fail_song(Some(scene::GAMEPLAY), "quick-restart (fallback)");
}

/// Recovery for a started-but-failed in-place reset: the song cue is
/// stopped and nothing else was touched, so force the natural-death
/// restart (fade + FAILED banner + the one-shot 29 → 28 redirect). Runs
/// on the frame thread from the song_reset driver.
fn restart_reset_recovery() {
    log_warn!("QuickRestartOrFail: in-place reset recovery -- forcing natural-death restart");
    fail_song(Some(scene::GAMEPLAY), "quick-restart (reset recovery)");
}

/// Returns true if the current gameplay is a multi-song course (Dan, etc.).
/// Checks DPS+0x98 (m_courseMaxStage) — values > 1 indicate a course.
fn is_course() -> bool {
    let actors = find_gameplay_actors();
    let Some(&actor) = actors.first() else {
        return false;
    };
    unsafe {
        let dps = *(actor.add(0x08) as *const *const u8);
        if dps.is_null() {
            return false;
        }
        let course_max_stage = *(dps.add(0x98) as *const i32);
        course_max_stage > 1
    }
}
