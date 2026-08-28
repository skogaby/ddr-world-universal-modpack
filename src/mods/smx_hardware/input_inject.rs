//! Input injection — wires the SMX transport's per-pad panel masks AND the
//! touch overlay's button state into `input_manager`'s injection seam.
//!
//! The provider is a plain `fn` (no captures — it's called from inside the
//! game's ark input reads) that maps an injection slot to live state:
//! - `PANEL_*` — the SMX stage pads' panel masks (Step 1).
//! - `MENU_*` — the touch overlay's menu-nav buttons (Step 3; consumed by
//!   the IO-dispatcher detour's object-byte injection).
//! - `PINPAD_*` — the touch overlay's pinpad keys (Step 3; consumed by the
//!   10-key vtable-impl detour).
//!
//! Card-in doesn't fit the boolean provider: the overlay's Insert-Card
//! press calls [`on_card_button`], which resolves the configured UID and
//! fires a one-shot `input_manager::request_card_scan` episode.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::services::input_manager::{self, inject_slot};
use crate::services::smx::{input_map, transport};
use crate::{log_info, log_warn};

use super::{overlay, overlay_model};

/// Configured card UIDs per player, packed LE into a u64 (0 = none).
/// Latched at activate() from the mod settings.
static CARD_UIDS: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];

/// The injection provider registered with `input_manager`. Panic-free, O(1):
/// one atomic load + a bit test per query.
fn provider(player: usize, slot: usize) -> bool {
    match slot {
        inject_slot::PANEL_UP => panel(player, input_map::PanelDir::Up),
        inject_slot::PANEL_DOWN => panel(player, input_map::PanelDir::Down),
        inject_slot::PANEL_LEFT => panel(player, input_map::PanelDir::Left),
        inject_slot::PANEL_RIGHT => panel(player, input_map::PanelDir::Right),
        inject_slot::MENU_START..=inject_slot::MENU_RIGHT => overlay::bit_held(
            player,
            overlay_model::BIT_MENU_BASE + (slot - inject_slot::MENU_START) as u32,
        ),
        // Pinpad keys are momentary: each touch reads as one fixed-length
        // pulse, not a level (a held level shows as a stuck key — the real
        // hardware pinpad has no held state; deploy #16 finding).
        s if (inject_slot::PINPAD_BASE..inject_slot::COUNT).contains(&s) => {
            overlay::pinpad_pulse_active(player, (s - inject_slot::PINPAD_BASE) as u8)
        }
        _ => false,
    }
}

fn panel(player: usize, dir: input_map::PanelDir) -> bool {
    input_map::panel_held(transport::input_mask(player), dir)
}

/// Register the provider and turn injection on. `cards` are the parsed
/// per-player UIDs from the config (None = no Insert-Card button).
pub fn activate(cards: [Option<[u8; 8]>; 2]) {
    for (p, card) in cards.iter().enumerate() {
        CARD_UIDS[p].store(card.map(u64::from_le_bytes).unwrap_or(0), Ordering::Relaxed);
    }
    input_manager::set_injection_provider(provider);
    input_manager::set_injection_active(true);
}

/// Turn injection off (the getter detours revert to stock behavior) and
/// cancel any in-flight card scan episode.
pub fn deactivate() {
    input_manager::set_injection_active(false);
    input_manager::clear_card_scans();
}

/// The overlay's Insert-Card button was pressed (press edge, from the
/// touch thread): fire a one-shot card scan with the configured UID.
pub fn on_card_button(player: usize) {
    if player >= 2 {
        return;
    }
    let packed = CARD_UIDS[player].load(Ordering::Relaxed);
    if packed == 0 {
        // Shouldn't happen (the button only exists with a card id), but
        // never inject a zero UID.
        log_warn!(
            "SmxHardware: INSERT CARD pressed with no card configured (P{})",
            player + 1
        );
        return;
    }
    log_info!("SmxHardware: INSERT CARD pressed (P{})", player + 1);
    input_manager::request_card_scan(player, packed.to_le_bytes());
}
