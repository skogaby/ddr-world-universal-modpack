//! Song-select S-MFC lamp state (server-upload design §4.5, D13–D15): the
//! per-side set of `(mcode, chart)` charts the player has S-MFC'd, fed from
//! two sources —
//!
//! 1. the backend's `option/smarv_scores` load field (`mcode:chart:clearkind|…`,
//!    entries whose S-Marv clear kind differs from the stock one — in
//!    practice the S-MFCs, 11), delivered per side by the persistence
//!    service's string-field registry after the card-in deferral; REPLACES
//!    the side's set;
//! 2. the upload producer, the moment it emits an `s_marv` node with
//!    `clearkind == 11` (so the violet lamp shows at the very next song
//!    select, before any server round-trip).
//!
//! The codec (`decode`/`encode`) is pure and shares its test vectors with the
//! bemani-buddy side. The badge itself (the header-card detour + texture) is
//! `lamp_badge` (plan Step 8); this module is state + wire only.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use once_cell::sync::Lazy;

use crate::log_info;
use crate::services::custom_options_persistence as persistence;

use super::lamp_codec::{decode, smfc_set, WIRE_NAME};

/// Per-side S-MFC chart sets.
static SETS: Lazy<[Mutex<HashSet<(i32, i32)>>; 2]> =
    Lazy::new(|| [Mutex::new(HashSet::new()), Mutex::new(HashSet::new())]);

/// Registration is idempotent (the persistence registry has no unregister
/// for string fields; the callbacks gate on `ACTIVE`).
static REGISTERED: AtomicBool = AtomicBool::new(false);
/// Mod-enabled gate for the load callback (a disabled mod ignores the field).
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Register the load-side field + card-in reset with the persistence
/// service (idempotent) and mark the mod active for the callbacks.
pub fn activate() {
    ACTIVE.store(true, Ordering::Release);
    if !REGISTERED.swap(true, Ordering::AcqRel) {
        persistence::register_string_field(WIRE_NAME, |_side| None, on_load);
        persistence::register_card_in_callback(clear);
    }
}

/// Stop reacting to loads and forget both sets (mod disable).
pub fn deactivate() {
    ACTIVE.store(false, Ordering::Release);
    clear(0);
    clear(1);
}

/// Load-side consumer: REPLACE the side's set with the wire list's S-MFCs.
fn on_load(side: u8, text: &str) {
    if !ACTIVE.load(Ordering::Acquire) || side > 1 {
        return;
    }
    let set = smfc_set(&decode(text));
    let n = set.len();
    if let Ok(mut s) = SETS[side as usize].lock() {
        *s = set;
    }
    log_info!(
        "SMarvelous: lamp set side={} loaded {} S-MFC chart(s) from server",
        side,
        n
    );
}

/// Card-in reset (the persistence service fires it before any load apply).
pub fn clear(side: u8) {
    if side > 1 {
        return;
    }
    if let Ok(mut s) = SETS[side as usize].lock() {
        s.clear();
    }
}

/// Local feed from the upload producer: this play was an S-MFC.
pub fn insert_local(side: usize, mcode: i32, chart: i32) {
    if side > 1 {
        return;
    }
    if let Ok(mut s) = SETS[side].lock() {
        if s.insert((mcode, chart)) {
            log_info!(
                "SMarvelous: lamp set side={} +local S-MFC mcode={} chart={}",
                side,
                mcode,
                chart
            );
        }
    }
}

/// Whether the side has an S-MFC on this chart.
pub fn is_smfc(side: usize, mcode: i32, chart: i32) -> bool {
    if side > 1 {
        return false;
    }
    SETS[side]
        .lock()
        .map(|s| s.contains(&(mcode, chart)))
        .unwrap_or(false)
}

/// Number of S-MFC charts the side currently knows (diagnostics / fast
/// empty check for the badge detour).
pub fn count(side: usize) -> usize {
    if side > 1 {
        return 0;
    }
    SETS[side].lock().map(|s| s.len()).unwrap_or(0)
}
