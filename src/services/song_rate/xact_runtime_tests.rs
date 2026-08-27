use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};

use crate::core::xact::rate::RateRatio;

use super::clock_patch::{RatePublication, IDENTITY_Q31};
use super::xact_runtime::{
    current_frame, enter_frame, MaintenanceEvent, MaintenanceKind, MaintenanceQueue, RedirectToken,
    SlotError, TlsError, XactSlotPhase, XactSlots, MAX_XACT_SLOTS,
};

fn token(nonce: u64, depth: u8, generation: u64) -> RedirectToken {
    RedirectToken {
        call_nonce: nonce,
        call_depth: depth,
        generation,
        requested_percent: 75,
        participant_mask: 1,
        stage_index: 2,
        effective_rate: RateRatio::new(3, 4).unwrap(),
    }
}

#[test]
fn nested_tls_frames_are_call_nonced_and_guarded() {
    assert!(current_frame().is_none());
    let outer = enter_frame(10).unwrap();
    let outer_identity = outer.identity();
    assert_eq!(outer_identity.depth, 1);
    assert_eq!(current_frame().unwrap(), outer_identity);
    {
        let inner = enter_frame(20).unwrap();
        assert_eq!(inner.identity().depth, 2);
        assert_ne!(inner.identity().nonce, outer_identity.nonce);
        assert_eq!(current_frame().unwrap().file_id, 20);
    }
    assert_eq!(current_frame().unwrap(), outer_identity);
    drop(outer);
    assert!(current_frame().is_none());
}

#[test]
fn tls_overflow_fails_closed_and_does_not_damage_existing_frames() {
    let a = enter_frame(1).unwrap();
    let b = enter_frame(2).unwrap();
    let c = enter_frame(3).unwrap();
    let d = enter_frame(4).unwrap();
    assert_eq!(enter_frame(5).unwrap_err(), TlsError::Overflow);
    assert_eq!(current_frame().unwrap().file_id, 4);
    drop(d);
    drop(c);
    drop(b);
    drop(a);
    assert!(current_frame().is_none());
}

#[test]
fn slots_validate_exact_identity_and_recover_only_one_match() {
    let slots = XactSlots::new();
    let slot = slots.claim(100, 7, 1, 44).unwrap();
    let redirect = token(7, 1, 9);
    slots.expose(slot, 100, 7, 1, 44, redirect).unwrap();
    assert_eq!(slots.phase(slot), Some(XactSlotPhase::Exposed));
    assert_eq!(slots.recover_exposed(100, 7, 44), Ok(slot));
    assert_eq!(
        slots.recover_exposed(101, 7, 44),
        Err(SlotError::NoExactMatch)
    );
    assert_eq!(slots.commit(slot, 100, 7, 44).unwrap(), redirect);
    assert_eq!(slots.phase(slot), Some(XactSlotPhase::Committed));
    slots.begin_release(slot, 44).unwrap();
    slots.finish_release(slot).unwrap();
    assert_eq!(slots.phase(slot), Some(XactSlotPhase::Free));
}

#[test]
fn only_four_concurrent_calls_can_claim_slots() {
    let slots = Arc::new(XactSlots::new());
    let barrier = Arc::new(Barrier::new(9));
    let threads: Vec<_> = (0..8)
        .map(|thread| {
            let slots = Arc::clone(&slots);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                slots.claim(thread, thread + 1, 1, thread as i32)
            })
        })
        .collect();
    barrier.wait();
    let claims: Vec<_> = threads
        .into_iter()
        .filter_map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(claims.len(), MAX_XACT_SLOTS);
    claims
        .into_iter()
        .for_each(|slot| slots.abandon(slot).unwrap());
}

#[test]
fn maintenance_queue_is_fifo_bounded_and_never_overwrites() {
    let queue = MaintenanceQueue::<4>::new();
    for id in 0..4 {
        queue
            .push(MaintenanceEvent {
                kind: MaintenanceKind::ReclaimBinding,
                slot_index: id as u8,
            })
            .unwrap();
    }
    assert!(queue
        .push(MaintenanceEvent {
            kind: MaintenanceKind::ReclaimBinding,
            slot_index: 9,
        })
        .is_err());
    for id in 0..4 {
        assert_eq!(queue.pop().unwrap().slot_index, id as u8);
    }
    assert!(queue.pop().is_none());
}

#[test]
fn transaction_primitives_never_change_identity_publication() {
    let factor: &'static AtomicU64 = Box::leak(Box::new(AtomicU64::new(IDENTITY_Q31)));
    let publication = RatePublication::new(factor);
    let slots = XactSlots::new();
    let slot = slots.claim(1, 1, 1, 1).unwrap();
    slots.abandon(slot).unwrap();
    let frame = enter_frame(1).unwrap();
    drop(frame);
    let snapshot = publication.read();
    assert_eq!(snapshot.effective_rate, RateRatio::IDENTITY);
    assert!(!snapshot.committed);
    assert_eq!(factor.load(Ordering::Acquire), IDENTITY_Q31);
}

#[test]
fn bank_timeline_roundtrips_and_orders_events() {
    use super::xact_runtime::{BankCreatePath, BankEvent, BankEventKind, BankTimeline};
    let timeline = BankTimeline::new();
    timeline.record(BankEventKind::Create, 42, 1, BankCreatePath::Stock);
    timeline.record(BankEventKind::Create, 43, 1, BankCreatePath::Committed);
    timeline.record(BankEventKind::Unregister, 42, 0, BankCreatePath::None);
    let first = timeline.pop().unwrap();
    assert_eq!(
        (first.kind, first.file_id, first.status, first.path),
        (BankEventKind::Create, 42, 1, BankCreatePath::Stock)
    );
    let second = timeline.pop().unwrap();
    assert_eq!(
        (second.kind, second.file_id, second.path),
        (BankEventKind::Create, 43, BankCreatePath::Committed)
    );
    let third: BankEvent = timeline.pop().unwrap();
    assert_eq!(
        (third.kind, third.file_id, third.path),
        (BankEventKind::Unregister, 42, BankCreatePath::None)
    );
    assert!(timeline.pop().is_none());
    assert_eq!(timeline.take_dropped(), 0);
}

#[test]
fn bank_timeline_negative_ids_and_all_paths_survive_packing() {
    use super::xact_runtime::{BankCreatePath, BankEventKind, BankTimeline};
    let timeline = BankTimeline::new();
    for (id, path) in [
        (-1, BankCreatePath::None),
        (i32::MAX, BankCreatePath::LateFailed),
        (i32::MIN, BankCreatePath::RecoveryFailed),
        (7, BankCreatePath::TlsOverflow),
    ] {
        timeline.record(BankEventKind::Create, id, 255, path);
        let event = timeline.pop().unwrap();
        assert_eq!((event.file_id, event.status, event.path), (id, 255, path));
    }
}

#[test]
fn bank_timeline_overflow_drops_and_counts_without_corruption() {
    use super::xact_runtime::{
        BankCreatePath, BankEventKind, BankTimeline, BANK_TIMELINE_CAPACITY,
    };
    let timeline = BankTimeline::new();
    for id in 0..(BANK_TIMELINE_CAPACITY as i32 + 8) {
        timeline.record(BankEventKind::Create, id, 1, BankCreatePath::Stock);
    }
    assert_eq!(timeline.take_dropped(), 8);
    // The retained window is the oldest CAPACITY events, in order.
    for id in 0..BANK_TIMELINE_CAPACITY as i32 {
        assert_eq!(timeline.pop().unwrap().file_id, id);
    }
    assert!(timeline.pop().is_none());
}

#[test]
fn finish_quarantine_frees_only_a_quarantined_slot() {
    let slots = XactSlots::new();
    let slot = slots.claim(100, 7, 1, 44).unwrap();
    // Quarantine requires Exposed first.
    assert_eq!(slots.finish_quarantine(slot), Err(SlotError::IllegalPhase));
    slots.expose(slot, 100, 7, 1, 44, token(7, 1, 9)).unwrap();
    slots.quarantine(slot).unwrap();
    assert_eq!(slots.phase(slot), Some(XactSlotPhase::Quarantined));
    // The drain frees it exactly once; the slot is reusable afterwards.
    slots.finish_quarantine(slot).unwrap();
    assert_eq!(slots.phase(slot), Some(XactSlotPhase::Free));
    assert_eq!(slots.finish_quarantine(slot), Err(SlotError::IllegalPhase));
    assert!(slots.claim(200, 8, 1, 45).is_some());
    assert_eq!(
        slots.finish_quarantine(MAX_XACT_SLOTS),
        Err(SlotError::InvalidIndex)
    );
}
