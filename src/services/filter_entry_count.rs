//! Filtersort entry-count overrides — the ONE owner of the detour on the
//! per-category filter entry-count leaf (`filter_entry_count_table`,
//! `fn(category: u32) -> u32`; stock counts 0→20 MUSIC TITLE, 1→9 VERSION,
//! …, 12→9 CLEAR TYPE).
//!
//! The count bounds the profile round-trip of each filter category's saved
//! `u64` (`filtersort/<name>`): the load apply-loop and the save mask-builder
//! only visit selection indices below it, and the category's "active"
//! indicator is built from the same mask. A mod that gives a filter menu more
//! items than stock must report the larger count or selections past the stock
//! count never persist (and never light the indicator).
//!
//! Contributors own one category each: `series_expansion` (VERSION, category
//! 1) and `improved_song_title_sorting` (MUSIC TITLE, category 0). Categories
//! without an override — including CLEAR TYPE (12), which shares VERSION's
//! switch body in the stock binary — delegate to the original.
//!
//! The detour is installed on the first [`set_override`] and never removed;
//! [`clear_override`] makes that category pass through again. Counts above 64
//! alias in the game's own bit loops (the saved value is one `u64`), so
//! overrides are clamped to 64.

use std::ptr::{addr_of, addr_of_mut};
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};
use std::sync::Mutex;

use retour::GenericDetour;

use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};

type CountFn = unsafe extern "C" fn(u32) -> u32;

/// Categories the game defines (0..=12) with headroom.
const CATEGORIES: usize = 16;
const NO_OVERRIDE: u32 = u32::MAX;
/// One saved `u64` per category.
const MAX_COUNT: u32 = 64;

static TARGET: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static OVERRIDES: [AtomicU32; CATEGORIES] = [const { AtomicU32::new(NO_OVERRIDE) }; CATEGORIES];
/// Serialises the one-time install (callers may toggle from different threads).
static INSTALL: Mutex<()> = Mutex::new(());
/// Written once under `INSTALL`, then only read by `count_hook`.
static mut HOOK: Option<GenericDetour<CountFn>> = None;

/// Record the entry-count function. Installs nothing.
pub fn init(signatures: &SignatureStore) -> bool {
    match signatures.get_address("filter_entry_count_table") {
        Some(addr) => {
            TARGET.store(addr as *mut u8, Ordering::Release);
            true
        }
        None => false,
    }
}

pub fn is_available() -> bool {
    !TARGET.load(Ordering::Acquire).is_null()
}

/// Report `count` (clamped to 64) for `category`. Installs the detour on
/// first use; false (one WARN) when the service is unavailable.
pub fn set_override(category: u32, count: u32) -> bool {
    let Some(slot) = OVERRIDES.get(category as usize) else {
        log_warn!("FilterEntryCount: category {} out of range", category);
        return false;
    };
    if !ensure_installed() {
        return false;
    }
    let count = count.min(MAX_COUNT);
    slot.store(count, Ordering::Release);
    log_info!(
        "FilterEntryCount: category {} reports {} entries",
        category,
        count
    );
    true
}

/// Return `category` to the stock count.
pub fn clear_override(category: u32) {
    if let Some(slot) = OVERRIDES.get(category as usize) {
        slot.store(NO_OVERRIDE, Ordering::Release);
    }
}

fn ensure_installed() -> bool {
    let target = TARGET.load(Ordering::Acquire);
    if target.is_null() {
        log_warn!(
            "FilterEntryCount: filter_entry_count_table unresolved — extra filter selections will not persist"
        );
        return false;
    }
    let _guard = match INSTALL.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    unsafe {
        if (*addr_of!(HOOK)).is_some() {
            return true;
        }
        let original: CountFn = std::mem::transmute(target);
        match crate::core::hooks::install_enabled(addr_of_mut!(HOOK), original, count_hook) {
            Ok(()) => {
                log_info!("FilterEntryCount: entry-count detour installed");
                true
            }
            Err(e) => {
                log_warn!(
                    "FilterEntryCount: entry-count detour install failed: {:?} — extra filter selections will not persist",
                    e
                );
                false
            }
        }
    }
}

/// Panic-free: no `unwrap`/indexing; a missing original degrades to 0 (the
/// stock function's own default-case return).
unsafe extern "C" fn count_hook(category: u32) -> u32 {
    if let Some(slot) = OVERRIDES.get(category as usize) {
        let count = slot.load(Ordering::Acquire);
        if count != NO_OVERRIDE {
            return count;
        }
    }
    match &*addr_of!(HOOK) {
        Some(hook) => hook.call(category),
        None => 0,
    }
}
