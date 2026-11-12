//! Stage-save skip for injected songs.
//!
//! After each song the game's `SavePlayerDataActor` asks for a per-stage
//! save: `stage_save_request(side, stage)` marshals the stage record into
//! the save staging buffer (`ReflectSavePlayerData`, savekind 2) and arms
//! the side's save state machine; the next update hands the staging buffer
//! to the ark library, which copies it into a job for the ess sender.
//!
//! For an injected song that work is wasted — the save is suppressed in the
//! ess sender anyway (`score_guard`) — and for a long chart it is fatal.
//! The record's ghost stream holds one byte per note of the chart, and both
//! copies assume it fits the 8,192-character wire field: the marshal writes
//! every byte into a 0x2004-byte staging field with no bound, and the ark
//! copy is a `strcpy_s(dst, 0x2001, …)` whose overrun raises the CRT
//! invalid-parameter fault (`0xC0000417`) — the game dies on the way to the
//! results screen, before the sender is reached. No stock chart comes near
//! that many notes; a StepMania marathon does.
//!
//! So the request is never made for an injected song. The detour returns
//! the request's own success value without calling it, which leaves the
//! side's save state at 0 (idle); the actor's poll reports "finished,
//! success" for an idle side — the same outputs a completed save produces
//! — and the actor moves on. Nothing is marshalled and no job is created.
//! The session taint the sender's suppression would have latched is latched
//! here instead (it is also set at gameplay load, `taint.rs`), so the
//! card-out save is sanitised exactly as before.
//!
//! RE: `docs/stepmania_runtime_packs_feasibility.md` §20. Fail-open on the
//! hook itself: an unresolved signature or a failed install leaves the stock
//! request in place (one WARN) — saves stay suppressed by the sender, but a
//! chart over 8,192 notes crashes at the end of the song.

use std::ptr::addr_of;
use std::sync::atomic::{AtomicBool, Ordering};

use retour::GenericDetour;

use crate::core::hooks;
use crate::core::signatures::SignatureStore;
use crate::services::score_guard;
use crate::{log_info, log_warn};

/// `bool request(int side, int stage)`.
type RequestFn = unsafe extern "C" fn(i32, i32) -> u8;

static mut REQUEST_HOOK: Option<GenericDetour<RequestFn>> = None;
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Install the detour (mod `init`, once). `false` when the signature is
/// unresolved or the install failed.
pub fn install(signatures: &SignatureStore) -> bool {
    if INSTALLED.load(Ordering::Acquire) {
        return true;
    }
    let Some(target) = signatures.get_address("stage_save_request") else {
        log_warn!(
            "StepMania: stage_save_request unresolved -- a chart over 8,192 notes will crash the game at the end of the song"
        );
        return false;
    };
    // SAFETY: `target` is the matched function entry; the storage is a
    // process-lifetime static only written here, from the init thread.
    unsafe {
        let t: RequestFn = std::mem::transmute(target);
        if let Err(e) =
            hooks::install_enabled(std::ptr::addr_of_mut!(REQUEST_HOOK), t, request_hook)
        {
            log_warn!(
                "StepMania: stage_save_request detour failed ({:?}) -- a chart over 8,192 notes will crash the game at the end of the song",
                e
            );
            return false;
        }
    }
    INSTALLED.store(true, Ordering::Release);
    log_info!(
        "StepMania: stage-save skip installed @ {:p} (injected songs never request a stage save)",
        target
    );
    true
}

unsafe extern "C" fn request_hook(side: i32, stage: i32) -> u8 {
    let Some(hook) = (*addr_of!(REQUEST_HOOK)).as_ref() else {
        return 0;
    };
    if !score_guard::is_injected_song_tainted() {
        return hook.call(side, stage);
    }
    if let Ok(side) = usize::try_from(side) {
        score_guard::mark_session_tainted(side);
    }
    log_info!(
        "StepMania: P{} stage {} save request skipped (injected song)",
        side.wrapping_add(1),
        stage
    );
    1
}
