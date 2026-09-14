//! Game-folder resolution and the safety gate (design §4.2).
//!
//! The updater lives next to `spice64.exe` and is normally invoked from
//! `gamestart.bat`, but it must not trust the working directory: a
//! double-clicked exe or a mis-written bat still has to target the right
//! folder, and an exe copied into `Downloads` must never scatter files there.
//! So the game folder is the directory containing the executable (or the
//! `--game-dir` override), and it is accepted only if it recognisably holds the
//! game (`spice64.exe`) or the hook (`ddr_world_hook.dll`).

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// File names whose presence marks a folder as the game folder.
pub const ANCHORS: [&str; 2] = ["spice64.exe", "ddr_world_hook.dll"];

/// Name of the updater's private work directory inside the game folder.
pub const WORK_DIR_NAME: &str = ".ddr_world_hook_updater";
/// Install manifest written after every successful update.
pub const MANIFEST_NAME: &str = "ddr_world_hook_updater.manifest.json";
/// Per-run log, overwritten each start.
pub const LOG_NAME: &str = "ddr_world_hook_updater.log";

/// The resolved game folder and the updater's work directory inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameDir {
    /// Canonical path of the folder holding `spice64.exe`.
    pub root: PathBuf,
    /// `root/.ddr_world_hook_updater`.
    pub work: PathBuf,
}

impl GameDir {
    /// A `GameDir` rooted at `root` without the safety gate (unit tests).
    #[cfg(test)]
    pub fn for_test(root: &Path) -> Self {
        Self::new(root.to_path_buf())
    }

    fn new(root: PathBuf) -> Self {
        let work = root.join(WORK_DIR_NAME);
        Self { root, work }
    }

    /// Where the release zip is downloaded to.
    pub fn download_dir(&self) -> PathBuf {
        self.work.join("download")
    }

    /// Where the release zip is extracted before being applied.
    pub fn stage_dir(&self) -> PathBuf {
        self.work.join("stage")
    }

    /// Pre-run copies of every file an apply replaces or removes.
    pub fn backup_dir(&self) -> PathBuf {
        self.work.join("backup")
    }

    /// Present only while an apply is in flight (crash recovery).
    pub fn journal_path(&self) -> PathBuf {
        self.work.join("journal.json")
    }

    /// The install manifest.
    pub fn manifest_path(&self) -> PathBuf {
        self.root.join(MANIFEST_NAME)
    }

    /// The per-run log.
    pub fn log_path(&self) -> PathBuf {
        self.root.join(LOG_NAME)
    }
}

/// Why a folder was not accepted. Every variant renders as one human-readable
/// line; the caller prints it and exits 0 (nothing was done).
#[derive(Debug)]
pub enum Refusal {
    /// Neither anchor file exists in the examined folder.
    NotAGameFolder(PathBuf),
    /// The executable's own location could not be determined and no override
    /// was given.
    UnknownExeLocation,
    /// The candidate folder could not be canonicalised (does not exist, or is
    /// unreadable).
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
                "Could not determine where this updater is running from; pass --game-dir <DIR>. Nothing done."
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

/// [`resolve`] with its environment injected: `exe_dir` stands in for the
/// executable's directory (None = unknown) and `exists` probes for the anchor
/// files. Tests use this to avoid depending on where the test binary lives.
pub fn resolve_with(
    cli_override: Option<&Path>,
    exe_dir: Option<PathBuf>,
    exists: &dyn Fn(&Path) -> bool,
) -> Result<GameDir, Refusal> {
    let candidate = match cli_override {
        Some(dir) => dir.to_path_buf(),
        None => exe_dir.ok_or(Refusal::UnknownExeLocation)?,
    };
    // dunce: like std's canonicalize but without the `\\?\` verbatim prefix on
    // Windows whenever the plain form is equivalent (readable logs, safe joins).
    let root =
        dunce::canonicalize(&candidate).map_err(|e| Refusal::Unreadable(candidate.clone(), e))?;
    if ANCHORS.iter().any(|name| exists(&root.join(name))) {
        Ok(GameDir::new(root))
    } else {
        Err(Refusal::NotAGameFolder(root))
    }
}

fn current_exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// Fresh, unique, empty temp directory (removed by the caller via `Temp`).
    struct Temp(PathBuf);

    impl Temp {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "ddr_updater_gamedir_test_{}_{n}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Temp(dir)
        }

        fn touch(&self, name: &str) {
            fs::write(self.0.join(name), b"").unwrap();
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn fs_probe(p: &Path) -> bool {
        p.is_file()
    }

    #[test]
    fn g1_spice_exe_accepted_with_canonical_root_and_work() {
        let t = Temp::new();
        t.touch("spice64.exe");
        let g = resolve_with(Some(&t.0), None, &fs_probe).unwrap();
        assert_eq!(g.root, dunce::canonicalize(&t.0).unwrap());
        assert_eq!(g.work, g.root.join(".ddr_world_hook_updater"));
    }

    #[test]
    fn g2_hook_dll_alone_accepted() {
        let t = Temp::new();
        t.touch("ddr_world_hook.dll");
        assert!(resolve_with(Some(&t.0), None, &fs_probe).is_ok());
    }

    #[test]
    fn g3_empty_folder_refused_with_explanatory_message() {
        let t = Temp::new();
        let err = resolve_with(Some(&t.0), None, &fs_probe).unwrap_err();
        assert!(matches!(err, Refusal::NotAGameFolder(_)));
        let msg = err.to_string();
        let canonical = dunce::canonicalize(&t.0).unwrap();
        assert!(msg.contains(&canonical.display().to_string()), "{msg}");
        assert!(
            msg.contains("spice64.exe") && msg.contains("ddr_world_hook.dll"),
            "{msg}"
        );
    }

    #[test]
    fn g4_override_wins_over_exe_location() {
        let over = Temp::new();
        over.touch("spice64.exe");
        let exe = Temp::new();
        exe.touch("spice64.exe");
        let g = resolve_with(Some(&over.0), Some(exe.0.clone()), &fs_probe).unwrap();
        assert_eq!(g.root, dunce::canonicalize(&over.0).unwrap());
    }

    #[test]
    fn g5_exe_relative_resolution() {
        let exe = Temp::new();
        exe.touch("ddr_world_hook.dll");
        let g = resolve_with(None, Some(exe.0.clone()), &fs_probe).unwrap();
        assert_eq!(g.root, dunce::canonicalize(&exe.0).unwrap());
    }

    #[test]
    fn g6_unknown_exe_location_without_override_is_refused() {
        let err = resolve_with(None, None, &fs_probe).unwrap_err();
        assert!(matches!(err, Refusal::UnknownExeLocation));
        assert!(err.to_string().contains("--game-dir"));
    }

    #[test]
    fn g7_derived_paths() {
        let t = Temp::new();
        t.touch("spice64.exe");
        let g = resolve_with(Some(&t.0), None, &fs_probe).unwrap();
        let w = g.root.join(".ddr_world_hook_updater");
        assert_eq!(g.download_dir(), w.join("download"));
        assert_eq!(g.stage_dir(), w.join("stage"));
        assert_eq!(g.backup_dir(), w.join("backup"));
        assert_eq!(g.journal_path(), w.join("journal.json"));
        assert_eq!(
            g.manifest_path(),
            g.root.join("ddr_world_hook_updater.manifest.json")
        );
        assert_eq!(g.log_path(), g.root.join("ddr_world_hook_updater.log"));
    }

    #[test]
    fn g8_nonexistent_override_is_refused_naming_the_path() {
        let t = Temp::new();
        let missing = t.0.join("does_not_exist");
        let err = resolve_with(Some(&missing), None, &fs_probe).unwrap_err();
        assert!(matches!(err, Refusal::Unreadable(_, _)));
        assert!(err.to_string().contains("does_not_exist"));
    }
}
