//! Real Speed Core BPM — patches SetScrollSpeed to divide by Core BPM
//! instead of Max BPM (R24/R25/R26 patches).

use std::sync::atomic::{AtomicPtr, Ordering};

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};

/// R24: JA +5 → JMP +0x64 (2 bytes at anchor − 0x1C).
const R24_OFFSET: isize = -0x1C;
const R24_PATCHED: [u8; 2] = [0xEB, 0x64];

/// R25: ModR/M byte at anchor + 0x03 (1 byte).
const R25_OFFSET: isize = 0x03;
const R25_PATCHED: u8 = 0xC2;

/// R26: 12-byte cave at anchor + 0x4A.
const R26_OFFSET: isize = 0x4A;
const R26_CAVE: [u8; 12] = [
    0xF2, 0x0F, 0x10, 0x93, 0x88, 0x00, 0x00, 0x00, // movsd xmm2, [rbx+0x88]
    0x77, 0x97, // ja rel8 (back to original JA-taken target)
    0xEB, 0x90, // jmp rel8 (back to original fall-through)
];

static ANCHOR_ADDR: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static ORIGINAL_R24: [std::sync::atomic::AtomicU8; 2] = [
    std::sync::atomic::AtomicU8::new(0),
    std::sync::atomic::AtomicU8::new(0),
];
static ORIGINAL_R25: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
static ORIGINAL_R26: [std::sync::atomic::AtomicU8; 12] = [
    std::sync::atomic::AtomicU8::new(0),
    std::sync::atomic::AtomicU8::new(0),
    std::sync::atomic::AtomicU8::new(0),
    std::sync::atomic::AtomicU8::new(0),
    std::sync::atomic::AtomicU8::new(0),
    std::sync::atomic::AtomicU8::new(0),
    std::sync::atomic::AtomicU8::new(0),
    std::sync::atomic::AtomicU8::new(0),
    std::sync::atomic::AtomicU8::new(0),
    std::sync::atomic::AtomicU8::new(0),
    std::sync::atomic::AtomicU8::new(0),
    std::sync::atomic::AtomicU8::new(0),
];

/// Called during mod init to resolve and store the anchor address.
pub fn init(signatures: &SignatureStore) -> bool {
    let Some(anchor) = signatures.get_address("real_speed_bpm_anchor") else {
        log_warn!("real_speed: real_speed_bpm_anchor not resolved");
        return false;
    };
    ANCHOR_ADDR.store(anchor as *mut u8, Ordering::Release);
    true
}

pub fn enable() {
    let anchor = ANCHOR_ADDR.load(Ordering::Acquire);
    if anchor.is_null() {
        return;
    }

    unsafe {
        let r24_addr = anchor.offset(R24_OFFSET);
        let r25_addr = anchor.offset(R25_OFFSET);
        let r26_addr = anchor.offset(R26_OFFSET);

        // Save original bytes.
        for i in 0..2 {
            ORIGINAL_R24[i].store(
                memory::read_u8(r24_addr.add(i) as *const u8),
                Ordering::Release,
            );
        }
        ORIGINAL_R25.store(memory::read_u8(r25_addr as *const u8), Ordering::Release);
        for i in 0..12 {
            ORIGINAL_R26[i].store(
                memory::read_u8(r26_addr.add(i) as *const u8),
                Ordering::Release,
            );
        }

        // Apply R24.
        let old_prot = memory::make_writable(r24_addr as *const u8, 2);
        for (i, &b) in R24_PATCHED.iter().enumerate() {
            memory::write_u8(r24_addr.add(i), b);
        }
        memory::restore_protection(r24_addr as *const u8, 2, old_prot);

        // Apply R25.
        let old_prot = memory::make_writable(r25_addr as *const u8, 1);
        memory::write_u8(r25_addr, R25_PATCHED);
        memory::restore_protection(r25_addr as *const u8, 1, old_prot);

        // Apply R26.
        let old_prot = memory::make_writable(r26_addr as *const u8, 12);
        for (i, &b) in R26_CAVE.iter().enumerate() {
            memory::write_u8(r26_addr.add(i), b);
        }
        memory::restore_protection(r26_addr as *const u8, 12, old_prot);

        log_info!("real_speed: BPM divisor swap applied (R24/R25/R26)");
    }
}

pub fn disable() {
    let anchor = ANCHOR_ADDR.load(Ordering::Acquire);
    if anchor.is_null() {
        return;
    }

    unsafe {
        // Restore R24.
        let r24_addr = anchor.offset(R24_OFFSET);
        let old_prot = memory::make_writable(r24_addr as *const u8, 2);
        for i in 0..2 {
            memory::write_u8(r24_addr.add(i), ORIGINAL_R24[i].load(Ordering::Acquire));
        }
        memory::restore_protection(r24_addr as *const u8, 2, old_prot);

        // Restore R25.
        let r25_addr = anchor.offset(R25_OFFSET);
        let old_prot = memory::make_writable(r25_addr as *const u8, 1);
        memory::write_u8(r25_addr, ORIGINAL_R25.load(Ordering::Acquire));
        memory::restore_protection(r25_addr as *const u8, 1, old_prot);

        // Restore R26.
        let r26_addr = anchor.offset(R26_OFFSET);
        let old_prot = memory::make_writable(r26_addr as *const u8, 12);
        for i in 0..12 {
            memory::write_u8(r26_addr.add(i), ORIGINAL_R26[i].load(Ordering::Acquire));
        }
        memory::restore_protection(r26_addr as *const u8, 12, old_prot);
    }

    log_info!("real_speed: BPM divisor swap reverted");
}
