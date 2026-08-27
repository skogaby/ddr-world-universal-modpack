use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use crate::core::memory_patch::{PatchBackend, PatchStep};
use crate::core::xact::rate::RateRatio;

use super::clock_patch::{
    build_clock_stub, install_clock_with_backend, scale_music_count_q31, ClockInstallError,
    RatePublication, ResetOutcome, CLOCK_PATCH_BYTES, IDENTITY_Q31,
};

const PATCH: usize = 0x1000;
const STUB: usize = 0x2000;

#[derive(Clone, Copy)]
struct Protection;

struct SparseMemory {
    bytes: BTreeMap<usize, u8>,
    allocation: Option<usize>,
    fail_write_at: Option<usize>,
}

impl SparseMemory {
    fn new() -> Self {
        let mut bytes = BTreeMap::new();
        for (index, byte) in CLOCK_PATCH_BYTES.into_iter().enumerate() {
            bytes.insert(PATCH + index, byte);
        }
        Self {
            bytes,
            allocation: Some(STUB),
            fail_write_at: None,
        }
    }
}

impl PatchBackend for SparseMemory {
    type Protection = Protection;

    fn read(&mut self, address: usize, length: usize, _step: PatchStep) -> Result<Vec<u8>, ()> {
        Ok((0..length)
            .map(|offset| *self.bytes.get(&(address + offset)).unwrap_or(&0))
            .collect())
    }

    fn make_writable(&mut self, _address: usize, _length: usize) -> Result<Protection, ()> {
        Ok(Protection)
    }

    fn write(&mut self, address: usize, bytes: &[u8], _step: PatchStep) -> Result<(), ()> {
        if self.fail_write_at == Some(address) {
            return Err(());
        }
        for (offset, byte) in bytes.iter().copied().enumerate() {
            self.bytes.insert(address + offset, byte);
        }
        Ok(())
    }

    fn flush(&mut self, _address: usize, _length: usize, _step: PatchStep) -> Result<(), ()> {
        Ok(())
    }

    fn restore_protection(
        &mut self,
        _address: usize,
        _length: usize,
        _protection: Protection,
        _step: PatchStep,
    ) -> Result<(), ()> {
        Ok(())
    }

    fn allocate_near(&mut self, _near: usize, _size: usize) -> Option<usize> {
        self.allocation
    }
}

#[test]
fn identity_q31_preserves_complete_signed_music_count() {
    for value in [i32::MIN, -1_000_000, -1, 0, 1, 1_000_000, i32::MAX] {
        assert_eq!(scale_music_count_q31(value, IDENTITY_Q31), value);
    }
    let slow = RateRatio::new(3, 4).unwrap().q31().unwrap() as u64;
    let fast = RateRatio::new(5, 4).unwrap().q31().unwrap() as u64;
    assert_eq!(scale_music_count_q31(1_000, slow), 750);
    assert_eq!(scale_music_count_q31(-1_000, slow), -750);
    assert_eq!(scale_music_count_q31(1_000, fast), 1_250);
    assert_eq!(scale_music_count_q31(-1_000, fast), -1_250);
}

#[test]
fn scalar_domain_boundary_q31_factors_scale_exactly() {
    // 25%: factor 2^29 (the slowest supported rate).
    let slowest = RateRatio::new(1, 4).unwrap().q31().unwrap() as u64;
    assert_eq!(slowest, 1u64 << 29);
    assert_eq!(scale_music_count_q31(1_000, slowest), 250);
    assert_eq!(scale_music_count_q31(-1_000, slowest), -250);
    // Half-away rounding: 2147483647/4 = 536870911.75 → 536870912.
    assert_eq!(scale_music_count_q31(i32::MAX, slowest), 536_870_912);
    // MIN = -2^31 divides exactly.
    assert_eq!(scale_music_count_q31(i32::MIN, slowest), i32::MIN / 4);

    // 175%: factor 7/4 · 2^31 — EXCEEDS i32::MAX, proving the 64-bit factor
    // slot and i128 product path (the fastest supported rate).
    let fastest = RateRatio::new(7, 4).unwrap().q31().unwrap() as u64;
    assert_eq!(fastest, 7u64 << 29);
    assert!(fastest > i32::MAX as u64);
    assert_eq!(scale_music_count_q31(1_000, fastest), 1_750);
    assert_eq!(scale_music_count_q31(-1_000, fastest), -1_750);
    // Saturation at the extremes instead of wrap.
    assert_eq!(scale_music_count_q31(i32::MAX, fastest), i32::MAX);
    assert_eq!(scale_music_count_q31(i32::MIN, fastest), i32::MIN);
}

#[test]
fn emitted_stub_replays_instructions_has_aligned_factor_and_returns_exactly() {
    let layout = build_clock_stub(STUB, PATCH + CLOCK_PATCH_BYTES.len()).unwrap();
    assert_eq!(&layout.bytes[..4], &[0x44, 0x8d, 0x34, 0x18]);
    assert!(layout
        .bytes
        .windows(4)
        .any(|window| window == [0x4c, 0x8d, 0x67, 0x58]));
    assert_eq!((STUB + layout.factor_offset) % 8, 0);
    assert_eq!(
        u64::from_le_bytes(
            layout.bytes[layout.factor_offset..layout.factor_offset + 8]
                .try_into()
                .unwrap()
        ),
        IDENTITY_Q31
    );
    let jump_end = STUB + layout.return_jump_offset + 5;
    let displacement = i32::from_le_bytes(
        layout.bytes[layout.return_jump_offset + 1..layout.return_jump_offset + 5]
            .try_into()
            .unwrap(),
    );
    assert_eq!(
        (jump_end as i64 + i64::from(displacement)) as usize,
        PATCH + 8
    );
}

#[test]
fn checked_install_publishes_readiness_last_and_fails_closed() {
    let readiness = AtomicBool::new(false);
    let mut memory = SparseMemory::new();
    let installed = install_clock_with_backend(&mut memory, PATCH, &readiness).unwrap();
    assert!(readiness.load(Ordering::Acquire));
    assert_eq!(installed.stub_address, STUB);
    assert_eq!((installed.factor_address as usize) % 8, 0);
    assert_eq!(memory.read(PATCH, 1, PatchStep::Readback).unwrap()[0], 0xe9);

    let readiness = AtomicBool::new(true);
    let mut memory = SparseMemory::new();
    memory.allocation = None;
    assert_eq!(
        install_clock_with_backend(&mut memory, PATCH, &readiness),
        Err(ClockInstallError::Allocate)
    );
    assert!(!readiness.load(Ordering::Acquire));
    assert_eq!(
        memory.read(PATCH, 8, PatchStep::Readback).unwrap(),
        CLOCK_PATCH_BYTES
    );

    let readiness = AtomicBool::new(true);
    let mut memory = SparseMemory::new();
    memory.fail_write_at = Some(PATCH);
    assert!(install_clock_with_backend(&mut memory, PATCH, &readiness).is_err());
    assert!(!readiness.load(Ordering::Acquire));
    assert_eq!(
        memory.read(PATCH, 8, PatchStep::Readback).unwrap(),
        CLOCK_PATCH_BYTES
    );
}

#[test]
fn publication_readers_never_observe_torn_identity_snapshots() {
    let factor: &'static AtomicU64 = Box::leak(Box::new(AtomicU64::new(IDENTITY_Q31)));
    let publication = Arc::new(RatePublication::new(factor));
    let stop = Arc::new(AtomicBool::new(false));
    let readers: Vec<_> = (0..4)
        .map(|_| {
            let publication = Arc::clone(&publication);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(Ordering::Acquire) {
                    let snapshot = publication.read();
                    assert_eq!(snapshot.requested_percent, 100);
                    assert_eq!(snapshot.effective_rate, RateRatio::IDENTITY);
                    assert!(!snapshot.committed);
                    assert_eq!(factor.load(Ordering::Acquire), IDENTITY_Q31);
                }
            })
        })
        .collect();
    for generation in 1..=20_000 {
        publication.publish_identity(generation, (generation & 3) as u8);
        if generation % 3 == 0 {
            let _ = publication.reset_identity();
        }
    }
    stop.store(true, Ordering::Release);
    for reader in readers {
        reader.join().unwrap();
    }
    publication.reset_identity();
    assert_eq!(publication.read().generation, 0);
    assert_eq!(factor.load(Ordering::Acquire), IDENTITY_Q31);
}

#[test]
fn reset_defers_behind_writer_and_is_applied_before_release() {
    let factor: &'static AtomicU64 = Box::leak(Box::new(AtomicU64::new(IDENTITY_Q31)));
    let publication = RatePublication::new(factor);
    let guard = publication.begin_identity_write_for_test().unwrap();
    assert_eq!(publication.reset_identity(), ResetOutcome::Deferred);
    guard.finish(99, 3);
    assert_eq!(publication.read().generation, 0);
    assert_eq!(publication.read().participant_mask, 0);
    assert_eq!(factor.load(Ordering::Acquire), IDENTITY_Q31);
}

#[test]
fn non_identity_commit_predicate_selects_the_tick_conversion_path() {
    use super::clock_patch::RateSnapshot;
    // The tick_domain conversion path (design req 30; formerly Step 4's
    // scaffold gate) engages exactly when a non-identity generation is
    // COMMITTED.
    assert!(!RateSnapshot::IDENTITY.is_non_identity_commit());
    let committed_75 = RateSnapshot {
        generation: 3,
        requested_percent: 75,
        participant_mask: 0b01,
        effective_rate: RateRatio::new(3, 4).unwrap(),
        committed: true,
    };
    assert!(committed_75.is_non_identity_commit());
    // Uncommitted (armed/failed attempts) never converts.
    let uncommitted_75 = RateSnapshot {
        committed: false,
        ..committed_75
    };
    assert!(!uncommitted_75.is_non_identity_commit());
    // A 100% snapshot never converts even if committed (defensive: identity
    // never arms, so a committed identity is unreachable by construction).
    let committed_identity = RateSnapshot {
        requested_percent: 100,
        effective_rate: RateRatio::IDENTITY,
        ..committed_75
    };
    assert!(!committed_identity.is_non_identity_commit());
}

#[test]
fn csv_rate_cells_emit_the_committed_exact_ratio_or_uniform_identity() {
    use super::clock_patch::RateSnapshot;
    use crate::core::xact::rate::target_for_percent;
    // Identity, committed-100, and uncommitted non-identity snapshots all
    // emit the uniform identity schema (PUS CSV design req 34): a song that
    // never committed a rate reads exactly like a plain 100% song.
    let committed_100 = RateSnapshot {
        committed: true,
        ..RateSnapshot::IDENTITY
    };
    let uncommitted_75 = RateSnapshot {
        generation: 5,
        requested_percent: 75,
        participant_mask: 0b01,
        effective_rate: RateRatio::new(3, 4).unwrap(),
        committed: false,
    };
    for snapshot in [RateSnapshot::IDENTITY, committed_100, uncommitted_75] {
        assert_eq!(snapshot.csv_rate_cells(), (100, "1/1".to_string()));
    }
    // Committed non-identity: the requested percent + the committed EXACT
    // ratio as a source/output fraction (never a rounded decimal). Built
    // through the production target_for_percent path with the
    // non-block-clean fixture; literal pins on the reduced pairs.
    let cells = |percent: u32| {
        let target = target_for_percent(9_876_543, 128, percent).unwrap();
        RateSnapshot {
            generation: 9,
            requested_percent: percent as i32,
            participant_mask: 0b01,
            effective_rate: target.rate,
            committed: true,
        }
        .csv_rate_cells()
    };
    assert_eq!(cells(50), (50, "9876543/19753088".to_string()));
    // 125% reduces by gcd 3: the emitted fraction is the GCD-reduced pair
    // the publication carries, not the raw frame counts.
    assert_eq!(cells(125), (125, "3292181/2633728".to_string()));
}
