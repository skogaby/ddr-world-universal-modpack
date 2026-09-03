//! Input Manager — Polls arkmdxbio2.dll exports for button state.

use once_cell::sync::Lazy;
use retour::GenericDetour;
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::core::module_resolver::resolve_ark_module;
use crate::core::scanner::scan_first_call_rel32;
use crate::types::buttons::*;
use crate::{log_info, log_warn};

use windows::core::PCSTR;
use windows::Win32::System::LibraryLoader::GetProcAddress;

type TriggerHoldFn = unsafe extern "C" fn(i32, *mut u32, *mut u32);
type TenKeyFn = unsafe extern "C" fn(i32, *mut [u8; 12], *mut [u8; 12]);
/// `arkMDXGetPanel{Up,Down,Left,Right}(player, *trigger, *hold, *release, *counter)`
/// — the stage-panel export wrappers (see `docs/input_system_research.md`).
type PanelGetFn = unsafe extern "C" fn(i32, *mut u8, *mut u8, *mut u8, *mut u32);

struct ArkExports {
    get_start: TriggerHoldFn,
    get_up: TriggerHoldFn,
    get_down: TriggerHoldFn,
    get_left: TriggerHoldFn,
    get_right: TriggerHoldFn,
    get_10key: TenKeyFn,
    /// Stage-panel getters `[Up, Down, Left, Right]`. Best-effort: `None`
    /// when any export failed to resolve (panel events unavailable).
    panel_getters: Option<[PanelGetFn; 4]>,
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
    /// Pinpad keys: slot = `PINPAD_BASE + k` where `k` is the
    /// `arkMDXGet10Key` buffer index (0..=9 digits, 10 = "00",
    /// 11 = decimal point).
    pub const PINPAD_BASE: usize = 9;
    pub const PINPAD_KEYS: usize = 12;
    pub const COUNT: usize = PINPAD_BASE + PINPAD_KEYS;
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

// ── Pinpad pulse injection (generic mod-facing one-shot API) ────────
//
// A minimal sibling of the SMX injection provider: any mod can request a
// one-shot pinpad key pulse (classic_difficulty synthesizes the game's
// difficulty-change pinpad presses from dance-pad double-taps). Pulses
// ride the same 10-key vtable-impl detour the SMX overlay uses, but are
// independent of the SMX provider/active gates. Like SMX pinpad presses
// (deploy #17), pulses are visible to BOTH the game and the modpack's
// own poll. The pulse length mirrors the SMX overlay's cabinet-proven
// value: level injection reads as a stuck key, ~120 ms reads as a tap.

/// How long a requested pinpad pulse reads as "key down".
const PINPAD_PULSE_MS: u64 = 120;

/// Millisecond epoch for pulse deadlines (Instant isn't atomic-friendly).
static PULSE_EPOCH: Lazy<std::time::Instant> = Lazy::new(std::time::Instant::now);

fn pulse_now_ms() -> u64 {
    PULSE_EPOCH.elapsed().as_millis() as u64
}

/// Per-(player, 10-key buffer index) pulse deadline in [`PULSE_EPOCH`]
/// millis (0 = idle). Indices 0..=9 digits, 10 = "00", 11 = decimal point.
static PINPAD_PULSE_DEADLINE: [[AtomicU64; 12]; 2] = {
    #[allow(clippy::declare_interior_mutable_const)]
    const Z: AtomicU64 = AtomicU64::new(0);
    [[Z; 12], [Z; 12]]
};

/// Set once any consumer wants injection detours without registering the
/// (single-slot, SMX-owned) injection provider — a second trigger for the
/// lazy vtable-detour install in [`poll`].
static AUX_INJECTION_WANTED: AtomicBool = AtomicBool::new(false);

/// Declare that a mod will call [`request_pinpad_pulse`], so the lazy
/// 10-key vtable-impl detour installs even when the SMX mod is disabled.
/// Call from the mod's `enable` (before the first pulse). One-way for the
/// process lifetime — the installed detour is pass-through while idle.
pub fn request_pinpad_injection() {
    AUX_INJECTION_WANTED.store(true, Ordering::Release);
}

/// Request a one-shot ~120 ms pinpad key pulse for `player` (0/1),
/// `key` = 10-key buffer index (0..=9 digits, 10 = "00", 11 = point).
/// Callable from any thread (atomics only). Requires
/// [`request_pinpad_injection`] to have been called at some enable point
/// so the vtable detour is installed.
pub fn request_pinpad_pulse(player: usize, key: usize) {
    if player >= 2 || key >= 12 {
        return;
    }
    PINPAD_PULSE_DEADLINE[player][key].store(pulse_now_ms() + PINPAD_PULSE_MS, Ordering::Release);
}

/// Cancel all in-flight pinpad pulses (mod disable hygiene; pulses also
/// self-expire in ~120 ms).
pub fn clear_pinpad_pulses() {
    for player in &PINPAD_PULSE_DEADLINE {
        for key in player {
            key.store(0, Ordering::Release);
        }
    }
}

// ── Stage-panel event polling (opt-in) ──────────────────────────────
//
// When enabled, [`poll_player`] also reads the four `arkMDXGetPanel*`
// exports and reports the dance-pad panels as `button::PANEL_*`
// InputEvents. Off by default so boots without a panel consumer keep the
// exact stock poll footprint. The exports funnel through the same vtable
// impls the SMX injection detours cover, so injected SMX pad presses show
// up here too — consistent with cabinet presses.

static PANEL_POLLING: AtomicBool = AtomicBool::new(false);

/// Enable/disable dance-pad panel polling (PANEL_* InputEvents).
/// Currently a plain flag, not a refcount: if two mods ever consume panel
/// events, the second `set_panel_polling(false)` would starve the first —
/// promote to a refcount at that point.
pub fn set_panel_polling(enabled: bool) {
    PANEL_POLLING.store(enabled, Ordering::Release);
}

/// Panel poll order ↔ button bits (matches `ArkExports::panel_getters`).
const PANEL_BUTTONS: [u32; 4] = [
    button::PANEL_UP,
    button::PANEL_DOWN,
    button::PANEL_LEFT,
    button::PANEL_RIGHT,
];

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

// The four panel-getter detours are installed in `arkMDXGetPanel*` export
// order (Up, Down, Left, Right). Their VTABLE byte offsets are NOT constant
// across ark builds — they are derived per boot from the export wrappers
// (`derive_ark_vtable_slots`): 0x310..0x328 on the 2026 arks but
// 0x2F0..0x308 on the 20250805 ark, whose vtable has four fewer slots.
// The 20250805 boot crash (log 2026-09-02) was exactly this: the hardcoded
// "10-key" slot 0x308 held a 6-argument panel getter there, and calling it
// through the 4-argument 10-key detour wrote through a garbage stack arg.

/// The arkMDXIO vtable layout the MdxHWIO FIELD MAP below (menu edge/level
/// bytes, card block, scan gates, override words) was reverse-engineered
/// against: 10-key impl at +0x308, panel getters at +0x310..+0x328. The
/// menu/card injection (IO-dispatcher detour) is only installed when the
/// derived layout matches it — a different vtable shape means a different
/// ark generation whose field offsets are unverified.
const ARK_VERIFIED_TENKEY_SLOT: usize = 0x308;
const ARK_VERIFIED_PANEL_SLOTS: [usize; 4] = [0x310, 0x318, 0x320, 0x328];

/// arkMDXIO vtable slot offsets derived from the export wrappers.
#[derive(Clone, Copy)]
struct ArkVtableSlots {
    tenkey: usize,
    panels: [usize; 4],
}

/// Decode the vtable slot an `arkMDXGet*` export wrapper dispatches through.
/// Every wrapper (verified byte-identical apart from the disp32 on the
/// 20250805 and 20260721 arks) ends in `JMP qword [R10+disp32]`
/// (`41 FF A2 d32`) or `CALL qword [R11+disp32]` (`41 FF 91 d32`): scan the
/// first 0x80 bytes for a REX.B `FF /2` or `FF /4` with mod=10 and a
/// non-SIB base, and take its disp32. Returns None when the shape is absent.
unsafe fn derive_vtable_slot_from_export(export: *const u8) -> Option<usize> {
    let body = std::slice::from_raw_parts(export, 0x80);
    for i in 0..body.len() - 7 {
        if body[i] != 0x41 || body[i + 1] != 0xFF {
            continue;
        }
        let modrm = body[i + 2];
        let is_mod10 = modrm & 0xC0 == 0x80;
        let reg = modrm & 0x38;
        let rm = modrm & 0x07;
        if is_mod10 && (reg == 0x10 || reg == 0x20) && rm != 0x04 {
            let disp = u32::from_le_bytes([body[i + 3], body[i + 4], body[i + 5], body[i + 6]]);
            return Some(disp as usize);
        }
    }
    None
}

/// Derive the 10-key and four panel-getter vtable slots from the ark's own
/// export wrappers. Validates the shape (panels consecutive 8-byte slots,
/// everything inside a plausible vtable span, 10-key distinct from the
/// panels); any miss ⇒ None and the caller installs NO injection detours.
fn derive_ark_vtable_slots(exports: &ArkExports) -> Option<ArkVtableSlots> {
    let panel_fns = exports.panel_getters?;
    unsafe {
        let tenkey = derive_vtable_slot_from_export(exports.get_10key as *const u8)?;
        let mut panels = [0usize; 4];
        for (i, f) in panel_fns.iter().enumerate() {
            panels[i] = derive_vtable_slot_from_export(*f as *const u8)?;
        }
        let plausible = |s: usize| (0x40..=0x1000).contains(&s) && s % 8 == 0;
        if !plausible(tenkey) || !panels.iter().all(|&s| plausible(s)) {
            return None;
        }
        if panels.windows(2).any(|w| w[1] != w[0] + 8) || panels.contains(&tenkey) {
            return None;
        }
        Some(ArkVtableSlots { tenkey, panels })
    }
}

static mut PANEL_IMPL_UP_DETOUR: Option<GenericDetour<PanelImplFn>> = None;
static mut PANEL_IMPL_DOWN_DETOUR: Option<GenericDetour<PanelImplFn>> = None;
static mut PANEL_IMPL_LEFT_DETOUR: Option<GenericDetour<PanelImplFn>> = None;
static mut PANEL_IMPL_RIGHT_DETOUR: Option<GenericDetour<PanelImplFn>> = None;

/// One-shot latch: the lazy installer runs at most once per process.
static PANEL_IMPL_INSTALL_ATTEMPTED: AtomicBool = AtomicBool::new(false);

/// Per-(player, panel-direction) previous held state for the trigger-edge
/// synthesis (indices: [player][export order Up/Down/Left/Right]).
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

// ── Touch-overlay injection: IO dispatcher + 10-key vtable detours ───
//
// Step 3 (touchscreen overlay) RE, Ghidra-verified on
// arkmdxbio2_20260721 (addresses file-relative to 0x180000000):
//
// - The design table's `arkMDXGetEAPass` card plan was WRONG: gamemdx
//   resolves that export but never calls it, and its impl (+0x2d8) is a
//   trigger/hold BYTE getter, not a UID reader. The ark owns the whole
//   card flow internally (ENTRYFLOW scenes) and every consumer reads the
//   MdxHWIO object's card fields, all written by `MdxHWIO::stepUpdate`
//   (`FUN_1800ce320`)'s reader state machine.
// - Menu buttons (deploy #16 correction — the first shape missed the
//   operator IO test menu): the ark's raw-digest LEVEL reader
//   `FUN_18007e910` ORs a dormant per-player OVERRIDE WORD
//   (`DAT_180c47f50[player]`, one reader / zero writers — the ark's own
//   dev-build injection surface, spotted in deploy #5) into every read.
//   stepUpdate copies those override'd reads into the object level bytes
//   (+0x61A..), the panel counters consume them, and the TEST MENU reads
//   the digest level directly — so writing the override word covers every
//   consumer through the ark's own front door. The EDGE bytes (+0x60D..)
//   however come from `FUN_180084850` = `~prev & cur` of the RAW digest
//   (no override), so injected presses never edge naturally: the
//   dispatcher detour synthesizes the rising-edge byte post-original.
//   (Deploy #16 also proved the first implementation had the byte
//   semantics swapped: +0x61A.. is the LEVEL byte, +0x60D.. the EDGE —
//   in-game nav worked by acting as auto-repeat, the test menu didn't.)
// - Card-in rides the dispatcher detour: replicate exactly the
//   writes stepUpdate's physical-card path performs (verified against
//   its decompilation and the acio decoder `FUN_18007f250`).
// - Pinpad: the 10-key vtable impl (+0x308 = `FUN_1800c9420`,
//   `(this, player, *buf1[12], *buf2[12])`, one-hot) is the single
//   funnel — its keycode source `FUN_18007ecd0` has no other caller —
//   so one impl detour covers the export and internal PIN scenes.
//
// MdxHWIO field map (verified):
//   menu LEVEL bytes (stepUpdate ← override'd digest read): P1
//     Start 0x61A, Left 0x61B, Right 0x61C, Up 0x61D, Down 0x61E;
//     P2 = P1 + 5 (0x61F..0x623). Not written directly — fed by the
//     override word.
//   menu EDGE bytes (stepUpdate ← raw-digest edge): P1 Start 0x60D,
//     Left 0x60E, Right 0x60F, Up 0x610, Down 0x611; P2 = P1 + 5
//     (0x612..0x616). Synthesized for injected presses.
//   digest mask bits (FUN_18007e910's 2nd arg / the override word):
//     Start 0x01, Left 0x02, Right 0x04, Up 0x08, Down 0x10.
//   card block: base +0x5BC (P1) / +0x5D4 (P2), stride 0x18:
//     {uid[8] @+0, type_bool @+8, presence @+9, type_int @+0xC,
//      debounce_count @+0x14}. Card type: uid[0]==0xE0 ⇒ 1 (ISO15693)
//     else 2 (FeliCa) — the acio decoder's own rule.
//   card trigger +0x60B/+0x60C (set once on a NEW uid), card hold
//     +0x624/+0x625 (held while the card is on the reader) — both
//     zeroed at stepUpdate's top every frame.
//   scan-enabled gate +0x6F8/+0x6F9 (set by MdxHWIO::setEAPassReadStart
//     — the entry flow arms the reader on its card-wait screens).

/// Menu-button EDGE byte offsets in the MdxHWIO object, indexed
/// [inject_slot::MENU_*][player] (the level bytes are fed by the
/// override word through stepUpdate — never written directly).
const MENU_EDGE_OFFSETS: [[usize; 2]; 5] = [
    [0x60D, 0x612], // Start
    [0x610, 0x615], // Up
    [0x611, 0x616], // Down
    [0x60E, 0x613], // Left
    [0x60F, 0x614], // Right
];

/// Raw-digest mask bit per menu button (the override word's bit space),
/// indexed by `inject_slot::MENU_*` order.
const MENU_DIGEST_MASK: [u32; 5] = [0x01, 0x08, 0x10, 0x02, 0x04];

/// Address of the ark's per-player digest override words (2 × u32), or 0
/// when the AOB didn't resolve. Derived in the lazy installer from the
/// `TEST [RBP+RDI*4+disp32], ESI` in `FUN_18007e910` (RBP = module base,
/// so disp32 is module-relative).
static MENU_OVERRIDE_BASE: AtomicUsize = AtomicUsize::new(0);

/// AOB for the override-word TEST inside the digest LEVEL reader:
/// `CALL rel32; TEST [RBP+RDI*4+disp32], ESI; MOV RBX,[RSP+0x30]`.
const MENU_OVERRIDE_PATTERN: &str = "E8 ?? ?? ?? ?? 85 B4 BD ?? ?? ?? ?? 48 8B 5C 24 30";

/// Card block bases per player (see field map above).
const CARD_BLOCK_BASE: [usize; 2] = [0x5BC, 0x5D4];
const CARD_TRIGGER_OFFSET: [usize; 2] = [0x60B, 0x60C];
const CARD_HOLD_OFFSET: [usize; 2] = [0x624, 0x625];
const CARD_SCAN_ENABLED_OFFSET: [usize; 2] = [0x6F8, 0x6F9];

/// IO dispatcher (vtable +0x28) and 10-key impl (vtable +0x308) shapes.
type IoDispatchFn = unsafe extern "C" fn(*mut std::ffi::c_void) -> u64;
type TenKeyImplFn =
    unsafe extern "C" fn(*mut std::ffi::c_void, i32, *mut [u8; 12], *mut [u8; 12]) -> u64;

static mut IO_DISPATCH_DETOUR: Option<GenericDetour<IoDispatchFn>> = None;
static mut TENKEY_IMPL_DETOUR: Option<GenericDetour<TenKeyImplFn>> = None;

/// Per-(player, menu-button) previous held state for trigger-edge
/// synthesis (mirrors PANEL_PREV_HELD).
static MENU_PREV_HELD: [[AtomicBool; 5]; 2] = {
    #[allow(clippy::declare_interior_mutable_const)]
    const B: AtomicBool = AtomicBool::new(false);
    [[B; 5], [B; 5]]
};

/// Card scan episodes (one per player): UID bytes packed LE into a u64
/// (memory order preserved on write), frames remaining, and a one-shot
/// "assert the trigger byte" latch for the episode's first armed frame.
static CARD_UID: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
static CARD_FRAMES: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];
static CARD_TRIGGER_PENDING: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
/// One-shot INFO/WARN latches for card episodes.
static CARD_INJECT_SEEN: AtomicBool = AtomicBool::new(false);
static CARD_NOT_ARMED_WARNED: AtomicBool = AtomicBool::new(false);

/// How long an injected "card on the reader" episode lasts, in ark IO
/// frames (~60/s at stock refresh). Roughly a 2 s card tap.
const CARD_EPISODE_FRAMES: u32 = 120;

/// Request a one-shot card scan for `player` (0/1) with the given raw
/// 8-byte UID. Callable from any thread (atomics only). The injection
/// itself happens in the IO-dispatcher detour and only takes effect
/// while the ark has that player's reader armed (its card-wait
/// screens); episodes always drain so a press on the wrong screen
/// cannot fire minutes later.
pub fn request_card_scan(player: usize, uid: [u8; 8]) {
    if player >= 2 {
        return;
    }
    CARD_UID[player].store(u64::from_le_bytes(uid), Ordering::Relaxed);
    CARD_TRIGGER_PENDING[player].store(true, Ordering::Relaxed);
    CARD_FRAMES[player].store(CARD_EPISODE_FRAMES, Ordering::Release);
    log_info!(
        "InputManager: card scan requested (player={}, uid[0]={:#04x})",
        player,
        uid[0]
    );
}

/// Snapshot the provider's menu-button state: per-player digest masks
/// (for the override words) + per-button held levels (for the edge
/// synthesis). Panic-free.
fn menu_held_snapshot() -> ([u32; 2], [[bool; 5]; 2]) {
    let mut masks = [0u32; 2];
    let mut held = [[false; 5]; 2];
    let raw = INJECTION_PROVIDER.load(Ordering::Acquire);
    if raw == 0 {
        return (masks, held);
    }
    // SAFETY: stored from a valid `InjectionProvider` fn pointer.
    let provider: InjectionProvider = unsafe { std::mem::transmute(raw) };
    for player in 0..2usize {
        for btn in 0..5usize {
            if provider(player, inject_slot::MENU_START + btn) {
                held[player][btn] = true;
                masks[player] |= MENU_DIGEST_MASK[btn];
            }
        }
    }
    (masks, held)
}

/// Publish the per-player override words (the ark's own dev injection
/// surface — nothing else ever writes them). Called every dispatcher
/// frame PRE-original so stepUpdate's level-byte copies and the test
/// menu's direct digest reads both see this frame's state; storing 0
/// when idle keeps the surface clean across disable.
unsafe fn write_menu_override(masks: [u32; 2]) {
    let base = MENU_OVERRIDE_BASE.load(Ordering::Acquire);
    if base == 0 {
        return;
    }
    let words = base as *mut u32;
    words.write_volatile(masks[0]);
    words.add(1).write_volatile(masks[1]);
}

/// Cancel any in-flight card scan episodes (the SMX mod's disable — a
/// frozen episode would otherwise resume on re-enable and fire whenever
/// the reader next arms).
pub fn clear_card_scans() {
    for player in 0..2 {
        CARD_FRAMES[player].store(0, Ordering::Release);
        CARD_TRIGGER_PENDING[player].store(false, Ordering::Relaxed);
    }
}

/// Post-dispatcher injection body: synthesize menu EDGE bytes for rising
/// edges and drive any pending card episode. `this` is the live MdxHWIO
/// object (bounds-checked by the installer's vtable validation; the
/// dispatcher is only ever invoked on the singleton).
///
/// # Safety
/// Called from the dispatcher detour with the original already run.
unsafe fn overlay_inject_post_dispatch(this: *mut u8, held: &[[bool; 5]; 2]) {
    if this.is_null() {
        return;
    }

    // Menu EDGE bytes: the raw-digest edge derivation never sees the
    // override word, so injected presses edge here — one pulse per
    // press via the prev-held latch (stepUpdate rewrote the byte from
    // the raw digest just now; our OR lasts exactly this frame).
    for player in 0..2usize {
        for btn in 0..5usize {
            let now = held[player][btn];
            let prev = MENU_PREV_HELD[player][btn].swap(now, Ordering::AcqRel);
            if now && !prev {
                *this.add(MENU_EDGE_OFFSETS[btn][player]) |= 1;
            }
        }
    }

    // Card episodes.
    for player in 0..2usize {
        let frames = CARD_FRAMES[player].load(Ordering::Acquire);
        if frames == 0 {
            continue;
        }
        CARD_FRAMES[player].store(frames - 1, Ordering::Release);
        if frames == 1 {
            // Episode over: clear a never-consumed trigger latch.
            CARD_TRIGGER_PENDING[player].store(false, Ordering::Relaxed);
        }
        // Only inject while the ark has this player's reader armed
        // (the entry flow's card-wait screens) — mirrors a physical
        // card only being read while the reader scans.
        if *this.add(CARD_SCAN_ENABLED_OFFSET[player]) == 0 {
            if !CARD_NOT_ARMED_WARNED.swap(true, Ordering::Relaxed) {
                log_warn!(
                    "InputManager: card scan requested while reader not scanning (player={}) -- press INSERT CARD on the card entry screen",
                    player
                );
            }
            continue;
        }
        let uid = CARD_UID[player].load(Ordering::Relaxed).to_le_bytes();
        let block = this.add(CARD_BLOCK_BASE[player]);
        // Replicate stepUpdate's physical-card writes exactly.
        std::ptr::copy_nonoverlapping(uid.as_ptr(), block, 8);
        let type_int: u8 = if uid[0] == 0xE0 { 1 } else { 2 };
        *block.add(8) = type_int - 1; // type_bool (0 = ISO15693, 1 = FeliCa)
        *block.add(9) = 1; // presence
        *(block.add(0xC) as *mut u32) = type_int as u32;
        *(block.add(0x14) as *mut u32) = 2; // debounce count (nonzero)
        *this.add(CARD_HOLD_OFFSET[player]) = 1;
        if CARD_TRIGGER_PENDING[player].swap(false, Ordering::AcqRel) {
            *this.add(CARD_TRIGGER_OFFSET[player]) = 1;
        }
        if !CARD_INJECT_SEEN.swap(true, Ordering::AcqRel) {
            log_info!(
                "InputManager: injecting card scan (player={}, type={})",
                player,
                type_int
            );
        }
    }
}

unsafe extern "C" fn io_dispatch_detour(this: *mut std::ffi::c_void) -> u64 {
    std::panic::catch_unwind(|| {
        // Pre-original: publish the menu override words so this frame's
        // stepUpdate (level bytes, panel counters) and the test menu's
        // direct digest reads see the injected state. Idle = all zero.
        let (masks, held) = if SMX_INJECTION_ACTIVE.load(Ordering::Acquire) {
            menu_held_snapshot()
        } else {
            ([0u32; 2], [[false; 5]; 2])
        };
        write_menu_override(masks);
        let ret = match &*std::ptr::addr_of!(IO_DISPATCH_DETOUR) {
            Some(hook) => hook.call(this),
            None => 0,
        };
        if SMX_INJECTION_ACTIVE.load(Ordering::Acquire) {
            overlay_inject_post_dispatch(this as *mut u8, &held);
        }
        ret
    })
    .unwrap_or(0)
}

unsafe extern "C" fn tenkey_impl_detour(
    this: *mut std::ffi::c_void,
    player: i32,
    buf1: *mut [u8; 12],
    buf2: *mut [u8; 12],
) -> u64 {
    std::panic::catch_unwind(|| {
        let ret = match &*std::ptr::addr_of!(TENKEY_IMPL_DETOUR) {
            Some(hook) => hook.call(this, player, buf1, buf2),
            None => 0,
        };
        // OR injected pinpad keys one-hot into both buffers — for the
        // GAME and for the modpack's own poll alike (deploy #17
        // feedback: touch pinpad presses should drive the modpack's
        // pinpad gestures — mod-menu 0-0-0, quick restart/fail, quick
        // logout — exactly like cabinet pinpad presses; the original
        // IN_MODPACK_POLL exclusion made touch keys game-only).
        if SMX_INJECTION_ACTIVE.load(Ordering::Acquire)
            && (0..2).contains(&player)
            && !buf1.is_null()
            && !buf2.is_null()
        {
            let raw = INJECTION_PROVIDER.load(Ordering::Acquire);
            if raw != 0 {
                // SAFETY: stored from a valid `InjectionProvider` fn pointer.
                let provider: InjectionProvider = std::mem::transmute(raw);
                for k in 0..inject_slot::PINPAD_KEYS {
                    if provider(player as usize, inject_slot::PINPAD_BASE + k) {
                        (*buf1)[k] |= 1;
                        (*buf2)[k] |= 1;
                    }
                }
            }
        }
        // Generic one-shot pinpad pulses (request_pinpad_pulse) — same
        // visibility rules as the SMX block above (game + modpack poll),
        // but independent of the SMX provider/active gates.
        if (0..2).contains(&player) && !buf1.is_null() && !buf2.is_null() {
            let deadlines = &PINPAD_PULSE_DEADLINE[player as usize];
            let mut now = 0u64;
            let mut now_read = false;
            for (k, deadline) in deadlines.iter().enumerate() {
                if deadline.load(Ordering::Acquire) == 0 {
                    continue;
                }
                if !now_read {
                    now = pulse_now_ms();
                    now_read = true;
                }
                if deadline.load(Ordering::Acquire) > now {
                    (*buf1)[k] |= 1;
                    (*buf2)[k] |= 1;
                } else {
                    deadline.store(0, Ordering::Release);
                }
            }
        }
        ret
    })
    .unwrap_or(0)
}

/// Lazily install the four panel vtable-impl detours. Called from [`poll`]
/// once the ark IO singleton is live and a provider is registered. The
/// implementation pointers are read from the live object's vtable (no AOB,
/// build-independent) and sanity-checked against the ark module's range.
unsafe fn install_panel_impl_hooks(singleton_obj: usize, slots: ArkVtableSlots) {
    let Some(ark) = crate::core::module_resolver::resolve_ark_module() else {
        log_warn!("InputManager: ark module unresolved -- panel injection unavailable");
        return;
    };
    let layout_verified =
        slots.tenkey == ARK_VERIFIED_TENKEY_SLOT && slots.panels == ARK_VERIFIED_PANEL_SLOTS;
    log_info!(
        "InputManager: ark vtable slots derived from exports (10-key +{:#x}, panels +{:#x}..+{:#x}, field-map verified={})",
        slots.tenkey,
        slots.panels[0],
        slots.panels[3],
        layout_verified
    );
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
    for (i, offset) in slots.panels.iter().enumerate() {
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
    // The touch-overlay targets: the IO dispatcher (+0x28, menu-byte +
    // card injection) and the 10-key impl (+0x308, pinpad injection).
    // Resolved alongside the panels from the same live vtable; a miss
    // degrades only the overlay slots (panels install regardless).
    let io_dispatch = std::ptr::read_volatile((vtable + 0x28) as *const usize);
    let tenkey_impl = std::ptr::read_volatile((vtable + slots.tenkey) as *const usize);
    // The impls are distinct functions on every known build; if a
    // future build merges them, a double detour on one address would fail —
    // bail loudly instead.
    let mut all = [
        targets[0],
        targets[1],
        targets[2],
        targets[3],
        io_dispatch,
        tenkey_impl,
    ];
    all.sort_unstable();
    if all.windows(2).any(|w| w[0] == w[1]) {
        log_warn!("InputManager: ark vtable impls alias -- injection unavailable");
        return;
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

    // Touch-overlay detours (menu bytes + card episodes + pinpad).
    // Best-effort: a miss leaves the panels working and the overlay
    // slots inert with one WARN each.
    if !layout_verified {
        // The dispatcher detour replicates stepUpdate's MdxHWIO field
        // writes (menu edge bytes, card block, scan gates) at offsets only
        // verified for the 2026 ark layout — never install it against an
        // unknown layout. Panels + pinpad pulses above go through the
        // derived slots and stay available.
        log_warn!(
            "InputManager: ark vtable layout differs from the verified one -- menu/card injection unavailable (panels + pinpad still injected)"
        );
    } else if in_ark(io_dispatch) {
        if let Err(e) = crate::core::hooks::install_enabled(
            std::ptr::addr_of_mut!(IO_DISPATCH_DETOUR),
            std::mem::transmute::<usize, IoDispatchFn>(io_dispatch),
            io_dispatch_detour as IoDispatchFn,
        ) {
            log_warn!(
                "InputManager: failed to install IO-dispatcher detour: {} -- menu/card injection unavailable",
                e
            );
        } else {
            log_info!(
                "InputManager: IO-dispatcher injection detour installed ({:#x})",
                io_dispatch
            );
        }
    } else {
        log_warn!(
            "InputManager: IO-dispatcher vtable slot outside ark module -- menu/card injection unavailable"
        );
    }
    if in_ark(tenkey_impl) {
        if let Err(e) = crate::core::hooks::install_enabled(
            std::ptr::addr_of_mut!(TENKEY_IMPL_DETOUR),
            std::mem::transmute::<usize, TenKeyImplFn>(tenkey_impl),
            tenkey_impl_detour as TenKeyImplFn,
        ) {
            log_warn!(
                "InputManager: failed to install 10-key impl detour: {} -- pinpad injection unavailable",
                e
            );
        } else {
            log_info!(
                "InputManager: 10-key impl injection detour installed ({:#x})",
                tenkey_impl
            );
        }
    } else {
        log_warn!(
            "InputManager: 10-key vtable slot outside ark module -- pinpad injection unavailable"
        );
    }

    // Resolve the ark's per-player digest OVERRIDE WORDS (menu-button
    // level injection incl. the operator test menu — see the module
    // comment). Exactly-one-match AOB on the ark module; the disp32 in
    // `TEST [RBP+RDI*4+disp32], ESI` is module-base-relative (RBP holds
    // the image base). A miss degrades menu injection with one WARN.
    let matches = crate::core::scanner::scan_pattern_all(ark.base, ark.size, MENU_OVERRIDE_PATTERN);
    if matches.len() == 1 {
        let disp = std::ptr::read_unaligned((matches[0].address as usize + 8) as *const u32);
        let addr = ark_lo + disp as usize;
        if addr >= ark_lo && addr + 8 <= ark_hi {
            MENU_OVERRIDE_BASE.store(addr, Ordering::Release);
            log_info!(
                "InputManager: digest override words resolved ({:#x}, module+{:#x})",
                addr,
                disp
            );
        } else {
            log_warn!(
                "InputManager: override-word disp {:#x} outside ark module -- menu injection unavailable",
                disp
            );
        }
    } else {
        log_warn!(
            "InputManager: digest override AOB matched {} times (want 1) -- menu injection unavailable",
            matches.len()
        );
    }
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

/// Shared body: forward to the original via `detour`, then zero the
/// out-params for game-side callers while suppression is active. SMX
/// touch-overlay menu injection does NOT happen here: it flows through
/// the ark's digest override words upstream (see the dispatcher detour),
/// so the original impl already returns injected state — an additional
/// OR here would double-apply. Suppression still runs last, so an open
/// mod menu wins over injected input exactly as it does over cabinet
/// input.
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
            // mod's enable) or a pinpad-pulse consumer declared itself
            // (request_pinpad_injection). One attempt per process; runs on
            // the render thread, which is fine for retour installs (every
            // other hook installs while game threads run too).
            if (INJECTION_PROVIDER.load(Ordering::Acquire) != 0
                || AUX_INJECTION_WANTED.load(Ordering::Acquire))
                && !PANEL_IMPL_INSTALL_ATTEMPTED.swap(true, Ordering::AcqRel)
            {
                match mgr.exports.as_ref().and_then(derive_ark_vtable_slots) {
                    Some(slots) => unsafe { install_panel_impl_hooks(singleton_obj, slots) },
                    None => log_warn!(
                        "InputManager: could not derive the arkMDXIO vtable slots from the export wrappers -- injection detours NOT installed"
                    ),
                }
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
        let (fns, get_10key, panel_fns) = match &mgr.exports {
            Some(e) => (
                [e.get_start, e.get_up, e.get_down, e.get_left, e.get_right],
                e.get_10key,
                e.panel_getters,
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

        // Dance-pad stage panels (opt-in — see set_panel_polling). The
        // exports aren't suppression-detoured, but funnel through the same
        // vtable impls the SMX injection covers, so injected pad presses
        // are visible here like cabinet ones.
        if PANEL_POLLING.load(Ordering::Acquire) {
            if let Some(panel_fns) = panel_fns {
                for (i, &bit) in PANEL_BUTTONS.iter().enumerate() {
                    let mut trigger: u8 = 0;
                    let mut hold: u8 = 0;
                    let mut release: u8 = 0;
                    let mut counter: u32 = 0;
                    unsafe {
                        panel_fns[i](
                            player as i32,
                            &mut trigger,
                            &mut hold,
                            &mut release,
                            &mut counter,
                        )
                    };
                    let active = trigger != 0 || hold != 0;
                    state =
                        update_button(state, bit, active, ages, player, RELEASE_DELAY, &mut events);
                }
            }
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

        // Stage-panel getters: best-effort (all-or-nothing). Panel button
        // events are simply unavailable if a future build drops an export.
        let panel_getters = (|| -> Option<[PanelGetFn; 4]> {
            let mut fns = [None; 4];
            for (i, name) in [
                "arkMDXGetPanelUp",
                "arkMDXGetPanelDown",
                "arkMDXGetPanelLeft",
                "arkMDXGetPanelRight",
            ]
            .iter()
            .enumerate()
            {
                fns[i] = Some(std::mem::transmute::<*const (), PanelGetFn>(resolve(name)?));
            }
            Some([fns[0]?, fns[1]?, fns[2]?, fns[3]?])
        })();
        if panel_getters.is_none() {
            log_warn!(
                "InputManager: arkMDXGetPanel* exports unresolved -- panel events unavailable"
            );
        }

        Some(ArkExports {
            get_start,
            get_up,
            get_down,
            get_left,
            get_right,
            get_10key,
            panel_getters,
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
