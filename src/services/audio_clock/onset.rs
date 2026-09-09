//! Pure arm / sanity-gate state machine of the deterministic audio clock
//! (design §2, §4, §5). Host-tested. The impure glue (`engine.rs` publishes
//! the [`Onset`] and the [`Line`](super::fit::Line) from the render thread;
//! `game.rs` feeds [`Session::frame`] from the clock-patch call-out on the
//! game thread) lives beside it and owns every atomic.
//!
//! Model recap: the stock music count is `T − A − S + J`. The corrected
//! clock replaces `T − A` with
//!
//! ```text
//! E(t_frame) = (P̂(t_frame) − F0) / Hz · 1000 + C + content_offset_wall_ms
//! C          = pass_period/2 + mean_lead + mean_margin (all in ms) + latency_bias
//! ```
//!
//! and the call-out returns `rbx' = round(E) − S` (the stub adds `J` after).
//! Every frame the session compares `E` with the stock `T − A`; it goes
//! ACTIVE once they agree within the sanity window, and it drops back to
//! passthrough (re-gating) whenever they disagree by more than it — which is
//! exactly what happens when the game re-anchors (seek/loop/adjust) before
//! `song_reset`'s content-origin publication for the new voice reaches us.
//! A voice that never agrees within `max_wait_frames` is Refused for good.

use super::fit::Line;

/// Onset of ONE voice instance, latched on the render thread inside the
/// first `0x43CAC0` produce for the voice's node (`node+0x5F8` 0 → >0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Onset {
    /// Voice-instance generation (increments at every identified start).
    pub generation: u64,
    /// Which cursor-accumulator epoch the frame values belong to (the fit
    /// bumps it on every reset that changes the frame domain).
    pub epoch: u64,
    /// `W` (frames written) at the start of the pass that mixed sample 0.
    pub f0: i64,
    /// QPC of that pass's cursor read.
    pub t_k: i64,
    /// Accumulated play / write-cursor / written frames at that pass.
    pub p_k: i64,
    pub wc_k: i64,
    pub w_k: i64,
    /// Output sample rate (frames per second).
    pub hz: u32,
}

impl Onset {
    /// This play's stock onset latency terms in ms: `(lead + margin)/Hz`.
    #[must_use]
    pub fn lead_margin_ms(&self) -> f64 {
        if self.hz == 0 {
            return 0.0;
        }
        (self.w_k - self.p_k) as f64 * 1000.0 / f64::from(self.hz)
    }
}

/// Gate parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GatePolicy {
    /// `|E − (T − A)|` must be within this to arm / stay active (ms).
    pub sanity_ms: f64,
    /// Consecutive disagreeing frames before the voice is refused.
    pub max_wait_frames: u32,
    /// A line older than this (QPC ticks behind `t_frame`) is stale: the
    /// render thread stalled — pass through until passes resume.
    pub stale_ticks: i64,
}

impl GatePolicy {
    #[must_use]
    pub fn new(frequency: i64) -> Self {
        Self {
            sanity_ms: 50.0,
            // Covers song_reset's 5 s cue-prepare timeout: a seek/replay's new
            // voice is identified BEFORE the anchor rewrite + origin
            // publication that make the two clocks agree again.
            max_wait_frames: 300,
            stale_ticks: frequency.saturating_mul(150).saturating_div(1000),
        }
    }
}

/// The mean-preserving latency constant `C` (ms) from the fit's window
/// statistics: half a pass period (the uniform Start→pass phase), the mean
/// lead ahead of the write cursor, the mean write→play margin, plus the
/// operator's bias. `None` while the line is not ready.
#[must_use]
pub fn latency_constant_ms(line: &Line, hz: u32, bias_ms: f64) -> Option<f64> {
    if !line.ready || hz == 0 || !(line.pass_frames > 0.0) {
        return None;
    }
    let per_frame_ms = 1000.0 / f64::from(hz);
    let c = line.pass_frames * per_frame_ms / 2.0
        + line.mean_lead * per_frame_ms
        + line.mean_margin * per_frame_ms
        + bias_ms;
    c.is_finite().then_some(c)
}

/// `E(t_frame)` in ms (design §2).
#[must_use]
pub fn elapsed_ms(line: &Line, t_frame: i64, onset: &Onset, c_ms: f64, offset_ms: i32) -> f64 {
    let frames = line.eval(t_frame) - onset.f0 as f64;
    frames * 1000.0 / f64::from(onset.hz.max(1)) + c_ms + f64::from(offset_ms)
}

/// Assist-tick alignment (design §10): the exact served shift a tick track
/// should start from so its clap for note `t` coincides with the song's
/// content `t`, given the tick voice's onset reading `e_tick0_ms` on the
/// corrected clock. Inputs in ms: `sound_offset` = S, `judgment_timing` =
/// sign-applied J, `wall_m0` = `wall(m0)` (the anchor the track was authored
/// against), `c_ms` = the latency constant. With `S == C` and no pass jitter
/// this equals the stock commit's `wall(mc_c − m0)`.
#[must_use]
pub fn tick_skip_star_ms(
    e_tick0_ms: f64,
    sound_offset: i32,
    judgment_timing: i32,
    wall_m0: f64,
    c_ms: f64,
) -> f64 {
    (e_tick0_ms - f64::from(sound_offset) + f64::from(judgment_timing)) - wall_m0 - c_ms
}

/// `rbx' = round(E) − S`, saturated to `i32`.
#[must_use]
pub fn corrected_rbx(elapsed_ms: f64, sound_offset: i32) -> i32 {
    let rounded = elapsed_ms.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32;
    rounded.saturating_sub(sound_offset)
}

/// Per-frame inputs gathered by the game-thread glue.
#[derive(Clone, Copy, Debug)]
pub struct FrameInput<'a> {
    /// The latest published onset (None = no identified voice).
    pub onset: Option<Onset>,
    /// The published line (the glue passes `None` when not ready or when
    /// its epoch differs from the onset's).
    pub line: Option<&'a Line>,
    /// QPC paired with this frame's `T`.
    pub t_frame: i64,
    /// Stock `T − A` (= `rbx + S`) as the stub computed it.
    pub stock_elapsed_ms: i32,
    /// `S` (`actor+0x16C`).
    pub sound_offset_ms: i32,
    /// `C` for this frame (None = not computable ⇒ passthrough).
    pub c_ms: Option<f64>,
    /// The current content-origin publication: wall ms of sample 0 (0 for a
    /// natural start) and its generation.
    pub origin_ms: i32,
    pub origin_generation: u64,
}

/// What the call-out returns for this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Decision {
    /// Return `rbx` unchanged.
    Passthrough,
    /// Return the corrected `rbx`.
    Corrected { rbx: i32, elapsed_ms: f64 },
}

/// Why a session left ACTIVE / never armed (for the INFO/WARN lines).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// Line missing / not ready / stale, or `C` unavailable.
    NoLine,
    /// The onset publication went away (stop/destroy/scene).
    NoOnset,
    /// A newer voice generation superseded the armed one.
    Superseded,
    /// `|E − (T − A)|` exceeded the sanity window while ACTIVE.
    Diverged,
    /// Explicit disarm from the glue.
    Explicit,
}

/// Notable transitions (one log line each on the impure side).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    Armed {
        generation: u64,
        /// `E − (T − A)` at the arm frame: this play's stock onset error.
        delta_ms: f64,
        elapsed_ms: f64,
        offset_ms: i32,
        c_ms: f64,
        /// Frames spent gating before agreement.
        waited_frames: u32,
    },
    Refused {
        generation: u64,
        last_delta_ms: f64,
    },
    Disarmed {
        generation: u64,
        reason: Reason,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Idle,
    Gating {
        generation: u64,
        frames: u32,
        last_delta: f64,
    },
    Active {
        generation: u64,
        offset_ms: i32,
        origin_generation: u64,
    },
    Refused {
        generation: u64,
    },
}

/// The game-thread arm/gate session.
#[derive(Clone, Copy, Debug)]
pub struct Session {
    phase: Phase,
    policy: GatePolicy,
}

impl Session {
    #[must_use]
    pub fn new(policy: GatePolicy) -> Self {
        Self {
            phase: Phase::Idle,
            policy,
        }
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        matches!(self.phase, Phase::Active { .. })
    }

    #[must_use]
    pub fn active_generation(&self) -> Option<u64> {
        match self.phase {
            Phase::Active { generation, .. } => Some(generation),
            _ => None,
        }
    }

    /// Explicit disarm (scene left the play scenes, mod disabled, ...).
    pub fn disarm(&mut self, reason: Reason) -> Option<Event> {
        let generation = match self.phase {
            Phase::Idle => return None,
            Phase::Gating { generation, .. }
            | Phase::Active { generation, .. }
            | Phase::Refused { generation } => generation,
        };
        self.phase = Phase::Idle;
        Some(Event::Disarmed { generation, reason })
    }

    /// One `GamePlayActor::onUpdate` evaluation.
    pub fn frame(&mut self, input: FrameInput<'_>) -> (Decision, Option<Event>) {
        let Some(onset) = input.onset else {
            return (Decision::Passthrough, self.disarm(Reason::NoOnset));
        };
        // A new voice instance supersedes whatever we were doing.
        let current = match self.phase {
            Phase::Idle => None,
            Phase::Gating { generation, .. }
            | Phase::Active { generation, .. }
            | Phase::Refused { generation } => Some(generation),
        };
        let mut event = None;
        if current != Some(onset.generation) {
            if let Some(generation) = current {
                if matches!(self.phase, Phase::Active { .. }) {
                    event = Some(Event::Disarmed {
                        generation,
                        reason: Reason::Superseded,
                    });
                }
            }
            self.phase = Phase::Gating {
                generation: onset.generation,
                frames: 0,
                last_delta: 0.0,
            };
        }
        if let Phase::Refused { .. } = self.phase {
            return (Decision::Passthrough, event);
        }
        let (Some(line), Some(c_ms)) = (input.line, input.c_ms) else {
            // Not ready / stale: stay in the phase we are in (an ACTIVE
            // session resumes with the same F0 when passes return).
            return (Decision::Passthrough, event);
        };
        if !line.ready || input.t_frame - line.t_ref > self.policy.stale_ticks {
            return (Decision::Passthrough, event);
        }
        match self.phase {
            Phase::Active {
                generation,
                offset_ms,
                origin_generation,
            } => {
                // A newer content-origin publication means the game
                // re-anchored (seek/adjust): re-gate against the new origin.
                let (offset_ms, origin_generation) = if origin_generation != input.origin_generation
                {
                    (input.origin_ms, input.origin_generation)
                } else {
                    (offset_ms, origin_generation)
                };
                let elapsed = elapsed_ms(line, input.t_frame, &onset, c_ms, offset_ms);
                let delta = elapsed - f64::from(input.stock_elapsed_ms);
                if delta.abs() <= self.policy.sanity_ms {
                    self.phase = Phase::Active {
                        generation,
                        offset_ms,
                        origin_generation,
                    };
                    return (
                        Decision::Corrected {
                            rbx: corrected_rbx(elapsed, input.sound_offset_ms),
                            elapsed_ms: elapsed,
                        },
                        event,
                    );
                }
                self.phase = Phase::Gating {
                    generation,
                    frames: 1,
                    last_delta: delta,
                };
                (
                    Decision::Passthrough,
                    Some(Event::Disarmed {
                        generation,
                        reason: Reason::Diverged,
                    }),
                )
            }
            Phase::Gating {
                generation, frames, ..
            } => {
                let elapsed = elapsed_ms(line, input.t_frame, &onset, c_ms, input.origin_ms);
                let delta = elapsed - f64::from(input.stock_elapsed_ms);
                if delta.abs() <= self.policy.sanity_ms {
                    self.phase = Phase::Active {
                        generation,
                        offset_ms: input.origin_ms,
                        origin_generation: input.origin_generation,
                    };
                    return (
                        Decision::Corrected {
                            rbx: corrected_rbx(elapsed, input.sound_offset_ms),
                            elapsed_ms: elapsed,
                        },
                        Some(Event::Armed {
                            generation,
                            delta_ms: delta,
                            elapsed_ms: elapsed,
                            offset_ms: input.origin_ms,
                            c_ms,
                            waited_frames: frames,
                        }),
                    );
                }
                let frames = frames + 1;
                if frames > self.policy.max_wait_frames {
                    self.phase = Phase::Refused { generation };
                    return (
                        Decision::Passthrough,
                        Some(Event::Refused {
                            generation,
                            last_delta_ms: delta,
                        }),
                    );
                }
                self.phase = Phase::Gating {
                    generation,
                    frames,
                    last_delta: delta,
                };
                (Decision::Passthrough, event)
            }
            Phase::Idle | Phase::Refused { .. } => (Decision::Passthrough, event),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FREQ: i64 = 10_000_000;
    const HZ: u32 = 44_100;

    fn line(t_ref: i64, p_ref: f64) -> Line {
        Line {
            n: 1000,
            t_ref,
            p_ref,
            slope: f64::from(HZ) / FREQ as f64,
            resid_sd: 148.0,
            mean_lead: 1764.0,  // 40 ms
            mean_margin: 441.0, // 10 ms
            pass_frames: 441.0,
            ready: true,
        }
    }

    fn onset(generation: u64, f0: i64) -> Onset {
        Onset {
            generation,
            epoch: 1,
            f0,
            t_k: 0,
            p_k: f0 - 2205,
            wc_k: f0 - 1764,
            w_k: f0,
            hz: HZ,
        }
    }

    fn policy() -> GatePolicy {
        GatePolicy::new(FREQ)
    }

    /// Frames → ms at 44.1 kHz.
    fn ms_of(frames: f64) -> f64 {
        frames * 1000.0 / f64::from(HZ)
    }

    /// `Corrected` with an exact rbx and an approximate elapsed.
    fn assert_corrected(d: Decision, rbx: i32, elapsed: f64) {
        match d {
            Decision::Corrected {
                rbx: got,
                elapsed_ms,
            } => {
                assert_eq!(got, rbx, "rbx (elapsed {elapsed_ms})");
                assert!(
                    (elapsed_ms - elapsed).abs() < 1e-6,
                    "{elapsed_ms} vs {elapsed}"
                );
            }
            other => panic!("expected Corrected, got {other:?}"),
        }
    }

    #[test]
    fn latency_constant_is_half_pass_plus_lead_plus_margin() {
        let l = line(0, 0.0);
        let c = latency_constant_ms(&l, HZ, 0.0).unwrap();
        assert!((c - (5.0 + 40.0 + 10.0)).abs() < 1e-9, "{c}");
        assert!((latency_constant_ms(&l, HZ, 1.5).unwrap() - 56.5).abs() < 1e-9);
        assert!(latency_constant_ms(&Line { ready: false, ..l }, HZ, 0.0).is_none());
        assert!(latency_constant_ms(&l, 0, 0.0).is_none());
        assert!(latency_constant_ms(
            &Line {
                pass_frames: 0.0,
                ..l
            },
            HZ,
            0.0
        )
        .is_none());
    }

    #[test]
    fn elapsed_and_rounding_follow_the_design_formula() {
        // 1 s after F0 at the DAC: E = 1000 + C + offset.
        let f0 = 100_000;
        let l = line(FREQ, (f0 + HZ as i64) as f64);
        let o = onset(1, f0);
        let e = elapsed_ms(&l, FREQ, &o, 55.0, 0);
        assert!((e - 1055.0).abs() < 1e-6, "{e}");
        let e = elapsed_ms(&l, FREQ, &o, 55.0, 30_000);
        assert!((e - 31_055.0).abs() < 1e-6);
        assert_eq!(corrected_rbx(1054.4, 20), 1034);
        assert_eq!(corrected_rbx(1054.6, 20), 1035);
        assert_eq!(corrected_rbx(-0.4, 0), 0);
        assert_eq!(corrected_rbx(f64::MAX, 0), i32::MAX);
        assert_eq!(corrected_rbx(f64::MIN, 1), i32::MIN);
        assert!((o.lead_margin_ms() - 50.0).abs() < 1e-9);
    }

    fn input<'a>(
        onset: Option<Onset>,
        line: Option<&'a Line>,
        t_frame: i64,
        stock: i32,
        origin: (i32, u64),
    ) -> FrameInput<'a> {
        FrameInput {
            onset,
            line,
            t_frame,
            stock_elapsed_ms: stock,
            sound_offset_ms: 20,
            c_ms: Some(55.0),
            origin_ms: origin.0,
            origin_generation: origin.1,
        }
    }

    #[test]
    fn natural_start_arms_on_the_first_agreeing_frame_and_reports_the_stock_error() {
        let mut s = Session::new(policy());
        let f0 = 500_000;
        // Frame 16 ms after the pass: DAC at f0 + 705.6 frames → E = 16 + 55 = 71 ms.
        let t = 160_000;
        let l = line(t, f0 as f64 + 705.6);
        // Stock says 68 ms (this play's onset landed 3 ms early vs the mean).
        let (d, e) = s.frame(input(Some(onset(1, f0)), Some(&l), t, 68, (0, 1)));
        match e {
            Some(Event::Armed {
                generation: 1,
                delta_ms,
                offset_ms: 0,
                waited_frames: 0,
                ..
            }) => assert!((delta_ms - 3.0).abs() < 1e-6, "{delta_ms}"),
            other => panic!("{other:?}"),
        }
        assert_corrected(d, 71 - 20, 71.0);
        assert!(s.is_active());
        assert_eq!(s.active_generation(), Some(1));
        // Steady state: 5 s later the game tick has drifted +0.1 ms; still corrected.
        let t2 = t + 5 * FREQ;
        let l2 = line(t2, f0 as f64 + 705.6 + 5.0 * f64::from(HZ));
        let (d2, e2) = s.frame(input(Some(onset(1, f0)), Some(&l2), t2, 5_068, (0, 1)));
        assert!(e2.is_none());
        assert_corrected(d2, 5_071 - 20, 5_071.0);
    }

    #[test]
    fn disagreement_waits_then_refuses_once() {
        let mut s = Session::new(policy());
        let f0 = 500_000;
        let l = line(0, f0 as f64);
        // Stock is 400 ms off (wrong voice / seek offset missing).
        for i in 0..policy().max_wait_frames {
            let (d, e) = s.frame(input(Some(onset(1, f0)), Some(&l), 0, 455, (0, 1)));
            assert_eq!(d, Decision::Passthrough, "frame {i}");
            assert!(e.is_none(), "frame {i}: {e:?}");
        }
        let (d, e) = s.frame(input(Some(onset(1, f0)), Some(&l), 0, 455, (0, 1)));
        assert_eq!(d, Decision::Passthrough);
        match e {
            Some(Event::Refused {
                generation: 1,
                last_delta_ms,
            }) => assert!((last_delta_ms + 400.0).abs() < 1e-6),
            other => panic!("{other:?}"),
        }
        // Stays refused silently for this generation, even if stock later agrees.
        let (d, e) = s.frame(input(Some(onset(1, f0)), Some(&l), 0, 55, (0, 1)));
        assert_eq!((d, e), (Decision::Passthrough, None));
        // A new voice generation gets a fresh gate.
        let (d, e) = s.frame(input(Some(onset(2, f0)), Some(&l), 0, 55, (0, 1)));
        assert!(matches!(e, Some(Event::Armed { generation: 2, .. })));
        assert!(matches!(d, Decision::Corrected { .. }));
    }

    #[test]
    fn seek_regates_on_divergence_and_rearms_with_the_published_origin() {
        let mut s = Session::new(policy());
        let f0 = 500_000;
        let l = line(0, f0 as f64);
        let (_, e) = s.frame(input(Some(onset(1, f0)), Some(&l), 0, 55, (0, 1)));
        assert!(matches!(e, Some(Event::Armed { .. })));
        // The game re-anchors to content 30 s (seek); our origin publication
        // has NOT arrived yet: divergence ⇒ passthrough, not a wrong clock.
        let (d, e) = s.frame(input(Some(onset(1, f0)), Some(&l), 0, 30_055, (0, 1)));
        assert_eq!(d, Decision::Passthrough);
        assert_eq!(
            e,
            Some(Event::Disarmed {
                generation: 1,
                reason: Reason::Diverged
            })
        );
        assert!(!s.is_active());
        // Origin arrives (wall 30 000 ms): agreement ⇒ re-armed.
        let (d, e) = s.frame(input(Some(onset(1, f0)), Some(&l), 0, 30_055, (30_000, 2)));
        assert!(matches!(
            e,
            Some(Event::Armed {
                offset_ms: 30_000,
                waited_frames: 1,
                ..
            })
        ));
        assert_corrected(d, 30_055 - 20, 30_055.0);
    }

    #[test]
    fn origin_published_while_active_is_adopted_when_stock_already_moved() {
        // adjust_run_to: stock re-anchors and the origin publication lands
        // on the SAME frame we evaluate.
        let mut s = Session::new(policy());
        let f0 = 500_000;
        let l = line(0, f0 as f64);
        s.frame(input(Some(onset(1, f0)), Some(&l), 0, 55, (0, 1)));
        let (d, e) = s.frame(input(Some(onset(1, f0)), Some(&l), 0, 12_055, (12_000, 2)));
        assert!(e.is_none());
        assert_corrected(d, 12_055 - 20, 12_055.0);
        assert!(s.is_active());
    }

    #[test]
    fn missing_or_stale_line_passes_through_without_losing_the_arm() {
        let mut s = Session::new(policy());
        let f0 = 500_000;
        let l = line(0, f0 as f64);
        s.frame(input(Some(onset(1, f0)), Some(&l), 0, 55, (0, 1)));
        // No line (fit reset) ⇒ passthrough, still armed.
        let (d, e) = s.frame(input(Some(onset(1, f0)), None, 100, 65, (0, 1)));
        assert_eq!((d, e), (Decision::Passthrough, None));
        assert!(s.is_active());
        // Stale line (render thread stalled > 150 ms) ⇒ passthrough.
        let stale_t = FREQ / 5; // 200 ms after the line's newest sample
        let (d, e) = s.frame(input(Some(onset(1, f0)), Some(&l), stale_t, 255, (0, 1)));
        assert_eq!((d, e), (Decision::Passthrough, None));
        assert!(s.is_active());
        // Not-ready line ⇒ passthrough.
        let nr = Line { ready: false, ..l };
        let (d, _) = s.frame(input(Some(onset(1, f0)), Some(&nr), 0, 55, (0, 1)));
        assert_eq!(d, Decision::Passthrough);
        // No C ⇒ passthrough.
        let mut i = input(Some(onset(1, f0)), Some(&l), 0, 55, (0, 1));
        i.c_ms = None;
        assert_eq!(s.frame(i).0, Decision::Passthrough);
        // Passes resume with the same F0: corrected again, no new Armed event.
        let (d, e) = s.frame(input(Some(onset(1, f0)), Some(&l), 0, 55, (0, 1)));
        assert!(matches!(d, Decision::Corrected { .. }));
        assert!(e.is_none());
    }

    #[test]
    fn onset_removal_and_explicit_disarm_report_once() {
        let mut s = Session::new(policy());
        let f0 = 500_000;
        let l = line(0, f0 as f64);
        s.frame(input(Some(onset(1, f0)), Some(&l), 0, 55, (0, 1)));
        let (d, e) = s.frame(input(None, Some(&l), 0, 55, (0, 1)));
        assert_eq!(d, Decision::Passthrough);
        assert_eq!(
            e,
            Some(Event::Disarmed {
                generation: 1,
                reason: Reason::NoOnset
            })
        );
        assert_eq!(
            s.frame(input(None, Some(&l), 0, 55, (0, 1))),
            (Decision::Passthrough, None)
        );
        assert!(s.disarm(Reason::Explicit).is_none());
        s.frame(input(Some(onset(3, f0)), Some(&l), 0, 55, (0, 1)));
        assert_eq!(
            s.disarm(Reason::Explicit),
            Some(Event::Disarmed {
                generation: 3,
                reason: Reason::Explicit
            })
        );
        assert!(!s.is_active());
    }

    #[test]
    fn superseding_voice_disarms_the_old_one_and_gates_the_new() {
        let mut s = Session::new(policy());
        let f0 = 500_000;
        let l = line(0, f0 as f64);
        s.frame(input(Some(onset(1, f0)), Some(&l), 0, 55, (0, 1)));
        // Restart: new voice at F0' = f0 + 10 s of frames; the stock anchor
        // moved with it, so agreement is immediate.
        let f0b = f0 + 10 * HZ as i64;
        let lb = line(0, f0b as f64);
        let (d, e) = s.frame(input(Some(onset(2, f0b)), Some(&lb), 0, 55, (0, 1)));
        // The Superseded disarm is folded into the same frame's Armed.
        assert!(
            matches!(e, Some(Event::Armed { generation: 2, .. })),
            "{e:?}"
        );
        assert!(matches!(d, Decision::Corrected { .. }));
        assert_eq!(s.active_generation(), Some(2));
    }

    #[test]
    fn tick_skip_star_reproduces_the_stock_commit_when_calibrated_and_jitter_free() {
        // Stock: Play at wall w_c; count mc_c = w_c − S + J; served skip =
        // mc_c − m0 (identity rate). The tick's sample 0 is heard S later,
        // which on a DAC clock with C == S reads E_tick0 = w_c + C.
        let (s, j, m0, c) = (87, 10, 1_234, 87.0);
        let w_c = 20_000.0;
        let mc_c = w_c - f64::from(s) + f64::from(j);
        let stock_skip = mc_c - f64::from(m0);
        let e_tick0 = w_c + c;
        let skip = tick_skip_star_ms(e_tick0, s, j, f64::from(m0), c);
        assert!((skip - stock_skip).abs() < 1e-9, "{skip} vs {stock_skip}");
        // A tick that started one pass (10 ms) later than the mean phase
        // must be served 10 ms further into the track.
        let late = tick_skip_star_ms(e_tick0 + 10.0, s, j, f64::from(m0), c);
        assert!((late - (stock_skip + 10.0)).abs() < 1e-9);
        // An operator SOUND_OFFSET 5 ms above the measured latency shifts the
        // authored positions by −5 (the `−S`), so the served start must move
        // by the same −5 to keep the claps on the song.
        let miscal = tick_skip_star_ms(e_tick0, s + 5, j, f64::from(m0), c);
        assert!((miscal - (stock_skip - 5.0)).abs() < 1e-9);
    }

    #[test]
    fn sub_millisecond_evaluation_rounds_consistently() {
        // The corrected count must be the ROUNDED elapsed minus S — never a
        // truncation that would bias every judgement by up to 1 ms.
        let mut s = Session::new(policy());
        let f0 = 500_000;
        let l = line(0, f0 as f64 + 26.46); // +0.6 ms
        s.frame(input(Some(onset(1, f0)), Some(&l), 0, 55, (0, 1)));
        let d = s
            .frame(input(Some(onset(1, f0)), Some(&l), 0, 55, (0, 1)))
            .0;
        // E = 55.6 → round = 56, minus S=20 (a truncation would give 35).
        assert_corrected(d, 36, 55.6);
        let l = line(0, f0 as f64 + 17.64); // +0.4 ms
        let d = s
            .frame(input(Some(onset(1, f0)), Some(&l), 0, 55, (0, 1)))
            .0;
        assert_corrected(d, 35, 55.4);
        let ms = ms_of(26.46);
        assert!((ms - 0.6).abs() < 1e-9);
    }
}
