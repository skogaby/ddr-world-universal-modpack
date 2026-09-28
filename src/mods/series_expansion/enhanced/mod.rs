//! Enhanced (config-defined) VERSION layout — see the parent module's
//! "Enhanced mode" section. `model` validates the config; this module owns
//! the game-memory table; `hooks` owns the builder and group-press detours.

pub mod hooks;
pub mod labels;
pub mod model;

use crate::core::memory;
use crate::services::avs_layeredfs::mod_paths;
use model::{encode_row, EnhancedPlan, ENTRY_STRIDE};

/// Allocate (near the module, never freed) and fill the enhanced filter
/// table: one row per cell, a sentinel and inert padding (`table_rows`).
/// Returns the table and its row count, or `None` if allocation failed.
///
/// # Safety
/// `module_base` must be the gamemdx base (the table is reached through
/// RIP-relative disp32 patches).
pub unsafe fn build_table(module_base: *const u8, plan: &EnhancedPlan) -> Option<(*mut u8, usize)> {
    let rows = plan.table_rows();
    let table = memory::alloc_near(module_base, rows.len() * ENTRY_STRIDE);
    if table.is_null() {
        return None;
    }
    for (i, row) in rows.iter().enumerate() {
        // Long labels: a leaked NUL-terminated copy (the game's std::function
        // captures keep the table pointer for the process lifetime).
        let bytes = encode_row(row, |text| {
            let mut owned = text.as_bytes().to_vec();
            owned.push(0);
            Box::leak(owned.into_boxed_slice()).as_ptr() as u64
        });
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), table.add(i * ENTRY_STRIDE), ENTRY_STRIDE);
    }
    Some((table, rows.len()))
}

/// Whether `jacket_thumbnails_<ja|ua>_<n>.arc` exists in the install's
/// `data/arc/thumbnail/` or in a LayeredFS mod folder.
pub fn thumbnail_arc_exists(n: u8) -> bool {
    ["ja", "ua"].iter().any(|region| {
        let rel = format!("arc/thumbnail/jacket_thumbnails_{}_{}.arc", region, n);
        std::path::Path::new(&format!("data/{}", rel)).is_file()
            || mod_paths::find_first_modfile(&rel).is_some()
    })
}
