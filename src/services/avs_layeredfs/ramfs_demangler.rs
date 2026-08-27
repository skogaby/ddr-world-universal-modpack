//! RAM FS Demangler — tracks file→RAM→mount chain to map virtual paths back to real IFS paths.
//!
//! When the game loads an IFS into RAM and mounts it as a virtual filesystem,
//! the virtual path no longer resembles the original file path. This module
//! tracks the open→read→mount sequence to reconstruct the mapping, so mod
//! lookups work for RAM-loaded IFS files.

use once_cell::sync::Lazy;
use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use crate::{log_info, log_warn};

// ── Types ────────────────────────────────────────────────────────────

struct CleanupInfo {
    handle: i32,
    buffer: Option<usize>,
    ramfs_path: Option<String>,
    link_path: Option<String>,
    mounted_path: Option<String>,
}

struct DemanglerState {
    /// AVS file handle → original IFS path
    open_file_map: HashMap<i32, String>,
    /// Buffer address → original IFS path
    ram_load_map: HashMap<usize, String>,
    /// ramfs virtual path → original IFS path
    ramfs_map: BTreeMap<String, String>,
    /// imagefs mount point → original IFS path (used for demangling)
    mangling_map: BTreeMap<String, String>,
    /// Original path → cleanup info (for stale mapping removal on re-open)
    cleanup_map: BTreeMap<String, CleanupInfo>,
    /// Basename ("foo.ifs") → demangled inner path
    /// ("<arc norm with .arc→_arc>/.../foo.ifs"). Populated by the arc handler
    /// when it scans a mod's `*_arc/*_ifs/` subdirs. The game extracts an
    /// inner ifs into a ramfs buffer through a path that bypasses our hooks
    /// (we never see the open and so the buffer pointer isn't in ram_load_map),
    /// so we have to fall back to matching on the basename of the mountpoint.
    arc_inner_by_basename: HashMap<String, String>,
}

static STATE: Lazy<Mutex<DemanglerState>> = Lazy::new(|| {
    Mutex::new(DemanglerState {
        open_file_map: HashMap::new(),
        ram_load_map: HashMap::new(),
        ramfs_map: BTreeMap::new(),
        mangling_map: BTreeMap::new(),
        cleanup_map: BTreeMap::new(),
        arc_inner_by_basename: HashMap::new(),
    })
});

// ── Public API ───────────────────────────────────────────────────────

/// Track an avs_fs_open call for `.ifs` and `.arc` files. Cleans up stale
/// mappings on re-open. Tracking `.arc` (in addition to `.ifs`) lets the
/// downstream demangler resolve mounts that originate from arc-extracted
/// inner IFS files.
pub fn on_fs_open(path: &str, handle: i32) {
    let lower = path.to_lowercase();
    if handle < 0 || (!lower.ends_with(".ifs") && !lower.ends_with(".arc")) {
        return;
    }

    let mut state = STATE.lock().unwrap();
    let key = lower;

    // Clean up stale mappings if this IFS was previously opened
    if let Some(old) = state.cleanup_map.remove(&key) {
        state.open_file_map.remove(&old.handle);
        if let Some(buf) = old.buffer {
            state.ram_load_map.remove(&buf);
        }
        if let Some(ref rp) = old.ramfs_path {
            state.ramfs_map.remove(rp);
        }
        if let Some(ref lp) = old.link_path {
            state.ramfs_map.remove(lp);
        }
        if let Some(ref mp) = old.mounted_path {
            state.mangling_map.remove(mp);
        }
    }

    state.cleanup_map.insert(
        key,
        CleanupInfo {
            handle,
            buffer: None,
            ramfs_path: None,
            link_path: None,
            mounted_path: None,
        },
    );
    state.open_file_map.insert(handle, path.to_string());
}

/// Register an inner-ifs basename ("foo.ifs") → demangled path
/// ("<arc_norm with .arc→_arc>/.../foo.ifs"). When a later ramfs mount's
/// mountpoint or fsroot has that basename and we *don't* have a buffer
/// mapping (the game extracted the inner ifs without going through our
/// hooks), the mount is demangled to the stored path instead. Used for
/// arc-embedded ifs files.
pub fn register_arc_inner_ifs(basename: &str, demangled_path: &str) {
    let mut state = STATE.lock().unwrap();
    if let Some(existing) = state.arc_inner_by_basename.get(basename) {
        if existing != demangled_path {
            log_warn!(
                "LayeredFS: arc demangle: basename collision for '{}' ({} vs {}), later one wins",
                basename,
                existing,
                demangled_path
            );
        }
    }
    state
        .arc_inner_by_basename
        .insert(basename.to_string(), demangled_path.to_string());
    log_info!(
        "LayeredFS: arc inner basename '{}' -> {}",
        basename,
        demangled_path
    );
}

/// Track an avs_fs_read call — associates a buffer address with the source file.
pub fn on_fs_read(handle: i32, buffer_addr: usize) {
    let mut state = STATE.lock().unwrap();

    if let Some(path) = state.open_file_map.get(&handle).cloned() {
        state.ram_load_map.insert(buffer_addr, path.clone());

        let key = path.to_lowercase();
        if let Some(cleanup) = state.cleanup_map.get_mut(&key) {
            cleanup.buffer = Some(buffer_addr);
        }
    }
}

/// Track an avs_fs_mount call — handles ramfs, link, and imagefs mount types.
pub fn on_fs_mount(mountpoint: &str, fsroot: &str, fstype: &str, flags: &str) {
    let mut state = STATE.lock().unwrap();

    match fstype {
        "ramfs" => {
            // Extract base= pointer from flags
            let base_addr = match parse_base_pointer(flags) {
                Some(addr) => addr,
                None => return,
            };

            // strip trailing '/' on mountpoint — DDR uses "/dev/ram/foo.ifs/"
            let mount_path = format!("{}/{}", mountpoint.trim_end_matches('/'), fsroot);

            if let Some(orig_path) = state.ram_load_map.get(&base_addr).cloned() {
                log_info!("LayeredFS: ramfs mount mapped to {}", orig_path);

                let key = orig_path.to_lowercase();
                if let Some(cleanup) = state.cleanup_map.get_mut(&key) {
                    cleanup.ramfs_path = Some(mount_path.clone());
                }
                state.ramfs_map.insert(mount_path, orig_path);
            } else {
                // ifs-inside-arc: the game extracted an inner ifs into a ramfs
                // buffer through a path our hooks didn't see. Fall back to
                // matching on the basename of the mount.
                let bn = ifs_basename_of(mountpoint).or_else(|| ifs_basename_of(fsroot));
                if let Some(bn) = bn {
                    if let Some(orig_path) = state.arc_inner_by_basename.get(&bn).cloned() {
                        log_info!(
                            "LayeredFS: ramfs mount basename '{}' mapped to {}",
                            bn,
                            orig_path
                        );
                        // No cleanup_map entry: we never saw the open for this inner ifs.
                        state.ramfs_map.insert(mount_path, orig_path);
                    }
                }
            }
        }
        "link" => {
            if let Some(orig_path) = state.ramfs_map.get(fsroot).cloned() {
                log_info!("LayeredFS: link mount mapped to {}", orig_path);

                let key = orig_path.to_lowercase();
                if let Some(cleanup) = state.cleanup_map.get_mut(&key) {
                    cleanup.link_path = Some(mountpoint.to_string());
                }
                state.ramfs_map.insert(mountpoint.to_string(), orig_path);
            }
        }
        "imagefs" => {
            // Longest prefix match in ramfs_map
            if let Some(orig_path) = longest_prefix_match(&state.ramfs_map, fsroot) {
                log_info!("LayeredFS: imagefs mount mapped to {}", orig_path);

                let key = orig_path.to_lowercase();
                if let Some(cleanup) = state.cleanup_map.get_mut(&key) {
                    cleanup.mounted_path = Some(mountpoint.to_string());
                }
                state.mangling_map.insert(mountpoint.to_string(), orig_path);
            } else if fsroot.to_lowercase().ends_with(".ifs") {
                // Two scenarios reach here:
                //  (1) imagefs mounted directly from a real file path
                //      (eg ./data/foo.ifs). mountpoint -> fsroot is what
                //      makes paths under the mountpoint normalise back to
                //      data-relative.
                //  (2) ifs-inside-ifs where the inner ifs is opaque/virtual
                //      (no ramfs_map entry and not a real path) — registering
                //      would lie about a mapping we don't actually have.
                // Demangle first to handle ifs-inside-real-ifs, then check
                // that the result is something normalise_path can use.
                let mut root = fsroot.to_string();
                demangle_nolock(&state.mangling_map, &mut root);
                // We can't call mod_paths::normalise_path here — it would
                // re-acquire STATE and deadlock — but after demangle_nolock
                // any normalisable path either contains "data/" or has an
                // ".ifs/" marker.
                if normalise_path_recognizable(&root) {
                    log_info!("LayeredFS: imagefs mount mapped to {}", root);
                    state.mangling_map.insert(mountpoint.to_string(), root);
                }
            }
        }
        _ => {}
    }
}

/// Demangle a virtual path — replace the longest matching mount prefix with the original IFS path.
pub fn demangle(path: &mut String) {
    let state = STATE.lock().unwrap();
    demangle_nolock(&state.mangling_map, path);
}

// ── Internal helpers ─────────────────────────────────────────────────

fn demangle_nolock(mangling_map: &BTreeMap<String, String>, path: &mut String) {
    if let Some((prefix, replacement)) = longest_prefix_entry(mangling_map, path) {
        *path = format!("{}{}", replacement, &path[prefix.len()..]);
    }
}

/// Find the longest key in the map that is a prefix of `haystack`.
fn longest_prefix_match(map: &BTreeMap<String, String>, haystack: &str) -> Option<String> {
    longest_prefix_entry(map, haystack).map(|(_, v)| v)
}

fn longest_prefix_entry(
    map: &BTreeMap<String, String>,
    haystack: &str,
) -> Option<(String, String)> {
    let mut best: Option<(String, String)> = None;
    for (key, val) in map.iter() {
        if haystack.starts_with(key.as_str()) {
            match &best {
                Some((bk, _)) if bk.len() >= key.len() => {}
                _ => best = Some((key.clone(), val.clone())),
            }
        }
    }
    best
}

/// Mirrors the predicate `mod_paths::normalise_path(p).is_some()` without
/// touching STATE — usable from inside `on_fs_mount` after a `demangle_nolock`
/// pass (which is what `normalise_path` itself would have done first).
fn normalise_path_recognizable(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.contains("data/") || lower.contains(".ifs/")
}

/// Strip trailing slashes, take the last path component, return it only if it ends in ".ifs".
fn ifs_basename_of(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches('/');
    let bn = trimmed.rsplit('/').next().unwrap_or(trimmed);
    if bn.to_lowercase().ends_with(".ifs") {
        Some(bn.to_string())
    } else {
        None
    }
}

/// Parse "base=0xABCD1234" from AVS mount flags string.
fn parse_base_pointer(flags: &str) -> Option<usize> {
    let base_str = flags.find("base=").map(|i| &flags[i + 5..])?;
    // Take until next comma, space, or end
    let end = base_str.find([',', ' ']).unwrap_or(base_str.len());
    let num_str = &base_str[..end];
    // Parse as hex (0x prefix) or decimal
    if let Some(hex) = num_str
        .strip_prefix("0x")
        .or_else(|| num_str.strip_prefix("0X"))
    {
        usize::from_str_radix(hex, 16).ok()
    } else {
        num_str.parse::<usize>().ok()
    }
}
