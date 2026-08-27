//! Host tests for the pure assist-tick content→wall domain algebra
//! (design req 30: tick positions and restart skips convert through the
//! committed exact `RateRatio`; `sound_offset` stays unscaled; the
//! judgment-timing term follows the clock stub's content domain; 100 % and
//! uncommitted are bit-identical to the legacy arithmetic).

use crate::core::xact::rate::{target_for_percent, RateRatio};

use super::clock_patch::RateSnapshot;
use super::tick_domain::{restart_skip_ms, tick_track_positions};

/// The literal legacy arithmetic the identity path must reproduce
/// bit-identically (the mod's pre-conversion formula, sign already applied
/// to the judgment-timing term).
fn legacy_positions(times: &[i32], judgment_timing: i32, sound_offset: i32, m0: i32) -> Vec<i32> {
    let shift = judgment_timing
        .saturating_sub(sound_offset)
        .saturating_sub(m0);
    times.iter().map(|&t| t.saturating_add(shift)).collect()
}

/// Independent conversion oracle: round-half-away(content · output / source)
/// in i128 — deliberately NOT `content_to_wall_ms` (the thing under test
/// composes it; the oracle must not).
fn oracle_wall(content: i64, rate: RateRatio) -> i64 {
    let num = i128::from(content) * i128::from(rate.output_frames);
    let den = i128::from(rate.source_frames);
    let negative = num < 0;
    let magnitude = if negative { -num } else { num };
    let rounded = (magnitude + den / 2) / den;
    (if negative { -rounded } else { rounded }) as i64
}

/// A committed snapshot at `percent`, built through the production
/// `target_for_percent` path with a deliberately non-block-clean source
/// frame count (fixture honesty: real banks' durations are never
/// block-exact).
fn committed_snapshot(percent: u32) -> RateSnapshot {
    let target = target_for_percent(9_876_543, 128, percent).expect("supported percent");
    RateSnapshot {
        generation: 7,
        requested_percent: percent as i32,
        participant_mask: 0b01,
        effective_rate: target.rate,
        committed: true,
    }
}

const TIMES: [i32; 5] = [-1_500, 0, 437, 60_000, 299_999];
const JT: i32 = 33;
const SO: i32 = 125;
const M0: i32 = 911;

#[test]
fn identity_and_uncommitted_paths_are_bit_identical_to_the_legacy_arithmetic() {
    let committed_100 = RateSnapshot {
        committed: true,
        ..RateSnapshot::IDENTITY
    };
    let uncommitted_75 = RateSnapshot {
        committed: false,
        ..committed_snapshot(75)
    };
    let extremes = [i32::MIN, -2_000, 0, 5_000, i32::MAX];
    for snapshot in [RateSnapshot::IDENTITY, committed_100, uncommitted_75] {
        assert!(!snapshot.is_non_identity_commit());
        for jt in [-100, 0, 33, 100] {
            for so in [0, 12, 125] {
                for m0 in extremes {
                    let times: Vec<i32> =
                        TIMES.iter().copied().chain([i32::MIN, i32::MAX]).collect();
                    assert_eq!(
                        tick_track_positions(&times, jt, so, m0, &snapshot),
                        legacy_positions(&times, jt, so, m0),
                        "positions must be bit-identical (jt={jt} so={so} m0={m0})"
                    );
                    for mc in extremes {
                        assert_eq!(
                            restart_skip_ms(mc, m0, &snapshot),
                            mc.saturating_sub(m0),
                            "skip must be bit-identical (mc={mc} m0={m0})"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn positions_convert_exactly_at_each_supported_rate() {
    for percent in [25u32, 50, 75, 125, 175] {
        let snapshot = committed_snapshot(percent);
        let expected: Vec<i32> = TIMES
            .iter()
            .map(|&t| {
                let content = i64::from(t) + i64::from(JT) - i64::from(M0);
                (oracle_wall(content, snapshot.effective_rate) - i64::from(SO)) as i32
            })
            .collect();
        assert_eq!(
            tick_track_positions(&TIMES, JT, SO, M0, &snapshot),
            expected,
            "exact conversion at {percent}%"
        );
    }
    // Literal pins (computed independently, half-away rounding, the
    // 9_876_543-frame fixture): sound_offset subtracted AFTER the conversion
    // (wall domain, unscaled) — a scaled sound_offset would shift every value.
    assert_eq!(
        tick_track_positions(&TIMES, JT, SO, M0, &committed_snapshot(50)),
        vec![-4_881, -1_881, -1_007, 118_119, 598_117]
    );
    assert_eq!(
        tick_track_positions(&TIMES, JT, SO, M0, &committed_snapshot(175)),
        vec![-1_484, -627, -377, 33_659, 170_802]
    );
    assert_eq!(
        tick_track_positions(&TIMES, JT, SO, M0, &committed_snapshot(25)),
        vec![-9_637, -3_637, -1_889, 236_363, 1_196_359]
    );
}

#[test]
fn restart_skips_convert_exactly_at_each_supported_rate() {
    let pairs = [(911, 911), (5_000, 911), (400, 911), (123_456, 911)];
    for percent in [25u32, 50, 75, 125, 175] {
        let snapshot = committed_snapshot(percent);
        for (mc, m0) in pairs {
            let expected = oracle_wall(i64::from(mc) - i64::from(m0), snapshot.effective_rate);
            assert_eq!(
                i64::from(restart_skip_ms(mc, m0, &snapshot)),
                expected,
                "exact skip at {percent}% (mc={mc} m0={m0})"
            );
        }
    }
    // Literal pins: negative skips (mc < m0 — a rewind past the anchor) pass
    // through converted; the mod's `.max(0)` guard is downstream.
    let snap_50 = committed_snapshot(50);
    assert_eq!(restart_skip_ms(5_000, 911, &snap_50), 8_178);
    assert_eq!(restart_skip_ms(400, 911, &snap_50), -1_022);
    let snap_125 = committed_snapshot(125);
    assert_eq!(restart_skip_ms(123_456, 911, &snap_125), 98_035);
}

#[test]
fn committed_rate_synthesis_proceeds_with_converted_positions() {
    // The inverted Step-4 scaffold expectation (design req 32 retired): a
    // committed non-identity generation CONVERTS instead of refusing — the
    // pure layer yields a full, converted position list (the mod-side
    // refusal variant no longer exists; structural removal is compile-level).
    let snapshot = committed_snapshot(50);
    assert!(snapshot.is_non_identity_commit());
    let converted = tick_track_positions(&TIMES, JT, SO, M0, &snapshot);
    assert_eq!(converted.len(), TIMES.len());
    assert_ne!(
        converted,
        legacy_positions(&TIMES, JT, SO, M0),
        "a 50% commit must actually convert"
    );
}

#[test]
fn conversion_clamps_to_i32_and_fails_soft_on_a_degenerate_ratio() {
    // Clamp: at 25% the wall domain is ~4x content — i32::MAX-adjacent
    // content overflows i32 and must clamp, not wrap.
    let snap_25 = committed_snapshot(25);
    assert_eq!(
        tick_track_positions(&[i32::MAX], 0, 0, 0, &snap_25),
        vec![i32::MAX]
    );
    assert_eq!(restart_skip_ms(i32::MAX, 0, &snap_25), i32::MAX);
    assert_eq!(restart_skip_ms(i32::MIN, 0, &snap_25), i32::MIN);

    // Fail-soft: a degenerate zero ratio (structurally can't-happen behind
    // the seqlock — published ratios are validated) falls back to the
    // identity arithmetic instead of panicking on the judge path.
    let degenerate = RateSnapshot {
        effective_rate: RateRatio {
            source_frames: 0,
            output_frames: 1,
        },
        ..committed_snapshot(75)
    };
    assert!(degenerate.is_non_identity_commit());
    assert_eq!(
        tick_track_positions(&TIMES, JT, SO, M0, &degenerate),
        legacy_positions(&TIMES, JT, SO, M0)
    );
    assert_eq!(restart_skip_ms(5_000, 911, &degenerate), 4_089);
}
