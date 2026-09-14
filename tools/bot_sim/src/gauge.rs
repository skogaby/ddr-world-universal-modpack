//! `sequence::dance::NormalGaugeActor` — exact integer transcription of the
//! game's NORMAL gauge (`docs/gauge_and_judge_scoring_research.md` §4).
//!
//! Every division is C truncating division (`trunc`), as in the binary.

/// Gauge units: 10000 = 100 %.
pub const FULL: i32 = 10_000;
/// A fresh NORMAL gauge starts half full (`0x18035B7B4 = 0.5f`).
pub const INITIAL: i32 = 5_000;
/// `DAT_180399C98 = 0.28f` — the "danger" display threshold.
pub const DANGER: i32 = 2_800;
/// `GameWork` level index 3 → `LEVEL_TABLE[3] = 10` → `(10 << 6) / 100 = 6`.
pub const LEVEL_FACTOR: i32 = 6;

/// C-style truncating division.
#[inline]
pub fn trunc(a: i64, b: i64) -> i64 {
    a / b
}

/// The gauge's mutable state (the actor fields the formula reads).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NormalGauge {
    /// `+0x90` gauge value 0..=10000.
    pub value: i32,
    /// `+0x90` at construction — the formulas use the INITIAL value.
    pub init: i32,
    /// `+0xC4` / `+0xC8` (from msg `0x1033`).
    pub combo: i32,
    pub max_combo: i32,
    /// `+0xCC` consecutive-bad streak, `+0xD0` `cur_beat` at its start.
    pub streak: i32,
    pub streak_start_beat: i32,
    /// `+0xB8`
    pub dead: bool,
    /// `+0xD8` — the `GamePlayActor+0x2B7` instant-death gate (0 in normal
    /// play; nonzero = a death latches but never ends the run).
    pub gate: bool,
    /// Whether the death broadcast (`0x103A`) fired.
    pub died: bool,
    /// Lowest value seen (for the report).
    pub min_value: i32,
}

impl NormalGauge {
    pub fn new() -> Self {
        Self::with_initial(INITIAL)
    }

    pub fn with_initial(init: i32) -> Self {
        NormalGauge {
            value: init,
            init,
            combo: 0,
            max_combo: 0,
            streak: 0,
            streak_start_beat: 0,
            dead: init <= 0,
            gate: false,
            died: false,
            min_value: init,
        }
    }

    /// Msg `0x1033`: the judge's post-judgement combo / max combo.
    pub fn set_combo(&mut self, combo: i32, max_combo: i32) {
        self.combo = combo;
        self.max_combo = max_combo;
    }

    /// `FUN_1800751D0`: apply one judgement. `grade` is the game's 0..=7 index
    /// (0 Marv, 1 Perf, 2 Great, 3 Good, 4 Boo, 5 Miss, 6 O.K., 7 N.G.),
    /// `delta_ms` = `event − note.mc`, `cur_beat` / `music_count` = the last
    /// `0x1045` frame values.
    pub fn apply(&mut self, grade: u8, delta_ms: i32, cur_beat: i32, music_count: i32) -> i32 {
        if matches!(grade, 4 | 5 | 7) {
            if self.streak == 0 {
                self.streak_start_beat = cur_beat;
            }
            self.streak += 1;
        } else {
            self.streak = 0;
        }
        let mut pts = self.judge_point(grade, delta_ms, cur_beat, music_count);
        if self.dead {
            pts = 0;
        }
        // rec_pct == dmg_pct == 100 for the NORMAL gauge ⇒ (100 · pts) / 100.
        let delta = trunc(100 * pts as i64, 100) as i32;
        self.value += delta;
        if self.value < 1 {
            if !self.gate {
                self.value = 0;
                if !self.dead {
                    self.died = true;
                }
                self.dead = true;
            } else {
                self.value = 1;
            }
        } else if self.value > FULL {
            self.value = FULL;
        }
        self.min_value = self.min_value.min(self.value);
        delta
    }

    /// `FUN_180075370` — NormalGaugeActor's judge-point function.
    pub fn judge_point(&self, grade: u8, delta_ms: i32, cur_beat: i32, music_count: i32) -> i32 {
        if grade == 3 {
            return 0;
        }
        let lvl = LEVEL_FACTOR as i64;
        let init = self.init as i64;
        let combo = self.combo as i64;
        let maxcombo = self.max_combo as i64;

        let (sev, recover) = match grade {
            6 => (0, true),
            7 => (9, false),
            5 => (13, false),
            _ => {
                let u = trunc(delta_ms as i64 * 150, 1000) - 1;
                let sev = u.abs() / 2;
                (sev, sev < 9)
            }
        };

        let value: i64 = if recover {
            let mut t = trunc((combo + 1) * combo, maxcombo + 1);
            if combo < 1025 {
                if t > 20 {
                    t = trunc(t - 20, 10) + 20;
                }
            } else {
                t = 21;
            }
            t = t.min(30);
            let mut rec = trunc(
                trunc(
                    trunc((t * t + 400) * trunc(20 - sev, 2), 500) * 20000,
                    init + 10000,
                ) * (96 - lvl)
                    * 2,
                192,
            );
            if self.dead {
                rec = trunc(rec, 2);
            }
            rec = rec.max(2);
            if self.dead {
                rec = 0;
            }
            if init == 0 && combo < 3 {
                rec = 0;
            }
            rec
        } else {
            let ct = maxcombo.min(30);
            let v = trunc(
                trunc((ct * ct + 700) * sev * 4, 700) * (init + 5000) * 2,
                30000,
            ) * (lvl * 3 + 64);
            let mut s = self.streak as i64;
            if (12 - maxcombo) < s && s > 4 {
                s = 0;
            }
            if (cur_beat as i64) > self.streak_start_beat as i64 + 3072 {
                s = 0;
            } else {
                s = s.min(8);
            }
            let mut dmg = trunc(trunc(v, 64) * 3, s + 2);
            if init < dmg && init > 1250 {
                dmg = init - 625;
            }
            -dmg
        };

        let x = trunc((music_count as i64 - 20000) * 64, 20000);
        let bell = (4096 - x * x).max(0);
        (trunc((bell + 4096) * value, 8192) * 10) as i32
    }
}

impl Default for NormalGauge {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(combo: i32, maxcombo: i32, streak: i32) -> NormalGauge {
        let mut g = NormalGauge::new();
        g.set_combo(combo, maxcombo);
        g.streak = streak;
        g
    }

    // Anchors computed by hand from the transcription (see the RE doc §4.2).
    #[test]
    fn marvelous_recovery_anchors() {
        assert_eq!(g(1, 1, 0).judge_point(0, 0, 0, 20000), 90);
        assert_eq!(g(100, 100, 0).judge_point(0, 0, 0, 20000), 280);
        assert_eq!(g(50, 50, 0).judge_point(2, 80, 0, 20000), 150);
    }

    #[test]
    fn good_never_moves_the_gauge() {
        assert_eq!(g(1, 1, 0).judge_point(3, 0, 0, 20000), 0);
        assert_eq!(g(500, 500, 0).judge_point(3, 120, 0, 20000), 0);
    }

    #[test]
    fn miss_damage_anchors() {
        assert_eq!(g(0, 30, 1).judge_point(5, 0, 0, 20000), -990);
        // 5th consecutive miss with maxcombo > 7 resets the divisor to 2.
        assert_eq!(g(0, 30, 5).judge_point(5, 0, 0, 20000), -1480);
        assert_eq!(g(0, 5, 1).judge_point(5, 0, 0, 20000), -440);
        // bell = 0 at song start halves the change.
        assert_eq!(g(0, 30, 1).judge_point(5, 0, 0, 0), -490);
        assert_eq!(g(0, 30, 1).judge_point(7, 0, 0, 20000), -690);
    }

    #[test]
    fn streak_divisor_mercy_then_reset() {
        let base = g(0, 30, 1).judge_point(5, 0, 0, 20000); // /3
        let s2 = g(0, 30, 2).judge_point(5, 0, 0, 20000); // /4
        let s4 = g(0, 30, 4).judge_point(5, 0, 0, 20000); // /6
        let s5 = g(0, 30, 5).judge_point(5, 0, 0, 20000); // reset → /2
        assert!(s2 > base && s4 > s2, "{base} {s2} {s4}");
        assert!(s5 < base, "{s5} vs {base}");
        // Streak that started > 3/4 measure ago also resets.
        let mut old = g(0, 30, 3);
        old.streak_start_beat = 0;
        assert_eq!(old.judge_point(5, 0, 4000, 20000), -1480);
    }

    #[test]
    fn apply_bookkeeping_and_death() {
        let mut gg = NormalGauge::new();
        gg.set_combo(0, 30);
        let mut mc = 20000;
        let mut n = 0;
        while !gg.dead {
            gg.apply(5, 0, 0, mc);
            gg.streak = 0; // isolated misses (a Good in between resets the streak)
            mc += 2000;
            n += 1;
            assert!(n < 50);
        }
        assert_eq!(gg.value, 0);
        assert!(gg.died);
        // After death nothing changes.
        gg.set_combo(10, 30);
        assert_eq!(gg.apply(0, 0, 0, 20000), 0);
        assert_eq!(gg.value, 0);
    }

    #[test]
    fn gate_pins_at_one_and_never_dies() {
        let mut gg = NormalGauge::new();
        gg.gate = true;
        gg.set_combo(0, 30);
        for _ in 0..20 {
            gg.apply(5, 0, 0, 20000);
        }
        assert_eq!(gg.value, 1);
        assert!(!gg.died);
        assert!(!gg.dead);
    }

    #[test]
    fn clamps_at_full() {
        let mut gg = NormalGauge::with_initial(9990);
        gg.set_combo(100, 100);
        gg.apply(0, 0, 0, 20000);
        assert_eq!(gg.value, FULL);
    }
}
