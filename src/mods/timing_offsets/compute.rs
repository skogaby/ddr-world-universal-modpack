//! Auto-calibration pure decision layer — dependency-free (std only) so the
//! offline validation harness (`scripts/validate_auto_calibration.sh`) can
//! mount this file into a throwaway host crate via `#[path]` and run the
//! `#[cfg(test)]` suite on any host.
//!
//! Holds the entered-side census (plan Step 2) and the calibration decision
//! core (plan Step 3): filtered per-step ms-error sums in, an apply/refuse
//! `Outcome` out.

/// Minimum valid samples for an apply (below: refuse).
pub const MIN_SAMPLES: u32 = 30;
/// Maximum credible |mean| in ms (above: garbage run, refuse). A genuine
/// audio chain is never half a second off.
pub const MAX_ABS_MEAN_MS: f64 = 500.0;
/// The offset clamp, matching the timing-offsets field clamp.
pub const OFFSET_MIN: i32 = -1000;
pub const OFFSET_MAX: i32 = 1000;

/// Result of the GAMEPLAY-entry entered-side census.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CensusOutcome {
    /// Exactly one entered side — calibrate it (0 = P1, 1 = P2).
    Single { side: u8 },
    /// Both sides entered — calibration disabled for the song (toast).
    TwoPlayers,
    /// Nothing readable/entered — refuse silently (WARN only).
    NonePlaying,
}

/// Classify the entered-side census. `p1`/`p2` are
/// `stage_records::side_entered` results: `Some(true)` = entered,
/// `Some(false)` = not entered, `None` = unreadable. Any `None` is treated
/// conservatively as "can't establish a single player" unless the OTHER side
/// alone can't make it two (an unreadable side could be entered, so we never
/// claim `Single` while one side is unknown).
pub fn census(p1: Option<bool>, p2: Option<bool>) -> CensusOutcome {
    match (p1, p2) {
        (Some(true), Some(true)) => CensusOutcome::TwoPlayers,
        (Some(true), Some(false)) => CensusOutcome::Single { side: 0 },
        (Some(false), Some(true)) => CensusOutcome::Single { side: 1 },
        // Both explicitly not entered, or anything unreadable: refuse.
        _ => CensusOutcome::NonePlaying,
    }
}

// ── Decision core (plan Step 3) ─────────────────────────────────────────

/// Filtered per-step ms-error accumulator snapshot (grades M/P/G/Gd/Boo of
/// the calibrated side only; Miss and OK excluded at the tap).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CalibStats {
    /// Sum of signed ms errors (negative = early, positive = late).
    pub sum: i64,
    /// Sum of squared ms errors (stddev derivation, log-only).
    pub sum_sq: i64,
    /// Number of samples.
    pub count: u32,
}

/// The end-of-song decision.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    /// Write `new_offset` (mean/stddev carried for the toast + INFO log).
    Apply {
        new_offset: i32,
        mean: f64,
        stddev: f64,
    },
    /// Fewer than [`MIN_SAMPLES`] valid steps.
    RefuseTooFewSamples { count: u32 },
    /// |mean| beyond [`MAX_ABS_MEAN_MS`] — a garbage run.
    RefuseMeanOutOfRange { mean: f64 },
    /// The playing side was autoplay-tainted; the measurement is meaningless.
    RefuseAutoplay,
}

/// Decide the calibration outcome. Model: `mean(error) ≈ real_latency −
/// SOUND_OFFSET` with "higher = audio later" semantics, so the correction is
/// `new = clamp(old + round(mean))` — a player hitting consistently LATE
/// (positive mean) raises the offset. (Sign direction cabinet-verified via
/// the apply-path INFO log; a wrong sign is the `+` below.)
pub fn compute(stats: &CalibStats, old_offset: i32, autoplay_tainted: bool) -> Outcome {
    if autoplay_tainted {
        return Outcome::RefuseAutoplay;
    }
    if stats.count < MIN_SAMPLES {
        return Outcome::RefuseTooFewSamples { count: stats.count };
    }
    let n = stats.count as f64;
    let mean = stats.sum as f64 / n;
    if mean.abs() > MAX_ABS_MEAN_MS {
        return Outcome::RefuseMeanOutOfRange { mean };
    }
    // Population stddev from the running sums: sqrt(E[x²] − E[x]²), floored
    // at 0 against float rounding. Log-only — never gates.
    let variance = (stats.sum_sq as f64 / n - mean * mean).max(0.0);
    let stddev = variance.sqrt();
    // Round the MEAN (the delta the result toast displays) half away from
    // zero, then apply it — this keeps the displayed delta and the written
    // value consistent (`new == old + delta` exactly, before clamping).
    let delta = mean.round(); // f64::round = half away from zero
    let new_offset = (old_offset as f64 + delta).clamp(OFFSET_MIN as f64, OFFSET_MAX as f64) as i32;
    Outcome::Apply {
        new_offset,
        mean,
        stddev,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn census_single_sides() {
        assert_eq!(
            census(Some(true), Some(false)),
            CensusOutcome::Single { side: 0 }
        );
        assert_eq!(
            census(Some(false), Some(true)),
            CensusOutcome::Single { side: 1 }
        );
    }

    #[test]
    fn census_two_players() {
        assert_eq!(census(Some(true), Some(true)), CensusOutcome::TwoPlayers);
    }

    #[test]
    fn census_refusals() {
        // Nobody entered.
        assert_eq!(census(Some(false), Some(false)), CensusOutcome::NonePlaying);
        // Any unreadable side refuses — an unknown side could be entered, so
        // `Single` can never be claimed next to a `None`.
        assert_eq!(census(None, None), CensusOutcome::NonePlaying);
        assert_eq!(census(Some(true), None), CensusOutcome::NonePlaying);
        assert_eq!(census(None, Some(true)), CensusOutcome::NonePlaying);
        assert_eq!(census(Some(false), None), CensusOutcome::NonePlaying);
        assert_eq!(census(None, Some(false)), CensusOutcome::NonePlaying);
    }

    // ── Decision core ────────────────────────────────────────────────

    /// Stats for `count` samples all carrying the same `err` ms error.
    fn uniform(err: i64, count: u32) -> CalibStats {
        CalibStats {
            sum: err * count as i64,
            sum_sq: err * err * count as i64,
            count,
        }
    }

    fn expect_apply(o: Outcome) -> (i32, f64, f64) {
        match o {
            Outcome::Apply {
                new_offset,
                mean,
                stddev,
            } => (new_offset, mean, stddev),
            other => panic!("expected Apply, got {other:?}"),
        }
    }

    #[test]
    fn compute_sample_count_boundary() {
        let just_under = uniform(10, MIN_SAMPLES - 1);
        assert_eq!(
            compute(&just_under, 87, false),
            Outcome::RefuseTooFewSamples {
                count: MIN_SAMPLES - 1
            }
        );
        let at_min = uniform(10, MIN_SAMPLES);
        let (new, mean, _) = expect_apply(compute(&at_min, 87, false));
        assert_eq!(new, 97);
        assert_eq!(mean, 10.0);
    }

    #[test]
    fn compute_sign_direction() {
        // Late hits (positive mean) raise the offset ("higher = audio later").
        let (new, ..) = expect_apply(compute(&uniform(25, 100), 87, false));
        assert_eq!(new, 112);
        // Early hits (negative mean) lower it.
        let (new, ..) = expect_apply(compute(&uniform(-25, 100), 87, false));
        assert_eq!(new, 62);
    }

    #[test]
    fn compute_mean_out_of_range() {
        // Exactly at the bound applies; strictly beyond refuses.
        let (new, ..) = expect_apply(compute(&uniform(500, 40), 0, false));
        assert_eq!(new, 500);
        match compute(&uniform(501, 40), 0, false) {
            Outcome::RefuseMeanOutOfRange { mean } => assert_eq!(mean, 501.0),
            other => panic!("expected RefuseMeanOutOfRange, got {other:?}"),
        }
        match compute(&uniform(-501, 40), 0, false) {
            Outcome::RefuseMeanOutOfRange { mean } => assert_eq!(mean, -501.0),
            other => panic!("expected RefuseMeanOutOfRange, got {other:?}"),
        }
    }

    #[test]
    fn compute_rounding_half_away_from_zero() {
        // mean = +0.5 → +1; mean = −0.5 → −1.
        let half_up = CalibStats {
            sum: 15,
            sum_sq: 1000,
            count: 30,
        };
        let (new, mean, _) = expect_apply(compute(&half_up, 87, false));
        assert_eq!(mean, 0.5);
        assert_eq!(new, 88);
        let half_down = CalibStats {
            sum: -15,
            sum_sq: 1000,
            count: 30,
        };
        let (new, mean, _) = expect_apply(compute(&half_down, 87, false));
        assert_eq!(mean, -0.5);
        assert_eq!(new, 86);
    }

    #[test]
    fn compute_clamps_to_offset_range() {
        let (new, ..) = expect_apply(compute(&uniform(400, 100), 900, false));
        assert_eq!(new, 1000);
        let (new, ..) = expect_apply(compute(&uniform(-400, 100), -900, false));
        assert_eq!(new, -1000);
    }

    #[test]
    fn compute_stddev_derivation() {
        // Uniform samples: stddev 0.
        let (_, _, sd) = expect_apply(compute(&uniform(10, 50), 87, false));
        assert!(sd.abs() < 1e-9, "uniform stddev should be 0, got {sd}");
        // Half at 0, half at 20: mean 10, stddev 10.
        let split = CalibStats {
            sum: 20 * 25,
            sum_sq: 400 * 25,
            count: 50,
        };
        let (_, mean, sd) = expect_apply(compute(&split, 87, false));
        assert_eq!(mean, 10.0);
        assert!((sd - 10.0).abs() < 1e-9, "expected stddev 10, got {sd}");
    }

    #[test]
    fn compute_autoplay_takes_precedence() {
        // Autoplay refuses even when everything else would apply — and even
        // when the sample count is ALSO too low (precedence).
        assert_eq!(
            compute(&uniform(10, 100), 87, true),
            Outcome::RefuseAutoplay
        );
        assert_eq!(compute(&uniform(10, 5), 87, true), Outcome::RefuseAutoplay);
    }
}
