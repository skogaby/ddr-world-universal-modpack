use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use crate::core::memory_patch::{PatchBackend, PatchStep};
use crate::core::xact::rate::RateRatio;

use super::clock_patch::{
    build_clock_stub, build_clock_stub_with_callout, install_clock_with_backend,
    install_clock_with_backend_and_callout, scale_music_count_q31, ClockInstallError,
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

/// Decode a tiny subset of x86-64 well enough to walk the call-out prologue:
/// returns (length, mnemonic-ish tag) for the instruction at `at`.
fn decode(bytes: &[u8], at: usize) -> (usize, &'static str) {
    match &bytes[at..] {
        [0x49, 0x89, 0xe4, ..] => (3, "mov r12,rsp"),
        [0x48, 0x83, 0xe4, 0xf0, ..] => (4, "and rsp,-16"),
        [0x48, 0x81, 0xec, ..] => (7, "sub rsp,imm32"),
        [0x0f, 0x11, m, 0x24, _, ..] if m & 0xc7 == 0x44 => (5, "movups [rsp+d8],xmm"),
        [0x0f, 0x10, m, 0x24, _, ..] if m & 0xc7 == 0x44 => (5, "movups xmm,[rsp+d8]"),
        [0x48 | 0x4c, 0x89, m, 0x24, ..] if m & 0xc7 == 0x84 => (8, "mov [rsp+d32],r"),
        [0x48 | 0x4c, 0x8b, m, 0x24, ..] if m & 0xc7 == 0x84 => (8, "mov r,[rsp+d32]"),
        [0x48, 0x89, 0xf9, ..] => (3, "mov rcx,rdi"),
        [0x89, 0xda, ..] => (2, "mov edx,ebx"),
        [0x48, 0xb8, ..] => (10, "mov rax,imm64"),
        [0xff, 0xd0, ..] => (2, "call rax"),
        [0x89, 0xc3, ..] => (2, "mov ebx,eax"),
        [0x4c, 0x89, 0xe4, ..] => (3, "mov rsp,r12"),
        [0x44, 0x8d, 0x34, 0x18, ..] => (4, "lea r14d,[rax+rbx]"),
        _ => (0, "?"),
    }
}

#[test]
fn callout_stub_saves_every_volatile_register_and_keeps_the_identity_body_intact() {
    const CALLOUT: usize = 0x7fff_1234_5678_9abc;
    let plain = build_clock_stub(STUB, PATCH + 8).unwrap();
    let with = build_clock_stub_with_callout(STUB, PATCH + 8, Some(CALLOUT)).unwrap();
    assert_eq!(plain.callout_imm_offset, None);
    let imm = with.callout_imm_offset.unwrap();
    assert_eq!(
        u64::from_le_bytes(with.bytes[imm..imm + 8].try_into().unwrap()),
        CALLOUT as u64
    );
    // Walk the prologue instruction by instruction until the displaced lea.
    let mut at = 0;
    let mut tags = Vec::new();
    loop {
        let (len, tag) = decode(&with.bytes, at);
        assert_ne!(
            len,
            0,
            "undecodable byte at {at}: {:02x?}",
            &with.bytes[at..at + 4]
        );
        tags.push(tag);
        at += len;
        if tag == "lea r14d,[rax+rbx]" {
            break;
        }
    }
    let prologue_len = at - 4;
    // Save set: 6 xmm stores, 7 GPR stores; restore set mirrors them.
    assert_eq!(
        tags.iter().filter(|t| **t == "movups [rsp+d8],xmm").count(),
        6
    );
    assert_eq!(tags.iter().filter(|t| **t == "mov [rsp+d32],r").count(), 7);
    assert_eq!(tags.iter().filter(|t| **t == "mov r,[rsp+d32]").count(), 7);
    assert_eq!(
        tags.iter().filter(|t| **t == "movups xmm,[rsp+d8]").count(),
        6
    );
    let order: Vec<&str> = tags
        .iter()
        .copied()
        .filter(|t| {
            !matches!(
                *t,
                "movups [rsp+d8],xmm"
                    | "mov [rsp+d32],r"
                    | "mov r,[rsp+d32]"
                    | "movups xmm,[rsp+d8]"
            )
        })
        .collect();
    assert_eq!(
        order,
        [
            "mov r12,rsp",
            "and rsp,-16",
            "sub rsp,imm32",
            "mov rcx,rdi",
            "mov edx,ebx",
            "mov rax,imm64",
            "call rax",
            "mov ebx,eax",
            "mov rsp,r12",
            "lea r14d,[rax+rbx]",
        ]
    );
    // The frame: 0xC0 keeps 16-byte alignment and covers shadow + saves
    // (`sub rsp` follows the 3-byte `mov r12,rsp` and the 4-byte `and`).
    assert_eq!(&with.bytes[7..14], &[0x48, 0x81, 0xec, 0xc0, 0, 0, 0]);
    // The identity body after the prologue starts with the displaced lea and
    // is the plain stub's body up to its rel32 fields (factor disp + return
    // jump), which must still resolve correctly from the new position.
    let body = &with.bytes[prologue_len..];
    assert_eq!(&body[..4], &plain.bytes[..4]);
    assert_eq!(&body[4..10], &plain.bytes[4..10]);
    assert_eq!((STUB + with.factor_offset) % 8, 0);
    assert_eq!(
        u64::from_le_bytes(
            with.bytes[with.factor_offset..with.factor_offset + 8]
                .try_into()
                .unwrap()
        ),
        IDENTITY_Q31
    );
    let jump_end = STUB + with.return_jump_offset + 5;
    let displacement = i32::from_le_bytes(
        with.bytes[with.return_jump_offset + 1..with.return_jump_offset + 5]
            .try_into()
            .unwrap(),
    );
    assert_eq!(
        (jump_end as i64 + i64::from(displacement)) as usize,
        PATCH + 8
    );
    // imul's rel32 points at the factor slot.
    let imul = with
        .bytes
        .windows(3)
        .position(|w| w == [0x48, 0xf7, 0x2d])
        .unwrap();
    let disp = i32::from_le_bytes(with.bytes[imul + 3..imul + 7].try_into().unwrap());
    assert_eq!(
        (STUB + imul + 7) as i64 + i64::from(disp),
        (STUB + with.factor_offset) as i64
    );
    // Fits the allocation with headroom.
    assert!(with.bytes.len() <= 320);
    // Installing through the backend with a call-out lands the same bytes.
    let readiness = AtomicBool::new(false);
    let mut memory = SparseMemory::new();
    let installed =
        install_clock_with_backend_and_callout(&mut memory, PATCH, &readiness, Some(CALLOUT))
            .unwrap();
    assert_eq!(installed.stub_address, STUB);
    assert_eq!(
        memory
            .read(STUB, with.bytes.len(), PatchStep::Readback)
            .unwrap(),
        with.bytes
    );
    assert!(readiness.load(Ordering::Acquire));
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
