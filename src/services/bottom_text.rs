//! Bottom-text (system HUD status line) hide service.
//!
//! DDR World draws a row of status readouts along the bottom edge of the
//! screen from OUTSIDE the scene graph — a per-frame "system HUD" tick
//! (`FUN_18000a9a0` on 20260825) owns EIGHT persistent text objects in one
//! pointer array and re-writes their strings every frame:
//!
//! | slot | position (1280×720 canvas)        | content                                      |
//! |------|-----------------------------------|----------------------------------------------|
//! | 0    | centre, bottom                    | `CREDIT%s:%2d` / `FREE PLAY` / `EVENT MODE`  |
//! | 1    | centre + 96, bottom               | `%s%s:%2d/%2d` (COIN / TOKEN count)          |
//! | 2    | x = 10, bottom (left corner)      | P1 `PASELI: %s [+ %s]` / `EXTRA PASELI: %s`  |
//! | 3    | right − 10, bottom (right corner) | P2 PASELI (right-aligned)                    |
//! | 4    | centre, bottom − 80               | `SOFTWARE ID: %s` (attract idle only)        |
//! | 5    | centre, bottom − 60               | `SYSTEM  ID: %s` (attract idle only)         |
//! | 6    | centre, bottom − 40               | `HARDWARE ID: %s` (attract idle only)        |
//! | 7    | centre − 104, bottom              | `ONLINE` / `CHECKING…` / `MAINTENANCE` / …   |
//!
//! Slots 0/1/2/3/7 are recomposed by the RENDERER (`bottom_text_render`,
//! `FUN_180009630`) on every frame in ark system status ∈ {3,5,6}; in every
//! other status the tick's else-branch writes the game's own `""` literal
//! into all eight (`bottom_text_blank_loop`). Slots 4–6 are written once by
//! a latch (`FUN_180009c60`) when the attract sequence goes idle and blanked
//! when it leaves. The text objects are NOT redrawn from scratch — they hold
//! whatever string was last set — which is why the else-branch exists.
//!
//! ## Mechanism
//!
//! ONE `GenericDetour` on the renderer entry. While any contributor asks to
//! hide, the callback does not call the original and instead runs an exact
//! replica of the tick's blank loop (`set_text(**(slot+0x18), "", 1)` for
//! each non-null slot — every offset, the flag and the `""` literal itself
//! come from the `bottom_text_blank_loop` match). Because the ID latch
//! writes slots 4–6 EARLIER in the same tick, the blank covers them too.
//! With no contributor the original runs untouched. A hide→show toggle is
//! live for slots 0/1/2/3/7 (the renderer rewrites them next frame); the
//! ID lines stay blank until the attract latch next re-arms (cosmetic —
//! they are only ever drawn on the idle attract screen).
//!
//! An early-return detour WITHOUT the blank (the shape
//! `docs/hex_edit_porting.md` Hack 3 recommends) would freeze the last
//! drawn strings on screen — verified against the tick's structure, which
//! is why `derive_bottom_text` refuses to publish the renderer without the
//! slot array.
//!
//! ## Contributors
//!
//! The service is deliberately mod-agnostic: [`HideReason`] is a small
//! contributor set, the effective state is their OR. The `hide-bottom-text`
//! mod is one contributor (the operator's cabinet-wide preference); the
//! power-user-statistics horizontal readout will be another — it needs the
//! stock text gone under its own layout regardless of the operator toggle.
//! Each contributor owns exactly one bit and toggles only that bit.
//!
//! ## Degradation
//!
//! `init` requires the renderer AOB, the derived slot array and the derived
//! `""` literal (all-or-nothing in `derive_bottom_text`); any miss ⇒
//! `is_available() == false`, `set_hidden` becomes a logged no-op, stock
//! text everywhere. The detour is installed once at init and never
//! removed; disabling every contributor makes it a pure passthrough.

use std::ffi::c_void;
use std::ptr::{addr_of, addr_of_mut};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};

use retour::GenericDetour;

use crate::core::hooks;
use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};

/// Who is asking for the bottom text to be hidden. Each variant owns one
/// bit of the contributor mask; the effective hide is the OR of all bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HideReason {
    /// The operator's cabinet-wide `hide-bottom-text` mod toggle.
    HideBottomTextMod,
    /// Power User Statistics' horizontal bottom readout — replaces the
    /// stock line with its own layout, so the stock text must be gone
    /// whatever the operator toggle says.
    PowerUserStatistics,
}

impl HideReason {
    const fn bit(self) -> u32 {
        match self {
            HideReason::HideBottomTextMod => 1 << 0,
            HideReason::PowerUserStatistics => 1 << 1,
        }
    }
}

/// Number of text-object slots in the game's array (attested by the
/// `bottom_text_blank_loop` match's `MOV EDI,8` imm32 in `derive_bottom_text`).
const SLOT_COUNT: usize = 8;
/// `MOV RAX,[RAX+0x18]` — the slot wrapper's holder pointer (literal in the AOB).
const HOLDER_OFFSET: usize = 0x18;
/// `CALL [RAX+0x10]` — vtable slot of `set_text(this, str, flag)` (literal in the AOB).
const SET_TEXT_VTABLE_OFFSET: usize = 0x10;
/// `MOV R8D,R14D` with R14D == 1 throughout the tick (literal in the AOB).
const SET_TEXT_FLAG: u32 = 1;

type RenderFn = unsafe extern "C" fn() -> u64;
type SetTextFn = unsafe extern "C" fn(*mut c_void, *const u8, u32);

static mut RENDER_HOOK: Option<GenericDetour<RenderFn>> = None;

/// OR-set of [`HideReason`] bits.
static HIDE_MASK: AtomicU32 = AtomicU32::new(0);
static AVAILABLE: AtomicBool = AtomicBool::new(false);
/// The 8 × pointer text-object array (`bottom_text_slots`).
static SLOTS: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
/// The game's own `""` literal (`bottom_text_empty_str`).
static EMPTY_STR: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());

#[must_use]
pub fn is_available() -> bool {
    AVAILABLE.load(Ordering::Acquire)
}

/// Effective hide state — true while ANY contributor bit is set.
#[must_use]
pub fn is_hidden() -> bool {
    HIDE_MASK.load(Ordering::Acquire) != 0
}

/// Whether one specific contributor currently asks to hide.
#[must_use]
pub fn is_hidden_by(reason: HideReason) -> bool {
    HIDE_MASK.load(Ordering::Acquire) & reason.bit() != 0
}

/// Set or clear one contributor's hide request. Touches only that
/// contributor's bit; the effective state is the OR of all bits. Logs one
/// INFO when the EFFECTIVE state flips. A no-op (with one WARN per call
/// site's first use) when the service is unavailable.
pub fn set_hidden(reason: HideReason, hidden: bool) {
    if !is_available() {
        log_warn!(
            "bottom_text: {:?} asked hidden={} but the service is unavailable -- stock text stays",
            reason,
            hidden
        );
        return;
    }
    let before = if hidden {
        HIDE_MASK.fetch_or(reason.bit(), Ordering::AcqRel)
    } else {
        HIDE_MASK.fetch_and(!reason.bit(), Ordering::AcqRel)
    };
    let after = if hidden {
        before | reason.bit()
    } else {
        before & !reason.bit()
    };
    if (before != 0) != (after != 0) {
        log_info!(
            "bottom_text: bottom status text now {} (by {:?}; mask 0x{:X} -> 0x{:X})",
            if after != 0 { "HIDDEN" } else { "SHOWN" },
            reason,
            before,
            after
        );
    }
}

/// Replica of the system-HUD tick's else-branch: write the game's `""`
/// literal into every non-null text object. Panic-free; every pointer on
/// the chain is null-checked (the stock loop only checks the slot — the
/// extra checks cost nothing and guard a half-constructed array).
///
/// # Safety
/// Must run on the game's HUD-tick thread with `SLOTS`/`EMPTY_STR` set
/// (guaranteed by the detour being the only caller).
unsafe fn blank_all_slots() {
    let slots = SLOTS.load(Ordering::Acquire);
    let empty = EMPTY_STR.load(Ordering::Acquire);
    if slots.is_null() || empty.is_null() {
        return;
    }
    for i in 0..SLOT_COUNT {
        let slot = *(slots.add(i * 8) as *const *mut u8);
        if slot.is_null() {
            continue;
        }
        let holder = *(slot.add(HOLDER_OFFSET) as *const *mut u8);
        if holder.is_null() {
            continue;
        }
        let inner = *(holder as *const *mut c_void);
        if inner.is_null() {
            continue;
        }
        let vtable = *(inner as *const *const u8);
        if vtable.is_null() {
            continue;
        }
        let fn_ptr = *(vtable.add(SET_TEXT_VTABLE_OFFSET) as *const *const u8);
        if fn_ptr.is_null() {
            continue;
        }
        let set_text: SetTextFn = std::mem::transmute(fn_ptr);
        set_text(inner, empty, SET_TEXT_FLAG);
    }
}

unsafe extern "C" fn render_hook() -> u64 {
    let Some(hook) = (&*addr_of!(RENDER_HOOK)).as_ref() else {
        return 0;
    };
    if is_hidden() {
        // Skip the recompose; blank every slot so the persistent text
        // objects (incl. the ID lines the latch wrote earlier this tick)
        // show nothing. The original's return value is ignored by its
        // sole caller (it jumps straight to the epilogue).
        blank_all_slots();
        return 0;
    }
    hook.call()
}

/// Install the renderer detour. Requires `bottom_text_render` plus the two
/// addresses `derive_bottom_text` publishes (`bottom_text_slots`,
/// `bottom_text_empty_str`). Idempotent; returns the availability.
pub fn init(signatures: &SignatureStore) -> bool {
    if AVAILABLE.load(Ordering::Acquire) {
        return true;
    }
    let (Some(render), Some(slots), Some(empty)) = (
        signatures.get_address("bottom_text_render"),
        signatures.get_address("bottom_text_slots"),
        signatures.get_address("bottom_text_empty_str"),
    ) else {
        log_warn!(
            "bottom_text: renderer / slot array / empty literal unresolved -- hide unavailable"
        );
        return false;
    };
    SLOTS.store(slots as *mut u8, Ordering::Release);
    EMPTY_STR.store(empty as *mut u8, Ordering::Release);
    let target: RenderFn = unsafe { std::mem::transmute(render) };
    if let Err(error) =
        unsafe { hooks::install_enabled(addr_of_mut!(RENDER_HOOK), target, render_hook) }
    {
        log_warn!(
            "bottom_text: renderer hook installation failed: {} -- hide unavailable",
            error
        );
        return false;
    }
    AVAILABLE.store(true, Ordering::Release);
    log_info!("bottom_text: renderer hooked (8 text slots, passthrough until a contributor hides)");
    true
}
