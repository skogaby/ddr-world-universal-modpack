//! The Training Mode per-frame driver (v1, Steps 3–4 — design §4.3): a
//! render-thread self-requeueing callback (the shipped `song_reset`
//! driver pattern), generation-tokened per song, armed at GAMEPLAY entry
//! only when there is work — the row-derived bound resolution
//! (`bounds::try_resolve_row_bounds` cannot complete at the scene-change
//! instant; the actor tree does not exist yet), the R15 one-shot
//! silent-start adjust when a bind-time pre-shift was requested, and the
//! Step-4 LOOP SONG leg while the loop is latched.
//!
//! The silent start's mechanism: the binding was created ALREADY SHIFTED
//! (Step 1's `BindContext.initial_mapping_ms` — the audio serves `lead`
//! silence then content from A; the true beginning is never decoded), the
//! natural start sequence ran untouched (READY panel, cue play, DPS state
//! 6's own `0x1044 {now}` anchored at content 0), and on the FIRST
//! anchored frame this driver fires ONE synchronous
//! [`song_reset::adjust_run_to`] — anchor `{now − wall(A) + lead}` +
//! record rebuild at A + freeze neutralization, NO cue stop/replay, NO
//! accumulator zeroing (the run just started). The adjust's target is
//! derived from the LIVE binding's applied mapping (blocks → wall ms →
//! content), so clock, audio, and claps land on the same served block
//! regardless of the arm-time desired-vs-committed rate epsilon.
//!
//! The loop leg (design §4.3, as amended 2026-08-14): while LOOP is
//! latched, `bounds` has PARKED the end cascade (the CMA `+0x94` display
//! threshold raised out of reach — `0x104A` is one-way song-scoped
//! state that strikes the lane furniture and breaks freeze scoring on
//! later passes, so it must never fire mid-grind); each frame compares
//! the raw music count against `min(b_live, +0x98 − margin)` (the
//! margin guards the stock thresholds only — the loop fires AT the
//! user's marker, 2026-08-15 re-demo amendment) and fires
//! the shipped `request_reset(a_live, TRAINING_LEAD_MS, Zero, None)`
//! back to the section start, indefinitely, until
//! quick-fail/quick-restart or song exit — every pass plays and scores
//! the full section. The loop also BYPASSES DEATH (Step-7 amendment
//! 2026-08-15): `bounds` armed the actors' instant-death gate at the
//! latch, so a gauge empty latches `m_isDead` without ending the run;
//! this driver detects it per frame and fires the same reset early —
//! the completion block's flag clear + gauge restore is the revive
//! (quick-fail/quick-restart remain the deliberate exits, and a
//! disarm restores stock death). One in-flight reset at a time (a
//! cooling latch until the count rewinds below the bound — which also
//! absorbs the stop/replay prepare window, where the pre-completion
//! anchor keeps counting); a refused iteration retries once next frame,
//! then disarms for the song with one WARN and UN-PARKS the cascade
//! (design §6 — the song can still end naturally).
//!
//! Fallback ladder (design §6): pre-shift missed with a live binding, or
//! the adjust refused ⇒ one WARN + a stop/replay seek
//! (`request_reset(a_ms, TRAINING_LEAD_MS, Zero, None)` — brief
//! true-beginning audibility); no binding at all, or the seek refusing
//! too ⇒ the song simply plays from 0 (nothing is broken).

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::time::Instant;

use super::bounds;
use super::section_math;
use super::TRAINING_LEAD_MS;
use crate::services::song_reset::{self, seek, AccumulatorPolicy, ResetOutcome};
use crate::services::{scene_manager, song_rate, widget_renderer};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

/// Give up requeueing after this long (a run that never anchors — e.g. a
/// wedged loader — must not requeue forever; scene exit normally kills
/// the loop long before this). Scoped to the PRE-ANCHOR phase only: a
/// live armed LOOP leg is exempt — a grind session is legitimately much
/// longer than any startup stall.
const DRIVER_TIMEOUT_SECS: u64 = 60;

/// How far the loop fire bound stays below every live STOCK end
/// threshold — the existing 1000 ms end-margin class
/// (`SEEK_END_MARGIN_MS`, the marker clamp); also covers the
/// ~150–300 ms stop/replay prepare window during which the
/// pre-completion anchor keeps counting. Applies to the threshold terms
/// only — the user's section end fires exactly (2026-08-15 re-demo
/// finding: B at 70 s looped at ~69 s under the subtract-from-min shape).
const LOOP_FIRE_MARGIN_MS: i32 = 1_000;

/// Song-scoped generation token: bumped at every GAMEPLAY entry AND exit,
/// so a queued step from a previous song self-cancels.
static GENERATION: AtomicUsize = AtomicUsize::new(0);
/// One-shot latch: the silent-start adjust (or its fallback) already ran
/// for this song.
static ADJUST_DONE: AtomicBool = AtomicBool::new(false);
/// Select-scene generation token (the highlight seeder's loop lifetime).
static SELECT_GENERATION: AtomicUsize = AtomicUsize::new(0);

// ── Loop-leg state (Step 4; reset at every GAMEPLAY entry) ───────────
/// One-way per-song disarm (degenerate section / double refusal): the
/// loop leg is dead for the song, the stock (or task-02-truncated)
/// thresholds end it.
static LOOP_DISARMED: AtomicBool = AtomicBool::new(false);
/// The computed fire bound (raw ms; 0 = not computed yet).
static LOOP_BOUND_MS: AtomicI32 = AtomicI32::new(0);
/// The live section end the bound was computed FROM (0 = none; −1 = no
/// compute yet) — a mid-grind B gesture surfaces as a changed value and
/// triggers a recompute (task req 3: B changes update the NEXT
/// iteration; A is read live at fire time and does not shape the bound).
static LOOP_BOUND_FROM_B: AtomicI32 = AtomicI32::new(-1);
/// One in-flight reset at a time: set at `Started`, cleared when the
/// observed count rewinds below the bound (completion rewinds the
/// anchor; the prepare window's still-climbing count stays above it).
static LOOP_COOLING: AtomicBool = AtomicBool::new(false);
/// A refused iteration retries exactly once (the next frame).
static LOOP_RETRY_USED: AtomicBool = AtomicBool::new(false);
/// One-per-song latch for the dropped display-term WARN.
static LOOP_T94_WARNED: AtomicBool = AtomicBool::new(false);

/// What the loop leg is doing this frame — the driver's requeue/timeout
/// input. Three-state because the pre-anchor wait must stay under the
/// soft timeout while a live grind must not (cabinet finding
/// 2026-08-14: the resolution completes seconds BEFORE the run anchors,
/// and the unanchored `+0x178` count reads as the raw frame tick — the
/// initial bound compute must wait for `first_anchored_frame`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum LoopState {
    /// Not latched, or disarmed for the song — the driver may exit.
    Idle,
    /// Latched, but the run has not anchored yet (no meaningful music
    /// count) — keep requeueing; the pre-anchor timeout APPLIES.
    AwaitingAnchor,
    /// The grind is live (fire bound computed) — keep requeueing,
    /// exempt from the soft timeout.
    Grinding,
}

/// Whether a loop-driver reset is in flight (the cooling latch: set at
/// `Started`, cleared once the observed count rewinds below the fire
/// bound) — the Step-7 scrub's yield check (one in-flight transaction
/// total across scrub + loop; concurrent dispatch is refusal-prone and
/// pointless).
pub(super) fn loop_reset_in_flight() -> bool {
    LOOP_COOLING.load(Ordering::Acquire)
}

/// SONG_SELECT-entry hook (R2 second amendment 2026-08-14 — song-scoped
/// bounds): start the per-frame highlight watcher. Whenever the
/// wheel-settle publication names a song the rows are not stamped for,
/// the rows re-seed to THAT song's timeline (START 0, END = its rounded
/// length) — the player opens the options menu and sees the highlighted
/// song's honest ending timestamp, and an untouched menu can never carry
/// one song's bounds into another.
pub(super) fn on_select_entry() {
    let generation = SELECT_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    if !widget_renderer::is_available() {
        return;
    }
    select_step(generation);
}

/// SONG_SELECT-exit hook: supersede the highlight watcher.
pub(super) fn on_select_exit() {
    SELECT_GENERATION.fetch_add(1, Ordering::AcqRel);
}

/// One highlight-watcher frame: one atomic-cell read per frame (the
/// publication seqlock), a seed only when the highlighted song actually
/// changed. Requeues itself for the lifetime of the select scene.
fn select_step(generation: usize) {
    widget_renderer::run_on_render_thread(move || {
        if SELECT_GENERATION.load(Ordering::Acquire) != generation {
            return; // superseded by a scene change
        }
        if scene_manager::current_scene() != scene::SONG_SELECT {
            return;
        }
        // Length sources, preferred first (2026-08-16 amendment):
        //
        // 1. The chart-length service — chart-derived (last event through
        //    the tempo chunk), available ~a frame after the wheel moves
        //    (music_wheel_song_length's selection poll drives requests).
        //    Tighter AND faster than audio: seed_end lands at the real
        //    playable end, so the row ranges and everything scaled from
        //    the seeded end (section previews, timeline scaling) match
        //    the chart instead of the audio tail.
        // 2. The wheel-settle audio publication — the original source;
        //    covers chart-parse failures and boots where the wheel mod is
        //    disabled (no service requests ⇒ `latest()` stays stale and
        //    the digest gate ignores it).
        //
        // Both stamp the SAME digest space (`song_code_digest` of the
        // song code), so the gameplay-entry coherence gate is unchanged.
        let chart = crate::services::chart_length::latest()
            .and_then(|l| l.secs.map(|s| (l.code_digest, s)));
        let seeded = match chart {
            Some((digest, secs)) if digest != bounds::rows_digest() => {
                let len_ms = secs.saturating_mul(1_000);
                super::seed_rows_for_highlight(digest, len_ms);
                log_info!(
                    "TrainingMode: bounds seeded from chart length -- end {} s (chart {} s)",
                    super::section_math::seed_end_seconds(len_ms),
                    secs
                );
                true
            }
            _ => false,
        };
        if !seeded {
            // Audio fallback ONLY when no chart publication exists at all
            // (wheel mod disabled ⇒ no requests; or nothing parsed yet).
            // NEVER when a chart publication is present with a different
            // digest: the two sources each stamping their own digest turns
            // a persistent publication skew (stale audio publication vs a
            // live chart one) into a per-frame seed ping-pong — 2 bound
            // rewrites + 4 row writes + a pre-shift refresh per frame,
            // which wedged a live cabinet (2026-08-25, 66k seeds). A
            // present-but-mismatched chart is transient: the wheel poll
            // re-requests and the chart arm converges on its own.
            if chart.is_none() {
                if let Some(info) = song_rate::selected_song::selected_song() {
                    if info.code_digest != bounds::rows_digest() {
                        super::seed_rows_for_highlight(info.code_digest, info.audio_len_ms);
                        log_info!(
                        "TrainingMode: bounds seeded for the highlighted song -- end {} s (len {} ms)",
                        super::section_math::seed_end_seconds(info.audio_len_ms),
                        info.audio_len_ms
                    );
                    }
                }
            }
        }
        select_step(generation);
    });
}

/// GAMEPLAY-entry hook (called from `bounds::on_scene_change` AFTER the
/// session state reset): arm the driver when there is work — a pending
/// row resolution or a requested pre-shift. Zero rows and no pre-shift ⇒
/// the driver never arms (the mod's zero-footprint requirement).
pub(super) fn on_gameplay_entry() {
    let generation = GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    ADJUST_DONE.store(false, Ordering::Release);
    LOOP_DISARMED.store(false, Ordering::Release);
    LOOP_BOUND_MS.store(0, Ordering::Release);
    LOOP_BOUND_FROM_B.store(-1, Ordering::Release);
    LOOP_COOLING.store(false, Ordering::Release);
    LOOP_RETRY_USED.store(false, Ordering::Release);
    LOOP_T94_WARNED.store(false, Ordering::Release);
    let (shift_ms, _) = song_rate::runtime::initial_content_mapping_ms();
    let resolution_pending = !bounds::try_resolve_row_bounds();
    if shift_ms == 0 && !resolution_pending {
        return;
    }
    if !widget_renderer::is_available() {
        log_warn!("TrainingMode: widget renderer unavailable -- driver cannot arm");
        return;
    }
    log_info!(
        "TrainingMode: driver armed (pre-shift {} ms, bounds resolution {})",
        shift_ms,
        if resolution_pending {
            "pending"
        } else {
            "settled"
        }
    );
    step(generation, Instant::now());
}

/// GAMEPLAY-exit hook: supersede any queued step.
pub(super) fn on_gameplay_exit() {
    GENERATION.fetch_add(1, Ordering::AcqRel);
}

/// One driver frame: retry the row-bound resolution until the actor tree
/// exists, then — when a pre-shift was requested — wait for the first
/// anchored frame and fire the one-shot adjust; then the Step-4 loop leg
/// while LOOP is latched. Requeues itself until all work is done,
/// superseded, or (pre-anchor work only) timed out — a live armed loop
/// is exempt from the soft timeout, since a grind session is
/// legitimately long.
fn step(generation: usize, started: Instant) {
    widget_renderer::run_on_render_thread(move || {
        if GENERATION.load(Ordering::Acquire) != generation {
            return; // superseded by a scene change / newer song
        }
        if scene_manager::current_scene() != scene::GAMEPLAY {
            return;
        }

        let resolved = bounds::try_resolve_row_bounds();

        let (shift_ms, _) = song_rate::runtime::initial_content_mapping_ms();
        // A shift request stamped for a DIFFERENT song than the one
        // playing is the fast-confirm race: the bind already declined the
        // mapping and the resolution declined the rows — skip the adjust
        // outright rather than walking the fallback ladder's WARNs for a
        // start that was never meant for this song.
        let stamp_coherent = song_rate::selected_song::digests_coherent(
            bounds::rows_digest(),
            song_rate::selected_song::selected_song().map(|info| info.code_digest),
        );
        let adjust_pending = shift_ms > 0 && stamp_coherent && !ADJUST_DONE.load(Ordering::Acquire);
        if adjust_pending && song_reset::first_anchored_frame() {
            ADJUST_DONE.store(true, Ordering::Release);
            silent_start_adjust();
            // The resolution (if somehow still unsettled) keeps its own
            // retries below; the adjust itself is strictly once per song.
        }

        // The Step-4 loop leg: only after the resolution settled (the
        // loop latch is taken there, and the bound compute needs the
        // live actor tree the resolution just proved).
        let loop_state = if resolved {
            loop_step()
        } else {
            LoopState::Idle
        };

        // An incoherent stamp also counts as "nothing outstanding" — the
        // adjust is permanently skipped for this song, so the loop must
        // not idle until the timeout waiting for it.
        let adjust_outstanding =
            shift_ms > 0 && stamp_coherent && !ADJUST_DONE.load(Ordering::Acquire);
        if resolved && !adjust_outstanding && loop_state == LoopState::Idle {
            return; // all driver work done for this song
        }
        if loop_state != LoopState::Grinding && started.elapsed().as_secs() >= DRIVER_TIMEOUT_SECS {
            // Pre-anchor work (resolution / adjust / anchor wait)
            // wedged — give up.
            log_warn!(
                "TrainingMode: driver timed out after {} s (run never anchored?) -- disarming",
                DRIVER_TIMEOUT_SECS
            );
            return;
        }
        step(generation, started);
    });
}

/// One frame of the LOOP SONG leg (design §4.3). [`LoopState::Idle`]
/// keeps the driver's exit condition exactly Step 3's for LOOP OFF
/// sessions (zero footprint) and after a disarm.
fn loop_step() -> LoopState {
    if !bounds::loop_latched() || LOOP_DISARMED.load(Ordering::Acquire) {
        return LoopState::Idle;
    }
    // The initial bound compute must wait for the run's FIRST anchored
    // frame with a CREDIBLE count: the resolution completes on the actor
    // tree seconds before DPS state 6 anchors the clock, and until the
    // anchor at `+0x160` lands the `+0x178` count reads as the raw frame
    // tick (minutes-since-boot scale — cabinet finding 2026-08-14; the
    // per-frame cache can hold it one frame past the anchor too), which
    // would trip the degeneracy disarm at song start. Both checks are the
    // shared `song_reset::run_in_song` predicate — the same gate the
    // training gestures use (2026-09-04 revision).
    let initial = LOOP_BOUND_FROM_B.load(Ordering::Acquire) == -1;
    if initial && !song_reset::run_in_song() {
        return LoopState::AwaitingAnchor;
    }
    // (Re)compute the fire bound when none exists yet or the live
    // section end moved (a mid-grind B gesture / press-5 clear). A
    // moved via gestures needs no recompute — it is read live at fire
    // time and does not shape the bound.
    let b_live = bounds::section_end();
    let b_key = b_live.unwrap_or(0);
    if initial || LOOP_BOUND_FROM_B.load(Ordering::Acquire) != b_key {
        if !compute_fire_bound(b_live, initial) {
            return LoopState::Idle; // disarmed (degenerate / no thresholds)
        }
        LOOP_BOUND_FROM_B.store(b_key, Ordering::Release);
    }
    let bound = LOOP_BOUND_MS.load(Ordering::Acquire);
    let Some(count) = song_reset::current_raw_music_count() else {
        return LoopState::Grinding; // no live count this frame — retry
    };
    if LOOP_COOLING.load(Ordering::Acquire) {
        // One in-flight reset at a time: the completed reset rewinds the
        // count below the bound (the prepare window's count still climbs
        // and stays absorbed here).
        if count < bound {
            LOOP_COOLING.store(false, Ordering::Release);
        }
        return LoopState::Grinding;
    }
    if song_reset::reset_in_flight() {
        // A non-loop transaction (a scrub) is mid-flight: one in-flight
        // transaction total — defer both fire conditions to a later frame.
        return LoopState::Grinding;
    }
    // Death under the loop (Step-7 amendment 2026-08-15): the loop latch
    // armed the actors' +0x2B7 gate, so a gauge death latches m_isDead
    // WITHOUT ending the run (both the STEP_GAME_OVER advance and the
    // DPS finish-poll are conditioned on the gate) — fire the loop back
    // to A immediately; the reset's completion block clears the death
    // flags and restores the gauge from the snapshot (the revive). A
    // refused fire walks the existing retry/disarm ladder, whose disarm
    // restores the gate — the still-latched death then fails the song
    // out naturally (stock behavior as the fallback).
    let died = song_reset::any_actor_dead();
    if !died && count < bound {
        return LoopState::Grinding;
    }
    // Fire: back to the LIVE section start (row-derived or
    // gesture-refined; none ⇒ 0 — the binding-free whole-song restart,
    // breakdown decision #2).
    let target = bounds::active_section_start().unwrap_or(0);
    match song_reset::request_reset(
        target,
        TRAINING_LEAD_MS as i32,
        AccumulatorPolicy::Zero,
        None,
    ) {
        ResetOutcome::Started => {
            LOOP_COOLING.store(true, Ordering::Release);
            LOOP_RETRY_USED.store(false, Ordering::Release);
            log_info!(
                "TrainingMode: loop iteration ({}) -- reset to {} ms at count {} ms (fire bound {} ms)",
                if died { "death revive" } else { "section end" },
                target,
                count,
                bound
            );
            LoopState::Grinding
        }
        outcome => {
            // Refused (or the legacy Unsupported): retry exactly once on
            // the next frame, then disarm with one WARN — the cascade is
            // un-parked so the song continues to its (threshold-
            // truncated or natural) end.
            if LOOP_RETRY_USED.swap(true, Ordering::AcqRel) {
                disarm_loop(&format!("loop reset refused twice ({outcome:?})"));
                LoopState::Idle
            } else {
                LoopState::Grinding
            }
        }
    }
}

/// Compute the loop fire bound from the live thresholds and section end
/// ([`section_math::loop_fire_bound`]).
///
/// - **Cascade parked** (the normal case — `bounds` raised `+0x94` at
///   the loop latch, so `0x104A`/`0x104B` can never fire mid-grind):
///   `min(b_live, +0x98 − margin)`, both reset paths. `+0x98` is live
///   but STOCK (the raise keeps it honest), so the whole chart plays
///   and scores on every pass.
/// - **Raise failed** (WARN-once ladder): the conservative Step-4
///   original — `min(b_live, min(raw(+0x94), +0x98) − margin)` on EVERY
///   path, keeping the cascade unfired at the cost of the last
///   `margin` of chart (`0x104A` is one-way song-scoped state that
///   strikes the lane furniture and breaks freeze scoring on later
///   passes — never let it fire under a loop).
///
/// On the INITIAL compute a degenerate section (bound ≤ 0, already
/// behind the count, or a section start at/above the bound) disarms
/// with one WARN; recomputes accept a bound behind the count — a
/// mid-grind B set behind the cursor simply loops on the next frame
/// (the "end here" class).
fn compute_fire_bound(b_live: Option<i32>, initial: bool) -> bool {
    // Fold BOTH sides' live thresholds (versus-training lift: each side
    // plays its own chart, so the bound must respect the shorter one —
    // the shipped seek gate's own MIN-across-sides pattern). Solo/doubles
    // degrade to the single live side exactly as before.
    let mut t98_min: Option<i32> = None;
    let mut t94_min: Option<i32> = None;
    let mut conversion_missed = false;
    let raised = bounds::loop_thresholds_raised();
    for side in 0..2 {
        let Some((t94_display, t98_raw)) = song_reset::chart_end_thresholds(side) else {
            continue;
        };
        t98_min = Some(t98_min.map_or(t98_raw, |current: i32| current.min(t98_raw)));
        if !raised {
            // The cascade is live — convert THIS side's display threshold
            // through its own note vector.
            match song_reset::decoded_notes(side)
                .filter(|notes| !notes.is_empty())
                .and_then(|notes| seek::raw_for_display(&notes, t94_display))
            {
                Some(raw) => t94_min = Some(t94_min.map_or(raw, |current: i32| current.min(raw))),
                None => conversion_missed = true,
            }
        }
    }
    let Some(t98_raw) = t98_min else {
        disarm_loop("end thresholds unreadable");
        return false;
    };
    let t94_raw = if raised {
        // The cascade is parked — no display-threshold term.
        None
    } else if conversion_missed {
        // ANY side failing conversion degrades the whole term (a partial
        // min could exceed the failed side's true threshold and let its
        // 0x104A fire mid-grind — same accepted degradation as the
        // shipped single-side conversion failure).
        if !LOOP_T94_WARNED.swap(true, Ordering::AcqRel) {
            log_warn!(
                "TrainingMode: display threshold conversion failed -- loop bound clamps on the raw threshold only (a one-shot loop is possible)"
            );
        }
        None
    } else {
        t94_min
    };
    let Some(bound) = section_math::loop_fire_bound(b_live, t94_raw, t98_raw, LOOP_FIRE_MARGIN_MS)
    else {
        disarm_loop("degenerate section (fire bound <= 0)");
        return false;
    };
    if initial {
        let count = song_reset::current_raw_music_count().unwrap_or(0);
        let a_live = bounds::active_section_start().unwrap_or(0);
        if count >= bound || a_live >= bound {
            disarm_loop("degenerate section (count/start at or past the fire bound)");
            return false;
        }
    }
    LOOP_BOUND_MS.store(bound, Ordering::Release);
    log_info!(
        "TrainingMode: loop fire bound {} ms (b {:?} ms, display-threshold raw {:?} ms, raw threshold {} ms, margin {} ms, cascade {})",
        bound,
        b_live,
        t94_raw,
        t98_raw,
        LOOP_FIRE_MARGIN_MS,
        if bounds::loop_thresholds_raised() {
            "parked"
        } else {
            "live (conservative bound)"
        }
    );
    true
}

/// Disarm the loop for the song with one WARN, restoring the parked
/// cascade so the run can still end naturally
/// ([`bounds::on_loop_disarmed`] — mandatory: a raised `+0x94` with no
/// loop to fire would soft-lock the song at its end).
fn disarm_loop(why: &str) {
    LOOP_DISARMED.store(true, Ordering::Release);
    log_warn!("TrainingMode: {} -- loop disarmed for this song", why);
    bounds::on_loop_disarmed();
}

/// The one-shot silent-start completion (design §4.3 + the §6 ladder).
/// Derives the adjust target from the LIVE binding's applied mapping and
/// fires `adjust_run_to`; every degraded path falls through the ladder.
fn silent_start_adjust() {
    // The pre-shift must actually be applied to the live binding (the
    // expectation check — a refused bind, or a bind that raced the arm,
    // leaves the mapping empty).
    let mapping = song_rate::runtime::active_content_mapping();
    let Some((shift_blocks, lead_blocks)) = mapping else {
        // No live binding at all: a fallback seek needs the same binding,
        // so there is no silent start to deliver — the song plays from 0
        // (design §6: "no binding ⇒ no silent start, song plays normally").
        log_warn!(
            "TrainingMode: silent start unavailable -- no live song-rate binding (song plays from the top)"
        );
        return;
    };
    if shift_blocks == 0 {
        // Binding live but unshifted: the pre-shift missed the bind.
        log_warn!(
            "TrainingMode: pre-shift missed the bind -- falling back to a stop/replay seek (brief true-beginning audibility)"
        );
        fallback_seek();
        return;
    }
    // blocks → wall ms (the served grid) → content ms (the committed
    // rate): the exact position the audio is already serving from.
    let Some(grid) = song_rate::runtime::active_content_grid() else {
        log_warn!("TrainingMode: content grid unavailable -- falling back to a stop/replay seek");
        fallback_seek();
        return;
    };
    let snapshot = song_rate::clock_patch::snapshot();
    let (Some(shift_wall_ms), Some(lead_wall_ms)) = (
        seek::blocks_to_wall_ms(shift_blocks, grid.samples_per_block, grid.sample_rate),
        seek::blocks_to_wall_ms(lead_blocks, grid.samples_per_block, grid.sample_rate),
    ) else {
        log_warn!("TrainingMode: degenerate served grid -- falling back to a stop/replay seek");
        fallback_seek();
        return;
    };
    let t_q = seek::content_ms(shift_wall_ms, &snapshot);
    if song_reset::adjust_run_to(t_q, lead_wall_ms.max(0) as u64) {
        log_info!(
            "TrainingMode: silent skip-first start adjusted -- content A={} ms (lead {} ms; true beginning never decoded)",
            t_q,
            lead_wall_ms
        );
    } else {
        log_warn!(
            "TrainingMode: silent-start adjust refused -- falling back to a stop/replay seek"
        );
        fallback_seek();
    }
}

/// The design §6 fallback: a stop/replay seek to the row-derived section
/// start (brief true-beginning audibility — R15 is violated only on this
/// degraded path). The seek refusing too leaves the song playing from 0.
fn fallback_seek() {
    let (row_a, _) = bounds::row_derived_bounds();
    if row_a <= 0 {
        log_warn!("TrainingMode: no row-derived section start -- song plays from the top");
        return;
    }
    match song_reset::request_reset(
        row_a,
        TRAINING_LEAD_MS as i32,
        AccumulatorPolicy::Zero,
        None,
    ) {
        ResetOutcome::Started => {}
        outcome => {
            log_warn!(
                "TrainingMode: fallback seek refused ({:?}) -- song plays from the top",
                outcome
            );
        }
    }
}
