//! Skill model (design §4.6) — pure.
//!
//! Every note is decided by four per-level curves (all geometric / linear in
//! the bot level L ∈ 1..=10 between named anchors): the planned judge offset
//! is `d = lean + jitter`, where
//!
//! * **lean** — a systematic off-beat tendency of `±LEAN(L)` ms. It is what
//!   makes exclusive Marvelous MORE common than S-Marvelous: the S-Marv band
//!   `|d| ≤ 12` is 24 ms wide while the Marvelous shell `12 < |d| ≤ 17` is
//!   only 10 ms wide, so ANY zero-centred bell — however wide — puts ≥ 2.4×
//!   more Marvelous-tier hits in the S-Marv band than in the shell. Centring
//!   the tight core ON the shell is the only way round that (the error
//!   density MUST dip at zero; only the magnitude is forced, not the side).
//!   The lean is largest at level 1 (a beginner is further off the beat —
//!   their pocket hits spill into Perfect and rarely reach S-Marvelous),
//!   tapers to the shell centre at the knee level, then relaxes toward zero
//!   so S-Marvelous takes over at the top. The SIDE of each step is a
//!   two-state Markov chain: a per-song late-vs-early bias (`p_late`, drawn
//!   around 50/50) with sticky runs of a few notes (rushing one phrase,
//!   dragging the next) — so a song's FAST/SLOW readout comes out 40/60-ish
//!   with both sides present, like a person's, never 100/0. A slow AR(1)
//!   drift on the MAGNITUDE (a tight stretch, a sloppy stretch — independent
//!   of side) finishes the picture.
//! * **jitter** — a two-regime mixture: with probability `p_tight(L)` the
//!   step is "in the pocket" (motor-floor σ ≈ 3 ms, i.e. a Marvelous-tier
//!   hit around the lean), otherwise "loose" (σ_loose(L), the source of
//!   Perfects / Greats / Goods and the tail Misses beyond ±124).
//! * **p_miss(L)** — an outright per-note Miss (a flubbed step).
//!
//! Grade windows are DDR World's (inclusive ms): Marvelous ±17, Perfect ±34,
//! Great ±84, **Good ±124 — the outermost graded window**. The judge's table
//! still carries a ±160 "Boo" row, but its accept test is `grade < 4`, so an
//! event 125..160 ms out is matched and REJECTED: the note stays unjudged
//! until the `mc > note.mc + 160` Miss mark. World has no Boo; a planned
//! offset beyond ±124 IS a Miss. Dependency-free so the host tools can mount it.

// Tuned 2026-09-14 on the full World chart corpus with `scripts/bot_sim.sh`
// (1,586 files × 5 SINGLE difficulties × 10 levels × 3 seeds through the
// judge/gauge model) to the maintainer's targets after the first cabinet
// playtest — the 2026-09-13 zero-mean Gaussian (σ 60→5.4, p_miss 0.13) was
// 71 % MFC at L10, 60 % fail at L1, ≥ 50 % S-Marvelous from L6 and EX%
// saturated (96.6 / 99.0 / 99.9) over L8–10. Shipped shape (`DEFAULT`):
//
//   L  SMrv% Marv% Perf% Grt% Good% Miss% |  EX%   FC%  PFC% MFC% fail%
//   1   15.3  18.3  28.3 24.1   7.6   6.4 | 62.4   3.5   0.0  0.0  11.6
//   4   20.7  31.8  30.4 14.0   1.8   1.3 | 78.6  16.6   0.1  0.0   0.1
//   7   26.0  46.3  23.2  4.0   0.1   0.4 | 89.7  44.4   2.8  0.0   0.0
//   8   27.9  50.6  19.3  2.0   0.0   0.2 | 92.4  61.0  10.4  0.0   0.0
//   9   42.2  48.1   9.1  0.6   0.0   0.1 | 96.7  80.8  34.4  0.3   0.0
//  10   60.9  36.8   2.3  0.0   0.0   0.0 | 99.2  98.0  98.0  9.8   0.0
//
// i.e. L10 ≈ 10 % MFC corpus-wide (28 % Beginner … 1 % Expert/Challenge —
// P(Marv)^notes), L1 fails ≈ 12 % (2 % Beginner … 20 % Challenge), exclusive
// Marvelous > S-Marvelous through L9 with S-Marvelous ≈ ¼ of steps at L7 and
// dominant only at L10, EX% climbing ≈ 4–5 points per level with no plateau,
// and a fail ramp 12 / 4 / 1 / 0 % over L1–4 (the per-song `form_sd`
// smooths what the NORMAL gauge's sharp miss-rate knee would otherwise make a
// cliff). Every anchor below is a `Params` field the simulator can override
// (`--set key=value`); re-run the simulator after ANY change here.

/// The tunable anchors of the four curves. `DEFAULT` is what the DLL ships;
/// the simulator builds a modified copy for what-ifs and feeds it through the
/// SAME [`curve_from`] so the two can never drift apart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    /// Lean magnitude (ms) at level 1 — a beginner's off-beat lean is
    /// LARGER (it pushes their in-the-pocket hits toward the Marvelous shell
    /// and Perfect, away from S-Marvelous).
    pub lean_l1_ms: f64,
    /// Lean magnitude at `lean_knee_level` (linear from level 1).
    pub lean_knee_ms: f64,
    /// The level the lean reaches `lean_knee_ms`; from there it relaxes
    /// linearly to `lean_l10_ms` — the S-Marvelous take-over.
    pub lean_knee_level: u8,
    /// Lean magnitude at level 10.
    pub lean_l10_ms: f64,
    /// "In the pocket" jitter std-dev (ms) at level 1 / level 10 (geometric).
    pub tight_l1_ms: f64,
    pub tight_l10_ms: f64,
    /// Stationary std-dev of the lean's slow drift, as a ratio of the tight σ.
    pub drift_ratio: f64,
    /// "Loose" jitter std-dev (ms) at level 1 / level 10 (geometric).
    pub loose_l1_ms: f64,
    pub loose_l10_ms: f64,
    /// Probability a step is in the pocket at level 1 (level 10 is exactly 1).
    pub pocket_l1: f64,
    /// Exponent shaping how fast the pocket share rises toward 1
    /// (`1 − (1 − pocket_l1) · ((10 − L)/9)^exp`; 1.0 = linear).
    pub pocket_exp: f64,
    /// Per-note miss probability at level 1 (level 10 is exactly 0).
    pub p_miss_l1: f64,
    /// Exponent shaping how fast `p_miss` falls toward 0
    /// (`p₁ · ((10 − L)/9)^exp`; 1.0 = linear).
    pub p_miss_exp: f64,
    /// Per-song "form" — log-normal std-dev of a factor applied to BOTH the
    /// loose σ and `p_miss` for the whole song (a bad day is wilder AND
    /// flubbier). Smooths the fail-rate ramp across levels: the NORMAL
    /// gauge's knee is sharp in the miss rate, so without per-song variance
    /// the fail% jumps from ~10 % to ~0 between two adjacent levels.
    pub form_sd: f64,
    /// Std-dev of the per-song late-vs-early bias around 50/50 (`p_late =
    /// clamp(0.5 + sd·z, 0.1, 0.9)`): 0.15 ⇒ a typical song is 35–65 % late,
    /// an occasional one 20/80.
    pub late_bias_sd: f64,
    /// Probability a step keeps the previous step's side (else the side is
    /// redrawn from `p_late`). 0.7 ⇒ FAST/SLOW runs of ~3 notes on average.
    pub sign_stickiness: f64,
}

/// The shipped anchors.
pub const DEFAULT: Params = Params {
    lean_l1_ms: 17.0,
    lean_knee_ms: 14.5,
    lean_knee_level: 8,
    lean_l10_ms: 11.7,
    tight_l1_ms: 3.5,
    tight_l10_ms: 2.5,
    drift_ratio: 0.6,
    loose_l1_ms: 60.0,
    loose_l10_ms: 12.0,
    pocket_l1: 0.35,
    pocket_exp: 1.0,
    p_miss_l1: 0.015,
    p_miss_exp: 1.4,
    form_sd: 0.35,
    late_bias_sd: 0.15,
    sign_stickiness: 0.7,
};

/// Per-note autocorrelation of the lean drift (decorrelates over ~30 notes).
pub const DRIFT_RHO: f64 = 0.97;

/// Half-width of the outermost GRADED window (Good). An event beyond it is
/// never graded — the note becomes a Miss.
pub const GOOD_WINDOW_MS: i32 = 124;
/// The judge marks a note Missed once `mc > note.mc + MISS_WINDOW_MS`.
pub const MISS_WINDOW_MS: i32 = 160;
/// Grade value the game assigns to a Miss.
pub const GRADE_MISS: u8 = 5;

/// Inclusive half-widths of the graded windows, index = grade
/// (0 Marvelous … 3 Good).
const WINDOW_HALF_MS: [i32; 4] = [17, 34, 84, 124];

/// The per-level curves, evaluated once per song.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Curve {
    pub lean_ms: f64,
    pub tight_ms: f64,
    pub drift_ms: f64,
    pub loose_ms: f64,
    pub p_tight: f64,
    pub p_miss: f64,
    pub form_sd: f64,
    pub late_bias_sd: f64,
    pub sign_stickiness: f64,
}

impl Curve {
    /// One-line description for logs.
    pub fn describe(&self) -> String {
        format!(
            "lean={:.1}ms tight={:.1}ms drift={:.1}ms loose={:.1}ms pocket={:.0}% p_miss={:.2}% form_sd={:.2}",
            self.lean_ms,
            self.tight_ms,
            self.drift_ms,
            self.loose_ms,
            self.p_tight * 100.0,
            self.p_miss * 100.0,
            self.form_sd
        )
    }
}

/// The shipped curves ([`DEFAULT`]). Levels outside 1..=10 clamp.
pub fn curve(level: u8) -> Curve {
    curve_from(&DEFAULT, level)
}

/// Evaluate the curves at `level` for any anchors.
pub fn curve_from(p: &Params, level: u8) -> Curve {
    let l = level.clamp(1, 10) as f64;
    let t = (l - 1.0) / 9.0; // 0 at L1 … 1 at L10
    let u = (10.0 - l) / 9.0; // 1 at L1 … 0 at L10
    let knee = p.lean_knee_level.clamp(1, 9) as f64;
    let lean_ms = if l <= knee {
        p.lean_l1_ms + (p.lean_knee_ms - p.lean_l1_ms) * (l - 1.0) / (knee - 1.0).max(1.0)
    } else {
        p.lean_knee_ms + (p.lean_l10_ms - p.lean_knee_ms) * (l - knee) / (10.0 - knee)
    };
    let tight_ms = p.tight_l1_ms * (p.tight_l10_ms / p.tight_l1_ms).powf(t);
    Curve {
        lean_ms,
        tight_ms,
        drift_ms: p.drift_ratio * tight_ms,
        loose_ms: p.loose_l1_ms * (p.loose_l10_ms / p.loose_l1_ms).powf(t),
        p_tight: 1.0 - (1.0 - p.pocket_l1) * u.powf(p.pocket_exp),
        p_miss: p.p_miss_l1 * u.powf(p.p_miss_exp),
        form_sd: p.form_sd,
        late_bias_sd: p.late_bias_sd,
        sign_stickiness: p.sign_stickiness.clamp(0.0, 1.0),
    }
}

/// xorshift64* seeded through splitmix64 (so a zero seed is not a fixed point).
#[derive(Debug, Clone)]
pub struct Rng(u64);

fn splitmix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut s = splitmix64(seed);
        if s == 0 {
            s = 0x9E37_79B9_7F4A_7C15;
        }
        Rng(s)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Standard normal (Marsaglia polar method).
    pub fn gaussian(&mut self) -> f64 {
        loop {
            let u = 2.0 * self.next_f64() - 1.0;
            let v = 2.0 * self.next_f64() - 1.0;
            let s = u * u + v * v;
            if s > 0.0 && s < 1.0 {
                return u * (-2.0 * s.ln() / s).sqrt();
            }
        }
    }
}

/// The dancer's per-song state: their late-vs-early bias, which side the
/// current run is on, where the magnitude drift sits and today's form.
/// Rolled once per song (a restart re-rolls).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Form {
    /// This song's probability that a step is LATE (`+`); the Markov chain's
    /// stationary distribution.
    pub p_late: f64,
    /// Side of the current run: +1 late (SLOW) / −1 early (FAST).
    pub sign: f64,
    /// Current AR(1) drift of the lean MAGNITUDE (ms, signed: negative =
    /// closer to the beat), stationary std-dev `drift_ms`.
    pub drift: f64,
    /// Today's form: multiplies the loose σ and `p_miss` (log-normal, median 1).
    pub factor: f64,
}

impl Form {
    pub fn new(rng: &mut Rng, c: &Curve) -> Self {
        let p_late = (0.5 + c.late_bias_sd * rng.gaussian()).clamp(0.1, 0.9);
        Form {
            p_late,
            sign: Self::draw_sign(rng, p_late),
            drift: c.drift_ms * rng.gaussian(),
            factor: (c.form_sd * rng.gaussian()).exp(),
        }
    }

    fn draw_sign(rng: &mut Rng, p_late: f64) -> f64 {
        if rng.next_f64() < p_late {
            1.0
        } else {
            -1.0
        }
    }

    /// The lean for the next note: keep or redraw the side (sticky Markov
    /// chain), advance the drift one step.
    fn next_lean(&mut self, rng: &mut Rng, c: &Curve) -> f64 {
        if rng.next_f64() >= c.sign_stickiness {
            self.sign = Self::draw_sign(rng, self.p_late);
        }
        let innovation = c.drift_ms * (1.0 - DRIFT_RHO * DRIFT_RHO).sqrt();
        self.drift = DRIFT_RHO * self.drift + innovation * rng.gaussian();
        // The drift wanders the MAGNITUDE (how far off the beat), the chain
        // picks the side — so a "tight stretch" stays tight across a side
        // flip, which is what lets a whole song come out all-Marvelous.
        self.sign * (c.lean_ms + self.drift)
    }
}

/// The per-note decision. `d_ms` is the planned offset of the judge event
/// from the note's own music count (negative = early).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    Hit { d_ms: i32 },
    Miss,
}

/// Decide one note: Miss with probability `p_miss`, else
/// `d = round(lean + jitter)` with the jitter drawn from the pocket (tight)
/// or the loose regime, and an offset beyond the Good window is a Miss too
/// (the judge would never grade it).
pub fn decide(rng: &mut Rng, form: &mut Form, c: &Curve) -> Plan {
    if rng.next_f64() < c.p_miss * form.factor {
        return Plan::Miss;
    }
    let lean = form.next_lean(rng, c);
    let sigma = if rng.next_f64() < c.p_tight {
        c.tight_ms
    } else {
        c.loose_ms * form.factor
    };
    let d = (lean + sigma * rng.gaussian()).round();
    if d.abs() > GOOD_WINDOW_MS as f64 {
        return Plan::Miss;
    }
    Plan::Hit { d_ms: d as i32 }
}

/// Grade the game assigns to an event `d_ms` from the note: 0 Marvelous,
/// 1 Perfect, 2 Great, 3 Good, else [`GRADE_MISS`] (never 4 — World's judge
/// rejects the Boo row).
pub fn grade_for_offset(d_ms: i32) -> u8 {
    let a = d_ms.unsigned_abs();
    for (g, &half) in WINDOW_HALF_MS.iter().enumerate() {
        if a <= half as u32 {
            return g as u8;
        }
    }
    GRADE_MISS
}

/// Per-play seed: `splitmix64(qpc ^ (mcode << 32) ^ (difficulty << 8) ^ level)`.
pub fn seed(qpc: u64, mcode: i32, difficulty: i32, level: u8) -> u64 {
    splitmix64(
        qpc ^ ((mcode as u32 as u64) << 32) ^ ((difficulty as u32 as u64) << 8) ^ level as u64,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// S-Marvelous presentation window the histograms split Marvelous on.
    const SMARV_MS: i32 = 12;

    #[test]
    fn grade_windows_exact() {
        for d in [0, 17, -17] {
            assert_eq!(grade_for_offset(d), 0, "d={d}");
        }
        for d in [18, -18, 34, -34] {
            assert_eq!(grade_for_offset(d), 1, "d={d}");
        }
        for d in [35, -35, 84, -84] {
            assert_eq!(grade_for_offset(d), 2, "d={d}");
        }
        for d in [85, -85, 124, -124] {
            assert_eq!(grade_for_offset(d), 3, "d={d}");
        }
        // World has no Boo: 125..160 is rejected by the judge, i.e. a Miss.
        for d in [125, -125, 160, -160, 161, -161, 10_000, i32::MIN, i32::MAX] {
            assert_eq!(grade_for_offset(d), GRADE_MISS, "d={d}");
        }
    }

    #[test]
    fn curves_monotonic_and_anchored() {
        let first = curve(1);
        assert!((first.lean_ms - DEFAULT.lean_l1_ms).abs() < 1e-9);
        assert!((first.tight_ms - DEFAULT.tight_l1_ms).abs() < 1e-9);
        assert!((first.loose_ms - DEFAULT.loose_l1_ms).abs() < 1e-9);
        assert!((first.p_tight - DEFAULT.pocket_l1).abs() < 1e-9);
        assert!((first.p_miss - DEFAULT.p_miss_l1).abs() < 1e-9);
        let mut prev = first;
        for level in 2..=10u8 {
            let c = curve(level);
            assert!(c.tight_ms < prev.tight_ms, "tight L{level}");
            assert!(c.loose_ms < prev.loose_ms, "loose L{level}");
            assert!(c.p_tight > prev.p_tight, "pocket L{level}");
            assert!(c.p_miss < prev.p_miss, "p_miss L{level}");
            assert!(c.lean_ms <= prev.lean_ms, "lean L{level}");
            assert!((c.drift_ms - DEFAULT.drift_ratio * c.tight_ms).abs() < 1e-9);
            prev = c;
        }
        assert!((prev.lean_ms - DEFAULT.lean_l10_ms).abs() < 1e-9);
        assert!((prev.tight_ms - DEFAULT.tight_l10_ms).abs() < 1e-9);
        assert!((prev.loose_ms - DEFAULT.loose_l10_ms).abs() < 1e-9);
        assert_eq!(prev.p_tight, 1.0);
        assert_eq!(prev.p_miss, 0.0);
        // The lean tapers to the knee value and relaxes faster past it.
        assert!((curve(DEFAULT.lean_knee_level).lean_ms - DEFAULT.lean_knee_ms).abs() < 1e-9);
        assert!(curve(DEFAULT.lean_knee_level + 1).lean_ms < DEFAULT.lean_knee_ms);
        // Out-of-range levels clamp.
        assert_eq!(curve(0), curve(1));
        assert_eq!(curve(99), curve(10));
        // The simulator's override path is the same function.
        assert_eq!(curve_from(&DEFAULT, 7), curve(7));
    }

    /// Grade histogram over `n` decided notes: `[smarv, marv_excl, perf,
    /// great, good, miss]`, re-rolling the per-song form every `song` notes so
    /// both lean signs and many drift paths are sampled.
    fn histogram(level: u8, n: usize, seed: u64) -> [u32; 6] {
        let c = curve(level);
        let mut rng = Rng::new(seed);
        let mut form = Form::new(&mut rng, &c);
        let mut h = [0u32; 6];
        for i in 0..n {
            if i % 400 == 0 {
                form = Form::new(&mut rng, &c);
            }
            match decide(&mut rng, &mut form, &c) {
                Plan::Miss => h[5] += 1,
                Plan::Hit { d_ms } => match grade_for_offset(d_ms) {
                    0 if d_ms.abs() <= SMARV_MS => h[0] += 1,
                    0 => h[1] += 1,
                    g => h[1 + g as usize] += 1,
                },
            }
        }
        h
    }

    fn shares(h: &[u32; 6]) -> [f64; 6] {
        let n: u32 = h.iter().sum();
        h.map(|v| v as f64 / n as f64)
    }

    #[test]
    fn level_ten_is_marvelous_and_never_misses() {
        let h = histogram(10, 1_000_000, 0xDD00_1234);
        let s = shares(&h);
        let p_marv = s[0] + s[1];
        // lean 11.7 ± eff σ ≈ 2.9 ⇒ P(|d| ≤ 17) ≈ 0.98 — the razor edge the
        // ~10 % corpus MFC rate sits on (P(Marv)^notes; short Beginner charts
        // carry most of it).
        assert!((0.970..=0.990).contains(&p_marv), "P(Marv) = {p_marv}");
        assert_eq!(h[5], 0, "no misses at L10: {h:?}");
        assert_eq!(h[3] + h[4], 0, "no Greats/Goods at L10: {h:?}");
        // The top level is where S-Marvelous takes over.
        assert!(s[0] > s[1], "S-Marv {} vs Marv {}", s[0], s[1]);
    }

    #[test]
    fn marvelous_outnumbers_smarvelous_through_the_knee() {
        for level in 1..=DEFAULT.lean_knee_level {
            let s = shares(&histogram(level, 400_000, 0xC0FFEE + level as u64));
            assert!(
                s[1] > s[0],
                "L{level}: S-Marv {:.3} must be below exclusive Marv {:.3}",
                s[0],
                s[1]
            );
        }
    }

    #[test]
    fn smarvelous_is_rare_low_and_common_high() {
        let l1 = shares(&histogram(1, 400_000, 1));
        let l7 = shares(&histogram(7, 400_000, 7));
        let l10 = shares(&histogram(10, 400_000, 10));
        assert!(l1[0] > 0.02, "S-Marv attainable at L1: {}", l1[0]);
        assert!(l1[0] < 0.20, "S-Marv rare at L1: {}", l1[0]);
        assert!(l7[0] > l1[0] && l10[0] > l7[0], "S-Marv rises with level");
        assert!(l10[0] > 0.6, "S-Marv dominant at L10: {}", l10[0]);
    }

    #[test]
    fn level_one_spreads_and_misses() {
        // p_miss 1.5 % + the loose tail beyond ±124 (σ 60 × form around ±17,
        // 65 % of notes loose) ⇒ ≈ 5–8 % misses (the NORMAL gauge fails
        // ≈ 10 % of corpus songs at that rate); a wide grade spread.
        let s = shares(&histogram(1, 1_000_000, 0xBEEF_5678));
        let p_miss = s[5];
        assert!((0.045..=0.085).contains(&p_miss), "P(Miss) = {p_miss}");
        for (name, share) in ["smarv", "marv", "perf", "great", "good"]
            .iter()
            .zip(s.iter())
        {
            assert!(*share > 0.05, "L1 {name} share {share} too small");
        }
    }

    #[test]
    fn side_is_a_biased_sticky_chain_not_a_per_song_constant() {
        let c = curve(10); // pocket 100 %: every hit's sign IS the chain's
        let mut rng = Rng::new(99);
        let songs = 2000;
        let notes = 300;
        let mut late_shares = Vec::with_capacity(songs);
        let mut runs = 0u32;
        let mut both_sides = 0u32;
        for _ in 0..songs {
            let mut form = Form::new(&mut rng, &c);
            assert!((0.1..=0.9).contains(&form.p_late), "p_late {}", form.p_late);
            let mut late = 0u32;
            let mut prev = 0.0;
            for i in 0..notes {
                let d = form.next_lean(&mut rng, &c);
                let sign = d.signum();
                late += (sign > 0.0) as u32;
                if i > 0 && sign != prev {
                    runs += 1;
                }
                prev = sign;
            }
            let share = late as f64 / notes as f64;
            late_shares.push(share);
            both_sides += (late > 0 && late < notes) as u32;
        }
        // Every song shows BOTH FAST and SLOW steps …
        assert_eq!(both_sides as usize, songs, "a song came out all one side");
        // … the typical split is off 50/50 but not extreme …
        let mean = late_shares.iter().sum::<f64>() / songs as f64;
        let sd =
            (late_shares.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / songs as f64).sqrt();
        assert!((0.45..=0.55).contains(&mean), "mean late share {mean}");
        assert!((0.10..=0.20).contains(&sd), "late-share spread {sd}");
        assert!(
            late_shares.iter().any(|&x| x < 0.35) && late_shares.iter().any(|&x| x > 0.65),
            "some songs should lean clearly one way"
        );
        // … and the side comes in runs (mean run ≈ 1/((1−q)·2·p(1−p)) ≈ 6–7
        // notes at q 0.7, p ≈ 0.5), not per-note coin flips (mean run 2).
        let mean_run = (songs as f64 * notes as f64) / (runs as f64 + songs as f64);
        assert!((3.0..=12.0).contains(&mean_run), "mean run {mean_run}");
    }

    #[test]
    fn drift_is_bounded_and_form_factor_is_lognormal() {
        let c = curve(5);
        let mut rng = Rng::new(98);
        // The drift's sample std-dev stays near its stationary value.
        let mut form = Form::new(&mut rng, &c);
        let (mut s, mut s2, n) = (0.0, 0.0, 200_000);
        for _ in 0..n {
            let lean = form.next_lean(&mut rng, &c);
            let d = lean.abs() - c.lean_ms; // the magnitude drift
            s += d;
            s2 += d * d;
        }
        let var = s2 / n as f64 - (s / n as f64).powi(2);
        assert!(
            (var.sqrt() - c.drift_ms).abs() < 0.3,
            "drift sd {} vs {}",
            var.sqrt(),
            c.drift_ms
        );
        // The form factor is log-normal with median 1: half the songs are
        // above 1, essentially none beyond ×4 / ÷4 at sd 0.35.
        let mut above = 0;
        for _ in 0..1000 {
            let f = Form::new(&mut rng, &c).factor;
            assert!(f > 0.2 && f < 5.0, "factor {f}");
            above += (f > 1.0) as u32;
        }
        assert!((400..=600).contains(&above), "factor median {above}");
    }

    #[test]
    fn side_mixing_leaves_the_magnitude_untouched() {
        // Grades depend on |d| only, so the Markov side chain must not move
        // the tuned grade mix: compare the histogram with the sign frozen.
        let c = curve(7);
        let frozen = Curve {
            late_bias_sd: 0.0,
            sign_stickiness: 1.0,
            ..c
        };
        let n = 400_000;
        let mix = shares(&histogram(7, n, 0x51DE));
        let mut rng = Rng::new(0x51DE);
        let mut form = Form::new(&mut rng, &frozen);
        let mut h = [0u32; 6];
        for i in 0..n {
            if i % 400 == 0 {
                form = Form::new(&mut rng, &frozen);
            }
            match decide(&mut rng, &mut form, &frozen) {
                Plan::Miss => h[5] += 1,
                Plan::Hit { d_ms } => match grade_for_offset(d_ms) {
                    0 if d_ms.abs() <= SMARV_MS => h[0] += 1,
                    0 => h[1] += 1,
                    g => h[1 + g as usize] += 1,
                },
            }
        }
        let fro = shares(&h);
        for (i, (a, b)) in mix.iter().zip(fro.iter()).enumerate() {
            assert!((a - b).abs() < 0.01, "bucket {i}: mixed {a} vs frozen {b}");
        }
    }

    #[test]
    fn rng_deterministic_and_zero_seed_advances() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        let mut z = Rng::new(0);
        let first = z.next_u64();
        let second = z.next_u64();
        assert_ne!(first, 0);
        assert_ne!(first, second);
        for _ in 0..10_000 {
            let f = z.next_f64();
            assert!((0.0..1.0).contains(&f));
        }
    }

    #[test]
    fn gaussian_has_unit_variance() {
        let mut rng = Rng::new(7);
        let n = 200_000;
        let (mut s, mut s2) = (0.0f64, 0.0f64);
        for _ in 0..n {
            let z = rng.gaussian();
            s += z;
            s2 += z * z;
        }
        let mean = s / n as f64;
        let var = s2 / n as f64 - mean * mean;
        assert!(mean.abs() < 0.01, "mean {mean}");
        assert!((var - 1.0).abs() < 0.02, "var {var}");
    }

    #[test]
    fn seed_mixes_every_input() {
        let base = seed(1, 2, 3, 4);
        assert_ne!(base, seed(2, 2, 3, 4));
        assert_ne!(base, seed(1, 3, 3, 4));
        assert_ne!(base, seed(1, 2, 4, 4));
        assert_ne!(base, seed(1, 2, 3, 5));
        assert_eq!(base, seed(1, 2, 3, 4));
    }
}
