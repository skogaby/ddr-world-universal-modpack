//! Pure section-bound math (Training Mode v1, Step 3 — design §4.2):
//! the select-time effective clamp against the published audio length and
//! the gameplay-entry row-derived bound resolution against the live chart
//! end. Content (raw-ms) domain throughout; block quantization is the
//! caller's (it needs the live binding's grid — `bounds::quantize_marker`).
//!
//! Host-tested through the harness mount (this module is a dependency-free
//! leaf on purpose — `bounds.rs` itself walks live game state and cannot
//! compile on the host).

/// The minimum section length (design decision, maintainer-approved
/// 2026-08-13): `b_ms` is floored at `a_ms + MIN_SECTION_MS` so a section
/// can never collapse below five seconds — except at the very end of the
/// chart, where the chart-end cap wins and the section end degenerates to
/// "none" (natural chart end).
pub const MIN_SECTION_MS: i32 = 5_000;

/// [`MIN_SECTION_MS`] in the bound rows' seconds domain — the row-level
/// nudge distance ([`nudge_end_after_start`]/[`nudge_start_after_end`]).
pub const MIN_SECTION_S: i32 = MIN_SECTION_MS / 1_000;

/// The bound rows' shared range cap (R2 amendment 2026-08-14: both rows
/// are absolute timestamps, 0–200 s per the maintainer — no DDR chart
/// runs longer; raise if a marathon custom bank ever needs it). An END
/// TIME at this cap is the "natural end" sentinel — the row cannot
/// express an end past 3:20, so the cap universally means "play to the
/// song's own end" (also the row's registration default; the highlight
/// seeder then re-bounds the ROW ITSELF to the song's length, so the
/// live max is normally the song end, not this cap).
pub const BOUND_ROW_MAX_S: i32 = 200;

/// The bound rows' fine step (seconds) — the stepper's nudge distance
/// only. The highlight-time END seed is deliberately OFF this grid
/// (whole-second chart length, [`seed_end_seconds`]); the stepper is
/// add-then-clamp, so an off-grid max stays reachable.
pub const BOUND_ROW_STEP_S: i32 = 5;

/// Resolved row-derived section bounds, content domain, unquantized.
/// `0` = none for both fields (design §5's `b == chart_end` sentinel is
/// normalized to `0` here so every consumer keeps the `> 0` = engaged
/// convention the markers already use).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedBounds {
    /// Section start (raw ms; 0 = none — play from the chart start).
    pub a_ms: i32,
    /// Section end (raw ms; 0 = none — play to the natural chart end).
    pub b_ms: i32,
}

/// The select-time effective clamp (design §4.2/R2): cap a bound row's
/// seconds at the highlighted song's audio length when a publication
/// exists. Applied at USE time — the row's stored value is never
/// rewritten, so switching to a longer song restores the full value.
/// `None` (no publication yet / parse failed at select) skips the audio
/// cap entirely; the chart-derived runtime clamp still protects.
///
/// Applies to the START row only (the pre-shift arm + the resolution's
/// `a`): flooring the END row to whole audio seconds could land just
/// below `chart_end_raw` and fabricate a phantom section end — END is
/// governed solely by the chart-end normalization in [`resolve_bounds`].
#[must_use]
pub fn effective_bound_seconds(row_seconds: i32, audio_len_ms: Option<u32>) -> i32 {
    let row_seconds = row_seconds.max(0);
    match audio_len_ms {
        Some(len_ms) => row_seconds.min((len_ms / 1_000).min(i32::MAX as u32) as i32),
        None => row_seconds,
    }
}

/// The highlight-time seed value for the END row (UX amendment
/// 2026-08-18, superseding the R2 second amendment's 5 s grid): the
/// song's length rounded UP to the next whole second, capped at the row
/// max. On the chart-derived path this is EXACTLY the value the music
/// wheel's LENGTH readout shows (`chart_length` already publishes
/// ceil-to-second), so the END row clamps at the same number the player
/// just read off the wheel — the old pad-to-5 s seed showed an ending
/// past the song, which read as confusing on the cabinet. Still always
/// at/above the real end — a seeded (or untouched) END never truncates
/// the song. Songs longer than the row cap seed AT the cap, which is the
/// "natural end" sentinel [`resolve_bounds`] never converts into a
/// section end.
#[must_use]
pub fn seed_end_seconds(audio_len_ms: u32) -> i32 {
    audio_len_ms.div_ceil(1_000).min(BOUND_ROW_MAX_S as u32) as i32
}

/// Row-level coupling, START edited (R2 amendment 2026-08-14): the END
/// value that keeps the play window at least [`MIN_SECTION_S`] — END is
/// nudged UP to `start + MIN_SECTION_S` (capped at the row max) when the
/// edit closed the window; otherwise unchanged. At the extreme cap the
/// window may fall short (both rows pinned at the cap) — the runtime
/// resolution's floor remains authoritative.
#[must_use]
pub fn nudge_end_after_start(start_s: i32, end_s: i32) -> i32 {
    if end_s < start_s.saturating_add(MIN_SECTION_S) {
        start_s.saturating_add(MIN_SECTION_S).min(BOUND_ROW_MAX_S)
    } else {
        end_s
    }
}

/// Row-level coupling, END edited: the START value that keeps the play
/// window at least [`MIN_SECTION_S`] — START is bumped DOWN to
/// `end − MIN_SECTION_S` (floored at 0) when the edit closed the window;
/// otherwise unchanged. The pair converges in one step
/// (`nudge_start_after_end(s, nudge_end_after_start(s, e))` never moves).
#[must_use]
pub fn nudge_start_after_end(start_s: i32, end_s: i32) -> i32 {
    if end_s < start_s.saturating_add(MIN_SECTION_S) {
        end_s.saturating_sub(MIN_SECTION_S).max(0)
    } else {
        start_s
    }
}

/// The arm-time content→wall conversion for the bind-time pre-shift
/// (design §4.3/R15): at scene 25/26 no binding or committed rate exists
/// yet, so the DESIRED percent is the only rate available —
/// `wall = content · 100 / percent` (a 50 % song stretches content to
/// twice the wall time; identity is exact). The committed rate is
/// block-quantized and differs by a sub-block epsilon; that never reaches
/// the clock anchor because the Step-3 adjust re-derives its target from
/// the LIVE binding's applied mapping. Non-positive percents are treated
/// as identity (defensive — the SONG SPEED row clamps to 25..=175).
#[must_use]
pub fn pre_shift_wall_ms(content_ms: u64, desired_percent: i32) -> u64 {
    if desired_percent <= 0 || desired_percent == 100 {
        return content_ms;
    }
    content_ms.saturating_mul(100) / desired_percent as u64
}

/// What the song-end machinery does for the latched loop state and
/// resolved section end (Step 4, design §4.2/§4.3) — the SINGLE decision
/// point between the LOOP OFF threshold write and the LOOP ON driver
/// loop, mutually exclusive by construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndPolicy {
    /// LOOP OFF with a live section end: write the ControlMessageActor
    /// end thresholds to `b_ms` (raw + converted display) — the game
    /// runs its own stock tail early (banner → results, partial stats).
    WriteThresholds {
        /// The resolved section end (raw ms, > 0).
        b_ms: i32,
    },
    /// LOOP ON: the end cascade must NEVER fire while looping — `0x104A`
    /// (chart content over) is one-way song-scoped state that strikes
    /// the lane furniture and breaks freeze scoring on later passes
    /// (cabinet finding 2026-08-14). The apply RAISES the `+0x94`
    /// display threshold out of reach (stock pair stashed) and the loop
    /// driver fires below `+0x98`. Applies with or without a section
    /// end — loop-ON alone loops the whole song.
    ArmLoop,
    /// LOOP OFF without a section end: the stock natural end — nothing
    /// to do.
    Natural,
}

/// The end policy for `{loop_on, b_ms}` (`b_ms` uses the resolved-bound
/// sentinel: `<= 0` = no section end). Total and exclusive — exactly one
/// variant per input, and `loop_on` never yields `WriteThresholds`.
#[must_use]
pub fn end_policy(loop_on: bool, b_ms: i32) -> EndPolicy {
    if loop_on {
        EndPolicy::ArmLoop
    } else if b_ms > 0 {
        EndPolicy::WriteThresholds { b_ms }
    } else {
        EndPolicy::Natural
    }
}

/// What the LOOP OFF threshold-apply state machine does for one policy
/// evaluation (design §4.2 write points: resolution, gesture B-set,
/// press-5 clear — plus §5's `thresholds_written` applied-state).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyAction {
    /// Write the CMA thresholds to `b_ms` (stash the stock pair first if
    /// this song has no stash yet). A changed b is the same action — the
    /// write is an idempotent overwrite.
    Write { b_ms: i32 },
    /// Park the end cascade for a loop session: stash the stock pair
    /// (once) and raise the `+0x94` display threshold out of reach so
    /// `0x104A` never fires mid-grind (`+0x98` stays stock — every other
    /// reader stays honest). Idempotent.
    RaiseThresholds,
    /// Restore the stashed stock thresholds (the section end cleared to
    /// none under LOOP OFF).
    Restore,
    /// Leave the thresholds alone.
    Nothing,
}

/// The apply-state transition table: `policy` × "were the thresholds
/// already written this song". `WriteThresholds` always writes
/// (rewrites are idempotent); `Natural` restores exactly when something
/// was written; `ArmLoop` always raises (idempotent — re-evaluations
/// from mid-grind gestures re-apply the same raise; the disarm/boundary
/// restores are driven by their owners, not this table).
#[must_use]
pub fn apply_action(policy: EndPolicy, thresholds_written: bool) -> ApplyAction {
    match policy {
        EndPolicy::WriteThresholds { b_ms } => ApplyAction::Write { b_ms },
        EndPolicy::ArmLoop => ApplyAction::RaiseThresholds,
        EndPolicy::Natural => {
            if thresholds_written {
                ApplyAction::Restore
            } else {
                ApplyAction::Nothing
            }
        }
    }
}

/// The LOOP ON fire bound (design §4.3, breakdown decision #1): the
/// raw-ms count at which the loop driver fires its reset —
/// `min(b_live, min(stock_display_raw, stock_raw) − margin)`, where
/// `b_live` is the live section end (`None` = loop the whole song — the
/// term drops), `stock_display_raw` is the stock CMA `+0x94` display
/// threshold converted to raw ms (`None` = the converter failed — the
/// term drops with the caller's WARN; the `stock_raw` clamp still
/// guards the fatal step-5 edge), and `stock_raw` is the stock `+0x98`
/// song-over threshold (always present — no thresholds, no loop).
///
/// The margin applies ONLY to the stock-threshold terms: it keeps the
/// bound strictly below BOTH thresholds (the end cascade is one-way and
/// the seek gate refuses at cascade step ≥ 4 — one late iteration would
/// permanently break seeking mid-grind) and covers the ~150–300 ms
/// stop/replay prepare window during which the pre-completion anchor
/// keeps counting. The user's marker needs no such guard — B is already
/// `< chart_end` by resolution, and the min keeps the threshold guard
/// authoritative — so the loop fires AT the marker, not a second early
/// (2026-08-15 re-demo finding: B at 70 s looped at ~69 s under the old
/// `min(...) − margin` shape). `None` = degenerate (bound ≤ 0) — the
/// caller disarms the loop.
#[must_use]
pub fn loop_fire_bound(
    b_live: Option<i32>,
    stock_display_raw: Option<i32>,
    stock_raw: i32,
    margin: i32,
) -> Option<i32> {
    let mut threshold = stock_raw;
    if let Some(display_raw) = stock_display_raw {
        threshold = threshold.min(display_raw);
    }
    let mut bound = threshold.saturating_sub(margin);
    if let Some(b) = b_live {
        bound = bound.min(b);
    }
    (bound > 0).then_some(bound)
}

/// The FF/RW scrub increments' default and normalize range (Step 7, the
/// amended R12): `training_mode.{ff,rw}_increment_ms` absent ⇒ the
/// default; out-of-range values clamp into the range (the caller logs one
/// INFO — the `restart_delay_ms` pattern).
pub const SCRUB_INCREMENT_DEFAULT_MS: i32 = 5_000;
pub const SCRUB_INCREMENT_MIN_MS: i32 = 250;
pub const SCRUB_INCREMENT_MAX_MS: i32 = 60_000;

/// Normalize a configured scrub increment: absent key/block ⇒ the
/// default; present ⇒ clamped to
/// [`SCRUB_INCREMENT_MIN_MS`]..=[`SCRUB_INCREMENT_MAX_MS`]. The caller
/// compares raw vs normalized for its one-INFO report.
#[must_use]
pub fn normalize_scrub_increment_ms(raw: Option<i32>) -> i32 {
    match raw {
        Some(v) => v.clamp(SCRUB_INCREMENT_MIN_MS, SCRUB_INCREMENT_MAX_MS),
        None => SCRUB_INCREMENT_DEFAULT_MS,
    }
}

/// The FF/RW scrub target (Step 7, the amended R12): `clamp(current +
/// delta, 0, min(b_live?, chart_end) − margin)`, content (raw-ms) domain,
/// unquantized — block quantization stays in the live layer
/// (`bounds::quantize_marker`, the marker-set split).
///
/// The margin applies AFTER the min, to BOTH terms (the plan's spelled
/// formula — deliberately not the `loop_fire_bound` stock-only shape):
/// the seek transaction's own gate refuses at `min_end − 1000`, and a
/// target exactly AT a live B would land on an instant end/loop fire, so
/// the clamp keeps every target strictly inside the transaction's
/// acceptance window on every path (under a LOOP OFF truncated end the
/// live `chart_end` already reads B, keeping the two aligned).
///
/// A clamped-to-0 target is the caller's "rewind past the start" signal
/// (dispatched as the plain t=0 restart). Degenerate inputs — no live
/// chart, or the bound collapsing to ≤ 0 (chart shorter than the margin,
/// degenerate B) — refuse with `None` (the caller's fail-open drop).
#[must_use]
pub fn scrub_target(
    current_ms: i32,
    delta_ms: i32,
    b_live: Option<i32>,
    chart_end_ms: i32,
    margin_ms: i32,
) -> Option<i32> {
    if chart_end_ms <= 0 {
        return None;
    }
    let mut end = chart_end_ms;
    if let Some(b) = b_live {
        end = end.min(b);
    }
    let bound = end.saturating_sub(margin_ms);
    if bound <= 0 {
        return None;
    }
    Some(current_ms.saturating_add(delta_ms).clamp(0, bound))
}

/// Gameplay-entry bound resolution (design §4.2, R2 timestamp amendment
/// 2026-08-14 — both rows are absolute timestamps):
/// `a = min(start·1000, chart_end − margin)`;
/// `b = clamp(end·1000, a + MIN_SECTION, chart_end)`, normalized to the
/// `0` sentinel (natural end) when it lands at/past the chart end.
/// START 0 resolves to no section start; END at the row cap
/// ([`BOUND_ROW_MAX_S`] — the "whole song" default) resolves to no
/// section end regardless of the chart. Non-positive `chart_end_ms`
/// yields no bounds (no live chart — nothing to bound).
#[must_use]
pub fn resolve_bounds(
    start_time_s: i32,
    end_time_s: i32,
    chart_end_ms: i32,
    margin_ms: i32,
) -> ResolvedBounds {
    const NONE: ResolvedBounds = ResolvedBounds { a_ms: 0, b_ms: 0 };
    if chart_end_ms <= 0 {
        return NONE;
    }
    let start_time_s = start_time_s.max(0);
    let end_time_s = end_time_s.max(0);

    let a_ms = if start_time_s == 0 {
        0
    } else {
        start_time_s
            .saturating_mul(1_000)
            .min(chart_end_ms.saturating_sub(margin_ms))
            .max(0)
    };

    let b_ms = if end_time_s >= BOUND_ROW_MAX_S {
        // The row cap is the "natural end" sentinel (the default).
        0
    } else {
        let target = end_time_s.saturating_mul(1_000);
        let floored = target.max(a_ms.saturating_add(MIN_SECTION_MS));
        let capped = floored.min(chart_end_ms);
        // At (or past) the chart end the section end is meaningless — the
        // song ends there anyway. Normalize to the "none" sentinel.
        if capped >= chart_end_ms {
            0
        } else {
            capped
        }
    };

    ResolvedBounds { a_ms, b_ms }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHART_END: i32 = 120_000;
    /// The existing 1000 ms end-margin class (`MARKER_END_MARGIN_MS`).
    const MARGIN: i32 = 1_000;

    #[test]
    fn effective_clamp_truncates_against_audio_length() {
        // START at the row cap on a 90 s song caps at the audio length
        // (AC 2).
        assert_eq!(effective_bound_seconds(200, Some(90_000)), 90);
        // A row inside the song is untouched.
        assert_eq!(effective_bound_seconds(30, Some(90_000)), 30);
        // No publication: the audio cap is skipped entirely.
        assert_eq!(effective_bound_seconds(200, None), 200);
        // Defensive: negative rows normalize to 0.
        assert_eq!(effective_bound_seconds(-5, Some(90_000)), 0);
    }

    #[test]
    fn default_rows_resolve_to_no_bounds() {
        // START 0 + END at the row cap (the defaults) = the whole song.
        assert_eq!(
            resolve_bounds(0, BOUND_ROW_MAX_S, CHART_END, MARGIN),
            ResolvedBounds { a_ms: 0, b_ms: 0 }
        );
    }

    #[test]
    fn nominal_timestamps_resolve_directly() {
        // START 1:00, END 1:30 — both literal timestamps.
        let bounds = resolve_bounds(60, 90, CHART_END, MARGIN);
        assert_eq!(bounds.a_ms, 60_000);
        assert_eq!(bounds.b_ms, 90_000);
    }

    #[test]
    fn start_past_end_caps_at_chart_end_minus_margin() {
        let bounds = resolve_bounds(199, BOUND_ROW_MAX_S, CHART_END, MARGIN);
        assert_eq!(bounds.a_ms, CHART_END - MARGIN);
        assert_eq!(bounds.b_ms, 0);
    }

    #[test]
    fn end_below_start_floors_at_min_section_above_a() {
        // END 0:05 under START 0:10 (rows normally can't express this —
        // the nudges keep them apart — but the resolution stays safe).
        let bounds = resolve_bounds(10, 5, CHART_END, MARGIN);
        assert_eq!(bounds.a_ms, 10_000);
        assert_eq!(bounds.b_ms, 10_000 + MIN_SECTION_MS);
    }

    #[test]
    fn end_only_keeps_a_at_none() {
        // START 0 (natural start) + END 1:30.
        let bounds = resolve_bounds(0, 90, CHART_END, MARGIN);
        assert_eq!(bounds.a_ms, 0);
        assert_eq!(bounds.b_ms, 90_000);
    }

    #[test]
    fn end_at_or_past_the_chart_end_is_natural() {
        // The song is 2:00; END 2:00 and END 4:00 both mean natural end.
        for end_s in [120, 150, 199] {
            let bounds = resolve_bounds(30, end_s, CHART_END, MARGIN);
            assert_eq!(bounds.a_ms, 30_000);
            assert_eq!(bounds.b_ms, 0, "END {end_s}s on a 120s song = natural end");
        }
    }

    #[test]
    fn min_section_floor_gives_way_at_the_chart_end() {
        // START lands 2 s before the end; a + MIN_SECTION overshoots the
        // chart end, so the section end degenerates to "none".
        let bounds = resolve_bounds(118, 119, CHART_END, MARGIN);
        assert_eq!(bounds.a_ms, 118_000);
        assert_eq!(bounds.b_ms, 0);
    }

    #[test]
    fn dead_chart_end_resolves_to_no_bounds() {
        assert_eq!(
            resolve_bounds(60, 90, 0, MARGIN),
            ResolvedBounds { a_ms: 0, b_ms: 0 }
        );
    }

    #[test]
    fn raising_start_nudges_end_up() {
        // START 90 against END 60: END follows to START + 5.
        assert_eq!(nudge_end_after_start(90, 60), 95);
        // Window exactly MIN_SECTION: untouched.
        assert_eq!(nudge_end_after_start(90, 95), 95);
        // Ample window: untouched.
        assert_eq!(nudge_end_after_start(30, 90), 90);
        // At the cap the nudge saturates (both pinned at the cap).
        assert_eq!(nudge_end_after_start(BOUND_ROW_MAX_S, 60), BOUND_ROW_MAX_S);
    }

    #[test]
    fn lowering_end_bumps_start_down() {
        // END 30 against START 60: START follows to END − 5.
        assert_eq!(nudge_start_after_end(60, 30), 25);
        // Window exactly MIN_SECTION: untouched.
        assert_eq!(nudge_start_after_end(25, 30), 25);
        // Ample window: untouched.
        assert_eq!(nudge_start_after_end(30, 90), 30);
        // At the floor the bump saturates (END 0 pins START at 0).
        assert_eq!(nudge_start_after_end(60, 0), 0);
    }

    #[test]
    fn nudges_are_idempotent_and_open_the_window_or_saturate() {
        // One nudge settles the pair: re-applying the SAME nudge never
        // moves it again (no oscillation), and the result either satisfies
        // the MIN_SECTION window or sits pinned at the range boundary
        // (cap/cap or 0/0 — where the runtime resolution's floor remains
        // authoritative).
        for start in (0..=BOUND_ROW_MAX_S).step_by(5) {
            for end in (0..=BOUND_ROW_MAX_S).step_by(35) {
                let end_nudged = nudge_end_after_start(start, end);
                assert_eq!(
                    nudge_end_after_start(start, end_nudged),
                    end_nudged,
                    "end nudge not idempotent at start {start} end {end}"
                );
                assert!(
                    end_nudged >= start.saturating_add(MIN_SECTION_S)
                        || end_nudged == BOUND_ROW_MAX_S,
                    "window not opened nor saturated: start {start} end' {end_nudged}"
                );

                let start_nudged = nudge_start_after_end(start, end);
                assert_eq!(
                    nudge_start_after_end(start_nudged, end),
                    start_nudged,
                    "start nudge not idempotent at start {start} end {end}"
                );
                assert!(
                    end >= start_nudged.saturating_add(MIN_SECTION_S) || start_nudged == 0,
                    "window not opened nor saturated: start' {start_nudged} end {end}"
                );
            }
        }
    }

    #[test]
    fn seed_end_ceils_to_the_second_and_never_truncates() {
        // 123.4 s → 124 (the next whole second — NOT the old 5 s pad).
        assert_eq!(seed_end_seconds(123_400), 124);
        // Whole seconds seed exactly (the chart-derived path: the service
        // publishes ceil-to-second, so the seed == the wheel's LENGTH
        // readout).
        assert_eq!(seed_end_seconds(120_000), 120);
        assert_eq!(seed_end_seconds(123_000), 123);
        // Just past a whole second rounds up (120.001 s → 121).
        assert_eq!(seed_end_seconds(120_001), 121);
        // The seed is always >= the real length (never truncates).
        for len_ms in [1, 4_999, 90_000, 123_400, 199_999] {
            assert!(
                seed_end_seconds(len_ms) as i64 * 1_000 >= i64::from(len_ms),
                "seed below the real end for {len_ms} ms"
            );
        }
        // Songs at/past the row cap seed AT the cap (the natural-end
        // sentinel — resolve_bounds never converts it to a section end).
        assert_eq!(seed_end_seconds(200_000), BOUND_ROW_MAX_S);
        assert_eq!(seed_end_seconds(700_000), BOUND_ROW_MAX_S);
        // Degenerate zero length seeds 0 (no publication should produce
        // this — parse rejects zero durations — but stay total).
        assert_eq!(seed_end_seconds(0), 0);
    }

    #[test]
    fn pre_shift_wall_conversion_follows_the_desired_rate() {
        // Identity: wall == content.
        assert_eq!(pre_shift_wall_ms(60_000, 100), 60_000);
        // 50%: the song plays at half speed — content takes twice the wall.
        assert_eq!(pre_shift_wall_ms(60_000, 50), 120_000);
        // 175%: content passes faster than wall (Step-2 demo class).
        assert_eq!(pre_shift_wall_ms(60_000, 175), 34_285);
        // Zero content is zero wall at any rate.
        assert_eq!(pre_shift_wall_ms(0, 25), 0);
        // Defensive: non-positive percents are identity.
        assert_eq!(pre_shift_wall_ms(60_000, 0), 60_000);
        assert_eq!(pre_shift_wall_ms(60_000, -5), 60_000);
    }

    #[test]
    fn end_policy_is_exclusive_and_total() {
        // LOOP OFF + a live section end: the thresholds are written.
        assert_eq!(
            end_policy(false, 30_000),
            EndPolicy::WriteThresholds { b_ms: 30_000 }
        );
        // LOOP ON never yields WriteThresholds — the loop driver owns
        // the end, whether or not a section end exists (breakdown
        // decision #2: loop-ON alone loops the whole song).
        assert_eq!(end_policy(true, 30_000), EndPolicy::ArmLoop);
        assert_eq!(end_policy(true, 0), EndPolicy::ArmLoop);
        // LOOP OFF without a section end: the stock natural end.
        assert_eq!(end_policy(false, 0), EndPolicy::Natural);
        // Defensive: a negative b_ms is "none" (the sentinels are >= 0).
        assert_eq!(end_policy(false, -1), EndPolicy::Natural);
        assert_eq!(end_policy(true, -1), EndPolicy::ArmLoop);
    }

    #[test]
    fn apply_action_transition_table() {
        // LOOP OFF + a section end always writes — a NEW b is an
        // idempotent rewrite whether or not thresholds are already
        // written this song.
        for written in [false, true] {
            assert_eq!(
                apply_action(EndPolicy::WriteThresholds { b_ms: 30_000 }, written),
                ApplyAction::Write { b_ms: 30_000 }
            );
        }
        // A section end cleared back to none restores the stash — once.
        assert_eq!(apply_action(EndPolicy::Natural, true), ApplyAction::Restore);
        assert_eq!(
            apply_action(EndPolicy::Natural, false),
            ApplyAction::Nothing
        );
        // LOOP ON always raises (idempotent): the cascade must never
        // fire mid-grind — 0x104A is one-way song-scoped state that
        // strikes the lane furniture and breaks freeze scoring on later
        // passes (cabinet finding 2026-08-14). Truncating writes are
        // exclusive to LOOP OFF.
        for written in [false, true] {
            assert_eq!(
                apply_action(EndPolicy::ArmLoop, written),
                ApplyAction::RaiseThresholds
            );
            assert!(!matches!(
                apply_action(EndPolicy::ArmLoop, written),
                ApplyAction::Write { .. }
            ));
        }
    }

    #[test]
    fn scrub_target_moves_by_the_delta() {
        // Normal FF (AC 1) / RW (AC 2): the target is current + delta.
        assert_eq!(
            scrub_target(30_000, 5_000, None, CHART_END, MARGIN),
            Some(35_000)
        );
        assert_eq!(
            scrub_target(30_000, -5_000, None, CHART_END, MARGIN),
            Some(25_000)
        );
        // A zero delta stays put (no hidden bias in the clamp).
        assert_eq!(
            scrub_target(30_000, 0, None, CHART_END, MARGIN),
            Some(30_000)
        );
    }

    #[test]
    fn scrub_target_clamps_at_the_end_bound() {
        // No live section end: chart_end − margin (AC 3).
        assert_eq!(
            scrub_target(118_500, 5_000, None, CHART_END, MARGIN),
            Some(CHART_END - MARGIN)
        );
        // A live B below the chart end wins the min; the margin applies
        // AFTER the min (the task's spelled formula — seeking exactly TO
        // B would land on an instant end/loop fire).
        assert_eq!(
            scrub_target(85_000, 5_000, Some(88_000), CHART_END, MARGIN),
            Some(87_000)
        );
        // Defensive: a B above the chart end never widens the bound.
        assert_eq!(
            scrub_target(118_500, 5_000, Some(130_000), CHART_END, MARGIN),
            Some(CHART_END - MARGIN)
        );
    }

    #[test]
    fn scrub_target_clamps_at_zero() {
        // Rewind past the start (AC 4): the caller dispatches 0 as the
        // plain restart.
        assert_eq!(
            scrub_target(3_000, -5_000, None, CHART_END, MARGIN),
            Some(0)
        );
        // From the pre-song approach (negative raw count) a rewind still
        // clamps at 0 …
        assert_eq!(
            scrub_target(-2_000, -5_000, None, CHART_END, MARGIN),
            Some(0)
        );
        // … and an FF lands where the delta says.
        assert_eq!(
            scrub_target(-2_000, 5_000, None, CHART_END, MARGIN),
            Some(3_000)
        );
    }

    #[test]
    fn scrub_target_refuses_degenerate_inputs() {
        // No live chart.
        assert_eq!(scrub_target(30_000, 5_000, None, 0, MARGIN), None);
        assert_eq!(scrub_target(30_000, 5_000, None, -1, MARGIN), None);
        // Bound collapses to (or below) zero: chart shorter than the
        // margin, or a degenerate live B.
        assert_eq!(scrub_target(500, 1_000, None, MARGIN, MARGIN), None);
        assert_eq!(scrub_target(500, 1_000, Some(500), CHART_END, MARGIN), None);
        assert_eq!(scrub_target(500, 1_000, Some(0), CHART_END, MARGIN), None);
    }

    #[test]
    fn scrub_increment_normalizes_absent_and_out_of_range() {
        // Absent block / key: the default (req 1 "defaults").
        assert_eq!(
            normalize_scrub_increment_ms(None),
            SCRUB_INCREMENT_DEFAULT_MS
        );
        // Out-of-range clamps to 250..=60000 (the caller logs one INFO).
        assert_eq!(
            normalize_scrub_increment_ms(Some(100)),
            SCRUB_INCREMENT_MIN_MS
        );
        assert_eq!(
            normalize_scrub_increment_ms(Some(0)),
            SCRUB_INCREMENT_MIN_MS
        );
        assert_eq!(
            normalize_scrub_increment_ms(Some(-5)),
            SCRUB_INCREMENT_MIN_MS
        );
        assert_eq!(
            normalize_scrub_increment_ms(Some(100_000)),
            SCRUB_INCREMENT_MAX_MS
        );
        // In-range values pass through untouched.
        for v in [
            SCRUB_INCREMENT_MIN_MS,
            SCRUB_INCREMENT_DEFAULT_MS,
            SCRUB_INCREMENT_MAX_MS,
        ] {
            assert_eq!(normalize_scrub_increment_ms(Some(v)), v);
        }
    }

    #[test]
    fn loop_fire_bound_composes_min_and_margin() {
        const MARGIN: i32 = 1_000;
        // b_live smallest (the ordinary sectioned grind): the loop fires
        // AT the marker — no margin on the user's term (2026-08-15
        // re-demo finding: B at 70 s must not loop at 69 s).
        assert_eq!(
            loop_fire_bound(Some(60_000), Some(110_000), 118_000, MARGIN),
            Some(60_000)
        );
        // The display threshold's raw equivalent smallest (a chart whose
        // last-note-end sits well inside the outro): margin applies.
        assert_eq!(
            loop_fire_bound(Some(115_000), Some(110_000), 118_000, MARGIN),
            Some(109_000)
        );
        // t98 smallest (defensive shape): margin applies.
        assert_eq!(
            loop_fire_bound(Some(115_000), Some(120_000), 112_000, MARGIN),
            Some(111_000)
        );
        // b inside the threshold margin window: the guarded threshold
        // term wins (the marker never drags the bound INTO the cascade).
        assert_eq!(
            loop_fire_bound(Some(117_500), Some(120_000), 118_000, MARGIN),
            Some(117_000)
        );
        // No b (loop-ON alone — whole-song loop): thresholds − margin.
        assert_eq!(
            loop_fire_bound(None, Some(110_000), 118_000, MARGIN),
            Some(109_000)
        );
        // Converter failed (dropped term): b exact + the t98 guard.
        assert_eq!(
            loop_fire_bound(Some(60_000), None, 118_000, MARGIN),
            Some(60_000)
        );
        // Both optional terms gone: the t98 − margin step-5 guard alone.
        assert_eq!(loop_fire_bound(None, None, 118_000, MARGIN), Some(117_000));
        // Degenerate sections refuse: bound at/below zero.
        assert_eq!(loop_fire_bound(None, None, MARGIN, MARGIN), None);
        assert_eq!(loop_fire_bound(Some(500), None, MARGIN, MARGIN), None);
        assert_eq!(loop_fire_bound(Some(0), None, 118_000, MARGIN), None);
        assert_eq!(loop_fire_bound(None, None, 0, MARGIN), None);
    }
}
