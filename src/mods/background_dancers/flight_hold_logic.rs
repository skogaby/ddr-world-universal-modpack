//! The flight stages' READY hold — pure, std-only (harness-mounted).
//!
//! In the zan games the take-off is a cut-in BEFORE gameplay: the play
//! sequence waits for the stage's intro camera (FuruFuru Party
//! `FUN_800302e8` state 4: `FUN_8003dfd0` + 1 s; MUSIC FIT's intro list is
//! the 10 s take-off, `FUN_80048290`) and READY comes after it
//! (`docs/wii_ddr_hottest_party_2_3_research.md` §7.5 "READY delay").
//! World's DancePlaySequence waits in step 5 until its elapsed-time counter
//! (`DPS+0x130`) reaches the 5.0 s READY? dwell, with the stage panel
//! (ShutterActor kind 3) covering the screen; the song starts at step 6/7
//! and the panel then reveals and shows its own READY? / HERE WE GO!.
//!
//! The hold (driven every frame by `flight_hold.rs`, before the DPS update):
//! the stock panel shows for [`HOLD_AT_S`] of the dwell, then — once the
//! flight scene is built and the panel has settled (ShutterActor state 4,
//! covered: its cut-in / `in` animation done) — the panel is hidden, the
//! SceneGraph is enabled (World only enables it once the dwell gate passes,
//! so nothing 3D draws during step 5 otherwise) and the timer is kept at
//! [`HOLD_AT_S`] while the take-off plays on its own wall clock (the music
//! count is not running yet). DDR SELECTION's legacy panel, which skips the
//! dwell, defers to an announced hold (`services::ready_hold`). [`RELEASE_LEAD_S`] before the
//! take-off's end the panel comes back and the dwell is released (the timer
//! seeded past any threshold, the quick-restart technique), so the song —
//! and the flight, which switches at the music's start — begins where the
//! take-off ends, and World's READY? follows it.
//!
//! Fail-open everywhere: another driver seeding the dwell (quick restart,
//! DDR SELECTION's legacy intro), a scene not ready in time, a DPS that moved
//! on, or a hold running past its budget stand the hold down and the song
//! starts as stock (the take-off then plays over the song's first seconds,
//! as before the hold).

/// The stock panel shows this long (DPS dwell timer, s) before the take-off.
pub const HOLD_AT_S: f32 = 4.0;
/// The dwell is released this long before the take-off's end: DPS step 5 →
/// 6 → 7 plus the music count's lead (its first anchored value is about
/// −276 ms), so music 0 — the switch — lands on the take-off's end.
pub const RELEASE_LEAD_S: f64 = 0.30;
/// How long the hold waits (wall s, timer held) for the flight scene to be
/// built and the panel to settle before giving up.
pub const WAIT_SCENE_MAX_S: f64 = 8.0;
/// A take-off that has not released after its length + this (wall s) is
/// released anyway.
pub const PLAY_SAFETY_S: f64 = 5.0;
/// A dwell timer at or above this was seeded by another driver.
pub const FOREIGN_SEED_MIN: f32 = 100.0;
/// The release seed (far above any plausible dwell threshold, like quick
/// restart's).
pub const RELEASE_VALUE: f32 = 1000.0;
/// DancePlaySequence's READY? dwell step.
pub const STEP_READY: i32 = 5;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    /// Not holding this DPS (no flight take-off, stood down, or unavailable).
    Off,
    /// Waiting for the dwell to reach [`HOLD_AT_S`] / the scene to be ready.
    Armed { waited_s: f64 },
    /// The take-off plays; the panel is hidden, the dwell held.
    Playing { played_s: f64 },
    /// Released: the song follows (the timer is seeded while still in step 5).
    Released,
}

/// What the panel should do this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelOp {
    Keep,
    Hide,
    Show,
}

/// One frame's observations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Inputs {
    /// The live DPS step (`None` = no DPS).
    pub step: Option<i32>,
    /// The DPS dwell timer (`None` = unreadable).
    pub timer: Option<f32>,
    /// The flight scene is built and has a take-off.
    pub scene_ready: bool,
    /// The stage panel has settled (ShutterActor state 4 = covered, or no
    /// panel at all): the take-off may replace it.
    pub panel_settled: bool,
    /// The 3D scene is masked off (Background Movies = MOVIE ONLY over a
    /// drawn movie): a held take-off would be 10 s of nothing.
    pub scene_hidden: bool,
    /// The take-off clock (s since the take-off began; `Playing` only).
    pub intro_s: f64,
    /// The take-off's length (the flight switch, s).
    pub takeoff_s: f64,
    /// Wall seconds since the previous frame.
    pub dt: f64,
}

/// One frame's decisions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Actions {
    /// Write the DPS dwell timer.
    pub write_timer: Option<f32>,
    pub panel: PanelOp,
    /// The take-off begins this frame (set its clock to 0).
    pub started: bool,
    /// Set the SceneGraph enable bit (the take-off draws during step 5).
    pub enable_graph: bool,
    /// Released this frame (why).
    pub released: Option<&'static str>,
    /// Stood down this frame without holding (why).
    pub stood_down: Option<&'static str>,
}

impl Actions {
    const NONE: Actions = Actions {
        write_timer: None,
        panel: PanelOp::Keep,
        started: false,
        enable_graph: false,
        released: None,
        stood_down: None,
    };
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hold {
    phase: Phase,
}

impl Hold {
    /// `armed` = this DPS is a flight take-off and the hold's sites exist.
    pub fn new(armed: bool) -> Hold {
        Hold {
            phase: if armed {
                Phase::Armed { waited_s: 0.0 }
            } else {
                Phase::Off
            },
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The take-off moved before the song: the song's music 0 is the
    /// switch (the scene clock is the music count + the take-off length).
    pub fn offsets_song(&self) -> bool {
        matches!(self.phase, Phase::Playing { .. } | Phase::Released)
    }

    /// The take-off clock free-runs (no music count to follow yet).
    pub fn free_runs(&self, anchored: bool) -> bool {
        match self.phase {
            Phase::Playing { .. } => true,
            Phase::Released => !anchored,
            _ => false,
        }
    }

    pub fn step(&mut self, i: Inputs) -> Actions {
        let mut a = Actions::NONE;
        let dt = if i.dt.is_finite() { i.dt.max(0.0) } else { 0.0 };
        match self.phase {
            Phase::Off => {}
            Phase::Armed { waited_s } => {
                match i.step {
                    None => {}
                    Some(s) if s < STEP_READY => {}
                    Some(s) if s > STEP_READY => {
                        self.phase = Phase::Off;
                        a.stood_down = Some("the song started before the take-off could hold it");
                    }
                    Some(_) => match i.timer {
                        None => {
                            self.phase = Phase::Off;
                            a.stood_down = Some("the READY? dwell timer is unreadable");
                        }
                        Some(t) if t >= FOREIGN_SEED_MIN => {
                            self.phase = Phase::Off;
                            a.stood_down = Some("another driver seeds the READY? dwell (quick restart / DDR SELECTION)");
                        }
                        Some(t) if t < HOLD_AT_S => {}
                        Some(_) if i.scene_hidden => {
                            self.phase = Phase::Off;
                            a.stood_down = Some("the 3D scene is hidden (Background Movies)");
                        }
                        Some(_) if i.scene_ready && i.panel_settled => {
                            self.phase = Phase::Playing { played_s: 0.0 };
                            a.write_timer = Some(HOLD_AT_S);
                            a.panel = PanelOp::Hide;
                            a.started = true;
                            a.enable_graph = true;
                        }
                        Some(_) => {
                            let waited_s = waited_s + dt;
                            if waited_s > WAIT_SCENE_MAX_S {
                                self.phase = Phase::Off;
                                a.stood_down = Some(if i.scene_ready {
                                    "the stage panel did not settle in time"
                                } else {
                                    "the flight scene was not ready in time"
                                });
                            } else {
                                self.phase = Phase::Armed { waited_s };
                                a.write_timer = Some(HOLD_AT_S);
                            }
                        }
                    },
                }
            }
            Phase::Playing { played_s } => {
                let played_s = played_s + dt;
                let why = if i.step != Some(STEP_READY) {
                    Some("the DancePlaySequence left the READY? step")
                } else if i.timer.is_some_and(|t| t >= FOREIGN_SEED_MIN) {
                    Some("another driver seeded the READY? dwell")
                } else if i.intro_s >= i.takeoff_s - RELEASE_LEAD_S {
                    Some("take-off end")
                } else if played_s > i.takeoff_s + PLAY_SAFETY_S {
                    Some("safety budget")
                } else {
                    None
                };
                match why {
                    Some(w) => {
                        self.phase = Phase::Released;
                        a.panel = PanelOp::Show;
                        a.released = Some(w);
                        if i.step == Some(STEP_READY) {
                            a.write_timer = Some(RELEASE_VALUE);
                        }
                    }
                    None => {
                        self.phase = Phase::Playing { played_s };
                        a.write_timer = Some(HOLD_AT_S);
                        a.panel = PanelOp::Hide;
                        a.enable_graph = true;
                    }
                }
            }
            Phase::Released => {
                if i.step == Some(STEP_READY) {
                    a.write_timer = Some(RELEASE_VALUE);
                }
            }
        }
        a
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inp(step: i32, timer: f32, ready: bool, intro_s: f64) -> Inputs {
        Inputs {
            step: Some(step),
            timer: Some(timer),
            scene_ready: ready,
            panel_settled: true,
            scene_hidden: false,
            intro_s,
            takeoff_s: 10.0,
            dt: 1.0 / 120.0,
        }
    }

    #[test]
    fn holds_from_the_panel_through_the_take_off_then_releases() {
        let mut h = Hold::new(true);
        assert_eq!(h.step(inp(3, 1.0, false, 0.0)), Actions::NONE);
        assert_eq!(h.step(inp(5, 2.0, true, 0.0)), Actions::NONE);
        // dwell at HOLD_AT, scene ready: the take-off begins
        let a = h.step(inp(5, 4.01, true, 0.0));
        assert!(a.started);
        assert_eq!(a.panel, PanelOp::Hide);
        assert_eq!(a.write_timer, Some(HOLD_AT_S));
        assert!(h.offsets_song() && h.free_runs(false));
        // mid take-off: held, panel kept hidden
        let a = h.step(inp(5, 4.01, true, 5.0));
        assert_eq!((a.write_timer, a.panel), (Some(HOLD_AT_S), PanelOp::Hide));
        // the end: released ahead of the switch
        let a = h.step(inp(5, 4.01, true, 10.0 - RELEASE_LEAD_S));
        assert_eq!(a.released, Some("take-off end"));
        assert_eq!(
            (a.write_timer, a.panel),
            (Some(RELEASE_VALUE), PanelOp::Show)
        );
        // still step 5 next frame: keep the release seed
        assert_eq!(
            h.step(inp(5, 1000.0, true, 9.8)).write_timer,
            Some(RELEASE_VALUE)
        );
        // the song: nothing more, the offset stays
        assert_eq!(h.step(inp(7, 1000.0, true, 9.9)), Actions::NONE);
        assert!(h.offsets_song());
        assert!(h.free_runs(false) && !h.free_runs(true));
    }

    #[test]
    fn waits_for_the_panel_to_settle_and_enables_the_graph_while_playing() {
        let mut h = Hold::new(true);
        let a = h.step(Inputs {
            panel_settled: false,
            ..inp(5, 4.2, true, 0.0)
        });
        assert!(!a.started && !a.enable_graph);
        assert_eq!(a.write_timer, Some(HOLD_AT_S));
        let a = h.step(inp(5, 4.0, true, 0.0));
        assert!(a.started && a.enable_graph);
        assert!(h.step(inp(5, 4.0, true, 1.0)).enable_graph);
        // released: the game enables it at its own gate
        let a = h.step(inp(5, 4.0, true, 9.8));
        assert!(a.released.is_some() && !a.enable_graph);
    }

    #[test]
    fn waits_for_the_scene_with_the_timer_held_then_gives_up() {
        let mut h = Hold::new(true);
        let a = h.step(inp(5, 4.2, false, 0.0));
        assert_eq!(a.write_timer, Some(HOLD_AT_S));
        assert!(!a.started);
        let mut out = None;
        for _ in 0..1000 {
            let a = h.step(inp(5, 4.0, false, 0.0));
            if a.stood_down.is_some() {
                out = Some(a);
                break;
            }
        }
        let a = out.expect("gives up");
        assert_eq!(a.write_timer, None);
        assert_eq!(h.phase(), Phase::Off);
        assert!(!h.offsets_song());
    }

    #[test]
    fn stands_down_for_a_foreign_seed_or_a_late_start() {
        let mut h = Hold::new(true);
        let a = h.step(inp(5, 1000.0, true, 0.0));
        assert!(a.stood_down.is_some() && a.write_timer.is_none());
        let mut h = Hold::new(true);
        assert!(h.step(inp(7, 0.0, true, 0.0)).stood_down.is_some());
        let mut h = Hold::new(false);
        assert_eq!(h.step(inp(5, 4.5, true, 0.0)), Actions::NONE);
        // a scene masked off by the movie: no hold at all
        let mut h = Hold::new(true);
        let a = h.step(Inputs {
            scene_hidden: true,
            ..inp(5, 4.5, true, 0.0)
        });
        assert!(a.stood_down.is_some() && a.write_timer.is_none() && !a.started);
    }

    #[test]
    fn a_dps_that_moves_on_mid_take_off_releases_without_a_seed() {
        let mut h = Hold::new(true);
        assert!(h.step(inp(5, 4.1, true, 0.0)).started);
        let a = h.step(inp(6, 4.1, true, 2.0));
        assert_eq!(a.panel, PanelOp::Show);
        assert_eq!(a.write_timer, None);
        assert!(a.released.is_some());
        // a foreign seed mid take-off releases too
        let mut h = Hold::new(true);
        h.step(inp(5, 4.1, true, 0.0));
        assert!(h.step(inp(5, 1000.0, true, 1.0)).released.is_some());
    }

    #[test]
    fn the_safety_budget_releases_a_stuck_take_off() {
        let mut h = Hold::new(true);
        h.step(inp(5, 4.1, true, 0.0));
        let mut released = None;
        for _ in 0..(20 * 120) {
            // the intro clock never advances (a broken clock)
            let a = h.step(inp(5, 4.0, true, 0.0));
            if a.released.is_some() {
                released = a.released;
                break;
            }
        }
        assert_eq!(released, Some("safety budget"));
    }
}
