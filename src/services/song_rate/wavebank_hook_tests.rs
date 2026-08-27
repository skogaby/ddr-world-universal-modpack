use std::sync::atomic::{AtomicUsize, Ordering};

use super::wavebank_hook::{
    call_create_identity, call_unregister_identity, readiness, IdentityReadiness,
};
use super::xact_runtime::current_frame;

#[test]
fn create_calls_original_exactly_once_and_clears_tls() {
    let calls = AtomicUsize::new(0);
    let result = call_create_identity(
        44,
        |file_id| {
            assert_eq!(file_id, 44);
            assert_eq!(current_frame().unwrap().file_id, 44);
            calls.fetch_add(1, Ordering::AcqRel);
            7
        },
        || {},
    );
    assert_eq!(result, 7);
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert!(current_frame().is_none());
}

#[test]
fn nested_create_frames_remain_isolated() {
    let result = call_create_identity(
        1,
        |_| {
            let outer = current_frame().unwrap();
            let nested = call_create_identity(2, |_| current_frame().unwrap().depth, || {});
            assert_eq!(nested, 2);
            assert_eq!(current_frame().unwrap(), outer);
            1
        },
        || {},
    );
    assert_eq!(result, 1);
    assert!(current_frame().is_none());
}

#[test]
fn post_original_panic_preserves_result_and_exactly_once_count() {
    let calls = AtomicUsize::new(0);
    let result = call_create_identity(
        3,
        |_| {
            calls.fetch_add(1, Ordering::AcqRel);
            1
        },
        || panic!("injected post failure"),
    );
    assert_eq!(result, 1);
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert!(current_frame().is_none());
}

#[test]
fn unregister_calls_original_before_post_and_contains_post_panic() {
    let order = AtomicUsize::new(0);
    call_unregister_identity(
        9,
        |_| {
            assert_eq!(order.fetch_add(1, Ordering::AcqRel), 0);
        },
        || {
            assert_eq!(order.fetch_add(1, Ordering::AcqRel), 1);
            panic!("injected unregister post failure");
        },
    );
    assert_eq!(order.load(Ordering::Acquire), 2);
}

#[test]
fn readiness_requires_every_identity_prerequisite() {
    assert!(IdentityReadiness {
        clock: true,
        wavebank_create: true,
        wavebank_unregister: true,
        binding: true,
        movie_policy: true,
    }
    .is_ready());
    for missing in 0..5 {
        let mut values = [true; 5];
        values[missing] = false;
        assert!(!IdentityReadiness {
            clock: values[0],
            wavebank_create: values[1],
            wavebank_unregister: values[2],
            binding: values[3],
            movie_policy: values[4],
        }
        .is_ready());
    }
}

#[test]
fn readiness_binding_leg_tracks_the_installed_integration() {
    // Step-1 planted a structurally-false tripwire here; plan Step 4's final
    // task INVERTED it (deliberately, per the identity-base record): the
    // binding leg is no longer hardwired false — it reports the real
    // installed state of the IO-callback detour pair through
    // `binding::integration_available()`. Host-side no hooks ever install,
    // so the LINKAGE is the live assertion; the conjunction itself is ready
    // exactly when every leg is true (the all-true/each-single-false matrix
    // in `readiness_requires_every_identity_prerequisite` above).
    let live = readiness(true);
    assert_eq!(
        live.binding,
        super::binding::integration_available(),
        "the binding leg must report the installed integration, not a constant"
    );
    // With every OTHER leg forced true, readiness is exactly the binding
    // leg — the flip is the integration install, nothing else.
    let forced = IdentityReadiness {
        clock: true,
        wavebank_create: true,
        wavebank_unregister: true,
        movie_policy: true,
        ..live
    };
    assert_eq!(forced.is_ready(), super::binding::integration_available());
}

// ── The bind/unbind composition (the windows detours' host-tested core) ──

mod composition {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use super::super::binding::{
        song_code_digest, BindRefusal, BindingRegistry, PollOutcome, ServeOutcome, SourceView,
    };
    use super::super::clock_patch::{RatePublication, IDENTITY_Q31};
    use super::super::generator_tests::replay_fixture;
    use super::super::lifecycle::{
        ArmOutcome, ArmRequest, EligibilityDecision, GenerationPhase, LifecycleSink, LifecycleState,
    };
    use super::super::transaction::{
        call_create, BindOutcome, CreateOutcome, FaultSelector, SessionTaint, TransactionParts,
        MAINTENANCE_CAPACITY,
    };
    use super::super::wavebank_hook::{
        bind_for_create, retire_after_create, unregister_prelude, BindContext,
    };
    use super::super::xact_runtime::{MaintenanceKind, MaintenanceQueue, XactSlotPhase, XactSlots};
    use crate::services::score_guard::RateSaveLedger;

    const OWNER: u64 = 0x2000;
    const FILE_ID: i32 = 5;

    #[derive(Default)]
    struct TaintProbe(Mutex<Vec<usize>>);

    impl SessionTaint for TaintProbe {
        fn taint(&self, side: usize) {
            self.0.lock().unwrap().push(side);
        }
    }

    struct NullSink;
    impl LifecycleSink for NullSink {
        fn publish_identity(&self, _generation: u64, _mask: u8) {}
        fn reset_identity(&self) {}
        fn set_movie_suppressed(&self, _suppressed: bool) {}
    }

    struct Fixture {
        slots: XactSlots,
        maintenance: MaintenanceQueue<MAINTENANCE_CAPACITY>,
        publication: RatePublication,
        ledger: RateSaveLedger,
        lifecycle: LifecycleState,
        registry: BindingRegistry,
        factor: &'static AtomicU64,
        taint: TaintProbe,
    }

    impl Fixture {
        fn new() -> Self {
            let factor: &'static AtomicU64 = Box::leak(Box::new(AtomicU64::new(IDENTITY_Q31)));
            Self {
                slots: XactSlots::new(),
                maintenance: MaintenanceQueue::new(),
                publication: RatePublication::new(factor),
                ledger: RateSaveLedger::new(),
                lifecycle: LifecycleState::new(),
                registry: BindingRegistry::new(),
                factor,
                taint: TaintProbe::default(),
            }
        }

        fn arm(&self, percent: i32) -> u64 {
            let outcome = self.lifecycle.on_scene26(
                &NullSink,
                EligibilityDecision::Arm(ArmRequest {
                    requested_percent: percent,
                    preserve_pitch: true,
                    sync_movie: false,
                    participant_mask: 0b01,
                    stage_index: 0,
                }),
            );
            let ArmOutcome::Armed { generation } = outcome else {
                panic!("arm failed: {outcome:?}");
            };
            generation
        }

        fn context(&self) -> BindContext<'_> {
            BindContext {
                lifecycle: &self.lifecycle,
                slots: &self.slots,
                registry: &self.registry,
                publication: &self.publication,
                fault: FaultSelector::default(),
                owner_thread: OWNER,
                initial_mapping_ms: (0, 0),
            }
        }

        fn parts<'a>(&'a self, confirm_movie: &'a (dyn Fn() + Sync)) -> TransactionParts<'a> {
            TransactionParts {
                slots: &self.slots,
                maintenance: &self.maintenance,
                publication: &self.publication,
                ledger: &self.ledger,
                lifecycle: &self.lifecycle,
                confirm_movie,
                taint_session: &self.taint,
                fault: FaultSelector::default(),
            }
        }

        /// One full create through the REAL bind closure: the exact shape
        /// the windows `create_hook` composes.
        fn run_create(
            &self,
            song_code: Option<&str>,
            source: Option<&[u8]>,
            original_result: u8,
        ) -> (u8, CreateOutcome, BindOutcome) {
            let confirm = || {};
            let parts = self.parts(&confirm);
            let bind_outcome = std::cell::Cell::new(BindOutcome::Stock);
            let (result, outcome) = call_create(
                &parts,
                FILE_ID,
                OWNER,
                |id| {
                    let view = source.map(SourceView::new);
                    let outcome = bind_for_create(&self.context(), id, song_code, view.as_ref());
                    bind_outcome.set(outcome);
                    outcome
                },
                |_| original_result,
            );
            let bound = bind_outcome.get() == BindOutcome::Bound;
            let _ = retire_after_create(&self.registry, outcome, bound, FILE_ID);
            (result, outcome, bind_outcome.get())
        }

        /// Simulate the maintenance drain until the retired list is empty
        /// (slot recycling is exercised separately).
        fn drain_reclaim(&self) {
            let deadline = Instant::now() + Duration::from_secs(10);
            while self.registry.retired_count() > 0 {
                assert!(Instant::now() < deadline, "reclaim never became eligible");
                self.registry.sweep(|_, _| {});
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }

    #[test]
    fn refusal_lands_early_failed_with_a_stock_create_and_one_drain_note() {
        let fixture = Fixture::new();
        fixture.arm(50);
        let junk = vec![0u8; 64];
        let (result, outcome, bind) = fixture.run_create(Some("tst1"), Some(&junk), 1);
        // The original ran (once, unbound) and its result stands.
        assert_eq!((result, outcome), (1, CreateOutcome::Stock));
        assert_eq!(bind, BindOutcome::Refused);
        assert_eq!(fixture.lifecycle.phase(), GenerationPhase::EarlyFailed);
        // No binding, no token, clock identity.
        assert_eq!(fixture.registry.active_generation(), None);
        assert_eq!(fixture.registry.retired_count(), 0);
        assert_eq!(fixture.factor.load(Ordering::Acquire), IDENTITY_Q31);
        assert!(!fixture.publication.read().committed);
        // Exactly one refusal note for the drain's bounded WARN.
        let (refusal, file_id, count) = fixture.registry.take_refusal().expect("note");
        assert_eq!(refusal, BindRefusal::UnsupportedProfile);
        assert_eq!((file_id, count), (FILE_ID, 1));
    }

    #[test]
    fn missing_source_is_a_source_read_refusal() {
        let fixture = Fixture::new();
        fixture.arm(50);
        let (result, outcome, bind) = fixture.run_create(Some("tst1"), None, 1);
        assert_eq!((result, outcome), (1, CreateOutcome::Stock));
        assert_eq!(bind, BindOutcome::Refused);
        assert_eq!(fixture.lifecycle.phase(), GenerationPhase::EarlyFailed);
        let (refusal, ..) = fixture.registry.take_refusal().expect("note");
        assert_eq!(refusal, BindRefusal::SourceRead);
    }

    #[test]
    fn non_dance_create_while_armed_declines_silently() {
        let fixture = Fixture::new();
        fixture.arm(50);
        let fixture_bytes = replay_fixture(false);
        let (result, outcome, bind) = fixture.run_create(None, Some(&fixture_bytes), 1);
        assert_eq!((result, outcome), (1, CreateOutcome::Stock));
        assert_eq!(bind, BindOutcome::Stock);
        // NOT EarlyFailed: the dance bank arrives later in the same load.
        assert_eq!(fixture.lifecycle.phase(), GenerationPhase::Armed);
        assert_eq!(fixture.registry.take_refusal(), None);
    }

    #[test]
    fn bind_then_success_commits_with_q31_last() {
        let fixture = Fixture::new();
        let generation = fixture.arm(50);
        let source = replay_fixture(false);
        // Ordering probe: at movie-confirm time the score is written but
        // the snapshot and factor are still identity (Q31 strictly last).
        let probe: Mutex<Vec<(usize, bool, u64)>> = Mutex::new(Vec::new());
        let confirm = || {
            probe.lock().unwrap().push((
                fixture.ledger.pending_count(0),
                fixture.publication.read().committed,
                fixture.factor.load(Ordering::Acquire),
            ));
        };
        let parts = fixture.parts(&confirm);
        let bound = std::cell::Cell::new(false);
        let (result, outcome) = call_create(
            &parts,
            FILE_ID,
            OWNER,
            |id| {
                let view = SourceView::new(&source);
                let outcome = bind_for_create(&fixture.context(), id, Some("tst1"), Some(&view));
                bound.set(outcome == BindOutcome::Bound);
                outcome
            },
            |_| 1,
        );
        assert_eq!(result, 1);
        assert_eq!(outcome, CreateOutcome::Committed { generation });
        assert!(bound.get());
        assert_eq!(*probe.lock().unwrap(), vec![(1, false, IDENTITY_Q31)]);
        // Committed: snapshot live, factor is the plan's exact Q31, the
        // binding is the active one, the slot is Committed, the song is
        // bound to the generation.
        let snapshot = fixture.publication.read();
        assert!(snapshot.committed);
        assert_eq!(snapshot.generation, generation);
        assert_eq!(snapshot.requested_percent, 50);
        let expected_q31 = u64::try_from(snapshot.effective_rate.q31().unwrap()).unwrap();
        assert_eq!(fixture.factor.load(Ordering::Acquire), expected_q31);
        assert_ne!(fixture.factor.load(Ordering::Acquire), IDENTITY_Q31);
        assert_eq!(fixture.lifecycle.phase(), GenerationPhase::Committed);
        assert_eq!(
            fixture.lifecycle.bound_song(),
            Some(song_code_digest("tst1"))
        );
        assert_eq!(fixture.registry.active_generation(), Some(generation));
        assert_eq!(fixture.slots.phase(0), Some(XactSlotPhase::Committed));
        assert_eq!(fixture.ledger.pending_count(0), 1);
        // Cleanup: stop the producer.
        assert!(fixture.registry.retire_by_file(FILE_ID));
    }

    /// The training identity arm's full composition (training design §4.5):
    /// arm at 100% → bind → commit. The commit must carry NO score ledger
    /// entry, NO session taint, and NO movie confirmation — arming alone is
    /// not an alteration (the served audio is byte-identical) — and the
    /// snapshot must read as identity to every consumer.
    #[test]
    fn training_identity_arm_commits_without_taint_ledger_or_movie() {
        let fixture = Fixture::new();
        let generation = fixture.arm(100);
        let source = replay_fixture(false);
        let movie_confirms = std::sync::atomic::AtomicUsize::new(0);
        let confirm = || {
            movie_confirms.fetch_add(1, Ordering::AcqRel);
        };
        let parts = fixture.parts(&confirm);
        let bound = std::cell::Cell::new(false);
        let (result, outcome) = call_create(
            &parts,
            FILE_ID,
            OWNER,
            |id| {
                let view = SourceView::new(&source);
                let outcome = bind_for_create(&fixture.context(), id, Some("tst1"), Some(&view));
                bound.set(outcome == BindOutcome::Bound);
                outcome
            },
            |_| 1,
        );
        assert_eq!(result, 1);
        assert_eq!(outcome, CreateOutcome::Committed { generation });
        assert!(bound.get());
        // Score containment: NOTHING is tainted or pended by the arm alone.
        assert_eq!(fixture.ledger.pending_count(0), 0);
        assert_eq!(fixture.ledger.pending_count(1), 0);
        assert!(fixture.taint.0.lock().unwrap().is_empty());
        // No movie suppression confirmation for identity arms.
        assert_eq!(movie_confirms.load(Ordering::Acquire), 0);
        // The snapshot commits at 100% and reads as identity to every
        // consumer (tick_domain, real_speed, the rate ledger).
        let snapshot = fixture.publication.read();
        assert!(snapshot.committed);
        assert_eq!(snapshot.requested_percent, 100);
        assert!(!snapshot.is_non_identity_commit());
        assert_eq!(fixture.factor.load(Ordering::Acquire), IDENTITY_Q31);
        assert_eq!(fixture.lifecycle.phase(), GenerationPhase::Committed);
        // The live binding is the identity passthrough.
        let serve_mode = fixture
            .registry
            .with_active(|binding| binding.serve_mode())
            .expect("binding is live");
        assert_eq!(
            serve_mode,
            super::super::binding::ServeMode::IdentityPassthrough
        );
        assert_eq!(fixture.registry.active_generation(), Some(generation));
        assert!(fixture.registry.retire_by_file(FILE_ID));
    }

    /// The bind-time pre-shift (R15/training design §4.5): an initial
    /// mapping carried on the bind context lands on the binding BEFORE
    /// publication — the first byte the engine ever reads is already
    /// shifted (a post-publication call would lose the race against bank
    /// prepare's buffering reads). The ms→block conversion floors onto the
    /// main entry's block grid: at 8 kHz / 128-sample blocks, 1000 ms →
    /// 62 blocks and 500 ms → 31 blocks.
    #[test]
    fn bind_time_initial_mapping_lands_before_publication() {
        let fixture = Fixture::new();
        fixture.arm(100);
        let source = replay_fixture(false);
        let confirm = || {};
        let parts = fixture.parts(&confirm);
        let (result, outcome) = call_create(
            &parts,
            FILE_ID,
            OWNER,
            |id| {
                let view = SourceView::new(&source);
                let mut context = fixture.context();
                context.initial_mapping_ms = (1_000, 500);
                bind_for_create(&context, id, Some("tst1"), Some(&view))
            },
            |_| 1,
        );
        assert_eq!(result, 1);
        assert!(matches!(outcome, CreateOutcome::Committed { .. }));
        let mapping = fixture
            .registry
            .with_active(|binding| binding.content_mapping())
            .expect("binding is live");
        assert_eq!(mapping, (62, 31));
        assert!(fixture.registry.retire_by_file(FILE_ID));
    }

    #[test]
    fn create_failure_after_bind_retires_the_binding_and_never_publishes_q31() {
        let fixture = Fixture::new();
        let generation = fixture.arm(50);
        let source = replay_fixture(false);
        let (result, outcome, bind) = fixture.run_create(Some("tst1"), Some(&source), 0);
        assert_eq!(result, 0);
        assert_eq!(
            outcome,
            CreateOutcome::LateFailed {
                generation,
                enqueued: true
            }
        );
        assert_eq!(bind, BindOutcome::Bound);
        // The binding this call published was retired post-create.
        assert_eq!(fixture.registry.active_generation(), None);
        assert_eq!(fixture.registry.retired_count(), 1);
        // Q31 never published; slot quarantined with its reclaim record.
        assert_eq!(fixture.factor.load(Ordering::Acquire), IDENTITY_Q31);
        assert!(!fixture.publication.read().committed);
        assert_eq!(fixture.lifecycle.phase(), GenerationPhase::LateFailed);
        assert_eq!(fixture.slots.phase(0), Some(XactSlotPhase::Quarantined));
        let event = fixture.maintenance.pop().expect("reclaim event");
        assert_eq!(event.kind, MaintenanceKind::ReclaimBinding);
        // Reclamation still honors quiescence.
        fixture.drain_reclaim();
    }

    #[test]
    fn unregister_prelude_cancels_pending_reads_and_releases_the_slot() {
        let fixture = Fixture::new();
        fixture.arm(50);
        let source = replay_fixture(false);
        let (result, _, bind) = fixture.run_create(Some("tst1"), Some(&source), 1);
        assert_eq!(result, 1);
        assert_eq!(bind, BindOutcome::Bound);

        // Arm a read just below the virtual EOF: almost certainly not yet
        // produced, so it defers into a pending slot.
        let virtual_size = fixture
            .registry
            .with_active(|binding| binding.layout().virtual_size)
            .expect("active binding");
        let mut buffer = vec![0u8; 64];
        let mut accumulator = 0u64;
        let served = fixture
            .registry
            .with_active(|binding| unsafe {
                binding.serve(virtual_size - 64, 64, buffer.as_mut_ptr(), &mut accumulator)
            })
            .expect("active binding");

        assert!(unregister_prelude(
            &fixture.registry,
            &fixture.slots,
            &fixture.maintenance,
            FILE_ID
        ));
        // The committed slot moved to ReleasePending with its reclaim
        // record; the binding is retired and off the active slot.
        assert_eq!(fixture.slots.phase(0), Some(XactSlotPhase::ReleasePending));
        let event = fixture.maintenance.pop().expect("reclaim event");
        assert_eq!(event.kind, MaintenanceKind::ReclaimBinding);
        assert_eq!(event.slot_index, 0);
        assert_eq!(fixture.registry.active_generation(), None);

        if matches!(served, ServeOutcome::Pending) {
            // The retire cancelled the armed read with clamp semantics:
            // the poll completes (0-byte completion permitted), no hang.
            let completed = fixture
                .registry
                .with_retired(FILE_ID, |binding| {
                    matches!(
                        unsafe { binding.poll(&mut accumulator) },
                        PollOutcome::Complete(_)
                    )
                })
                .unwrap_or(false);
            assert!(completed, "cancelled read must complete");
        }
        // Reclamation happens only at reader quiescence (+ cooldown).
        fixture.drain_reclaim();
        // A second prelude is a no-op (nothing bound, slot already gone).
        assert!(!unregister_prelude(
            &fixture.registry,
            &fixture.slots,
            &fixture.maintenance,
            FILE_ID
        ));
    }

    #[test]
    fn quick_restart_re_binds_the_same_generation_from_offset_zero() {
        let fixture = Fixture::new();
        let generation = fixture.arm(50);
        let source = replay_fixture(false);
        let (result, outcome, _) = fixture.run_create(Some("tst1"), Some(&source), 1);
        assert_eq!(result, 1);
        assert_eq!(outcome, CreateOutcome::Committed { generation });

        // Song unload: unregister prelude + the drain's slot/buffer work.
        assert!(unregister_prelude(
            &fixture.registry,
            &fixture.slots,
            &fixture.maintenance,
            FILE_ID
        ));
        let event = fixture.maintenance.pop().expect("reclaim event");
        fixture
            .slots
            .finish_release(usize::from(event.slot_index))
            .expect("slot release");
        fixture.drain_reclaim();

        // Quick Restart: the game re-creates the SAME song's bank while the
        // generation is Committed — mark_reexposed path, fresh binding.
        assert_eq!(fixture.lifecycle.phase(), GenerationPhase::Committed);
        let (result, outcome, bind) = fixture.run_create(Some("tst1"), Some(&source), 1);
        assert_eq!(result, 1);
        assert_eq!(outcome, CreateOutcome::Committed { generation });
        assert_eq!(bind, BindOutcome::Bound);
        // Same generation identity, regeneration from offset zero: the new
        // producer serves the header read (offset 0) again.
        assert_eq!(fixture.registry.active_generation(), Some(generation));
        let mut buffer = vec![0u8; 0x1000];
        let mut accumulator = 0u64;
        let outcome = fixture
            .registry
            .with_active(|binding| unsafe {
                binding.serve(0, 0x1000, buffer.as_mut_ptr(), &mut accumulator)
            })
            .expect("active binding");
        match outcome {
            ServeOutcome::Served(bytes) => assert_eq!(bytes, 0x1000),
            ServeOutcome::Pending => {
                let deadline = Instant::now() + Duration::from_secs(10);
                loop {
                    match fixture
                        .registry
                        .with_active(|binding| unsafe { binding.poll(&mut accumulator) })
                        .expect("active binding")
                    {
                        PollOutcome::Complete(bytes) => {
                            assert_eq!(bytes, 0x1000);
                            break;
                        }
                        _ => assert!(Instant::now() < deadline, "header read never completed"),
                    }
                }
            }
            ServeOutcome::Refused => panic!("re-bound binding refused the header read"),
        }
        // Taint/ledger idempotence: still exactly one pending append.
        assert_eq!(fixture.ledger.pending_count(0), 1);
        assert!(fixture.registry.retire_by_file(FILE_ID));
    }

    #[test]
    fn quick_restart_refusal_keeps_committed_and_resets_the_clock() {
        let fixture = Fixture::new();
        let generation = fixture.arm(50);
        let source = replay_fixture(false);
        let (result, outcome, _) = fixture.run_create(Some("tst1"), Some(&source), 1);
        assert_eq!(result, 1);
        assert_eq!(outcome, CreateOutcome::Committed { generation });
        assert_ne!(fixture.factor.load(Ordering::Acquire), IDENTITY_Q31);

        assert!(unregister_prelude(
            &fixture.registry,
            &fixture.slots,
            &fixture.maintenance,
            FILE_ID
        ));
        let event = fixture.maintenance.pop().expect("reclaim event");
        fixture
            .slots
            .finish_release(usize::from(event.slot_index))
            .expect("slot release");
        fixture.drain_reclaim();

        // The QR re-create's preflight refuses (unreadable source): the
        // generation stays Committed (the gameplay-exit boundary owns its
        // completion) but the committed Q31 must not survive against the
        // stock audio the re-created bank now carries.
        let junk = vec![0u8; 64];
        let (result, outcome, bind) = fixture.run_create(Some("tst1"), Some(&junk), 1);
        assert_eq!((result, outcome), (1, CreateOutcome::Stock));
        assert_eq!(bind, BindOutcome::Refused);
        assert_eq!(fixture.lifecycle.phase(), GenerationPhase::Committed);
        assert_eq!(fixture.factor.load(Ordering::Acquire), IDENTITY_Q31);
        assert!(!fixture.publication.read().committed);
        assert_eq!(fixture.registry.active_generation(), None);
        let (refusal, ..) = fixture.registry.take_refusal().expect("note");
        assert_eq!(refusal, BindRefusal::UnsupportedProfile);
    }
}
