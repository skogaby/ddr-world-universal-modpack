//! Gameplay FAST/SLOW indicator for Marvelous judgements (2026-09-01
//! directive): the HIGHEST judgement tier is exempt from FAST/SLOW. Stock,
//! that tier is Marvelous; with the mod enabled it is S-Marvelous — so a
//! non-S Marvelous shows the stock `dance_fast_slow` FAST/SLOW word like
//! every lower grade (the player sees which way it missed the tighter
//! window), while an S-Marvelous stays clean.
//!
//! Stock gate (NoteResultActor msg handler, grade case 0x1028..0x102D —
//! `docs/s_marvelous_judgement_research.md` §3.2): after driving the
//! judgement word, the fast/slow clip is HIDDEN when `delta == 0 ||
//! grade == 0`, else shown at `in_fast`/`in_slow` and repositioned. The
//! `grade == 0` test is the only thing excluding Marvelous — the ms delta
//! (this+0x98) is already stored for every grade.
//!
//! Mechanism, two halves:
//! 1. ONE byte in the gate: the second compare is `CMP dword [RDI+0x94],
//!    imm8 0`; the mod rewrites the imm8 to `-1` (0xFF). Grade is 0..=5
//!    inside this branch, so the compare never holds and its JZ is never
//!    taken — the show path runs for every Marvelous. The `delta == 0` hide
//!    stays. Restored to `0` on disable. A single aligned byte store inside
//!    an instruction that executes once per judgement: either value keeps
//!    the instruction well-formed, so no thread suspension is needed (the
//!    anytime-speedmod pattern).
//! 2. S-Marvelous re-hide: the gate cannot know about S-Marvelous (a
//!    display-layer notion), so the flash re-drive — which already runs
//!    post-original on every S-Marv event with the NoteResultActor in hand
//!    — clears the clip's visibility bit again ([`hide_for_smarvelous`]),
//!    one event later in the same frame, before anything renders.
//!
//! Cabinet-wide (the handler is shared by both sides' actors) and respects
//! the per-player FAST/SLOW option: the clip at this+0xA8 is only created
//! when that option is on — null clip ⇒ nothing to show, unchanged.
//!
//! Fail-open: no/ambiguous signature match or a non-stock imm8 read-back ⇒
//! no patch, one WARN, stock behaviour.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::services::bm2d_api;
use crate::{log_info, log_warn};

/// Offset of the grade-compare imm8 within the `note_result_fast_slow_gate`
/// match (`83 BF 98 00 00 00 00 74 ?? 83 BF 94 00 00 00 00 | 00 | 74 ??`).
const IMM_OFFSET: usize = 15;
/// Stock imm8: `grade == 0` (Marvelous) hides the indicator.
const STOCK_IMM: u8 = 0x00;
/// Patched imm8: `grade == -1` never holds for grades 0..=5.
const NEVER_IMM: u8 = 0xFF;

/// Address of the imm8 byte; 0 = unresolved (patch inert).
static IMM_ADDR: AtomicUsize = AtomicUsize::new(0);

/// True while the gate is patched (Marvelous shows FAST/SLOW). The flash
/// re-drive consults this to hide the indicator again for S-Marvelous
/// events — the highest tier stays exempt, exactly as stock Marvelous was.
static PATCHED: AtomicBool = AtomicBool::new(false);

pub fn is_active() -> bool {
    PATCHED.load(Ordering::Acquire)
}

/// Resolve the gate (mod init). Requires exactly one match and the stock
/// imm8 in place; anything else leaves the patch inert with one WARN.
pub fn install(signatures: &SignatureStore) -> bool {
    let matches = signatures.get_all_matches("note_result_fast_slow_gate");
    if matches.len() != 1 {
        log_warn!(
            "SMarvelous: expected exactly 1 fast/slow gate match, found {} -- Marvelous FAST/SLOW stays stock",
            matches.len()
        );
        return false;
    }
    let imm = unsafe { matches[0].add(IMM_OFFSET) };
    let current = unsafe { memory::read_u8(imm) };
    if current != STOCK_IMM {
        log_warn!(
            "SMarvelous: fast/slow gate imm8 is {:#04x}, not stock 0 -- Marvelous FAST/SLOW stays stock",
            current
        );
        return false;
    }
    IMM_ADDR.store(imm as usize, Ordering::Release);
    log_info!("SMarvelous: fast/slow gate resolved @ {:p}", imm);
    true
}

/// Rewrite the imm8 with page protection handled.
unsafe fn write_imm(value: u8) -> bool {
    let addr = IMM_ADDR.load(Ordering::Acquire);
    if addr == 0 {
        return false;
    }
    let p = addr as *mut u8;
    let old = memory::make_writable(p as *const u8, 1);
    memory::write_u8(p, value);
    memory::restore_protection(p as *const u8, 1, old);
    true
}

/// Mod enable: Marvelous shows FAST/SLOW.
pub fn activate() {
    if unsafe { write_imm(NEVER_IMM) } {
        PATCHED.store(true, Ordering::Release);
        log_info!("SMarvelous: Marvelous FAST/SLOW indicator enabled");
    }
}

/// Mod disable: stock gate (Marvelous never shows FAST/SLOW).
pub fn deactivate() {
    PATCHED.store(false, Ordering::Release);
    if unsafe { write_imm(STOCK_IMM) } {
        log_info!("SMarvelous: Marvelous FAST/SLOW indicator restored to stock");
    }
}

/// NoteResultActor field: the `dance_fast_slow` CMovieClip wrapper (research
/// §3.2; null when the player's FAST/SLOW option is off).
const NOTE_RESULT_FAST_SLOW_WRAPPER_OFFSET: usize = 0xA8;

/// Post-original, on an S-MARVELOUS event: re-hide the indicator the
/// patched gate just showed. The stock hide branch is `play` +
/// `set_attribute(visible, 0)`; the play already ran in the show branch,
/// so only the visibility write is needed. `note_result_actor` = the side's
/// NoteResultActor (resolved by the flash re-drive). Silent no-op when the
/// patch is inactive, the actor is unknown, or the clip is absent.
pub fn hide_for_smarvelous(note_result_actor: Option<*mut u8>) {
    if !is_active() {
        return;
    }
    let Some(nra) = note_result_actor else {
        return;
    };
    let layer_id = unsafe {
        let wrapper =
            (nra.add(NOTE_RESULT_FAST_SLOW_WRAPPER_OFFSET) as *const *const u8).read_unaligned();
        if wrapper.is_null() {
            return;
        }
        (wrapper.add(0x08) as *const u32).read_unaligned()
    };
    if layer_id == 0 {
        return;
    }
    let _ = bm2d_api::layer_set_attribute_raw(layer_id, 0x1, 0x0);
}
