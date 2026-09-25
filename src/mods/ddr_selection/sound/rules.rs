//! A3's in-game announcer + crowd rules for the legacy skins (pure,
//! host-tested).
//!
//! Dependency-free (mounted by `scripts/validate_ddr_selection.sh`).
//!
//! A transcription of A3 `CallVoiceActor::onUpdate` (`FUN_1800369e0` in
//! `gamemdx_20240402`) for skins 1..=5 over the actor's fields, which World's
//! actor keeps at A3's offsets and its own `onMessage` keeps filling (RE:
//! `.agents/planning/2026-09-22-ddr-selection/research/announcer-crowd.md`).
//! [`step`] is one frame of a running actor (step state 1 / 2 — the caller
//! returns early for 0 / 3 like A3); the engine executes the returned plays
//! IN ORDER (a guarded play checks the handle an earlier play of the same
//! frame stored, as in A3).

/// A3's gauge thresholds (f32 bit-exact: `DAT_180265034`, `DAT_1802645f8`,
/// `DAT_1802888ac`, `DAT_180288c60`).
pub const LOW: f32 = 0.2;
pub const CROWD_QUIET: f32 = 0.4;
pub const HIGH: f32 = 0.8;
/// State-voice / crowd periods (added to the next-due times).
pub const STATE_PERIOD: i32 = 0x8000;
pub const CROWD_PERIOD: i32 = 0x10000;
/// The regain voice needs the second time word above this.
pub const REGAIN_AFTER: i32 = 20000;
/// Guarded plays are also skipped while `combo % 100 >= 91` (a combo
/// callout is about to come).
pub const COMBO_QUIET_FROM: i32 = 91;
/// Crowd SEs stay off while the gauge is ≤ [`CROWD_QUIET`] and the combo is
/// below this.
pub const CROWD_MIN_COMBO: i32 = 13;

/// One sound. Names are NUL-terminated for the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Play {
    /// A combo callout: played unguarded, its handle stored.
    Voice(&'static str),
    /// A voice played only when the stored handle is not playing; its
    /// handle stored.
    Guarded(&'static str),
    /// A crowd SE (handle not stored).
    Se(&'static str),
}

impl Play {
    pub fn name(self) -> &'static str {
        match self {
            Play::Voice(n) | Play::Guarded(n) | Play::Se(n) => n,
        }
    }
}

/// The actor fields the rules read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fields {
    pub gauge: [f32; 2],
    pub combo: [i32; 2],
    pub difficulty: [i32; 2],
    /// `+0x88` / `+0x8C`.
    pub time: i32,
    pub time2: i32,
    /// `+0x90` / `+0x94`.
    pub next_state: i32,
    pub next_crowd: i32,
    /// `+0x98`.
    pub milestone: i32,
    /// `+0x9C` / `+0x9D` / `+0x9E`.
    pub voice_off: bool,
    pub se_off: bool,
    pub was_low: bool,
}

/// What one frame writes back, and plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Step {
    pub next_state: i32,
    pub next_crowd: i32,
    pub milestone: i32,
    pub was_low: bool,
    /// At most a combo callout, a state voice and a crowd SE.
    pub plays: [Option<Play>; 3],
}

const SN2_COMBO: [&str; 10] = [
    "sn2_dgm25\0",
    "sn2_dgm26\0",
    "sn2_dgm27\0",
    "sn2_dgm28\0",
    "sn2_dgm29\0",
    "sn2_dgm30\0",
    "sn2_dgm31\0",
    "sn2_dgm32\0",
    "sn2_dgm33\0",
    "sn2_dgm34\0",
];
const VO_COMBO: [&str; 10] = [
    "vo_ingame_combo_100\0",
    "vo_ingame_combo_200\0",
    "vo_ingame_combo_300\0",
    "vo_ingame_combo_400\0",
    "vo_ingame_combo_500\0",
    "vo_ingame_combo_600\0",
    "vo_ingame_combo_700\0",
    "vo_ingame_combo_800\0",
    "vo_ingame_combo_900\0",
    "vo_ingame_combo_1000\0",
];

/// Every cue [`step`] can play (the bank must hold them all).
pub fn all_cues() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    v.extend(SN2_COMBO);
    v.extend(VO_COMBO);
    v.extend([
        "vo_ingame_combo_over\0",
        "vo_ingame_combo_gen\0",
        "ACT6\0",
        "sn2_dgm_high\0",
        "vo_ingame_high\0",
        "sn2_dgm_middle\0",
        "vo_ingame_gen\0",
        "vo_ingame_regain\0",
        "vo_ingame_low_hard\0",
        "vo_ingame_low_easy\0",
        "2nd_BIG2\0",
        "2nd_KANSEI_B\0",
        "STG_APP03\0",
        "STG_APP02\0",
    ]);
    v
}

/// Skins 4 / 5 (X, 2013-A) use A3's own announcer; A3's `skin - 1 > 2`.
fn a3_voice(skin: u8) -> bool {
    !(1..=3).contains(&skin)
}

/// One frame for `skin` ∈ 1..=5 (`None` otherwise — the caller runs World's).
pub fn step(f: &Fields, skin: u8) -> Option<Step> {
    if !(1..=5).contains(&skin) {
        return None;
    }
    let mut plays: [Option<Play>; 3] = [None; 3];
    let c = f.combo[0].max(f.combo[1]);
    let quiet = c % 100 >= COMBO_QUIET_FROM;

    // Combo callouts.
    let m = f.milestone;
    if m > 0 && m <= c {
        if m % 100 == 0 {
            let table: &[&'static str] = match skin {
                1 => &[],
                2 | 3 => &SN2_COMBO,
                _ => &VO_COMBO,
            };
            let idx = m / 100 - 1;
            if idx >= 0 && (idx as usize) < table.len() {
                plays[0] = Some(Play::Voice(table[idx as usize]));
            } else if a3_voice(skin) && !quiet {
                plays[0] = Some(Play::Guarded("vo_ingame_combo_over\0"));
            }
        } else if a3_voice(skin) && !quiet {
            plays[0] = Some(Play::Guarded("vo_ingame_combo_gen\0"));
        }
    }
    let milestone = (c / 50 + 1) * 50;

    // The side with the higher gauge (ties: side 1, A3's `g0 <= g1`).
    let side = usize::from(f.gauge[0] <= f.gauge[1]);
    let g = f.gauge[side];

    // State voice.
    let mut next_state = f.next_state;
    let mut was_low = f.was_low;
    if !f.voice_off && next_state < f.time {
        next_state = next_state.wrapping_add(STATE_PERIOD);
        let voice: Option<&'static str> = if !(g <= HIGH) {
            Some(match skin {
                1 => "ACT6\0",
                2 | 3 => "sn2_dgm_high\0",
                _ => "vo_ingame_high\0",
            })
        } else if LOW <= g {
            let v = if !f.was_low || f.time2 <= REGAIN_AFTER {
                match skin {
                    1 => None,
                    2 | 3 => Some("sn2_dgm_middle\0"),
                    _ => Some("vo_ingame_gen\0"),
                }
            } else if a3_voice(skin) {
                Some("vo_ingame_regain\0")
            } else {
                None
            };
            was_low = false;
            v
        } else if a3_voice(skin) {
            was_low = true;
            let d = f.difficulty[side];
            Some(if (3..=4).contains(&d) {
                "vo_ingame_low_hard\0"
            } else {
                "vo_ingame_low_easy\0"
            })
        } else {
            None
        };
        if let Some(v) = voice.filter(|_| !quiet) {
            plays[1] = Some(Play::Guarded(v));
        }
    }

    // Crowd SE.
    let mut next_crowd = f.next_crowd;
    if !f.se_off && f.time > next_crowd {
        next_crowd = next_crowd.wrapping_add(CROWD_PERIOD);
        if !(g <= CROWD_QUIET && c < CROWD_MIN_COMBO) {
            plays[2] = Some(Play::Se(match skin {
                1 => "2nd_BIG2\0",
                2 => "2nd_KANSEI_B\0",
                3 => "STG_APP03\0",
                _ => "STG_APP02\0",
            }));
        }
    }

    Some(Step {
        next_state,
        next_crowd,
        milestone,
        was_low,
        plays,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Fields {
        Fields {
            gauge: [0.5, 0.0],
            combo: [0, 0],
            difficulty: [2, 0],
            time: 0,
            time2: 0,
            next_state: 0xBE70,
            next_crowd: 0xFC7C,
            milestone: 50,
            voice_off: false,
            se_off: false,
            was_low: false,
        }
    }

    fn names(s: &Step) -> Vec<(char, &'static str)> {
        s.plays
            .iter()
            .flatten()
            .map(|p| {
                let k = match p {
                    Play::Voice(_) => 'v',
                    Play::Guarded(_) => 'g',
                    Play::Se(_) => 's',
                };
                (k, p.name().trim_end_matches('\0'))
            })
            .collect()
    }

    #[test]
    fn thresholds_are_a3s_bits() {
        assert_eq!(LOW.to_bits(), 0x3E4C_CCCD);
        assert_eq!(CROWD_QUIET.to_bits(), 0x3ECC_CCCD);
        assert_eq!(HIGH.to_bits(), 0x3F4C_CCCD);
        assert_eq!(STATE_PERIOD, 0x8000);
        assert_eq!(CROWD_PERIOD, 0x10000);
    }

    #[test]
    fn only_legacy_skins() {
        assert!(step(&base(), 0).is_none());
        assert!(step(&base(), 6).is_none());
    }

    #[test]
    fn nothing_before_the_first_due_time() {
        for skin in 1..=5 {
            let s = step(&base(), skin).unwrap();
            assert!(names(&s).is_empty());
            assert_eq!(s.milestone, 50);
            assert_eq!((s.next_state, s.next_crowd), (0xBE70, 0xFC7C));
        }
    }

    #[test]
    fn combo_callouts_per_skin() {
        let mut f = base();
        f.combo = [100, 37];
        f.milestone = 100;
        assert!(names(&step(&f, 1).unwrap()).is_empty());
        assert_eq!(names(&step(&f, 2).unwrap()), [('v', "sn2_dgm25")]);
        assert_eq!(names(&step(&f, 3).unwrap()), [('v', "sn2_dgm25")]);
        assert_eq!(names(&step(&f, 4).unwrap()), [('v', "vo_ingame_combo_100")]);
        assert_eq!(names(&step(&f, 5).unwrap()), [('v', "vo_ingame_combo_100")]);
        // the higher side's combo counts
        f.combo = [3, 1000];
        f.milestone = 1000;
        assert_eq!(names(&step(&f, 2).unwrap()), [('v', "sn2_dgm34")]);
        assert_eq!(step(&f, 2).unwrap().milestone, 1050);
        // 1100+: A3 voice only, guarded
        f.combo = [1100, 0];
        f.milestone = 1100;
        assert!(names(&step(&f, 2).unwrap()).is_empty());
        assert_eq!(
            names(&step(&f, 4).unwrap()),
            [('g', "vo_ingame_combo_over")]
        );
        // odd 50s: A3 voice only
        f.combo = [150, 0];
        f.milestone = 150;
        assert!(names(&step(&f, 3).unwrap()).is_empty());
        assert_eq!(names(&step(&f, 5).unwrap()), [('g', "vo_ingame_combo_gen")]);
        assert_eq!(step(&f, 5).unwrap().milestone, 200);
    }

    #[test]
    fn milestone_follows_the_combo_even_after_a_break() {
        let mut f = base();
        f.combo = [42, 0];
        f.milestone = 250; // combo broke at 230
        let s = step(&f, 4).unwrap();
        assert!(names(&s).is_empty());
        assert_eq!(s.milestone, 50);
    }

    #[test]
    fn state_voices_per_skin_and_gauge() {
        let mut f = base();
        f.time = 0xBE71;
        let at = |g: f32, skin: u8, f: &Fields| {
            let mut f = *f;
            f.gauge = [g, 0.0];
            names(&step(&f, skin).unwrap())
        };
        assert_eq!(at(0.9, 1, &f), [('g', "ACT6")]);
        assert_eq!(at(0.9, 2, &f), [('g', "sn2_dgm_high")]);
        assert_eq!(at(0.9, 4, &f), [('g', "vo_ingame_high")]);
        assert_eq!(at(0.8, 3, &f), [('g', "sn2_dgm_middle")]);
        assert!(at(0.5, 1, &f).is_empty());
        assert_eq!(at(0.5, 5, &f), [('g', "vo_ingame_gen")]);
        assert_eq!(at(0.2, 4, &f), [('g', "vo_ingame_gen")]);
        assert!(at(0.1, 1, &f).is_empty());
        assert!(at(0.1, 3, &f).is_empty());
        assert_eq!(at(0.1, 4, &f), [('g', "vo_ingame_low_easy")]);
        f.difficulty = [3, 0];
        assert_eq!(at(0.1, 5, &f), [('g', "vo_ingame_low_hard")]);
        f.difficulty = [4, 0];
        assert_eq!(at(0.1, 5, &f), [('g', "vo_ingame_low_hard")]);
        let s = step(
            &Fields {
                gauge: [0.1, 0.0],
                ..f
            },
            4,
        )
        .unwrap();
        assert!(s.was_low);
        assert_eq!(s.next_state, 0xBE70 + 0x8000);
        // skins 1–3 never set the latch
        assert!(
            !step(
                &Fields {
                    gauge: [0.1, 0.0],
                    ..f
                },
                2
            )
            .unwrap()
            .was_low
        );
    }

    #[test]
    fn low_voice_uses_the_higher_gauge_sides_difficulty() {
        let mut f = base();
        f.time = 0xBE71;
        f.gauge = [0.05, 0.1];
        f.difficulty = [4, 1];
        assert_eq!(names(&step(&f, 4).unwrap()), [('g', "vo_ingame_low_easy")]);
        f.gauge = [0.1, 0.1]; // tie: side 1
        assert_eq!(names(&step(&f, 4).unwrap()), [('g', "vo_ingame_low_easy")]);
        f.gauge = [0.15, 0.1];
        assert_eq!(names(&step(&f, 4).unwrap()), [('g', "vo_ingame_low_hard")]);
    }

    #[test]
    fn regain_after_low_and_twenty_seconds() {
        let mut f = base();
        f.time = 0xBE71;
        f.gauge = [0.5, 0.0];
        f.was_low = true;
        f.time2 = 20000;
        assert_eq!(names(&step(&f, 4).unwrap()), [('g', "vo_ingame_gen")]);
        f.time2 = 20001;
        assert_eq!(names(&step(&f, 4).unwrap()), [('g', "vo_ingame_regain")]);
        assert!(!step(&f, 4).unwrap().was_low);
        // skins 2/3: nothing, latch still cleared
        let s = step(&f, 3).unwrap();
        assert!(names(&s).is_empty());
        assert!(!s.was_low);
        // high keeps the latch
        f.gauge = [0.9, 0.0];
        assert!(step(&f, 4).unwrap().was_low);
    }

    #[test]
    fn guarded_plays_wait_near_a_combo_callout() {
        let mut f = base();
        f.time = 0xBE71;
        f.gauge = [0.9, 0.0];
        f.combo = [191, 0];
        f.milestone = 200;
        assert!(names(&step(&f, 4).unwrap()).is_empty());
        f.combo = [190, 0];
        assert_eq!(names(&step(&f, 4).unwrap()), [('g', "vo_ingame_high")]);
        // direct callouts are not held back
        f.combo = [200, 0];
        f.milestone = 200;
        assert_eq!(
            names(&step(&f, 2).unwrap()),
            [('v', "sn2_dgm26"), ('g', "sn2_dgm_high")]
        );
    }

    #[test]
    fn voice_off_stops_state_voices_only() {
        let mut f = base();
        f.time = 0x1_0000;
        f.voice_off = true;
        f.gauge = [0.9, 0.0];
        f.combo = [100, 0];
        f.milestone = 100;
        let s = step(&f, 4).unwrap();
        assert_eq!(
            names(&s),
            [('v', "vo_ingame_combo_100"), ('s', "STG_APP02")]
        );
        assert_eq!(s.next_state, 0xBE70);
        f.se_off = true;
        assert_eq!(names(&step(&f, 4).unwrap()), [('v', "vo_ingame_combo_100")]);
    }

    #[test]
    fn crowd_per_skin_and_quiet_rule() {
        let mut f = base();
        f.time = 0xFC7D;
        f.next_state = 0x7FFF_0000;
        f.gauge = [0.5, 0.0];
        let crowd = |skin| names(&step(&f, skin).unwrap());
        assert_eq!(crowd(1), [('s', "2nd_BIG2")]);
        assert_eq!(crowd(2), [('s', "2nd_KANSEI_B")]);
        assert_eq!(crowd(3), [('s', "STG_APP03")]);
        assert_eq!(crowd(4), [('s', "STG_APP02")]);
        assert_eq!(crowd(5), [('s', "STG_APP02")]);
        assert_eq!(step(&f, 1).unwrap().next_crowd, 0xFC7C + 0x10000);
        f.gauge = [0.4, 0.0];
        f.combo = [12, 0];
        assert!(names(&step(&f, 1).unwrap()).is_empty());
        assert_eq!(step(&f, 1).unwrap().next_crowd, 0xFC7C + 0x10000);
        f.combo = [13, 0];
        assert_eq!(names(&step(&f, 1).unwrap()), [('s', "2nd_BIG2")]);
        // due only strictly after the time
        f.time = 0xFC7C;
        assert!(names(&step(&f, 1).unwrap()).is_empty());
    }

    #[test]
    fn every_cue_is_in_the_era_bank_manifest() {
        let manifest = super::super::cues::all();
        for c in all_cues() {
            let n = c.trim_end_matches('\0');
            assert!(manifest.contains(&n), "{n} missing from sound::cues");
        }
    }

    #[test]
    fn every_cue_is_nul_terminated_and_unique() {
        let all = all_cues();
        for c in &all {
            assert!(c.ends_with('\0') && !c[..c.len() - 1].contains('\0'), "{c}");
        }
        let mut s = all.clone();
        s.sort();
        s.dedup();
        assert_eq!(s.len(), all.len());
    }
}
