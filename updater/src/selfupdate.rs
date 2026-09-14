//! Self-update by rename-swap (design §4.12, R24).
//!
//! Windows (and Wine) refuse to overwrite or delete a running executable but
//! allow renaming it: the running image becomes `ddr_world_hook_updater.exe.old`,
//! the new file takes its name, and the leftover `.old` is deleted on the next
//! start (by then the old process has exited).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const OLD_SUFFIX: &str = ".old";

/// What a completed swap looks like, for undo.
#[derive(Debug)]
pub struct SwapReceipt {
    pub running: PathBuf,
    pub old: PathBuf,
    /// False when there was no previous exe in the folder (plain move).
    pub had_previous: bool,
}

pub fn old_path(running_exe: &Path) -> PathBuf {
    let mut name = running_exe
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(OLD_SUFFIX);
    running_exe.with_file_name(name)
}

/// Delete a leftover `.old` from a previous self-update (best-effort).
pub fn cleanup_stale(running_exe: &Path) -> bool {
    let old = old_path(running_exe);
    old.exists() && fs::remove_file(&old).is_ok()
}

/// Rename `running_exe` → `.old` (if present), then move `new_exe` into place.
pub fn swap_in(new_exe: &Path, running_exe: &Path) -> io::Result<SwapReceipt> {
    let old = old_path(running_exe);
    let had_previous = running_exe.exists();
    if had_previous {
        // A stale .old from an older run would block the rename on Windows.
        let _ = fs::remove_file(&old);
        fs::rename(running_exe, &old)?;
    }
    if let Err(e) = fs::rename(new_exe, running_exe) {
        // Put the old image back so the folder still has a working updater.
        if had_previous {
            let _ = fs::rename(&old, running_exe);
        }
        return Err(e);
    }
    Ok(SwapReceipt {
        running: running_exe.to_path_buf(),
        old,
        had_previous,
    })
}

/// Reverse a swap: remove the new image, restore the old one.
pub fn undo(receipt: &SwapReceipt) -> io::Result<()> {
    if receipt.running.exists() {
        fs::remove_file(&receipt.running)?;
    }
    if receipt.had_previous {
        fs::rename(&receipt.old, &receipt.running)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("ddr_updater_selfupdate_{}_{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn swap_then_undo_then_cleanup() {
        let d = temp();
        let running = d.join("ddr_world_hook_updater.exe");
        let staged = d.join("staged.exe");
        fs::write(&running, b"OLD").unwrap();
        fs::write(&staged, b"NEW").unwrap();

        let receipt = swap_in(&staged, &running).unwrap();
        assert_eq!(fs::read(&running).unwrap(), b"NEW");
        assert_eq!(
            fs::read(d.join("ddr_world_hook_updater.exe.old")).unwrap(),
            b"OLD"
        );
        assert!(!staged.exists());
        assert!(receipt.had_previous);

        undo(&receipt).unwrap();
        assert_eq!(fs::read(&running).unwrap(), b"OLD");
        assert!(!d.join("ddr_world_hook_updater.exe.old").exists());

        fs::write(d.join("ddr_world_hook_updater.exe.old"), b"stale").unwrap();
        assert!(cleanup_stale(&running));
        assert!(!cleanup_stale(&running), "nothing left to clean");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn first_install_without_previous_exe() {
        let d = temp();
        let running = d.join("ddr_world_hook_updater.exe");
        let staged = d.join("staged.exe");
        fs::write(&staged, b"NEW").unwrap();
        let receipt = swap_in(&staged, &running).unwrap();
        assert!(!receipt.had_previous);
        assert_eq!(fs::read(&running).unwrap(), b"NEW");
        undo(&receipt).unwrap();
        assert!(!running.exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn failed_second_rename_restores_old_image() {
        let d = temp();
        let running = d.join("ddr_world_hook_updater.exe");
        fs::write(&running, b"OLD").unwrap();
        let missing = d.join("does_not_exist.exe");
        assert!(swap_in(&missing, &running).is_err());
        assert_eq!(fs::read(&running).unwrap(), b"OLD");
        assert!(!d.join("ddr_world_hook_updater.exe.old").exists());
        let _ = fs::remove_dir_all(&d);
    }
}
