//! The song clock latch (design §3.3 / FR-8, FR-9) — pure, std-only.
//!
//! Three rules, evaluated every frame from live game state:
//! 1. graph disabled (a fresh `DancePlaySequence` before step 5) ⇒ hidden,
//!    no time, the latch is dropped (the next anchor re-latches);
//! 2. graph enabled, run NOT anchored ⇒ visible: before the first anchor
//!    the scene shows its PRE-SONG pose (`None` — the caller uses a fixed
//!    pre-start music count, see `lifecycle::PRE_SONG_MC_MS`); once a run
//!    WAS anchored, hold the last music count (the song-end tail: A3's
//!    dancers keep going until the sequence is finalised);
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    /// The first anchored count of the current run (diagnostics).
    first_count: Option<i32>,
    last_count: Option<i32>,
    anchored_prev: bool,
}

impl Clock {
    pub const fn new() -> Clock {
        Clock {
            first_count: None,
            last_count: None,
            anchored_prev: false,
        }
    }

    pub fn first_count(&self) -> Option<i32> {
        self.first_count
    }

    /// One frame. `count` is only meaningful when `anchored`.
    /// Returns `(music count ms — None = pre-song pose, visible, event)`.
    pub fn step(
        &mut self,
        graph_enabled: bool,
        anchored: bool,
        count: Option<i32>,
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
                } else if let Some(prev) = self.last_count {
                    if c < prev - REWIND_EPS_MS {
                        event = ClockEvent::Rewound { from: prev, to: c };
                    }
                }
                self.anchored_prev = true;
                self.last_count = Some(c);
                (Some(c), true, event)
            }
            _ => {
                self.anchored_prev = false;
                // Hold the last count after a run was anchored (song-end
                // tail); pre-song otherwise.
                (self.last_count, true, ClockEvent::None)
            }
        }
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
        assert_eq!(c.step(false, false, None), (None, false, ClockEvent::None));
        assert_eq!(
            c.step(false, true, Some(500)),
            (None, false, ClockEvent::None)
        );
        // graph on, not anchored: visible, pre-song
        assert_eq!(c.step(true, false, None), (None, true, ClockEvent::None));
        assert_eq!(c.first_count(), None);
    }

    #[test]
    fn latch_on_anchor_edge_then_the_count_flows_through() {
        let mut c = Clock::new();
        c.step(true, false, None);
        let (mc, vis, ev) = c.step(true, true, Some(-276));
        assert_eq!((mc, vis), (Some(-276), true));
        assert_eq!(ev, ClockEvent::Latched { mc: -276 });
        let (mc, _, ev) = c.step(true, true, Some(300));
        assert_eq!(mc, Some(300));
        assert_eq!(ev, ClockEvent::None);
        // small jitter backwards is NOT a rewind
        let (mc, _, ev) = c.step(true, true, Some(260));
        assert_eq!(mc, Some(260));
        assert_eq!(ev, ClockEvent::None);
    }

    #[test]
    fn rewind_is_reported_and_the_count_simply_follows() {
        let mut c = Clock::new();
        c.step(true, true, Some(-276));
        c.step(true, true, Some(30_000));
        // training rewind to 4 s
        let (mc, _, ev) = c.step(true, true, Some(4_000));
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
        let (mc, _, ev) = c.step(true, true, Some(-276));
        assert_eq!(mc, Some(-276));
        assert!(matches!(ev, ClockEvent::Rewound { .. }));
        assert_eq!(c.first_count(), Some(-276));
    }

    #[test]
    fn tail_holds_the_last_count_and_a_new_anchor_is_an_edge() {
        let mut c = Clock::new();
        c.step(true, true, Some(0));
        c.step(true, true, Some(2_500));
        // anchor lost with the graph still on (song-end tail): hold
        let (mc, vis, ev) = c.step(true, false, None);
        assert_eq!((mc, vis), (Some(2_500), true));
        assert_eq!(ev, ClockEvent::None);
        // a new anchor after the loss is an edge again
        let (mc, _, ev) = c.step(true, true, Some(-800));
        assert_eq!(mc, Some(-800));
        assert_eq!(ev, ClockEvent::Latched { mc: -800 });
        // fresh DPS (graph off) drops everything
        assert_eq!(c.step(false, false, None), (None, false, ClockEvent::None));
        assert_eq!(c, Clock::new());
    }
}
