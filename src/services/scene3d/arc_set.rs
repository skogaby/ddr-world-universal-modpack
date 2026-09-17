//! Arc registration through the game's `FileManager` (design §4.2.1).
//!
//! `FileManager::Load(path)` takes ONLY a path — the engine dispatches each
//! arc member to the registered file callback by extension (`.model` →
//! `ModelFileCallback`, `.dds` → `DdsFileCallback`, …), asynchronously on a
//! worker thread. Loading is refcounted by the engine, so every successful
//! load must be paired with exactly one free — [`ArcSet`] is not `Clone` and
//! [`free`] consumes it.
//!
//! LayeredFS mod-folder overrides are honoured: a `data_mods/<mod>/arc/<file>`
//! wins over the stock `data/arc/<file>`; the FileManager is then handed the
//! mod file's filesystem path (the AVS layer is not involved — the FileManager
//! opens through the CRT).
//!
//! Engine calls ([`load`], [`free`]) are GAME-THREAD ONLY. [`resolve_path`]
//! and [`read_bytes`] are thread-agnostic.

use std::ffi::CString;
use std::path::Path;

use crate::log_warn;
use crate::services::avs_layeredfs::mod_paths;

pub use super::pure::{data_relative, resolve_with, Resolved};

/// A set of loaded arcs: `(logical game path, FileManager handle)` per
/// successfully loaded file. Not `Clone`: exactly-once release by type.
#[derive(Debug, Default)]
pub struct ArcSet {
    handles: Vec<(String, i32)>,
}

impl ArcSet {
    pub fn len(&self) -> usize {
        self.handles.len()
    }
    pub fn is_empty(&self) -> bool {
        self.handles.is_empty()
    }
    /// The logical game paths that loaded.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.handles.iter().map(|(p, _)| p.as_str())
    }
}

/// Resolve a game-relative arc path (`data/arc/pl_emi00.arc`) to the file
/// that will actually be read: a LayeredFS mod-folder override if one exists,
/// else the stock file under the game folder. `None` when neither exists.
pub fn resolve_path(game_rel: &str) -> Option<String> {
    resolve(game_rel).map(|r| r.path().to_string())
}

/// [`resolve_path`] keeping the override/stock distinction.
pub fn resolve(game_rel: &str) -> Option<Resolved> {
    let mod_override = mod_paths::find_first_modfile(data_relative(game_rel));
    resolve_with(game_rel, mod_override.as_deref(), Path::new("."), |p| {
        p.is_file()
    })
}

/// Read the resolved file's bytes for the DLL's own parsing (any thread).
pub fn read_bytes(game_rel: &str) -> Option<Vec<u8>> {
    let path = resolve_path(game_rel)?;
    match std::fs::read(&path) {
        Ok(b) => Some(b),
        Err(e) => {
            log_warn!("scene3d: read_bytes({}) failed: {}", path, e);
            None
        }
    }
}

/// Register every path with the FileManager. Paths that do not resolve or
/// whose `Load` fails are skipped with one WARN each; the returned set holds
/// only the successes. GAME THREAD ONLY.
///
/// The path handed to the engine is the game-relative one for stock files
/// (what the game itself passes) and the resolved filesystem path for mod
/// overrides.
pub fn load(paths: &[&str]) -> ArcSet {
    let mut set = ArcSet::default();
    let Some(inner) = super::inner() else {
        log_warn!("scene3d: load() with the service unavailable");
        return set;
    };
    let Some(manager) = super::file_manager() else {
        log_warn!("scene3d: FileManager singleton is null -- nothing loaded");
        return set;
    };
    for &game_rel in paths {
        // Stock files go to the engine by their game-relative path (what the
        // game itself passes); mod overrides by their filesystem path.
        let engine_path = match resolve(game_rel) {
            Some(Resolved::ModOverride(p)) => p,
            Some(Resolved::Stock(_)) => game_rel.to_string(),
            None => {
                log_warn!(
                    "scene3d: {} not found (mod folders or stock) -- skipped",
                    game_rel
                );
                continue;
            }
        };
        let Ok(c_path) = CString::new(engine_path.as_str()) else {
            log_warn!(
                "scene3d: {} is not a valid C string -- skipped",
                engine_path
            );
            continue;
        };
        // SAFETY: `manager` is the live FileManager (non-null, just read);
        // `file_load` is the AOB-resolved member with this exact prototype.
        let handle = unsafe { (inner.file_load)(manager, c_path.as_ptr()) };
        if handle < 0 {
            log_warn!(
                "scene3d: FileManager::Load(\"{}\") returned {} -- skipped",
                engine_path,
                handle
            );
            continue;
        }
        set.handles.push((game_rel.to_string(), handle));
    }
    set
}

/// Release every handle (`FileManager::Free`, drained asynchronously by the
/// engine). Consumes the set. GAME THREAD ONLY.
pub fn free(set: ArcSet) {
    if set.handles.is_empty() {
        return;
    }
    let Some(inner) = super::inner() else {
        return;
    };
    let Some(manager) = super::file_manager() else {
        return;
    };
    for (_, handle) in set.handles {
        if handle >= 0 {
            // SAFETY: as in `load`; the handle came from this manager's Load.
            unsafe { (inner.file_free)(manager, handle) };
        }
    }
}
