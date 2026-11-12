//! Pure decision + pacing logic for FPS Unlock. Dependency-free (no
//! `crate::` imports) so `scripts/validate_fps_unlock.sh` can mount it into a
//! host crate and run the tests below.
//!
//! The one policy decision lives in [`mode_for`]: a target ABOVE the stock 60
//! is realised exactly as it always was (the fullscreen refresh request), the
//! stock value needs nothing, and ONLY a target BELOW 60 takes the frame
//! limiter path. A refresh request cannot express a sub-60 target — no
//! monitor offers a 20/30 Hz mode, D3D9 fails `CreateDevice` for a
//! non-enumerated fullscreen rate, and windowed mode ignores the value — so
//! sub-60 targets keep the stock refresh and pace the game loop instead
//! (`docs/fps_frame_limiter.md`).

use std::cmp::Ordering;

/// The stock refresh target (`onBoot`'s `0x3C`; 75 only on the non-existent
/// MachineType==1 cabinet).
pub const STOCK_FPS: i32 = 60;
/// Normalization bounds. The floor keeps the limiter's frame time and the
/// raised per-frame dt clamp (`2 / target` s) in a sane range; values below it
/// were never usable (they failed the fullscreen display-mode request too).
pub const FPS_MIN: i32 = 10;
pub const FPS_MAX: i32 = 1000;
/// The game clamps each frame's delta-time to TWO stock frames
/// (`2.0 / 59.94` s, computed in `onBoot`). The limiter keeps the same
/// two-frame headroom at its own rate.
pub const DT_CLAMP_FRAMES: f32 = 2.0;

/// Fallback preset list (matches `config::default_fps_presets`), used when the
/// operator's list normalizes to empty.
pub fn default_presets() -> Vec<i32> {
    vec![60, 120, 144, 165, 240, 360]
}

/// Normalize the operator's preset list: keep only in-range entries, sort
/// ascending, dedupe; fall back to defaults if that empties the list; clamp
/// `selected` into range and ensure it's present (auto-add). Returns the
/// normalized `(values, selected)`.
pub fn normalize(presets: &[i32], selected: i32) -> (Vec<i32>, i32) {
    let mut values: Vec<i32> = presets
        .iter()
        .copied()
        .filter(|v| (FPS_MIN..=FPS_MAX).contains(v))
        .collect();
    values.sort_unstable();
    values.dedup();
    if values.is_empty() {
        values = default_presets();
    }
    let sel = selected.clamp(FPS_MIN, FPS_MAX);
    if !values.contains(&sel) {
        values.push(sel);
        values.sort_unstable();
        values.dedup();
    }
    (values, sel)
}

/// How a selected target is realised this boot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The stock 60: nothing to patch, nothing to install.
    Stock,
    /// Above stock: patch the fullscreen refresh request (the original
    /// FPS Unlock behaviour, unchanged).
    Refresh(u32),
    /// Below stock: leave the refresh at stock and run the frame limiter at
    /// this rate.
    Limit(u32),
}

pub fn mode_for(selected: i32) -> Mode {
    let v = selected.clamp(FPS_MIN, FPS_MAX);
    match v.cmp(&STOCK_FPS) {
        Ordering::Equal => Mode::Stock,
        Ordering::Greater => Mode::Refresh(v as u32),
        Ordering::Less => Mode::Limit(v as u32),
    }
}

/// The fullscreen refresh rate the D3D device will be asked for — what a
/// display-mode check must match (custom_resolution's fail-safe).
pub fn requested_refresh_hz(selected: i32) -> u32 {
    match mode_for(selected) {
        Mode::Refresh(hz) => hz,
        Mode::Stock | Mode::Limit(_) => STOCK_FPS as u32,
    }
}

/// One limiter frame, in nanoseconds.
pub fn frame_period_ns(fps: u32) -> u64 {
    1_000_000_000 / u64::from(fps.max(1))
}

/// The per-frame dt clamp the limiter needs: two frames at its own rate, or
/// the stock clamp when that is already larger. A non-finite / non-positive
/// stock value (unreadable or corrupted) is ignored.
pub fn limiter_dt_clamp(fps: u32, stock: f32) -> f32 {
    let want = DT_CLAMP_FRAMES / fps.max(1) as f32;
    if stock.is_finite() && stock > want {
        stock
    } else {
        want
    }
}

/// Deadline-grid frame pacer. Each call marks one frame boundary; the grid
/// advances by exactly one period per frame so the AVERAGE rate is the
/// target even when individual waits jitter. A frame that ends up to one
/// period late keeps the grid (the next wait is shorter); a longer stall
/// (loading hitch, breakpoint) re-anchors the grid instead of bursting
/// frames to catch up.
#[derive(Clone, Debug)]
pub struct Pacer {
    period_ns: u64,
    next_ns: Option<u64>,
}

impl Pacer {
    pub fn new(period_ns: u64) -> Self {
        Self {
            period_ns: period_ns.max(1),
            next_ns: None,
        }
    }

    pub fn period_ns(&self) -> u64 {
        self.period_ns
    }

    /// Forget the grid; the next call starts a new one without waiting.
    pub fn reset(&mut self) {
        self.next_ns = None;
    }

    /// Record a frame boundary at `now_ns` (any monotonic clock) and return
    /// how long to wait before the frame may start. Never exceeds one period.
    pub fn wait_ns(&mut self, now_ns: u64) -> u64 {
        let period = self.period_ns;
        let Some(next) = self.next_ns else {
            self.next_ns = Some(now_ns.saturating_add(period));
            return 0;
        };
        if now_ns >= next {
            self.next_ns = Some(if now_ns - next >= period {
                now_ns.saturating_add(period)
            } else {
                next.saturating_add(period)
            });
            return 0;
        }
        self.next_ns = Some(next.saturating_add(period));
        next - now_ns
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    #[test]
    fn only_sub_stock_targets_take_the_limiter_path() {
        assert_eq!(mode_for(20), Mode::Limit(20));
        assert_eq!(mode_for(30), Mode::Limit(30));
        assert_eq!(mode_for(59), Mode::Limit(59));
        assert_eq!(mode_for(60), Mode::Stock);
        for hz in [61, 120, 144, 165, 240, 360, 1000] {
            assert_eq!(mode_for(hz), Mode::Refresh(hz as u32), "{hz}");
        }
    }

    #[test]
    fn mode_clamps_like_normalization() {
        assert_eq!(mode_for(1), Mode::Limit(FPS_MIN as u32));
        assert_eq!(mode_for(-5), Mode::Limit(FPS_MIN as u32));
        assert_eq!(mode_for(5000), Mode::Refresh(FPS_MAX as u32));
    }

    #[test]
    fn refresh_request_is_unchanged_at_and_above_stock() {
        for hz in [60, 120, 144, 165, 240, 360] {
            assert_eq!(requested_refresh_hz(hz), hz as u32);
        }
    }

    #[test]
    fn sub_stock_targets_request_the_stock_refresh() {
        assert_eq!(requested_refresh_hz(20), 60);
        assert_eq!(requested_refresh_hz(30), 60);
    }

    #[test]
    fn normalize_keeps_existing_presets_verbatim() {
        let (values, sel) = normalize(&default_presets(), 144);
        assert_eq!(values, default_presets());
        assert_eq!(sel, 144);
    }

    #[test]
    fn normalize_accepts_sub_stock_presets() {
        let (values, sel) = normalize(&[60, 30, 20, 120, 30], 30);
        assert_eq!(values, vec![20, 30, 60, 120]);
        assert_eq!(sel, 30);
    }

    #[test]
    fn normalize_drops_out_of_range_and_auto_adds_selected() {
        let (values, sel) = normalize(&[0, 5, 60, 2000], 45);
        assert_eq!(values, vec![45, 60]);
        assert_eq!(sel, 45);
        let (values, sel) = normalize(&[], 1);
        assert_eq!(sel, FPS_MIN);
        assert!(values.contains(&FPS_MIN));
        assert!(values.contains(&60));
    }

    #[test]
    fn frame_periods() {
        assert_eq!(frame_period_ns(30), 33_333_333);
        assert_eq!(frame_period_ns(20), 50_000_000);
        assert_eq!(frame_period_ns(0), 1_000_000_000);
    }

    #[test]
    fn dt_clamp_admits_two_limiter_frames() {
        let stock = 2.0 / 59.94_f32;
        let c30 = limiter_dt_clamp(30, stock);
        let c20 = limiter_dt_clamp(20, stock);
        assert!((c30 - 2.0 / 30.0).abs() < 1e-6);
        assert!((c20 - 0.1).abs() < 1e-6);
        // A limiter frame never hits the clamp.
        assert!(c20 > 0.050 && c30 > 0.0334);
        // A stock clamp that is already larger is kept.
        assert_eq!(limiter_dt_clamp(50, 0.5), 0.5);
        // Garbage stock values are ignored.
        assert!((limiter_dt_clamp(30, f32::NAN) - 2.0 / 30.0).abs() < 1e-6);
        assert!((limiter_dt_clamp(30, -1.0) - 2.0 / 30.0).abs() < 1e-6);
    }

    #[test]
    fn pacer_first_frame_never_waits() {
        let mut p = Pacer::new(33 * MS);
        assert_eq!(p.wait_ns(1_000 * MS), 0);
    }

    #[test]
    fn pacer_waits_out_the_rest_of_the_period() {
        let mut p = Pacer::new(33 * MS);
        p.wait_ns(0);
        // Frame took 10 ms -> wait 23 ms.
        assert_eq!(p.wait_ns(10 * MS), 23 * MS);
        // Woke on time at 33, next frame took 5 ms -> wait until 66.
        assert_eq!(p.wait_ns(38 * MS), 28 * MS);
    }

    #[test]
    fn pacer_average_rate_is_exact_despite_jitter() {
        let period = frame_period_ns(30);
        let mut p = Pacer::new(period);
        let mut now = 0u64;
        p.wait_ns(now);
        let frames = 300u64;
        for i in 0..frames {
            // Alternating 4 / 12 ms of work, then oversleep by 0..0.9 ms.
            now += if i % 2 == 0 { 4 * MS } else { 12 * MS };
            now += p.wait_ns(now) + (i % 10) * MS / 10;
        }
        let expected = frames * period;
        assert!(now.abs_diff(expected) < MS, "drifted: {now} vs {expected}");
    }

    #[test]
    fn pacer_keeps_the_grid_when_slightly_late() {
        let mut p = Pacer::new(33 * MS);
        p.wait_ns(0);
        // Frame overran by 5 ms (ended at 38 instead of 33): no wait, and the
        // next boundary stays at 66 on the original grid.
        assert_eq!(p.wait_ns(38 * MS), 0);
        assert_eq!(p.wait_ns(40 * MS), 26 * MS);
    }

    #[test]
    fn pacer_reanchors_after_a_long_stall() {
        let mut p = Pacer::new(33 * MS);
        p.wait_ns(0);
        // A 500 ms loading hitch: no burst of zero-wait frames afterwards.
        assert_eq!(p.wait_ns(500 * MS), 0);
        assert_eq!(p.wait_ns(510 * MS), 23 * MS);
    }

    #[test]
    fn pacer_wait_is_bounded_by_the_period() {
        let mut p = Pacer::new(50 * MS);
        p.wait_ns(0);
        for t in [0, 1, 49] {
            let mut q = p.clone();
            assert!(q.wait_ns(t * MS) <= 50 * MS);
        }
    }

    #[test]
    fn pacer_reset_starts_a_fresh_grid() {
        let mut p = Pacer::new(33 * MS);
        p.wait_ns(0);
        p.reset();
        assert_eq!(p.wait_ns(5 * MS), 0);
        assert_eq!(p.wait_ns(6 * MS), 32 * MS);
    }
}
