//! DDR SELECTION legacy song intro — the re-implemented A3 `ReadyGoActor`
//! state machine and the World-intro suppression rules (pure, host-tested).
//!
//! Dependency-free on purpose: `scripts/validate_ddr_selection.sh` mounts this
//! file into a throwaway host crate.
//!
//! A3's `sequence::dance::ReadyGoActor` (ctor `FUN_180042000`, msg
//! `FUN_180042570` on 20240402) owned two clips from `dance_message000N`,
//! `00_ready` and `00_here`, parked at frame 0, and reacted to the
//! ControlMessageActor's intro messages (A3 `0x104A/B/C`, World
//! `0x1047/48/49`):
//!
//! | msg | state | A3 action |
//! |---|---|---|
//! | READY | 0 → 1 | play `00_ready`; send `0x100D` (World `0x100c`) — dismiss the stage panel |
//! | HERE | 1 → 2 | goto `out` on `00_ready`, play `00_here`; skin 1: voice `ACT3_1` (`ACT4_2` on the final stage) |
//! | OUT | 2 → 3 | goto `out` on `00_here` (on `00_ready` when there is no HERE clip) |
//! | update | 3 | once every clip reached its `end` frame, the actor dies |
//!
//! World deleted the actor; its ControlMessageActor still fires the three
//! messages at the same chart-derived ticks and parks its StackStep at 1 / 2
//! / 3 after each, so the DLL drives this machine from the highest CMA step
//! (the earlier side in versus leads — the A3 "first trigger acts,
//! duplicates are no-ops" behaviour).

/// ControlMessageActor StackStep after READY / HERE / OUT fired.
pub const CMA_READY: i32 = 1;
pub const CMA_HERE: i32 = 2;
pub const CMA_OUT: i32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Clips created, parked at frame 0, invisible.
    Parked,
    Ready,
    Here,
    /// Out labels played; waiting for every clip's `end` frame.
    Out,
    /// Finished (or skipped) — clips may be destroyed.
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Show `00_ready` and play it from frame 0.
    PlayReady,
    /// Goto-and-play `out` on `00_ready`.
    ReadyOut,
    /// Show `00_here` and play it from frame 0 (plus the era voice, if any).
    PlayHere { voice: Option<&'static str> },
    /// Goto-and-play `out` on `00_here`.
    HereOut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadyGo {
    stage: Stage,
    has_here: bool,
}

impl ReadyGo {
    /// A machine whose clips were created while the cascade sat at
    /// `first_step`. A cascade already past READY (the clips became available
    /// late) skips the intro: never flash READY mid-song.
    pub fn new(has_here: bool, first_step: i32) -> Self {
        ReadyGo {
            stage: if first_step >= CMA_READY {
                Stage::Done
            } else {
                Stage::Parked
            },
            has_here,
        }
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    /// Advance to `cascade_step`, returning every action due, in order (a
    /// frame may carry several: the CMA fires READY and HERE in one tick when
    /// both thresholds already passed). `voice` = the HERE voice cue.
    pub fn advance(&mut self, cascade_step: i32, voice: Option<&'static str>) -> Vec<Action> {
        let mut out = Vec::new();
        loop {
            match self.stage {
                Stage::Parked if cascade_step >= CMA_READY => {
                    out.push(Action::PlayReady);
                    self.stage = Stage::Ready;
                }
                Stage::Ready if cascade_step >= CMA_HERE => {
                    if self.has_here {
                        out.push(Action::ReadyOut);
                    }
                    out.push(Action::PlayHere { voice });
                    self.stage = Stage::Here;
                }
                Stage::Here if cascade_step >= CMA_OUT => {
                    out.push(if self.has_here {
                        Action::HereOut
                    } else {
                        Action::ReadyOut
                    });
                    self.stage = Stage::Out;
                }
                _ => return out,
            }
        }
    }

    /// In [`Stage::Out`]: every clip reached its `end` frame ⇒ done.
    pub fn clips_ended(&mut self, ready_ended: bool, here_ended: bool) -> bool {
        if self.stage == Stage::Out && ready_ended && (here_ended || !self.has_here) {
            self.stage = Stage::Done;
            return true;
        }
        false
    }
}

/// The HERE WE GO voice A3 played from code: skin 1 only (`ACT3_1`, `ACT4_2`
/// on the final stage); the other skins' voices are inside their clips.
pub fn here_voice(skin: u8, final_stage: bool) -> Option<&'static str> {
    match skin {
        1 if final_stage => Some("ACT4_2"),
        1 => Some("ACT3_1"),
        _ => None,
    }
}

/// A3's final-stage test (`FUN_180123a20`, non-course branch): the 0-based
/// `stage` is the last one when `stage + 1` equals the final-stage override,
/// or — override not naming it — equals `max_stage + 1` (the operator's
/// normal stage count). Extra stages are not "final".
pub fn is_final_stage(stage: i32, max_stage: i32, override_stage: i32) -> bool {
    let n = stage + 1;
    n == override_stage || (n != override_stage && n == max_stage + 1)
}

/// Shutter states (`services::shutter`).
pub const SHUTTER_REVEALING: i32 = 5;
pub const SHUTTER_PARKED_REVEALED: i32 = 6;

/// What the DLL can see of World's stage panel (`shutter_play`) this frame.
///
/// World's clip (common_shutter_v3) plays its `stage_out` reveal from frame
/// 325: the panel elements (`jacket_usr`, `info_%dp_usr`) leave at frame 344
/// and World's own `ready` sprite (the "READY?") is placed at frame 350,
/// held through `ready_loop` (408) until 477. A legacy intro must take the
/// panel down between those two points.
#[derive(Clone, Copy, Debug, Default)]
pub struct PanelView {
    pub shutter_state: i32,
    /// The stage panel is the active kind and nothing else is queued.
    pub stage_panel_alone: bool,
    /// `jacket_usr` was seen on this clip earlier (the element signal is
    /// trustworthy for this art).
    pub elements_seen: bool,
    /// `jacket_usr` is currently absent.
    pub elements_gone: bool,
    /// World's `ready` sprite is currently placed.
    pub world_ready_shown: bool,
    pub clip_frame: Option<u32>,
    pub ready_loop_frame: Option<u32>,
    /// The ControlMessageActor already fired READY.
    pub ready_fired: bool,
}

/// Whether to dismiss World's stage panel now: once its `stage_out` reveal
/// has taken the panel elements away (the reveal plays out naturally), and
/// before World's READY? — backstops: World's `ready` sprite is already
/// placed, the clip reached `ready_loop`, or READY fired (A3's `0x100D`).
/// With no element signal and no readable frame (label-less art), at once.
/// Only the stage panel alone (no banner pending) and only once revealing.
pub fn should_dismiss_world_panel(v: &PanelView) -> bool {
    if !v.stage_panel_alone {
        return false;
    }
    match v.shutter_state {
        SHUTTER_PARKED_REVEALED => {
            let past_ready_loop = match (v.clip_frame, v.ready_loop_frame) {
                (Some(f), Some(r)) if r > 0 => Some(f >= r),
                _ => None,
            };
            v.ready_fired
                || (v.elements_seen && v.elements_gone)
                || v.world_ready_shown
                || past_ready_loop == Some(true)
                || (!v.elements_seen && past_ready_loop.is_none())
        }
        SHUTTER_REVEALING => v.ready_fired,
        _ => false,
    }
}

/// DancePlaySequence steps during which the READY? dwell timer is seeded
/// (the pre-song init; step 5 is the gate itself).
pub fn seeds_dwell(dps_step: i32) -> bool {
    (0..=5).contains(&dps_step)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_sequence_with_here() {
        let mut m = ReadyGo::new(true, 0);
        assert_eq!(m.advance(0, None), vec![]);
        assert_eq!(m.advance(1, None), vec![Action::PlayReady]);
        assert_eq!(m.advance(1, None), vec![]);
        assert_eq!(
            m.advance(2, Some("ACT3_1")),
            vec![
                Action::ReadyOut,
                Action::PlayHere {
                    voice: Some("ACT3_1")
                }
            ]
        );
        assert_eq!(m.advance(3, None), vec![Action::HereOut]);
        assert_eq!(m.stage(), Stage::Out);
        assert!(!m.clips_ended(true, false));
        assert!(m.clips_ended(true, true));
        assert_eq!(m.stage(), Stage::Done);
        assert_eq!(m.advance(5, None), vec![]);
    }

    #[test]
    fn several_messages_in_one_frame() {
        let mut m = ReadyGo::new(true, 0);
        assert_eq!(
            m.advance(3, None),
            vec![
                Action::PlayReady,
                Action::ReadyOut,
                Action::PlayHere { voice: None },
                Action::HereOut
            ]
        );
    }

    #[test]
    fn no_here_clip() {
        let mut m = ReadyGo::new(false, 0);
        assert_eq!(m.advance(1, None), vec![Action::PlayReady]);
        assert_eq!(m.advance(2, None), vec![Action::PlayHere { voice: None }]);
        assert_eq!(m.advance(3, None), vec![Action::ReadyOut]);
        assert!(m.clips_ended(true, false));
    }

    #[test]
    fn late_clips_skip_the_intro() {
        let mut m = ReadyGo::new(true, 1);
        assert_eq!(m.stage(), Stage::Done);
        assert_eq!(m.advance(3, None), vec![]);
    }

    #[test]
    fn cascade_never_goes_backwards() {
        let mut m = ReadyGo::new(true, 0);
        m.advance(2, None);
        // An in-place reset leaves the CMA steps alone; a lower reading is
        // ignored, never replayed.
        assert_eq!(m.advance(0, None), vec![]);
        assert_eq!(m.stage(), Stage::Here);
    }

    #[test]
    fn voices() {
        assert_eq!(here_voice(1, false), Some("ACT3_1"));
        assert_eq!(here_voice(1, true), Some("ACT4_2"));
        for s in 2..=5 {
            assert_eq!(here_voice(s, true), None);
        }
        assert_eq!(here_voice(0, false), None);
    }

    #[test]
    fn final_stage() {
        // 3-stage session (max_stage = 2), no override.
        assert!(!is_final_stage(0, 2, -1));
        assert!(!is_final_stage(1, 2, -1));
        assert!(is_final_stage(2, 2, -1));
        assert!(!is_final_stage(3, 2, -1)); // extra stage
                                            // An override naming stage 2 (1-based) ends there.
        assert!(is_final_stage(1, 2, 2));
    }

    fn view(state: i32) -> PanelView {
        PanelView {
            shutter_state: state,
            stage_panel_alone: true,
            elements_seen: true,
            elements_gone: false,
            world_ready_shown: false,
            clip_frame: Some(330),
            ready_loop_frame: Some(408),
            ready_fired: false,
        }
    }

    #[test]
    fn panel_dismissal() {
        // Covered (4): never — the DancePlaySequence waits on it.
        let mut v = view(4);
        v.ready_fired = true;
        v.elements_gone = true;
        assert!(!should_dismiss_world_panel(&v));
        // Revealing (5): only once READY fired.
        assert!(!should_dismiss_world_panel(&view(5)));
        let mut v = view(5);
        v.ready_fired = true;
        assert!(should_dismiss_world_panel(&v));
        // Parked (6), reveal still showing the panel elements: wait.
        assert!(!should_dismiss_world_panel(&view(6)));
        // The elements left (frame 344): dismiss, before World's READY? (350).
        let mut v = view(6);
        v.clip_frame = Some(345);
        v.elements_gone = true;
        assert!(should_dismiss_world_panel(&v));
        // Backstops: World's ready sprite placed / ready_loop reached / READY.
        let mut v = view(6);
        v.world_ready_shown = true;
        assert!(should_dismiss_world_panel(&v));
        let mut v = view(6);
        v.clip_frame = Some(408);
        assert!(should_dismiss_world_panel(&v));
        let mut v = view(6);
        v.ready_fired = true;
        assert!(should_dismiss_world_panel(&v));
        // Elements never seen: absence means nothing; frame backstop only.
        let mut v = view(6);
        v.elements_seen = false;
        v.elements_gone = true;
        assert!(!should_dismiss_world_panel(&v));
        // ... and with no readable frame either (label-less art): at once.
        v.clip_frame = None;
        assert!(should_dismiss_world_panel(&v));
        // A banner pending / another kind: never.
        let mut v = view(6);
        v.stage_panel_alone = false;
        v.ready_fired = true;
        v.elements_gone = true;
        assert!(!should_dismiss_world_panel(&v));
    }

    #[test]
    fn dwell_steps() {
        assert!(seeds_dwell(0));
        assert!(seeds_dwell(5));
        assert!(!seeds_dwell(6));
        assert!(!seeds_dwell(7));
        assert!(!seeds_dwell(-1));
    }
}
