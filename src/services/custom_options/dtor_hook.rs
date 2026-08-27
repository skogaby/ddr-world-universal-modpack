//! OptionForm-destructor detour for eager row invalidation.
//!
//! The custom-options framework hands the game raw pointers to mod-authored
//! option rows ([`super::rows::RowSlot::row_ptr`]). The game owns those rows
//! and frees them (donor dtor → CRT free) when the options overlay closes.
//! Until this hook existed, the framework only purged its stale
//! [`super::rows`] entries lazily, at the *start* of the next builder pass —
//! so between menu-close and the next open, every stored `row_ptr` dangled.
//! Any path that wrote the row "active" byte (`+0xB8`) or read `+0x60` in that
//! window (the options-scroll mask, the ShowWhen visibility pass, container
//! resolution) dereferenced freed memory and crashed with an access violation.
//!
//! This detour closes that window. It targets `OptionForm::~OptionForm`, which
//! RE confirmed fires exactly once per carded-in side the instant the overlay
//! closes (and never while it's open). On each call it reads the player side
//! from `OptionForm+0x228` and drops that side's row entries via
//! [`super::rows::clear_side`] *before* the original destructor frees the rows,
//! so no later code can observe a dangling pointer.
//!
//! Per-side (not both sides): the side field at `+0x228` is read at detour
//! entry where it is still intact, and clearing only the closing side avoids
//! disturbing any unrelated state. (The options overlay is synchronized across
//! players — either player's close tears down both forms — so the destructor
//! fires for each side anyway; there is no "one side still open" case to
//! preserve.)
//!
//! Graceful degradation: if `optionform_dtor` doesn't resolve or the detour
//! fails to install, [`init`] returns `false`. The framework still works; the
//! defensive guards in [`super::rows`] / `options_scroll` (which no-op when a
//! side has no rows) remain the backstop against a stale deref.

use retour::GenericDetour;

use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};

use super::rows;

/// `void OptionForm::~OptionForm(OptionForm* this)`.
type OptionFormDtorFn = unsafe extern "C" fn(*mut u8);

/// Offset of the player-side field (0 = P1, 1 = P2) within an OptionForm.
/// Matches what the row builder reads at `parent+0x228`; verified live to
/// hold its value into the destructor.
const OPTIONFORM_PLAYER_SIDE_OFFSET: usize = 0x228;

static mut DTOR_HOOK: Option<GenericDetour<OptionFormDtorFn>> = None;

/// Resolve `optionform_dtor` and install the detour. Returns `true` on
/// success; any failure logs WARN and returns `false`, leaving the framework
/// functional (the rows/scroll guards remain the backstop).
pub(crate) fn init(signatures: &SignatureStore) -> bool {
    let dtor_addr = match signatures.get_address("optionform_dtor") {
        Some(a) => a,
        None => {
            log_warn!(
                "custom_options/dtor_hook: optionform_dtor not resolved — stale rows cleared only on next menu open"
            );
            return false;
        }
    };

    unsafe {
        let target: OptionFormDtorFn = std::mem::transmute(dtor_addr);
        if let Err(e) = crate::core::hooks::install_enabled(
            std::ptr::addr_of_mut!(DTOR_HOOK),
            target,
            dtor_detour,
        ) {
            log_warn!(
                "custom_options/dtor_hook: detour install failed: {:?} — eager row invalidation disabled",
                e
            );
            return false;
        }
        log_info!(
            "custom_options/dtor_hook: OptionForm dtor detour installed @ {:p}",
            dtor_addr
        );
    }
    true
}

/// Detour body. Reads the closing side, clears that side's stale row entries,
/// then runs the original destructor. Wrapped so a panic can't unwind into
/// game code.
unsafe extern "C" fn dtor_detour(this: *mut u8) {
    let _ = std::panic::catch_unwind(|| dtor_detour_body(this));
}

fn dtor_detour_body(this: *mut u8) {
    unsafe {
        let detour = match (*std::ptr::addr_of!(DTOR_HOOK)).as_ref() {
            Some(d) => d,
            None => return,
        };

        // Read the player side BEFORE the original destructor runs. The rows
        // for this side are about to be freed, so drop our pointers to them
        // first — after this, no +0xB8 / +0x60 path can observe a dangling row.
        if !this.is_null() {
            let side_raw =
                std::ptr::read_unaligned(this.add(OPTIONFORM_PLAYER_SIDE_OFFSET) as *const i32);
            if (0..=1).contains(&side_raw) {
                rows::clear_side(side_raw as u8);
                // Notify menu-close subscribers (e.g. the WebUI preview overlay)
                // before the form is freed, so they can hide/release overlays
                // tied to this side's modal.
                super::fire_menu_close(side_raw as u8);
            } else {
                log_warn!(
                    "custom_options/dtor_hook: OptionForm+0x228 = {} (expected 0 or 1) — clearing both sides",
                    side_raw
                );
                rows::clear_side(0);
                rows::clear_side(1);
                super::fire_menu_close(0);
                super::fire_menu_close(1);
            }
        }

        detour.call(this);
    }
}
