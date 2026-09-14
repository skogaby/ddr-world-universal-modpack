//! `judge_submit` bookkeeping (`docs/gauge_and_judge_scoring_research.md` §3):
//! per-grade counters, combo, FC type, EX, money score, FAST/SLOW, plus the
//! S-Marvelous presentation count and the rank table.

use crate::chart::Difficulty;

/// Game grade indices.
pub const MARV: usize = 0;
pub const PERF: usize = 1;
pub const GREAT: usize = 2;
pub const GOOD: usize = 3;
pub const BOO: usize = 4;
pub const MISS: usize = 5;
pub const OK: usize = 6;
pub const NG: usize = 7;

/// Full-combo type as broadcast in msg `0x1034`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullCombo {
    Marvelous,
    Perfect,
    Great,
    Good,
}

impl FullCombo {
    pub fn label(self) -> &'static str {
        match self {
            FullCombo::Marvelous => "MFC",
            FullCombo::Perfect => "PFC",
            FullCombo::Great => "GFC",
            FullCombo::Good => "FC",
        }
    }
    pub fn code(self) -> u8 {
        match self {
            FullCombo::Marvelous => 0,
            FullCombo::Perfect => 1,
            FullCombo::Great => 2,
            FullCombo::Good => 3,
        }
    }
}

/// The per-song accumulator (`GamePlayActor+0x194..+0x1E0`).
#[derive(Debug, Clone, Default)]
pub struct Board {
    pub taps: u32,
    pub freezes: u32,
    pub shocks: u32,
    pub counts: [u32; 8],
    pub combo: u32,
    pub max_combo: u32,
    pub fast: u32,
    pub slow: u32,
    /// Kind-2 (freeze tail) O.K.s — the game's `+0x1C0`, the FC comparand.
    pub freeze_ok: u32,
    /// Marvelous hits within the S-Marvelous window (presentation layer).
    pub smarv: u32,
    pub smarv_window_ms: i32,
    /// The FC type fired at the moment the last step landed (if any).
    pub fc: Option<FullCombo>,
    /// Signed timing offsets of every graded tap (for the histogram).
    pub deltas: Vec<i32>,
}

impl Board {
    pub fn new(taps: u32, freezes: u32, shocks: u32, smarv_window_ms: i32) -> Self {
        Board {
            taps,
            freezes,
            shocks,
            smarv_window_ms,
            ..Default::default()
        }
    }

    /// One judgement. `is_tail` = a kind-2 freeze-end note (does not advance
    /// the combo). `delta_ms` = `event − note.mc` (0 for Miss/OK/NG).
    /// Returns the combo message the gauge would receive: `(combo, max)`.
    pub fn submit(&mut self, grade: usize, delta_ms: i32, is_tail: bool) -> (u32, u32) {
        if let Some(c) = self.counts.get_mut(grade) {
            *c += 1;
        }
        if grade == MARV && delta_ms.abs() <= self.smarv_window_ms {
            self.smarv += 1;
        }
        if (1..=4).contains(&grade) {
            if delta_ms < 0 {
                self.fast += 1;
            } else if delta_ms > 0 {
                self.slow += 1;
            }
        }
        if grade <= 3 {
            self.deltas.push(delta_ms);
        }
        if is_tail && grade == OK {
            self.freeze_ok += 1;
        }
        if grade < 4 || grade == OK {
            if !is_tail {
                self.combo += 1;
                self.max_combo = self.max_combo.max(self.combo);
            }
            // Full combo fires the moment the last step lands: the game
            // compares `freezeOK(+0x1C0)` (kind-2 O.K.s only) with `freezes`.
            if self.combo == self.taps + self.shocks && self.freeze_ok == self.freezes {
                self.fc = Some(self.fc_type());
            }
        } else {
            self.combo = 0;
        }
        (self.combo, self.max_combo)
    }

    fn fc_type(&self) -> FullCombo {
        if self.counts[GOOD] > 0 {
            FullCombo::Good
        } else if self.counts[GREAT] > 0 {
            FullCombo::Great
        } else if self.counts[PERF] > 0 {
            FullCombo::Perfect
        } else {
            FullCombo::Marvelous
        }
    }

    /// `EX = (Marv + OK)·3 + Perf·2 + Great`.
    pub fn ex(&self) -> u32 {
        (self.counts[MARV] + self.counts[OK]) * 3 + self.counts[PERF] * 2 + self.counts[GREAT]
    }

    /// Maximum EX for the chart (every tap Marvelous, every freeze O.K.,
    /// every shock O.K.).
    pub fn ex_max(&self) -> u32 {
        (self.taps + self.freezes + self.shocks) * 3
    }

    /// The money score, exactly as `judge_submit` computes it.
    pub fn score(&self) -> i32 {
        let c = |g: usize| self.counts[g] as i64;
        let total = (self.taps + self.freezes + self.shocks) as i64;
        if total == 0 {
            return 0;
        }
        let num = ((c(MARV) + c(OK) + c(PERF)) * 5 + c(GREAT) * 3 + c(GOOD)) * 200_000;
        let q = (num / (total * 10)) as i32;
        (q - c(GOOD) as i32 - c(GREAT) as i32 - c(PERF) as i32) * 10
    }

    /// Whether every graded note was Marvelous inside the S-Marv window and
    /// the song was an MFC.
    pub fn is_smfc(&self) -> bool {
        self.fc == Some(FullCombo::Marvelous) && self.smarv == self.counts[MARV]
    }
}

/// Community-documented DDR rank thresholds (money score). NOT reverse
/// engineered — see the RE doc §5.
pub const RANKS: [(i32, &str); 15] = [
    (990_000, "AAA"),
    (950_000, "AA+"),
    (900_000, "AA"),
    (890_000, "AA-"),
    (850_000, "A+"),
    (800_000, "A"),
    (790_000, "A-"),
    (750_000, "B+"),
    (700_000, "B"),
    (690_000, "B-"),
    (650_000, "C+"),
    (600_000, "C"),
    (590_000, "C-"),
    (550_000, "D+"),
    (0, "D"),
];

pub fn rank(score: i32, failed: bool) -> &'static str {
    if failed {
        return "E";
    }
    RANKS
        .iter()
        .find(|(min, _)| score >= *min)
        .map(|(_, r)| *r)
        .unwrap_or("D")
}

/// One simulated play, ready for the report.
#[derive(Debug, Clone)]
pub struct Scorecard {
    pub chart: String,
    pub difficulty: Difficulty,
    pub level: u8,
    pub seed_idx: u32,
    pub note_count: u32,
    pub taps: u32,
    pub freezes: u32,
    pub shocks: u32,
    pub duration_ms: i32,
    pub counts: [u32; 8],
    pub smarv: u32,
    pub max_combo: u32,
    pub score: i32,
    pub ex: u32,
    pub ex_max: u32,
    pub fc: Option<FullCombo>,
    pub smfc: bool,
    pub fast: u32,
    pub slow: u32,
    pub gauge_final: i32,
    pub gauge_min: i32,
    pub failed: bool,
    /// Music count at which the gauge emptied (if it did).
    pub failed_at_ms: Option<i32>,
    pub rank: &'static str,
    /// Planner-tally vs judged-grade disagreements (model self-check; 0 =
    /// the planner predicted every grade the judge model assigned).
    pub mismatches: u32,
    /// Timing histogram of graded taps: 25 bins of 10 ms over −125..=125.
    pub delta_hist: [u32; 25],
}

pub fn delta_bin(d: i32) -> usize {
    ((d.clamp(-124, 124) + 125) / 10) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_and_ex_match_the_formula() {
        // 10 taps, 0 freezes, 0 shocks: all Marvelous ⇒ 1,000,000 / EX 30.
        let mut b = Board::new(10, 0, 0, 12);
        for _ in 0..10 {
            b.submit(MARV, 0, false);
        }
        assert_eq!(b.score(), 1_000_000);
        assert_eq!(b.ex(), 30);
        assert_eq!(b.fc, Some(FullCombo::Marvelous));
        assert!(b.is_smfc());

        // 9 Marv + 1 Perfect: (9·5 + 5)·200000/100 = 100000 − 1 = 99999 ·10.
        let mut b = Board::new(10, 0, 0, 12);
        for _ in 0..9 {
            b.submit(MARV, 0, false);
        }
        b.submit(PERF, 20, false);
        assert_eq!(b.score(), 999_990);
        assert_eq!(b.ex(), 29);
        assert_eq!(b.fc, Some(FullCombo::Perfect));
        assert!(!b.is_smfc());
        assert_eq!(b.slow, 1);
    }

    #[test]
    fn freezes_count_as_ok_and_do_not_advance_combo() {
        // 2 taps + 1 freeze (head is one of the taps).
        let mut b = Board::new(2, 1, 0, 12);
        b.submit(MARV, 0, false);
        b.submit(MARV, 0, false);
        assert_eq!(b.fc, None, "FC waits for the freeze O.K.");
        b.submit(OK, 0, true);
        assert_eq!(b.combo, 2);
        assert_eq!(b.fc, Some(FullCombo::Marvelous));
        // (2 + 1)·5·200000 / 30 = 100000 ⇒ 1,000,000.
        assert_eq!(b.score(), 1_000_000);
        assert_eq!(b.ex(), 9);
        assert_eq!(b.ex_max(), 9);
    }

    #[test]
    fn miss_breaks_combo_and_no_fc() {
        let mut b = Board::new(3, 0, 0, 12);
        b.submit(MARV, 0, false);
        b.submit(MISS, 0, false);
        assert_eq!(b.combo, 0);
        b.submit(MARV, 0, false);
        assert_eq!(b.max_combo, 1);
        assert_eq!(b.fc, None);
        // (2·5)·200000/30 = 66666 → ·10.
        assert_eq!(b.score(), 666_660);
    }

    #[test]
    fn shock_ok_counts_toward_fc_and_score() {
        let mut b = Board::new(1, 0, 1, 12);
        b.submit(MARV, 0, false);
        b.submit(OK, 0, false); // the shock avoided (not a tail)
        assert_eq!(b.combo, 2);
        assert_eq!(b.fc, Some(FullCombo::Marvelous));
        assert_eq!(b.score(), 1_000_000);
    }

    #[test]
    fn smarv_window_split() {
        let mut b = Board::new(3, 0, 0, 12);
        b.submit(MARV, 12, false);
        b.submit(MARV, -13, false);
        b.submit(MARV, 17, false);
        assert_eq!(b.smarv, 1);
        assert_eq!(b.counts[MARV], 3);
    }

    #[test]
    fn ranks() {
        assert_eq!(rank(1_000_000, false), "AAA");
        assert_eq!(rank(990_000, false), "AAA");
        assert_eq!(rank(989_990, false), "AA+");
        assert_eq!(rank(549_990, false), "D");
        assert_eq!(rank(1_000_000, true), "E");
    }

    #[test]
    fn delta_bins_cover_the_graded_range() {
        assert_eq!(delta_bin(-124), 0);
        assert_eq!(delta_bin(0), 12);
        assert_eq!(delta_bin(124), 24);
        assert_eq!(delta_bin(-500), 0);
    }
}
