//! Network persistence wiring for Per-Song Judgement Offsets (plan Step 6;
//! design §Components → string-field extension, Detailed Requirements 6/7).
//!
//! The mod is the first consumer of `custom_options_persistence`'s
//! string-field registry: the entered side's whole session map rides every
//! player-data save as the `mod_judge_offsets` kbin `str` child
//! (`code|offset|...`, sorted, capped), and a profile load replaces that
//! side's session map at SONG_SELECT entry — strictly after the card-in
//! callback has reset the side to the CSV baseline, so a profile with no
//! stored field lands on the baseline and a profile with one gets exactly
//! its own map (design merge rule, register D6).

use super::store;
use crate::services::custom_options_persistence;
use crate::{log_info, log_warn};

pub const WIRE_NAME: &str = "mod_judge_offsets";

/// Register the wire field + card-in callback (called from `enable()` after
/// the row registration succeeded; idempotent — the fns themselves gate on
/// the mod's active flag so a later disable makes them inert).
pub fn register() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static REGISTERED: AtomicBool = AtomicBool::new(false);
    if REGISTERED.swap(true, Ordering::AcqRel) {
        return;
    }
    custom_options_persistence::register_string_field(WIRE_NAME, save_offsets, load_offsets);
    custom_options_persistence::register_card_in_callback(on_card_in);
    log_info!(
        "judgement_offsets: network persistence registered ({})",
        WIRE_NAME
    );
}

/// Save producer: the entered side's encoded session map. `None` (field
/// omitted) while the mod is inactive or the store never armed — an empty
/// map encodes as `Some("")`, the server-clear signal.
fn save_offsets(side: u8) -> Option<String> {
    if !super::is_active() {
        return None;
    }
    store::with_store(|s| {
        if !s.is_armed() {
            return None;
        }
        Some(s.encode_side(side as usize))
    })
}

/// Load consumer: replace the side's session map with the profile's.
fn load_offsets(side: u8, value: &str) {
    if !super::is_active() {
        return;
    }
    let (stats, count) = store::with_store(|s| {
        let stats = s.apply_server_string(side as usize, value);
        let count = value.split('|').count() / 2;
        (stats, count)
    });
    if !stats.is_clean() {
        log_warn!(
            "judgement_offsets: P{} server offsets partially malformed ({} skipped, {} clamped, truncated={})",
            side + 1,
            stats.skipped,
            stats.clamped,
            stats.truncated
        );
    }
    log_info!(
        "judgement_offsets: P{} offsets loaded from profile (~{} song(s))",
        side + 1,
        count
    );
}

/// Card-in: back to the CSV baseline before any server data applies.
fn on_card_in(side: u8) {
    if !super::is_active() {
        return;
    }
    store::with_store(|s| s.reset_to_baseline(side as usize));
    log_info!(
        "judgement_offsets: P{} session reset to CSV baseline (card-in)",
        side + 1
    );
}
