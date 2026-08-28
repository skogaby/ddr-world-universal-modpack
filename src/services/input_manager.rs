//! Input Manager — Polls arkmdxbio2.dll exports for button state.

use once_cell::sync::Lazy;
use retour::GenericDetour;
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

// ── SMX input injection ─────────────────────────────────────────────
//
// When active (the `smx-hardware` mod's enable), game-side reads of the
// menu-button and stage-panel getters get SMX-derived state OR'd into
// their out-params (ADDITIVE — cabinet buttons keep working). The
// modpack's own poll is excluded via the same IN_MODPACK_POLL re-entry
// flag the suppression path uses. Trigger (edge) values are synthesized
// from the provider's held (level) state via a per-(player, slot)
// previous-state latch.
//
// The provider is a plain `fn` pointer (stored as usize — hook callbacks
// can't capture) registered by the SMX mod; slots it doesn't feed return
// false. Default inactive: `input_manager` behaves exactly as before
// unless the SMX mod turns this on.

/// Injection slot indices for [`InjectionProvider`].
pub mod inject_slot {
    pub const MENU_START: usize = 0;
    pub const MENU_UP: usize = 1;
    pub const MENU_DOWN: usize = 2;
    pub const MENU_LEFT: usize = 3;
    pub const MENU_RIGHT: usize = 4;
    pub const PANEL_UP: usize = 5;
    pub const PANEL_DOWN: usize = 6;
    pub const PANEL_LEFT: usize = 7;
    pub const PANEL_RIGHT: usize = 8;
    pub const COUNT: usize = 9;
}

/// `(player 0/1, slot) -> currently held`. Must be panic-free and O(1):
/// it runs inside the game's getter reads on the render/IO path.
pub type InjectionProvider = fn(player: usize, slot: usize) -> bool;

static SMX_INJECTION_ACTIVE: AtomicBool = AtomicBool::new(false);
/// The provider fn pointer as usize (0 = none). Atomic so detour bodies can
/// read it without a lock.
static INJECTION_PROVIDER: AtomicUsize = AtomicUsize::new(0);

/// Register the injection provider (call before activating).
pub fn set_injection_provider(provider: InjectionProvider) {
    INJECTION_PROVIDER.store(provider as usize, Ordering::Release);
}

/// Turn SMX input injection on/off. Off (the default) leaves every getter
/// detour behaving exactly as before this feature existed.
pub fn set_injection_active(active: bool) {
    SMX_INJECTION_ACTIVE.store(active, Ordering::Release);
}

/// OR the injected held-level into a getter's current-state out-param.
///
/// Writes ONLY the low byte: gamemdx's input poll passes `u8` locals for
/// these out-params (confirmed in its ark poll loop), while the modpack's
/// own poll passes `u32`s and reads `& 0xFF` — a byte write is correct for
/// both. The getters' out pair is (current state, previous state); the
/// game derives press edges downstream from the pair, so level injection
/// into the current-state byte is complete — no edge synthesis needed
/// (the untouched prev byte comes from the ark's own state).
///
/// Panic-free.
///
/// # Safety
/// `state` must be the getter's first out-param (may be null).
unsafe fn inject_state_byte(player: i32, slot: usize, state: *mut u8) {
    if !SMX_INJECTION_ACTIVE.load(Ordering::Acquire)
        || IN_MODPACK_POLL.load(Ordering::Acquire)
        || !(0..2).contains(&player)
        || state.is_null()
    {
        return;
    }
    let raw = INJECTION_PROVIDER.load(Ordering::Acquire);
    if raw == 0 {
        return;
    }
    // SAFETY: the usize was stored from a valid `InjectionProvider` fn
    // pointer and fn pointers are never deallocated.
    let provider: InjectionProvider = std::mem::transmute(raw);
    if provider(player as usize, slot) {
        *state |= 1;
    }
}

// ── Stage panel injection detours (arkMDXIO vtable level) ───────────
//
// Cabinet-caught 2026-08-27 (deploy #4): injecting at the
// `arkMDXGetPanel*` EXPORTS was invisible to the test menu and unreliable
// for gameplay, because the exports are only ONE consumer's door into the
// panel state. The ark layer's own update loop reads the panel getters
// through the IO singleton's VTABLE directly (maintaining panel counters
// and the state the I/O-check screens display), and gamemdx's poll
// forwards the getters' per-sensor out-args too. The vtable
// implementations (slots +0x310/+0x318/+0x320/+0x328 = Up/Down/Left/Right)
// are the single funnel every consumer goes through — the exports call
// these same slots — so the injection lives there.
//
// Impl shape (Ghidra, all four confirmed identical):
//   u64 impl(this, player_i32, *state_u8, *trigger_u8, *sensors_a_u64,
//            *sensors_b_u64)
// `state` = digested held level; `trigger` = press-edge byte (the ark's
// counter bookkeeping increments on it); the sensor blobs are 4×u16
// per-panel sensor levels shown by the I/O-check screen. player indices
// 4..=11 are debug-keyboard rows — injection only touches 0/1.
//
// Injection: OR the held level into `state`; synthesize a rising-edge
// `trigger` via a per-(player, panel) previous-state latch
// (first-reader-after-press wins — good enough for counters/test UI);
// fill zero sensor blobs with a plausible constant while held so the
// I/O-check screen displays the press.
//
// Install is LAZY: the vtable only exists once the game's arkMDXInitialize
// has populated the IO singleton (seconds after our init), so [`poll`]
// installs the detours on its first tick where the singleton is live and
// an injection provider is registered (i.e. the SMX mod is enabled).

type PanelImplFn =
    unsafe extern "C" fn(*mut std::ffi::c_void, i32, *mut u8, *mut u8, *mut u64, *mut u64) -> u64;

/// Byte offsets of the four panel-getter slots in the arkMDXIO vtable,
/// paired with their injection slots (order: Up, Down, Left, Right —
/// matching the `arkMDXGetPanel*` export wrappers).
const PANEL_VTABLE_SLOTS: [(usize, usize); 4] = [
    (0x310, inject_slot::PANEL_UP),
    (0x318, inject_slot::PANEL_DOWN),
    (0x320, inject_slot::PANEL_LEFT),
    (0x328, inject_slot::PANEL_RIGHT),
];

static mut PANEL_IMPL_UP_DETOUR: Option<GenericDetour<PanelImplFn>> = None;
static mut PANEL_IMPL_DOWN_DETOUR: Option<GenericDetour<PanelImplFn>> = None;
static mut PANEL_IMPL_LEFT_DETOUR: Option<GenericDetour<PanelImplFn>> = None;
static mut PANEL_IMPL_RIGHT_DETOUR: Option<GenericDetour<PanelImplFn>> = None;

/// One-shot latch: the lazy installer runs at most once per process.
static PANEL_IMPL_INSTALL_ATTEMPTED: AtomicBool = AtomicBool::new(false);

/// Per-(player, panel-direction) previous held state for the trigger-edge
/// synthesis (indices: [player][PANEL_VTABLE_SLOTS position]).
static PANEL_PREV_HELD: [[AtomicBool; 4]; 2] = {
    #[allow(clippy::declare_interior_mutable_const)]
    const B: AtomicBool = AtomicBool::new(false);
    [[B; 4], [B; 4]]
};

/// Sensor level written into zeroed sensor blobs while an injected press is
/// held (4×u16 per blob) so the I/O-check screen shows the press.
const INJECTED_SENSOR_LEVEL: u16 = 200;

/// Shared body for the four panel vtable-impl detours.
///
/// # Safety
/// Called only from the installed detours; forwards to the original first.
unsafe fn panel_impl_body(
    hook: &Option<GenericDetour<PanelImplFn>>,
    dir_index: usize,
    slot: usize,
    this: *mut std::ffi::c_void,
    player: i32,
    state: *mut u8,
    trigger: *mut u8,
    sensors_a: *mut u64,
    sensors_b: *mut u64,
) -> u64 {
    let ret = match hook {
        Some(hook) => hook.call(this, player, state, trigger, sensors_a, sensors_b),
        None => 0,
    };
    // One-shot diagnostics (cabinet validation aids): prove the detoured
    // impls are actually consulted, and that injection fires.
    static IMPL_CALL_SEEN: AtomicBool = AtomicBool::new(false);
    static INJECT_PRESS_SEEN: AtomicBool = AtomicBool::new(false);
    if !IMPL_CALL_SEEN.swap(true, Ordering::AcqRel) {
        log_info!(
            "InputManager: panel getter impl consulted (dir={} player={})",
            dir_index,
            player
        );
    }
    if !SMX_INJECTION_ACTIVE.load(Ordering::Acquire) || !(0..2).contains(&player) {
        return ret;
    }
    let raw = INJECTION_PROVIDER.load(Ordering::Acquire);
    if raw == 0 {
        return ret;
    }
    // SAFETY: stored from a valid `InjectionProvider` fn pointer.
    let provider: InjectionProvider = std::mem::transmute(raw);
    let held = provider(player as usize, slot);
    let prev = PANEL_PREV_HELD[player as usize][dir_index].swap(held, Ordering::AcqRel);
    if held {
        if !INJECT_PRESS_SEEN.swap(true, Ordering::AcqRel) {
            log_info!(
                "InputManager: first injected panel press (dir={} player={})",
                dir_index,
                player
            );
        }
        if !state.is_null() {
            *state |= 1;
        }
        if !prev && !trigger.is_null() {
            *trigger |= 1;
        }
        for blob in [sensors_a, sensors_b] {
            if !blob.is_null() && *blob == 0 {
                let level = INJECTED_SENSOR_LEVEL as u64;
                *blob = level | (level << 16) | (level << 32) | (level << 48);
            }
        }
    }
    ret
}

macro_rules! panel_impl_detour {
    ($name:ident, $static:ident, $dir:expr, $slot:expr) => {
        unsafe extern "C" fn $name(
            this: *mut std::ffi::c_void,
            player: i32,
            state: *mut u8,
            trigger: *mut u8,
            sensors_a: *mut u64,
            sensors_b: *mut u64,
        ) -> u64 {
            std::panic::catch_unwind(|| {
                panel_impl_body(
                    &*std::ptr::addr_of!($static),
                    $dir,
                    $slot,
                    this,
                    player,
                    state,
                    trigger,
                    sensors_a,
                    sensors_b,
                )
            })
            .unwrap_or(0)
        }
    };
}

panel_impl_detour!(
    panel_impl_up_detour,
    PANEL_IMPL_UP_DETOUR,
    0,
    inject_slot::PANEL_UP
);
panel_impl_detour!(
    panel_impl_down_detour,
    PANEL_IMPL_DOWN_DETOUR,
    1,
    inject_slot::PANEL_DOWN
);
panel_impl_detour!(
    panel_impl_left_detour,
    PANEL_IMPL_LEFT_DETOUR,
    2,
    inject_slot::PANEL_LEFT
);
panel_impl_detour!(
    panel_impl_right_detour,
    PANEL_IMPL_RIGHT_DETOUR,
    3,
    inject_slot::PANEL_RIGHT
);

/// Lazily install the four panel vtable-impl detours. Called from [`poll`]
/// once the ark IO singleton is live and a provider is registered. The
/// implementation pointers are read from the live object's vtable (no AOB,
/// build-independent) and sanity-checked against the ark module's range.
unsafe fn install_panel_impl_hooks(singleton_obj: usize) {
    let Some(ark) = crate::core::module_resolver::resolve_ark_module() else {
        log_warn!("InputManager: ark module unresolved -- panel injection unavailable");
        return;
    };
    let ark_lo = ark.base as usize;
    let ark_hi = ark_lo + ark.size;
    let in_ark = |p: usize| p >= ark_lo && p < ark_hi;

    if !in_ark(singleton_obj) && singleton_obj < 0x10000 {
        log_warn!("InputManager: implausible ark IO singleton -- panel injection unavailable");
        return;
    }
    let vtable = std::ptr::read_volatile(singleton_obj as *const usize);
    if !in_ark(vtable) {
        log_warn!(
            "InputManager: ark IO vtable {:#x} outside ark module -- panel injection unavailable",
            vtable
        );
        return;
    }

    let mut targets = [0usize; 4];
    for (i, (offset, _)) in PANEL_VTABLE_SLOTS.iter().enumerate() {
        let fn_ptr = std::ptr::read_volatile((vtable + offset) as *const usize);
        if !in_ark(fn_ptr) {
            log_warn!(
                "InputManager: panel vtable slot {:#x} target {:#x} outside ark module -- panel injection unavailable",
                offset,
                fn_ptr
            );
            return;
        }
        targets[i] = fn_ptr;
    }
    // The four impls are distinct functions on every known build; if a
    // future build merges them, a double detour on one address would fail —
    // bail loudly instead.
    for i in 0..4 {
        for j in (i + 1)..4 {
            if targets[i] == targets[j] {
                log_warn!("InputManager: panel vtable impls alias -- panel injection unavailable");
                return;
            }
        }
    }

    macro_rules! install {
        ($idx:expr, $detour:ident, $static:ident, $label:literal) => {
            if let Err(e) = crate::core::hooks::install_enabled(
                std::ptr::addr_of_mut!($static),
                std::mem::transmute::<usize, PanelImplFn>(targets[$idx]),
                $detour as PanelImplFn,
            ) {
                log_warn!(
                    "InputManager: failed to install {} impl detour: {}",
                    $label,
                    e
                );
            }
        };
    }
    install!(0, panel_impl_up_detour, PANEL_IMPL_UP_DETOUR, "PanelUp");
    install!(
        1,
        panel_impl_down_detour,
        PANEL_IMPL_DOWN_DETOUR,
        "PanelDown"
    );
    install!(
        2,
        panel_impl_left_detour,
        PANEL_IMPL_LEFT_DETOUR,
        "PanelLeft"
    );
    install!(
        3,
        panel_impl_right_detour,
        PANEL_IMPL_RIGHT_DETOUR,
        "PanelRight"
    );
    log_info!(
        "InputManager: panel vtable-impl injection detours installed (up={:#x} down={:#x} left={:#x} right={:#x})",
        targets[0],
        targets[1],
        targets[2],
        targets[3]
    );
}

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

/// Shared body: forward to the original via `detour`, OR in any SMX-injected
/// state for game-side callers (inert unless the SMX mod activated
/// injection), then zero the out-params for game-side callers while
/// suppression is active. Injection runs BEFORE suppression so an open
/// overlay wins over injected input, exactly as it does over cabinet input.
unsafe fn menu_button_detour_body(
    detour: &Option<GenericDetour<TriggerHoldFn>>,
    slot: usize,
    player: i32,
    trigger: *mut u32,
    hold: *mut u32,
) {
    if let Some(ref hook) = *detour {
        hook.call(player, trigger, hold);
    }
    // Injection writes only the low byte (game-side callers pass u8
    // out-buffers — see inject_state_byte's doc).
    inject_state_byte(player, slot, trigger.cast::<u8>());
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
    ($name:ident, $static:ident, $slot:expr) => {
        unsafe extern "C" fn $name(player: i32, trigger: *mut u32, hold: *mut u32) {
            let _ = std::panic::catch_unwind(|| {
                menu_button_detour_body(
                    &*std::ptr::addr_of!($static),
                    $slot,
                    player,
                    trigger,
                    hold,
                );
            });
        }
    };
}

menu_button_detour!(get_start_detour, GET_START_DETOUR, inject_slot::MENU_START);
menu_button_detour!(get_up_detour, GET_UP_DETOUR, inject_slot::MENU_UP);
menu_button_detour!(get_down_detour, GET_DOWN_DETOUR, inject_slot::MENU_DOWN);
menu_button_detour!(get_left_detour, GET_LEFT_DETOUR, inject_slot::MENU_LEFT);
menu_button_detour!(get_right_detour, GET_RIGHT_DETOUR, inject_slot::MENU_RIGHT);

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
        // A failed singleton resolution (io_singleton == 0) keeps the
        // long-standing "poll ungated" behavior; panel injection simply
        // stays unavailable in that case.
        if mgr.io_singleton != 0 {
            let singleton_obj =
                unsafe { std::ptr::read_volatile(mgr.io_singleton as *const usize) };
            if singleton_obj == 0 {
                return;
            }
            // Lazy panel-injection install: the arkMDXIO vtable only exists
            // once the game populated the singleton, and the detours are
            // only wanted once an injection provider registered (the SMX
            // mod's enable). One attempt per process; runs on the render
            // thread, which is fine for retour installs (every other hook
            // installs while game threads run too).
            if INJECTION_PROVIDER.load(Ordering::Acquire) != 0
                && !PANEL_IMPL_INSTALL_ATTEMPTED.swap(true, Ordering::AcqRel)
            {
                unsafe { install_panel_impl_hooks(singleton_obj) };
            }
        }
    }
    for p in 0..2u8 {
        poll_player(p);
    }
}

// ── Private ─────────────────────────────────────────────────────

/// The live `arkmdxbio2` I/O singleton object address (the concrete
/// `MdxHWIO` instance), or 0 if the ark hasn't populated it yet. Resolved
/// from the same singleton-pointer landmark the panel injection uses.
/// Callable from any thread (short lock); the SMX transport reads the ark's
/// light-output buffers off this to mirror the operator test-menu LAMP CHECK
/// (which the ark drives internally, bypassing the `arkMDX*` exports).
pub fn io_object_addr() -> usize {
    let Ok(mgr) = INPUT_MANAGER.lock() else {
        return 0;
    };
    if mgr.io_singleton == 0 {
        return 0;
    }
    let obj = unsafe { std::ptr::read_volatile(mgr.io_singleton as *const usize) };
    if obj < 0x10000 {
        return 0;
    }
    obj
}

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
