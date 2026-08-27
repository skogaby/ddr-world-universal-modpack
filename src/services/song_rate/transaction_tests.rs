//! Host tests for the exactly-once wave-bank transaction: commit ordering
//! (score → movie → snapshot → Q31-last), late-failure quarantine, exact
//! token recovery, panic containment, and fault injection.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

use super::clock_patch::{RatePublication, IDENTITY_Q31};
use super::lifecycle::{
    ArmOutcome, ArmRequest, EligibilityDecision, GenerationPhase, LifecycleSink, LifecycleState,
};
use super::transaction::{
    call_create, BindOutcome, CreateOutcome, FaultSelector, SessionTaint, TransactionParts,
};
use super::xact_runtime::{
    attach_slot_to_current, current_frame, MaintenanceKind, MaintenanceQueue, RedirectToken,
    XactSlotPhase, XactSlots,
};
use crate::core::xact::rate::RateRatio;
use crate::services::score_guard::RateSaveLedger;

/// Records tainted sides without touching process globals (parallel-safe).
#[derive(Default)]
struct TaintProbe(Mutex<Vec<usize>>);

impl TaintProbe {
    fn sides(&self) -> Vec<usize> {
        self.0.lock().unwrap().clone()
    }
}

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
    maintenance: MaintenanceQueue<{ super::transaction::MAINTENANCE_CAPACITY }>,
    publication: RatePublication,
    ledger: RateSaveLedger,
    lifecycle: LifecycleState,
    factor: &'static AtomicU64,
    movie_probe: Mutex<Vec<(usize, bool, u64)>>,
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
            factor,
            movie_probe: Mutex::new(Vec::new()),
            taint: TaintProbe::default(),
        }
    }

    /// Arm and walk the lifecycle to Binding so an in-original expose is
    /// legal; returns the generation.
    fn arm_to_binding(&self) -> u64 {
        let outcome = self.lifecycle.on_scene26(
            &NullSink,
            EligibilityDecision::Arm(ArmRequest {
                requested_percent: 75,
                preserve_pitch: true,
                sync_movie: false,
                participant_mask: 0b01,
                stage_index: 0,
            }),
        );
        let ArmOutcome::Armed { generation } = outcome else {
            panic!("arm failed: {outcome:?}");
        };
        self.lifecycle.begin_binding(generation).unwrap();
        generation
    }

    fn rate() -> RateRatio {
        RateRatio::new(3, 4).unwrap()
    }

    fn token(&self, generation: u64, nonce: u64, depth: u8) -> RedirectToken {
        RedirectToken {
            call_nonce: nonce,
            call_depth: depth,
            generation,
            requested_percent: 75,
            participant_mask: 0b01,
            stage_index: 0,
            effective_rate: Self::rate(),
        }
    }

    /// Expose inside the current TLS frame (mimics the in-create binding):
    /// claim a slot with the live frame identity, expose the token, attach.
    fn expose_in_frame(&self, owner: u64, generation: u64) {
        let frame = current_frame().expect("frame active");
        let slot = self
            .slots
            .claim(owner, frame.nonce, frame.depth, frame.file_id)
            .expect("free slot");
        self.slots
            .expose(
                slot,
                owner,
                frame.nonce,
                frame.depth,
                frame.file_id,
                self.token(generation, frame.nonce, frame.depth),
            )
            .unwrap();
        attach_slot_to_current(frame.nonce, slot).unwrap();
        self.lifecycle.mark_exposed(generation).unwrap();
    }

    fn parts<'a>(
        &'a self,
        confirm_movie: &'a (dyn Fn() + Sync),
        fault: FaultSelector,
    ) -> TransactionParts<'a> {
        TransactionParts {
            slots: &self.slots,
            maintenance: &self.maintenance,
            publication: &self.publication,
            ledger: &self.ledger,
            lifecycle: &self.lifecycle,
            confirm_movie,
            taint_session: &self.taint,
            fault,
        }
    }
}

const OWNER: u64 = 0x1000;

fn expected_q31() -> u64 {
    u64::try_from(Fixture::rate().q31().unwrap()).unwrap()
}

#[test]
fn commit_applies_score_movie_snapshot_then_q31_in_order() {
    let fixture = Fixture::new();
    let generation = fixture.arm_to_binding();
    let calls = AtomicUsize::new(0);
    // The movie confirm is the mid-point probe: score state must already be
    // written, the snapshot must NOT yet be committed, and the factor must
    // still be identity when it runs.
    let probe = |ledger_pending: usize, committed: bool, factor: u64| {
        fixture
            .movie_probe
            .lock()
            .unwrap()
            .push((ledger_pending, committed, factor));
    };
    let confirm = || {
        probe(
            fixture.ledger.pending_count(0),
            fixture.publication.read().committed,
            fixture.factor.load(Ordering::Acquire),
        );
    };
    let parts = fixture.parts(&confirm, FaultSelector::default());
    let (result, outcome) = call_create(
        &parts,
        42,
        OWNER,
        |_| BindOutcome::Stock,
        |id| {
            calls.fetch_add(1, Ordering::AcqRel);
            fixture.expose_in_frame(OWNER, generation);
            assert_eq!(id, 42);
            1
        },
    );
    assert_eq!(result, 1);
    assert_eq!(outcome, CreateOutcome::Committed { generation });
    assert_eq!(calls.load(Ordering::Acquire), 1);
    // Ordering probe: at movie-confirm time score was written, snapshot and
    // factor were not.
    assert_eq!(
        *fixture.movie_probe.lock().unwrap(),
        vec![(1, false, IDENTITY_Q31)]
    );
    // Final state: snapshot committed with the exact rate, factor Q31 last.
    let snapshot = fixture.publication.read();
    assert!(snapshot.committed);
    assert_eq!(snapshot.generation, generation);
    assert_eq!(snapshot.effective_rate, Fixture::rate());
    assert_eq!(fixture.factor.load(Ordering::Acquire), expected_q31());
    assert_eq!(fixture.lifecycle.phase(), GenerationPhase::Committed);
    assert_eq!(fixture.slots.phase(0), Some(XactSlotPhase::Committed));
    assert_eq!(fixture.taint.sides(), vec![0]);
}

#[test]
fn commit_with_sync_movie_latched_skips_the_movie_confirm() {
    // Background-movie-sync design §4: a generation that latched SYNC
    // BACKGROUND VIDEO (effective — the arm's platform gate already
    // applied) commits WITHOUT re-asserting the movie suppression; score
    // protection, the snapshot, and the Q31-last ordering land in full
    // regardless (a rate-played song is score-contained whether its movie
    // plays or not).
    let fixture = Fixture::new();
    let outcome = fixture.lifecycle.on_scene26(
        &NullSink,
        EligibilityDecision::Arm(ArmRequest {
            requested_percent: 75,
            preserve_pitch: true,
            sync_movie: true,
            participant_mask: 0b01,
            stage_index: 0,
        }),
    );
    let ArmOutcome::Armed { generation } = outcome else {
        panic!("arm failed: {outcome:?}");
    };
    assert!(fixture.lifecycle.sync_movie());
    fixture.lifecycle.begin_binding(generation).unwrap();
    let confirm = || panic!("movie confirm must not run when sync_movie is latched");
    let parts = fixture.parts(&confirm, FaultSelector::default());
    let (result, outcome) = call_create(
        &parts,
        42,
        OWNER,
        |_| BindOutcome::Stock,
        |_| {
            fixture.expose_in_frame(OWNER, generation);
            1
        },
    );
    assert_eq!(result, 1);
    assert_eq!(outcome, CreateOutcome::Committed { generation });
    assert_eq!(fixture.ledger.pending_count(0), 1);
    assert_eq!(fixture.taint.sides(), vec![0]);
    let snapshot = fixture.publication.read();
    assert!(snapshot.committed);
    assert_eq!(snapshot.generation, generation);
    assert_eq!(fixture.factor.load(Ordering::Acquire), expected_q31());
    assert_eq!(fixture.lifecycle.phase(), GenerationPhase::Committed);
}

#[test]
fn unrelated_bank_commits_nothing_and_calls_original_once() {
    let fixture = Fixture::new();
    let calls = AtomicUsize::new(0);
    let confirm = || panic!("movie confirm must not run for a stock bank");
    let parts = fixture.parts(&confirm, FaultSelector::default());
    let (result, outcome) = call_create(
        &parts,
        9,
        OWNER,
        |_| BindOutcome::Stock,
        |_| {
            calls.fetch_add(1, Ordering::AcqRel);
            1
        },
    );
    assert_eq!((result, outcome), (1, CreateOutcome::Stock));
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert!(!fixture.publication.read().committed);
    assert_eq!(fixture.factor.load(Ordering::Acquire), IDENTITY_Q31);
    assert_eq!(fixture.ledger.pending_count(0), 0);
}

#[test]
fn pre_original_panic_still_calls_original_exactly_once_without_redirect() {
    let fixture = Fixture::new();
    let calls = AtomicUsize::new(0);
    let confirm = || {};
    let parts = fixture.parts(
        &confirm,
        FaultSelector {
            pre_original_panic: true,
            ..FaultSelector::default()
        },
    );
    let (result, outcome) = call_create(
        &parts,
        9,
        OWNER,
        |_| BindOutcome::Stock,
        |_| {
            calls.fetch_add(1, Ordering::AcqRel);
            // The frame was cleared by the pre-original panic containment: no
            // conversion could attach anything.
            assert!(current_frame().is_none());
            1
        },
    );
    assert_eq!((result, outcome), (1, CreateOutcome::Stock));
    assert_eq!(calls.load(Ordering::Acquire), 1);
}

#[test]
fn post_original_panic_is_contained_and_commit_still_lands() {
    let fixture = Fixture::new();
    let generation = fixture.arm_to_binding();
    let calls = AtomicUsize::new(0);
    let confirm = || {};
    let parts = fixture.parts(
        &confirm,
        FaultSelector {
            post_original_panic: true,
            ..FaultSelector::default()
        },
    );
    let (result, outcome) = call_create(
        &parts,
        42,
        OWNER,
        |_| BindOutcome::Stock,
        |_| {
            calls.fetch_add(1, Ordering::AcqRel);
            fixture.expose_in_frame(OWNER, generation);
            1
        },
    );
    assert_eq!(calls.load(Ordering::Acquire), 1);
    // The fallback containment pass consumed the exposed slot exactly once.
    assert_eq!(result, 1);
    assert_eq!(outcome, CreateOutcome::Committed { generation });
    assert_eq!(fixture.slots.phase(0), Some(XactSlotPhase::Committed));
    assert!(fixture.publication.read().committed);
}

#[test]
fn late_failure_quarantines_without_score_or_clock_writes() {
    let fixture = Fixture::new();
    let generation = fixture.arm_to_binding();
    let confirm = || panic!("movie confirm must not run on late failure");
    let parts = fixture.parts(&confirm, FaultSelector::default());
    let (result, outcome) = call_create(
        &parts,
        42,
        OWNER,
        |_| BindOutcome::Stock,
        |_| {
            fixture.expose_in_frame(OWNER, generation);
            0 // XACT rejected the generated bank.
        },
    );
    assert_eq!(result, 0);
    assert_eq!(
        outcome,
        CreateOutcome::LateFailed {
            generation,
            enqueued: true
        }
    );
    assert_eq!(fixture.slots.phase(0), Some(XactSlotPhase::Quarantined));
    assert_eq!(fixture.lifecycle.phase(), GenerationPhase::LateFailed);
    assert_eq!(fixture.factor.load(Ordering::Acquire), IDENTITY_Q31);
    assert!(!fixture.publication.read().committed);
    assert_eq!(fixture.ledger.pending_count(0), 0);
    // Exactly one maintenance record for the quarantined slot.
    let event = fixture.maintenance.pop().expect("reclaim event");
    assert_eq!(event.kind, MaintenanceKind::ReclaimBinding);
    assert_eq!(event.slot_index, 0);
    assert!(fixture.maintenance.pop().is_none());
}

#[test]
fn xact_reject_fault_forces_the_late_failure_leg() {
    let fixture = Fixture::new();
    let generation = fixture.arm_to_binding();
    let confirm = || {};
    let parts = fixture.parts(
        &confirm,
        FaultSelector {
            xact_reject: true,
            ..FaultSelector::default()
        },
    );
    let (result, outcome) = call_create(
        &parts,
        42,
        OWNER,
        |_| BindOutcome::Stock,
        |_| {
            fixture.expose_in_frame(OWNER, generation);
            1 // The original succeeded, but the injected fault rejects it.
        },
    );
    assert_eq!(result, 0);
    assert!(matches!(outcome, CreateOutcome::LateFailed { .. }));
    assert_eq!(fixture.slots.phase(0), Some(XactSlotPhase::Quarantined));
}

#[test]
fn token_mismatch_after_exposure_fails_closed_with_quarantine() {
    let fixture = Fixture::new();
    let generation = fixture.arm_to_binding();
    let confirm = || panic!("movie confirm must not run on recovery failure");
    let parts = fixture.parts(
        &confirm,
        FaultSelector {
            token_mismatch: true,
            ..FaultSelector::default()
        },
    );
    let (result, outcome) = call_create(
        &parts,
        42,
        OWNER,
        |_| BindOutcome::Stock,
        |_| {
            fixture.expose_in_frame(OWNER, generation);
            1
        },
    );
    // Exposure was known but no exact record matched: the return is forced
    // to failure, every candidate is quarantined, both sides are tainted
    // conservatively, and the clock stays identity.
    assert_eq!(result, 0);
    assert_eq!(outcome, CreateOutcome::RecoveryFailed);
    assert_eq!(fixture.slots.phase(0), Some(XactSlotPhase::Quarantined));
    assert_eq!(fixture.factor.load(Ordering::Acquire), IDENTITY_Q31);
    assert_eq!(fixture.taint.sides(), vec![0, 1]);
}

#[test]
fn full_maintenance_queue_leaves_the_slot_pinned() {
    let fixture = Fixture::new();
    let generation = fixture.arm_to_binding();
    let confirm = || {};
    let parts = fixture.parts(
        &confirm,
        FaultSelector {
            maintenance_saturation: true,
            ..FaultSelector::default()
        },
    );
    let (result, outcome) = call_create(
        &parts,
        42,
        OWNER,
        |_| BindOutcome::Stock,
        |_| {
            fixture.expose_in_frame(OWNER, generation);
            0
        },
    );
    assert_eq!(result, 0);
    assert_eq!(
        outcome,
        CreateOutcome::LateFailed {
            generation,
            enqueued: false
        }
    );
    // The slot stays pinned: quarantined phase, no event.
    assert_eq!(fixture.slots.phase(0), Some(XactSlotPhase::Quarantined));
    assert!(fixture.maintenance.pop().is_none());
}

#[test]
fn nested_unrelated_call_uses_its_own_frame() {
    let fixture = Fixture::new();
    let generation = fixture.arm_to_binding();
    let outer_calls = AtomicUsize::new(0);
    let inner_calls = AtomicUsize::new(0);
    let confirm = || {};
    let parts = fixture.parts(&confirm, FaultSelector::default());
    let (result, outcome) = call_create(
        &parts,
        42,
        OWNER,
        |_| BindOutcome::Stock,
        |_| {
            outer_calls.fetch_add(1, Ordering::AcqRel);
            // A nested create for an unrelated bank (reused worker thread).
            let inner_parts = fixture.parts(&confirm, FaultSelector::default());
            let (inner_result, inner_outcome) = call_create(
                &inner_parts,
                77,
                OWNER,
                |_| BindOutcome::Stock,
                |_| {
                    inner_calls.fetch_add(1, Ordering::AcqRel);
                    1
                },
            );
            assert_eq!((inner_result, inner_outcome), (1, CreateOutcome::Stock));
            // The outer frame is intact after the nested call.
            fixture.expose_in_frame(OWNER, generation);
            1
        },
    );
    assert_eq!(result, 1);
    assert_eq!(outcome, CreateOutcome::Committed { generation });
    assert_eq!(outer_calls.load(Ordering::Acquire), 1);
    assert_eq!(inner_calls.load(Ordering::Acquire), 1);
}

#[test]
fn deferred_reset_wins_over_a_racing_commit() {
    let fixture = Fixture::new();
    let generation = fixture.arm_to_binding();
    // Simulate a definitive reset racing the in-flight XACT call: the reset
    // lost the seqlock (writer held) and deferred.
    {
        let guard = fixture
            .publication
            .begin_identity_write_for_test()
            .expect("writer");
        assert_eq!(
            fixture.publication.reset_identity(),
            super::clock_patch::ResetOutcome::Deferred
        );
        drop(guard); // Abandoned writer: fields identity, RESET_PENDING stays.
    }
    let confirm = || {};
    let parts = fixture.parts(&confirm, FaultSelector::default());
    let (result, outcome) = call_create(
        &parts,
        42,
        OWNER,
        |_| BindOutcome::Stock,
        |_| {
            fixture.expose_in_frame(OWNER, generation);
            1
        },
    );
    assert_eq!(result, 1);
    assert!(matches!(outcome, CreateOutcome::Committed { .. }));
    // The commit performed its safety publication, then the pending identity
    // reset was applied: the factor never survives non-identity.
    assert_eq!(fixture.factor.load(Ordering::Acquire), IDENTITY_Q31);
    assert!(!fixture.publication.read().committed);
}

#[test]
fn recommit_of_the_same_generation_is_idempotent() {
    let fixture = Fixture::new();
    let generation = fixture.arm_to_binding();
    let confirm = || {};
    let parts = fixture.parts(&confirm, FaultSelector::default());
    let (result, _) = call_create(
        &parts,
        42,
        OWNER,
        |_| BindOutcome::Stock,
        |_| {
            fixture.expose_in_frame(OWNER, generation);
            1
        },
    );
    assert_eq!(result, 1);
    // Unload releases the slot (unregister leg).
    let index = fixture.slots.begin_release_by_file(42).unwrap();
    fixture.slots.finish_release(index).unwrap();
    // Reload: re-expose the SAME generation and recommit.
    fixture.lifecycle.mark_reexposed(generation).unwrap();
    let (result, outcome) = call_create(
        &parts,
        42,
        OWNER,
        |_| BindOutcome::Stock,
        |_| {
            let frame = current_frame().expect("frame");
            let slot = fixture
                .slots
                .claim(OWNER, frame.nonce, frame.depth, frame.file_id)
                .unwrap();
            fixture
                .slots
                .expose(
                    slot,
                    OWNER,
                    frame.nonce,
                    frame.depth,
                    frame.file_id,
                    fixture.token(generation, frame.nonce, frame.depth),
                )
                .unwrap();
            attach_slot_to_current(frame.nonce, slot).unwrap();
            1
        },
    );
    assert_eq!(result, 1);
    assert_eq!(outcome, CreateOutcome::Committed { generation });
    // Exactly one pending ledger entry across both commits.
    assert_eq!(fixture.ledger.pending_count(0), 1);
    let snapshot = fixture.publication.read();
    assert!(snapshot.committed);
    assert_eq!(snapshot.generation, generation);
    assert_eq!(fixture.factor.load(Ordering::Acquire), expected_q31());
}

#[test]
fn bind_runs_pre_original_exactly_once() {
    let fixture = Fixture::new();
    let order = Mutex::new(Vec::new());
    let confirm = || {};
    let parts = fixture.parts(&confirm, FaultSelector::default());
    let (result, outcome) = call_create(
        &parts,
        42,
        OWNER,
        |file_id| {
            assert_eq!(file_id, 42);
            // The TLS frame is open when the bind runs (the closure can
            // claim/expose against it).
            assert!(current_frame().is_some());
            order.lock().unwrap().push("bind");
            BindOutcome::Stock
        },
        |_| {
            order.lock().unwrap().push("original");
            1
        },
    );
    assert_eq!((result, outcome), (1, CreateOutcome::Stock));
    assert_eq!(*order.lock().unwrap(), vec!["bind", "original"]);
}

#[test]
fn pre_original_panic_fault_skips_the_bind() {
    let fixture = Fixture::new();
    let confirm = || {};
    let parts = fixture.parts(
        &confirm,
        FaultSelector {
            pre_original_panic: true,
            ..FaultSelector::default()
        },
    );
    let (result, outcome) = call_create(
        &parts,
        42,
        OWNER,
        |_| panic!("bind must not run after the pre-original panic"),
        |_| 1,
    );
    assert_eq!((result, outcome), (1, CreateOutcome::Stock));
}

#[test]
fn bind_panic_is_contained_and_the_original_still_runs_once() {
    let fixture = Fixture::new();
    let calls = AtomicUsize::new(0);
    let confirm = || {};
    let parts = fixture.parts(&confirm, FaultSelector::default());
    let (result, outcome) = call_create(
        &parts,
        42,
        OWNER,
        |_| panic!("injected bind failure"),
        |_| {
            calls.fetch_add(1, Ordering::AcqRel);
            // The panic containment cleared the frame — no binding is
            // attributable, exactly like the pre-original fault leg.
            assert!(current_frame().is_none());
            1
        },
    );
    assert_eq!((result, outcome), (1, CreateOutcome::Stock));
    assert_eq!(calls.load(Ordering::Acquire), 1);
}

#[test]
fn fault_selector_parses_exactly_the_documented_values() {
    for (value, probe) in [
        ("pre-original", 0usize),
        ("post-original", 1),
        ("token-mismatch", 2),
        ("xact-reject", 3),
        ("maintenance-saturation", 4),
        ("source-read", 5),
        ("header-synth", 6),
        ("generator-start", 7),
        ("bind-refused", 8),
        ("mid-song-failure", 9),
    ] {
        let fault = FaultSelector::parse(value).expect(value);
        let flags = [
            fault.pre_original_panic,
            fault.post_original_panic,
            fault.token_mismatch,
            fault.xact_reject,
            fault.maintenance_saturation,
            fault.source_read,
            fault.header_synth,
            fault.generator_start,
            fault.bind_refused,
            fault.mid_song_failure,
        ];
        assert_eq!(flags.iter().filter(|flag| **flag).count(), 1, "{value}");
        assert!(flags[probe], "{value}");
    }
    for retired in ["unknown", "validation", "conversion"] {
        assert_eq!(FaultSelector::parse(retired), None, "{retired}");
    }
}
