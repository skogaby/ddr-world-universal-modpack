//! Assist Tick Mod — a clap at each arrow's chart timestamp, as an audible
//! timing reference (the DDR World equivalent of StepMania's assist tick).
//!
//! Ticks are **chart-time driven, never judgment driven**: the game's judgment
//! fires on player input or late-window expiry, so a judgment-driven clap
//! would follow the player's mistakes — the opposite of a timing reference.
//! The tick timestamps come from the game's **own** per-note records: the
//! dispatched actor's Results vector carries one 0x40-byte entry per note
//! with an authoritative millisecond timestamp, complete at the first judge
//! dispatch of a song. (The SSQ chart file is deliberately not consulted —
//! the repo's chart-timing helper has a suspected TPS normalization defect,
//! and the engine's records are already normalized.)
//!
//! ## The pre-mixed tick track
//!
//! Ticks are delivered as ONE continuous mono waveform per song with every
//! clap mixed at its exact sample position, played once as a single cue
//! through the game's own XACT engine (FR-1). Clap spacing is therefore
//! sample-exact by construction — the per-tick trigger path this replaces
//! stacked two quantizers (frame-boundary detection ±½ frame + the engine's
//! ~10 ms packet-grid cue starts, RE-proven to have no sample-accurate start
//! primitive) into audible 8th/16th-burst jitter. Riding the same engine as
//! the music means the same mixer clock: zero within-song drift, and only a
//! constant per-song start offset remains.
//!
//! Per song: the first judge dispatch builds the tick list (unchanged FR-2
//! predicate + coalescing) and reads the per-song-latched cabinet
//! `SOUND_OFFSET` (actor `+0x16c`); the first *chosen-side* dispatch latches
//! the anchor `m0` (plus the committed song-rate snapshot) and hands off to a
//! background synthesis thread (mix + MS-ADPCM encode, pure CPU — NFR-1); a
//! later chosen-side dispatch commits the result ON THE GAME THREAD:
//! register the immortal tick bank (once per process), stop, rewrite the
//! wave's sample bytes **shifted by the wall-converted `mc − m0`** whole
//! ADPCM blocks, and play. The shift replaces the engine's
//! `Play(timeOffset)` — live-refuted as a seek (it only fast-forwards the
//! cue event timeline; an already-due wave starts at sample 0) — and unifies
//! normal start, late start, and clock-rewind re-anchoring into one code
//! path: content already in the past simply shifts out of the track.
//!
//! Each clap lands at the **judgement moment** of its row (FR-3, rate-aware
//! per the song-rate streaming design's req 30):
//! `wall_ms(i) = content_to_wall_ms(t_i + SIGN·JT − m0) − SOUND_OFFSET` —
//! `music_count`'s baseline is `beginTick = tick − soundOffset`, so
//! subtracting the cabinet's declared audio latency puts the clap where the
//! game itself thinks the step lands, to the precision of the cabinet's
//! calibration. The conversion rides the AwaitAnchor-latched committed
//! `RateRatio` (identity/uncommitted reproduces the legacy arithmetic
//! bit-identically); the domain algebra lives host-tested in
//! `services::song_rate::tick_domain`. There is deliberately NO latency knob
//! (FR-4): a persisted legacy `assist_tick.offset_ms` is ignored with one
//! INFO, never reinterpreted.
//!
//! No fallback path (FR-6): boot-time prerequisite failure (clap asset,
//! services) fails `init()` and the mod never appears; a per-song synthesis
//! or playback failure yields a silent song and one WARN.
//!
//! ## Score suppression (training design §4.7/R5 — deliberate behavior change)
//!
//! A side that plays a song with ASSIST TICK enabled is assisted play: its
//! per-stage score save is **suppressed** and its card-out logout save is
//! sanitised (profile persists, scores stripped), exactly like Autoplay.
//! The taint is level-written into `score_guard` for both sides at every
//! GAMEPLAY entry, mirroring the per-song latch — the parent toggle OFF the
//! next song reads clean again.
//!
//! Design: `.agents/planning/20260729-assist-tick-premixed-track/design/detailed-design.md`
//! (incl. the 2026-07-29 block-shift amendment). RE record: that feature's
//! `research/` + `docs/xact_audio_research.md`.

use once_cell::sync::Lazy;
use std::ffi::CStr;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, Ordering};
use std::sync::{Arc, Mutex};

use crate::mods::config;
use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::custom_options::{self, RegisterSpec, ScalarFormat, ShowWhen};
use crate::services::game_audio::{self, TickBankHandle, TickBankRequest};
use crate::services::judge_hook::{self, CallbackHandle, Priority};
use crate::services::scene_manager;
use crate::services::score_guard;
use crate::services::se_bank_synth;
use crate::services::song_rate::clock_patch::{self, RateSnapshot};
use crate::services::song_rate::tick_domain;
use crate::services::song_reset;
use crate::types::game_note::{actor_results_range, for_each_result, kind, state, GameNote};
use crate::types::scenes::scene;
use crate::{log_debug, log_info, log_warn};

/// The tick bank's cue, by internal name — must equal
/// [`se_bank_synth::BANK_NAME`] (asserted at init). NOT `asti`: the engine
/// pairs banks globally by internal name, and `asti` is the retired shipped
/// bank's name (a collision cross-paired the banks on the first live test).
const CUE: &CStr = c"astk";
/// A drop in the chosen side's judge clock larger than this means a mid-song
/// rewind rather than jitter (the clock is milliseconds and rises
/// monotonically within a song). Belt-and-braces: the scene callback is the
/// primary reset; this guard catches any path that rewinds the clock without
/// a scene transition, and re-anchors the playing track via a re-shifted
/// rewrite (FR-7).
const REWIND_MS: i32 = 1_000;
/// Offset of the play-side enum (0=left/P1, 1=right/P2) within the gameplay
/// actor struct. Same documented constant `autoplay` uses. NOTE: the claim
/// that doubles reads 0 here is UNVERIFIED (a doubles session started from
/// the P2 reader may plausibly read 1) — which is why doubles is detected by
/// the style field, never by this one.
const ACTOR_PLAY_SIDE_OFFSET: usize = 0x84;
/// Offset of the play-style **int enum** on the gameplay actor; `1` = DOUBLE
/// (the engine reads it three independent times to switch per-panel loops
/// between 4 and 8). It is NOT a pointer — a prior research note's
/// `ACTOR_SESSION_OFFSET = 0x88` session-struct chain is wrong; never
/// dereference this field.
const ACTOR_PLAY_STYLE_OFFSET: usize = 0x88;
/// Play-style value meaning doubles.
const STYLE_DOUBLE: i32 = 1;
/// Offset of the per-song-latched cabinet `SOUND_OFFSET` (i32, ms) on the
/// gameplay actor — the game's own declaration of the audio chain's latency
/// (`music_count` baseline = `tick − soundOffset`). Verified in the
/// timing-offsets RE (`20260626-timing-offsets/research/r3-field-semantics.md`).
const ACTOR_SOUND_OFFSET: usize = 0x16c;
/// Offset of the parent `DancePlaySequence` pointer on the gameplay actor.
const ACTOR_PARENT_OFFSET: usize = 0x08;
/// Offset of the first-child pointer on an actor/sequence node.
const FIRST_CHILD_OFFSET: usize = 0x18;
/// Offset of the next-sibling pointer on an actor/sequence node.
const NEXT_SIBLING_OFFSET: usize = 0x10;
/// Upper bound on the sibling walk. A DancePlaySequence has a handful of
/// children; anything past this means a corrupted `+0x10` chain, and the walk
/// must not loop unbounded inside the judge dispatch.
const MAX_SIBLING_WALK: usize = 64;
/// `SongState::tick_side` value meaning "no side latched / song inert".
const SIDE_NONE: i32 = -1;
/// How many of a song's leading timestamps the once-per-song diagnostic
/// prints. Enough to eyeball monotonicity and the lead-in cutoff.
const LOG_FIRST_TIMESTAMPS: usize = 8;
/// Coalescing window (FR-2): timestamps closer together than this collapse to
/// one tick. Jumps are already one record (one timestamp), and exact dedup
/// catches identical values — this catches charts authored at TPS 150, whose
/// millisecond rounding can place two adjacent rows on the same or *adjacent*
/// millisecond.
const COALESCE_MS: i32 = 4;
/// The per-player option row's id (unchanged): kbin wire element
/// `mod_assist_tick`, label texture `seop_item_assist_tick`, JSON cache key
/// `custom_options.p1/p2.assist_tick`.
const OPT_ID: &str = "assist_tick";

// ── TICK EFFECT VOLUME (the per-player gain child row) ───────────────

/// The volume child row's id: kbin wire element `mod_assist_tick_volume`,
/// label texture `seop_item_assist_tick_volume`, JSON cache key
/// `custom_options.p1/p2.assist_tick_volume`. Shown only while this side's
/// ASSIST TICK is ON (`ShowWhen::Equals` on [`OPT_ID`]).
const OPT_VOLUME_ID: &str = "assist_tick_volume";
/// Linear-amplitude percentage bounds and steps — deliberately identical to
/// the SONG PLAYBACK SPEED row's scroll semantics (design R2). The 25 floor
/// means the row cannot fully mute; the parent toggle is the mute.
const VOLUME_MIN: i32 = 25;
const VOLUME_MAX: i32 = 175;
const VOLUME_STEP: i32 = 5;
const VOLUME_COARSE: i32 = 10;
/// Exact unity: at 100 the synthesis path must not touch the samples at all
/// (design R4 — a default-valued row yields a byte-identical track).
const VOLUME_DEFAULT: i32 = 100;

// ── JUDGMENT TIMING (FR-3's per-side term) ───────────────────────────

/// Sign applied to the tick side's JUDGMENT TIMING in the content formula
/// (`content_ms = t_i + SIGN·JT − SOUND_OFFSET − m0`). `+1` — validated by
/// ear on the cabinet (2026-07-29, 10×-amplified direction test): a positive
/// `timing_music` moves the judgement moment (and therefore the clap) LATER.
const JUDGMENT_TIMING_SIGN: i32 = 1;
/// Offset of the embedded `ddr::player::Option` within a side's context
/// object (`*(table[side]) + 0xE0` — the game's own accessor shape,
/// `FUN_1801e7530` on 20260324).
const CTX_OPTION_OFFSET: usize = 0xE0;
/// Offset of JUDGMENT TIMING (`timing_music`, ±100 ms) within the Option.
const OPTION_TIMING_MUSIC: usize = 0x24;
/// Sanity bounds on the value read — the game's UI clamps to ±100 ms;
/// anything outside means the chain read garbage and must not be trusted.
const JUDGMENT_TIMING_LIMIT: i32 = 100;

/// The derived per-side context table (`player_option_table`), stashed at
/// init. Never null after a successful init — the derivation is a required
/// signature (FR-6/NFR-4: missing ⇒ the mod never appears).
static PLAYER_OPTION_TABLE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
/// Latches the "chain read failed" warning (once per session).
static JT_READ_WARNED: AtomicBool = AtomicBool::new(false);

/// Read the tick side's JUDGMENT TIMING (ms) at song-build time (options are
/// locked during gameplay, so a per-song read is a latch). Fail-soft to 0 —
/// a null link or an out-of-range value means the claps mark the objective
/// beat for this song instead of the player's judge bias, warned once.
fn judgment_timing_ms(side: i32) -> i32 {
    let table = PLAYER_OPTION_TABLE.load(Ordering::Acquire);
    if table.is_null() {
        return 0;
    }
    let side = side.clamp(0, 1) as usize;
    unsafe {
        let holder = *(table.add(side * 8) as *const *const u8);
        if holder.is_null() {
            return warn_jt_unreadable("ctx-table entry is null", side);
        }
        let ctx = *(holder as *const *const u8);
        if ctx.is_null() {
            return warn_jt_unreadable("side context is null", side);
        }
        let value = *(ctx.add(CTX_OPTION_OFFSET + OPTION_TIMING_MUSIC) as *const i32);
        if !(-JUDGMENT_TIMING_LIMIT..=JUDGMENT_TIMING_LIMIT).contains(&value) {
            return warn_jt_unreadable("value outside +/-100 ms", side);
        }
        value
    }
}

fn warn_jt_unreadable(why: &str, side: usize) -> i32 {
    if !JT_READ_WARNED.swap(true, Ordering::Relaxed) {
        log_warn!(
            "AssistTick: JUDGMENT TIMING unreadable for side {} ({}) -- using 0 for this song (warned once)",
            side,
            why
        );
    }
    0
}

// ── Module state ─────────────────────────────────────────────────────
// The judge callback is a plain `fn` (the dispatcher's callbacks cannot
// capture), so everything lives in module statics.

/// The clap PCM, read once in `init()` (file IO belongs on the init thread)
/// and shared with every song's synthesis thread. `None` never occurs after
/// a successful init — a missing/short asset fails the mod (FR-6).
static CLAP: Mutex<Option<Arc<Vec<i16>>>> = Mutex::new(None);
/// The registered tick bank, stashed at the first successful commit and
/// consumed by every later one. One per process (NFR-2).
static TICK_BANK: Mutex<Option<TickBankHandle>> = Mutex::new(None);
/// `GamePlayActor`'s vtable address (RTTI-resolved `gameplay_actor_vtable`
/// signature), stashed at init for the sibling walk's identity test. Null =
/// unresolved → the walk is unavailable and side selection runs degraded.
/// Deliberately NOT in `required_signatures()` — a missing vtable degrades
/// side selection instead of disabling the mod (D11 keeps the shipped
/// semantics; FR-6's hard-fail stance applies to the NEW prerequisites).
static GAMEPLAY_ACTOR_VTABLE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
/// Latches the degraded-mode warning so a whole session in degraded mode
/// yields exactly one WARN, not one per song.
static DEGRADED_WARNED: AtomicBool = AtomicBool::new(false);
/// Latches the per-session synthesis/playback failure warnings (NFR-3).
static SYNTH_FAILURE_WARNED: AtomicBool = AtomicBool::new(false);
static COMMIT_FAILURE_WARNED: AtomicBool = AtomicBool::new(false);

/// Live per-side option values, written ONLY by [`on_option_change`] — which
/// fires on the init thread, the render thread, the ess save/load hook's
/// thread and a spawned JSON-prime background thread, so atomics are the only
/// legal store. Consumed by the gameplay-entry latch, never by the judge path
/// directly (mid-session changes apply next song).
static ASSIST_TICK_ENABLED: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
/// Per-song latch of the enables, taken at GAMEPLAY entry. The choose step
/// and the degraded-mode refinement read these for the whole song.
static LATCHED_ENABLED: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];

/// Live per-side TICK EFFECT VOLUME values (percent), written ONLY by
/// [`on_volume_change`] — same multi-thread callers as
/// [`ASSIST_TICK_ENABLED`], so atomics. Consumed by the gameplay-entry
/// latch, never by the judge path directly.
static TICK_VOLUME: [AtomicI32; 2] = [
    AtomicI32::new(VOLUME_DEFAULT),
    AtomicI32::new(VOLUME_DEFAULT),
];
/// Per-song latch of the volumes, taken at GAMEPLAY entry beside
/// [`LATCHED_ENABLED`]. The song build reads the CHOSEN side's entry;
/// mid-session changes apply next song, and quick-restart's in-place reset
/// keeps the latch (same song, same latch).
static LATCHED_VOLUME: [AtomicI32; 2] = [
    AtomicI32::new(VOLUME_DEFAULT),
    AtomicI32::new(VOLUME_DEFAULT),
];

/// Where a song is in the build → synthesize → play pipeline. All
/// transitions happen under the [`SONG`] lock; game calls happen only with
/// it released.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    /// No tick track this song (option off, no usable chart, or a failure).
    Idle,
    /// Tick list built; waiting for the first CHOSEN-side dispatch to latch
    /// the anchor `m0` (the dispatch that triggered the build may belong to
    /// the other side, whose clock must not anchor this side's track).
    AwaitAnchor,
    /// Background synthesis in flight for `SongState::generation`.
    Building,
    /// Encoded track ready; the next chosen-side dispatch commits it.
    Ready,
    /// The track is playing (or was committed — a failed play stays Idle).
    Playing,
}

/// Per-song state. Touched from the game thread (judge + scene callbacks)
/// and — for the `Building → Ready` hand-off — the synthesis thread, so a
/// plain mutex with no lock held across any game call is the right shape.
struct SongState {
    /// The side whose chart drives the ticks; [`SIDE_NONE`] = none chosen or
    /// the song is inert. Kept for logging; the per-dispatch identity check
    /// is [`Self::tick_actor`].
    tick_side: i32,
    /// The chosen actor's address (`0` = none/inert). Stored as `usize` so
    /// `SongState` stays `Send`; only ever compared, never dereferenced
    /// after the rebuild.
    tick_actor: usize,
    /// Monotonic song token: the synthesis thread deposits its result only
    /// if this still matches, so a stale thread from a quick-restarted song
    /// can never install the wrong track.
    generation: u32,
    /// Sorted, coalesced note timestamps (engine ms), held between the list
    /// build and the anchor latch (then moved into the synthesis thread).
    times: Vec<i32>,
    /// The chosen actor's per-song `SOUND_OFFSET` (read at build).
    sound_offset: i32,
    /// The chosen side's JUDGMENT TIMING (read at build — options are locked
    /// during gameplay, so this is a per-song latch).
    judgment_timing: i32,
    /// The chosen side's TICK EFFECT VOLUME percent (read at build from
    /// [`LATCHED_VOLUME`]). Baked into the synthesized track — constant for
    /// the whole song, including rewind re-anchors (which reuse `encoded`).
    volume_percent: i32,
    /// The anchor: the chosen side's `music_count` at synthesis hand-off.
    /// Content is authored relative to it; the commit shifts by the
    /// wall-converted `mc − m0`.
    m0: i32,
    /// The song-rate snapshot latched at the anchor hand-off (strictly after
    /// any loader-thread commit lands — the first judge dispatch). The
    /// synthesis positions and the commit/rewind skips convert through THIS
    /// copy: the live publication resets to identity at gameplay exit and
    /// must never be re-read later in the song.
    rate: RateSnapshot,
    phase: Phase,
    /// The encoded track, retained after commit so a rewind re-anchor can
    /// re-shift from it without resynthesis. `Arc` so the game calls can
    /// borrow it with the lock released.
    encoded: Option<Arc<Vec<u8>>>,
    /// The chosen side's previous `music_count`; `i32::MIN` = no frame seen
    /// yet this song. Rewind guard only.
    last_music_count: i32,
    /// The latest in-place reset's content target (the `on_song_reset`
    /// notification's `t_ms`) — the clap floor: notes BEFORE it were
    /// rebuilt consumed-neutral and must not clap, so every commit mutes
    /// the track region before the target's clap position (the loop's
    /// silent approach lead stays clap-free). `None` = no reset this song
    /// (fresh-song commits mute nothing). Overwritten by each reset,
    /// kept across commits (the latest reset governs the rest of the run).
    reset_floor_ms: Option<i32>,
    /// Set at gameplay entry; consumed by the first judge dispatch of the
    /// song, which is the earliest moment the actor (and its Results vector)
    /// exists — scene callbacks fire *before* the next scene is constructed.
    rebuild_pending: bool,
}

impl SongState {
    const fn empty() -> Self {
        Self {
            tick_side: SIDE_NONE,
            tick_actor: 0,
            generation: 0,
            times: Vec::new(),
            sound_offset: 0,
            judgment_timing: 0,
            volume_percent: VOLUME_DEFAULT,
            m0: 0,
            rate: RateSnapshot::IDENTITY,
            phase: Phase::Idle,
            encoded: None,
            last_music_count: i32::MIN,
            reset_floor_ms: None,
            rebuild_pending: false,
        }
    }

    /// Reset everything except the generation counter (which must keep
    /// climbing so in-flight synthesis threads stay invalidated).
    fn clear(&mut self) {
        let generation = self.generation.wrapping_add(1);
        *self = Self::empty();
        self.generation = generation;
    }
}

static SONG: Lazy<Mutex<SongState>> = Lazy::new(|| Mutex::new(SongState::empty()));

// ── Scene wiring ─────────────────────────────────────────────────────

/// Gameplay entry/exit. Entry can re-fire without a results screen (quick
/// restart installs a `STAGE_RESULT → GAMEPLAY` redirect), so song state is
/// reset on **entry**, not only on exit. No actor exists yet on entry —
/// which is exactly why the tick list is built on the first judge dispatch
/// instead of here. Both transitions stop any playing track (the scene hook
/// dispatches on the game thread).
fn on_scene_change(prev: i32, next: i32) {
    if next == scene::GAMEPLAY {
        // Latch each side's option value for the whole song BEFORE arming
        // the rebuild, so the choice at the first judge dispatch sees one
        // consistent snapshot. Mid-session changes apply next song.
        for side in 0..2usize {
            let enabled = ASSIST_TICK_ENABLED[side].load(Ordering::Acquire);
            LATCHED_ENABLED[side].store(enabled, Ordering::Release);
            // Score containment (training design §4.7/R5, a deliberate
            // behavior change): a side that plays with claps enabled must
            // not submit its score. Level-written — true AND false every
            // song, mirroring the latch (the autoplay model), so no reset
            // ordering exists to race: `score_guard::reset_song_taint`
            // never touches this flag.
            score_guard::set_assist_tick_taint(side, enabled);
            LATCHED_VOLUME[side]
                .store(TICK_VOLUME[side].load(Ordering::Acquire), Ordering::Release);
        }
        stop_track_if_any("gameplay entry");
        if let Ok(mut song) = SONG.lock() {
            song.clear();
            song.rebuild_pending = true;
        }
        log_info!("AssistTick: gameplay entry -- song state armed for rebuild");
    } else if prev == scene::GAMEPLAY {
        stop_track_if_any("gameplay exit");
        if let Ok(mut song) = SONG.lock() {
            song.clear();
        }
        log_info!("AssistTick: gameplay exit -- song state cleared");
    }
}

/// Stop the tick cue if a bank exists (D8: immediate stop on every gameplay
/// boundary, always before any later rewrite). No-op when nothing plays —
/// `SoundBank::Stop` on an idle cue returns success.
fn stop_track_if_any(why: &str) {
    let handle = TICK_BANK.lock().ok().and_then(|g| *g);
    if let Some(h) = handle {
        let ok = game_audio::stop_cue(&h, CUE);
        log_debug!("AssistTick: stop_cue ({}) -> {}", why, ok);
    }
}

// ── Judge wiring — the per-song state machine ────────────────────────

/// Judge pre-callback (`Priority::Normal`). Invoked once per frame **per
/// side** during gameplay. The body is O(1) in the steady state (a phase
/// check under the lock); the build, anchor, and commit steps each fire once
/// per song. Panic-free: the dispatcher `catch_unwind`s each subscriber, but
/// this body must not rely on that.
fn tick_clock(actor: *mut u8, music_count: i32) {
    if actor.is_null() {
        return;
    }

    // Rebuild decision under the lock; the build itself runs with the lock
    // released (it walks game structures but calls no engine function).
    let rebuild = match SONG.lock() {
        Ok(mut song) => {
            let pending = song.rebuild_pending;
            if pending {
                song.rebuild_pending = false;
            }
            pending
        }
        Err(_) => return,
    };
    if rebuild {
        rebuild_for(actor);
    }

    // Everything below is chosen-side only: m0, the rewind guard, and the
    // commit must all read ONE side's clock so the anchor arithmetic
    // (the wall-converted `skip = mc − m0`) can never mix two sides'
    // counters.
    enum Action {
        None,
        /// Latch m0 and spawn synthesis for (generation, times, sound_offset,
        /// judgment_timing, m0, rate, volume_percent).
        Anchor(u32, Vec<i32>, i32, i32, i32, RateSnapshot, i32),
        /// Commit the ready track with this skip (wall ms since m0) and
        /// clap-floor mute bound (track ms; None = no mute).
        Commit(Arc<Vec<u8>>, i32, Option<i32>),
        /// Re-anchor after a clock rewind: stop, re-shift, re-play.
        Rewind(Arc<Vec<u8>>, i32, Option<i32>),
    }
    /// The reset clap floor as a track position: the FIRST legitimate clap
    /// after the latest reset target — computed with the synthesis
    /// formula itself (`tick_track_positions` on the one-element target),
    /// so the mute boundary matches the authored positions exactly. A
    /// note AT the target judges (the rebuild consumes strictly-before
    /// only) and its clap survives the mute.
    fn clap_floor_track_ms(song: &SongState) -> Option<i32> {
        let target = song.reset_floor_ms?;
        tick_domain::tick_track_positions(
            &[target],
            JUDGMENT_TIMING_SIGN * song.judgment_timing,
            song.sound_offset,
            song.m0,
            &song.rate,
        )
        .first()
        .copied()
    }
    let action = match SONG.lock() {
        Ok(mut song) => {
            if song.tick_actor == 0 || actor as usize != song.tick_actor {
                Action::None
            } else {
                let prev = song.last_music_count;
                song.last_music_count = music_count;
                match song.phase {
                    Phase::AwaitAnchor => {
                        // Latch the committed song-rate snapshot for the
                        // whole song (design req 30): the anchor fires
                        // strictly after any loader-thread commit lands
                        // (first judge dispatch), and the live publication
                        // resets to identity at gameplay exit — this copy is
                        // what the synthesis positions and the commit/rewind
                        // skips convert through. One lock-free seqlock read.
                        song.rate = clock_patch::snapshot();
                        song.m0 = music_count;
                        song.phase = Phase::Building;
                        let times = std::mem::take(&mut song.times);
                        Action::Anchor(
                            song.generation,
                            times,
                            song.sound_offset,
                            song.judgment_timing,
                            music_count,
                            song.rate,
                            song.volume_percent,
                        )
                    }
                    Phase::Ready => match song.encoded.clone() {
                        Some(encoded) => {
                            let skip =
                                tick_domain::restart_skip_ms(music_count, song.m0, &song.rate);
                            if skip < 0 {
                                // The clock is still behind the anchor (a
                                // future-dated delayed restart — quick
                                // restart's countdown, the training lead):
                                // the track's first byte IS count m0, so
                                // wait for the count to reach it and commit
                                // exactly. On the fresh-song path skip is
                                // always >= 0 here (the count only rises
                                // past m0 while synthesis runs).
                                Action::None
                            } else {
                                let floor = clap_floor_track_ms(&song);
                                song.phase = Phase::Playing;
                                Action::Commit(encoded, skip, floor)
                            }
                        }
                        None => {
                            // Unreachable by construction; fail the song
                            // rather than spin.
                            song.phase = Phase::Idle;
                            Action::None
                        }
                    },
                    Phase::Playing
                        if prev != i32::MIN && music_count < prev.saturating_sub(REWIND_MS) =>
                    {
                        match song.encoded.clone() {
                            Some(encoded) => Action::Rewind(
                                encoded,
                                tick_domain::restart_skip_ms(music_count, song.m0, &song.rate),
                                clap_floor_track_ms(&song),
                            ),
                            None => Action::None,
                        }
                    }
                    _ => Action::None,
                }
            }
        }
        Err(_) => return,
    };

    // Game/engine calls with the lock released (repo norm).
    match action {
        Action::None => {}
        Action::Anchor(generation, times, sound_offset, judgment_timing, m0, rate, volume) => {
            spawn_synthesis(
                generation,
                times,
                sound_offset,
                judgment_timing,
                m0,
                rate,
                volume,
            );
        }
        Action::Commit(encoded, elapsed_ms, floor) => {
            commit_track(&encoded, elapsed_ms, floor, "commit");
        }
        Action::Rewind(encoded, elapsed_ms, floor) => {
            log_info!(
                "AssistTick: clock rewind detected -- re-anchoring at {} ms",
                elapsed_ms
            );
            commit_track(&encoded, elapsed_ms, floor, "rewind re-anchor");
        }
    }
}

/// Latch-and-synthesize hand-off: compute each tick's track (wall) position
/// (FR-3, rate-aware) and mix + encode on a background thread (NFR-1 — never
/// on the judge dispatch). The result is deposited back into [`SONG`] iff
/// the song generation still matches.
fn spawn_synthesis(
    generation: u32,
    times: Vec<i32>,
    sound_offset: i32,
    judgment_timing: i32,
    m0: i32,
    rate: RateSnapshot,
    volume_percent: i32,
) {
    let clap = match CLAP.lock() {
        Ok(guard) => guard.clone(),
        Err(_) => None,
    };
    let Some(clap) = clap else {
        // Impossible after a successful init; fail the song, once.
        if !SYNTH_FAILURE_WARNED.swap(true, Ordering::Relaxed) {
            log_warn!("AssistTick: clap PCM unavailable at synthesis -- song silent");
        }
        if let Ok(mut song) = SONG.lock() {
            if song.generation == generation {
                song.phase = Phase::Idle;
            }
        }
        return;
    };

    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(move || {
            let started = std::time::Instant::now();
            // FR-3 + song-rate req 30: each clap's track position is the
            // WALL moment of its row's judgement —
            // content_to_wall_ms(t_i + SIGN·JT − m0) − SOUND_OFFSET, with
            // the AwaitAnchor-latched committed ratio supplying the
            // conversion (identity/uncommitted reproduces the legacy
            // arithmetic bit-identically — host-pinned in tick_domain).
            // The anchor m0 cancels against the commit's converted
            // `mc − m0` shift, so the claps land at judgement moments
            // regardless of when the track actually starts.
            let track_ms = tick_domain::tick_track_positions(
                &times,
                JUDGMENT_TIMING_SIGN * judgment_timing,
                sound_offset,
                m0,
                &rate,
            );
            // TICK EFFECT VOLUME: linear-amplitude pre-scale of the clap,
            // baked into this song's track. The identity path is a hard
            // requirement (design R4): at 100 the samples are NOT touched,
            // so a default-valued row yields a byte-identical track.
            let synth = if volume_percent == VOLUME_DEFAULT {
                se_bank_synth::synthesize_track(&clap, &track_ms)
            } else {
                let scaled = se_bank_synth::scale_pcm(&clap, volume_percent);
                se_bank_synth::synthesize_track(&scaled, &track_ms)
            };
            (synth, started.elapsed().as_millis())
        });
        match result {
            Ok((synth, elapsed_ms)) => {
                log_info!(
                    "AssistTick: synthesis done -- m0={} sound_offset={} judgment_timing={} rate={}% volume={}% mixed={} clipped={} dropped={} in {} ms",
                    m0,
                    sound_offset,
                    judgment_timing,
                    rate.requested_percent,
                    volume_percent,
                    synth.mixed,
                    synth.clipped,
                    synth.dropped,
                    elapsed_ms
                );
                // FR-8: ticks past the fixed tick-track capacity
                // (TICK_CAPACITY_MS, 1200 s wall — D15) are truncated —
                // audible as a clapless tail on over-length charts
                // (courses). Named loudly, once per affected song.
                if synth.dropped > 0 {
                    log_warn!(
                        "AssistTick: chart exceeds the {} s tick-track capacity -- {} tick(s) past the cap dropped (FR-8 truncation; the song's tail plays without claps)",
                        se_bank_synth::TICK_CAPACITY_MS / 1000,
                        synth.dropped
                    );
                }
                if let Ok(mut song) = SONG.lock() {
                    if song.generation == generation && song.phase == Phase::Building {
                        song.encoded = Some(Arc::new(synth.encoded));
                        song.phase = Phase::Ready;
                    }
                    // else: the song moved on (restart/exit) — drop silently.
                }
            }
            Err(_) => {
                if !SYNTH_FAILURE_WARNED.swap(true, Ordering::Relaxed) {
                    log_warn!("AssistTick: synthesis thread panicked -- song silent (warned once)");
                }
                if let Ok(mut song) = SONG.lock() {
                    if song.generation == generation {
                        song.phase = Phase::Idle;
                        song.encoded = None;
                    }
                }
            }
        }
    });
}

/// Commit the encoded track ON THE GAME THREAD: ensure the bank exists
/// (registered once per process), stop any playing cue (paranoia — the
/// buffer must be quiescent before the rewrite), rewrite shifted by the
/// elapsed anchor time, and play. A failure clears the song to Idle with one
/// WARN per session (FR-6: silent song, no fallback).
///
/// `mute_until_track_ms` is the reset clap floor as a track position (the
/// first legitimate post-reset clap): served content BEFORE it is replaced
/// with encoded silence, so claps for consumed pre-target notes never sound
/// during the loop's silent approach lead. `None` = no mute (fresh song).
fn commit_track(
    encoded: &Arc<Vec<u8>>,
    elapsed_ms: i32,
    mute_until_track_ms: Option<i32>,
    what: &str,
) {
    let handle = ensure_tick_bank_registered();
    let Some(h) = handle else {
        // register_tick_bank already warned (once, latched).
        if let Ok(mut song) = SONG.lock() {
            song.phase = Phase::Idle;
            song.encoded = None;
        }
        return;
    };

    let _ = game_audio::stop_cue(&h, CUE);
    let skip_bytes = se_bank_synth::shift_bytes_for_ms(elapsed_ms.max(0));
    // Track bytes before the clap floor are served muted: [skip, floor)
    // relative to the buffer start = [0, floor_bytes − skip_bytes). Both
    // sides come from the same block-rounding helper, so the difference
    // stays block-aligned; a floor at/behind the skip mutes nothing (the
    // scrub case — its skip already starts past the floor).
    let mute_head_bytes = mute_until_track_ms
        .map(|floor| se_bank_synth::shift_bytes_for_ms(floor).saturating_sub(skip_bytes))
        .unwrap_or(0);
    let rewrote = game_audio::rewrite_tick_wave(&h, encoded, skip_bytes, mute_head_bytes);
    let played = rewrote && game_audio::play_tick_track(&h, CUE);
    log_info!(
        "AssistTick: {} -- shift {} ms ({} bytes, mute head {} bytes), rewrite -> {}, play -> {}",
        what,
        elapsed_ms,
        skip_bytes,
        mute_head_bytes,
        rewrote,
        played
    );
    if !played {
        if !COMMIT_FAILURE_WARNED.swap(true, Ordering::Relaxed) {
            log_warn!(
                "AssistTick: track {} failed -- song silent (warned once)",
                what
            );
        }
        if let Ok(mut song) = SONG.lock() {
            song.phase = Phase::Idle;
            song.encoded = None;
        }
    }
}

/// Build the tick containers and register the immortal bank on first use.
/// GAME THREAD ONLY — reached from the judge dispatch, which is itself proof
/// that gameplay state is live. Idempotent (and `register_tick_bank` latches
/// its own permanent failures).
fn ensure_tick_bank_registered() -> Option<TickBankHandle> {
    if let Ok(guard) = TICK_BANK.lock() {
        if let Some(h) = *guard {
            return Some(h);
        }
    }
    let c = se_bank_synth::build_tick_containers();
    let handle = game_audio::register_tick_bank(TickBankRequest {
        name: se_bank_synth::BANK_NAME,
        xwb: c.xwb_bytes,
        xsb: c.xsb_bytes,
        sample_seg_offset: c.sample_seg_offset,
        sample_seg_len: c.sample_seg_len,
    });
    if let (Some(h), Ok(mut guard)) = (handle, TICK_BANK.lock()) {
        *guard = Some(h);
    }
    handle
}

// ── Side selection (unchanged from the shipped mod, D11) ─────────────

/// Side of a gameplay actor, normalized the way `autoplay` does: any value
/// outside `0..=1` is treated as side 0.
fn actor_side(actor: *mut u8) -> i32 {
    let side = unsafe { *(actor.add(ACTOR_PLAY_SIDE_OFFSET) as *const i32) };
    if side == 1 {
        1
    } else {
        0
    }
}

/// Play style of a gameplay actor ([`STYLE_DOUBLE`] = doubles). An int enum —
/// never a pointer (see [`ACTOR_PLAY_STYLE_OFFSET`]).
fn actor_style(actor: *mut u8) -> i32 {
    unsafe { *(actor.add(ACTOR_PLAY_STYLE_OFFSET) as *const i32) }
}

/// One live gameplay actor, as seen by the sibling walk. The address is a
/// `usize` for the same never-dereferenced-after-rebuild reason as
/// `SongState::tick_actor` (choice and list build happen inside the rebuild,
/// where the pointers are live).
#[derive(Clone, Copy)]
struct ActorInfo {
    actor: usize,
    side: i32,
    style: i32,
}

/// Enumerate the live gameplay actors by walking the sibling chain from the
/// dispatched actor's parent `DancePlaySequence`, matching children by raw
/// vtable compare. The engine itself walks this exact `+0x18`/`+0x10` chain
/// one call earlier in the same frame (its per-frame `0x1045` broadcast), so
/// the list is provably live and complete at the first judge dispatch.
///
/// Returns `None` when the walk cannot be trusted — vtable unresolved, null
/// parent, or the walked list not containing the dispatched actor itself
/// (the containment check is the cheap end-to-end validation of the chain's
/// layout assumptions). The caller degrades to the dispatched actor.
fn enumerate_actors(dispatched: *mut u8) -> Option<Vec<ActorInfo>> {
    let target_vtable = GAMEPLAY_ACTOR_VTABLE.load(Ordering::Acquire);
    if target_vtable.is_null() {
        return None;
    }
    let mut out: Vec<ActorInfo> = Vec::new();
    let mut contains_dispatched = false;
    unsafe {
        let dps = *(dispatched.add(ACTOR_PARENT_OFFSET) as *const *mut u8);
        if dps.is_null() {
            return None;
        }
        let mut child = *(dps.add(FIRST_CHILD_OFFSET) as *const *mut u8);
        let mut steps = 0usize;
        while !child.is_null() && steps < MAX_SIBLING_WALK {
            let vtable = *(child as *const *mut u8);
            if vtable == target_vtable {
                out.push(ActorInfo {
                    actor: child as usize,
                    side: actor_side(child),
                    style: actor_style(child),
                });
                if child == dispatched {
                    contains_dispatched = true;
                }
            }
            child = *(child.add(NEXT_SIBLING_OFFSET) as *const *mut u8);
            steps += 1;
        }
    }
    if !contains_dispatched {
        return None;
    }
    Some(out)
}

/// FR-5, gated by the per-song latched enables:
///
/// | Enabled actors | Conclusion | Choice |
/// |---|---|---|
/// | 0 | nobody wants ticks | `None` — the song is inert |
/// | 1 | solo / doubles / 2P-with-one-side-on | that actor |
/// | 2 | 2-player versus, both on | side 0 |
///
/// Doubles is gated on the actor's own `+0x84` side (never assumed to be 0).
/// Candidates are ordered by the side FIELD, never by list position —
/// child-list order is undocumented and plausibly reverse-of-creation.
fn choose_actor(actors: &[ActorInfo], dispatched: ActorInfo) -> Option<ActorInfo> {
    let enabled: Vec<ActorInfo> = actors
        .iter()
        .copied()
        .filter(|a| latched_enabled(a.side))
        .collect();
    if enabled.is_empty() {
        return None;
    }
    if enabled.len() >= 2 {
        if enabled.iter().any(|a| a.style == STYLE_DOUBLE) {
            log_warn!(
                "AssistTick: {} actors with a doubles style present -- impossible by construction; treating as 2P",
                enabled.len()
            );
        }
        return Some(
            enabled
                .iter()
                .copied()
                .min_by_key(|a| a.side)
                .unwrap_or(dispatched),
        );
    }
    enabled.first().copied()
}

/// This side's option value as latched at gameplay entry.
fn latched_enabled(side: i32) -> bool {
    LATCHED_ENABLED
        .get(side.clamp(0, 1) as usize)
        .map(|a| a.load(Ordering::Acquire))
        .unwrap_or(false)
}

/// Change callback for the `ASSIST TICK` option row. Fires on several threads
/// (init, render, the ess save/load hook, a background JSON-prime thread), so
/// the body is an atomic store and nothing else — a panic here would
/// permanently no-op the callback.
fn on_option_change(player_side: u8, new_value: i32) {
    if let Some(slot) = ASSIST_TICK_ENABLED.get(player_side as usize) {
        slot.store(new_value != 0, Ordering::Release);
    }
}

/// Clamp to `[VOLUME_MIN, VOLUME_MAX]` and snap to the nearest
/// [`VOLUME_STEP`]. The persistence load transform (a stale server value or
/// a hand-edited JSON cache entry must land on a legal scroll stop) and a
/// defensive pass in [`on_volume_change`]. Same math as the song-speed row's
/// `snap_rate_percent` — deliberately a private copy: the two ranges are
/// semantically unrelated and must be free to diverge.
fn normalize_volume(v: i32) -> i32 {
    let clamped = v.clamp(VOLUME_MIN, VOLUME_MAX);
    let snapped = ((clamped + VOLUME_STEP / 2) / VOLUME_STEP) * VOLUME_STEP;
    snapped.clamp(VOLUME_MIN, VOLUME_MAX)
}

/// Change callback for the `TICK EFFECT VOLUME (%)` child row. Same
/// multi-thread callers and same atomic-store-only body as
/// [`on_option_change`].
fn on_volume_change(player_side: u8, new_value: i32) {
    if let Some(slot) = TICK_VOLUME.get(player_side as usize) {
        slot.store(normalize_volume(new_value), Ordering::Release);
    }
}

/// This side's TICK EFFECT VOLUME as latched at gameplay entry.
fn latched_volume(side: i32) -> i32 {
    LATCHED_VOLUME
        .get(side.clamp(0, 1) as usize)
        .map(|a| a.load(Ordering::Acquire))
        .unwrap_or(VOLUME_DEFAULT)
}

/// Register the `TICK EFFECT VOLUME (%)` child row. Called only after the
/// parent row is known registered (both the fresh and `Duplicate` arms —
/// `ShowWhen` rejects an unknown parent), and additionally gated on the
/// scalar-row machinery: bool rows don't need the scalar donor, so the
/// parent can exist while this row can't. Every failure path is fail-open
/// to unity volume (design R8) — the atomics keep their 100 default and the
/// synthesis identity path leaves the track untouched.
fn register_volume_row() {
    if !custom_options::row_injection_available() {
        log_warn!(
            "AssistTick: scalar row machinery unavailable -- TICK EFFECT VOLUME row absent, ticks at unity volume"
        );
        return;
    }
    let spec = RegisterSpec::scalar(
        OPT_VOLUME_ID,
        VOLUME_MIN,
        VOLUME_MAX,
        VOLUME_STEP,
        ScalarFormat::Unit { unit: "%" },
    )
    .display_name("Tick Effect Volume")
    .description("Loudness of the assist tick clap")
    .step_coarse(VOLUME_COARSE)
    .default_value(VOLUME_DEFAULT)
    .show_when(ShowWhen::Equals {
        parent_id: OPT_ID.into(),
        value: 1,
    })
    .persist_transform(|_id, v| v, |_id, v| normalize_volume(v))
    .on_change(on_volume_change);
    match custom_options::register_option(spec) {
        Ok(_handle) => {
            log_info!("AssistTick: registered TICK EFFECT VOLUME option under ASSIST TICK");
        }
        Err(custom_options::RegisterError::Duplicate { .. }) => {
            // Re-enable: reseed the live atomics (the duplicate path does
            // not re-fire on_change).
            for side in 0..2u8 {
                on_volume_change(
                    side,
                    custom_options::get_value(side, OPT_VOLUME_ID).unwrap_or(VOLUME_DEFAULT),
                );
            }
        }
        Err(e) => {
            log_warn!("AssistTick: volume row registration failed: {e} -- ticks at unity volume");
        }
    }
}

/// First judge dispatch of a song: choose the tick side from the live actor
/// set (FR-5), build the tick list from the **chosen** actor's Results
/// vector — which may not be the dispatched actor; the sibling walk is what
/// makes the other side's actor reachable — and read the chosen actor's
/// per-song `SOUND_OFFSET`. The anchor (`m0`) and the synthesis hand-off
/// happen on the chosen side's next dispatch (possibly this same one). Logs
/// exactly once per song.
fn rebuild_for(actor: *mut u8) {
    let dispatched = ActorInfo {
        actor: actor as usize,
        side: actor_side(actor),
        style: actor_style(actor),
    };
    let (actors, degraded) = match enumerate_actors(actor) {
        Some(list) => (list, false),
        None => {
            // Degraded mode: walk unavailable or untrustworthy. Latch the
            // dispatched actor; the only misbehaviour is "in 2P we may
            // follow P2's chart" — audible but benign, and this one WARN
            // explains it.
            if !DEGRADED_WARNED.swap(true, Ordering::Relaxed) {
                log_warn!(
                    "AssistTick: actor enumeration unavailable (vtable unresolved, null parent, or walk failed validation) -- degraded to the dispatched actor"
                );
            }
            // Refinement: in degraded mode only the dispatched actor is
            // visible, so if ITS side is disabled, leave the rebuild armed
            // instead of latching — the other side's actor can then claim it
            // on its own dispatch.
            if !latched_enabled(dispatched.side) {
                if let Ok(mut song) = SONG.lock() {
                    song.rebuild_pending = true;
                }
                return;
            }
            (vec![dispatched], true)
        }
    };
    let Some(chosen) = choose_actor(&actors, dispatched) else {
        // No participating side has the option on: the song is inert, and
        // the list is deliberately not even built.
        log_info!(
            "AssistTick: no participating side has ASSIST TICK on (sides={:?}) -- song inert",
            actors.iter().map(|a| a.side).collect::<Vec<i32>>()
        );
        if let Ok(mut song) = SONG.lock() {
            song.tick_side = SIDE_NONE;
            song.tick_actor = 0;
            song.phase = Phase::Idle;
        }
        return;
    };
    let side = chosen.side;

    let (stats, times) = build_tick_list(chosen.actor as *mut u8);
    let kept = times.len();
    let sound_offset =
        unsafe { *((chosen.actor as *const u8).add(ACTOR_SOUND_OFFSET) as *const i32) };
    let judgment_timing = judgment_timing_ms(side);
    let volume_percent = latched_volume(side);

    let sides: Vec<i32> = actors.iter().map(|a| a.side).collect();
    let styles: Vec<i32> = actors.iter().map(|a| a.style).collect();

    if kept == 0 {
        // Empty, misaligned or reversed Results range (the walk helper
        // refuses malformed ranges), or a chart with no eligible rows —
        // the song is inert, once, no crash.
        log_warn!(
            "AssistTick: no usable timestamps (side={} results={}) -- song inert",
            side,
            stats.results
        );
        if let Ok(mut song) = SONG.lock() {
            song.tick_side = SIDE_NONE;
            song.tick_actor = 0;
            song.phase = Phase::Idle;
        }
        return;
    }

    let first: Vec<i32> = times.iter().take(LOG_FIRST_TIMESTAMPS).copied().collect();
    log_info!(
        "AssistTick: song build -- dispatched={:p} siblings={}{} sides={:?} styles={:?} chosen_side={} results={} kept={} rej_kind={} rej_shock={} rej_panel={} rej_neg={} coalesced={} sound_offset={} judgment_timing={} volume={} first={:?}",
        actor,
        actors.len(),
        if degraded { " (DEGRADED)" } else { "" },
        sides,
        styles,
        side,
        stats.results,
        kept,
        stats.rej_kind,
        stats.rej_shock,
        stats.rej_no_panel,
        stats.rej_negative,
        stats.coalesced,
        sound_offset,
        judgment_timing,
        volume_percent,
        first
    );
    if let Ok(mut song) = SONG.lock() {
        song.tick_side = side;
        song.tick_actor = chosen.actor;
        song.times = times;
        song.sound_offset = sound_offset;
        song.judgment_timing = judgment_timing;
        song.volume_percent = volume_percent;
        song.phase = Phase::AwaitAnchor;
        song.encoded = None;
        song.last_music_count = i32::MIN;
    }
}

// ── Tick-list build (unchanged FR-2 predicate + coalescing) ──────────

/// Per-song list-build statistics for the once-per-song diagnostic. The
/// counts partition the Results vector exactly:
/// `results == kept + rej_kind + rej_shock + rej_no_panel + rej_negative
/// + coalesced` (dedup of *identical* timestamps is folded into `coalesced`,
/// since exact duplicates are the degenerate zero-distance case of the
/// coalescing window).
#[derive(Default)]
struct BuildStats {
    results: usize,
    rej_kind: usize,
    rej_shock: usize,
    rej_no_panel: usize,
    rej_negative: usize,
    coalesced: usize,
}

/// FR-2: tick iff this note record is a vanilla step row the player is
/// expected to hit — normal taps, jumps (one record ⇒ one tick), and freeze
/// heads. Excluded: freeze tails, shock arrows, modifier-suppressed notes,
/// mod-injected note types (mines), tempo/event markers, and pre-chart notes.
///
/// **`length[]` (per-panel freeze length, +0x3C) is deliberately NOT
/// consulted.** That is the finding, not an omission: a freeze head is an
/// ordinary `kind == 0` tap the player steps on, so it ticks like any other
/// row, and the freeze *tail* is already excluded by kind (it is `kind == 2`).
/// Reading `length[]` would also make this predicate wrong under the
/// `FREEZE ARROW: OFF` player modifier, which zeroes that array while leaving
/// the (still steppable) head in place. Do not "improve" this by reading it.
///
/// Returned as `Ok(timestamp)` or `Err(reason)` so the caller can count each
/// rejection exactly once for the reconciliation diagnostic.
///
/// # Safety
/// `note` must point at a live 0x60-byte note record; the walk helper has
/// already null-checked it. Every branch is a pure read of the record.
unsafe fn should_tick(note: *const GameNote) -> Result<i32, RejectReason> {
    let n = &*note;

    // 1. Vanilla step rows only. This single whitelist test excludes freeze
    //    TAILS (kind 2), modifier-suppressed THINOUT notes (kind 1),
    //    tempo/event markers (kind < 0) and every mod-injected kind, present
    //    and future (MINE = 20, and whatever NoteTypeRegistry adds next).
    if n.kind != kind::ARROW {
        return Err(RejectReason::Kind);
    }

    // 2. Shock arrows: the engine's own discriminator, verbatim
    //    (step::Note::isShock). An OR over the two 4-panel groups that never
    //    consults the actor's side — which is what makes it correct for a
    //    1P-side actor, a 2P-side actor and doubles alike. A shock must be
    //    *avoided*, so a "step here" cue would be actively misleading.
    let low_all_trg = n.state[0] == state::TRG
        && n.state[1] == state::TRG
        && n.state[2] == state::TRG
        && n.state[3] == state::TRG;
    let high_all_trg = n.state[4] == state::TRG
        && n.state[5] == state::TRG
        && n.state[6] == state::TRG
        && n.state[7] == state::TRG;
    if low_all_trg || high_all_trg {
        return Err(RejectReason::Shock);
    }

    // 3. There must be an arrow on some panel. Analyze's trim pass already
    //    guarantees this for kind == 0, so it is a cheap invariant guard, not
    //    a filter. `!= 0` (not `== TRG`) matches what the renderer draws.
    if !n.state.iter().any(|&s| s != 0) {
        return Err(RejectReason::NoPanel);
    }

    // 4. Notes before the chart start are auto-credited by the engine at
    //    Results build time and can never be played.
    if n.music_count < 0 {
        return Err(RejectReason::Negative);
    }

    Ok(n.music_count)
}

/// Why [`should_tick`] rejected a note — one variant per predicate branch, so
/// the per-song counts partition the vector exactly.
enum RejectReason {
    Kind,
    Shock,
    NoPanel,
    Negative,
}

/// Walk the actor's Results vector, keep every note [`should_tick`] accepts,
/// then sort and coalesce: any timestamp within [`COALESCE_MS`] of the
/// previously kept one collapses into it (the earlier survives). Exact
/// duplicates are the zero-distance case of the same rule. Returns the stats
/// and the final list.
fn build_tick_list(actor: *mut u8) -> (BuildStats, Vec<i32>) {
    let mut stats = BuildStats::default();
    let mut times: Vec<i32> = Vec::new();
    unsafe {
        let (begin, end) = actor_results_range(actor);
        for_each_result(begin, end, |_entry, note| {
            stats.results += 1;
            match should_tick(note) {
                Ok(music_count) => times.push(music_count),
                Err(RejectReason::Kind) => stats.rej_kind += 1,
                Err(RejectReason::Shock) => stats.rej_shock += 1,
                Err(RejectReason::NoPanel) => stats.rej_no_panel += 1,
                Err(RejectReason::Negative) => stats.rej_negative += 1,
            }
        });
    }
    times.sort_unstable();
    let before = times.len();
    let mut last_kept: Option<i32> = None;
    times.retain(|&t| match last_kept {
        Some(prev) if t - prev < COALESCE_MS => false,
        _ => {
            last_kept = Some(t);
            true
        }
    });
    stats.coalesced = before - times.len();
    (stats, times)
}

// ── The Mod ──────────────────────────────────────────────────────────

pub struct AssistTickMod {
    scene_cb_id: Option<usize>,
    reset_cb_id: Option<usize>,
    judge_handle: Option<CallbackHandle>,
}

impl AssistTickMod {
    pub fn new() -> Self {
        Self {
            scene_cb_id: None,
            reset_cb_id: None,
            judge_handle: None,
        }
    }
}

impl Mod for AssistTickMod {
    fn id(&self) -> &str {
        "assist-tick"
    }
    fn name(&self) -> &str {
        "Assist Tick"
    }
    fn description(&self) -> &str {
        "Pre-mixed clap track at each arrow's chart timestamp"
    }
    fn required_signatures(&self) -> &[&str] {
        // The JUDGMENT TIMING chain (FR-6/NFR-4: derivation missing ⇒ the
        // mod never appears — deliberately NOT a degraded mode). The other
        // prerequisites are services, gated in init(): game_audio owns the
        // audio signatures, judge_hook owns judge_notes.
        &["player_option_table"]
    }

    fn init(&mut self, _ctx: &ModContext) -> bool {
        debug_assert_eq!(
            CUE.to_bytes(),
            se_bank_synth::BANK_NAME.as_bytes(),
            "the cue name must equal the bank's internal name"
        );

        // Opportunistic (NOT in required_signatures): the sibling walk's
        // identity test. Unresolved → the walk stays unavailable and side
        // selection runs degraded, warned once at the first rebuild.
        if let Some(vt) = _ctx.signatures.get_address("gameplay_actor_vtable") {
            GAMEPLAY_ACTOR_VTABLE.store(vt as *mut u8, Ordering::Release);
        }

        // The JUDGMENT TIMING table (required — the registry already refused
        // to init us if it was missing, so this read cannot fail here).
        match _ctx.signatures.get_address("player_option_table") {
            Some(table) => PLAYER_OPTION_TABLE.store(table as *mut u8, Ordering::Release),
            None => {
                log_warn!("AssistTick: player_option_table unresolved -- mod disabled");
                return false;
            }
        }

        // Service prerequisites (FR-6: any missing ⇒ the mod never appears).
        // game_audio::is_available() is necessary but NOT sufficient — it
        // means "addresses resolved" and does not imply the XACT engine
        // module is loaded (that is COM-instantiated later, during the
        // game's onBoot). A wrong engine surfaces at the first registration
        // as one declined registration, not here.
        let ga = game_audio::is_available();
        let jh = judge_hook::is_available();
        let sm = scene_manager::is_available();
        if !(ga && jh && sm) {
            log_warn!(
                "AssistTick: missing prerequisites (game_audio={} judge_hook={} scene_manager={}) -- mod disabled",
                ga,
                jh,
                sm
            );
            return false;
        }

        // The clap asset (FR-6 prerequisite). File IO belongs here (init
        // thread), never on a per-frame path; the loader WARNs with the
        // reason on any failure.
        let Some(clap) = se_bank_synth::load_clap_pcm() else {
            log_warn!("AssistTick: clap PCM asset unavailable -- mod disabled");
            return false;
        };
        if let Ok(mut guard) = CLAP.lock() {
            *guard = Some(Arc::new(clap));
        }
        true
    }

    fn enable(&mut self) {
        self.judge_handle = judge_hook::register_pre(Priority::Normal, tick_clock);
        if self.judge_handle.is_none() {
            log_warn!("AssistTick: judge dispatcher unavailable -- mod inactive");
            return;
        }
        self.scene_cb_id = Some(scene_manager::on_scene_change(Box::new(|prev, next| {
            on_scene_change(prev, next);
        })));

        // In-place song reset / seek (quick restart's instant path, the
        // training loop's iterations, FF/RW scrubs): the clock re-anchors
        // with NO scene transition. The encoded track stays VALID — it is
        // authored against the content timeline relative to `m0`, and a
        // reset moves only the wall anchor — so a committed track is
        // demoted to `Ready` and the next judge dispatch re-commits it
        // shifted to the LIVE count (the shipped rewind-guard mechanism),
        // resuming claps within a frame. Never `clear()` here: the full
        // rebuild + background resynthesis it forced took 2–3 s of
        // silence after every reset (the Step-7 scrub demo finding
        // 2026-08-15, and the checkpoint-4 loop gap before it). Earlier
        // phases need nothing: an in-flight synthesis is still authored
        // against the unchanged `m0`, and its `Ready` commit reads the
        // live count anyway.
        if song_reset::is_available() {
            self.reset_cb_id = Some(song_reset::on_song_reset(|t_ms| {
                stop_track_if_any("song reset");
                if let Ok(mut song) = SONG.lock() {
                    // The clap floor (any phase — an in-flight synthesis'
                    // eventual commit must respect it too): notes before
                    // the reset target were rebuilt consumed-neutral and
                    // must not clap. The latest reset governs.
                    song.reset_floor_ms = Some(t_ms.max(0));
                    if song.phase == Phase::Playing && song.encoded.is_some() {
                        song.phase = Phase::Ready;
                        log_info!("AssistTick: song reset -- tick track re-armed (re-shift)");
                    }
                }
            }));
        }

        // FR-4: the latency knob is retired. A persisted legacy
        // `assist_tick.offset_ms` (the old per-tick path's by-ear trim,
        // 125–150 on tuned installs) must be IGNORED, never reinterpreted —
        // the pre-mixed track derives its timing from game state.
        if let Some(legacy) = config::get()
            .and_then(|c| c.assist_tick.as_ref())
            .and_then(|a| a.offset_ms)
        {
            log_info!(
                "AssistTick: legacy config key assist_tick.offset_ms={} is retired and ignored (the pre-mixed track needs no latency knob)",
                legacy
            );
        }

        // The per-player ASSIST TICK row (unchanged). Default OFF;
        // builder-default persistence (Full: network save/load + offline
        // JSON cache = follows the card). There is no unregister API, so a
        // re-enable gets Duplicate — treated as success, with the atomics
        // reseeded from the registry because the duplicate path does not
        // re-fire on_change. A registration failure leaves both enables OFF:
        // the row is the only enable source, so the mod is silent rather
        // than gateless.
        if custom_options::is_available() {
            let spec = RegisterSpec::bool_toggle(OPT_ID)
                .display_name("Assist Tick")
                .description("Plays a clap sound at every arrow's judgement moment")
                .default_value(0)
                .on_change(on_option_change);
            match custom_options::register_option(spec) {
                Ok(_handle) => {
                    log_info!("AssistTick: registered ASSIST TICK option on the MODS tab");
                    register_volume_row();
                }
                Err(custom_options::RegisterError::Duplicate { .. }) => {
                    for side in 0..2u8 {
                        on_option_change(
                            side,
                            custom_options::get_value(side, OPT_ID).unwrap_or(0),
                        );
                    }
                    register_volume_row();
                }
                Err(e) => {
                    log_warn!(
                        "AssistTick: option registration failed: {e} -- no enable source, mod will stay silent"
                    );
                }
            }
        } else {
            log_warn!(
                "AssistTick: custom_options unavailable -- no enable source, mod will stay silent"
            );
        }

        log_info!("AssistTick: enabled (pre-mixed tick track, judgement-aligned; no latency knob)");
    }

    fn disable(&mut self) {
        if let Some(h) = self.judge_handle.take() {
            judge_hook::unregister(h);
        }
        if let Some(id) = self.scene_cb_id.take() {
            scene_manager::remove_callback(id);
        }
        if let Some(id) = self.reset_cb_id.take() {
            song_reset::remove_callback(id);
        }
        // Stop a playing track: with the callbacks gone, nothing else would
        // silence it before its 1200 s capacity ran out. Mod-menu toggles run
        // on the game thread (the overlay is driven from a game hook), so
        // the engine call is legal here.
        stop_track_if_any("mod disabled");
        for side in 0..2usize {
            ASSIST_TICK_ENABLED[side].store(false, Ordering::Release);
            LATCHED_ENABLED[side].store(false, Ordering::Release);
            // Level semantics extend to the producer going away: without
            // this, a taint from the last clapped song would go stale (the
            // scene callback that refreshes it is gone) and suppress an
            // honest later song.
            score_guard::set_assist_tick_taint(side, false);
            TICK_VOLUME[side].store(VOLUME_DEFAULT, Ordering::Release);
            LATCHED_VOLUME[side].store(VOLUME_DEFAULT, Ordering::Release);
        }
        if let Ok(mut song) = SONG.lock() {
            song.clear();
        }
        // The registered tick bank is deliberately left in place: destroying
        // an XACT bank a live cue may still reference is a crash class this
        // codebase has already been burned by, and an idle bank costs
        // nothing.
        log_info!("AssistTick: disabled (tick bank deliberately left in place)");
    }
}
