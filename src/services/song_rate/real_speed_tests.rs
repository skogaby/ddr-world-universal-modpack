//! Host tests for the Real Speed × effective-rate derivation (design
//! req 33): at a committed non-identity rate the normalized multiplier
//! derives from `Core BPM × effective_rate` — structurally independent of
//! the Real Speed Fix toggle, which never enters the math. Identity and
//! uncommitted snapshots derive NOTHING (`None`): no write happens, so each
//! toggle state's stock behavior is preserved bit-identically by
//! construction.

use crate::core::xact::rate::{target_for_percent, RateRatio};

use super::clock_patch::RateSnapshot;
use super::real_speed::rate_adjusted_multiplier;

/// A committed snapshot at `percent`, built through the production
/// `target_for_percent` path with a deliberately non-block-clean source
/// frame count (fixture honesty — same fixture as the tick-domain suite).
fn committed_snapshot(percent: u32) -> RateSnapshot {
    let target = target_for_percent(9_876_543, 128, percent).expect("supported percent");
    RateSnapshot {
        generation: 11,
        requested_percent: percent as i32,
        participant_mask: 0b01,
        effective_rate: target.rate,
        committed: true,
    }
}

const CORE_BPM: f64 = 148.5;

#[test]
fn non_identity_derivation_matches_the_native_formula_at_each_rate() {
    // Literal pins computed independently (IEEE f64, trunc, clamp [25,800])
    // for core 148.5 and the 9_876_543-frame fixture. The 800 rows pin the
    // clamp interaction: slow rates inflate the multiplier past the native
    // ceiling exactly as the stock setter would clamp it.
    let expect: &[(u32, &[(i32, i32)])] = &[
        (25, &[(100, 269), (400, 800), (600, 800)]),
        (50, &[(100, 134), (400, 538), (600, 800)]),
        (75, &[(100, 89), (400, 359), (600, 538)]),
        (125, &[(100, 53), (400, 215), (600, 323)]),
        (175, &[(100, 38), (400, 153), (600, 230)]),
    ];
    for &(percent, rows) in expect {
        let snapshot = committed_snapshot(percent);
        for &(target, multiplier) in rows {
            assert_eq!(
                rate_adjusted_multiplier(target, CORE_BPM, &snapshot),
                Some(multiplier),
                "target {target} at {percent}%"
            );
        }
    }
}

#[test]
fn derivation_clamps_to_the_native_bounds() {
    let snapshot = committed_snapshot(175);
    // Tiny target: trunc(10·100 / 259.87…) = 3 → clamped to 25.
    assert_eq!(rate_adjusted_multiplier(10, CORE_BPM, &snapshot), Some(25));
    // Huge (but in-domain) target: far past 800 → clamped to 800.
    assert_eq!(
        rate_adjusted_multiplier(10_000, CORE_BPM, &snapshot),
        Some(800)
    );
}

#[test]
fn identity_and_uncommitted_snapshots_derive_nothing() {
    // AC-2: no derivation ⇒ no write ⇒ BOTH fix-toggle states keep today's
    // stock behavior bit-identically (the native/patched setter output is
    // never touched).
    let committed_100 = RateSnapshot {
        committed: true,
        ..RateSnapshot::IDENTITY
    };
    let uncommitted_75 = RateSnapshot {
        committed: false,
        ..committed_snapshot(75)
    };
    for snapshot in [RateSnapshot::IDENTITY, committed_100, uncommitted_75] {
        for target in [100, 400, 600] {
            assert_eq!(rate_adjusted_multiplier(target, CORE_BPM, &snapshot), None);
        }
    }
}

#[test]
fn degenerate_inputs_fail_soft_to_none() {
    let snapshot = committed_snapshot(50);
    // Core BPM outside the trusted domain: unreadable/garbage chain values
    // must never produce a write.
    for core in [0.0, -148.5, f64::NAN, f64::INFINITY, 20_000.0] {
        assert_eq!(rate_adjusted_multiplier(600, core, &snapshot), None);
    }
    // A zero/negative or absurd target means an unset field or a misread —
    // skip the write (stock behavior) rather than derive from garbage.
    for target in [0, -5, 1_000_000] {
        assert_eq!(rate_adjusted_multiplier(target, CORE_BPM, &snapshot), None);
    }
    // A degenerate zero ratio (structurally can't-happen behind the seqlock)
    // zeroes the divisor — guarded, not a panic or a wild value.
    let degenerate = RateSnapshot {
        effective_rate: RateRatio {
            source_frames: 0,
            output_frames: 1,
        },
        ..committed_snapshot(75)
    };
    assert!(degenerate.is_non_identity_commit());
    assert_eq!(rate_adjusted_multiplier(600, CORE_BPM, &degenerate), None);
}

#[test]
fn the_fix_toggle_is_structurally_absent_from_the_rate_path() {
    // Req 33's "independent of the Real Speed Fix toggle", pinned as a
    // purity property: the derivation is a function of (target, core,
    // snapshot) ONLY — same inputs, same output, no external state consulted.
    let snapshot = committed_snapshot(50);
    let first = rate_adjusted_multiplier(600, CORE_BPM, &snapshot);
    for _ in 0..3 {
        assert_eq!(rate_adjusted_multiplier(600, CORE_BPM, &snapshot), first);
    }
    assert_eq!(first, Some(800));
}
