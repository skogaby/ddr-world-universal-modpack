//! Target Score replay — the per-note decision source that turns the human's
//! loaded pacemaker GHOST into the bot's play. Pure.
//!
//! The ghost is one grade-class byte per Results entry, in chart order (the
//! stage record's `+0xB8` stream; the wire `<ghost>` is the same bytes as
//! `'0' + grade`): 0 Marvelous, 1 Perfect, 2 Great, 3 Good, 4 Boo, 5 Miss,
//! 6 O.K. (freeze held / shock avoided), 7 N.G. (freeze dropped / shock
//! stepped). It carries NO timing, so a replayed tap is given a random offset
//! INSIDE its grade's inclusive window (Marvelous ≤ 17, Perfect 18–34, Great
//! 35–84, Good 85–124): the grade — and therefore the money score, which is a
//! pure function of the grade counts — is reproduced, while the FAST/SLOW
//! side comes from the skill model's sticky per-song side chain so a replay
//! looks like a person and not a metronome.
//!
//! S-Marvelous exclusion (maintainer directive, 2026-09-14): with the
//! S-Marvelous mod armed at window `W`, a ghost Marvelous samples
//! `|d| ∈ [W+1, 17]` — exclusive Marvelous only — so a replay never produces
//! an S-Marvelous the target's play could not have had (the ghost alphabet
//! predates the tier). `W = 16` leaves exactly `|d| = 17`.
//!
//! World's judge has no Boo (the ±160 row is matched but rejected), so a `4`
//! is unreproducible and lands on Miss like a `5`; a `7` on a tap likewise. A
//! `6` on a tap (never written by stock, tolerated) reads as Marvelous.
//! Dependency-free so `tools/bot_sim` can mount it.

use super::skill::{Curve, Form, Plan, Rng, GRADE_MISS};

pub const GRADE_MARVELOUS: u8 = 0;
pub const GRADE_PERFECT: u8 = 1;
pub const GRADE_GREAT: u8 = 2;
pub const GRADE_GOOD: u8 = 3;
pub const GRADE_BOO: u8 = 4;
pub const GRADE_OK: u8 = 6;
pub const GRADE_NG: u8 = 7;
/// Number of grade classes the stream can carry (`0..=7`).
pub const GRADE_CLASSES: usize = 8;

/// Stock Marvelous half-window (inclusive ms).
pub const MARVELOUS_MAX_MS: i32 = 17;

/// The inclusive `|d|` band a replayed TAP samples from for ghost `grade`,
/// or `None` when the grade is reproduced as a Miss. `smarv_floor` is the
/// armed S-Marvelous window (`0` = none): a Marvelous band then starts at
/// `min(floor + 1, 17)` so the replay never lands inside the S-Marv tier.
pub fn tap_band(grade: u8, smarv_floor: i32) -> Option<(i32, i32)> {
    match grade {
        GRADE_MARVELOUS | GRADE_OK => {
            let lo = if smarv_floor > 0 {
                (smarv_floor.saturating_add(1)).min(MARVELOUS_MAX_MS)
            } else {
                0
            };
            Some((lo, MARVELOUS_MAX_MS))
        }
        GRADE_PERFECT => Some((18, 34)),
        GRADE_GREAT => Some((35, 84)),
        GRADE_GOOD => Some((85, 124)),
        _ => None, // Boo (unreproducible in World), Miss, N.G., garbage
    }
}

/// The grade the replay AIMS to reproduce for a tap carrying ghost byte
/// `byte` — what the reproduction-miss counter compares the planner's
/// resolved grade against.
pub fn expected_tap_grade(byte: u8) -> u8 {
    match tap_band(byte, 0) {
        Some(_) => match byte {
            GRADE_OK => GRADE_MARVELOUS,
            g => g,
        },
        None => GRADE_MISS,
    }
}

/// Decide one replayed tap: a Miss for the Miss-class bytes, else a hit
/// whose magnitude is uniform over the grade's band and whose side comes
/// from the skill model's sticky side chain.
pub fn decide_tap(rng: &mut Rng, form: &mut Form, c: &Curve, byte: u8, smarv_floor: i32) -> Plan {
    let Some((lo, hi)) = tap_band(byte, smarv_floor) else {
        return Plan::Miss;
    };
    let span = (hi - lo).max(0) as u32;
    let magnitude = lo + rng.below(span + 1) as i32;
    if magnitude == 0 {
        // Advance the chain anyway so a run of exact hits keeps the same
        // per-note cadence as every other note.
        let _ = form.next_side(rng, c);
        return Plan::Hit { d_ms: 0 };
    }
    let side = form.next_side(rng, c);
    let d = if side < 0.0 { -magnitude } else { magnitude };
    Plan::Hit { d_ms: d }
}

/// Per-grade-class histogram of a ghost stream (out-of-alphabet bytes are
/// dropped) — the song-end diagnostic line's `target=[…]`.
pub fn histogram(bytes: &[u8]) -> [u32; GRADE_CLASSES] {
    let mut h = [0u32; GRADE_CLASSES];
    for &b in bytes {
        if let Some(slot) = h.get_mut(b as usize) {
            *slot += 1;
        }
    }
    h
}

#[cfg(test)]
mod tests {
    use super::super::skill;
    use super::*;

    fn setup() -> (Rng, Form, Curve) {
        let c = skill::curve(10);
        let mut rng = Rng::new(0x6E05);
        let form = Form::new(&mut rng, &c);
        (rng, form, c)
    }

    #[test]
    fn bands_are_the_judge_windows() {
        assert_eq!(tap_band(GRADE_MARVELOUS, 0), Some((0, 17)));
        assert_eq!(tap_band(GRADE_PERFECT, 0), Some((18, 34)));
        assert_eq!(tap_band(GRADE_GREAT, 0), Some((35, 84)));
        assert_eq!(tap_band(GRADE_GOOD, 0), Some((85, 124)));
        assert_eq!(
            tap_band(GRADE_OK, 0),
            Some((0, 17)),
            "O.K. on a tap reads Marvelous"
        );
        for miss in [GRADE_BOO, GRADE_MISS, GRADE_NG, 8, 0xFF] {
            assert_eq!(tap_band(miss, 0), None, "byte {miss}");
        }
        // Adjacent bands tile the graded range exactly (no gap, no overlap).
        let bands: Vec<_> = (0..4u8).map(|g| tap_band(g, 0).unwrap()).collect();
        for w in bands.windows(2) {
            assert_eq!(w[0].1 + 1, w[1].0);
        }
        assert_eq!(bands[3].1, skill::GOOD_WINDOW_MS);
        // Every band value grades as its own grade.
        for g in 0..4u8 {
            let (lo, hi) = tap_band(g, 0).unwrap();
            for d in lo..=hi {
                assert_eq!(skill::grade_for_offset(d), g, "grade {g} d={d}");
                assert_eq!(skill::grade_for_offset(-d), g, "grade {g} d=-{d}");
            }
        }
    }

    #[test]
    fn smarv_floor_excludes_the_smarvelous_tier() {
        assert_eq!(tap_band(GRADE_MARVELOUS, 12), Some((13, 17)));
        assert_eq!(tap_band(GRADE_MARVELOUS, 1), Some((2, 17)));
        assert_eq!(tap_band(GRADE_MARVELOUS, 16), Some((17, 17)));
        // A floor at or past the Marvelous edge degenerates to the edge.
        assert_eq!(tap_band(GRADE_MARVELOUS, 17), Some((17, 17)));
        assert_eq!(tap_band(GRADE_MARVELOUS, 99), Some((17, 17)));
        assert_eq!(tap_band(GRADE_OK, 12), Some((13, 17)));
        // Other grades are unaffected by the floor.
        for g in [GRADE_PERFECT, GRADE_GREAT, GRADE_GOOD] {
            assert_eq!(tap_band(g, 12), tap_band(g, 0));
        }
    }

    #[test]
    fn expected_grade_table() {
        assert_eq!(expected_tap_grade(GRADE_MARVELOUS), 0);
        assert_eq!(expected_tap_grade(GRADE_PERFECT), 1);
        assert_eq!(expected_tap_grade(GRADE_GREAT), 2);
        assert_eq!(expected_tap_grade(GRADE_GOOD), 3);
        assert_eq!(expected_tap_grade(GRADE_OK), 0);
        for miss in [GRADE_BOO, GRADE_MISS, GRADE_NG, 0xFF] {
            assert_eq!(expected_tap_grade(miss), GRADE_MISS, "byte {miss}");
        }
    }

    #[test]
    fn decisions_stay_inside_their_band_and_use_both_sides() {
        let (mut rng, mut form, c) = setup();
        for g in 0..4u8 {
            let (lo, hi) = tap_band(g, 0).unwrap();
            let (mut neg, mut pos) = (0u32, 0u32);
            let mut seen_lo = false;
            let mut seen_hi = false;
            for _ in 0..50_000 {
                match decide_tap(&mut rng, &mut form, &c, g, 0) {
                    Plan::Miss => panic!("grade {g} must never miss"),
                    Plan::Hit { d_ms } => {
                        let a = d_ms.abs();
                        assert!(
                            (lo..=hi).contains(&a),
                            "grade {g}: |{d_ms}| outside [{lo},{hi}]"
                        );
                        assert_eq!(skill::grade_for_offset(d_ms), g);
                        seen_lo |= a == lo;
                        seen_hi |= a == hi;
                        if d_ms < 0 {
                            neg += 1;
                        } else if d_ms > 0 {
                            pos += 1;
                        }
                    }
                }
            }
            assert!(seen_lo && seen_hi, "grade {g}: band edges reachable");
            assert!(
                neg > 5_000 && pos > 5_000,
                "grade {g}: both sides ({neg}/{pos})"
            );
        }
    }

    #[test]
    fn smarv_floor_never_yields_an_smarvelous() {
        let (mut rng, mut form, c) = setup();
        for floor in [1, 12, 16] {
            let mut hit_edge = false;
            for _ in 0..20_000 {
                let Plan::Hit { d_ms } =
                    decide_tap(&mut rng, &mut form, &c, GRADE_MARVELOUS, floor)
                else {
                    panic!("Marvelous never misses");
                };
                assert!(
                    d_ms.abs() > floor,
                    "floor {floor}: {d_ms} inside the S-Marv tier"
                );
                assert!(d_ms.abs() <= 17);
                assert_eq!(skill::grade_for_offset(d_ms), GRADE_MARVELOUS);
                hit_edge |= d_ms.abs() == 17;
            }
            assert!(hit_edge, "floor {floor}: the Marvelous edge is reachable");
        }
    }

    #[test]
    fn miss_class_bytes_decide_miss() {
        let (mut rng, mut form, c) = setup();
        for b in [GRADE_BOO, GRADE_MISS, GRADE_NG, 9, 0xFF] {
            assert_eq!(
                decide_tap(&mut rng, &mut form, &c, b, 0),
                Plan::Miss,
                "byte {b}"
            );
        }
    }

    #[test]
    fn histogram_counts_alphabet_only() {
        let h = histogram(&[0, 0, 1, 2, 3, 5, 6, 7, 7, 8, 0xFF]);
        assert_eq!(h, [2, 1, 1, 1, 0, 1, 1, 2]);
        assert_eq!(histogram(&[]), [0; 8]);
    }
}
