//! Transactional apply (design §6): execute a [`Plan`] against the game folder
//! with a backup of every file it replaces or removes, roll everything back on
//! the first error, and write the manifest last.
//!
//! Every mutation is a same-volume `rename` (stage and backup live inside the
//! game folder's work directory), so each step is atomic on its own and the
//! set of steps performed so far is exactly what a rollback has to undo. The
//! journal names the planned actions before the first mutation; deleting it
//! after the manifest write marks the run complete (crash recovery reads it).

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::fault::Fault;
use crate::gamedir::GameDir;
use crate::manifest::{self, Manifest};
use crate::plan::{Action, MergedFiles, Plan};
use crate::relpath::RelPath;
use crate::selfupdate::{self, SwapReceipt};

/// Written before the first mutation, deleted after the last.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Journal {
    pub tag: String,
    pub started_unix: u64,
    pub actions: Vec<Action>,
}

impl Journal {
    pub fn new(tag: &str, actions: &[Action]) -> Self {
        Journal {
            tag: tag.to_string(),
            started_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            actions: actions.to_vec(),
        }
    }

    pub fn write(&self, path: &Path) -> io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        fs::write(path, json)
    }

    pub fn read(path: &Path) -> io::Result<Journal> {
        let text = fs::read_to_string(path)?;
        serde_json::from_str(&text).map_err(io::Error::other)
    }
}

/// What a successful run did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Summary {
    pub written: usize,
    pub pruned: usize,
    pub kept_modified: Vec<RelPath>,
    pub merged_written: Vec<RelPath>,
    /// False when every file action succeeded but the manifest could not be
    /// written (the next run simply re-installs).
    pub manifest_written: bool,
    /// The updater's own executable was replaced.
    pub self_updated: bool,
    /// Directories under `data_mods/` removed because a prune emptied them.
    pub dirs_removed: usize,
}

impl Summary {
    /// The §4.13 "Installing ..." line.
    pub fn install_line(&self) -> String {
        let mut s = format!("{} files written", self.written);
        if self.pruned > 0 {
            s.push_str(&format!(", {} obsolete files removed", self.pruned));
        }
        if !self.kept_modified.is_empty() {
            s.push_str(&format!(
                ", {} locally modified file{} kept",
                self.kept_modified.len(),
                if self.kept_modified.len() == 1 {
                    ""
                } else {
                    "s"
                }
            ));
        }
        for m in &self.merged_written {
            s.push_str(&format!(", {m} written"));
        }
        if self.self_updated {
            s.push_str(", updater replaced");
        }
        s
    }
}

#[derive(Debug)]
pub enum ApplyError {
    /// The run failed and every performed action was undone.
    RolledBack(String),
    /// The run failed and at least one undo failed too.
    RollbackFailed {
        cause: String,
        restore_errors: Vec<String>,
    },
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApplyError::RolledBack(c) => write!(f, "update failed and was rolled back ({c})"),
            ApplyError::RollbackFailed {
                cause,
                restore_errors,
            } => write!(
                f,
                "update failed ({cause}) AND {} file(s) could not be restored",
                restore_errors.len()
            ),
        }
    }
}

/// How to undo one performed action.
#[derive(Debug)]
enum Done {
    /// Target replaced; original is at `backup`.
    Replaced { target: PathBuf, backup: PathBuf },
    /// Target created where nothing existed.
    Created { target: PathBuf },
    /// Target moved to `backup`.
    Pruned { target: PathBuf, backup: PathBuf },
    /// The updater executable was swapped.
    SelfSwapped(SwapReceipt),
}

/// Execute `plan`. The staged release lives at `stage_root`; `merged` carries
/// the bytes for any `WriteMerged` actions; `manifest` is written last.
pub fn execute(
    game: &GameDir,
    plan: &Plan,
    stage_root: &Path,
    merged: &MergedFiles,
    manifest: &Manifest,
    fault: Fault,
) -> Result<Summary, ApplyError> {
    let backup_root = game.backup_dir();
    let prepare = || -> io::Result<()> {
        fs::create_dir_all(&game.work)?;
        if backup_root.exists() {
            fs::remove_dir_all(&backup_root)?;
        }
        fs::create_dir_all(&backup_root)?;
        Journal::new(&manifest.tag, &plan.actions).write(&game.journal_path())
    };
    if let Err(e) = prepare() {
        // Nothing has been touched yet.
        return Err(ApplyError::RolledBack(format!(
            "could not prepare the work directory: {e}"
        )));
    }

    let mut done: Vec<Done> = Vec::new();
    let mut summary = Summary {
        manifest_written: true,
        ..Summary::default()
    };
    let mut failure: Option<String> = None;
    let mut pruned_rels: Vec<RelPath> = Vec::new();

    for action in &plan.actions {
        let result = match action {
            Action::Write { rel, existed } => {
                let target = rel.to_path(&game.root);
                let source = rel.to_path(stage_root);
                move_into_place(&source, &target, *existed, &backup_root, rel, &mut done).map(
                    |_| {
                        summary.written += 1;
                    },
                )
            }
            Action::Prune { rel } => {
                let target = rel.to_path(&game.root);
                let backup = rel.to_path(&backup_root);
                ensure_parent(&backup)
                    .and_then(|_| fs::rename(&target, &backup))
                    .map(|_| {
                        done.push(Done::Pruned { target, backup });
                        summary.pruned += 1;
                        pruned_rels.push(rel.clone());
                    })
                    .map_err(|e| format!("cannot remove obsolete {rel}: {e}"))
            }
            Action::KeepModified { rel } => {
                summary.kept_modified.push(rel.clone());
                Ok(())
            }
            Action::WriteMerged { rel, existed } => {
                let bytes = match rel.as_str() {
                    "mod-config.json" => merged.config.as_deref(),
                    "judgement_offsets.csv" => merged.csv.as_deref(),
                    _ => None,
                };
                match bytes {
                    None => Err(format!("no merged bytes for {rel}")),
                    Some(bytes) => {
                        let target = rel.to_path(&game.root);
                        let tmp = game
                            .work
                            .join(format!("{}.tmp", rel.as_str().replace('/', "_")));
                        fs::write(&tmp, bytes)
                            .map_err(|e| format!("cannot write {rel}: {e}"))
                            .and_then(|_| {
                                move_into_place(
                                    &tmp,
                                    &target,
                                    *existed,
                                    &backup_root,
                                    rel,
                                    &mut done,
                                )
                            })
                            .map(|_| summary.merged_written.push(rel.clone()))
                    }
                }
            }
            Action::SelfUpdate { rel } => {
                let staged = rel.to_path(stage_root);
                let running = rel.to_path(&game.root);
                match selfupdate::swap_in(&staged, &running) {
                    Ok(receipt) => {
                        done.push(Done::SelfSwapped(receipt));
                        summary.self_updated = true;
                    }
                    Err(e) => {
                        // R24: never fatal — the old updater keeps working.
                        crate::log_warn!(
                            "could not replace {rel} ({e}); keeping the current updater"
                        );
                    }
                }
                Ok(())
            }
            Action::WriteManifest => {
                if let Err(e) = manifest::write(&game.manifest_path(), manifest) {
                    // Non-fatal by design (§7): the next run re-installs.
                    crate::log_warn!(
                        "could not write {}: {e}; the next run will repeat this update",
                        game.manifest_path().display()
                    );
                    summary.manifest_written = false;
                }
                Ok(())
            }
        };

        if let Err(msg) = result {
            failure = Some(msg);
            break;
        }

        match fault {
            Fault::ApplyAfter(n) if done.len() >= n => {
                failure = Some(format!("injected fault after {} action(s)", done.len()));
                break;
            }
            Fault::Rollback if done.iter().any(has_backup) => {
                sabotage_first_restore(&done);
                failure = Some("injected fault with unrestorable backup".to_string());
                break;
            }
            Fault::CrashAfter(n) if done.len() >= n => {
                // Simulated crash: no rollback, journal left behind.
                crate::log_error!(
                    "injected crash after {} action(s); exiting without rollback",
                    done.len()
                );
                std::process::exit(70);
            }
            _ => {}
        }
    }

    match failure {
        None => {
            let _ = fs::remove_file(game.journal_path());
            let _ = fs::remove_dir_all(game.download_dir());
            let _ = fs::remove_dir_all(game.stage_dir());
            summary.dirs_removed = remove_emptied_dirs(&game.root, &pruned_rels);
            Ok(summary)
        }
        Some(cause) => {
            let restore_errors = rollback(&done);
            if restore_errors.is_empty() {
                // Folder is back to its pre-run state: nothing left to keep.
                let _ = fs::remove_file(game.journal_path());
                let _ = fs::remove_dir_all(game.download_dir());
                let _ = fs::remove_dir_all(game.stage_dir());
                Err(ApplyError::RolledBack(cause))
            } else {
                // Leave the journal in place: it marks the folder as suspect.
                Err(ApplyError::RollbackFailed {
                    cause,
                    restore_errors,
                })
            }
        }
    }
}

/// Move `source` over `target`, backing the existing target up first when
/// `existed`. Records the undo step in `done` only once the move succeeded.
fn move_into_place(
    source: &Path,
    target: &Path,
    existed: bool,
    backup_root: &Path,
    rel: &RelPath,
    done: &mut Vec<Done>,
) -> Result<(), String> {
    ensure_parent(target).map_err(|e| format!("cannot create directory for {rel}: {e}"))?;
    if existed {
        let backup = rel.to_path(backup_root);
        ensure_parent(&backup)
            .map_err(|e| format!("cannot create backup directory for {rel}: {e}"))?;
        fs::rename(target, &backup).map_err(|e| format!("cannot back up {rel}: {e}"))?;
        // From here on the original is safe in backup/; even if the next
        // rename fails, rollback restores it.
        done.push(Done::Replaced {
            target: target.to_path_buf(),
            backup,
        });
        fs::rename(source, target).map_err(|e| format!("cannot install {rel}: {e}"))?;
    } else {
        fs::rename(source, target).map_err(|e| format!("cannot install {rel}: {e}"))?;
        done.push(Done::Created {
            target: target.to_path_buf(),
        });
    }
    Ok(())
}

fn ensure_parent(path: &Path) -> io::Result<()> {
    match path.parent() {
        Some(p) => fs::create_dir_all(p),
        None => Ok(()),
    }
}

/// Undo performed actions in reverse. Returns the errors encountered (empty =
/// the folder is exactly as before the run).
fn rollback(done: &[Done]) -> Vec<String> {
    let mut errors = Vec::new();
    for d in done.iter().rev() {
        let result = match d {
            Done::Replaced { target, backup } => {
                if target.exists() {
                    if let Err(e) = fs::remove_file(target) {
                        errors.push(format!("{}: cannot remove new file: {e}", target.display()));
                        continue;
                    }
                }
                fs::rename(backup, target)
            }
            Done::Created { target } => fs::remove_file(target),
            Done::Pruned { target, backup } => fs::rename(backup, target),
            Done::SelfSwapped(receipt) => selfupdate::undo(receipt),
        };
        if let Err(e) = result {
            let what = match d {
                Done::Replaced { target, .. } | Done::Pruned { target, .. } => {
                    format!("{}: cannot restore from backup: {e}", target.display())
                }
                Done::Created { target } => {
                    format!("{}: cannot remove new file: {e}", target.display())
                }
                Done::SelfSwapped(r) => {
                    format!("{}: cannot undo the updater swap: {e}", r.running.display())
                }
            };
            errors.push(what);
        }
    }
    errors
}

fn has_backup(d: &Done) -> bool {
    matches!(d, Done::Replaced { .. } | Done::Pruned { .. })
}

/// After prunes: remove directories under `data_mods/` that are now empty,
/// walking up each pruned path's parents and stopping at the first non-empty
/// ancestor. `data_mods/` itself is never removed. Best-effort.
fn remove_emptied_dirs(root: &Path, pruned: &[RelPath]) -> usize {
    let mut removed = 0;
    for rel in pruned {
        let mut cur = rel.parent();
        while let Some(dir) = cur {
            let s = dir.as_str();
            if s == "data_mods" || !s.starts_with("data_mods/") {
                break;
            }
            let path = dir.to_path(root);
            let empty = fs::read_dir(&path)
                .map(|mut it| it.next().is_none())
                .unwrap_or(false);
            if !empty || fs::remove_dir(&path).is_err() {
                break;
            }
            removed += 1;
            cur = dir.parent();
        }
    }
    removed
}

/// What start-up recovery found.
#[derive(Debug, PartialEq, Eq)]
pub enum Recovery {
    /// No journal: the previous run finished (or never started an apply).
    Clean,
    /// A journal was present; this is what restoring it did.
    Recovered {
        restored: usize,
        removed: usize,
        errors: Vec<String>,
    },
}

/// Design §6.4: if `journal.json` exists the previous run died mid-apply.
/// Restore every backed-up file, delete files the run created, delete the
/// journal. The manifest still names the OLD release (it is written last), so
/// the interrupted update simply happens again afterwards.
pub fn recover_if_interrupted(game: &GameDir) -> Recovery {
    let journal_path = game.journal_path();
    if !journal_path.exists() {
        return Recovery::Clean;
    }
    let journal = match Journal::read(&journal_path) {
        Ok(j) => j,
        Err(e) => {
            crate::log_warn!(
                "found an unreadable journal from an interrupted run ({e}); removing it"
            );
            let _ = fs::remove_file(&journal_path);
            return Recovery::Recovered {
                restored: 0,
                removed: 0,
                errors: vec![format!("unreadable journal: {e}")],
            };
        }
    };
    let backup_root = game.backup_dir();
    let mut restored = 0;
    let mut removed = 0;
    let mut errors = Vec::new();
    for action in journal.actions.iter().rev() {
        match action {
            Action::Write { rel, existed: true }
            | Action::WriteMerged { rel, existed: true }
            | Action::Prune { rel } => {
                let backup = rel.to_path(&backup_root);
                if !backup.exists() {
                    continue; // never reached, or already restored
                }
                let target = rel.to_path(&game.root);
                if target.exists() {
                    if let Err(e) = fs::remove_file(&target) {
                        errors.push(format!("{rel}: cannot remove partial file: {e}"));
                        continue;
                    }
                }
                match ensure_parent(&target).and_then(|_| fs::rename(&backup, &target)) {
                    Ok(()) => restored += 1,
                    Err(e) => errors.push(format!("{rel}: cannot restore from backup: {e}")),
                }
            }
            Action::Write {
                rel,
                existed: false,
            }
            | Action::WriteMerged {
                rel,
                existed: false,
            } => {
                let target = rel.to_path(&game.root);
                if target.exists() {
                    match fs::remove_file(&target) {
                        Ok(()) => removed += 1,
                        Err(e) => errors.push(format!("{rel}: cannot remove created file: {e}")),
                    }
                }
            }
            // An interrupted self-update is not undone here: the running
            // image cannot be replaced from within (the .old is cleaned up at
            // start-up). Nothing to do for the rest.
            Action::SelfUpdate { .. } | Action::KeepModified { .. } | Action::WriteManifest => {}
        }
    }
    let _ = fs::remove_file(&journal_path);
    let _ = fs::remove_dir_all(game.stage_dir());
    let _ = fs::remove_dir_all(game.download_dir());
    Recovery::Recovered {
        restored,
        removed,
        errors,
    }
}

/// Dev fault: make the first restorable action fail by deleting its backup.
fn sabotage_first_restore(done: &[Done]) {
    for d in done {
        match d {
            Done::Replaced { backup, .. } | Done::Pruned { backup, .. } => {
                let _ = fs::remove_file(backup);
                return;
            }
            Done::Created { .. } | Done::SelfSwapped(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_round_trip() {
        let dir = std::env::temp_dir().join(format!("ddr_updater_journal_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let actions = vec![
            Action::Write {
                rel: RelPath::new("a").unwrap(),
                existed: true,
            },
            Action::WriteManifest,
        ];
        let j = Journal::new("v1", &actions);
        let path = dir.join("journal.json");
        j.write(&path).unwrap();
        let back = Journal::read(&path).unwrap();
        assert_eq!(back, j);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recovery_restores_backups_removes_created_and_deletes_journal() {
        let dir = std::env::temp_dir().join(format!("ddr_updater_recovery_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let game = GameDir::for_test(&dir);
        fs::create_dir_all(game.backup_dir().join("data_mods")).unwrap();
        fs::create_dir_all(game.root.join("data_mods")).unwrap();
        // Replaced file: partial new content on disk, original in backup.
        fs::write(game.root.join("ddr_world_hook.dll"), b"PARTIAL").unwrap();
        fs::write(game.backup_dir().join("ddr_world_hook.dll"), b"ORIGINAL").unwrap();
        // Created file (no backup): must be removed.
        fs::write(game.root.join("README.md"), b"new").unwrap();
        // Pruned file: sits in backup, must come back.
        fs::write(game.backup_dir().join("data_mods/gone.png"), b"png").unwrap();
        // A Write that never happened (no backup, no file): nothing to do.
        let actions = vec![
            Action::Write {
                rel: RelPath::new("ddr_world_hook.dll").unwrap(),
                existed: true,
            },
            Action::Write {
                rel: RelPath::new("README.md").unwrap(),
                existed: false,
            },
            Action::Prune {
                rel: RelPath::new("data_mods/gone.png").unwrap(),
            },
            Action::Write {
                rel: RelPath::new("data_mods/never.png").unwrap(),
                existed: true,
            },
            Action::WriteManifest,
        ];
        Journal::new("v9", &actions)
            .write(&game.journal_path())
            .unwrap();

        let r = recover_if_interrupted(&game);
        assert_eq!(
            r,
            Recovery::Recovered {
                restored: 2,
                removed: 1,
                errors: vec![]
            }
        );
        assert_eq!(
            fs::read(game.root.join("ddr_world_hook.dll")).unwrap(),
            b"ORIGINAL"
        );
        assert!(!game.root.join("README.md").exists());
        assert_eq!(
            fs::read(game.root.join("data_mods/gone.png")).unwrap(),
            b"png"
        );
        assert!(!game.journal_path().exists());
        assert_eq!(recover_if_interrupted(&game), Recovery::Clean);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn emptied_dirs_are_removed_but_data_mods_and_non_empty_dirs_stay() {
        let dir =
            std::env::temp_dir().join(format!("ddr_updater_emptydirs_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("data_mods/a/b")).unwrap();
        fs::create_dir_all(dir.join("data_mods/c")).unwrap();
        fs::write(dir.join("data_mods/c/keep.txt"), b"x").unwrap();
        let pruned = vec![
            RelPath::new("data_mods/a/b/gone.png").unwrap(),
            RelPath::new("data_mods/c/gone.png").unwrap(),
            RelPath::new("top_level_gone.txt").unwrap(),
        ];
        assert_eq!(remove_emptied_dirs(&dir, &pruned), 2);
        assert!(!dir.join("data_mods/a").exists());
        assert!(dir.join("data_mods/c/keep.txt").exists());
        assert!(dir.join("data_mods").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn install_line_wording() {
        let s = Summary {
            written: 3,
            pruned: 0,
            kept_modified: vec![],
            merged_written: vec![],
            manifest_written: true,
            self_updated: false,
            dirs_removed: 0,
        };
        assert_eq!(s.install_line(), "3 files written");
        let s = Summary {
            written: 411,
            pruned: 2,
            kept_modified: vec![RelPath::new("README.md").unwrap()],
            merged_written: vec![RelPath::new("mod-config.json").unwrap()],
            manifest_written: true,
            self_updated: true,
            dirs_removed: 0,
        };
        assert_eq!(
            s.install_line(),
            "411 files written, 2 obsolete files removed, 1 locally modified file kept, mod-config.json written, updater replaced"
        );
    }
}
