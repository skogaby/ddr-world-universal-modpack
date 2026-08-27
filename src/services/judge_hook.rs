//! Judge Hook Service — Shared dispatcher for `GamePlayActor::judgeNotes`.
//!
//! Installs exactly one `retour::GenericDetour` on `judgeNotes`. Mods
//! register pre-judge and post-judge callbacks with a priority. The
//! detour dispatches:
//!
//!   1. All pre-judge callbacks in ascending priority order
//!   2. The original `judgeNotes` function
//!   3. All post-judge callbacks in ascending priority order
//!
//! Within the same priority, callbacks fire in registration order. Do NOT
//! rely on that — use a distinct priority if ordering matters.
//!
//! Callbacks are plain `fn` pointers (not closures) because they are invoked
//! from within an `extern "C"` detour callback and must not require captured
//! state. Per-subscriber state lives in the subscriber's own `static mut`
//! slots; the subscriber consults those slots inside its callback.
//!
//! Rationale for the single-dispatcher shape: stacking two independent
//! `retour::GenericDetour` handles on the same target function does not
//! compose — the second detour's "call original" path captures the first
//! detour's jmp, so one subscriber's callback silently bypasses the other.

use once_cell::sync::Lazy;
use retour::GenericDetour;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};

/// Default offset, within a `GamePlayActor` instance, of the pointer
/// slot that holds the active `IFootPanel` implementation — the value
/// used on older builds. Subscribers that swap the panel (autoplay) or
/// read it for per-frame input (mine-hit detection) consult this offset.
const FOOT_PANEL_PTR: usize = 0x270;
/// Alternative offset used on newer builds. The service's detector
/// probes `judgeNotes`'s disassembly for whichever of the two the
/// build in question embeds.
const FOOT_PANEL_PTR_ALT: usize = 0x278;

/// Ordering bucket for pre/post callbacks.
///
/// Callbacks fire in ascending `Priority` order (Early before Normal before
/// Late). Within the same priority, order is registration order — do NOT
/// rely on it; use a distinct priority if ordering matters.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Priority {
    Early = 0,
    Normal = 1,
    Late = 2,
}

/// Callback signature for both pre-judge and post-judge callbacks.
///
/// `actor` is the `GamePlayActor` pointer (first argument to the original
/// `judgeNotes`). `music_count` is the current playhead value.
///
/// Plain `fn` (not `Fn`) so the callback can be stored and invoked from the
/// `extern "C"` detour context without heap-allocated closure state.
pub type JudgeCallback = fn(actor: *mut u8, music_count: i32);

/// Handle returned by `register_pre` / `register_post`. Pass back to
/// `unregister` to remove the callback.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CallbackHandle(usize);

/// Original `GamePlayActor::judgeNotes` signature.
type JudgeNotesFn = unsafe extern "C" fn(*mut u8, i32);

struct Entry {
    id: usize,
    priority: Priority,
    callback: JudgeCallback,
}

impl Clone for Entry {
    fn clone(&self) -> Self {
        Entry {
            id: self.id,
            priority: self.priority,
            callback: self.callback,
        }
    }
}

struct JudgeHookInner {
    pre: Vec<Entry>,
    post: Vec<Entry>,
}

static JUDGE_HOOK: Lazy<Mutex<JudgeHookInner>> = Lazy::new(|| {
    Mutex::new(JudgeHookInner {
        pre: Vec::new(),
        post: Vec::new(),
    })
});

static HOOK_ACTIVE: AtomicBool = AtomicBool::new(false);
static NEXT_CALLBACK_ID: AtomicUsize = AtomicUsize::new(1);
/// Detected at `init()` from `judgeNotes`'s disassembly. Readers consult
/// `foot_panel_offset()` — never this directly. `0` sentinel means "not
/// yet detected" (service init has not run or detection failed).
static FOOT_PANEL_OFFSET: AtomicUsize = AtomicUsize::new(0);

// The single retour detour on judgeNotes. Accessed only from `init()` (write)
// and `judge_notes_dispatcher` (read, to call the original).
static mut JUDGE_DETOUR: Option<GenericDetour<JudgeNotesFn>> = None;

/// The detour callback: runs pre-callbacks, calls the original, runs
/// post-callbacks. `extern "C"` — must not panic or unwind across FFI.
unsafe extern "C" fn judge_notes_dispatcher(actor: *mut u8, music_count: i32) {
    // Snapshot the callback lists while holding the lock briefly. Running
    // callbacks outside the lock avoids deadlocks if a callback tries to
    // touch any other Mutex that the detour installer might hold.
    let (pre, post) = {
        let inner = match JUDGE_HOOK.lock() {
            Ok(g) => g,
            Err(_) => {
                // Poisoned — fall through to the original judgeNotes so
                // vanilla play still works.
                if let Some(ref hook) = JUDGE_DETOUR {
                    hook.call(actor, music_count);
                }
                return;
            }
        };
        (inner.pre.clone(), inner.post.clone())
    };

    for e in pre.iter() {
        let cb = e.callback;
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cb(actor, music_count);
        }));
    }

    if let Some(ref hook) = JUDGE_DETOUR {
        hook.call(actor, music_count);
    }

    for e in post.iter() {
        let cb = e.callback;
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cb(actor, music_count);
        }));
    }
}

/// Initialize the service — installs the single retour detour on `judgeNotes`.
///
/// Must be called once during `lib.rs` init, after signature resolution and
/// before any subscriber that needs to register callbacks is enabled.
///
/// Returns `true` if the detour was installed. Returns `false` if the
/// `judge_notes` signature is unavailable or the detour install failed;
/// subsequent `register_pre`/`register_post` calls will return `None`.
///
/// Also detects the offset of the `IFootPanel*` pointer slot on
/// `GamePlayActor` (the slot the judge dereferences to invoke each
/// frame's panel callbacks) by scanning the disassembly of `judgeNotes`
/// for the byte pattern of either known displacement (`0x270` on older
/// builds, `0x278` on newer). Subscribers that need to read/write the
/// slot on the actor consult `foot_panel_offset()`.
pub fn init(signatures: &SignatureStore) -> bool {
    let addr = match signatures.get_address("judge_notes") {
        Some(a) => a,
        None => {
            log_warn!("JudgeHook: judge_notes signature not resolved -- service disabled");
            return false;
        }
    };

    // Detect foot-panel slot offset by scanning the first 512 bytes of the
    // function body for either known displacement. Prefer the newer offset
    // so newer builds resolve first when both patterns happen to appear.
    unsafe {
        let fn_bytes = std::slice::from_raw_parts(addr, 512);
        for off in [FOOT_PANEL_PTR_ALT, FOOT_PANEL_PTR] {
            let disp_bytes = (off as u32).to_le_bytes();
            if fn_bytes.windows(4).any(|w| w == disp_bytes) {
                FOOT_PANEL_OFFSET.store(off, Ordering::Release);
                log_info!("JudgeHook: detected foot panel offset 0x{:X}", off);
                break;
            }
        }
        if FOOT_PANEL_OFFSET.load(Ordering::Acquire) == 0 {
            log_warn!(
                "JudgeHook: foot panel offset not detected -- subscribers that need it will no-op"
            );
        }

        let target: JudgeNotesFn = std::mem::transmute(addr);
        match crate::core::hooks::install_enabled(
            std::ptr::addr_of_mut!(JUDGE_DETOUR),
            target,
            judge_notes_dispatcher,
        ) {
            Ok(()) => {
                HOOK_ACTIVE.store(true, Ordering::Release);
                log_info!("JudgeHook: installed dispatcher on judgeNotes @ {:p}", addr);
                true
            }
            Err(e) => {
                log_warn!("JudgeHook: failed to install detour: {}", e);
                false
            }
        }
    }
}

/// Returns the detected offset, within a `GamePlayActor` instance, of
/// the pointer slot that holds the active `IFootPanel` implementation,
/// or `None` if the service hasn't initialized or detection failed.
/// Subscribers use this to read/write the slot that stores the active
/// `IFootPanel*` on the actor.
pub fn foot_panel_offset() -> Option<usize> {
    match FOOT_PANEL_OFFSET.load(Ordering::Acquire) {
        0 => None,
        v => Some(v),
    }
}

/// Register a callback to run BEFORE the original `judgeNotes`.
///
/// Returns a `CallbackHandle` on success. Returns `None` if the service was
/// not initialized. Pass the handle to `unregister` to remove the callback.
pub fn register_pre(priority: Priority, callback: JudgeCallback) -> Option<CallbackHandle> {
    if !HOOK_ACTIVE.load(Ordering::Acquire) {
        return None;
    }
    let id = NEXT_CALLBACK_ID.fetch_add(1, Ordering::Relaxed);
    let mut inner = match JUDGE_HOOK.lock() {
        Ok(g) => g,
        Err(_) => return None,
    };
    inner.pre.push(Entry {
        id,
        priority,
        callback,
    });
    // Stable sort by priority preserves registration order within each bucket.
    inner.pre.sort_by_key(|e| e.priority);
    Some(CallbackHandle(id))
}

/// Register a callback to run AFTER the original `judgeNotes`.
///
/// Returns a `CallbackHandle` on success. Returns `None` if the service was
/// not initialized. Pass the handle to `unregister` to remove the callback.
pub fn register_post(priority: Priority, callback: JudgeCallback) -> Option<CallbackHandle> {
    if !HOOK_ACTIVE.load(Ordering::Acquire) {
        return None;
    }
    let id = NEXT_CALLBACK_ID.fetch_add(1, Ordering::Relaxed);
    let mut inner = match JUDGE_HOOK.lock() {
        Ok(g) => g,
        Err(_) => return None,
    };
    inner.post.push(Entry {
        id,
        priority,
        callback,
    });
    inner.post.sort_by_key(|e| e.priority);
    Some(CallbackHandle(id))
}

/// Remove a previously-registered callback. Searches both the pre and post
/// lists for the handle's id and removes whichever match is found. No-op if
/// the handle has already been removed or was never registered.
pub fn unregister(handle: CallbackHandle) {
    let mut inner = match JUDGE_HOOK.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    inner.pre.retain(|e| e.id != handle.0);
    inner.post.retain(|e| e.id != handle.0);
}

/// Returns `true` if the dispatcher is installed and accepting registrations.
pub fn is_available() -> bool {
    HOOK_ACTIVE.load(Ordering::Acquire)
}
