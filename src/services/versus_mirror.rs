//! Versus option-row mirror — the shared "one value governs both players"
//! driver for custom-option rows whose backing mechanism is cabinet-global.
//!
//! Some features expose per-player rows but can only APPLY one value for
//! the whole cabinet (the song-rate engine has one clock factor and one
//! dance bank; training mode's section bounds move the one shared
//! timeline). In versus those rows are MIRRORED: while both sides are
//! entered, the registered rows hold one shared value on both sides, with
//! P1 as the authoritative initial seed.
//!
//! Mechanics (the pattern proven by the SONG SPEED versus mirror,
//! 2026-08-31, now shared with training mode):
//!
//! - **Engage** on the first SONG SELECT frame with both sides entered
//!   (per-frame driver via `input_manager::on_frame`). Song select is
//!   deliberate: edits are only possible there, and by the first
//!   song-select frame both sides' network profile loads have resolved —
//!   so P1's value genuinely seeds the shared state instead of racing a
//!   late P2 profile load. Engaging seeds P2 ← P1 for every registered
//!   row via [`custom_options::set_value`] (fires P2's `on_change`, so
//!   each mod's runtime state follows; persistence is pull-based at save
//!   time, so both profiles save the shared value at logout).
//! - **Live mirror**: each registered row's `on_change` calls
//!   [`mirror_edit`] at its tail — while engaged, the edit propagates to
//!   the OTHER side through `set_value` (the framework-documented
//!   cross-side sync; its unchanged-value check terminates the recursion
//!   at depth one). Last writer wins — no contention policy, matching the
//!   per-song-judgement-offsets precedent. Derived side effects (e.g.
//!   training's silent sibling nudge) propagate by RE-EXECUTION: the
//!   other side's `on_change` runs the same logic over the same mirrored
//!   inputs, so they re-derive identically rather than being copied.
//! - **Disengage** the instant either side's entered flag drops (any
//!   scene), so a finished session can never leak the mirror into the
//!   next one's card-in loads.
//!
//! Registration is dynamic (mods register their rows at enable and
//! unregister at disable); a registration landing while the mirror is
//! already engaged seeds the new rows immediately.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use once_cell::sync::Lazy;

use crate::log_info;
use crate::services::{
    custom_options, input_manager, options_scroll, scene_manager, stage_records,
};
use crate::types::scenes::scene;

/// The registered row ids (static strs — option ids are compile-time
/// constants in every consumer).
static IDS: Lazy<Mutex<Vec<&'static str>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// True while the mirror is engaged (both sides entered, latched at song
/// select — see the module doc for why song select).
static ENGAGED: AtomicBool = AtomicBool::new(false);

/// One-shot latch for the frame-callback registration (the callback
/// self-gates on the registry being non-empty; no removal needed).
static FRAME_CALLBACK_REGISTERED: AtomicBool = AtomicBool::new(false);

/// Lock [`IDS`], recovering from poison (callers are hook-adjacent).
fn lock_ids() -> MutexGuard<'static, Vec<&'static str>> {
    IDS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Register rows for versus mirroring (mod enable). Installs the frame
/// driver on first use; if the mirror is already engaged (a mod enabled
/// live mid-versus at song select), the new rows seed P2 ← P1 immediately.
pub fn register(ids: &[&'static str]) {
    {
        let mut list = lock_ids();
        for id in ids {
            if !list.contains(id) {
                list.push(id);
            }
        }
    }
    if ENGAGED.load(Ordering::Acquire) {
        seed_p2_from_p1(ids);
    }
    if !FRAME_CALLBACK_REGISTERED.swap(true, Ordering::AcqRel) {
        input_manager::on_frame(Arc::new(on_frame));
    }
}

/// Remove rows from the mirror (mod disable). Values already mirrored
/// stay as they are — future edits simply stop propagating.
pub fn unregister(ids: &[&'static str]) {
    lock_ids().retain(|id| !ids.contains(id));
}

/// Whether the versus mirror is currently engaged.
#[must_use]
pub fn engaged() -> bool {
    ENGAGED.load(Ordering::Acquire)
}

/// Live mirror leg — call at the tail of a registered row's `on_change`.
/// While engaged, propagates the edit to the OTHER side (`set_value`
/// fires that side's `on_change`; its unchanged-value check terminates
/// the recursion), then reapplies that side's scroll mask so ShowWhen
/// children track a mirrored parent on the same frame (internally
/// tab-gated — a no-op unless that side is viewing the Mods tab).
pub fn mirror_edit(id: &str, side: u8, value: i32) {
    if side >= 2 || !ENGAGED.load(Ordering::Acquire) {
        return;
    }
    let other = 1 - side;
    custom_options::set_value(id, other, value);
    options_scroll::reapply_mask_for_side(other);
}

/// Seed P2's rows from P1's current values (the engage-time authoritative
/// seed). `set_value` fires P2's `on_change`, so each consumer's runtime
/// state (desired atomics, bounds, …) follows automatically.
fn seed_p2_from_p1(ids: &[&'static str]) {
    for id in ids {
        if let Some(value) = custom_options::get_value(0, id) {
            custom_options::set_value(id, 1, value);
        }
    }
    options_scroll::reapply_mask_for_side(1);
}

/// Per-frame driver (render/game thread; idle path: one atomic load +
/// two entered-flag reads). Engages at the first song-select frame with
/// both sides entered; disengages the moment the session stops being
/// versus (any scene).
fn on_frame() {
    let both_entered = stage_records::side_entered(0) == Some(true)
        && stage_records::side_entered(1) == Some(true);
    if !both_entered {
        ENGAGED.store(false, Ordering::Release);
        return;
    }
    if ENGAGED.load(Ordering::Acquire) || scene_manager::current_scene() != scene::SONG_SELECT {
        return;
    }
    let ids: Vec<&'static str> = lock_ids().clone();
    if ids.is_empty() {
        // Nothing to mirror — stay disengaged so a later registration
        // (mod enabled mid-versus) engages through this same path.
        return;
    }
    ENGAGED.store(true, Ordering::Release);
    seed_p2_from_p1(&ids);
    log_info!(
        "versus_mirror: engaged — {} option row(s) mirrored across sides (P1 seeds)",
        ids.len()
    );
}
