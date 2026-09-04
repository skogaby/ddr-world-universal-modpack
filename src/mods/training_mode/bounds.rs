//! A/B section markers, gameplay gestures, and the Step-3 row-derived
//! bound resolution (Training Mode v1 — design §4.2; the LOOP wiring
//! arrives in Step 4).
//!
//! Marker state lives in the content (raw-ms) domain, block-quantized
//! through the SAME composition the seek transaction applies (wall-domain
//! quantization on the live binding's served grid, then back to content),
//! so a stored marker is exactly the position a seek to it lands on.
//! `0` = no marker (design §5 data model). Markers are song-scoped:
//! cleared on every gameplay entry AND exit.
//!
//! Step 3 layers the SONG START TIME / SONG END TIME rows on top (absolute
//! timestamps, R2 amendment 2026-08-14): at GAMEPLAY entry
//! a pending resolution is queued and completed by the driver once the
//! actor tree exists ([`try_resolve_row_bounds`] — effective audio-length
//! clamp, then the chart-end formula with [`section_math::MIN_SECTION_MS`],
//! block-quantized). The resolved row-derived values seed the live bounds
//! and remain the press-5 RESTORE target; the session-active latch
//! ([`training_session_active`]) records row- or gesture-driven sessions
//! for the driver arm and Step 5's taint.
//!
//! Gestures (R3, as amended 2026-08-13 — all marker gestures live on the
//! pinpad's MIDDLE row 4-5-6), active only at GAMEPLAY with the mod
//! enabled, SINGLE-press like the FF/RW scrub (2026-08-18, superseding
//! the original triple-press design): **4** sets A at the current
//! position, **6** sets B, **5** restores the row-derived bounds
//! (clear-to-none when no rows were set). No button is shared with
//! quick_logout's triple-9 (SONG SELECT, scene 25) — and the scene gate
//! keeps every training gesture inert outside gameplay regardless.
//!
//! 2026-09-04 revision (loop/marker/timeline): EVERY gesture additionally
//! waits for [`song_reset::run_in_song`] (clock anchor landed + credible
//! count — the "READY?" banner window is inert; pre-anchor `+0x178` holds
//! the raw frame tick, and a B set from it soft-locked a LOOP-OFF song),
//! and the marker gestures 4/5/6 are LOOP-ONLY (gated on
//! [`loop_latched`], one hint toast per song when refused). SONG START /
//! END TIME are LOOP SONG's child rows; with LOOP OFF their retained
//! values are ignored at every reader (GAMEPLAY-entry arm, resolution,
//! pre-shift), so the v1 "LOOP OFF + section end ⇒ early natural end"
//! behavior is retired — [`section_math::end_policy`]'s `WriteThresholds`
//! arm survives only as dead-defensive code.
//!
//! Step 7 adds FF/RW scrobbling (the amended R12): SINGLE-press pinpad
//! **7 = rewind** / **9 = fast-forward** by the configured
//! `training_mode.{rw,ff}_increment_ms` (default 5000, normalized
//! 250..=60000 at enable — [`load_scrub_increments`]), dispatched through
//! the shipped seek transaction with the marker path's clamp/quantize
//! split ([`section_math::scrub_target`] → [`quantize_marker`]) and NO
//! approach lead (maintainer amendment 2026-08-15: the scrub is a pure
//! timeline adjuster — playback picks up AT the target, music-player
//! style; `TRAINING_LEAD_MS` remains the section-practice lead only). A
//! target clamping to 0 (rewind past the start) dispatches the plain
//! instant t=0 restart — the loop driver's whole-song-restart precedent.
//! One scrub in flight ([`SCRUB_COOLING`], lazily cleared once
//! [`song_reset::reset_in_flight`] drops), yielding to the loop
//! driver's in-flight reset; refused presses are dropped (never queued),
//! structural failures WARN once per song and the song plays on. Each
//! dispatched scrub flashes the RW/FF screen indicator
//! ([`super::scrub_indicator`]).

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};

use super::section_math;
use crate::mods::config;
use crate::services::song_rate;
use crate::services::song_reset::{self, seek, AccumulatorPolicy, ResetOutcome};
use crate::types::buttons::{button, InputEvent, InputEventType, Player};
use crate::types::scenes::scene;
use crate::{log_debug, log_info, log_warn};

/// Marker targets stay this far below the chart end (the seek transaction
/// enforces its own identical margin at fire time; clamping at SET time
/// keeps the stored marker honest).
const MARKER_END_MARGIN_MS: i32 = 1_000;

/// Whether the gesture surface is live (mirrors the mod's ACTIVE latch;
/// the input callback checks it first so a disabled mod is zero-footprint).
pub(super) static GESTURES_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Section start marker A (raw ms; 0 = none) — the LIVE bound (row-derived
/// at resolution, then overwritten by gestures).
static A_MS: AtomicI32 = AtomicI32::new(0);
/// Section end marker B (raw ms; 0 = none — consumed by Steps 4–6).
static B_MS: AtomicI32 = AtomicI32::new(0);

/// Row-derived section bounds (Step 3, design §4.2): latched once per song
/// by [`try_resolve_row_bounds`], block-quantized, content domain. The
/// press-5 RESTORE source — gestures refine the live `A_MS`/`B_MS`;
/// press-5 brings the row-derived values back (0/none when the rows were
/// 0, which degenerates to Step 2's clear-to-none). 0 = none.
static ROW_A_MS: AtomicI32 = AtomicI32::new(0);
static ROW_B_MS: AtomicI32 = AtomicI32::new(0);
/// The latched stock chart end for the resolved side (0 = unresolved).
static CHART_END_MS: AtomicI32 = AtomicI32::new(0);
/// Whether this song's row-derived resolution is still outstanding: set at
/// GAMEPLAY entry (the actors — and thus `chart_end_raw` — do not exist at
/// the scene-change instant), retried per frame by the Step-3 driver until
/// the actor tree resolves.
static RESOLUTION_PENDING: AtomicBool = AtomicBool::new(false);
/// The training-session-active latch (design §4.1): rows > 0 at gameplay
/// entry, or a gesture set a marker mid-song. Consumed by the driver arm
/// and Step 5's taint. Cleared with the markers at song boundaries.
static SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Per-side SONG START TIME row values (seconds — the option row's raw
/// timestamp domain, R2 amendment 2026-08-14). Written by the row's change
/// callback; consumed by the gameplay-entry bound resolution (design
/// §4.2). Session-scoped by the row's `PersistMode::Session` (card-in
/// resets the row, whose callback resets these).
static ROW_START_TIME_S: [AtomicI32; 2] = [AtomicI32::new(0), AtomicI32::new(0)];
/// Per-side SONG END TIME row values (seconds; the row cap
/// [`section_math::BOUND_ROW_MAX_S`] = "natural end", also the default —
/// mirrored here so an unregistered/unseeded state reads as no-op).
static ROW_END_TIME_S: [AtomicI32; 2] = [
    AtomicI32::new(section_math::BOUND_ROW_MAX_S),
    AtomicI32::new(section_math::BOUND_ROW_MAX_S),
];

/// The song the current row values belong to (R2 second amendment
/// 2026-08-14: the bound rows are SONG-scoped — the highlight seeder
/// re-stamps on every highlighted-song change, and the resolution
/// declines rows stamped for a DIFFERENT song than the one being played,
/// which closes the fast-confirm race where a song is entered before its
/// wheel-settle publication landed). `0` = unstamped (forces the seeder
/// to run; also the publication-less fail-open wildcard).
static ROWS_DIGEST: AtomicU64 = AtomicU64::new(0);
/// The END value the highlight seeder last wrote (the song's rounded
/// length) — the "row untouched" reference for the driver-arming
/// predicate: a seeded END is NOT an engaged section end.
static SEEDED_END_S: AtomicI32 = AtomicI32::new(section_math::BOUND_ROW_MAX_S);

/// Per-side LOOP SONG row values (Step 4, design §4.1). A PLAIN Session
/// row (breakdown decision #3): NOT song-scoped — grind mode survives
/// song switches within the session (no highlight seeder, no digest
/// stamp); the card-in session reset restores OFF for the next player.
static ROW_LOOP_SONG: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
/// The per-song loop latch (design §4.2/§4.3): the governing side's LOOP
/// SONG value, latched once per song at resolution (the options modal is
/// select-only — no mid-song toggle path exists). The governing side is
/// the resolving side (side 0 in versus, where the rows are mirrored by
/// `versus_mirror` anyway). The end policy's input and the Step-4 loop
/// driver's arm gate. Cleared with the session state.
static LOOP_LATCHED: AtomicBool = AtomicBool::new(false);
/// One-per-song latch for the LOOP-OFF marker-gesture hint toast
/// (2026-09-04 revision): the first refused 4/5/6 press on a song whose
/// loop is not latched flashes "Enable LOOP SONG to set markers"; later
/// presses drop silently. Cleared with the session state.
static LOOP_HINT_SHOWN: AtomicBool = AtomicBool::new(false);

// ── LOOP OFF threshold apply state (Step 4, design §4.2/§5) ──────────
/// Whether this song's CMA end thresholds currently hold OUR values —
/// LOOP OFF's truncated pair or LOOP ON's raised `+0x94` (design §5's
/// `thresholds_written`) — the restore/rewrite idempotence latch.
static THRESHOLDS_WRITTEN: AtomicBool = AtomicBool::new(false);
/// Whether the LOOP ON raise is in effect (the `+0x94` display threshold
/// parked out of reach — the cascade cannot fire; the loop driver's
/// bound needs no display-threshold term). Cleared by the session reset
/// and by [`on_loop_disarmed`]'s restore.
static LOOP_THRESHOLDS_RAISED: AtomicBool = AtomicBool::new(false);
/// PER-SIDE stock threshold pairs, stashed on the song's FIRST write so a
/// section end cleared back to none can restore the natural end.
/// Per-side is load-bearing since the 2026-08-31 versus-training lift: in
/// versus each side plays its OWN chart (different difficulties ⇒
/// different `+0x94/+0x98`), so one side's pair written onto the other
/// side's CMA would corrupt its natural end. `STASH_DONE` is the
/// once-per-song capture latch (later writes would stash our own values).
static STASH_DONE: AtomicBool = AtomicBool::new(false);
static STASH_VALID: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
static STASH_DISPLAY_MS: [AtomicI32; 2] = [AtomicI32::new(0), AtomicI32::new(0)];
static STASH_RAW_MS: [AtomicI32; 2] = [AtomicI32::new(0), AtomicI32::new(0)];
/// One-per-song latch for the apply ladder's WARN (design §6: converter
/// failure / notes unavailable / write refused ⇒ natural end, ONE WARN).
static END_APPLY_WARNED: AtomicBool = AtomicBool::new(false);

// ── LOOP death bypass (Step-7 amendment 2026-08-15) ──────────────────
/// Whether we armed the actors' instant-death gauge gate (`+0x2B7`) for
/// this song's loop — the restore-idempotence latch.
static DEATH_GATE_ARMED: AtomicBool = AtomicBool::new(false);
/// PER-SIDE stock gate values stashed at arm (immortal-class gauges carry
/// a nonzero stock gate, and versus sides can run different gauge
/// classes — blind-restoring one side's stock to both would un-gate or
/// over-gate the other).
static DEATH_GATE_STASH_VALID: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
static DEATH_GATE_STASH: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];

/// Arm the loop's death bypass (called at the loop latch, where the live
/// actor tree is proven): stash each live side's stock `+0x2B7` gate and
/// set the gate on every actor. With the gate up, a gauge death latches
/// `m_isDead` but can neither advance the actor to STEP_GAME_OVER nor
/// finish the DPS (both engine paths are conditioned on the gate —
/// 20260721 decompile), so the loop driver can detect the death at its
/// leisure and fire the loop back to A; the reset's own completion block
/// clears the death flags and restores the gauge. In versus EITHER side's
/// death revives BOTH into the next pass (one shared grind). Fail-open:
/// an unreadable/unwritable gate leaves stock death behavior (the grind
/// just still fails out on a miss cascade).
pub(super) fn arm_death_bypass() {
    let mut any = false;
    for side in 0..2usize {
        if let Some(stock) = song_reset::death_gate_for_side(side as i32) {
            DEATH_GATE_STASH[side].store(stock, Ordering::Release);
            DEATH_GATE_STASH_VALID[side].store(true, Ordering::Release);
            any = true;
        }
    }
    if !any {
        log_warn!("TrainingMode: death gate unreadable -- loop keeps stock death behavior");
        return;
    }
    if !song_reset::set_death_gate(true) {
        for side in 0..2usize {
            DEATH_GATE_STASH_VALID[side].store(false, Ordering::Release);
        }
        log_warn!("TrainingMode: death gate write refused -- loop keeps stock death behavior");
        return;
    }
    DEATH_GATE_ARMED.store(true, Ordering::Release);
    log_info!(
        "TrainingMode: loop death bypass armed (stock gates p1={:?} p2={:?})",
        DEATH_GATE_STASH_VALID[0]
            .load(Ordering::Acquire)
            .then(|| DEATH_GATE_STASH[0].load(Ordering::Acquire)),
        DEATH_GATE_STASH_VALID[1]
            .load(Ordering::Acquire)
            .then(|| DEATH_GATE_STASH[1].load(Ordering::Acquire)),
    );
}

/// Restore each stashed side's death gate (loop disarm / session end).
/// Refusals at song boundaries are harmless — the gate is per-actor state
/// and dies with the actor tree; fresh actors are stock.
pub(super) fn disarm_death_bypass() {
    if !DEATH_GATE_ARMED.swap(false, Ordering::AcqRel) {
        return;
    }
    let mut restored = false;
    for side in 0..2usize {
        if DEATH_GATE_STASH_VALID[side].load(Ordering::Acquire)
            && song_reset::set_death_gate_for_side(
                side as i32,
                DEATH_GATE_STASH[side].load(Ordering::Acquire),
            )
        {
            restored = true;
        }
    }
    if restored {
        log_info!("TrainingMode: loop death bypass disarmed (stock gates restored)");
    }
}

/// Row change callback target: store one side's LOOP SONG state.
pub(super) fn set_row_loop_song(side: u8, on: bool) {
    if let Some(slot) = ROW_LOOP_SONG.get(side as usize) {
        slot.store(on, Ordering::Release);
    }
}

/// One side's LOOP SONG row value.
pub fn row_loop_song(side: usize) -> bool {
    ROW_LOOP_SONG
        .get(side)
        .map(|slot| slot.load(Ordering::Acquire))
        .unwrap_or(false)
}

/// Whether LOOP SONG is latched for the current song (the entered side's
/// row value at resolution) — the loop driver's arm gate and the end
/// policy's loop input.
pub fn loop_latched() -> bool {
    LOOP_LATCHED.load(Ordering::Acquire)
}

/// Whether the LOOP ON `+0x94` raise is live for this song — the loop
/// driver's fire bound drops the display-threshold term (the cascade is
/// parked and cannot fire).
pub fn loop_thresholds_raised() -> bool {
    LOOP_THRESHOLDS_RAISED.load(Ordering::Acquire)
}

/// The loop driver's disarm hook (refusal ladder / degenerate section):
/// the grind is over for this song but the RUN CONTINUES — the raised
/// `+0x94` MUST be restored or the cascade can never fire and the song
/// can never end (a soft-lock at the natural end). Also drops the loop
/// latch so later policy evaluations (gesture B-sets) behave as LOOP
/// OFF for the rest of the run.
pub(super) fn on_loop_disarmed() {
    LOOP_LATCHED.store(false, Ordering::Release);
    LOOP_THRESHOLDS_RAISED.store(false, Ordering::Release);
    // The death bypass is loop-scoped: with no loop to catch a death, the
    // gate MUST come down or the player could never fail out naturally.
    disarm_death_bypass();
    if THRESHOLDS_WRITTEN.load(Ordering::Acquire) && restore_stock_thresholds() {
        THRESHOLDS_WRITTEN.store(false, Ordering::Release);
        log_info!("TrainingMode: stock end thresholds restored (loop disarmed)");
    }
}

/// Stamp the rows as belonging to `digest`, remembering the seeded END
/// (the highlight seeder's bookkeeping half; the row writes themselves go
/// through the registry + [`set_row_start_time`]/[`set_row_end_time`]).
pub(super) fn stamp_rows(digest: u64, seeded_end_s: i32) {
    SEEDED_END_S.store(seeded_end_s, Ordering::Release);
    ROWS_DIGEST.store(digest, Ordering::Release);
}

/// The rows' current song stamp (0 = unstamped).
pub(super) fn rows_digest() -> u64 {
    ROWS_DIGEST.load(Ordering::Acquire)
}

/// Clear the song stamp so the highlight seeder re-runs even on the same
/// song (the card-in session reset's hook: the reset put the rows back to
/// their abstract defaults, and the new player must see the seeded
/// timestamps, not the abstract cap).
pub(super) fn clear_rows_digest() {
    ROWS_DIGEST.store(0, Ordering::Release);
    SEEDED_END_S.store(section_math::BOUND_ROW_MAX_S, Ordering::Release);
}

/// Row change callback target: store one side's SONG START TIME seconds.
pub(super) fn set_row_start_time(side: u8, seconds: i32) {
    if let Some(slot) = ROW_START_TIME_S.get(side as usize) {
        slot.store(seconds.max(0), Ordering::Release);
    }
}

/// Row change callback target: store one side's SONG END TIME seconds.
pub(super) fn set_row_end_time(side: u8, seconds: i32) {
    if let Some(slot) = ROW_END_TIME_S.get(side as usize) {
        slot.store(seconds.max(0), Ordering::Release);
    }
}

/// One side's SONG START TIME row value in seconds (0 = natural start).
/// The raw row value — the effective per-song clamp is applied at use
/// time (design §4.2).
pub fn row_start_time(side: usize) -> i32 {
    ROW_START_TIME_S
        .get(side)
        .map(|slot| slot.load(Ordering::Acquire))
        .unwrap_or(0)
}

/// One side's SONG END TIME row value in seconds (the row cap = natural
/// end).
pub fn row_end_time(side: usize) -> i32 {
    ROW_END_TIME_S
        .get(side)
        .map(|slot| slot.load(Ordering::Acquire))
        .unwrap_or(section_math::BOUND_ROW_MAX_S)
}

// ── FF/RW scrobbling (Step 7, the amended R12) ───────────────────────
/// Per-press scrub increments (raw ms), latched from
/// `training_mode.{ff,rw}_increment_ms` at mod enable
/// ([`load_scrub_increments`] — normalized 250..=60000, default 5000).
static FF_INCREMENT_MS: AtomicI32 = AtomicI32::new(section_math::SCRUB_INCREMENT_DEFAULT_MS);
static RW_INCREMENT_MS: AtomicI32 = AtomicI32::new(section_math::SCRUB_INCREMENT_DEFAULT_MS);
/// One scrub in flight: set at `Started`, lazily cleared at the next
/// press once the underlying transaction is no longer in flight
/// ([`song_reset::reset_in_flight`] — cleared there at completion, every
/// recovery, and scene changes), and cleared with the session state.
static SCRUB_COOLING: AtomicBool = AtomicBool::new(false);
/// One-per-song latch for the scrub's fail-open WARN (task req 6: every
/// structural gate failure drops the press with at most one WARN per
/// song; the song is never disturbed).
static SCRUB_WARNED: AtomicBool = AtomicBool::new(false);

/// Latch the scrub increments from `training_mode.{ff,rw}_increment_ms`
/// (mod enable): absent keys/block default to
/// [`section_math::SCRUB_INCREMENT_DEFAULT_MS`]; out-of-range values
/// normalize into 250..=60000 with ONE INFO naming both effective values
/// (the `restart_delay_ms` pattern).
pub(super) fn load_scrub_increments() {
    let block = config::get().and_then(|c| c.training_mode.as_ref());
    let raw_ff = block.and_then(|t| t.ff_increment_ms);
    let raw_rw = block.and_then(|t| t.rw_increment_ms);
    let ff = section_math::normalize_scrub_increment_ms(raw_ff);
    let rw = section_math::normalize_scrub_increment_ms(raw_rw);
    if raw_ff.is_some_and(|v| v != ff) || raw_rw.is_some_and(|v| v != rw) {
        log_info!(
            "TrainingMode: scrub increments normalized into {}..={} ms -- ff {} ms, rw {} ms",
            section_math::SCRUB_INCREMENT_MIN_MS,
            section_math::SCRUB_INCREMENT_MAX_MS,
            ff,
            rw
        );
    }
    FF_INCREMENT_MS.store(ff, Ordering::Release);
    RW_INCREMENT_MS.store(rw, Ordering::Release);
}

/// The scrub's fail-open WARN — once per song (cleared with the session
/// state). Cooling refusals and transient no-live-count drops stay at
/// debug level; this ladder is for structural failures only.
fn warn_scrub_once(what: &str) {
    if !SCRUB_WARNED.swap(true, Ordering::AcqRel) {
        log_warn!("TrainingMode: {}", what);
    }
}

/// One FF/RW scrub press (Step 7): compute the clamped target (pure math
/// + live quantize, the marker-set split) and fire the shipped seek
/// transaction. `side` is the pressing player's — the taint target.
/// Frame-thread, panic-free: guarded reads + atomics only, no locks held
/// across the service calls.
fn scrub(side: usize, delta_ms: i32) {
    // One in-flight transaction total (task req 5): our own latch —
    // lazily cleared once the underlying transaction completed or
    // recovered — plus the transaction's own flag and the loop driver's
    // cooling latch. A refused press is DROPPED, never queued.
    if SCRUB_COOLING.load(Ordering::Acquire) {
        if song_reset::reset_in_flight() {
            log_debug!("TrainingMode: scrub dropped -- own scrub still in flight");
            return;
        }
        SCRUB_COOLING.store(false, Ordering::Release);
    }
    if song_reset::reset_in_flight() || super::driver::loop_reset_in_flight() {
        log_debug!("TrainingMode: scrub dropped -- another reset is in flight");
        return;
    }
    // Transient absence of a live count = not in a run (silent drop, the
    // set_marker precedent).
    let Some(current) = song_reset::current_raw_music_count() else {
        log_debug!("TrainingMode: scrub dropped -- no live music count");
        return;
    };
    // A live binding is required on EVERY scrub path: the seek transaction
    // preflights one for t > 0, and gating the t = 0 restart on it too
    // keeps the scrub confined to training-armed sessions (course never
    // arms). In versus (armable since the 2026-08-31 lift) EITHER player's
    // scrub seeks the ONE shared clock — both runs move together by
    // construction, and both sides taint below.
    if song_rate::runtime::active_content_grid().is_none() {
        warn_scrub_once("no live song-rate binding -- scrub unavailable this song");
        return;
    }
    let Some(chart_end) = (0..2).filter_map(song_reset::chart_end_raw).min() else {
        warn_scrub_once("chart end unreadable -- scrub unavailable this song");
        return;
    };
    let Some(target) = section_math::scrub_target(
        current,
        delta_ms,
        section_end(),
        chart_end,
        MARKER_END_MARGIN_MS,
    ) else {
        warn_scrub_once("degenerate scrub bound -- scrub unavailable this song");
        return;
    };
    // Quantize AFTER the clamp (the marker-set split): the grid moves the
    // target by at most ~one ADPCM block (~3 ms content), absorbed by the
    // 1000 ms margin, and the transaction re-quantizes identically at
    // fire time. A target at/below 0 is "rewind past the start" — the
    // plain t=0 restart (the loop driver's whole-song precedent).
    let t_q = quantize_marker(target).max(0);
    if t_q > 0 && !song_reset::seek_available() {
        warn_scrub_once("seek machinery unavailable -- scrub disabled this song");
        return;
    }
    // NO approach lead (maintainer amendment 2026-08-15, mid-demo): the
    // scrub is a pure timeline adjuster — playback picks up AT the
    // target immediately, like a music player's FF/RW (the only silence
    // is the inherent ~150–300 ms cue re-prepare). TRAINING_LEAD_MS
    // stays the section-practice lead (restart-from-A / loop passes),
    // where scroll-in time is the point.
    match song_reset::request_reset(t_q, 0, AccumulatorPolicy::Zero, None) {
        ResetOutcome::Started => {
            SCRUB_COOLING.store(true, Ordering::Release);
            // The scrub alters the played song (AC 7): session latch +
            // taint, the set_marker gesture pattern. The Step-5
            // on_song_reset(t>0) subscriber re-taints t > 0 seeks anyway
            // (idempotent); setting SESSION_ACTIVE here is what carries
            // the t = 0 rewind-to-start restart into its predicate.
            SESSION_ACTIVE.store(true, Ordering::Release);
            super::taint_entered_sides();
            super::scrub_indicator::show(delta_ms >= 0);
            log_info!(
                "TrainingMode: scrub {} -- {} ms -> {} ms (delta {} ms, side {})",
                if delta_ms >= 0 { "FF" } else { "RW" },
                current,
                t_q,
                delta_ms,
                side
            );
        }
        outcome => {
            warn_scrub_once(&format!(
                "scrub reset refused ({outcome:?}) -- press dropped"
            ));
        }
    }
}

/// The active section start for restart-from-A, or `None` when no A
/// marker is set (or the mod is inactive).
pub fn active_section_start() -> Option<i32> {
    if !GESTURES_ACTIVE.load(Ordering::Acquire) {
        return None;
    }
    let a_ms = A_MS.load(Ordering::Acquire);
    (a_ms > 0).then_some(a_ms)
}

/// The B marker (Steps 4–6 consume it; exposed for symmetry/diagnostics).
pub fn section_end() -> Option<i32> {
    let b_ms = B_MS.load(Ordering::Acquire);
    (b_ms > 0).then_some(b_ms)
}

/// Clear both markers (song change / disable). Returns whether any
/// marker actually existed (the gesture arm's toast gate — lifecycle
/// clears must stay silent).
pub(super) fn clear_markers(reason: &str) -> bool {
    // Both swaps must ALWAYS run: `||` would short-circuit past the B
    // clear whenever A was set (cabinet bug 2026-08-14 — the clear
    // gesture cleared only the start marker and the loop kept firing at
    // the old end).
    let had_a = A_MS.swap(0, Ordering::AcqRel) > 0;
    let had_b = B_MS.swap(0, Ordering::AcqRel) > 0;
    let had = had_a | had_b;
    if had {
        log_info!("TrainingMode: markers cleared ({})", reason);
    }
    had
}

/// Reset the Step-3 per-song session state (row-derived latches, chart
/// end, session-active, pending resolution) alongside the marker clear,
/// plus the Step-4 loop latch and threshold-apply state. The full
/// song-boundary / mod-disable reset. Written thresholds are restored
/// first when the run is still live (the mid-song mod-disable case);
/// the restore is fail-closed on [`song_reset::set_chart_end_thresholds_per_side`]'
/// own gates (live-actor walk, lock-free), so the song-boundary calls
/// simply refuse against the dying/absent actor tree. Deliberately NO
/// `scene_manager::current_scene()` read here: this runs inside the
/// scene-change callback, and re-entering the scene manager from a
/// callback deadlocked the frame thread on the 2026-08-14 cabinet run
/// (fixed in scene_manager too — callbacks now fire outside the lock —
/// but this function stays lock-free regardless).
pub(super) fn clear_session_state(reason: &str) {
    if THRESHOLDS_WRITTEN.load(Ordering::Acquire) && restore_stock_thresholds() {
        log_info!("TrainingMode: stock end thresholds restored ({})", reason);
    }
    // Death-bypass gate down BEFORE the stash clears below (the disarm
    // restores from the per-side stashes). Mid-song disable case; at song
    // boundaries the restore refuses harmlessly against the dying/absent
    // actor tree — the gate dies with the actors.
    disarm_death_bypass();
    clear_markers(reason);
    ROW_A_MS.store(0, Ordering::Release);
    ROW_B_MS.store(0, Ordering::Release);
    CHART_END_MS.store(0, Ordering::Release);
    RESOLUTION_PENDING.store(false, Ordering::Release);
    SESSION_ACTIVE.store(false, Ordering::Release);
    LOOP_LATCHED.store(false, Ordering::Release);
    THRESHOLDS_WRITTEN.store(false, Ordering::Release);
    LOOP_THRESHOLDS_RAISED.store(false, Ordering::Release);
    STASH_DONE.store(false, Ordering::Release);
    for side in 0..2usize {
        STASH_VALID[side].store(false, Ordering::Release);
        STASH_DISPLAY_MS[side].store(0, Ordering::Release);
        STASH_RAW_MS[side].store(0, Ordering::Release);
        DEATH_GATE_STASH_VALID[side].store(false, Ordering::Release);
        DEATH_GATE_STASH[side].store(false, Ordering::Release);
    }
    END_APPLY_WARNED.store(false, Ordering::Release);
    SCRUB_COOLING.store(false, Ordering::Release);
    SCRUB_WARNED.store(false, Ordering::Release);
    LOOP_HINT_SHOWN.store(false, Ordering::Release);
}

/// Whether a training session is active for the current song (design
/// §4.1's predicate): row-derived bounds engaged at entry, or a gesture
/// set a marker mid-song. The Step-3 driver's arm gate and Step 5's taint
/// source. Latched per song; cleared with [`clear_session_state`].
pub fn training_session_active() -> bool {
    SESSION_ACTIVE.load(Ordering::Acquire)
}

/// The latched row-derived bounds `(a_ms, b_ms)` (0 = none) — the press-5
/// restore source; the Step-3/4 drivers' section reference.
pub fn row_derived_bounds() -> (i32, i32) {
    (
        ROW_A_MS.load(Ordering::Acquire),
        ROW_B_MS.load(Ordering::Acquire),
    )
}

/// Attempt the once-per-song row-derived bound resolution (design §4.2).
/// Returns `true` when nothing is (or remains) outstanding — either the
/// resolution completed (now or earlier) or none is pending; `false` means
/// "actors not up yet, retry next frame" (the Step-3 driver's loop).
///
/// The governing side is the FIRST side whose ControlMessageActor
/// resolves — side 0 in versus (both actors live; P1 governs, matching
/// the scene-26 classifier, and the bound rows are mirrored across sides
/// by `versus_mirror` so the side choice is value-neutral), the single
/// entered side otherwise.
pub fn try_resolve_row_bounds() -> bool {
    if !RESOLUTION_PENDING.load(Ordering::Acquire) {
        return true;
    }
    let Some((side, chart_end)) =
        (0..2).find_map(|side| song_reset::chart_end_raw(side).map(|end| (side, end)))
    else {
        return false;
    };

    // Loop latch (Step 4, design §4.2): the governing side's LOOP SONG
    // value, once per song (rows mirrored in versus). Taken BEFORE the song-coherence gate — the
    // loop row is a plain Session row (not song-scoped), so its value is
    // valid whichever song the bound rows were stamped for. Loop-ON is a
    // training session by itself (breakdown decision #2: it loops the
    // whole song even with no bounds set).
    let loop_on = row_loop_song(side as usize);
    if loop_on {
        LOOP_LATCHED.store(true, Ordering::Release);
        SESSION_ACTIVE.store(true, Ordering::Release);
        // Step 5 (design §4.7/R5): a latched loop WILL grind this song —
        // taint every entered side (versus: the loop moves the ONE shared
        // timeline, so both players' runs are altered). Deliberately on
        // both digest paths (the latch precedes the coherence gate): even
        // when stale bound rows resolve as defaults, the loop still fires.
        super::taint_entered_sides();
        // Step-7 amendment (2026-08-15): a loop session bypasses death —
        // gate the actors' instant-death byte so a gauge empty can never
        // end the run; the driver detects the latched m_isDead and loops
        // back to A (the reset revives).
        arm_death_bypass();
        log_info!("TrainingMode: LOOP SONG latched for this song (side {side})");
    } else {
        // LOOP OFF (2026-09-04 revision): SONG START/END TIME are LOOP
        // SONG's children and their retained values are IGNORED — the
        // song resolves as defaults (no A/B, no threshold write, whole
        // song from 0). Normally unreachable (the GAMEPLAY-entry arm
        // requires the loop row), kept as the resolution's own safety
        // property against any other caller.
        CHART_END_MS.store(chart_end, Ordering::Release);
        RESOLUTION_PENDING.store(false, Ordering::Release);
        apply_end_policy();
        return true;
    }

    // Song-coherence gate (R2 second amendment): the rows must describe
    // THIS song. The gameplay bank create republished the playing song's
    // digest, so a stamp naming a different song means the rows are stale
    // (the fast-confirm race — the song was entered before its
    // wheel-settle publication seeded them). Stale rows resolve as
    // defaults: the song plays whole, exactly as an untouched menu
    // promises.
    let publication = song_rate::selected_song::selected_song();
    let fresh_digest = publication.map(|info| info.code_digest);
    if !song_rate::selected_song::digests_coherent(rows_digest(), fresh_digest) {
        log_info!(
            "TrainingMode: row bounds stamped for a different song -- resolving as defaults (fast-confirm race)"
        );
        CHART_END_MS.store(chart_end, Ordering::Release);
        RESOLUTION_PENDING.store(false, Ordering::Release);
        apply_end_policy();
        return true;
    }

    // Effective clamp (select-time audio length, design §4.2/R2) composed
    // with the chart-derived resolution formula — START only (the audio
    // cap is an upper bound, audio >= chart; END is governed solely by the
    // chart-end normalization inside `resolve_bounds`, so a whole-second
    // audio floor can never fabricate a phantom section end).
    let audio_len = publication.map(|info| info.audio_len_ms);
    let start_s = section_math::effective_bound_seconds(row_start_time(side as usize), audio_len);
    let end_s = row_end_time(side as usize);
    let bounds = section_math::resolve_bounds(start_s, end_s, chart_end, MARKER_END_MARGIN_MS);
    let a_ms = if bounds.a_ms > 0 {
        quantize_marker(bounds.a_ms)
    } else {
        0
    };
    let b_ms = if bounds.b_ms > 0 {
        quantize_marker(bounds.b_ms)
    } else {
        0
    };

    CHART_END_MS.store(chart_end, Ordering::Release);
    ROW_A_MS.store(a_ms, Ordering::Release);
    ROW_B_MS.store(b_ms, Ordering::Release);
    // The rows seed the LIVE bounds (markers were cleared at entry);
    // gestures refine them from here.
    A_MS.store(a_ms, Ordering::Release);
    B_MS.store(b_ms, Ordering::Release);
    if a_ms > 0 || b_ms > 0 {
        SESSION_ACTIVE.store(true, Ordering::Release);
        // Step 5 (design §4.7/R5): engaged section bounds alter the played
        // song for EVERY participant (one shared timeline) — taint every
        // entered side's per-stage save.
        super::taint_entered_sides();
        log_info!(
            "TrainingMode: row-derived bounds resolved -- a={} ms, b={} ms (chart end {} ms, side {}, start {} s / end {} s)",
            a_ms,
            b_ms,
            chart_end,
            side,
            start_s,
            end_s
        );
    }
    RESOLUTION_PENDING.store(false, Ordering::Release);
    apply_end_policy();
    true
}

/// The song-end threshold apply (Step 4, design §4.2): evaluate the end
/// policy over the latched loop state and the LIVE section end, then
/// write / raise / restore / leave the ControlMessageActor thresholds
/// per the pure transition table. Driven from every point the section
/// end can change — resolution completion, a gesture B-set, and the
/// press-5 clear. LOOP ON parks the cascade (raised `+0x94`) instead of
/// truncating; every failure leaves the thresholds untouched with ONE
/// WARN per song and the song ends naturally (design §6).
fn apply_end_policy() {
    let b_live = B_MS.load(Ordering::Acquire);
    let policy = section_math::end_policy(loop_latched(), b_live);
    let written = THRESHOLDS_WRITTEN.load(Ordering::Acquire);
    match section_math::apply_action(policy, written) {
        section_math::ApplyAction::Write { b_ms } => write_end_thresholds(b_ms),
        section_math::ApplyAction::RaiseThresholds => raise_end_thresholds(),
        section_math::ApplyAction::Restore => {
            if restore_stock_thresholds() {
                THRESHOLDS_WRITTEN.store(false, Ordering::Release);
                log_info!("TrainingMode: stock end thresholds restored (section end cleared)");
            } else {
                warn_end_apply_once(
                    "stock threshold restore refused -- truncated end stays for this song",
                );
            }
        }
        section_math::ApplyAction::Nothing => {}
    }
}

/// Capture each live side's stock threshold pair once per song (shared by
/// the truncating write and the loop raise — later writes would stash our
/// own values). Per-side since the versus-training lift: the sides play
/// different charts, so each CMA's pair must round-trip through its own
/// stash. `false` = no side readable (the caller's WARN ladder).
fn ensure_stock_stash() -> bool {
    if STASH_DONE.load(Ordering::Acquire) {
        return (0..2).any(|side| STASH_VALID[side].load(Ordering::Acquire));
    }
    let mut any = false;
    for side in 0..2usize {
        if let Some((display, raw)) = song_reset::chart_end_thresholds(side as i32) {
            STASH_DISPLAY_MS[side].store(display, Ordering::Release);
            STASH_RAW_MS[side].store(raw, Ordering::Release);
            STASH_VALID[side].store(true, Ordering::Release);
            any = true;
        }
    }
    if any {
        STASH_DONE.store(true, Ordering::Release);
    }
    any
}

/// The per-side restore list from the stash: `(side, display, raw)` for
/// every stashed side — [`song_reset::set_chart_end_thresholds_per_side`]'s
/// input shape.
fn stash_restore_writes() -> Vec<(i32, i32, i32)> {
    (0..2usize)
        .filter(|&side| STASH_VALID[side].load(Ordering::Acquire))
        .map(|side| {
            (
                side as i32,
                STASH_DISPLAY_MS[side].load(Ordering::Acquire),
                STASH_RAW_MS[side].load(Ordering::Acquire),
            )
        })
        .collect()
}

/// Restore the stashed stock thresholds on every stashed side
/// (all-or-nothing through the per-side writer). `false` = nothing
/// restored (empty stash or the writer refused).
fn restore_stock_thresholds() -> bool {
    let writes = stash_restore_writes();
    !writes.is_empty() && song_reset::set_chart_end_thresholds_per_side(&writes)
}

/// The `RaiseThresholds` arm of [`apply_end_policy`] (LOOP ON): park the
/// end cascade by raising the `+0x94` display threshold to the sane-max
/// sentinel (unreachable — each loop iteration re-anchors the clock).
/// `+0x98` is written back at EACH SIDE'S OWN stock value: it is never
/// reached with the cascade parked below step 4, and live readers (marker
/// clamps, seek clamps, the loop fire bound) stay honest. `0x104A`
/// therefore never fires mid-grind — it is one-way song-scoped state that
/// strikes the lane furniture and breaks freeze scoring on later passes
/// (cabinet finding 2026-08-14). Idempotent; failure ⇒ WARN once and
/// the loop driver falls back to the conservative below-`+0x94` bound.
fn raise_end_thresholds() {
    if LOOP_THRESHOLDS_RAISED.load(Ordering::Acquire) {
        return;
    }
    if !ensure_stock_stash() {
        warn_end_apply_once(
            "stock end thresholds unreadable -- loop keeps the conservative fire bound",
        );
        return;
    }
    let writes: Vec<(i32, i32, i32)> = (0..2usize)
        .filter(|&side| STASH_VALID[side].load(Ordering::Acquire))
        .map(|side| {
            (
                side as i32,
                song_reset::CHART_END_SANE_MAX_MS,
                STASH_RAW_MS[side].load(Ordering::Acquire),
            )
        })
        .collect();
    if writes.is_empty() || !song_reset::set_chart_end_thresholds_per_side(&writes) {
        warn_end_apply_once("threshold raise refused -- loop keeps the conservative fire bound");
        return;
    }
    THRESHOLDS_WRITTEN.store(true, Ordering::Release);
    LOOP_THRESHOLDS_RAISED.store(true, Ordering::Release);
    log_info!(
        "TrainingMode: end cascade parked for the loop -- +0x94 raised to {} on {} side(s) (per-side stock raws kept)",
        song_reset::CHART_END_SANE_MAX_MS,
        writes.len()
    );
}

/// The `Write` arm of [`apply_end_policy`]: stash the stock thresholds
/// once per song, convert the raw-ms section end into the display domain
/// through EACH SIDE'S OWN note vector (versus sides play different
/// charts), and write each side's thresholds on its own CMA. Any failure
/// on any side fires the WARN-once ladder with the thresholds untouched —
/// the section end is applied whole (all sides) or not at all.
fn write_end_thresholds(b_ms: i32) {
    if !ensure_stock_stash() {
        warn_end_apply_once("stock end thresholds unreadable -- natural end");
        return;
    }
    let mut writes: Vec<(i32, i32, i32)> = Vec::new();
    for side in 0..2usize {
        if !STASH_VALID[side].load(Ordering::Acquire) {
            continue;
        }
        let Some(notes) = song_reset::decoded_notes(side as i32).filter(|n| !n.is_empty()) else {
            warn_end_apply_once("note vector unavailable -- natural end");
            return;
        };
        let Some(display_b) = seek::display_for_raw(&notes, b_ms) else {
            warn_end_apply_once(
                "display-domain conversion failed (degenerate note vector) -- natural end",
            );
            return;
        };
        writes.push((side as i32, display_b, b_ms));
    }
    if writes.is_empty() || !song_reset::set_chart_end_thresholds_per_side(&writes) {
        warn_end_apply_once("threshold write refused -- natural end");
        return;
    }
    THRESHOLDS_WRITTEN.store(true, Ordering::Release);
    log_info!(
        "TrainingMode: early natural end armed -- raw={} ms on {} side(s) (displays {:?})",
        b_ms,
        writes.len(),
        writes
            .iter()
            .map(|(side, display, _)| (*side, *display))
            .collect::<Vec<_>>()
    );
}

/// The design §6 apply ladder's WARN — once per song.
fn warn_end_apply_once(what: &str) {
    if !END_APPLY_WARNED.swap(true, Ordering::AcqRel) {
        log_warn!("TrainingMode: {}", what);
    }
}

/// The press-5 gesture (maintainer decision 2026-08-14, superseding the
/// Step-3 restore-to-rows semantics): clear the live bounds to NONE —
/// the rest of the run plays the whole song (LOOP OFF: the stock end is
/// restored by the end policy; LOOP ON: the grind becomes whole-song).
/// The row values are untouched — they re-resolve on the next song as
/// the menu promises. Returns whether the live bounds changed (the
/// toast gate).
fn clear_live_bounds() -> bool {
    // Both swaps must ALWAYS run (no `||` short-circuit — the cabinet
    // bug 2026-08-14: with A set, B was never cleared).
    let cleared_a = A_MS.swap(0, Ordering::AcqRel) > 0;
    let cleared_b = B_MS.swap(0, Ordering::AcqRel) > 0;
    let changed = cleared_a | cleared_b;
    if changed {
        log_info!("TrainingMode: markers cleared to the whole song (press 5)");
    }
    changed
}

/// Block-quantize a content-domain target through the seek's own
/// composition (wall-domain grid quantization → content). Falls back to
/// the raw value when no binding is live — the seek's gates still
/// protect, this only loses the cosmetic pre-quantization.
fn quantize_marker(t_ms: i32) -> i32 {
    let Some(grid) = song_rate::runtime::active_content_grid() else {
        return t_ms;
    };
    let snapshot = song_rate::clock_patch::snapshot();
    let wall = seek::wall_ms(t_ms, &snapshot);
    match seek::quantize_seek(
        wall,
        grid.samples_per_block,
        grid.sample_rate,
        grid.stream_blocks,
    ) {
        Some(quantized) => seek::content_ms(quantized.t_q_ms, &snapshot),
        None => t_ms,
    }
}

/// The chart-end clamp bound: the MIN of both sides' live chart ends
/// minus the margin, when any side resolves.
fn marker_clamp_bound() -> Option<i32> {
    let end = (0..2).filter_map(song_reset::chart_end_raw).min()?;
    Some(end.saturating_sub(MARKER_END_MARGIN_MS))
}

/// Latch A (or B) from the live music count. Refuses silently when no
/// live count exists (not in a run). `side` is the pressing player's —
/// log attribution only; the taint covers every entered side (one shared
/// timeline, design §4.7/R5 as amended by the versus-training lift).
fn set_marker(which: char, side: usize) {
    let Some(current) = song_reset::current_raw_music_count() else {
        return;
    };
    let mut target = quantize_marker(current.max(0));
    if let Some(bound) = marker_clamp_bound() {
        target = target.min(bound);
    }
    if target <= 0 {
        // The design's 0-sentinel means "none"; a marker at the literal
        // song start is meaningless (that IS restart-at-0).
        return;
    }
    match which {
        'A' => {
            A_MS.store(target, Ordering::Release);
            // A mid-song gesture makes this a training session (design
            // §4.1's predicate — Step 5's taint consumes the latch).
            SESSION_ACTIVE.store(true, Ordering::Release);
            super::taint_entered_sides();
            crate::services::toast::flash("Set beginning marker");
            log_info!(
                "TrainingMode: section start A set at {} ms (press 4, side {})",
                target,
                side
            );
        }
        _ => {
            B_MS.store(target, Ordering::Release);
            SESSION_ACTIVE.store(true, Ordering::Release);
            super::taint_entered_sides();
            crate::services::toast::flash("Set end marker");
            log_info!(
                "TrainingMode: section end B set at {} ms (press 6, side {})",
                target,
                side
            );
            // The section end changed: re-evaluate the end policy. The
            // gate in `on_input_event` guarantees the loop is latched
            // here, so this is the idempotent `RaiseThresholds` re-apply
            // (the loop driver re-reads B live for its fire bound). The
            // v1 LOOP-OFF "early natural end" write is retired
            // (2026-09-04): a section is only playable as a loop.
            apply_end_policy();
        }
    }
}

/// Input callback body (frame thread, panic-free): single-press 4/5/6
/// during eligible gameplay (the pinpad's middle row — A / clear / B),
/// plus the Step-7 single-press 7/9 FF/RW scrub. One press = one action
/// throughout (2026-08-18, superseding the triple-press marker
/// gestures) — no GestureBuffer anywhere on this surface.
///
/// 2026-09-04 revision — two gates over every press, decided by the pure
/// [`section_math::gesture_gate`]:
///
/// * **In-song** ([`song_reset::run_in_song`]): the run's clock anchor
///   has landed and the music count is credible. Before that (the
///   "READY?" banner window) `+0x178` holds the raw frame tick, and a B
///   set from it on a LOOP-OFF song soft-locked the game. EVERY gesture
///   (4/5/6 AND 7/9) drops silently until the arrows are scrolling.
/// * **Loop latched** ([`loop_latched`]): the marker gestures 4/5/6 are
///   loop-only — a section is only playable as a loop — and drop with a
///   one-per-song hint toast otherwise. 7/9 scrub is a plain timeline
///   adjuster and stays available.
pub(super) fn on_input_event(event: &InputEvent) {
    if event.event_type != InputEventType::Pressed {
        return;
    }
    if !GESTURES_ACTIVE.load(Ordering::Acquire) {
        return;
    }
    // Classify the press first so every other button returns before any
    // engine read (this callback runs per frame for every pinpad).
    let kind = match event.button {
        button::NUM_4 | button::NUM_5 | button::NUM_6 => section_math::GestureKind::Marker,
        // No conflict with quick_logout's triple-9, which is
        // song-select-scoped while this whole surface is GAMEPLAY-only.
        button::NUM_7 | button::NUM_9 => section_math::GestureKind::Scrub,
        _ => return,
    };
    if crate::services::scene_manager::current_scene() != scene::GAMEPLAY {
        return;
    }
    match section_math::gesture_gate(kind, song_reset::run_in_song(), loop_latched()) {
        section_math::GestureVerdict::Allow => {}
        section_math::GestureVerdict::DropPreSong => {
            log_debug!(
                "TrainingMode: pinpad {} dropped -- run not in-song yet (READY window / tail)",
                event.button_name
            );
            return;
        }
        section_math::GestureVerdict::DropLoopOff => {
            if !LOOP_HINT_SHOWN.swap(true, Ordering::AcqRel) {
                crate::services::toast::flash("Enable LOOP SONG to set markers");
                log_info!(
                    "TrainingMode: pinpad {} dropped -- LOOP SONG not latched this song (hint shown once)",
                    event.button_name
                );
            }
            return;
        }
    }
    let side = match event.player {
        Player::P1 => 0usize,
        Player::P2 => 1,
    };
    match event.button {
        // Scrub arm (Step 7): 7 = rewind, 9 = fast-forward.
        button::NUM_7 | button::NUM_9 => {
            let delta = if event.button == button::NUM_9 {
                FF_INCREMENT_MS.load(Ordering::Acquire)
            } else {
                -RW_INCREMENT_MS.load(Ordering::Acquire)
            };
            scrub(side, delta);
        }
        // Marker arm: 4 = set A, 6 = set B, 5 = clear.
        button::NUM_4 => set_marker('A', side),
        button::NUM_6 => set_marker('B', side),
        _ => {
            // 5: clear the live bounds — the rest of the run plays the
            // whole song (2026-08-14 maintainer decision).
            if clear_live_bounds() {
                crate::services::toast::flash("Cleared markers");
                // The section end moved to none: keep the parked cascade
                // with a whole-song fire bound (the loop is latched here
                // by construction of the gate above).
                apply_end_policy();
            }
        }
    }
}

/// Scene callback body: markers, row-derived bounds, and the session
/// latch are song-scoped; a fresh GAMEPLAY entry queues the row-derived
/// resolution and arms the Step-3 driver (which retries the resolution
/// once the actor tree exists and fires the one-shot silent-start
/// adjust). Scene 25/26 entries refresh the bind-time pre-shift.
pub(super) fn on_scene_change(prev: i32, next: i32) {
    super::on_scene_for_pre_shift(next);
    if next == scene::SONG_SELECT {
        super::driver::on_select_entry();
    } else if prev == scene::SONG_SELECT {
        super::driver::on_select_exit();
    }
    if next == scene::GAMEPLAY {
        clear_session_state("song change");
        // Row-derived resolution is pending only while the mod is live AND
        // some side's LOOP SONG row reads ON (2026-09-04 revision): a
        // section is only playable as a loop, so SONG START/END TIME —
        // now LOOP SONG's children, RETAINED while hidden — alter nothing
        // on their own. LOOP OFF ⇒ zero footprint (the driver never arms;
        // gesture markers are loop-gated too). LOOP ON ⇒ the resolution
        // is where the loop latches, and a loop session must arm the
        // driver even with no bounds set (it loops the whole song).
        let rows_engaged = (0..2).any(row_loop_song);
        if GESTURES_ACTIVE.load(Ordering::Acquire) && rows_engaged {
            RESOLUTION_PENDING.store(true, Ordering::Release);
        }
        super::driver::on_gameplay_entry();
    } else if prev == scene::GAMEPLAY {
        super::driver::on_gameplay_exit();
        clear_session_state("song change");
    }
}
