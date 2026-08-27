//! Host tests for the pure song-rate lifecycle: scalar rate-domain
//! validation, scene-26 eligibility classification, and the scene-transition
//! engine's effect ordering (through a recording sink) plus end-state
//! behavior (through a sink wired to a real `MoviePolicy` + `RatePublication`).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use super::clock_patch::{RatePublication, IDENTITY_Q31};
use super::lifecycle::{
    classify_scene26, is_supported_rate_percent, snap_rate_percent, ArmOutcome, ArmRequest,
    EligibilityDecision, EligibilityInputs, GenerationPhase, IdentityReason, LifecycleSink,
    LifecycleState, PhaseError, TransitionOutcome, IDENTITY_PERCENT, MAX_RATE_PERCENT,
    MIN_RATE_PERCENT,
};
use crate::services::movie_policy::{MoviePolicy, MovieSuppressor};
use crate::types::scenes::scene;

// ── Test doubles ─────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Effect {
    PublishIdentity { generation: u64, mask: u8 },
    ResetIdentity,
    Movie(bool),
}

#[derive(Default)]
struct RecordingSink {
    effects: Mutex<Vec<Effect>>,
}

impl RecordingSink {
    fn take(&self) -> Vec<Effect> {
        std::mem::take(&mut self.effects.lock().unwrap())
    }
}

impl LifecycleSink for RecordingSink {
    fn publish_identity(&self, generation: u64, mask: u8) {
        self.effects
            .lock()
            .unwrap()
            .push(Effect::PublishIdentity { generation, mask });
    }
    fn reset_identity(&self) {
        self.effects.lock().unwrap().push(Effect::ResetIdentity);
    }
    fn set_movie_suppressed(&self, suppressed: bool) {
        self.effects.lock().unwrap().push(Effect::Movie(suppressed));
    }
}

/// Sink over the real shared pieces, to verify end states (identity Q31,
/// snapshot fields, movie contributor) rather than call traces.
struct RealSink {
    factor: &'static AtomicU64,
    publication: RatePublication,
    movie: MoviePolicy,
}

impl RealSink {
    fn new() -> Self {
        let factor: &'static AtomicU64 = Box::leak(Box::new(AtomicU64::new(0)));
        Self {
            factor,
            publication: RatePublication::new(factor),
            movie: MoviePolicy::new(),
        }
    }
    fn factor(&self) -> u64 {
        self.factor.load(Ordering::Acquire)
    }
}

impl LifecycleSink for RealSink {
    fn publish_identity(&self, generation: u64, mask: u8) {
        self.publication.publish_identity(generation, mask);
    }
    fn reset_identity(&self) {
        let _ = self.publication.reset_identity();
    }
    fn set_movie_suppressed(&self, suppressed: bool) {
        self.movie.set(MovieSuppressor::SongRate, suppressed);
    }
}

/// Eligible baseline: P1 entered desiring 75%, P2 desiring 125% — distinct
/// non-identity values so per-side selection is observable. The
/// preserve-pitch flags are likewise distinct (P1 false, P2 true) so the
/// per-side flag latch is observable through the same assertions.
fn eligible_inputs() -> EligibilityInputs {
    EligibilityInputs {
        services_ready: true,
        desired: Some([75, 125]),
        desired_preserve: [false, true],
        desired_sync: [false, false],
        training_arm: false,
        course_field: Some(0),
        entered: Some([true, false]),
        stage_index: Some(0),
    }
}

fn armed_state(sink: &dyn LifecycleSink) -> (LifecycleState, u64) {
    let state = LifecycleState::new();
    let outcome = state.on_scene26(
        sink,
        EligibilityDecision::Arm(ArmRequest {
            requested_percent: 75,
            preserve_pitch: true,
            sync_movie: false,
            participant_mask: 0b01,
            stage_index: 0,
        }),
    );
    let ArmOutcome::Armed { generation } = outcome else {
        panic!("expected armed outcome, got {outcome:?}");
    };
    (state, generation)
}

// ── Rate-domain validation ───────────────────────────────────────────

#[test]
fn snap_normalizes_any_persisted_value_into_the_domain() {
    // In-domain values pass through untouched.
    for percent in [
        MIN_RATE_PERCENT,
        50,
        75,
        IDENTITY_PERCENT,
        125,
        MAX_RATE_PERCENT,
    ] {
        assert_eq!(snap_rate_percent(percent), percent);
    }
    // Non-multiples snap half-up to the nearest step.
    assert_eq!(snap_rate_percent(77), 75);
    assert_eq!(snap_rate_percent(78), 80);
    assert_eq!(snap_rate_percent(101), 100);
    assert_eq!(snap_rate_percent(103), 105);
    assert_eq!(snap_rate_percent(172), 170);
    assert_eq!(snap_rate_percent(173), 175);
    // Out-of-range values clamp to the boundaries.
    for (value, expected) in [
        (i32::MIN, MIN_RATE_PERCENT),
        (-75, MIN_RATE_PERCENT),
        (0, MIN_RATE_PERCENT),
        (24, MIN_RATE_PERCENT),
        (176, MAX_RATE_PERCENT),
        (1_000, MAX_RATE_PERCENT),
        (i32::MAX, MAX_RATE_PERCENT),
    ] {
        assert_eq!(snap_rate_percent(value), expected, "snap({value})");
    }
    // The snapped result is ALWAYS a supported percent — the normalizer can
    // never emit a value the classifier would fail closed on.
    for value in [i32::MIN, -1, 0, 23, 26, 77, 99, 101, 174, 176, i32::MAX] {
        assert!(
            is_supported_rate_percent(snap_rate_percent(value)),
            "snap({value}) left the domain"
        );
    }
}

#[test]
fn rate_domain_accepts_exactly_the_scalar_multiples_of_five() {
    assert!(is_supported_rate_percent(MIN_RATE_PERCENT));
    assert!(is_supported_rate_percent(MAX_RATE_PERCENT));
    assert!(is_supported_rate_percent(IDENTITY_PERCENT));
    for percent in [30, 55, 75, 95, 105, 125, 150, 170] {
        assert!(is_supported_rate_percent(percent), "{percent} must pass");
    }
    for percent in [i32::MIN, -75, 0, 5, 20, 24, 26, 77, 101, 176, 180, i32::MAX] {
        assert!(!is_supported_rate_percent(percent), "{percent} must fail");
    }
}

// ── Scene-26 eligibility (AC1) ───────────────────────────────────────

#[test]
fn solo_p1_arms_with_its_mask_and_stage() {
    let decision = classify_scene26(&eligible_inputs());
    assert_eq!(
        decision,
        EligibilityDecision::Arm(ArmRequest {
            requested_percent: 75,
            preserve_pitch: false,
            sync_movie: false,
            participant_mask: 0b01,
            stage_index: 0,
        })
    );
}

#[test]
fn p2_started_doubles_arms_side1_mask_with_side1_rate() {
    let mut inputs = eligible_inputs();
    inputs.entered = Some([false, true]);
    inputs.stage_index = Some(2);
    // The ENTERED side's desired value selects the shared rate — P2's 125,
    // not P1's 75.
    assert_eq!(
        classify_scene26(&inputs),
        EligibilityDecision::Arm(ArmRequest {
            requested_percent: 125,
            preserve_pitch: true,
            sync_movie: false,
            participant_mask: 0b10,
            stage_index: 2,
        })
    );
}

/// The preserve-pitch flag latches from the ENTERED side, both values,
/// and never affects the eligibility decision itself.
#[test]
fn preserve_pitch_latches_from_the_entered_side() {
    for (entered, side) in [([true, false], 0usize), ([false, true], 1usize)] {
        for flag in [false, true] {
            let mut inputs = eligible_inputs();
            inputs.entered = Some(entered);
            let mut preserve = [!flag, !flag];
            preserve[side] = flag;
            inputs.desired_preserve = preserve;
            match classify_scene26(&inputs) {
                EligibilityDecision::Arm(request) => assert_eq!(
                    request.preserve_pitch, flag,
                    "side {side} flag {flag} must latch from the entered side"
                ),
                other => panic!("expected an arm, got {other:?}"),
            }
        }
    }
    // Identity outcomes ignore the flag entirely (100% desired).
    let mut inputs = eligible_inputs();
    inputs.desired = Some([100, 100]);
    inputs.desired_preserve = [false, false];
    assert_eq!(
        classify_scene26(&inputs),
        EligibilityDecision::Identity(IdentityReason::IdentityRate)
    );
}

/// The sync-background-video flag latches from the ENTERED side (mirror of
/// the preserve-pitch carriage) and never affects eligibility itself.
#[test]
fn sync_movie_latches_from_the_entered_side() {
    for (entered, side) in [([true, false], 0usize), ([false, true], 1usize)] {
        for flag in [false, true] {
            let mut inputs = eligible_inputs();
            inputs.entered = Some(entered);
            let mut sync = [!flag, !flag];
            sync[side] = flag;
            inputs.desired_sync = sync;
            match classify_scene26(&inputs) {
                EligibilityDecision::Arm(request) => assert_eq!(
                    request.sync_movie, flag,
                    "side {side} flag {flag} must latch from the entered side"
                ),
                other => panic!("expected an arm, got {other:?}"),
            }
        }
    }
}

/// Background-movie-sync design (decision matrix): a non-identity arm with
/// sync latched ON skips the tentative movie suppression — the graph
/// builds normally for `movie_sync` to rate-lock — while stale suppression
/// from a prior attempt is still explicitly cleared. The next identity
/// scene 26 resets the latch.
#[test]
fn sync_on_arm_keeps_the_movie_unsuppressed_and_identity_resets_the_latch() {
    let sink = RealSink::new();
    let state = LifecycleState::new();
    // Simulate a prior attempt's stale suppression.
    sink.movie.set(MovieSuppressor::SongRate, true);
    let outcome = state.on_scene26(
        &sink,
        EligibilityDecision::Arm(ArmRequest {
            requested_percent: 75,
            preserve_pitch: true,
            sync_movie: true,
            participant_mask: 0b01,
            stage_index: 0,
        }),
    );
    assert!(matches!(outcome, ArmOutcome::Armed { .. }));
    assert!(state.sync_movie(), "the arm must latch the flag");
    assert!(
        !sink.movie.is_suppressed(MovieSuppressor::SongRate),
        "sync ON: the tentative suppression is skipped (and stale suppression cleared)"
    );
    // The clock stays identity through the arm exactly as with sync OFF.
    assert_eq!(sink.factor(), IDENTITY_Q31);
    // A later identity resolution resets the latch alongside the rest.
    let outcome = state.on_scene26(
        &sink,
        EligibilityDecision::Identity(IdentityReason::IdentityRate),
    );
    assert_eq!(outcome, ArmOutcome::Identity(IdentityReason::IdentityRate));
    assert!(!state.sync_movie(), "identity must reset the latch");
    // And a sync-OFF re-arm suppresses exactly as shipped.
    let outcome = state.on_scene26(
        &sink,
        EligibilityDecision::Arm(ArmRequest {
            requested_percent: 75,
            preserve_pitch: true,
            sync_movie: false,
            participant_mask: 0b01,
            stage_index: 0,
        }),
    );
    assert!(matches!(outcome, ArmOutcome::Armed { .. }));
    assert!(!state.sync_movie());
    assert!(sink.movie.is_suppressed(MovieSuppressor::SongRate));
}

// ── Training-arm identity acceptance (training mode Step 1) ──────────

#[test]
fn training_arm_makes_identity_percent_armable() {
    // The training-arm request (training design §4.5): an eligible 100%
    // entry ARMS instead of resolving IdentityRate, so gestures can seek
    // even at identity. Everything else about the request is the ordinary
    // arm shape.
    let mut inputs = eligible_inputs();
    inputs.desired = Some([100, 100]);
    inputs.training_arm = true;
    assert_eq!(
        classify_scene26(&inputs),
        EligibilityDecision::Arm(ArmRequest {
            requested_percent: 100,
            preserve_pitch: false,
            sync_movie: false,
            participant_mask: 0b01,
            stage_index: 0,
        })
    );

    // The identity pin (unchanged shipped behavior): without the request,
    // a 100% desire never arms.
    let mut inputs = eligible_inputs();
    inputs.desired = Some([100, 100]);
    assert_eq!(
        classify_scene26(&inputs),
        EligibilityDecision::Identity(IdentityReason::IdentityRate)
    );

    // Non-identity desires arm exactly as before with the request set.
    let mut inputs = eligible_inputs();
    inputs.training_arm = true;
    assert_eq!(
        classify_scene26(&inputs),
        EligibilityDecision::Arm(ArmRequest {
            requested_percent: 75,
            preserve_pitch: false,
            sync_movie: false,
            participant_mask: 0b01,
            stage_index: 0,
        })
    );
}

#[test]
fn training_arm_never_weakens_the_eligibility_gates() {
    // Ineligible sessions fail closed identically with the request set
    // (ordinary solo/doubles only — versus, course, unknown all refuse).
    let with_training = |mutate: fn(&mut EligibilityInputs)| {
        let mut inputs = eligible_inputs();
        inputs.desired = Some([100, 100]);
        inputs.training_arm = true;
        mutate(&mut inputs);
        classify_scene26(&inputs)
    };
    assert_eq!(
        with_training(|inputs| inputs.entered = Some([true, true])),
        EligibilityDecision::Identity(IdentityReason::LocalVersus)
    );
    assert_eq!(
        with_training(|inputs| inputs.course_field = Some(1)),
        EligibilityDecision::Identity(IdentityReason::CourseMode)
    );
    assert_eq!(
        with_training(|inputs| inputs.entered = None),
        EligibilityDecision::Identity(IdentityReason::UnknownSession)
    );
    assert_eq!(
        with_training(|inputs| inputs.stage_index = None),
        EligibilityDecision::Identity(IdentityReason::StageUnknown)
    );
    assert_eq!(
        with_training(|inputs| inputs.services_ready = false),
        EligibilityDecision::Identity(IdentityReason::ServicesUnavailable)
    );
}

#[test]
fn identity_percent_arm_does_not_suppress_movies() {
    // Training design §4.5: no movie suppression for identity arms — the
    // clock and audio stay 1:1, so the DirectShow graph can follow. The
    // arm still publishes the generation identity, and the movie effect
    // slot carries an explicit CLEAR (stale suppression from a prior
    // attempt must not leak into the identity arm).
    let sink = RecordingSink::default();
    let state = LifecycleState::new();
    let outcome = state.on_scene26(
        &sink,
        EligibilityDecision::Arm(ArmRequest {
            requested_percent: 100,
            preserve_pitch: true,
            sync_movie: false,
            participant_mask: 0b01,
            stage_index: 0,
        }),
    );
    let ArmOutcome::Armed { generation } = outcome else {
        panic!("identity arm must be accepted, got {outcome:?}");
    };
    assert_eq!(state.phase(), GenerationPhase::Armed);
    assert_eq!(state.requested_percent(), 100);
    assert_eq!(
        sink.take(),
        vec![
            Effect::Movie(false),
            Effect::PublishIdentity {
                generation,
                mask: 0b01
            },
        ]
    );
}

#[test]
fn boundary_rates_arm_across_the_full_domain() {
    for percent in [MIN_RATE_PERCENT, 50, 175] {
        let mut inputs = eligible_inputs();
        inputs.desired = Some([percent, 100]);
        assert_eq!(
            classify_scene26(&inputs),
            EligibilityDecision::Arm(ArmRequest {
                requested_percent: percent,
                preserve_pitch: false,
                sync_movie: false,
                participant_mask: 0b01,
                stage_index: 0,
            }),
            "{percent}% must arm"
        );
    }
}

#[test]
fn every_excluded_or_ambiguous_mode_resolves_to_identity() {
    let cases: Vec<(EligibilityInputs, IdentityReason)> = vec![
        (
            EligibilityInputs {
                services_ready: false,
                ..eligible_inputs()
            },
            IdentityReason::ServicesUnavailable,
        ),
        (
            EligibilityInputs {
                desired: None,
                ..eligible_inputs()
            },
            IdentityReason::NoRateSource,
        ),
        (
            EligibilityInputs {
                desired: Some([100, 125]),
                ..eligible_inputs()
            },
            IdentityReason::IdentityRate,
        ),
        // Out-of-domain desired values fail closed — never partially arm.
        (
            EligibilityInputs {
                desired: Some([77, 100]),
                ..eligible_inputs()
            },
            IdentityReason::UnsupportedRate,
        ),
        (
            EligibilityInputs {
                desired: Some([20, 100]),
                ..eligible_inputs()
            },
            IdentityReason::UnsupportedRate,
        ),
        (
            EligibilityInputs {
                desired: Some([180, 100]),
                ..eligible_inputs()
            },
            IdentityReason::UnsupportedRate,
        ),
        (
            EligibilityInputs {
                course_field: Some(1),
                ..eligible_inputs()
            },
            IdentityReason::CourseMode,
        ),
        (
            EligibilityInputs {
                course_field: None,
                ..eligible_inputs()
            },
            IdentityReason::UnknownSession,
        ),
        (
            EligibilityInputs {
                entered: Some([false, false]),
                ..eligible_inputs()
            },
            IdentityReason::NoSideEntered,
        ),
        (
            EligibilityInputs {
                entered: Some([true, true]),
                ..eligible_inputs()
            },
            IdentityReason::LocalVersus,
        ),
        (
            EligibilityInputs {
                entered: None,
                ..eligible_inputs()
            },
            IdentityReason::UnknownSession,
        ),
        (
            EligibilityInputs {
                stage_index: None,
                ..eligible_inputs()
            },
            IdentityReason::StageUnknown,
        ),
        (
            EligibilityInputs {
                stage_index: Some(-1),
                ..eligible_inputs()
            },
            IdentityReason::StageUnknown,
        ),
    ];
    for (inputs, reason) in cases {
        assert_eq!(
            classify_scene26(&inputs),
            EligibilityDecision::Identity(reason),
            "expected identity({reason:?})"
        );
    }
}

// ── Arm transitions and tentative movie policy (AC1/AC4) ─────────────

#[test]
fn accepted_arm_suppresses_movie_and_publishes_identity_generation() {
    let sink = RecordingSink::default();
    let (state, generation) = armed_state(&sink);
    assert_eq!(state.phase(), GenerationPhase::Armed);
    assert_eq!(state.generation(), generation);
    assert_eq!(state.participant_mask(), 0b01);
    assert_eq!(state.stage_index(), 0);
    assert_eq!(state.requested_percent(), 75);
    // Movie suppression precedes any possible stage construction; the
    // publication carries the new generation at identity.
    assert_eq!(
        sink.take(),
        vec![
            Effect::Movie(true),
            Effect::PublishIdentity {
                generation,
                mask: 0b01
            },
        ]
    );
}

#[test]
fn arm_leaves_the_clock_factor_at_identity() {
    let sink = RealSink::new();
    let (_state, generation) = armed_state(&sink);
    assert_eq!(sink.factor(), IDENTITY_Q31);
    let snapshot = sink.publication.read();
    assert_eq!(snapshot.generation, generation);
    assert!(!snapshot.committed);
    assert!(sink.movie.is_suppressed(MovieSuppressor::SongRate));
    assert!(!sink.movie.is_suppressed(MovieSuppressor::NonNativeOs));
}

#[test]
fn identity_arm_clears_any_stale_movie_suppression() {
    let sink = RealSink::new();
    let (state, _generation) = armed_state(&sink);
    assert!(sink.movie.is_suppressed(MovieSuppressor::SongRate));
    // The next scene 26 resolves to identity (e.g. an excluded mode).
    let outcome = state.on_scene26(
        &sink,
        EligibilityDecision::Identity(IdentityReason::IdentityRate),
    );
    assert_eq!(outcome, ArmOutcome::Identity(IdentityReason::IdentityRate));
    assert_eq!(state.phase(), GenerationPhase::Identity);
    assert!(!sink.movie.is_suppressed(MovieSuppressor::SongRate));
    assert_eq!(sink.factor(), IDENTITY_Q31);
}

#[test]
fn rearm_at_a_later_scene26_takes_a_new_generation() {
    let sink = RecordingSink::default();
    let (state, first) = armed_state(&sink);
    sink.take();
    let outcome = state.on_scene26(
        &sink,
        EligibilityDecision::Arm(ArmRequest {
            requested_percent: 75,
            preserve_pitch: true,
            sync_movie: false,
            participant_mask: 0b10,
            stage_index: 1,
        }),
    );
    let ArmOutcome::Armed { generation: second } = outcome else {
        panic!("expected re-arm");
    };
    assert!(second > first);
    assert_eq!(state.participant_mask(), 0b10);
    assert_eq!(state.stage_index(), 1);
    // Movie suppression is (re)asserted for the new attempt.
    assert!(sink.take().contains(&Effect::Movie(true)));
}

#[test]
fn contended_arm_fails_closed_without_touching_state() {
    struct ReentrantSink<'a> {
        state: &'a LifecycleState,
        inner_outcome: Mutex<Option<ArmOutcome>>,
    }
    impl LifecycleSink for ReentrantSink<'_> {
        fn publish_identity(&self, _generation: u64, _mask: u8) {}
        fn reset_identity(&self) {}
        fn set_movie_suppressed(&self, _suppressed: bool) {
            // Re-enter while the writer guard is held: the nested arm must
            // observe Busy and change nothing.
            let outcome = self.state.on_scene26(
                &NullSink,
                EligibilityDecision::Arm(ArmRequest {
                    requested_percent: 75,
                    preserve_pitch: true,
                    sync_movie: false,
                    participant_mask: 0b10,
                    stage_index: 3,
                }),
            );
            *self.inner_outcome.lock().unwrap() = Some(outcome);
        }
    }
    struct NullSink;
    impl LifecycleSink for NullSink {
        fn publish_identity(&self, _generation: u64, _mask: u8) {}
        fn reset_identity(&self) {}
        fn set_movie_suppressed(&self, _suppressed: bool) {}
    }

    let state = LifecycleState::new();
    let sink = ReentrantSink {
        state: &state,
        inner_outcome: Mutex::new(None),
    };
    let outcome = state.on_scene26(
        &sink,
        EligibilityDecision::Arm(ArmRequest {
            requested_percent: 75,
            preserve_pitch: true,
            sync_movie: false,
            participant_mask: 0b01,
            stage_index: 0,
        }),
    );
    assert!(matches!(outcome, ArmOutcome::Armed { .. }));
    assert_eq!(
        sink.inner_outcome.lock().unwrap().take(),
        Some(ArmOutcome::Busy)
    );
    // The outer arm's state won.
    assert_eq!(state.participant_mask(), 0b01);
    assert_eq!(state.stage_index(), 0);
}

// ── Definitive transition rules (AC4) ────────────────────────────────

#[test]
fn armed_attempt_survives_the_interstitial_corridor() {
    let sink = RecordingSink::default();
    let (state, _) = armed_state(&sink);
    sink.take();
    for (prev, next) in [
        (scene::SONG_TO_STAGE_INTERSTITIAL, scene::STAGE_INDICATOR),
        (scene::STAGE_INDICATOR, scene::GAMEPLAY),
    ] {
        assert_eq!(
            state.on_transition(&sink, prev, next),
            TransitionOutcome::NoChange
        );
    }
    assert_eq!(state.phase(), GenerationPhase::Armed);
    assert!(sink.take().is_empty());
}

#[test]
fn pre_exposure_corridor_exit_abandons_to_identity() {
    let sink = RealSink::new();
    let (state, _) = armed_state(&sink);
    // The interstitial bailed back to song select without gameplay.
    assert_eq!(
        state.on_transition(&sink, scene::SONG_TO_STAGE_INTERSTITIAL, scene::SONG_SELECT),
        TransitionOutcome::AbandonedPreExposure
    );
    assert_eq!(state.phase(), GenerationPhase::Identity);
    assert!(!sink.movie.is_suppressed(MovieSuppressor::SongRate));
    assert_eq!(sink.factor(), IDENTITY_Q31);
}

#[test]
fn quick_restart_retains_everything() {
    let sink = RecordingSink::default();
    let (state, generation) = armed_state(&sink);
    sink.take();
    assert_eq!(
        state.on_transition(&sink, scene::GAMEPLAY, scene::GAMEPLAY),
        TransitionOutcome::QuickRestartRetained
    );
    assert_eq!(state.phase(), GenerationPhase::Armed);
    assert_eq!(state.generation(), generation);
    assert!(sink.take().is_empty());
}

#[test]
fn gameplay_exit_completes_with_identity_reset_before_movie_clear() {
    for phase_setup in ["armed", "early_failed", "committed"] {
        let sink = RecordingSink::default();
        let (state, generation) = armed_state(&sink);
        match phase_setup {
            "early_failed" => state.mark_early_failed(generation).unwrap(),
            "committed" => {
                state.begin_binding(generation).unwrap();
                state.mark_exposed(generation).unwrap();
                state.mark_committed(generation).unwrap();
            }
            _ => {}
        }
        sink.take();
        assert_eq!(
            state.on_transition(&sink, scene::GAMEPLAY, scene::STAGE_RESULT),
            TransitionOutcome::CompletedAttempt,
            "from {phase_setup}"
        );
        assert_eq!(state.phase(), GenerationPhase::Completed);
        // The design's definitive rule: write Q31 identity FIRST, then clear
        // the movie contributor.
        assert_eq!(
            sink.take(),
            vec![Effect::ResetIdentity, Effect::Movie(false)],
            "from {phase_setup}"
        );
    }
}

#[test]
fn early_failed_keeps_movie_suppression_until_the_definitive_boundary() {
    let sink = RealSink::new();
    let (state, generation) = armed_state(&sink);
    state.mark_early_failed(generation).unwrap();
    // The fallback song plays with stock audio, movie still suppressed
    // through the attempt (design req 42).
    assert!(sink.movie.is_suppressed(MovieSuppressor::SongRate));
    assert_eq!(
        state.on_transition(&sink, scene::GAMEPLAY, scene::STAGE_RESULT),
        TransitionOutcome::CompletedAttempt
    );
    assert!(!sink.movie.is_suppressed(MovieSuppressor::SongRate));
}

#[test]
fn gameplay_exit_with_identity_phase_changes_nothing() {
    let sink = RecordingSink::default();
    let state = LifecycleState::new();
    assert_eq!(
        state.on_transition(&sink, scene::GAMEPLAY, scene::STAGE_RESULT),
        TransitionOutcome::NoChange
    );
    assert!(sink.take().is_empty());
}

#[test]
fn late_failed_holds_movie_until_the_next_clean_selection_or_reset() {
    let sink = RealSink::new();
    let (state, generation) = armed_state(&sink);
    state.begin_binding(generation).unwrap();
    state.mark_exposed(generation).unwrap();
    state.mark_late_failed(generation).unwrap();
    // A loading-abort path does not pass a gameplay exit; suppression holds.
    assert_eq!(
        state.on_transition(&sink, scene::STAGE_INDICATOR, scene::SONG_SELECT),
        TransitionOutcome::NoChange
    );
    assert!(sink.movie.is_suppressed(MovieSuppressor::SongRate));
    // The next clean scene-26 selection clears it.
    let outcome = state.on_scene26(
        &sink,
        EligibilityDecision::Identity(IdentityReason::IdentityRate),
    );
    assert_eq!(outcome, ArmOutcome::Identity(IdentityReason::IdentityRate));
    assert!(!sink.movie.is_suppressed(MovieSuppressor::SongRate));
    assert_eq!(state.phase(), GenerationPhase::Identity);
}

#[test]
fn attract_reset_forces_identity_from_any_settled_phase() {
    for phase_setup in ["armed", "late_failed"] {
        let sink = RealSink::new();
        let (state, generation) = armed_state(&sink);
        if phase_setup == "late_failed" {
            state.begin_binding(generation).unwrap();
            state.mark_exposed(generation).unwrap();
            state.mark_late_failed(generation).unwrap();
        }
        assert_eq!(
            state.on_transition(&sink, scene::THANK_YOU, scene::ATTRACT_DEMO),
            TransitionOutcome::ForcedIdentity,
            "from {phase_setup}"
        );
        assert_eq!(state.phase(), GenerationPhase::Identity);
        assert!(!sink.movie.is_suppressed(MovieSuppressor::SongRate));
        assert_eq!(sink.factor(), IDENTITY_Q31);
    }
}

#[test]
fn arm_during_xact_in_flight_is_deferred_untouched() {
    let sink = RecordingSink::default();
    let (state, generation) = armed_state(&sink);
    state.begin_binding(generation).unwrap();
    state.mark_exposed(generation).unwrap();
    sink.take();
    let outcome = state.on_scene26(
        &sink,
        EligibilityDecision::Arm(ArmRequest {
            requested_percent: 75,
            preserve_pitch: true,
            sync_movie: false,
            participant_mask: 0b10,
            stage_index: 4,
        }),
    );
    assert_eq!(outcome, ArmOutcome::Deferred);
    assert_eq!(state.phase(), GenerationPhase::XactInFlight);
    assert_eq!(state.generation(), generation);
    assert_eq!(state.participant_mask(), 0b01);
    assert!(sink.take().is_empty(), "deferred arm must have no effects");
    // Scene resets are equally refused while the native call is unresolved.
    assert_eq!(
        state.on_transition(&sink, scene::THANK_YOU, scene::ATTRACT_DEMO),
        TransitionOutcome::XactStillInFlight
    );
    assert!(sink.take().is_empty());
}

// ── Phase entry-point legality (Task 2 contract) ─────────────────────

#[test]
fn phase_entry_points_validate_phase_and_generation() {
    let sink = RecordingSink::default();
    let (state, generation) = armed_state(&sink);
    // Illegal from Armed.
    assert_eq!(
        state.mark_exposed(generation),
        Err(PhaseError::WrongPhase(GenerationPhase::Armed))
    );
    // Stale generation.
    assert_eq!(
        state.begin_binding(generation + 1),
        Err(PhaseError::GenerationMismatch)
    );
    // Legal chain to commit (Armed → Binding → XactInFlight → Committed).
    state.begin_binding(generation).unwrap();
    assert_eq!(state.phase(), GenerationPhase::Binding);
    state.mark_exposed(generation).unwrap();
    assert_eq!(state.phase(), GenerationPhase::XactInFlight);
    state.mark_committed(generation).unwrap();
    assert_eq!(state.phase(), GenerationPhase::Committed);
    // Early-fail is illegal once the binding is in flight.
    assert_eq!(
        state.mark_early_failed(generation),
        Err(PhaseError::WrongPhase(GenerationPhase::Committed))
    );
}

#[test]
fn binding_can_early_fail_back_to_stock() {
    // The design's Binding → EarlyFailed leg: a refused preflight falls back
    // to stock 100% before any binding is published.
    let sink = RecordingSink::default();
    let (state, generation) = armed_state(&sink);
    state.begin_binding(generation).unwrap();
    state.mark_early_failed(generation).unwrap();
    assert_eq!(state.phase(), GenerationPhase::EarlyFailed);
}

#[test]
fn illegal_entry_points_from_identity_change_nothing() {
    let state = LifecycleState::new();
    assert_eq!(
        state.mark_exposed(0),
        Err(PhaseError::WrongPhase(GenerationPhase::Identity))
    );
    assert_eq!(state.mark_committed(1), Err(PhaseError::GenerationMismatch));
    assert_eq!(state.phase(), GenerationPhase::Identity);
}
