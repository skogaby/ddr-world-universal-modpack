//! DDR SELECTION cabinet-wide settings: the overlay menu's DDR SELECTION
//! section (GLOBAL SETTINGS) and the `ddr_selection` config section it
//! persists.
//!
//! * **Era Cut-In** (`ddr_selection.era_cutin`, default ON): A3's era cut-in
//!   (`choice_cutin` of `common_choice_cutin000N` — the big skin-number
//!   animation with its `sele_*` SE) before the legacy stage panel. OFF skips
//!   it entirely: its packages are not requested and the stage panel plays
//!   `in` as soon as World swaps it in (A3's own "cut-in not loaded" path).
//!   Read when the stage panel is hosted (`panel::arm`), so an edit applies
//!   from the next song.
//!
//! Every row edit rewrites the WHOLE section from the live values
//! (`save_json_key` replaces the section).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::log_info;
use crate::mods::config;

/// The owning mod id (the GLOBAL SETTINGS section header is its name).
const MOD_ID: &str = "ddr-selection";
const ROW_ERA_CUTIN: &str = "ddr_selection_era_cutin";
const DEFAULT_ERA_CUTIN: bool = true;

static ERA_CUTIN: AtomicBool = AtomicBool::new(DEFAULT_ERA_CUTIN);

/// Whether the era cut-in plays before the legacy stage panel.
pub fn era_cutin() -> bool {
    ERA_CUTIN.load(Ordering::Acquire)
}

/// Seed the live values from the config file (mod enable).
pub fn load() {
    let cutin = config::get()
        .and_then(|c| c.ddr_selection.as_ref())
        .and_then(|s| s.era_cutin)
        .unwrap_or(DEFAULT_ERA_CUTIN);
    ERA_CUTIN.store(cutin, Ordering::Release);
}

fn persist_section() {
    config::save_json_key(
        "ddr_selection",
        serde_json::json!({
            "era_cutin": era_cutin(),
        }),
    );
}

/// Register the section's rows (mod enable; idempotent — rows replace by
/// key).
pub fn register_rows() {
    use crate::mods::mod_menu::{register_enum_row, EnumRowSpec};
    register_enum_row(EnumRowSpec {
        key: ROW_ERA_CUTIN.to_string(),
        label: "Era Cut-In".to_string(),
        hint: "The era animation and sound before the legacy stage panel. OFF shows the stage panel straight away (a few seconds sooner); START also skips it. Next song."
            .to_string(),
        parent_row_key: Some(MOD_ID.to_string()),
        values: vec![0, 1],
        labels: vec!["OFF".to_string(), "ON".to_string()],
        initial_value: i32::from(era_cutin()),
        on_change: Arc::new(|v| {
            let on = v != 0;
            if ERA_CUTIN.swap(on, Ordering::AcqRel) != on {
                persist_section();
                log_info!(
                    "DDR SELECTION: era cut-in {} (next song)",
                    if on { "ON" } else { "OFF" }
                );
            }
        }),
    });
}

/// Drop the section's rows (mod disable).
pub fn remove_rows() {
    crate::mods::mod_menu::remove_rows_for(&[ROW_ERA_CUTIN]);
}
