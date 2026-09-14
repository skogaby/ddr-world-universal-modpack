//! Extra-stage guard (design §4.9, R10 / D21).
//!
//! The game's extra-stage grant (`extra_stage_grant`, A.4) runs at the
//! results window-out of stage 0 and requires EVERY side with
//! `PlayerWork+0x4 != 0` to have AAA'd — inside the bot's play window the
//! bot side is "entered", so a low-level bot that did not AAA would block the
//! human's extra stage. This ONE `GenericDetour` clears the bot's entered
//! byte around the original while an impersonation is active and puts it
//! back afterwards (a scope guard, so an unwinding original cannot leave it
//! cleared). The bot can never ADD a grant — the human must still pass.
//!
//! Fail-open: a missing AOB (soft `get_address` consumer) or a failed
//! install leaves the stock rule in place with one WARN; the mod still
//! enables. `disable` is a passthrough flag — the detour is never
//! uninstalled (one detour per target, never removed at runtime).

use std::ptr::addr_of;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use retour::GenericDetour;

use super::impersonation;
use crate::core::hooks;
use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::services::stage_records;
use crate::{log_info, log_warn};

type GrantFn = unsafe extern "C" fn(i32);

static mut GRANT_HOOK: Option<GenericDetour<GrantFn>> = None;
/// Resolved `extra_stage_grant` entry (null = signature missing).
static TARGET: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static INSTALLED: AtomicBool = AtomicBool::new(false);
/// Passthrough flag (mod disabled ⇒ the original always runs untouched).
static ENABLED: AtomicBool = AtomicBool::new(false);

const PW_ENTERED: usize = 0x4;
const PW_PROBE_LEN: usize = 0x8;

/// Resolve the detour target. `false` (+ one WARN) when the signature is
/// missing — the caller still enables the mod.
pub fn init(signatures: &SignatureStore) -> bool {
    match signatures.get_address("extra_stage_grant") {
        Some(addr) if !addr.is_null() => {
            TARGET.store(addr as *mut u8, Ordering::Release);
            true
        }
        _ => {
            log_warn!(
                "MultiplayerBot: extra_stage_grant signature missing -- the extra-stage grant will consider the bot (stock rule)"
            );
            false
        }
    }
}

/// Arm the guard; installs the detour on the first call.
pub fn enable() {
    ENABLED.store(true, Ordering::Release);
    if INSTALLED.load(Ordering::Acquire) {
        return;
    }
    let target = TARGET.load(Ordering::Acquire);
    if target.is_null() {
        return; // init already WARNed
    }
    let target_fn: GrantFn = unsafe { std::mem::transmute(target) };
    match unsafe {
        hooks::install_enabled(std::ptr::addr_of_mut!(GRANT_HOOK), target_fn, grant_hook)
    } {
        Ok(()) => {
            INSTALLED.store(true, Ordering::Release);
            log_info!("MultiplayerBot: extra-stage guard installed");
        }
        Err(e) => {
            log_warn!(
                "MultiplayerBot: extra-stage guard install failed: {} -- the extra-stage grant will consider the bot (stock rule)",
                e
            );
        }
    }
}

/// Passthrough (the detour stays installed).
pub fn disable() {
    ENABLED.store(false, Ordering::Release);
}

pub fn is_installed() -> bool {
    INSTALLED.load(Ordering::Acquire)
}

/// Writes the saved entered byte back on drop.
struct EnteredRestore {
    slot: *mut u8,
    value: u8,
}

impl Drop for EnteredRestore {
    fn drop(&mut self) {
        unsafe { memory::write_u8(self.slot, self.value) };
    }
}

unsafe extern "C" fn grant_hook(arg: i32) {
    let Some(hook) = (&*addr_of!(GRANT_HOOK)).as_ref() else {
        return;
    };
    // Panic-free: atomics + guarded pointer reads only.
    let bot = if ENABLED.load(Ordering::Acquire) {
        impersonation::active_bot_side()
    } else {
        None
    };
    let slot = bot
        .and_then(stage_records::player_work)
        .filter(|pw| memory::is_readable(*pw, PW_PROBE_LEN))
        .map(|pw| pw.add(PW_ENTERED))
        .filter(|slot| memory::read_u8(*slot) != 0);
    match (bot, slot) {
        (Some(side), Some(slot)) => {
            let value = memory::read_u8(slot);
            memory::write_u8(slot, 0);
            let _restore = EnteredRestore { slot, value };
            log_info!(
                "MultiplayerBot: extra-stage grant evaluated without the bot (side {})",
                side
            );
            hook.call(arg);
        }
        _ => hook.call(arg),
    }
}
