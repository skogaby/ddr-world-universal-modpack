//! Call-voice hooks — the ONE owner of the detour on World's gameplay
//! announcer, `sequence::dance::CallVoiceActor::onUpdate` (vtable slot 6 =
//! the `announcer_dispatcher` AOB; `derive_call_voice`).
//!
//! Subscribers (one slot each, plain `fn` pointers in atomics — the detour
//! runs every gameplay frame and takes no lock):
//!
//! * **mute** (announcer_mute) — `fn() -> bool`; `true` ⇒ the frame is skipped
//!   entirely (World's announcer AND any override: a muted announcer is
//!   silent on every skin);
//! * **override** (ddr_selection's A3 announcer rules) — `fn(actor) -> bool`;
//!   `true` ⇒ handled, World's `onUpdate` does not run.
//!
//! Otherwise the original runs. The detour installs on the first
//! [`acquire`] and stays for the session. Also exposes the game's own
//! voice-guard primitive ([`cue_is_playing`], the call World's guard makes
//! under the AVS lock). Game thread only.

use std::ptr::addr_of;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use retour::GenericDetour;

use crate::core::hooks;
use crate::core::memory;
use crate::core::signatures::{CallVoiceSites, SignatureStore};
use crate::{log_info, log_warn};

pub type MuteFn = fn() -> bool;
pub type OverrideFn = fn(*mut u8) -> bool;

type UpdateRawFn = unsafe extern "C" fn(*mut u8);
type IsPlayingFn = unsafe extern "C" fn(usize, u32) -> u8;
type LockFn = unsafe extern "C" fn(i32);

static mut HOOK: Option<GenericDetour<UpdateRawFn>> = None;

struct Sites(CallVoiceSites);
unsafe impl Send for Sites {}
unsafe impl Sync for Sites {}

static SITES: OnceLock<Sites> = OnceLock::new();
static INSTALLED: AtomicBool = AtomicBool::new(false);
static INSTALL_LOCK: Mutex<()> = Mutex::new(());
static MUTE: AtomicUsize = AtomicUsize::new(0);
static OVERRIDE: AtomicUsize = AtomicUsize::new(0);

/// Resolve the sites (lib.rs service init). Installs nothing.
pub fn init(signatures: &SignatureStore) -> bool {
    match signatures.call_voice_sites() {
        Some(s) => {
            let _ = SITES.set(Sites(s));
            log_info!("CallVoiceHooks: announcer onUpdate + voice guard resolved");
            true
        }
        None => {
            log_warn!("CallVoiceHooks: CallVoiceActor unresolved -- no announcer hooks");
            false
        }
    }
}

pub fn is_available() -> bool {
    SITES.get().is_some()
}

/// Install the detour (idempotent). `false` when unresolved or it failed.
pub fn acquire() -> bool {
    if INSTALLED.load(Ordering::Acquire) {
        return true;
    }
    let Some(s) = SITES.get() else {
        return false;
    };
    let _g = INSTALL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if INSTALLED.load(Ordering::Acquire) {
        return true;
    }
    let target: UpdateRawFn = unsafe { std::mem::transmute(s.0.update) };
    match unsafe { hooks::install_enabled(std::ptr::addr_of_mut!(HOOK), target, update_hook) } {
        Ok(()) => {
            INSTALLED.store(true, Ordering::Release);
            log_info!("CallVoiceHooks: announcer onUpdate detour installed");
            true
        }
        Err(e) => {
            log_warn!("CallVoiceHooks: announcer onUpdate detour failed: {}", e);
            false
        }
    }
}

/// Set (or clear with `None`) the mute predicate.
pub fn set_mute(f: Option<MuteFn>) {
    MUTE.store(f.map(|f| f as usize).unwrap_or(0), Ordering::Release);
}

/// Set (or clear with `None`) the override.
pub fn set_override(f: Option<OverrideFn>) {
    OVERRIDE.store(f.map(|f| f as usize).unwrap_or(0), Ordering::Release);
}

unsafe extern "C" fn update_hook(actor: *mut u8) {
    let Some(hook) = (*addr_of!(HOOK)).as_ref() else {
        return;
    };
    let mute = MUTE.load(Ordering::Acquire);
    if mute != 0 {
        let f: MuteFn = std::mem::transmute::<usize, MuteFn>(mute);
        if std::panic::catch_unwind(f).unwrap_or(false) {
            return;
        }
    }
    let ov = OVERRIDE.load(Ordering::Acquire);
    if ov != 0 && !actor.is_null() {
        let f: OverrideFn = std::mem::transmute::<usize, OverrideFn>(ov);
        if std::panic::catch_unwind(|| f(actor)).unwrap_or(false) {
            return;
        }
    }
    hook.call(actor);
}

/// Whether the cue `handle` (a handle the game's `se_play` returned) is
/// still playing — World's voice guard, verbatim: the AVS lock around the
/// game's `is_playing(manager, handle)`. `false` for `u32::MAX`, before
/// init, or with no audio manager. Game thread.
pub fn cue_is_playing(handle: u32) -> bool {
    if handle == u32::MAX {
        return false;
    }
    let Some(s) = SITES.get() else {
        return false;
    };
    let s = &s.0;
    unsafe {
        let mgr = memory::read_ptr(s.audio_manager_global);
        if mgr.is_null() {
            return false;
        }
        let lock: LockFn = std::mem::transmute(memory::read_ptr(s.lock_iat));
        let unlock: LockFn = std::mem::transmute(memory::read_ptr(s.unlock_iat));
        let is_playing: IsPlayingFn = std::mem::transmute(s.is_playing);
        let id = memory::read_i32(s.lock_count);
        if id > 0 {
            lock(id);
        }
        let r = is_playing(mgr as usize, handle) != 0;
        let id = memory::read_i32(s.lock_count);
        if id > 0 {
            unlock(id);
        }
        r
    }
}
