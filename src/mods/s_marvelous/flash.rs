//! Gameplay flash re-drive: on an S-Marvelous classification, jump the
//! game's own `dance_judge` clip — which the stock 0x1028 broadcast just
//! played at `in_marvelous`, synchronously inside `judge_submit`'s original
//! — to the mod-synthesized `in_smarvelous` label, one event later in the
//! same frame (before anything renders). Design §4.4.
//!
//! No play/visibility calls: the stock handler already set them for this
//! judgement. Calibration hide and per-player judgement styling apply
//! automatically (same clip, opacity/scale operate at the layer level).
//!
//! Runs on the game thread (inside the judge_submit detour) — the capture
//! registry and every libafp call are game-thread-only, which this path
//! satisfies by construction. Panic-free, lock-free (the one mutex is
//! `bm2d_api`'s uncontended API cell, the house pattern for libafp calls).

use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use crate::mods::overlay_element_styling;
use crate::services::bm2d_api;
use crate::{log_info, log_warn};

use super::afp_patches;

/// Pool-wrapper field offsets (BM2D CMovieClip pool slot, stride 0x240):
/// +0x08 = type-1 AFP layer id, +0x110 = type-4 MC id (docs/afp_system.md
/// §6; same offsets the FC splash handler reads — display-side RE §5).
const WRAPPER_MC_ID_OFFSET: usize = 0x110;

/// The synthesized frame label (must match `assets::NEW_LABEL`).
const LABEL: &std::ffi::CStr = c"in_smarvelous";

/// One-shot WARN latches per failure class (a per-event WARN would spam at
/// judgement rate).
static WARNED_NO_CLIP: AtomicBool = AtomicBool::new(false);
static WARNED_OP_FAILED: AtomicBool = AtomicBool::new(false);
/// One-shot INFO on the first successful re-drive of the session — the
/// cabinet-log confirmation that the whole chain is live.
static FIRST_REDRIVE_LOGGED: AtomicBool = AtomicBool::new(false);

/// Clear the one-shot latches (called at GAMEPLAY entry so a transient
/// failure class can re-report on a later song during diagnosis).
pub fn reset_latches() {
    WARNED_NO_CLIP.store(false, Ordering::Relaxed);
    WARNED_OP_FAILED.store(false, Ordering::Relaxed);
}

/// `sequence::dance::NoteResultActor` vftable (RTTI-resolved at init; null
/// when unresolved). Identifies the actor in the judge actor's subtree so
/// the flash can drive the SAME wrapper the stock grade handler drives
/// (`NoteResultActor+0xA0`) instead of the captured pool wrapper — the
/// captured one proved to be an outer 1-frame instance (deploy #8/#9:
/// readback 0 both op families).
static NOTE_RESULT_VTABLE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());

pub fn set_note_result_vtable(vt: Option<*const u8>) {
    if let Some(vt) = vt {
        NOTE_RESULT_VTABLE.store(vt as *mut u8, Ordering::Release);
    }
}

/// Actor-node layout (judge_submit RE, gamemdx `FUN_18005fd30` +
/// `FUN_18022eaa0`): +0x00 vftable, +0x10 next sibling, +0x18 child list
/// head. Depth/width bounded, null-safe.
const ACTOR_CHILD_OFFSET: usize = 0x18;
const ACTOR_NEXT_OFFSET: usize = 0x10;

/// Find the NoteResultActor in `actor`'s subtree by vftable identity.
/// GAME-THREAD-ONLY, called inside the judge_submit dispatch where the
/// tree is live. Returns None when the vtable is unresolved or absent.
unsafe fn find_note_result_actor(actor: *mut u8, depth: u8) -> Option<*mut u8> {
    let want = NOTE_RESULT_VTABLE.load(Ordering::Acquire);
    if want.is_null() || actor.is_null() || depth > 4 {
        return None;
    }
    let mut child = (actor.add(ACTOR_CHILD_OFFSET) as *const *mut u8).read_unaligned();
    let mut hops = 0usize;
    while !child.is_null() && hops < 64 {
        let vt = (child as *const *mut u8).read_unaligned();
        if vt == want {
            return Some(child);
        }
        if let Some(hit) = find_note_result_actor(child, depth + 1) {
            return Some(hit);
        }
        child = (child.add(ACTOR_NEXT_OFFSET) as *const *mut u8).read_unaligned();
        hops += 1;
    }
    None
}

/// The stock grade handler's wrapper: `NoteResultActor+0xA0` (the actor's
/// stored dance_judge CMovieClip wrapper — gamemdx `FUN_18007b300` 0x1028
/// case drives exactly this one).
const NOTE_RESULT_JUDGE_WRAPPER_OFFSET: usize = 0xA0;

/// Re-drive the side's judgement clip to `in_smarvelous`. Called from the
/// judge tap when an event classified S-Marvelous (armed sides only — the
/// caller's classification return gates this). `judge_actor` = the
/// judge_submit dispatch actor (the NoteResultActor lives in its subtree).
pub fn on_smarvelous(side: usize, judge_actor: *mut u8) {
    let nra = unsafe { find_note_result_actor(judge_actor, 0) };

    // S-Marvelous is the highest tier ⇒ exempt from FAST/SLOW: re-hide the
    // indicator the patched gate just showed for this grade-0 event.
    // Independent of the word re-drive below (which needs the patched
    // template) — the hide is correct even when the word shows stock.
    super::fast_slow::hide_for_smarvelous(nra);

    // Without the patched template the label does not exist — a goto would
    // be a benign no-op, but skipping keeps the fail-open contract exact
    // (stock word shows, one WARN came from the patch layer already).
    if !afp_patches::patch_applied() {
        return;
    }
    // Preferred target: the actor's OWN stored wrapper — the exact object
    // the stock grade handler just drove for this event. Fallback: the
    // captured pool wrapper (known-wrong outer instance, kept only so a
    // failed walk still logs through the old path).
    let actor_wrapper = unsafe {
        nra.and_then(|nra| {
            let w = (nra.add(NOTE_RESULT_JUDGE_WRAPPER_OFFSET) as *const *mut u8).read_unaligned();
            if w.is_null() {
                None
            } else {
                Some(w)
            }
        })
    };
    let (wrapper, via_actor) = match actor_wrapper {
        Some(w) => (w, true),
        None => match overlay_element_styling::judge_clip(side) {
            Some(w) => (w, false),
            None => {
                if !WARNED_NO_CLIP.swap(true, Ordering::Relaxed) {
                    log_warn!(
                        "SMarvelous: flash — no NoteResultActor wrapper and no captured clip (side {}); stock word shows",
                        side
                    );
                }
                return;
            }
        },
    };
    let mc_id = unsafe { (wrapper.add(WRAPPER_MC_ID_OFFSET) as *const u32).read_unaligned() };
    if mc_id < 1 {
        if !WARNED_NO_CLIP.swap(true, Ordering::Relaxed) {
            log_warn!("SMarvelous: flash — captured clip has no MC id; stock word shows");
        }
        return;
    }
    // Drive with the LABEL op (0xF09): its handler auto-redirects to the
    // clip's `aep_dummy` child timeline when one exists (libafp
    // `afp_mc_op` case 0xF09: flag 0x4000000 + name gate) and resolves the
    // label against THAT timeline's own table/frame numbering. This is the
    // op that matches the template's real structure — the visible word
    // timeline is the inner sprite section (own labels, own numbering),
    // which the multi-section clone now patches too (deploys #6–#8 story:
    // root-only clone + root-frame seeks = blank). The 0x1012 pre-check
    // stays for observability (0xF09 swallows lookup failures).
    if bm2d_api::mc_frame_by_label(mc_id, LABEL).is_none()
        && !WARNED_OP_FAILED.swap(true, Ordering::Relaxed)
    {
        log_warn!(
            "SMarvelous: flash — label lookup failed on the captured clip (mc {}); attempting 0xF09 anyway",
            mc_id
        );
    }
    if bm2d_api::mc_op_str(mc_id, 0xF09, LABEL) {
        if !FIRST_REDRIVE_LOGGED.swap(true, Ordering::Relaxed) {
            log_info!(
                "SMarvelous: flash live — first in_smarvelous re-drive (side {}, via_actor {})",
                side,
                via_actor
            );
        }
    } else if !WARNED_OP_FAILED.swap(true, Ordering::Relaxed) {
        log_warn!("SMarvelous: flash — mc_op(0xF09) refused; stock word shows");
    }
}
