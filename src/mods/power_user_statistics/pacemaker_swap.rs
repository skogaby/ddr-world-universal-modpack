//! Pacemaker → MsError swap — when the per-player option is ON, replaces
//! the pacemaker score-delta readout with the most recent ms-error, and
//! forces the white color when |error| < threshold.
//!
//! Patches the 7-byte `MOV RDX, [RDI+0xB0]` instruction inside the
//! pacemaker render case (opcode 0x1036) of the score-render function.
//! A JMP to a hand-assembled stub overrides ESI (the formatter input)
//! with our ms-error value before proceeding.
//!
//! # White-zone color (decoupled from the value, 2026-08-31)
//!
//! ESI feeds BOTH the digit formatter and the downstream color branch
//! (`TEST ESI,ESI` — zero takes the white path, nonzero picks the
//! plus/minus color). The original hex-edit mod forced the flags at the
//! TEST site so the REAL digits rendered in white; the first port of this
//! mod instead zeroed ESI, which also blanked the displayed number for
//! the whole white zone (tester-reported 2026-08-31). The color is now
//! forced without touching the value: both colored paths load their 0.5
//! color component RIP-relative from one shared constant
//! (`MOVSS xmm,[rip]` reading 0.5f — twice, once per sign path; byte
//! shape identical on 20250805/20260616/20260721). At enable both
//! disp32s are redirected (cull_window-style content-verified rewrite)
//! at a mod-owned float co-located with the stub allocation; the
//! callback writes it per dispatch — 0.5 = stock colors, 1.0 degenerates
//! both colored paths to (1,1,1,1) = the white branch's exact SetColor.
//! Sign choice and sign-slot placement keep using the REAL value, so a
//! white-zone readout is correct digits + correct sign in white.
//!
//! Fail-open: if the two 0.5 loads can't be derived, the callback falls
//! back to the legacy value-zeroing white zone (one WARN).
//!
//! # Exact-0 digit render (RESOLVED 2026-09-01)
//!
//! For a long time a value of exactly 0 rendered sign-only (the `±`
//! glyph with no `0` digit). Root cause was NOT this mod: the
//! real-speed-fix mod's ported "logf guard" (R15/R16 from the original
//! hex-edit modpack) anchored on an AOB that actually matches inside
//! THIS function's 0x1036 case, and its R15 byte rewrote the zero
//! branch's `LEA R13D,[RSI+1]; JMP +0x48` to `JMP +0x37` — rerouting
//! exact-0 dispatches through the log10f path with a STALE XMM6 (only
//! the nonzero branch loads it), so the sign-slot index collapsed to the
//! ONES slot and the sign overwrote the digit (the sign loop runs after
//! the digit formatter). Live-confirmed via CE register captures
//! (R13D=0/R9D=1 at the sprintf despite the LEA setting R13D=1) and the
//! runtime byte read `EB 37` where the image has `EB 48`. Fixed by
//! retiring the logf guard outright (see `mods/real_speed_fix/mod.rs`);
//! stock's zero branch needs no help (`R13D=1 → powf(10,1)=10` → sign at
//! the tens slot, digit at ones).
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
/// CMovieClip wrapper: AFP MovieClip id (the engine's own case-0x1032
/// SetFrame reads it there; the digit formatter receives `wrapper+0x10`
/// and reads the id at its +0x100 = this offset).
const CLIP_MC_ID_OFFSET: usize = 0x110;
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

// ── White-zone color redirect state ──────────────────────────────────
/// Mod-owned f32 the colored branch's two 0.5f loads are redirected at
/// (co-located in the stub allocation). 0.5 = stock colors, 1.0 = white.
static COLOR_SLOT: AtomicPtr<f32> = AtomicPtr::new(std::ptr::null_mut());
/// The two patched disp32 addresses (instruction+4) + their original
/// displacements, for the disable() restore.
static COLOR_DISP_ADDRS: [AtomicPtr<u8>; 2] = [
    AtomicPtr::new(std::ptr::null_mut()),
    AtomicPtr::new(std::ptr::null_mut()),
];
static COLOR_ORIG_DISPS: [std::sync::atomic::AtomicI32; 2] = [
    std::sync::atomic::AtomicI32::new(0),
    std::sync::atomic::AtomicI32::new(0),
];
/// True while the two color disp32 rewrites are live. When false the
/// callback falls back to the legacy value-zeroing white zone.
static COLOR_PATCHED: AtomicBool = AtomicBool::new(false);

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
/// If ON, returns the REAL ms-error (never zeroed — the white-zone color
/// is forced through the redirected color loads instead), and forces the
/// pacemaker's visibility for sides with no ghost/rival data (see the
/// module doc). `actor` = the NoteResultActor (RDI at the patch site).
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

/// Write the white-force float consumed by the redirected color loads
/// downstream in the SAME dispatch (same thread — plain program order).
fn set_white_force(white: bool) {
    let slot = COLOR_SLOT.load(Ordering::Acquire);
    if !slot.is_null() {
        unsafe { *slot = if white { 1.0 } else { 0.5 } };
    }
}

fn pacemaker_swap_inner(original_esi: i32, player_side: i32, actor: *mut u8) -> i32 {
    let color_live = COLOR_PATCHED.load(Ordering::Acquire);

    let side = player_side as u32;
    if side > 1 {
        set_white_force(false);
        return original_esi;
    }

    if !custom_options::is_available() {
        set_white_force(false);
        return original_esi;
    }

    let option_on = custom_options::get_value(side as u8, "pacemaker_to_mserror").unwrap_or(0) != 0;
    if !option_on {
        set_white_force(false);
        return original_esi;
    }

    // Auto-calibration: behave exactly as if the option were OFF for this
    // dispatch — stock value, no force-visible write (the ms-error readout
    // leaks the signal being calibrated). Set before the first judgment, so
    // the visibility byte is never latched during a calibration song.
    if super::calibration_suppressed() {
        set_white_force(false);
        return original_esi;
    }

    force_pacemaker_visible(actor);

    let ms_error = data_feed::latest_ms_error(side as usize);

    let threshold = custom_options::get_value(side as u8, "pacemaker_threshold")
        .unwrap_or(10)
        .max(0);
    let in_white_zone = threshold > 0 && ms_error.unsigned_abs() < threshold as u32;

    if color_live {
        set_white_force(in_white_zone);
        ms_error
    } else if in_white_zone {
        // Legacy fallback (color loads underivable): zeroing ESI is the
        // only way to reach the stock white branch, at the cost of also
        // blanking the displayed number for the whole white zone.
        0
    } else {
        ms_error
    }
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

/// Locate the colored branch's two `MOVSS xmm, [rip+disp32]` loads whose
/// current target reads exactly 0.5f, and redirect both displacements at
/// `slot`. Content-identified within a bounded window past the patch site
/// (register allocation may differ across builds, the loaded VALUE cannot).
/// Requires EXACTLY two matches; any refusal patches nothing and returns
/// false (the callback keeps the legacy value-zeroing white zone).
unsafe fn install_color_patch(patch_site: *const u8, slot: *mut f32) -> bool {
    /// Scan window past the patch site (the color branch sits ~0x80 bytes
    /// in on all attested builds; the next unrelated MOVSS rip-load past
    /// the sign-placement code is well outside 0x100).
    const WINDOW: usize = 0x100;

    // RIP targets are verified to sit inside the module image before the
    // 0.5f content read (a junk match from mid-instruction bytes could
    // otherwise point at unmapped memory).
    let Some(module) = crate::core::module_resolver::get_game_module() else {
        return false;
    };
    let mod_lo = module.base as usize;
    let mod_hi = mod_lo + module.size;

    // (disp32 address, original displacement, instruction end)
    let mut hits: Vec<(*mut u8, i32, *const u8)> = Vec::new();
    let start = patch_site.add(PATCH_SIZE);
    let mut i = 0usize;
    while i + 9 <= WINDOW {
        let p = start.add(i);
        // MOVSS xmm, m32: F3 [44] 0F 10 modrm; RIP form = mod 00, rm 101.
        let (modrm_off, len) = if memory::read_u8(p) == 0xF3
            && memory::read_u8(p.add(1)) == 0x0F
            && memory::read_u8(p.add(2)) == 0x10
        {
            (3usize, 8usize)
        } else if memory::read_u8(p) == 0xF3
            && memory::read_u8(p.add(1)) == 0x44
            && memory::read_u8(p.add(2)) == 0x0F
            && memory::read_u8(p.add(3)) == 0x10
        {
            (4usize, 9usize)
        } else {
            i += 1;
            continue;
        };
        if memory::read_u8(p.add(modrm_off)) & 0xC7 != 0x05 {
            i += 1;
            continue;
        }
        let disp_addr = p.add(modrm_off + 1);
        let insn_end = p.add(len);
        let target = crate::core::scanner::decode_rip_relative(disp_addr) as usize;
        if target >= mod_lo && target + 4 <= mod_hi && memory::read_f32(target as *const u8) == 0.5
        {
            hits.push((disp_addr as *mut u8, memory::read_i32(disp_addr), insn_end));
        }
        i += len;
    }

    if hits.len() != 2 {
        log_warn!(
            "pacemaker_swap: expected exactly 2 white-zone color loads, found {}",
            hits.len()
        );
        return false;
    }
    // Both new displacements must be RIP-reachable (guaranteed in practice:
    // the slot lives in the alloc_near stub block).
    for (_, _, insn_end) in &hits {
        let disp = slot as i64 - *insn_end as i64;
        if disp > i32::MAX as i64 || disp < i32::MIN as i64 {
            log_warn!("pacemaker_swap: color slot not RIP-reachable");
            return false;
        }
    }

    for (idx, (disp_addr, orig, insn_end)) in hits.iter().enumerate() {
        let disp = (slot as i64 - *insn_end as i64) as i32;
        let old = memory::make_writable(*disp_addr, 4);
        memory::write_i32(*disp_addr, disp);
        memory::restore_protection(*disp_addr, 4, old);
        COLOR_DISP_ADDRS[idx].store(*disp_addr, Ordering::Release);
        COLOR_ORIG_DISPS[idx].store(*orig, Ordering::Release);
    }
    COLOR_PATCHED.store(true, Ordering::Release);
    true
}

/// Restore the two color-load displacements written by
/// [`install_color_patch`]. No-op when the patch was never installed.
unsafe fn remove_color_patch() {
    if !COLOR_PATCHED.swap(false, Ordering::AcqRel) {
        return;
    }
    for idx in 0..2 {
        let disp_addr = COLOR_DISP_ADDRS[idx].load(Ordering::Acquire);
        if disp_addr.is_null() {
            continue;
        }
        let old = memory::make_writable(disp_addr, 4);
        memory::write_i32(disp_addr, COLOR_ORIG_DISPS[idx].load(Ordering::Acquire));
        memory::restore_protection(disp_addr, 4, old);
        COLOR_DISP_ADDRS[idx].store(std::ptr::null_mut(), Ordering::Release);
    }
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

        // Allocate the stub with 8 spare bytes: the white-force float slot
        // is carved from the tail of the same near allocation (RIP-reachable
        // from the color-branch loads, which sit ~0x80 bytes past the patch
        // site).
        let stub = memory::alloc_near(patch_site as *const u8, stub_bytes.len() + 8);
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

        // Carve the white-force float slot from the allocation tail (4-byte
        // aligned) and initialize it STOCK (0.5) before any load can be
        // redirected at it.
        let slot = stub.add((stub_bytes.len() + 3) & !3) as *mut f32;
        *slot = 0.5;
        COLOR_SLOT.store(slot, Ordering::Release);

        // Redirect the colored branch's two 0.5f component loads at the
        // slot (fail-open: the callback falls back to value-zeroing).
        if install_color_patch(patch_site as *const u8, slot) {
            log_info!("pacemaker_swap: white-zone color loads redirected (slot @ {slot:p})");
        } else {
            log_warn!(
                "pacemaker_swap: color loads not derived -- white zone falls back to value zeroing"
            );
        }

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
        // Restore the color-load displacements FIRST (while they still
        // point at our slot the callback keeps it stock-correct), then the
        // main patch.
        remove_color_patch();
        COLOR_SLOT.store(std::ptr::null_mut(), Ordering::Release);

        let old_prot = memory::make_writable(patch_site as *const u8, PATCH_SIZE);
        for i in 0..PATCH_SIZE {
            memory::write_u8(patch_site.add(i), ORIGINAL_BYTES[i].load(Ordering::Acquire));
        }
        memory::restore_protection(patch_site as *const u8, PATCH_SIZE, old_prot);
    }

    log_info!("pacemaker_swap: disabled");
}
