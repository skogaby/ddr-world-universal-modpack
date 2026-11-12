//! The song clock latch (design §3.3 / FR-8, FR-9) — pure, std-only.
//!
//! Three rules, evaluated every frame from live game state:
//! 1. graph disabled (a fresh `DancePlaySequence` before step 5) ⇒ hidden,
//!    no time, the latch is dropped (the next anchor re-latches);
//! 2. graph enabled, run NOT anchored ⇒ visible: before the first anchor
//!    the scene shows its PRE-SONG pose (`None` — the caller uses a fixed
//!    pre-start music count, see `lifecycle::PRE_SONG_MC_MS`); once a run
//!    WAS anchored, the count is EXTRAPOLATED on wall time from the last
//!    anchored frame at the rate the count was observed to advance (the
//!    song-end tail, DPS steps 8/9: A3's dancers keep going until the
//!    sequence is finalised, and the music is still playing). Holding the
//!    last count instead froze every dancer mid-pose for the whole tail —
//!    the "dancers stop just before the end of the song" report;
//! 3. anchored ⇒ the live content-domain music count `mc` (ms; negative
//!    before the music starts). The scene time is a PURE FUNCTION of `mc`
//!    (`tempo::TempoMap::tau`, or `mc/1000` without a tempo map), so a
//!    training rewind / loop moves the whole timeline back and an in-place
//!    restart, which resets the count to its song-start value, lands at the
//!    start by itself — nothing is ever re-latched. (Deploy 2026-09-16:
//!    re-latching an origin on every backwards jump made a training rewind
//!    restart the dance from clip 0.) A jump back larger than
//!    [`REWIND_EPS_MS`] is only REPORTED (`ClockEvent::Rewound`).

/// A music-count drop larger than this is reported as a rewind.
pub const REWIND_EPS_MS: i32 = 50;

/// The tail extrapolation measures the count's advance rate over at least
/// this much anchored wall time; a shorter run extrapolates at 1:1.
pub const RATE_MIN_SPAN_MS: f64 = 1_000.0;
/// Bounds on the measured count/wall rate (song-rate mods stay well
/// inside; a pathological measurement falls back to 1:1).
pub const RATE_MIN: f64 = 0.25;
pub const RATE_MAX: f64 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockEvent {
    None,
    /// The run just anchored (first anchored frame of this run); `mc` = the
    /// first count seen (≈ −276 ms: the anchor is future-dated by the lead).
    Latched {
        mc: i32,
    },
    /// The count jumped back by more than [`REWIND_EPS_MS`].
    Rewound {
        from: i32,
        to: i32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clock {
    /// The first anchored count of the current run (diagnostics).
    first_count: Option<i32>,
    /// The last ANCHORED count and the wall time (ms) it was read at.
    last_count: Option<i32>,
    last_wall_ms: f64,
    /// Start of the rate-measurement baseline `(count, wall ms)`: set on
    /// the latch and on every rewind, so the slope only spans continuous
    /// playback.
    rate_base: Option<(i32, f64)>,
    anchored_prev: bool,
}

impl Clock {
    pub const fn new() -> Clock {
        Clock {
            first_count: None,
            last_count: None,
            last_wall_ms: 0.0,
            rate_base: None,
            anchored_prev: false,
        }
    }

    pub fn first_count(&self) -> Option<i32> {
        self.first_count
    }

    /// Music-count ms per wall ms over the current baseline (1.0 until
    /// [`RATE_MIN_SPAN_MS`] of continuous anchored playback was seen, or
    /// when the measurement is out of [`RATE_MIN`]..=[`RATE_MAX`]).
    pub fn rate(&self) -> f64 {
        let (Some((c0, w0)), Some(c1)) = (self.rate_base, self.last_count) else {
            return 1.0;
        };
        let span = self.last_wall_ms - w0;
        if !(span >= RATE_MIN_SPAN_MS) {
            return 1.0;
        }
        let r = (c1 as f64 - c0 as f64) / span;
        if (RATE_MIN..=RATE_MAX).contains(&r) {
            r
        } else {
            1.0
        }
    }

    /// One frame. `count` is only meaningful when `anchored`; `wall_ms` is
    /// any monotonic millisecond clock (only differences are used).
    /// Returns `(music count ms — None = pre-song pose, visible, event)`.
    pub fn step(
        &mut self,
        graph_enabled: bool,
        anchored: bool,
        count: Option<i32>,
        wall_ms: f64,
    ) -> (Option<i32>, bool, ClockEvent) {
        if !graph_enabled {
            *self = Clock::new();
            return (None, false, ClockEvent::None);
        }
        match (anchored, count) {
            (true, Some(c)) => {
                let mut event = ClockEvent::None;
                if !self.anchored_prev || self.first_count.is_none() {
                    event = ClockEvent::Latched { mc: c };
                    self.first_count = Some(c);
                    self.rate_base = Some((c, wall_ms));
                } else if let Some(prev) = self.last_count {
                    if c < prev - REWIND_EPS_MS {
                        event = ClockEvent::Rewound { from: prev, to: c };
                        self.rate_base = Some((c, wall_ms));
                    }
                }
                self.anchored_prev = true;
                self.last_count = Some(c);
                self.last_wall_ms = wall_ms;
                (Some(c), true, event)
            }
            _ => {
                self.anchored_prev = false;
                // Song-end tail: keep the count running from the last
                // anchored frame; pre-song (never anchored) otherwise.
                (self.extrapolated(wall_ms), true, ClockEvent::None)
            }
        }
    }

    /// The last anchored count advanced by the wall time since, at
    /// [`rate`](Self::rate); `None` when the run never anchored.
    pub fn extrapolated(&self, wall_ms: f64) -> Option<i32> {
        let last = self.last_count?;
        let dt = wall_ms - self.last_wall_ms;
        let dt = if dt.is_finite() && dt > 0.0 { dt } else { 0.0 };
        let mc = last as f64 + dt * self.rate();
        Some(mc.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32)
    }
}

impl Default for Clock {
    fn default() -> Self {
        Clock::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_until_the_graph_enables_then_pre_song_pose() {
        let mut c = Clock::new();
        assert_eq!(
            c.step(false, false, None, 0.0),
            (None, false, ClockEvent::None)
        );
        assert_eq!(
            c.step(false, true, Some(500), 0.0),
            (None, false, ClockEvent::None)
        );
        // graph on, not anchored: visible, pre-song
        assert_eq!(
            c.step(true, false, None, 0.0),
            (None, true, ClockEvent::None)
        );
        assert_eq!(c.first_count(), None);
    }

    #[test]
    fn latch_on_anchor_edge_then_the_count_flows_through() {
        let mut c = Clock::new();
        c.step(true, false, None, 0.0);
        let (mc, vis, ev) = c.step(true, true, Some(-276), 0.0);
        assert_eq!((mc, vis), (Some(-276), true));
        assert_eq!(ev, ClockEvent::Latched { mc: -276 });
        let (mc, _, ev) = c.step(true, true, Some(300), 0.0);
        assert_eq!(mc, Some(300));
        assert_eq!(ev, ClockEvent::None);
        // small jitter backwards is NOT a rewind
        let (mc, _, ev) = c.step(true, true, Some(260), 0.0);
        assert_eq!(mc, Some(260));
        assert_eq!(ev, ClockEvent::None);
    }

    #[test]
    fn rewind_is_reported_and_the_count_simply_follows() {
        let mut c = Clock::new();
        c.step(true, true, Some(-276), 0.0);
        c.step(true, true, Some(30_000), 0.0);
        // training rewind to 4 s
        let (mc, _, ev) = c.step(true, true, Some(4_000), 0.0);
        assert_eq!(mc, Some(4_000));
        assert_eq!(
            ev,
            ClockEvent::Rewound {
                from: 30_000,
                to: 4_000
            }
        );
        assert_eq!(c.first_count(), Some(-276));
        // in-place restart: back to the song-start count, no re-latch
        let (mc, _, ev) = c.step(true, true, Some(-276), 0.0);
        assert_eq!(mc, Some(-276));
        assert!(matches!(ev, ClockEvent::Rewound { .. }));
        assert_eq!(c.first_count(), Some(-276));
    }

    #[test]
    fn tail_keeps_the_count_running_and_a_new_anchor_is_an_edge() {
        let mut c = Clock::new();
        c.step(true, true, Some(0), 10_000.0);
        let (mc, _, _) = c.step(true, true, Some(2_500), 12_500.0);
        assert_eq!(mc, Some(2_500));
        // anchor lost with the graph still on (song-end tail): the count
        // keeps advancing on wall time instead of freezing the dancers
        let (mc, vis, ev) = c.step(true, false, None, 12_500.0);
        assert_eq!((mc, vis), (Some(2_500), true));
        assert_eq!(ev, ClockEvent::None);
        let (mc, _, _) = c.step(true, false, None, 14_000.0);
        assert_eq!(mc, Some(4_000));
        let (mc, _, _) = c.step(true, false, None, 20_000.0);
        assert_eq!(mc, Some(10_000));
        // a new anchor after the loss is an edge again
        let (mc, _, ev) = c.step(true, true, Some(-800), 21_000.0);
        assert_eq!(mc, Some(-800));
        assert_eq!(ev, ClockEvent::Latched { mc: -800 });
        // fresh DPS (graph off) drops everything
        assert_eq!(
            c.step(false, false, None, 22_000.0),
            (None, false, ClockEvent::None)
        );
        assert_eq!(c, Clock::new());
    }

    #[test]
    fn tail_follows_the_measured_song_rate() {
        // 1.5x song rate: the content count advances 1.5 ms per wall ms
        let mut c = Clock::new();
        let mut wall = 0.0;
        let mut count = -276.0f64;
        for _ in 0..600 {
            c.step(true, true, Some(count.round() as i32), wall);
            wall += 1000.0 / 60.0;
            count += 1.5 * 1000.0 / 60.0;
        }
        assert!((c.rate() - 1.5).abs() < 0.01, "rate {}", c.rate());
        let last = c.extrapolated(wall - 1000.0 / 60.0).unwrap();
        let (mc, _, _) = c.step(true, false, None, wall - 1000.0 / 60.0 + 2_000.0);
        let mc = mc.unwrap();
        assert!((mc - last - 3_000).abs() <= 20, "{last} -> {mc}");
    }

    #[test]
    fn rate_falls_back_to_real_time_on_short_or_insane_baselines() {
        // under RATE_MIN_SPAN_MS of anchored playback: 1:1
        let mut c = Clock::new();
        c.step(true, true, Some(0), 0.0);
        c.step(true, true, Some(900), 500.0);
        assert_eq!(c.rate(), 1.0);
        let (mc, _, _) = c.step(true, false, None, 1_500.0);
        assert_eq!(mc, Some(1_900));
        // a rewind restarts the baseline (a training jump is not a rate)
        let mut c = Clock::new();
        c.step(true, true, Some(0), 0.0);
        c.step(true, true, Some(30_000), 30_000.0);
        c.step(true, true, Some(4_000), 30_016.0);
        assert_eq!(c.rate(), 1.0);
        c.step(true, true, Some(6_000), 32_016.0);
        assert!((c.rate() - 1.0).abs() < 1e-9);
        // an absurd slope (stale tick on the latch frame) is ignored
        let mut c = Clock::new();
        c.step(true, true, Some(0), 0.0);
        c.step(true, true, Some(1_000_000), 2_000.0);
        assert_eq!(c.rate(), 1.0);
        // a backwards / non-finite wall clock never moves the count back
        let mut c = Clock::new();
        c.step(true, true, Some(5_000), 100.0);
        assert_eq!(c.step(true, false, None, 50.0).0, Some(5_000));
        assert_eq!(c.step(true, false, None, f64::NAN).0, Some(5_000));
    }
}
