//! Input Manager — Polls arkmdxbio2.dll exports for button state.

use once_cell::sync::Lazy;
use retour::GenericDetour;
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::core::module_resolver::resolve_ark_module;
use crate::core::scanner::scan_first_call_rel32;
use crate::types::buttons::*;
use crate::{log_info, log_warn};

use windows::core::PCSTR;
use windows::Win32::System::LibraryLoader::GetProcAddress;

type TriggerHoldFn = unsafe extern "C" fn(i32, *mut u32, *mut u32);
type TenKeyFn = unsafe extern "C" fn(i32, *mut [u8; 12], *mut [u8; 12]);

struct ArkExports {
    get_start: TriggerHoldFn,
    get_up: TriggerHoldFn,
    get_down: TriggerHoldFn,
    get_left: TriggerHoldFn,
    get_right: TriggerHoldFn,
    get_10key: TenKeyFn,
}

unsafe impl Send for ArkExports {}
unsafe impl Sync for ArkExports {}

const MENU_BUTTONS: &[(usize, u32)] = &[
    (0, button::START),      // getStart
    (1, button::MENU_UP),    // getUp
    (2, button::MENU_DOWN),  // getDown
    (3, button::MENU_LEFT),  // getLeft
    (4, button::MENU_RIGHT), // getRight
];

const NUMPAD_BITS: &[u32] = &[
    button::NUM_0,
    button::NUM_1,
    button::NUM_2,
    button::NUM_3,
    button::NUM_4,
    button::NUM_5,
    button::NUM_6,
    button::NUM_7,
    button::NUM_8,
    button::NUM_9,
    button::NUM_STAR,
    button::NUM_HASH,
];

// arkmdxbio2's Get* functions return already-debounced state — the game consumes
// these for its own UI. Adding our own release-delay on top caused rapid taps to
// be merged into a single Pressed event. Keep at 0 so every transition is reported.
const RELEASE_DELAY: u32 = 0;

pub(crate) type InputCallback = Arc<dyn Fn(&InputEvent) + Send + Sync>;
pub(crate) type ExclusiveConsumer = Arc<dyn Fn(&InputEvent) -> bool + Send + Sync>;
/// Per-frame callback (see [`on_frame`]).
pub(crate) type FrameCallback = Arc<dyn Fn() + Send + Sync>;

pub(crate) struct InputManagerInner {
    exports: Option<ArkExports>,
    /// Absolute address of arkmdxbio2's I/O singleton pointer. Polling is gated on
    /// this being non-null (arkMDXInitialize has populated the singleton).
    /// Null means the resolver failed and the gate is disabled.
    io_singleton: usize,
    player_state: [u32; 2],
    player_age: [HashMap<u32, u32>; 2],
    callbacks: Vec<(usize, InputCallback)>,
    /// Per-frame callbacks dispatched at the top of [`poll`] — BEFORE the
    /// ark-exports gate, so frame consumers (the preview restart executor)
    /// run even on boots where ark I/O init failed. Same thread contract
    /// as input callbacks: the render/game thread, once per frame.
    frame_callbacks: Vec<(usize, FrameCallback)>,
    exclusive_consumer: Option<ExclusiveConsumer>,
    next_callback_id: usize,
}

pub(crate) static INPUT_MANAGER: Lazy<Mutex<InputManagerInner>> = Lazy::new(|| {
    Mutex::new(InputManagerInner {
        exports: None,
        io_singleton: 0,
        player_state: [0; 2],
        player_age: [HashMap::new(), HashMap::new()],
        callbacks: Vec::new(),
        frame_callbacks: Vec::new(),
        exclusive_consumer: None,
        next_callback_id: 0,
    })
});

/// When true, the `arkMDXGet10Key` detour zeros the buffers for game-side
/// callers so numpad presses don't reach the game. The modpack's own poll
/// continues to see real state via the `IN_MODPACK_POLL` re-entry flag.
static IS_INPUT_SUPPRESSED: AtomicBool = AtomicBool::new(false);

/// Set by `poll_player` around the modpack's `arkMDXGet10Key`/menu-button calls
/// so the suppression detours can distinguish modpack reads from game reads.
///
/// Intentionally a process-global flag, not `thread_local!`: it is correct only
/// because `poll()` and every game-side ark-getter call run on the **same
/// (render) thread** (poll is driven from `wrapper_render_hook`, and the game
/// reads these getters from its render/UI path). Set→read→clear all happen
/// within one `poll_player` call with no intervening await/yield, so a game-side
/// getter can never observe a stale `true`. If a getter were ever called from
/// another thread concurrently with the poll, suppression could briefly
/// misclassify that read — at which point this would need to become
/// `thread_local!`. Mirrors the long-standing `get_10key` suppression pattern.
static IN_MODPACK_POLL: AtomicBool = AtomicBool::new(false);

static mut GET_10KEY_DETOUR: Option<GenericDetour<TenKeyFn>> = None;

unsafe extern "C" fn get_10key_detour(player: i32, buf1: *mut [u8; 12], buf2: *mut [u8; 12]) {
    let _ = std::panic::catch_unwind(|| {
        if let Some(ref hook) = *std::ptr::addr_of!(GET_10KEY_DETOUR) {
            hook.call(player, buf1, buf2);
        }
        if !IN_MODPACK_POLL.load(Ordering::Acquire)
            && IS_INPUT_SUPPRESSED.load(Ordering::Acquire)
            && !buf1.is_null()
            && !buf2.is_null()
        {
            std::ptr::write_bytes(buf1 as *mut u8, 0, 12);
            std::ptr::write_bytes(buf2 as *mut u8, 0, 12);
        }
    });
}

// ── Cabinet menu-button suppression ─────────────────────────────────
//
// The five menu-button getters (arkMDXGetStart/Up/Down/Left/Right) share the
// `TriggerHoldFn` shape `(player, *trigger, *hold)`. While the overlay is open
// (IS_INPUT_SUPPRESSED), zero the trigger/hold out-params for game-side callers
// so cabinet-button navigation doesn't bleed into the game underneath. The
// modpack's own poll bypasses via the IN_MODPACK_POLL re-entry flag (poll_player
// sets it around all of its ark reads). Mirrors the get_10key detour.

static mut GET_START_DETOUR: Option<GenericDetour<TriggerHoldFn>> = None;
static mut GET_UP_DETOUR: Option<GenericDetour<TriggerHoldFn>> = None;
static mut GET_DOWN_DETOUR: Option<GenericDetour<TriggerHoldFn>> = None;
static mut GET_LEFT_DETOUR: Option<GenericDetour<TriggerHoldFn>> = None;
static mut GET_RIGHT_DETOUR: Option<GenericDetour<TriggerHoldFn>> = None;

/// Shared body: forward to the original via `detour`, then zero the out-params
/// for game-side callers while suppression is active.
unsafe fn menu_button_detour_body(
    detour: &Option<GenericDetour<TriggerHoldFn>>,
    player: i32,
    trigger: *mut u32,
    hold: *mut u32,
) {
    if let Some(ref hook) = *detour {
        hook.call(player, trigger, hold);
    }
    if !IN_MODPACK_POLL.load(Ordering::Acquire) && IS_INPUT_SUPPRESSED.load(Ordering::Acquire) {
        if !trigger.is_null() {
            *trigger = 0;
        }
        if !hold.is_null() {
            *hold = 0;
        }
    }
}

macro_rules! menu_button_detour {
    ($name:ident, $static:ident) => {
        unsafe extern "C" fn $name(player: i32, trigger: *mut u32, hold: *mut u32) {
            let _ = std::panic::catch_unwind(|| {
                menu_button_detour_body(&*std::ptr::addr_of!($static), player, trigger, hold);
            });
        }
    };
}

menu_button_detour!(get_start_detour, GET_START_DETOUR);
menu_button_detour!(get_up_detour, GET_UP_DETOUR);
menu_button_detour!(get_down_detour, GET_DOWN_DETOUR);
menu_button_detour!(get_left_detour, GET_LEFT_DETOUR);
menu_button_detour!(get_right_detour, GET_RIGHT_DETOUR);

/// Install the five menu-button suppression detours. `getters` is
/// `[start, up, down, left, right]`. Best-effort: logs and leaves a button
/// un-suppressed on individual failure (degraded, not fatal).
unsafe fn install_menu_button_detours(getters: &[TriggerHoldFn; 5]) {
    macro_rules! install {
        ($idx:expr, $detour:ident, $static:ident, $label:literal) => {
            if let Err(e) = crate::core::hooks::install_enabled(
                std::ptr::addr_of_mut!($static),
                getters[$idx],
                $detour as TriggerHoldFn,
            ) {
                log_warn!("InputManager: failed to install {} detour: {}", $label, e);
            }
        };
    }
    install!(0, get_start_detour, GET_START_DETOUR, "arkMDXGetStart");
    install!(1, get_up_detour, GET_UP_DETOUR, "arkMDXGetUp");
    install!(2, get_down_detour, GET_DOWN_DETOUR, "arkMDXGetDown");
    install!(3, get_left_detour, GET_LEFT_DETOUR, "arkMDXGetLeft");
    install!(4, get_right_detour, GET_RIGHT_DETOUR, "arkMDXGetRight");
    log_info!("InputManager: installed cabinet menu-button suppression detours");
}

/// Enable or disable game-side numpad input suppression. The modpack's own
/// poll continues to see real state regardless of this flag.
pub fn set_input_suppressed(suppressed: bool) {
    IS_INPUT_SUPPRESSED.store(suppressed, Ordering::Release);
}

pub fn init() -> bool {
    let ark_module = match resolve_ark_module() {
        Some(m) => m,
        None => return false,
    };

    let exports = match resolve_exports(&ark_module) {
        Some(e) => e,
        None => {
            log_warn!("InputManager: required exports not available");
            return false;
        }
    };

    // Resolve the address of the I/O singleton pointer inside arkmdxbio2.
    // arkMDXGetStart (and every other arkMDX*Get*) reads this pointer via an
    // internal wrapper and dereferences it with *no null check*. poll() gates
    // on a live read of this pointer — if null, skip the poll entirely.
    let io_singleton = resolve_io_singleton_ptr(&ark_module) as usize;
    if io_singleton == 0 {
        log_warn!("InputManager: could not locate ark I/O singleton pointer; poll will run ungated (may crash on uninitialized ark)");
    }

    // Install the arkMDXGet10Key detour for mod-menu input suppression.
    // The detour zeros the buffers for game-side callers when the menu is
    // open, so numpad navigation presses don't bleed through. Modpack
    // reads see real state via the IN_MODPACK_POLL re-entry flag.
    let get_10key_target = exports.get_10key;
    // Snapshot the menu-button getters (Copy fn pointers) before `exports` is
    // moved into the manager, so we can install their suppression detours.
    let menu_getters = [
        exports.get_start,
        exports.get_up,
        exports.get_down,
        exports.get_left,
        exports.get_right,
    ];
    match INPUT_MANAGER.lock() {
        Ok(mut mgr) => {
            mgr.exports = Some(exports);
            mgr.io_singleton = io_singleton;
        }
        Err(_) => {
            log_warn!("InputManager: state lock poisoned during init -- aborting");
            return false;
        }
    }
    unsafe {
        match crate::core::hooks::install_enabled(
            std::ptr::addr_of_mut!(GET_10KEY_DETOUR),
            get_10key_target,
            get_10key_detour,
        ) {
            Ok(()) => {
                log_info!("InputManager: installed arkMDXGet10Key suppression detour");
            }
            Err(e) => log_warn!("InputManager: failed to install get_10key detour: {}", e),
        }
        // Install cabinet menu-button suppression (Start/Up/Down/Left/Right).
        install_menu_button_detours(&menu_getters);
    }

    log_info!("InputManager initialized ({})", ark_module.name);
    true
}

pub fn on_input_event(callback: InputCallback) -> usize {
    // On a poisoned lock, return a sentinel id that owns no callback;
    // remove_callback(sentinel) is a harmless no-op.
    let Ok(mut mgr) = INPUT_MANAGER.lock() else {
        return usize::MAX;
    };
    let id = mgr.next_callback_id;
    mgr.next_callback_id += 1;
    mgr.callbacks.push((id, callback));
    id
}

pub fn remove_callback(id: usize) {
    if let Ok(mut mgr) = INPUT_MANAGER.lock() {
        mgr.callbacks.retain(|(cid, _)| *cid != id);
    }
}

/// Register a per-frame callback, dispatched once per render frame from
/// [`poll`] (the render/game thread — the one context game APIs are legal
/// in). Runs BEFORE the ark-exports gate so frame consumers work even on
/// boots where ark I/O init failed. Callbacks must keep their idle path
/// O(1) (poll runs at native refresh rate); each dispatch is individually
/// panic-contained. Returns an id for [`remove_frame_callback`]
/// (usize::MAX sentinel on a poisoned lock — a harmless no-op to remove).
pub fn on_frame(callback: FrameCallback) -> usize {
    let Ok(mut mgr) = INPUT_MANAGER.lock() else {
        return usize::MAX;
    };
    let id = mgr.next_callback_id;
    mgr.next_callback_id += 1;
    mgr.frame_callbacks.push((id, callback));
    id
}

pub fn remove_frame_callback(id: usize) {
    if let Ok(mut mgr) = INPUT_MANAGER.lock() {
        mgr.frame_callbacks.retain(|(cid, _)| *cid != id);
    }
}

pub fn get_button_state(player: Player) -> u32 {
    INPUT_MANAGER
        .lock()
        .map(|mgr| mgr.player_state[player as usize])
        .unwrap_or(0)
}

pub fn set_exclusive_consumer(callback: Arc<dyn Fn(&InputEvent) -> bool + Send + Sync>) {
    if let Ok(mut mgr) = INPUT_MANAGER.lock() {
        mgr.exclusive_consumer = Some(callback);
    }
}

pub fn clear_exclusive_consumer() {
    if let Ok(mut mgr) = INPUT_MANAGER.lock() {
        mgr.exclusive_consumer = None;
    }
}

pub fn is_available() -> bool {
    INPUT_MANAGER
        .lock()
        .map(|mgr| mgr.exports.is_some())
        .unwrap_or(false)
}

// ── Public polling entry point ──────────────────────────────────

/// Poll arcade button state for both players and fire input events.
/// Safe to call before `init()` (no-ops) or before arkMDXInitialize (no-ops via the singleton gate).
/// Intended to be called from the render thread (via widget_renderer's wrapper_render hook).
pub fn poll() {
    // Frame callbacks first — snapshotted out of the lock (a callback
    // registering/removing callbacks must not deadlock), each dispatch
    // panic-contained (this is a render-thread hook path; a panic must
    // not unwind into game code). Deliberately BEFORE the ark gate.
    let frame_callbacks: Vec<FrameCallback> = match INPUT_MANAGER.lock() {
        Ok(mgr) if !mgr.frame_callbacks.is_empty() => mgr
            .frame_callbacks
            .iter()
            .map(|(_, cb)| cb.clone())
            .collect(),
        _ => Vec::new(),
    };
    for cb in frame_callbacks {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cb()));
    }
    // Gate: skip poll if ark module isn't initialized or the I/O singleton is still null.
    {
        let Ok(mgr) = INPUT_MANAGER.lock() else {
            return;
        };
        if mgr.exports.is_none() {
            return;
        }
        if mgr.io_singleton != 0
            && unsafe { std::ptr::read_volatile(mgr.io_singleton as *const usize) } == 0
        {
            return;
        }
    }
    for p in 0..2u8 {
        poll_player(p);
    }
}

// ── Private ─────────────────────────────────────────────────────

fn poll_player(player: u8) {
    let mut events: Vec<InputEvent> = Vec::new();

    {
        let Ok(mut mgr) = INPUT_MANAGER.lock() else {
            return;
        };

        // Copy function pointers out before mutable borrow
        let (fns, get_10key) = match &mgr.exports {
            Some(e) => (
                [e.get_start, e.get_up, e.get_down, e.get_left, e.get_right],
                e.get_10key,
            ),
            None => return,
        };

        let mut state = mgr.player_state[player as usize];
        let ages = &mut mgr.player_age[player as usize];

        // Set the re-entry flag so our menu-button suppression detours pass
        // real state through to the modpack's own poll regardless of
        // `IS_INPUT_SUPPRESSED` (which only suppresses game-side callers).
        IN_MODPACK_POLL.store(true, Ordering::Release);
        for &(idx, bit) in MENU_BUTTONS {
            let mut trigger: u32 = 0;
            let mut hold: u32 = 0;
            unsafe { fns[idx](player as i32, &mut trigger, &mut hold) };
            let active = (trigger & 0xFF) != 0 || (hold & 0xFF) != 0;
            state = update_button(state, bit, active, ages, player, RELEASE_DELAY, &mut events);
        }
        IN_MODPACK_POLL.store(false, Ordering::Release);

        // 10-key numpad. Set the re-entry flag so our detour passes real
        // state through to us regardless of `IS_INPUT_SUPPRESSED` (which
        // only suppresses game-side callers).
        let mut buf1 = [0u8; 12];
        let mut buf2 = [0u8; 12];
        IN_MODPACK_POLL.store(true, Ordering::Release);
        unsafe { (get_10key)(player as i32, &mut buf1, &mut buf2) };
        IN_MODPACK_POLL.store(false, Ordering::Release);
        for (i, &bit) in NUMPAD_BITS.iter().enumerate() {
            let active = buf1[i] != 0;
            state = update_button(state, bit, active, ages, player, RELEASE_DELAY, &mut events);
        }

        mgr.player_state[player as usize] = state;
    }

    // Emit events outside the lock — clone callbacks to avoid deadlock
    // (callbacks may call back into input_manager to set exclusive consumer, etc.)
    if !events.is_empty() {
        let Some((exclusive, callbacks)) = INPUT_MANAGER.lock().ok().map(|mgr| {
            (
                mgr.exclusive_consumer.clone(),
                mgr.callbacks
                    .iter()
                    .map(|(id, cb)| (*id, cb.clone()))
                    .collect::<Vec<_>>(),
            )
        }) else {
            return;
        };
        for event in &events {
            if let Some(ref consumer) = exclusive {
                if consumer(event) {
                    continue;
                }
            }
            for (_, cb) in &callbacks {
                cb(event);
            }
        }
    }
}

fn update_button(
    mut state: u32,
    bit: u32,
    active: bool,
    ages: &mut HashMap<u32, u32>,
    player: u8,
    release_delay: u32,
    events: &mut Vec<InputEvent>,
) -> u32 {
    let was_held = (state & bit) != 0;
    let p = if player == 0 { Player::P1 } else { Player::P2 };

    if active {
        ages.insert(bit, 0);
        if !was_held {
            state |= bit;
            events.push(InputEvent {
                player: p,
                button: bit,
                button_name: BUTTON_NAMES.get(&bit).unwrap_or(&"?").to_string(),
                event_type: InputEventType::Pressed,
            });
        }
    } else {
        let age = ages.get(&bit).copied().unwrap_or(release_delay) + 1;
        ages.insert(bit, age);
        if was_held && age > release_delay {
            state &= !bit;
            events.push(InputEvent {
                player: p,
                button: bit,
                button_name: BUTTON_NAMES.get(&bit).unwrap_or(&"?").to_string(),
                event_type: InputEventType::Released,
            });
        }
    }

    state
}

fn resolve_exports(ark_module: &crate::core::module_resolver::GameModule) -> Option<ArkExports> {
    unsafe {
        let resolve = |name: &str| -> Option<*const ()> {
            let cname = CString::new(name).ok()?;
            let addr = GetProcAddress(ark_module.handle, PCSTR(cname.as_ptr() as *const u8))?;
            Some(addr as *const ())
        };

        let get_start = std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(i32, *mut u32, *mut u32),
        >(resolve("arkMDXGetStart")?);
        let get_up = std::mem::transmute::<*const (), unsafe extern "C" fn(i32, *mut u32, *mut u32)>(
            resolve("arkMDXGetUp")?,
        );
        let get_down = std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(i32, *mut u32, *mut u32),
        >(resolve("arkMDXGetDown")?);
        let get_left = std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(i32, *mut u32, *mut u32),
        >(resolve("arkMDXGetLeft")?);
        let get_right = std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(i32, *mut u32, *mut u32),
        >(resolve("arkMDXGetRight")?);
        let get_10key = std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(i32, *mut [u8; 12], *mut [u8; 12]),
        >(resolve("arkMDXGet10Key")?);

        Some(ArkExports {
            get_start,
            get_up,
            get_down,
            get_left,
            get_right,
            get_10key,
        })
    }
}

/// Derive the absolute address of arkmdxbio2's I/O singleton pointer.
///
/// Walks the call graph from `arkMDXGetStart`:
/// 1. Scan first 64 bytes of `arkMDXGetStart` for `CALL rel32` (opcode 0xE8) — targets `get_io_state()`.
/// 2. Scan first 32 bytes of `get_io_state()` for `MOV RAX, [RIP+disp32]` (bytes `48 8B 05`) —
///    that disp32 encodes the address of the singleton pointer.
///
/// Returns null if any step fails; caller treats null as "gate disabled".
fn resolve_io_singleton_ptr(ark_module: &crate::core::module_resolver::GameModule) -> *const usize {
    unsafe {
        let cname = match CString::new("arkMDXGetStart") {
            Ok(c) => c,
            Err(_) => return std::ptr::null(),
        };
        let Some(get_start) = GetProcAddress(ark_module.handle, PCSTR(cname.as_ptr() as *const u8))
        else {
            return std::ptr::null();
        };
        let get_start = get_start as *const u8;

        // Find the single CALL rel32 in arkMDXGetStart's prologue.
        let get_io_state = match scan_first_call_rel32(get_start, 64) {
            Some(p) => p,
            None => return std::ptr::null(),
        };

        // Find `MOV RAX, [RIP+disp32]` (48 8B 05 xx xx xx xx) in get_io_state's prologue.
        for i in 0..32 {
            let p = get_io_state.add(i);
            if *p == 0x48 && *p.add(1) == 0x8B && *p.add(2) == 0x05 {
                return crate::core::scanner::decode_rip_relative(p.add(3)) as *const usize;
            }
        }
        std::ptr::null()
    }
}
