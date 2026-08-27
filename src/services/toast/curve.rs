//! Toast fade curves — the pure animation-envelope layer of the shared
//! toast service. Dependency-free (std only) so the offline validation
//! harness (`scripts/validate_auto_calibration.sh`) can mount this file
//! into a throwaway host crate via `#[path]` and run the `#[cfg(test)]`
//! suite on any host.
//!
//! Two envelopes:
//! - `Flash { hold_ms }` — the original gesture-toast shape (100 ms linear
//!   fade-in → hold at 1.0 → 300 ms linear fade-out → done), with the hold
//!   parameterized (Training Mode keeps 250 ms; calibration uses 3 s
//!   refusal / 5 s result toasts).
//! - `Pulse` — the calibration-song indicator: an endless breathing loop
//!   (800 ms in → 800 ms hold → 800 ms out → 400 ms dark gap, period
//!   2800 ms). It never self-terminates; only supersession or `dismiss`
//!   ends it.

/// Flash fade-in ramp (soft leading edge).
pub const FLASH_FADE_IN_MS: u64 = 100;
/// Flash fade-out ramp (soft trailing edge).
pub const FLASH_FADE_OUT_MS: u64 = 300;
/// The original gesture-toast hold (the `flash()` default).
pub const FLASH_DEFAULT_HOLD_MS: u64 = 250;

/// Pulse segment lengths (fade-in / hold / fade-out / dark gap).
pub const PULSE_FADE_IN_MS: u64 = 800;
pub const PULSE_HOLD_MS: u64 = 800;
pub const PULSE_FADE_OUT_MS: u64 = 800;
pub const PULSE_GAP_MS: u64 = 400;
/// Full pulse period (sum of the four segments).
pub const PULSE_PERIOD_MS: u64 =
    PULSE_FADE_IN_MS + PULSE_HOLD_MS + PULSE_FADE_OUT_MS + PULSE_GAP_MS;

/// Animation envelope of a toast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastMode {
    /// One-shot: fade in, hold `hold_ms`, fade out, end (`None`).
    Flash { hold_ms: u64 },
    /// Endless breathing loop; never returns `None`.
    Pulse,
}

/// Alpha at `elapsed_ms` since the toast started, or `None` once a flash
/// has completed (a pulse never completes).
pub fn alpha_at(mode: ToastMode, elapsed_ms: u64) -> Option<f32> {
    match mode {
        ToastMode::Flash { hold_ms } => flash_alpha(hold_ms, elapsed_ms),
        ToastMode::Pulse => Some(pulse_alpha(elapsed_ms % PULSE_PERIOD_MS)),
    }
}

/// The original piecewise-linear flash envelope with a parameterized hold.
fn flash_alpha(hold_ms: u64, elapsed_ms: u64) -> Option<f32> {
    if elapsed_ms < FLASH_FADE_IN_MS {
        return Some(elapsed_ms as f32 / FLASH_FADE_IN_MS as f32);
    }
    let after_in = elapsed_ms - FLASH_FADE_IN_MS;
    if after_in < hold_ms {
        return Some(1.0);
    }
    let after_hold = after_in - hold_ms;
    if after_hold < FLASH_FADE_OUT_MS {
        return Some(1.0 - after_hold as f32 / FLASH_FADE_OUT_MS as f32);
    }
    None
}

/// One period of the pulse loop (`phase` in `0..PULSE_PERIOD_MS`).
fn pulse_alpha(phase: u64) -> f32 {
    if phase < PULSE_FADE_IN_MS {
        return phase as f32 / PULSE_FADE_IN_MS as f32;
    }
    let after_in = phase - PULSE_FADE_IN_MS;
    if after_in < PULSE_HOLD_MS {
        return 1.0;
    }
    let after_hold = after_in - PULSE_HOLD_MS;
    if after_hold < PULSE_FADE_OUT_MS {
        return 1.0 - after_hold as f32 / PULSE_FADE_OUT_MS as f32;
    }
    // Dark gap.
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-6, "expected {b}, got {a}");
    }

    #[test]
    fn flash_envelope_boundaries() {
        for &hold in &[FLASH_DEFAULT_HOLD_MS, 3000, 5000] {
            let mode = ToastMode::Flash { hold_ms: hold };
            // Start of fade-in.
            assert_close(alpha_at(mode, 0).unwrap(), 0.0);
            // Mid fade-in.
            assert_close(alpha_at(mode, 50).unwrap(), 0.5);
            // Hold start and end (inclusive of the whole hold window).
            assert_close(alpha_at(mode, FLASH_FADE_IN_MS).unwrap(), 1.0);
            assert_close(alpha_at(mode, FLASH_FADE_IN_MS + hold - 1).unwrap(), 1.0);
            // Mid fade-out.
            assert_close(
                alpha_at(mode, FLASH_FADE_IN_MS + hold + FLASH_FADE_OUT_MS / 2).unwrap(),
                0.5,
            );
            // Past the envelope: done.
            assert_eq!(
                alpha_at(mode, FLASH_FADE_IN_MS + hold + FLASH_FADE_OUT_MS),
                None
            );
            assert_eq!(alpha_at(mode, u64::MAX / 2), None);
        }
    }

    #[test]
    fn flash_default_matches_legacy_envelope() {
        // The legacy gesture toast was 100 in / 250 hold / 300 out = 650 total.
        let mode = ToastMode::Flash {
            hold_ms: FLASH_DEFAULT_HOLD_MS,
        };
        assert!(alpha_at(mode, 649).is_some());
        assert_eq!(alpha_at(mode, 650), None);
    }

    #[test]
    fn pulse_single_period_shape() {
        let m = ToastMode::Pulse;
        assert_close(alpha_at(m, 0).unwrap(), 0.0); // cycle start
        assert_close(alpha_at(m, 400).unwrap(), 0.5); // mid fade-in
        assert_close(alpha_at(m, 800).unwrap(), 1.0); // hold start
        assert_close(alpha_at(m, 1599).unwrap(), 1.0); // hold end
        assert_close(alpha_at(m, 2000).unwrap(), 0.5); // mid fade-out
        assert_close(alpha_at(m, 2400).unwrap(), 0.0); // gap start
        assert_close(alpha_at(m, 2600).unwrap(), 0.0); // mid gap
        assert_close(alpha_at(m, 2799).unwrap(), 0.0); // gap end
    }

    #[test]
    fn pulse_is_periodic_and_never_ends() {
        let m = ToastMode::Pulse;
        for k in 0..5u64 {
            let base = k * PULSE_PERIOD_MS;
            for &phase in &[0u64, 400, 800, 1600, 2000, 2400, 2600] {
                let now = alpha_at(m, base + phase);
                let first = alpha_at(m, phase);
                assert!(now.is_some(), "pulse must never return None");
                assert_close(now.unwrap(), first.unwrap());
            }
        }
        // Long-elapsed sanity: minutes in, still alive.
        assert!(alpha_at(m, 10 * 60 * 1000).is_some());
    }
}
