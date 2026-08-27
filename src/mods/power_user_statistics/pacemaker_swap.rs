//! Pacemaker → MsError swap — when the per-player option is ON, replaces
//! the pacemaker score-delta readout with the most recent ms-error, and
//! optionally forces the white color when |error| < threshold.
//!
//! Patches the 7-byte `MOV RDX, [RDI+0xB0]` instruction inside the
//! pacemaker render case (opcode 0x1036) of the score-render function.
//! A JMP to a hand-assembled stub overrides ESI (the formatter input)
//! with our ms-error value before proceeding. The white-zone color
//! trigger (the downstream `TEST ESI, ESI; JNE`) is handled by zeroing
//! ESI when |ms_error| < threshold, causing the fall-through to the
//! white path.
//!
//! The stub also passes RDI (= the `sequence::dance::NoteResultActor`
//! whose msg handler hosts the patch site) so the Rust callback can force
//! the pacemaker's VISIBILITY when the option is ON: the visibility byte
//! at `NoteResultActor+0xC0` is 0 from the ctor and only ever set to 1 by
//! `sequence::dance::GhostActor::onUpdate` after a successful ghost /
//! rival-target download — with no prior score and no rival the 0x1036
//! case still runs per judged step but re-hides the clip every time.
//! When `pacemaker_to_mserror` is ON the readout is OUR display, so the
//! callback writes the byte to 1 (vtable-guarded) and re-asserts the
//! clip's visibility for the current dispatch (the game's own set-visible
//! ran just before the patch site with the stale 0).
//!
//! # Stub layout (all offsets from stub base)
//!
//! ```text
//!   push rbx            ; align-pad (8 registers total → 64-byte push block, RSP stays 16-aligned)
//!   push rax
//!   push rcx
//!   push rdx
//!   push r8
//!   push r9
//!   push r10
//!   push r11
//!   sub  rsp, 0x20      ; shadow space
//!   mov  ecx, esi       ; arg1 = original ESI
//!   mov  edx, [r14]     ; arg2 = player_side
//!   mov  r8, rdi        ; arg3 = NoteResultActor (RDI at the patch site)
//!   movabs rax, <fn>
//!   call rax
//!   mov  esi, eax       ; capture return value into ESI
//!   add  rsp, 0x20
//!   pop  r11
//!   pop  r10
//!   pop  r9
//!   pop  r8
//!   pop  rdx
//!   pop  rcx
//!   pop  rax
//!   pop  rbx            ; restore align-pad register
//!   mov  rdx, [rdi+0xb0]  ; displaced original instruction
//!   jmp  rel32 <return_addr>  ; trampoline — does NOT touch any register
//! ```
//!
//! The `jmp rel32` is always valid because `alloc_near` guarantees the stub
//! is allocated within ±2 GB of the patch site, and `return_addr` is only
//! 7 bytes past the patch site (same allocation neighbourhood).

use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::services::{bm2d_api, custom_options};
use crate::{log_info, log_warn};

use super::data_feed;

/// We patch 11 bytes starting at the MOVSXD RSI,[R14+8] instruction (4 bytes)
/// followed by MOV RDX,[RDI+0xB0] (7 bytes). The stub executes the first
/// displaced instruction, overrides ESI with our value, then executes the
/// second displaced instruction before jumping back.
const PATCH_SIZE: usize = 11;

// ── NoteResultActor / pacemaker clip layout (0x18007a450 / 0x18007b300
// on 20260721; the +0xC0 byte attested by the ctor's `88 99 C0 00 00 00`
// write on 20260526/20260616/20260721) ───────────────────────────────
/// Visibility byte the 0x1036 case passes to the clip's set-visible every
/// dispatch. Ctor = 0; the ONLY stock writer of 1 is
/// `GhostActor::onUpdate` after a successful ghost/rival download.
const NOTE_RESULT_VIS_OFFSET: usize = 0xC0;
/// `dance_score_compare` CMovieClip wrapper.
const NOTE_RESULT_PACEMAKER_CLIP_OFFSET: usize = 0xB0;
/// CMovieClip wrapper: AFP layer id.
const CLIP_LAYER_ID_OFFSET: usize = 0x08;
/// `afp_layer_set_attribute` visibility bit.
const LAYER_ATTR_VISIBLE: u32 = 0x1;

static PATCH_ADDR: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static STUB_ADDR: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
/// NoteResultActor vtable (RTTI) — guards the visibility write: the byte
/// at +0xC0 is only touched when the actor the stub handed us really IS a
/// NoteResultActor on this build. Null = visibility forcing disabled
/// (value swap unaffected).
static NOTE_RESULT_VTABLE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
/// Set to true once the JMP patch is live; guards enable() against re-entry.
static PATCH_ACTIVE: AtomicBool = AtomicBool::new(false);
static ORIGINAL_BYTES: [std::sync::atomic::AtomicU8; PATCH_SIZE] = [
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

/// Rust-side logic called from the stub. Returns the ESI value to use.
/// If the option is OFF, returns the original score delta unchanged.
/// If ON, returns the ms-error (or 0 if below threshold for white-zone),
/// and forces the pacemaker's visibility for sides with no ghost/rival
/// data (see the module doc). `actor` = the NoteResultActor (RDI at the
/// patch site).
#[no_mangle]
unsafe extern "C" fn pacemaker_swap_get_esi(
    original_esi: i32,
    player_side: i32,
    actor: *mut u8,
) -> i32 {
    match std::panic::catch_unwind(|| pacemaker_swap_inner(original_esi, player_side, actor)) {
        Ok(v) => v,
        Err(_) => original_esi,
    }
}

fn pacemaker_swap_inner(original_esi: i32, player_side: i32, actor: *mut u8) -> i32 {
    let side = player_side as u32;
    if side > 1 {
        return original_esi;
    }

    if !custom_options::is_available() {
        return original_esi;
    }

    let option_on = custom_options::get_value(side as u8, "pacemaker_to_mserror").unwrap_or(0) != 0;
    if !option_on {
        return original_esi;
    }

    // Auto-calibration: behave exactly as if the option were OFF for this
    // dispatch — stock value, no force-visible write (the ms-error readout
    // leaks the signal being calibrated). Set before the first judgment, so
    // the visibility byte is never latched during a calibration song.
    if super::calibration_suppressed() {
        return original_esi;
    }

    force_pacemaker_visible(actor);

    let ms_error = data_feed::latest_ms_error(side as usize);

    let threshold = custom_options::get_value(side as u8, "pacemaker_threshold").unwrap_or(10);
    if ms_error.unsigned_abs() < threshold as u32 {
        return 0;
    }

    ms_error
}

/// With MS ERROR on, the readout is OUR display — show it even when the
/// player has no ghost/rival data (the only stock writer of the
/// visibility byte is `GhostActor::onUpdate` on a successful download).
/// Runs only while the byte is still 0, so at most once per song per
/// side. Also re-asserts the clip's layer visibility for THIS dispatch:
/// the handler's own set-visible consumed the stale 0 just before the
/// patch site.
fn force_pacemaker_visible(actor: *mut u8) {
    if actor.is_null() {
        return;
    }
    let vtable = NOTE_RESULT_VTABLE.load(Ordering::Acquire);
    if vtable.is_null() {
        return;
    }
    unsafe {
        if memory::read_ptr(actor) as *mut u8 != vtable {
            return;
        }
        if memory::read_u8(actor.add(NOTE_RESULT_VIS_OFFSET)) != 0 {
            return;
        }
        memory::write_u8(actor.add(NOTE_RESULT_VIS_OFFSET) as *mut u8, 1);
        let clip = memory::read_ptr(actor.add(NOTE_RESULT_PACEMAKER_CLIP_OFFSET));
        if !clip.is_null() {
            let layer_id = memory::read_i32(clip.add(CLIP_LAYER_ID_OFFSET));
            if layer_id > 0 {
                bm2d_api::layer_set_attribute_raw(layer_id as u32, LAYER_ATTR_VISIBLE, 1);
            }
        }
    }
}

pub fn init(signatures: &SignatureStore) -> bool {
    let Some(addr) = signatures.get_address("pacemaker_render_input") else {
        log_warn!("pacemaker_swap: pacemaker_render_input not resolved");
        return false;
    };
    PATCH_ADDR.store(addr as *mut u8, Ordering::Release);
    match signatures.get_address("note_result_actor_vtable") {
        Some(vt) => NOTE_RESULT_VTABLE.store(vt as *mut u8, Ordering::Release),
        None => log_warn!(
            "pacemaker_swap: note_result_actor_vtable unresolved -- ms-error display stays hidden without ghost/rival data"
        ),
    }
    true
}

pub fn enable() {
    let patch_site = PATCH_ADDR.load(Ordering::Acquire);
    if patch_site.is_null() {
        return;
    }

    // Guard against re-entry: if the patch is already live, ORIGINAL_BYTES already
    // holds the real pre-patch instruction. Overwriting it with the current (JMP)
    // bytes would mean disable() later restores a broken JMP instead of the original
    // MOV RDX — permanently breaking the pacemaker render path until game restart.
    if PATCH_ACTIVE.swap(true, Ordering::AcqRel) {
        log_info!("pacemaker_swap: enable() called while already active — skipping");
        return;
    }

    unsafe {
        for i in 0..PATCH_SIZE {
            ORIGINAL_BYTES[i].store(
                memory::read_u8(patch_site.add(i) as *const u8),
                Ordering::Release,
            );
        }

        let fn_addr = pacemaker_swap_get_esi as *const () as usize;

        // The stub will end with a jmp rel32 back to patch_site+PATCH_SIZE.
        // We compute the displacement after we know the stub address, so we
        // write a placeholder here and patch it in after alloc_near succeeds.
        // Stub size budget: 1+1+1+2+2+2+2+2+2 pushes (total 17 bytes for 8 regs)
        //   + 4 sub shadow + 2 mov ecx,esi + 3 mov edx,[r14] + 3 mov r8,rdi
        //   + 10 movabs+call + 2 mov esi,eax + 4 add shadow
        //   + 2+2+2+2+1+1+1 pops + 1 pop rbx
        //   + 7 displaced insn + 5 jmp rel32 = ~78 bytes; allocate 96.
        let mut stub_bytes: Vec<u8> = Vec::with_capacity(96);

        // --- Save caller-saved registers (8 regs = 64 bytes → RSP stays 16-aligned) ---
        // push rbx first as an alignment pad (rbx is callee-saved but cheap to save).
        stub_bytes.push(0x53); // push rbx
        stub_bytes.push(0x50); // push rax
        stub_bytes.push(0x51); // push rcx
        stub_bytes.push(0x52); // push rdx
        stub_bytes.extend_from_slice(&[0x41, 0x50]); // push r8
        stub_bytes.extend_from_slice(&[0x41, 0x51]); // push r9
        stub_bytes.extend_from_slice(&[0x41, 0x52]); // push r10
        stub_bytes.extend_from_slice(&[0x41, 0x53]); // push r11
                                                     // 8 pushes × 8 = 64 bytes; if entry RSP was 16-aligned it is still 16-aligned.

        // sub rsp, 0x20  (shadow space)
        stub_bytes.extend_from_slice(&[0x48, 0x83, 0xEC, 0x20]);

        // mov ecx, esi  (arg1 = original ESI value before our override)
        stub_bytes.extend_from_slice(&[0x89, 0xF1]);

        // mov edx, [r14]  (arg2 = player_side)
        stub_bytes.extend_from_slice(&[0x41, 0x8B, 0x16]);

        // mov r8, rdi  (arg3 = NoteResultActor — RDI throughout the score
        // -render function body; RDI is callee-saved and untouched by the
        // pushes above)
        stub_bytes.extend_from_slice(&[0x4C, 0x8B, 0xC7]);

        // movabs rax, <fn_addr>
        stub_bytes.extend_from_slice(&[0x48, 0xB8]);
        stub_bytes.extend_from_slice(&fn_addr.to_le_bytes());

        // call rax
        stub_bytes.extend_from_slice(&[0xFF, 0xD0]);

        // mov esi, eax  (override ESI with our value)
        stub_bytes.extend_from_slice(&[0x89, 0xC6]);
        // mov [r14+8], eax  (also write to the struct slot so any code that
        // re-reads [R14+8] after intermediate calls also sees our value)
        stub_bytes.extend_from_slice(&[0x41, 0x89, 0x46, 0x08]);

        // add rsp, 0x20  (tear down shadow space)
        stub_bytes.extend_from_slice(&[0x48, 0x83, 0xC4, 0x20]);

        // --- Restore registers in reverse push order ---
        stub_bytes.extend_from_slice(&[0x41, 0x5B]); // pop r11
        stub_bytes.extend_from_slice(&[0x41, 0x5A]); // pop r10
        stub_bytes.extend_from_slice(&[0x41, 0x59]); // pop r9
        stub_bytes.extend_from_slice(&[0x41, 0x58]); // pop r8
        stub_bytes.push(0x5A); // pop rdx
        stub_bytes.push(0x59); // pop rcx
        stub_bytes.push(0x58); // pop rax
        stub_bytes.push(0x5B); // pop rbx  (alignment pad — restores rbx cleanly)

        // Displaced instructions (11 bytes total):
        // 1. movsxd rsi, dword [r14+8]  — re-loads RSI from our written value
        stub_bytes.extend_from_slice(&[0x49, 0x63, 0x76, 0x08]);
        // 2. mov rdx, [rdi+0xb0]
        stub_bytes.extend_from_slice(&[0x48, 0x8B, 0x97, 0xB0, 0x00, 0x00, 0x00]);

        // jmp rel32 back to patch_site+PATCH_SIZE.
        // Using jmp rel32 is safe because alloc_near guarantees the stub is
        // within ±2 GB of the patch site. This avoids touching any register
        // (the old push+ret scheme clobbered RAX after we had already popped it).
        let jmp_opcode_offset = stub_bytes.len();
        stub_bytes.push(0xE9);
        stub_bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // placeholder disp32

        let stub = memory::alloc_near(patch_site as *const u8, stub_bytes.len());
        if stub.is_null() {
            log_warn!("pacemaker_swap: alloc_near failed");
            PATCH_ACTIVE.store(false, Ordering::Release);
            return;
        }

        // Patch the jmp rel32 displacement now that we know the stub address.
        // The JMP instruction ends at stub + jmp_opcode_offset + 5.
        let jmp_insn_end = stub.add(jmp_opcode_offset + 5);
        let return_addr = patch_site.add(PATCH_SIZE);
        let jmp_disp = (return_addr as isize) - (jmp_insn_end as isize);
        let disp_bytes = (jmp_disp as i32).to_le_bytes();
        stub_bytes[jmp_opcode_offset + 1] = disp_bytes[0];
        stub_bytes[jmp_opcode_offset + 2] = disp_bytes[1];
        stub_bytes[jmp_opcode_offset + 3] = disp_bytes[2];
        stub_bytes[jmp_opcode_offset + 4] = disp_bytes[3];

        std::ptr::copy_nonoverlapping(stub_bytes.as_ptr(), stub, stub_bytes.len());
        STUB_ADDR.store(stub, Ordering::Release);

        // Write the 7-byte patch: E9 <disp32> 90 90
        // The displacement is from the end of the 5-byte JMP (patch_site+5) to stub.
        let jmp_to_stub_disp = (stub as isize) - (patch_site.add(5) as isize);
        let old_prot = memory::make_writable(patch_site as *const u8, PATCH_SIZE);
        memory::write_u8(patch_site, 0xE9);
        memory::write_i32(patch_site.add(1), jmp_to_stub_disp as i32);
        for i in 5..PATCH_SIZE {
            memory::write_u8(patch_site.add(i), 0x90);
        }
        memory::restore_protection(patch_site as *const u8, PATCH_SIZE, old_prot);

        log_info!("pacemaker_swap: enabled (stub @ {:p})", stub);
    }
}

pub fn disable() {
    let patch_site = PATCH_ADDR.load(Ordering::Acquire);
    if patch_site.is_null() {
        return;
    }

    // Clear the guard so enable() can write a fresh stub if called again later.
    if !PATCH_ACTIVE.swap(false, Ordering::AcqRel) {
        // Was not active — nothing to restore.
        return;
    }

    unsafe {
        let old_prot = memory::make_writable(patch_site as *const u8, PATCH_SIZE);
        for i in 0..PATCH_SIZE {
            memory::write_u8(patch_site.add(i), ORIGINAL_BYTES[i].load(Ordering::Acquire));
        }
        memory::restore_protection(patch_site as *const u8, PATCH_SIZE, old_prot);
    }

    log_info!("pacemaker_swap: disabled");
}
