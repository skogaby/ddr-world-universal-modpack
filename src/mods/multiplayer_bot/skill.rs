//! Skill model (design §4.6) — pure.
//!
//! Two curves of the bot level L ∈ 1..=10 decide every note: a Gaussian timing
//! error with std-dev `σ(L)` and a per-note miss probability `p_miss(L)`, both
//! geometric in L between the anchors below. The constants are cabinet
//! tunables — the SHAPE is the design decision.
//!
//! Grade windows are DDR World's (inclusive ms): Marvelous ±17, Perfect ±34,
//! Great ±84, **Good ±124 — the outermost graded window**. The judge's table
//! still carries a ±160 "Boo" row, but its accept test is `grade < 4`, so an
//! event 125..160 ms out is matched and REJECTED: the note stays unjudged
//! until the `mc > note.mc + 160` Miss mark. World has no Boo; a planned
//! offset beyond ±124 IS a Miss. Dependency-free so the host tools can mount it.

// Tuned 2026-09-13 on the full World chart corpus with `scripts/bot_sim.sh`
// (1,586 files × 5 SINGLE difficulties × 10 levels through the judge/gauge
// model): L10 ≈ 71 % MFC-or-S-MFC (the rest PFCs), L1 fails ≈ 60 % of songs
// (7 % Beginner … 95 % Expert), L2 27 %, L3 5 %, L4+ ≈ 0 — a gradual ramp
// instead of the stock L1→L2 cliff. The NORMAL gauge's knee is ≈ 7 % misses,
// so the ramp comes from an explicit `p_miss` decaying slowly (exp 1.4)
// rather than from the Gaussian tail beyond ±124 (σ 60 ⇒ 3.9 %; σ 75 ⇒ 9.8 %).
// Re-run the simulator after ANY change here.

/// Timing-error std-dev at level 1 (ms).
pub const SIGMA_L1_MS: f64 = 60.0;
/// Timing-error std-dev at level 10 (ms). MFC probability is P(Marv)^notes,
/// so this is razor-sensitive: 5.0 ⇒ 86 % MFC+ on the corpus, 5.4 ⇒ 71 %,
/// 6.0 ⇒ 42 %.
pub const SIGMA_L10_MS: f64 = 5.4;
/// Per-note miss probability at level 1 (level 10 is exactly 0).
pub const P_MISS_L1: f64 = 0.13;
/// Exponent shaping how fast `p_miss` falls toward 0 with level
/// (`p₁ · ((10 − L)/9)^exp`; 1.0 = linear).
pub const P_MISS_EXP: f64 = 1.4;

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

/// The two per-level curves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Curve {
    pub sigma_ms: f64,
    pub p_miss: f64,
}

/// `σ(L) = σ₁ · (σ₁₀/σ₁)^((L−1)/9)`, `p(L) = p₁ · ((10−L)/9)^1.5`. Levels
/// outside 1..=10 clamp.
pub fn curve(level: u8) -> Curve {
    let l = level.clamp(1, 10) as f64;
    let t = (l - 1.0) / 9.0;
    let sigma_ms = SIGMA_L1_MS * (SIGMA_L10_MS / SIGMA_L1_MS).powf(t);
    let p_miss = P_MISS_L1 * ((10.0 - l) / 9.0).powf(P_MISS_EXP);
    Curve { sigma_ms, p_miss }
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

/// The per-note decision. `d_ms` is the planned offset of the judge event
/// from the note's own music count (negative = early).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    Hit { d_ms: i32 },
    Miss,
}

/// Decide one note: Miss with probability `p_miss`, else `d = round(σ·z)`,
/// and an offset beyond the Good window is a Miss too (the judge would never
/// grade it).
pub fn decide(rng: &mut Rng, c: &Curve) -> Plan {
    if rng.next_f64() < c.p_miss {
        return Plan::Miss;
    }
    let d = (c.sigma_ms * rng.gaussian()).round();
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
        let mut prev = curve(1);
        assert!((prev.sigma_ms - SIGMA_L1_MS).abs() < 1e-9);
        assert!((prev.p_miss - P_MISS_L1).abs() < 1e-9);
        for level in 2..=10u8 {
            let c = curve(level);
            assert!(c.sigma_ms < prev.sigma_ms, "sigma L{level}");
            assert!(c.p_miss < prev.p_miss, "p_miss L{level}");
            prev = c;
        }
        assert!((prev.sigma_ms - SIGMA_L10_MS).abs() < 1e-9);
        assert_eq!(prev.p_miss, 0.0);
        // Out-of-range levels clamp.
        assert_eq!(curve(0).sigma_ms, curve(1).sigma_ms);
        assert_eq!(curve(99).sigma_ms, curve(10).sigma_ms);
    }

    fn histogram(level: u8, n: usize, seed: u64) -> [u32; 6] {
        let c = curve(level);
        let mut rng = Rng::new(seed);
        let mut h = [0u32; 6];
        for _ in 0..n {
            let g = match decide(&mut rng, &c) {
                Plan::Miss => 5,
                Plan::Hit { d_ms } => grade_for_offset(d_ms),
            };
            h[g as usize] += 1;
        }
        h
    }

    #[test]
    fn level_ten_is_marvelous_and_never_misses() {
        let n = 1_000_000;
        let h = histogram(10, n, 0xDD00_1234);
        let p_marv = h[0] as f64 / n as f64;
        // σ = 5.4 ⇒ erf(17/(5.4√2)) ≈ 0.9983 per note.
        assert!(p_marv >= 0.997, "P(Marv) = {p_marv}");
        assert_eq!(h[5], 0, "no misses at L10: {h:?}");
    }

    #[test]
    fn level_one_spreads_and_misses() {
        // p_miss 13 % + the Gaussian tail beyond ±124 at σ = 60
        // ((1 − 0.13)·2·(1 − Φ(2.067)) ≈ 3.4 %) ⇒ ≈ 16.4 % misses;
        // P(Marv) = 0.87·erf(17/(60√2)) ≈ 0.87·0.222 ≈ 0.19.
        let n = 1_000_000;
        let h = histogram(1, n, 0xBEEF_5678);
        let p_marv = h[0] as f64 / n as f64;
        let p_miss = h[5] as f64 / n as f64;
        assert_eq!(h[4], 0, "grade 4 must never be produced");
        assert!((0.17..=0.22).contains(&p_marv), "P(Marv) = {p_marv}");
        assert!((0.14..=0.19).contains(&p_miss), "P(Miss) = {p_miss}");
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
