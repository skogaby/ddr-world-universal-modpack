//! Impure discovery layer for Split SSQ Auto-Discovery: enumerate split SSQ
//! files on disk and read the level set each one carries.
//!
//! Sources (design R3): the stock `data/mdb_apx/ssq/` directory plus every
//! LayeredFS mod folder's `mdb_apx/ssq/`. For the content check each distinct
//! `(basename, n)` is read from the file LayeredFS would actually serve
//! (`mod_paths::find_first_modfile`, else the stock path), so the index agrees
//! with what the game loads.
//!
//! Host `std::fs` on repo-relative paths (the process CWD is the game's
//! `contents/` directory) — the same pattern as `fast_bootup::identity`. Never
//! touches AVS, so it is safe to run from the enabling thread.

use std::fs;

use super::resolver::{collect_split_candidates, levels_in_blob, SplitFile};
use crate::log_warn;
use crate::services::avs_layeredfs::mod_paths;

const STOCK_DIR: &str = "data/mdb_apx/ssq";
const REL_DIR: &str = "mdb_apx/ssq";

/// Enumerate every `<basename>_<n>.ssq` across the stock directory and all
/// LayeredFS mod folders, read each one's type-3 level set, and return the
/// listing the resolver builds its index from.
///
/// `Err` only when the STOCK directory cannot be listed — a mod folder that
/// lacks `mdb_apx/ssq/` is normal and silently skipped. A candidate whose
/// backing file cannot be read is dropped with one WARN (the game will fail
/// to open it just the same; leaving it out yields the unsplit path).
pub fn scan() -> Result<Vec<SplitFile>, String> {
    let mut names: Vec<Vec<u8>> =
        list_dir(STOCK_DIR).map_err(|e| format!("cannot list {}: {}", STOCK_DIR, e))?;

    for mod_dir in mod_paths::available_mods() {
        if let Ok(more) = list_dir(&format!("{}/{}", mod_dir, REL_DIR)) {
            names.extend(more);
        }
    }

    let candidates = collect_split_candidates(names.iter().map(Vec::as_slice));
    let mut files = Vec::with_capacity(candidates.len());
    for (basename, n) in candidates {
        let file_name = format!("{}_{}.ssq", String::from_utf8_lossy(&basename), n);
        let rel = format!("{}/{}", REL_DIR, file_name);
        let path = mod_paths::find_first_modfile(&rel).unwrap_or_else(|| format!("data/{}", rel));
        match fs::read(&path) {
            Ok(blob) => files.push(SplitFile {
                basename,
                n,
                levels: levels_in_blob(&blob),
            }),
            Err(e) => log_warn!(
                "SplitSsqAutoDiscovery: cannot read {} ({}) -- not indexed",
                path,
                e
            ),
        }
    }
    Ok(files)
}

/// Bare filenames (regular files only) in `dir`.
fn list_dir(dir: &str) -> std::io::Result<Vec<Vec<u8>>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            out.push(
                entry
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
                    .into_bytes(),
            );
        }
    }
    Ok(out)
}
