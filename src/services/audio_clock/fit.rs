//! Pure sliding-window least-squares fit of the DirectSound PLAY cursor (in
//! frames) against QPC — the "fixed-lag smoother" of the deterministic audio
//! clock (design §2). Host-tested; no atomics, no allocation after `new`.
//!
//! Why a fit at all: the engine samples the DAC position once per 10 ms mix
//! pass through `IDirectSoundBuffer::GetCurrentPosition`, and on every platform
//! measured so far that position is a STAIRCASE (512 frames = 11.6 ms steps
//! under CrossOver; Win7 expected 10 ms-class). A single cursor read therefore
//! carries up to one step of error; the long-run slope of the staircase is
//! the DAC rate exactly, and its phase is recovered to a fraction of a step by
//! averaging a few seconds of reads. The extrapolation horizon is ONE pass
//! (the game evaluates the line at most ~20 ms after the newest sample), so
//! slope precision is irrelevant — a 100 ppm slope error is < 2 µs there.
//! This is NOT the rejected four-minute rate estimator.
//!
//! Numerics: all sums are exact `i128` over values relative to a per-window
//! origin that is re-centered (rebuilt from the ring, O(n)) whenever the
//! newest sample is more than [`RECENTER_WINDOWS`] windows past the origin.
//! With the origin that close, every partial product stays far inside `i128`
//! and the final f64 divisions see no catastrophic cancellation.

/// Ring capacity in samples. 100 passes/s × 60 s (the largest window the
/// config accepts) = 6000; 8192 leaves headroom for faster pass cadences.
pub const CAPACITY: usize = 8192;

/// Samples required before the line is trusted (~2.5 s of passes).
pub const DEFAULT_READY_SAMPLES: usize = 256;

/// Re-center the integer origin once the newest sample is this many windows
/// past it (bounds every intermediate `i128` product).
const RECENTER_WINDOWS: i64 = 2;

/// One mix-pass observation, already converted to FRAMES (bytes / blockalign).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Sample {
    /// QPC ticks at the cursor read (midpoint of the pre/post bracket).
    pub t: i64,
    /// Accumulated PLAY-cursor frames (`DS+0xD0 / blockalign`).
    pub p: i64,
    /// Accumulated WRITE-cursor frames (`DS+0xC8 / blockalign`).
    pub wc: i64,
    /// Frames written so far (`DS+0xC0 / blockalign`), read BEFORE this
    /// pass's Commit — the first frame of the block this pass writes.
    pub w: i64,
}

impl Sample {
    /// Frames queued ahead of the DirectSound write cursor before this
    /// pass's block is written.
    #[must_use]
    pub fn lead(&self) -> i64 {
        self.w - self.wc
    }

    /// Frames between the DirectSound write and play cursors.
    #[must_use]
    pub fn margin(&self) -> i64 {
        self.wc - self.p
    }
}

/// Why the fit discarded its history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reset {
    /// The accumulated play position went backwards (backend Stop zeroes the
    /// accumulators; a new backend object starts from zero).
    PlayDecreased,
    /// The engine's underrun clamp fired (`Wc > W` ⇒ `Wc := W`): the ring
    /// ran dry, so the cursor/write relationship is no longer steady-state.
    Underrun,
    /// More than `max_gap_ticks` since the previous pass (render thread
    /// stalled or was suspended).
    Gap,
    /// QPC went backwards between passes.
    TimeNotMonotonic,
    /// The caller's format changed (Hz / blockalign / backend) — the frame
    /// domains are not comparable.
    FormatChanged,
    /// Explicit reset by the caller.
    Requested,
}

/// Outcome of [`Fit::push`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Push {
    /// The sample joined the window.
    Accepted,
    /// The history was discarded for the given reason; the sample became
    /// the first of a fresh window.
    Reset(Reset),
}

/// Window / validity parameters (ticks are QPC ticks).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FitConfig {
    /// History retained behind the newest sample.
    pub window_ticks: i64,
    /// Largest accepted inter-pass gap.
    pub max_gap_ticks: i64,
    /// Samples before [`Line::ready`].
    pub ready_samples: usize,
}

impl FitConfig {
    /// Build from wall-time parameters and the QPC frequency.
    #[must_use]
    pub fn from_seconds(frequency: i64, window_seconds: u32, max_gap_ms: u32) -> Self {
        let window_seconds = i64::from(window_seconds.clamp(2, 60));
        Self {
            window_ticks: frequency.saturating_mul(window_seconds),
            max_gap_ticks: frequency
                .saturating_mul(i64::from(max_gap_ms.max(1)))
                .saturating_div(1000),
            ready_samples: DEFAULT_READY_SAMPLES,
        }
    }
}

/// The published straight line `P̂(t) = p_ref + slope · (t − t_ref)` plus
/// the statistics a consumer needs to trust it. `Copy` so it can be
/// published through a seqlock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    /// Samples in the window.
    pub n: usize,
    /// Reference time — the NEWEST sample's `t` (so evaluation never
    /// extrapolates further than one frame past the data).
    pub t_ref: i64,
    /// `P̂(t_ref)` in frames.
    pub p_ref: f64,
    /// Frames per QPC tick.
    pub slope: f64,
    /// Residual standard deviation of the play cursor about the line
    /// (frames) — the cursor staircase's size shows up here.
    pub resid_sd: f64,
    /// Mean of `w − wc` over the window (frames).
    pub mean_lead: f64,
    /// Mean of `wc − p` over the window (frames).
    pub mean_margin: f64,
    /// Mean frames written per pass (`ΔW` between consecutive passes).
    pub pass_frames: f64,
    /// Whether `n ≥ ready_samples` and the slope is well defined.
    pub ready: bool,
}

impl Line {
    /// Evaluate the fitted DAC position (frames) at `t`.
    #[must_use]
    pub fn eval(&self, t: i64) -> f64 {
        self.p_ref + self.slope * ((t - self.t_ref) as f64)
    }

    /// The `raw`-mode line through one sample: `P_k + (t − t_k) · Hz`.
    #[must_use]
    pub fn raw(sample: Sample, hz: u32, frequency: i64) -> Self {
        Self {
            n: 1,
            t_ref: sample.t,
            p_ref: sample.p as f64,
            slope: if frequency > 0 {
                f64::from(hz) / frequency as f64
            } else {
                0.0
            },
            resid_sd: 0.0,
            mean_lead: sample.lead() as f64,
            mean_margin: sample.margin() as f64,
            pass_frames: 0.0,
            ready: frequency > 0 && hz > 0,
        }
    }
}

/// Sliding-window LSQ state. Single-owner (the render thread); publish the
/// [`Line`] to other threads, never share the `Fit` itself.
pub struct Fit {
    cfg: FitConfig,
    ring: Box<[Sample]>,
    /// Index of the oldest sample.
    head: usize,
    len: usize,
    origin_t: i64,
    origin_p: i64,
    s_t: i128,
    s_p: i128,
    s_tt: i128,
    s_tp: i128,
    s_pp: i128,
    s_lead: i128,
    s_margin: i128,
}

impl Fit {
    /// Allocates the ring ONCE (call at init on the game thread).
    #[must_use]
    pub fn new(cfg: FitConfig) -> Self {
        Self {
            cfg,
            ring: vec![Sample::default(); CAPACITY].into_boxed_slice(),
            head: 0,
            len: 0,
            origin_t: 0,
            origin_p: 0,
            s_t: 0,
            s_p: 0,
            s_tt: 0,
            s_tp: 0,
            s_pp: 0,
            s_lead: 0,
            s_margin: 0,
        }
    }

    #[must_use]
    pub fn config(&self) -> FitConfig {
        self.cfg
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[must_use]
    pub fn newest(&self) -> Option<Sample> {
        (self.len > 0).then(|| self.ring[(self.head + self.len - 1) % CAPACITY])
    }

    #[must_use]
    pub fn oldest(&self) -> Option<Sample> {
        (self.len > 0).then(|| self.ring[self.head])
    }

    /// Discard all history.
    pub fn clear(&mut self) {
        self.len = 0;
        self.head = 0;
        self.s_t = 0;
        self.s_p = 0;
        self.s_tt = 0;
        self.s_tp = 0;
        self.s_pp = 0;
        self.s_lead = 0;
        self.s_margin = 0;
    }

    /// Feed one pass. `underrun` = the engine's clamp fired on this read.
    pub fn push(&mut self, sample: Sample, underrun: bool) -> Push {
        let reset = match self.newest() {
            Some(_) if underrun => Some(Reset::Underrun),
            Some(prev) if sample.t < prev.t => Some(Reset::TimeNotMonotonic),
            Some(prev) if sample.p < prev.p => Some(Reset::PlayDecreased),
            Some(prev) if sample.t - prev.t > self.cfg.max_gap_ticks => Some(Reset::Gap),
            None if underrun => Some(Reset::Underrun),
            _ => None,
        };
        if let Some(reason) = reset {
            self.clear();
            self.start(sample);
            return Push::Reset(reason);
        }
        if self.len == 0 {
            self.start(sample);
            return Push::Accepted;
        }
        // Slide: drop everything older than the window (and never overflow).
        while self.len > 0
            && (self.len == CAPACITY || sample.t - self.ring[self.head].t > self.cfg.window_ticks)
        {
            self.pop_oldest();
        }
        if self.len == 0 {
            self.start(sample);
            return Push::Accepted;
        }
        if sample.t - self.origin_t > self.cfg.window_ticks.saturating_mul(RECENTER_WINDOWS) {
            self.recenter();
        }
        self.append(sample);
        Push::Accepted
    }

    /// Explicit reset (format change, backend change, caller policy).
    pub fn reset(&mut self, reason: Reset) -> Push {
        self.clear();
        Push::Reset(reason)
    }

    fn start(&mut self, sample: Sample) {
        self.origin_t = sample.t;
        self.origin_p = sample.p;
        self.append(sample);
    }

    fn append(&mut self, sample: Sample) {
        let slot = (self.head + self.len) % CAPACITY;
        self.ring[slot] = sample;
        self.len += 1;
        self.add(sample, 1);
    }

    fn pop_oldest(&mut self) {
        let sample = self.ring[self.head];
        self.head = (self.head + 1) % CAPACITY;
        self.len -= 1;
        self.add(sample, -1);
    }

    fn add(&mut self, sample: Sample, sign: i128) {
        let t = i128::from(sample.t - self.origin_t);
        let p = i128::from(sample.p - self.origin_p);
        self.s_t += sign * t;
        self.s_p += sign * p;
        self.s_tt += sign * t * t;
        self.s_tp += sign * t * p;
        self.s_pp += sign * p * p;
        self.s_lead += sign * i128::from(sample.lead());
        self.s_margin += sign * i128::from(sample.margin());
    }

    /// Rebuild the sums about the oldest sample (bounded `i128` magnitudes).
    fn recenter(&mut self) {
        let Some(oldest) = self.oldest() else {
            return;
        };
        self.origin_t = oldest.t;
        self.origin_p = oldest.p;
        self.s_t = 0;
        self.s_p = 0;
        self.s_tt = 0;
        self.s_tp = 0;
        self.s_pp = 0;
        self.s_lead = 0;
        self.s_margin = 0;
        for i in 0..self.len {
            let sample = self.ring[(self.head + i) % CAPACITY];
            self.add(sample, 1);
        }
    }

    /// The current line, or `None` with fewer than two distinct times.
    #[must_use]
    pub fn line(&self) -> Option<Line> {
        let newest = self.newest()?;
        let n = self.len as i128;
        if n < 2 {
            return None;
        }
        let den = n * self.s_tt - self.s_t * self.s_t;
        if den <= 0 {
            return None;
        }
        let num = n * self.s_tp - self.s_t * self.s_p;
        let slope = num as f64 / den as f64;
        // Means in the origin-relative frame, then the value at the newest t.
        let n_f = n as f64;
        let t_mean = self.s_t as f64 / n_f;
        let p_mean = self.s_p as f64 / n_f;
        let t_new = (newest.t - self.origin_t) as f64;
        let p_ref = self.origin_p as f64 + p_mean + slope * (t_new - t_mean);
        // Residuals: n·SS_res = n·S_pp − (n·S_tp)² / (n·S_tt), all exact
        // until the final f64 division.
        let n_spp = (n * self.s_pp - self.s_p * self.s_p) as f64;
        let n_ssres = (n_spp - (num as f64) * (num as f64) / den as f64).max(0.0);
        let resid_sd = if n > 2 {
            (n_ssres / n_f / (n_f - 2.0)).sqrt()
        } else {
            0.0
        };
        let oldest = self.oldest()?;
        let pass_frames = if self.len > 1 {
            (newest.w - oldest.w) as f64 / (self.len as f64 - 1.0)
        } else {
            0.0
        };
        Some(Line {
            n: self.len,
            t_ref: newest.t,
            p_ref,
            slope,
            resid_sd,
            mean_lead: self.s_lead as f64 / n_f,
            mean_margin: self.s_margin as f64 / n_f,
            pass_frames,
            ready: self.len >= self.cfg.ready_samples && slope > 0.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FREQ: i64 = 10_000_000; // 10 MHz QPC
    const HZ: i64 = 44_100;
    const PASS_TICKS: i64 = FREQ / 100; // 10 ms passes
    const PASS_FRAMES: i64 = 441;

    fn cfg() -> FitConfig {
        FitConfig::from_seconds(FREQ, 10, 200)
    }

    /// Synthetic engine: true DAC position `p_true(t) = rate·t` (frames per
    /// tick scaled by `ppm`), reported as a `step`-frame staircase; write
    /// cursor = play + 441; W = wc + lead (sawtooth 40..50 ms).
    struct Engine {
        step: i64,
        ppm: f64,
        jitter_ticks: i64,
        k: i64,
    }

    impl Engine {
        fn true_p(&self, t: i64) -> f64 {
            (t as f64) * (HZ as f64 / FREQ as f64) * (1.0 + self.ppm * 1e-6)
        }

        fn sample_at(&mut self, t: i64) -> Sample {
            let p_true = self.true_p(t).floor() as i64;
            let p = if self.step > 1 {
                p_true - p_true.rem_euclid(self.step)
            } else {
                p_true
            };
            // The engine's own bookkeeping: the write cursor rides the play
            // cursor (+10 ms under Wine), W advances EXACTLY one block per
            // pass; the lead's wobble is the staircase showing through.
            let wc = p + PASS_FRAMES;
            let w = 5 * PASS_FRAMES + self.k * PASS_FRAMES;
            self.k += 1;
            Sample { t, p, wc, w }
        }

        fn pass_time(&self, k: i64) -> i64 {
            // Deterministic pseudo-jitter of the pass instant.
            let j = if self.jitter_ticks > 0 {
                ((k * 7919) % (2 * self.jitter_ticks + 1)) - self.jitter_ticks
            } else {
                0
            };
            1_000_000_000 + k * PASS_TICKS + j
        }
    }

    fn run(engine: &mut Engine, fit: &mut Fit, passes: i64) -> Vec<Sample> {
        let mut out = Vec::new();
        for k in 0..passes {
            let t = engine.pass_time(k);
            let s = engine.sample_at(t);
            assert_eq!(fit.push(s, false), Push::Accepted, "pass {k}");
            out.push(s);
        }
        out
    }

    #[test]
    fn smooth_cursor_is_recovered_exactly() {
        let mut fit = Fit::new(cfg());
        let mut e = Engine {
            step: 1,
            ppm: 0.0,
            jitter_ticks: 0,
            k: 0,
        };
        run(&mut e, &mut fit, 500);
        let line = fit.line().unwrap();
        assert!(line.ready);
        let t = e.pass_time(500) + PASS_TICKS / 2;
        let err = line.eval(t) - e.true_p(t);
        assert!(err.abs() < 1.0, "phase error {err} frames");
        let slope_ppm = (line.slope * FREQ as f64 / HZ as f64 - 1.0) * 1e6;
        assert!(slope_ppm.abs() < 5.0, "slope {slope_ppm} ppm");
        assert!(line.resid_sd < 1.0);
        assert!((line.mean_margin - PASS_FRAMES as f64).abs() < 1e-9);
        assert!((line.pass_frames - PASS_FRAMES as f64).abs() < 1e-6);
    }

    #[test]
    fn staircase_phase_error_is_far_below_one_millisecond() {
        // CrossOver: 512-frame (11.6 ms) staircase, pass instants jittered
        // ±1 ms, DAC 30 ppm fast. A staircase is biased by −step/2 on
        // average (floor), which is a CONSTANT and lands in C; only the
        // residual spread matters here.
        let mut fit = Fit::new(cfg());
        let mut e = Engine {
            step: 512,
            ppm: 30.0,
            jitter_ticks: FREQ / 1000,
            k: 0,
        };
        run(&mut e, &mut fit, 1000);
        let line = fit.line().unwrap();
        assert!(line.ready);
        assert_eq!(line.n, 1000);
        let mut worst = 0.0f64;
        let mut bias = 0.0f64;
        let mut count = 0.0;
        for k in 1000..1040 {
            // Continue the staircase and re-fit as the real thing would.
            let t = e.pass_time(k);
            let s = e.sample_at(t);
            fit.push(s, false);
            let line = fit.line().unwrap();
            let eval_t = t + PASS_TICKS / 2;
            let err = line.eval(eval_t) - e.true_p(eval_t);
            bias += err;
            count += 1.0;
            worst = worst.max((err - (-(512.0) / 2.0)).abs());
        }
        let bias = bias / count;
        // Bias ≈ −256 frames (half a step) is expected; the spread about it
        // must be far below 1 ms = 44.1 frames.
        assert!(
            (bias + 256.0).abs() < 20.0,
            "staircase bias {bias} frames (expected ≈ −256)"
        );
        assert!(worst < 15.0, "worst phase spread {worst} frames");
        // Slope precision is statistical here (σ ≈ step/√12 over a 10 s
        // window ≈ 35 ppm) and irrelevant to the clock: the line is only
        // ever evaluated within one pass of its newest sample.
        let slope_ppm = (line.slope * FREQ as f64 / HZ as f64 - 1.0) * 1e6;
        assert!(
            (slope_ppm - 30.0).abs() < 150.0,
            "slope {slope_ppm} ppm (true 30)"
        );
        // The staircase shows up in the residual SD (≈ step/√12 ≈ 148).
        assert!(
            line.resid_sd > 100.0 && line.resid_sd < 200.0,
            "{}",
            line.resid_sd
        );
    }

    #[test]
    fn window_slides_and_evicts_old_samples() {
        let mut fit = Fit::new(cfg());
        let mut e = Engine {
            step: 1,
            ppm: 0.0,
            jitter_ticks: 0,
            k: 0,
        };
        run(&mut e, &mut fit, 3000); // 30 s of passes into a 10 s window
        assert!(fit.len() <= 1001 && fit.len() >= 999, "{}", fit.len());
        let newest = fit.newest().unwrap();
        let oldest = fit.oldest().unwrap();
        assert!(newest.t - oldest.t <= cfg().window_ticks);
        let line = fit.line().unwrap();
        let t = newest.t + PASS_TICKS;
        assert!((line.eval(t) - e.true_p(t)).abs() < 1.0);
    }

    #[test]
    fn recentering_keeps_the_fit_exact_over_a_long_session() {
        // Origin far in the past (an hour of passes) with a 2-s window:
        // many re-centerings; the line must stay exact.
        let mut fit = Fit::new(FitConfig::from_seconds(FREQ, 2, 200));
        let mut e = Engine {
            step: 1,
            ppm: -20.0,
            jitter_ticks: 0,
            k: 0,
        };
        for k in 0..360_000 {
            let t = e.pass_time(k);
            let s = e.sample_at(t);
            assert_eq!(fit.push(s, false), Push::Accepted);
            if k % 50_000 == 0 && k > 0 {
                let line = fit.line().unwrap();
                let err = line.eval(t + PASS_TICKS) - e.true_p(t + PASS_TICKS);
                assert!(err.abs() < 1.0, "k={k} err={err}");
            }
        }
        assert!(fit.len() <= 201);
    }

    #[test]
    fn discontinuities_reset_the_window() {
        let mut fit = Fit::new(cfg());
        let mut e = Engine {
            step: 1,
            ppm: 0.0,
            jitter_ticks: 0,
            k: 0,
        };
        let samples = run(&mut e, &mut fit, 300);
        let last = *samples.last().unwrap();
        // Play cursor decreases (backend Stop / new backend).
        let reset = Sample {
            t: last.t + PASS_TICKS,
            p: 10,
            wc: 451,
            w: 2000,
        };
        assert_eq!(fit.push(reset, false), Push::Reset(Reset::PlayDecreased));
        assert_eq!(fit.len(), 1);
        assert!(fit.line().is_none());
        // Gap > 200 ms.
        let gap = Sample {
            t: reset.t + FREQ / 4,
            p: 20_000,
            wc: 20_441,
            w: 22_000,
        };
        assert_eq!(fit.push(gap, false), Push::Reset(Reset::Gap));
        // Underrun clamp.
        let under = Sample {
            t: gap.t + PASS_TICKS,
            p: 20_441,
            wc: 20_882,
            w: 20_882,
        };
        assert_eq!(fit.push(under, true), Push::Reset(Reset::Underrun));
        // Time going backwards.
        let back = Sample {
            t: under.t - 5,
            p: 20_882,
            wc: 21_000,
            w: 23_000,
        };
        assert_eq!(fit.push(back, false), Push::Reset(Reset::TimeNotMonotonic));
        assert_eq!(
            fit.reset(Reset::FormatChanged),
            Push::Reset(Reset::FormatChanged)
        );
        assert!(fit.is_empty());
    }

    #[test]
    fn readiness_requires_the_sample_count() {
        let mut fit = Fit::new(cfg());
        let mut e = Engine {
            step: 512,
            ppm: 0.0,
            jitter_ticks: 0,
            k: 0,
        };
        run(&mut e, &mut fit, DEFAULT_READY_SAMPLES as i64 - 1);
        assert!(!fit.line().unwrap().ready);
        let t = e.pass_time(DEFAULT_READY_SAMPLES as i64);
        let s = e.sample_at(t);
        fit.push(s, false);
        assert!(fit.line().unwrap().ready);
    }

    #[test]
    fn degenerate_time_span_has_no_line() {
        let mut fit = Fit::new(cfg());
        let s = Sample {
            t: 1000,
            p: 0,
            wc: 441,
            w: 2205,
        };
        assert_eq!(fit.push(s, false), Push::Accepted);
        assert!(fit.line().is_none());
        // Same t twice: den == 0.
        assert_eq!(fit.push(Sample { p: 441, ..s }, false), Push::Accepted);
        assert!(fit.line().is_none());
    }

    #[test]
    fn raw_line_is_the_single_sample_extrapolation() {
        let s = Sample {
            t: 5_000,
            p: 44_100,
            wc: 44_541,
            w: 46_305,
        };
        let line = Line::raw(s, 44_100, FREQ);
        assert!(line.ready);
        assert!((line.eval(5_000 + FREQ) - 88_200.0).abs() < 1e-6);
        assert_eq!(line.mean_lead, 1764.0);
        assert_eq!(line.mean_margin, 441.0);
        assert!(!Line::raw(s, 44_100, 0).ready);
    }

    #[test]
    fn ring_never_exceeds_capacity() {
        // A huge window with a fast cadence must cap at CAPACITY.
        let mut fit = Fit::new(FitConfig {
            window_ticks: i64::MAX / 4,
            max_gap_ticks: i64::MAX / 4,
            ready_samples: 1,
        });
        for k in 0..(CAPACITY as i64 + 500) {
            let s = Sample {
                t: k * 100,
                p: k * 441,
                wc: k * 441 + 441,
                w: k * 441 + 2205,
            };
            fit.push(s, false);
        }
        assert_eq!(fit.len(), CAPACITY);
        let line = fit.line().unwrap();
        assert!((line.slope - 4.41).abs() < 1e-9);
    }
}
