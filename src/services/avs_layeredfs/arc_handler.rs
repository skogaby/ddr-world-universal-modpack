//! ARC mod handler — overlays mod files onto game `.arc` archives.
//!
//! Mirrors upstream ifs_layeredfs's `handle_arc`: when an `.arc` is opened,
//! look for a sibling `<arc with .arc → _arc>` mod folder. Walk it for:
//!   1. Plain files (overlays) — repack into a cached copy of the arc.
//!   2. `*_ifs` subdirectories (inner-IFS mods) — register the basename with
//!      the demangler so the game's later ramfs mount of the extracted inner
//!      ifs maps back to the arc-qualified mod path our LayeredFS lookups use.
//!
//! The cached arc is written uncompressed (compression=0, 64-byte aligned).
//! Cache invalidation uses the shared `CacheHasher` against (original arc
//! path, all overlay file paths).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::core::arc::ArcArchive;
use crate::{log_info, log_warn};

use super::cache_hasher::{CacheHasher, CACHE_FOLDER};
use super::mod_paths;
use super::ramfs_demangler;
use super::shader_synthesis;
use super::xml_merger;

/// Result of scanning a mod's `*_arc` folder.
struct ArcModScan {
    /// Per-entry overrides. Keyed by relative path within the arc (e.g.
    /// "shader/foo.bin"); value is the absolute filesystem path of the mod
    /// file to read at pack time.
    files: BTreeMap<String, String>,
    /// Inner-ifs paths inside the arc, derived from `*_ifs` mod subdirs.
    /// Paths are arc-relative with the dot restored: a mod dir
    /// `sub/inner_ifs/` yields `sub/inner.ifs`. Stored in a set so multiple
    /// mods declaring the same inner ifs dedupe.
    inner_ifs_paths: BTreeSet<String>,
}

impl ArcModScan {
    fn is_empty(&self) -> bool {
        self.files.is_empty() && self.inner_ifs_paths.is_empty()
    }
}

/// Try to apply ARC mods for `norm_path` (a normalized arc path, e.g.
/// "arc/bm2d/foo.arc"). Returns the cache path to redirect the open to,
/// or None if there are no mods touching this arc.
///
/// `original_path` is the path the caller would have opened (post-mangling)
/// — used to load the original arc bytes through AVS when overlays exist.
pub fn handle_arc(norm_path: &str, original_path: &str) -> Option<String> {
    let arc_mod_path = norm_path.replace(".arc", "_arc");

    // Runtime-synthesized shader containers (extended gs_screencommand_*
    // GSPW files built from the stock arc + committed mod blobs) ride the
    // same repack path as regular overlay files.
    let synth_entries = if norm_path == "arc/shader.arc" {
        shader_synthesis::synthesize(original_path)
    } else {
        Vec::new()
    };

    // No mod folder for this arc and nothing synthesized → nothing to do.
    let has_mod_folder = mod_paths::find_first_modfolder(&arc_mod_path).is_some();
    if !has_mod_folder && synth_entries.is_empty() {
        return None;
    }

    let mut scan = if has_mod_folder {
        scan_arc_mod_folder(&arc_mod_path)
    } else {
        ArcModScan {
            files: BTreeMap::new(),
            inner_ifs_paths: BTreeSet::new(),
        }
    };
    // Synthesized entries join the overlay set; an operator's explicit
    // overlay of the same entry name wins over synthesis.
    for e in synth_entries {
        scan.files.entry(e.entry_name).or_insert(e.file_path);
    }
    if scan.is_empty() {
        log_info!(
            "LayeredFS: arc: mod folder for '{}' has no files, skipping",
            arc_mod_path
        );
        return None;
    }

    // Register inner-IFS basenames so the demangler can resolve mounts that
    // happen after the game extracts an inner ifs from the arc. Do this even
    // if there are no top-level overlay files — the game still needs to see
    // the inner-IFS modifications.
    for inner_rel in &scan.inner_ifs_paths {
        let basename = inner_rel
            .rfind('/')
            .map(|p| &inner_rel[p + 1..])
            .unwrap_or(inner_rel.as_str());
        let demangled = format!("data/{}/{}", arc_mod_path, inner_rel);
        ramfs_demangler::register_arc_inner_ifs(basename, &demangled);
    }

    // Inner-IFS-only case: nothing to repack, just let the game open the
    // original arc and let the demangler do its thing on the inner mount.
    if scan.files.is_empty() {
        return None;
    }

    let out = format!("{}/{}", CACHE_FOLDER, norm_path);
    let out_hashed = format!("{}.hashed", out);

    // Cache-hash inputs: original arc + every overlay file.
    let mut hasher = CacheHasher::new(&out_hashed);
    hasher.add(original_path);
    for path in scan.files.values() {
        hasher.add(path);
    }
    hasher.finish();

    if hasher.matches() && Path::new(&out).is_file() {
        log_info!("LayeredFS: arc cache up to date for {}", norm_path);
        return Some(out);
    }
    log_info!("LayeredFS: arc: regenerating cache for {}", norm_path);

    // Load the original arc through AVS. If it doesn't exist, build from
    // scratch — matches upstream's behavior for arc files that aren't in
    // the base game.
    let mut arc = match xml_merger::load_bytes_from_avs_path(original_path) {
        Some(bytes) => match ArcArchive::from_bytes(&bytes) {
            Some(a) => a,
            None => {
                log_warn!(
                    "LayeredFS: arc: load failed for {}, aborting modding",
                    original_path
                );
                return None;
            }
        },
        None => {
            log_info!(
                "LayeredFS: arc: no original file, creating from scratch: \"{}\"",
                norm_path
            );
            ArcArchive::empty()
        }
    };

    if let Some(parent) = out.rsplit_once('/').map(|(p, _)| p) {
        if !mod_paths::mkdir_p(parent) {
            log_warn!(
                "LayeredFS: arc: couldn't create output cache folder '{}'",
                parent
            );
            return None;
        }
    }

    for (name, path) in &scan.files {
        match std::fs::read(path) {
            Ok(data) => arc.add_or_replace(name.clone(), data),
            Err(e) => {
                log_warn!("LayeredFS: arc: couldn't read mod file '{}': {}", path, e);
            }
        }
    }

    if !arc.save(&out) {
        log_warn!("LayeredFS: arc: couldn't write output '{}'", out);
        return None;
    }

    hasher.commit();
    Some(out)
}

// ── Scanning ─────────────────────────────────────────────────────────

fn scan_arc_mod_folder(folder: &str) -> ArcModScan {
    let mut out = ArcModScan {
        files: BTreeMap::new(),
        inner_ifs_paths: BTreeSet::new(),
    };
    for mod_dir in mod_paths::available_mods() {
        let root = format!("{}/{}", mod_dir, folder);
        scan_arc_mod_onefolder(&mut out, &root, "");
    }
    out
}

fn scan_arc_mod_onefolder(out: &mut ArcModScan, folder: &str, rel_prefix: &str) {
    let entries = match std::fs::read_dir(folder) {
        Ok(e) => e,
        Err(_) => return,
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

        if meta.is_dir() {
            if name.to_lowercase().ends_with("_ifs") {
                // Convert inner_ifs → inner.ifs, prepend any prefix.
                let stem = &name[..name.len() - 4];
                let inner_rel = format!("{}{}.ifs", rel_prefix, stem);
                out.inner_ifs_paths.insert(inner_rel);
                continue;
            }
            let next_folder = format!("{}/{}", folder, name);
            let next_prefix = format!("{}{}/", rel_prefix, name);
            scan_arc_mod_onefolder(out, &next_folder, &next_prefix);
        } else {
            let rel = format!("{}{}", rel_prefix, name);
            // First mod wins (matches upstream's iteration over available_mods()).
            out.files
                .entry(rel)
                .or_insert_with(|| format!("{}/{}", folder, name));
        }
    }
}
