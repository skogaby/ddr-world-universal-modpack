//! Game-folder resolution and the safety gate.
//!
//! The tool lives next to `spice64.exe` and is normally invoked from
//! `gamestart.bat`, but it must not trust the working directory: a
//! double-clicked exe or a mis-written bat still has to target the right
//! folder, and an exe copied into `Downloads` must never create a
//! `data_mods/_cache` there. So the game folder is the directory containing
//! the executable (or the `--game-dir` override), accepted only if it
//! recognisably holds the game (`spice64.exe`) or the hook
//! (`ddr_world_hook.dll`). Same rule as the updater.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// File names whose presence marks a folder as the game folder.
pub const ANCHORS: [&str; 2] = ["spice64.exe", "ddr_world_hook.dll"];
/// Per-run log, overwritten each start.
pub const LOG_NAME: &str = "ddr_world_hook_prep.log";
/// The DLL's config file (two keys are read).
pub const CONFIG_NAME: &str = "mod-config.json";

/// The resolved game folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameDir {
    /// Canonical path of the folder holding `spice64.exe`.
    pub root: PathBuf,
}

impl GameDir {
    pub fn log_path(&self) -> PathBuf {
        self.root.join(LOG_NAME)
    }

    pub fn config_path(&self) -> PathBuf {
        self.root.join(CONFIG_NAME)
    }
}

/// Why a folder was not accepted. Renders as one human-readable line.
#[derive(Debug)]
pub enum Refusal {
    NotAGameFolder(PathBuf),
    UnknownExeLocation,
    Unreadable(PathBuf, io::Error),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::NotAGameFolder(path) => write!(
                f,
                "This folder does not look like a DDR World game folder (no {} or {} in {}); nothing done.",
                ANCHORS[0],
                ANCHORS[1],
                path.display()
            ),
            Refusal::UnknownExeLocation => write!(
                f,
                "Could not determine where this tool is running from; pass --game-dir <DIR>. Nothing done."
            ),
            Refusal::Unreadable(path, err) => write!(
                f,
                "Cannot read the game folder {} ({err}); nothing done.",
                path.display()
            ),
        }
    }
}

/// Resolve the game folder from the `--game-dir` override or the executable's
/// own directory, and apply the safety gate.
pub fn resolve(cli_override: Option<&Path>) -> Result<GameDir, Refusal> {
    resolve_with(cli_override, current_exe_dir(), &|p| p.is_file())
}

/// [`resolve`] with its environment injected (tests).
pub fn resolve_with(
    cli_override: Option<&Path>,
    exe_dir: Option<PathBuf>,
    exists: &dyn Fn(&Path) -> bool,
) -> Result<GameDir, Refusal> {
    let candidate = match cli_override {
        Some(dir) => dir.to_path_buf(),
        None => exe_dir.ok_or(Refusal::UnknownExeLocation)?,
    };
    let root =
        dunce::canonicalize(&candidate).map_err(|e| Refusal::Unreadable(candidate.clone(), e))?;
    if ANCHORS.iter().any(|name| exists(&root.join(name))) {
        Ok(GameDir { root })
    } else {
        Err(Refusal::NotAGameFolder(root))
    }
}

fn current_exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ddr_prep_gamedir_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn accepts_a_folder_with_either_anchor() {
        let dir = temp();
        let r = resolve_with(None, Some(dir.clone()), &|p| {
            p.file_name().is_some_and(|n| n == "ddr_world_hook.dll")
        })
        .unwrap();
        assert_eq!(r.root, dunce::canonicalize(&dir).unwrap());
        assert!(r.log_path().ends_with(LOG_NAME));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn override_wins_and_unanchored_is_refused() {
        let dir = temp();
        let other = temp();
        let r = resolve_with(Some(&other), Some(dir.clone()), &|p| {
            p.file_name().is_some_and(|n| n == "spice64.exe")
        })
        .unwrap();
        assert_eq!(r.root, dunce::canonicalize(&other).unwrap());
        let e = resolve_with(None, Some(dir.clone()), &|_| false).unwrap_err();
        assert!(matches!(e, Refusal::NotAGameFolder(_)));
        assert!(e.to_string().contains("nothing done"));
        let e = resolve_with(None, None, &|_| true).unwrap_err();
        assert!(matches!(e, Refusal::UnknownExeLocation));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&other);
    }
}
