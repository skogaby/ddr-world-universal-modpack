//! Render Notes Hook Service — Shared dispatcher for `ArrowRenderer::render_notes`.
//!
//! Installs exactly one `retour::GenericDetour` on the arrow renderer's
//! per-frame note draw function. Subscribers register pre/post callbacks
//! with a priority; the detour dispatches:
//!
//!   1. All pre callbacks in ascending priority order
//!   2. The original `render_notes` (the vanilla shock + tap/freeze passes)
//!   3. All post callbacks in ascending priority order
//!
//! One `render_notes` invocation = one `ArrowRenderer this` = one play side,
//! so callbacks get per-side context for free from the renderer pointer.
//!
//! Known subscribers and their ordering contract:
//! - `note_types_expansion::mine_render` — post @ Normal, appends the mine
//!   pass records (binds the default shader mid-pass).
//! - `player_perspective` — pre @ Normal (constant upload + window snapshot)
//!   and post @ Late (window walk/rewrite), deliberately AFTER mine_render so
//!   the captured window includes the mine pass's SetShader records.
//!
//! Callbacks are plain `fn` pointers (not closures) because they are invoked
//! from within an `extern "C"` detour callback and must not require captured
//! state. Per-subscriber state lives in the subscriber's own statics.
//!
//! Rationale for the single-dispatcher shape: stacking two independent
//! `retour::GenericDetour` handles on the same target function does not
//! compose — the second detour's "call original" path captures the first
//! detour's jmp, so one subscriber's callback silently bypasses the other.

use once_cell::sync::Lazy;
use retour::GenericDetour;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};

pub use crate::services::judge_hook::Priority;

/// Callback signature for both pre and post callbacks. `renderer` is the
/// `ArrowRenderer this` pointer (first argument to the original).
pub type RenderNotesCallback = fn(renderer: *mut u8);

/// Handle returned by `register_pre` / `register_post`. Pass back to
/// `unregister` to remove the callback.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CallbackHandle(usize);

/// Original `ArrowRenderer::render_notes` signature.
type RenderNotesFn = unsafe extern "C" fn(*mut u8);

#[derive(Clone)]
struct Entry {
    id: usize,
    priority: Priority,
    callback: RenderNotesCallback,
}

struct Inner {
    pre: Vec<Entry>,
    post: Vec<Entry>,
}

static HOOK_STATE: Lazy<Mutex<Inner>> = Lazy::new(|| {
    Mutex::new(Inner {
        pre: Vec::new(),
        post: Vec::new(),
    })
});

static HOOK_ACTIVE: AtomicBool = AtomicBool::new(false);
static NEXT_CALLBACK_ID: AtomicUsize = AtomicUsize::new(1);

/// The `screen_renderer_state` derived global (pointer to the ScreenRenderer
/// state pointer), resolved at `init()`. Used by `active_command_list()`.
static SCREEN_RENDERER_STATE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());

// The single retour detour on render_notes. Written only from `init()`
// (before HOOK_ACTIVE is set), read from the dispatcher.
static mut RN_DETOUR: Option<GenericDetour<RenderNotesFn>> = None;

/// The detour callback: runs pre callbacks, calls the original, runs post
/// callbacks. `extern "C"` — must not panic or unwind across FFI.
unsafe extern "C" fn render_notes_dispatcher(renderer: *mut u8) {
    // Snapshot the callback lists while holding the lock briefly; run the
    // callbacks outside the lock (a callback touching another Mutex must
    // not deadlock against a concurrent registration).
    let (pre, post) = {
        let inner = match HOOK_STATE.lock() {
            Ok(g) => g,
            Err(_) => {
                // Poisoned — fall through to the original so vanilla
                // rendering still works.
                if let Some(ref hook) = *std::ptr::addr_of!(RN_DETOUR) {
                    hook.call(renderer);
                }
                return;
            }
        };
        (inner.pre.clone(), inner.post.clone())
    };

    for e in pre.iter() {
        let cb = e.callback;
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cb(renderer);
        }));
    }

    if let Some(ref hook) = *std::ptr::addr_of!(RN_DETOUR) {
        hook.call(renderer);
    }

    for e in post.iter() {
        let cb = e.callback;
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cb(renderer);
        }));
    }
}

/// Initialize the service — installs the single retour detour on
/// `render_notes` and resolves the `screen_renderer_state` global for
/// `active_command_list()`.
///
/// Must be called during `lib.rs` init BEFORE mod registration:
/// `note_types_expansion` registers its mine-pass callback from `init()`.
///
/// Returns `true` if the detour was installed. On `false` (signature
/// missing / install failure) all `register_*` calls return `None`.
pub fn init(signatures: &SignatureStore) -> bool {
    let addr = match signatures.get_address("render_notes") {
        Some(a) => a,
        None => {
            log_warn!("RenderNotesHook: render_notes signature not resolved -- service disabled");
            return false;
        }
    };

    // Optional convenience global (subscribers can also carry their own):
    // without it the service still dispatches, but active_command_list()
    // returns null.
    match signatures.get_address("screen_renderer_state") {
        Some(a) => SCREEN_RENDERER_STATE.store(a as *mut u8, Ordering::Release),
        None => log_warn!(
            "RenderNotesHook: screen_renderer_state not resolved -- active_command_list() unavailable"
        ),
    }

    unsafe {
        let target: RenderNotesFn = std::mem::transmute(addr);
        match crate::core::hooks::install_enabled(
            std::ptr::addr_of_mut!(RN_DETOUR),
            target,
            render_notes_dispatcher,
        ) {
            Ok(()) => {
                HOOK_ACTIVE.store(true, Ordering::Release);
                log_info!(
                    "RenderNotesHook: installed dispatcher on render_notes @ {:p}",
                    addr
                );
                true
            }
            Err(e) => {
                log_warn!("RenderNotesHook: failed to install detour: {}", e);
                false
            }
        }
    }
}

/// Register a callback to run BEFORE the original `render_notes`.
/// Returns `None` if the service is not installed.
pub fn register_pre(priority: Priority, callback: RenderNotesCallback) -> Option<CallbackHandle> {
    register(priority, callback, /* pre = */ true)
}

/// Register a callback to run AFTER the original `render_notes`.
/// Returns `None` if the service is not installed.
pub fn register_post(priority: Priority, callback: RenderNotesCallback) -> Option<CallbackHandle> {
    register(priority, callback, /* pre = */ false)
}

fn register(
    priority: Priority,
    callback: RenderNotesCallback,
    pre: bool,
) -> Option<CallbackHandle> {
    if !HOOK_ACTIVE.load(Ordering::Acquire) {
        return None;
    }
    let id = NEXT_CALLBACK_ID.fetch_add(1, Ordering::Relaxed);
    let mut inner = match HOOK_STATE.lock() {
        Ok(g) => g,
        Err(_) => return None,
    };
    let list = if pre { &mut inner.pre } else { &mut inner.post };
    list.push(Entry {
        id,
        priority,
        callback,
    });
    // Stable sort by priority preserves registration order within a bucket.
    list.sort_by_key(|e| e.priority);
    Some(CallbackHandle(id))
}

/// Remove a previously-registered callback (searches both lists). No-op if
/// already removed.
pub fn unregister(handle: CallbackHandle) {
    let mut inner = match HOOK_STATE.lock() {
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

/// The CommandList at layer-table slot `index` (0..=8), or null. Same
/// Ghidra-verified layout as [`active_command_list`]. Used by the
/// overlay-draw emitter, which appends ONLY to the list the layer
/// dispatcher is about to walk on the current thread (blind appends to
/// arbitrary slots crashed — see docs/overlay_draw_research.md).
pub fn command_list_at(index: usize) -> *mut u8 {
    if index > 8 {
        return std::ptr::null_mut();
    }
    let global = SCREEN_RENDERER_STATE.load(Ordering::Acquire);
    if global.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        let state = *(global as *const *const u8);
        if state.is_null() {
            return std::ptr::null_mut();
        }
        *(state.add(0x40 + index * 8) as *const *mut u8)
    }
}

/// The active per-frame CommandList, or null if unavailable.
///
/// Layout (Ghidra-verified, stable across builds): the derived global holds
/// a pointer to the ScreenRenderer state; `[state+0x68]` = active list
/// index, `[state + 0x40 + index*8]` = CommandList*.
pub fn active_command_list() -> *mut u8 {
    let global = SCREEN_RENDERER_STATE.load(Ordering::Acquire);
    if global.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        let state = *(global as *const *const u8);
        if state.is_null() {
            return std::ptr::null_mut();
        }
        let index = *(state.add(0x68) as *const i32);
        if !(0..=8).contains(&index) {
            return std::ptr::null_mut();
        }
        *(state.add(0x40 + index as usize * 8) as *const *mut u8)
    }
}

/// The CommandList's current write pointer (`*(cl + 0x10)`) — records
/// emitted so far end here. Null in, null out.
///
/// # Safety
/// `cl` must be a valid CommandList pointer obtained from
/// `active_command_list()` (or null).
pub unsafe fn write_ptr(cl: *const u8) -> *mut u8 {
    if cl.is_null() {
        return std::ptr::null_mut();
    }
    *(cl.add(0x10) as *const *mut u8)
}
