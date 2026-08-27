//! logf(0) guard — allocates a small RWX stub that returns 0.0 for zero
//! input instead of letting bare logf produce -inf/NaN, then redirects
//! the R16 call site to use it.

use std::sync::atomic::{AtomicPtr, Ordering};

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};

/// R15: JMP rel8 displacement at anchor − 0x38. Stock value.
const R15_OFFSET: isize = -0x38;
const R15_PATCHED: u8 = 0x37;

/// R16: CALL rel32 displacement starts at anchor + 0x04.
const R16_DISP_OFFSET: isize = 0x04;

/// The 14-byte guarded-logf stub.
const STUB_TEMPLATE: [u8; 14] = [
    0x0F, 0x57, 0xC9, // xorps xmm1, xmm1
    0x0F, 0x2E, 0xC1, // ucomiss xmm0, xmm1
    0x75, 0x01, // jne +1 (skip ret)
    0xC3, // ret (xmm0 is already 0.0)
    0xE9, 0x00, 0x00, 0x00, 0x00, // jmp rel32 bare_logf (filled at runtime)
];

static ANCHOR_ADDR: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static STUB_ADDR: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static ORIGINAL_R15: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
static ORIGINAL_R16: [std::sync::atomic::AtomicU8; 4] = [
    std::sync::atomic::AtomicU8::new(0),
    std::sync::atomic::AtomicU8::new(0),
    std::sync::atomic::AtomicU8::new(0),
    std::sync::atomic::AtomicU8::new(0),
];

/// Called during mod init to resolve and store the anchor address.
pub fn init(signatures: &SignatureStore) -> bool {
    let Some(anchor) = signatures.get_address("real_speed_logf_anchor") else {
        log_warn!("logf_stub: real_speed_logf_anchor not resolved");
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
        // Derive bare logf from the existing CALL rel32 BEFORE patching.
        let call_opcode_addr = anchor.offset(R16_DISP_OFFSET - 1); // E8 byte
        let disp_addr = anchor.offset(R16_DISP_OFFSET);
        let original_rel32 = memory::read_i32(disp_addr as *const u8);
        let after_call = call_opcode_addr.add(5);
        let bare_logf = (after_call as isize + original_rel32 as isize) as *const u8;

        // Save original bytes for R15 and R16.
        let r15_addr = anchor.offset(R15_OFFSET);
        ORIGINAL_R15.store(memory::read_u8(r15_addr as *const u8), Ordering::Release);
        for i in 0..4 {
            ORIGINAL_R16[i].store(
                memory::read_u8(disp_addr.add(i) as *const u8),
                Ordering::Release,
            );
        }

        // Allocate the stub near the call site.
        let stub = memory::alloc_near(anchor as *const u8, STUB_TEMPLATE.len());
        if stub.is_null() {
            log_warn!("logf_stub: alloc_near failed — logf guard will not be applied");
            return;
        }
        STUB_ADDR.store(stub, Ordering::Release);

        // Write stub template.
        std::ptr::copy_nonoverlapping(STUB_TEMPLATE.as_ptr(), stub, STUB_TEMPLATE.len());

        // Patch the JMP rel32 inside the stub to point to bare logf.
        let jmp_disp_addr = stub.add(0x0A);
        let jmp_after_addr = stub.add(0x0E);
        let jmp_disp = (bare_logf as isize) - (jmp_after_addr as isize);
        memory::write_i32(jmp_disp_addr, jmp_disp as i32);

        // Apply R15 patch.
        let old_prot = memory::make_writable(r15_addr as *const u8, 1);
        memory::write_u8(r15_addr, R15_PATCHED);
        memory::restore_protection(r15_addr as *const u8, 1, old_prot);

        // Apply R16 patch — redirect CALL to our stub.
        let new_rel32 = (stub as isize) - (after_call as isize);
        let old_prot = memory::make_writable(disp_addr as *const u8, 4);
        memory::write_i32(disp_addr, new_rel32 as i32);
        memory::restore_protection(disp_addr as *const u8, 4, old_prot);

        log_info!(
            "logf_stub: enabled (stub @ {:p}, bare logf @ {:p})",
            stub,
            bare_logf
        );
    }
}

pub fn disable() {
    let anchor = ANCHOR_ADDR.load(Ordering::Acquire);
    if anchor.is_null() {
        return;
    }
    let stub = STUB_ADDR.load(Ordering::Acquire);
    if stub.is_null() {
        return;
    }

    unsafe {
        // Restore R16 first (so the call goes back to bare logf).
        let disp_addr = anchor.offset(R16_DISP_OFFSET);
        let old_prot = memory::make_writable(disp_addr as *const u8, 4);
        for i in 0..4 {
            memory::write_u8(disp_addr.add(i), ORIGINAL_R16[i].load(Ordering::Acquire));
        }
        memory::restore_protection(disp_addr as *const u8, 4, old_prot);

        // Restore R15.
        let r15_addr = anchor.offset(R15_OFFSET);
        let old_prot = memory::make_writable(r15_addr as *const u8, 1);
        memory::write_u8(r15_addr, ORIGINAL_R15.load(Ordering::Acquire));
        memory::restore_protection(r15_addr as *const u8, 1, old_prot);
    }

    log_info!("logf_stub: disabled");
}
