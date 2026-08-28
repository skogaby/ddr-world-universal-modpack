//! Stage input injection — wires the SMX transport's per-pad panel masks
//! into `input_manager`'s additive injection seam.
//!
//! The provider is a plain `fn` (no captures — it's called from inside the
//! game's `arkMDXGetPanel*` reads) that maps an injection slot to the live
//! SMX mask bit. Menu/keypad/card slots return false until Step 3's
//! touchscreen overlay feeds them.

use crate::services::input_manager::{self, inject_slot};
use crate::services::smx::{input_map, transport};

/// The injection provider registered with `input_manager`. Panic-free, O(1):
/// one atomic mask load + a bit test.
fn provider(player: usize, slot: usize) -> bool {
    let dir = match slot {
        inject_slot::PANEL_UP => input_map::PanelDir::Up,
        inject_slot::PANEL_DOWN => input_map::PanelDir::Down,
        inject_slot::PANEL_LEFT => input_map::PanelDir::Left,
        inject_slot::PANEL_RIGHT => input_map::PanelDir::Right,
        // Menu / keypad / card slots arrive with the Step 3 overlay.
        _ => return false,
    };
    input_map::panel_held(transport::input_mask(player), dir)
}

/// Register the provider and turn injection on.
pub fn activate() {
    input_manager::set_injection_provider(provider);
    input_manager::set_injection_active(true);
}

/// Turn injection off (the getter detours revert to stock behavior).
pub fn deactivate() {
    input_manager::set_injection_active(false);
}
