//! Mod path system — folder scanning, path normalization, and file lookup.
//!
//! Discovers mods in `data_mods/`, caches their contents, and resolves
//! game paths to mod file paths. Case-insensitive matching throughout.

use once_cell::sync::Lazy;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Mutex;

use crate::log_info;
use crate::log_warn;

use super::config;

// ── Types ────────────────────────────────────────────────────────────

struct ModContents {
    /// Full path to the mod folder (e.g., "./data_mods/my_mod")
    name: String,
    /// Relative paths within the mod, stored lowercase for case-insensitive lookup.
    /// Directories end with '/'.
    files: BTreeSet<String>,
}

struct ModPathState {
    mods: Vec<ModContents>,
    initialized: bool,
}

static STATE: Lazy<Mutex<ModPathState>> = Lazy::new(|| {
    Mutex::new(ModPathState {
        mods: Vec::new(),
        initialized: false,
    })
});

// ── Public API ───────────────────────────────────────────────────────

/// Scan mod folders and cache contents. Call once during init.
pub fn init_mod_paths() {
    let cfg = config();
    let mod_dirs = scan_mod_folders(&cfg.mod_folder, &cfg.allowlist, &cfg.blocklist);

    log_info!(
        "LayeredFS: found {} mod folder(s) in {}",
        mod_dirs.len(),
        cfg.mod_folder
    );

    let mut cached: Vec<ModContents> = Vec::new();
    for dir in &mod_dirs {
        let files = walk_dir(dir, "");
        if cfg.verbose {
            log_info!("LayeredFS:   {} ({} files)", dir, files.len());
        }
        if !cfg.developer_mode {
            cached.push(ModContents {
                name: dir.clone(),
                files,
            });
        }
    }

    let mut state = STATE.lock().unwrap();
    state.mods = cached;
    state.initialized = true;
}

/// Normalize a game path: strip `/data/` prefix, normalize slashes, collapse doubles.
/// Returns None if the path doesn't contain `data/`.
pub fn normalise_path(path: &str) -> Option<String> {
    let mut s = path.to_string();

    // Demangle ramfs virtual paths back to real IFS paths
    super::ramfs_demangler::demangle(&mut s);

    // Find "data/" (case-insensitive)
    let lower = s.to_lowercase();
    let data_pos = lower.find("data/");

    let mut result = if let Some(pos) = data_pos {
        // Strip everything up to and including "data/"
        s[pos + 5..].to_string()
    } else {
        // Fallback for .arc-contained IFS: extract IFS-relative path.
        // e.g. "/dev/ram/link/select_music_folder_v3.ifs/tex/texturelist.xml"
        //   → "select_music_folder_v3.ifs/tex/texturelist.xml"
        let ifs_marker = lower.find(".ifs/");
        if let Some(ifs_pos) = ifs_marker {
            // Walk backwards to find the start of the IFS filename component
            let before = &s[..ifs_pos];
            let component_start = before.rfind('/').map(|p| p + 1).unwrap_or(0);
            s[component_start..].to_string()
        } else {
            return None;
        }
    };

    // Normalize backslashes
    result = result.replace('\\', "/");
    // Collapse double slashes
    while result.contains("//") {
        result = result.replace("//", "/");
    }

    Some(result)
}

/// Find the first mod file matching the normalized path. Returns the full filesystem path.
pub fn find_first_modfile(norm_path: &str) -> Option<String> {
    let cfg = config();
    let lower = norm_path.to_lowercase();

    if cfg.developer_mode {
        for dir in &get_available_mod_dirs() {
            let mod_path = format!("{}/{}", dir, norm_path);
            if file_exists(&mod_path) {
                return Some(mod_path);
            }
        }
    } else {
        let state = STATE.lock().unwrap();
        for m in &state.mods {
            if m.files.contains(&lower) {
                return Some(format!("{}/{}", m.name, norm_path));
            }
        }
    }
    None
}

/// Find the first mod folder matching the normalized path. Returns the full filesystem path.
pub fn find_first_modfolder(norm_path: &str) -> Option<String> {
    let cfg = config();
    let lower = format!("{}/", norm_path.to_lowercase().trim_end_matches('/'));

    if cfg.developer_mode {
        for dir in &get_available_mod_dirs() {
            let mod_path = format!("{}/{}", dir, norm_path);
            if folder_exists(&mod_path) {
                return Some(format!("{}/", mod_path));
            }
        }
    } else {
        let state = STATE.lock().unwrap();
        for m in &state.mods {
            if m.files.contains(&lower) {
                return Some(format!("{}/{}/", m.name, norm_path));
            }
        }
    }
    None
}

/// Find all mod files matching the normalized path across all mods. Sorted for deterministic hashing.
pub fn find_all_modfile(norm_path: &str) -> Vec<String> {
    let cfg = config();
    let lower = norm_path.to_lowercase();
    let mut results = Vec::new();

    if cfg.developer_mode {
        for dir in &get_available_mod_dirs() {
            let mod_path = format!("{}/{}", dir, norm_path);
            if file_exists(&mod_path) {
                results.push(mod_path);
            }
        }
    } else {
        let state = STATE.lock().unwrap();
        for m in &state.mods {
            if m.files.contains(&lower) {
                results.push(format!("{}/{}", m.name, norm_path));
            }
        }
    }

    results.sort();
    results
}

/// Return list of active mod folder paths.
pub fn available_mods() -> Vec<String> {
    let cfg = config();
    if cfg.developer_mode {
        get_available_mod_dirs()
    } else {
        let state = STATE.lock().unwrap();
        state.mods.iter().map(|m| m.name.clone()).collect()
    }
}

// ── Internal helpers ─────────────────────────────────────────────────

/// Scan the mod root for subfolders, applying allowlist/blocklist filters.
fn scan_mod_folders(mod_folder: &str, allowlist: &[String], blocklist: &[String]) -> Vec<String> {
    let mut dirs = Vec::new();

    let entries = match std::fs::read_dir(mod_folder) {
        Ok(e) => e,
        Err(_) => {
            log_warn!(
                "LayeredFS: mod folder '{}' not found or not readable",
                mod_folder
            );
            return dirs;
        }
    };

    for entry in entries.flatten() {
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !meta.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();

        // Skip cache directory
        if name.eq_ignore_ascii_case("_cache") {
            continue;
        }

        // Allowlist check
        if !allowlist.is_empty() && !allowlist.iter().any(|a| a.eq_ignore_ascii_case(&name)) {
            log_info!("LayeredFS: ignoring non-allowlisted mod '{}'", name);
            continue;
        }

        // Blocklist check
        if blocklist.iter().any(|b| b.eq_ignore_ascii_case(&name)) {
            log_info!("LayeredFS: ignoring blocklisted mod '{}'", name);
            continue;
        }

        dirs.push(format!("{}/{}", mod_folder, name));
    }

    // Sort case-insensitively for deterministic priority order
    dirs.sort_by_key(|a| a.to_lowercase());
    dirs
}

/// Recursively walk a directory, collecting relative paths (lowercase) into a BTreeSet.
fn walk_dir(base: &str, prefix: &str) -> BTreeSet<String> {
    let mut result = BTreeSet::new();
    let full_path = if prefix.is_empty() {
        base.to_string()
    } else {
        format!("{}/{}", base, prefix)
    };

    let entries = match std::fs::read_dir(&full_path) {
        Ok(e) => e,
        Err(_) => return result,
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "." || name == ".." {
            continue;
        }

        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        let rel_path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}{}", prefix, name)
        };

        if meta.is_dir() {
            // Warn about common mistake
            if prefix.is_empty() && name.eq_ignore_ascii_case("data") {
                log_warn!(
                    "LayeredFS: 'data' folder detected in mod root '{}'. Move files to mod root.",
                    base
                );
            }

            let dir_entry = format!("{}/", rel_path);
            result.insert(dir_entry.to_lowercase());

            let sub = walk_dir(base, &format!("{}/", rel_path));
            result.extend(sub);
        } else {
            result.insert(rel_path.to_lowercase());
        }
    }

    result
}

/// Get available mod directories (re-scans in dev mode).
fn get_available_mod_dirs() -> Vec<String> {
    let cfg = config();
    let mut dirs = scan_mod_folders(&cfg.mod_folder, &cfg.allowlist, &cfg.blocklist);
    dirs.sort_by_key(|a| a.to_lowercase());
    dirs
}

fn file_exists(path: &str) -> bool {
    Path::new(path).is_file()
}

fn folder_exists(path: &str) -> bool {
    Path::new(path).is_dir()
}

/// Create directory and all parents (like mkdir -p).
pub fn mkdir_p(path: &str) -> bool {
    std::fs::create_dir_all(path).is_ok()
}
