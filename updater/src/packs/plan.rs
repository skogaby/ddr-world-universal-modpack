//! The file actions of one pack transaction (optional content packs design
//! §4.9) — pure functions from "what the new revision holds", "what the
//! installed marker records" and "what is on disk" to the action list the
//! transactional executor performs.
//!
//! A pack's files are the modpack's own: every file of the new revision is
//! written, over whatever is at its path. Files of the old revision that the
//! new one dropped are pruned only while unmodified, as are a removed pack's
//! files. The marker is written (or pruned) LAST, so an interrupted
//! transaction still shows the old state and is simply repeated.
//!
//! `keep_backup` names the replaced files worth keeping once the transaction
//! has committed: those that were NOT the recorded shipped file (their size
//! differs from the old record, or there was no record). Unmodified shipped
//! files are re-downloadable and their backups are deleted — a run that
//! updates every pack would otherwise leave gigabytes of them.

use std::collections::BTreeSet;

use super::state::Marker;
use crate::plan::Action;
use crate::relpath::RelPath;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackPlan {
    pub actions: Vec<Action>,
    pub keep_backup: BTreeSet<RelPath>,
}

/// Install (or replace, or repair) a pack. `size` reports a path's size on
/// disk (`None` = no file); `hash` its SHA-256 (`None` = unreadable) and is
/// asked only about files the new revision dropped.
pub fn install(
    new: &Marker,
    marker_rel: &RelPath,
    old: Option<&Marker>,
    size: &dyn Fn(&RelPath) -> Option<u64>,
    hash: &dyn Fn(&RelPath) -> Option<String>,
) -> PackPlan {
    let mut actions = Vec::new();
    let mut keep_backup = BTreeSet::new();

    for rel in new.files.keys() {
        let on_disk = size(rel);
        if let Some(bytes) = on_disk {
            let recorded = old.and_then(|m| m.files.get(rel));
            if recorded.is_none_or(|r| r.size != bytes) {
                keep_backup.insert(rel.clone());
            }
        }
        actions.push(Action::Write {
            rel: rel.clone(),
            existed: on_disk.is_some(),
        });
    }

    if let Some(old) = old {
        for (rel, record) in &old.files {
            if new.files.contains_key(rel) {
                continue;
            }
            actions.extend(drop_action(rel, record.size, &record.sha256, size, hash));
        }
    }

    actions.push(Action::Write {
        rel: marker_rel.clone(),
        existed: size(marker_rel).is_some(),
    });
    PackPlan {
        actions,
        keep_backup,
    }
}

/// Remove an installed pack: its unmodified files, then its marker.
pub fn remove(
    old: &Marker,
    marker_rel: &RelPath,
    size: &dyn Fn(&RelPath) -> Option<u64>,
    hash: &dyn Fn(&RelPath) -> Option<String>,
) -> PackPlan {
    let mut actions: Vec<Action> = old
        .files
        .iter()
        .filter_map(|(rel, record)| drop_action(rel, record.size, &record.sha256, size, hash))
        .collect();
    if size(marker_rel).is_some() {
        actions.push(Action::Prune {
            rel: marker_rel.clone(),
        });
    }
    PackPlan {
        actions,
        // Everything pruned was the shipped file: nothing worth keeping.
        keep_backup: BTreeSet::new(),
    }
}

/// What to do with a recorded file that is no longer wanted: nothing when it
/// is gone, prune when it is still the recorded file, keep when it was
/// modified (or cannot be read — never delete what cannot be identified).
fn drop_action(
    rel: &RelPath,
    recorded_size: u64,
    recorded_sha256: &str,
    size: &dyn Fn(&RelPath) -> Option<u64>,
    hash: &dyn Fn(&RelPath) -> Option<String>,
) -> Option<Action> {
    let on_disk = size(rel)?;
    let unmodified = on_disk == recorded_size
        && hash(rel).is_some_and(|h| h.eq_ignore_ascii_case(recorded_sha256));
    Some(if unmodified {
        Action::Prune { rel: rel.clone() }
    } else {
        Action::KeepModified { rel: rel.clone() }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packs::state::{parse_marker, tests::marker_json};
    use std::collections::HashMap;

    const MARKER: &str = "data_mods/custom_models/.packs/dancers-mix.json";

    fn rp(s: &str) -> RelPath {
        RelPath::new(s).unwrap()
    }

    fn p(name: &str) -> String {
        format!("data_mods/custom_models/dancers/Mix/{name}")
    }

    /// A marker of `(name, size, sha)` rows under the pack root.
    fn marker(rows: &[(&str, u64, &str)]) -> Marker {
        let named: Vec<(String, u64, &str)> = rows.iter().map(|(n, s, h)| (p(n), *s, *h)).collect();
        let refs: Vec<(&str, u64, &str)> =
            named.iter().map(|(n, s, h)| (n.as_str(), *s, *h)).collect();
        parse_marker(&marker_json("dancers-mix", "c", &refs), "dancers-mix").unwrap()
    }

    /// Disk state: path → (size, sha).
    struct Disk(HashMap<RelPath, (u64, String)>);

    impl Disk {
        fn new(rows: &[(&str, u64, &str)]) -> Self {
            Disk(
                rows.iter()
                    .map(|(n, s, h)| (rp(n), (*s, h.to_string())))
                    .collect(),
            )
        }
        fn plan_install(&self, new: &Marker, old: Option<&Marker>) -> PackPlan {
            install(
                new,
                &rp(MARKER),
                old,
                &|r| self.0.get(r).map(|v| v.0),
                &|r| self.0.get(r).map(|v| v.1.clone()),
            )
        }
        fn plan_remove(&self, old: &Marker) -> PackPlan {
            remove(old, &rp(MARKER), &|r| self.0.get(r).map(|v| v.0), &|r| {
                self.0.get(r).map(|v| v.1.clone())
            })
        }
    }

    fn write(name: &str, existed: bool) -> Action {
        Action::Write {
            rel: rp(name),
            existed,
        }
    }

    #[test]
    fn fresh_install_writes_everything_then_the_marker() {
        let new = marker(&[("a.arc", 10, "ha"), ("b.txt", 2, "hb")]);
        let plan = Disk::new(&[]).plan_install(&new, None);
        assert_eq!(
            plan.actions,
            vec![
                write(&p("a.arc"), false),
                write(&p("b.txt"), false),
                write(MARKER, false)
            ]
        );
        assert!(plan.keep_backup.is_empty());
    }

    #[test]
    fn a_foreign_file_at_a_shipped_path_is_overwritten_and_its_backup_kept() {
        let new = marker(&[("a.arc", 10, "ha")]);
        let disk = Disk::new(&[(&p("a.arc"), 77, "user")]);
        let plan = disk.plan_install(&new, None);
        assert_eq!(
            plan.actions,
            vec![write(&p("a.arc"), true), write(MARKER, false)]
        );
        assert_eq!(plan.keep_backup, BTreeSet::from([rp(&p("a.arc"))]));
    }

    #[test]
    fn new_revision_replaces_files_and_prunes_only_unmodified_dropped_ones() {
        let old = marker(&[
            ("kept.arc", 10, "h1"),
            ("edited.arc", 10, "h2"),
            ("gone_clean.arc", 5, "h3"),
            ("gone_edited_same_size.arc", 5, "h4"),
            ("gone_edited.arc", 5, "h5"),
            ("gone_missing.arc", 5, "h6"),
        ]);
        let new = marker(&[
            ("kept.arc", 12, "n1"),
            ("edited.arc", 12, "n2"),
            ("added.arc", 1, "n3"),
        ]);
        let disk = Disk::new(&[
            (&p("kept.arc"), 10, "h1"),
            (&p("edited.arc"), 99, "user"),
            (&p("gone_clean.arc"), 5, "h3"),
            (&p("gone_edited_same_size.arc"), 5, "user"),
            (&p("gone_edited.arc"), 6, "user"),
            (MARKER, 300, "m"),
        ]);
        let plan = disk.plan_install(&new, Some(&old));
        assert_eq!(
            plan.actions,
            vec![
                // The new revision's files, in path order.
                write(&p("added.arc"), false),
                write(&p("edited.arc"), true),
                write(&p("kept.arc"), true),
                // The old revision's leftovers, in path order.
                Action::Prune {
                    rel: rp(&p("gone_clean.arc"))
                },
                Action::KeepModified {
                    rel: rp(&p("gone_edited.arc"))
                },
                Action::KeepModified {
                    rel: rp(&p("gone_edited_same_size.arc"))
                },
                // The marker, last.
                write(MARKER, true),
            ]
        );
        // Only the file that was not the recorded one is worth a backup.
        assert_eq!(plan.keep_backup, BTreeSet::from([rp(&p("edited.arc"))]));
    }

    #[test]
    fn repairing_the_same_revision_rewrites_every_file() {
        let same = marker(&[("a.arc", 10, "ha"), ("b.arc", 10, "hb")]);
        let disk = Disk::new(&[(&p("b.arc"), 3, "truncated"), (MARKER, 1, "m")]);
        let plan = disk.plan_install(&same, Some(&same));
        assert_eq!(
            plan.actions,
            vec![
                write(&p("a.arc"), false),
                write(&p("b.arc"), true),
                write(MARKER, true)
            ]
        );
        assert_eq!(plan.keep_backup, BTreeSet::from([rp(&p("b.arc"))]));
    }

    #[test]
    fn removal_prunes_unmodified_files_keeps_modified_ones_and_the_marker_goes_last() {
        let old = marker(&[
            ("clean.arc", 10, "h1"),
            ("edited.arc", 10, "h2"),
            ("missing.arc", 10, "h3"),
            ("unreadable.arc", 10, "h4"),
        ]);
        let mut disk = Disk::new(&[
            (&p("clean.arc"), 10, "H1"),
            (&p("edited.arc"), 10, "user"),
            (MARKER, 300, "m"),
            // A user's own file in the source folder is not in the marker and
            // is therefore never even looked at.
            (&p("my_own.arc"), 1, "mine"),
        ]);
        // On disk at the recorded size, but its hash cannot be taken.
        let plan = remove(
            &old,
            &rp(MARKER),
            &|r| {
                if r.as_str() == p("unreadable.arc") {
                    Some(10)
                } else {
                    disk.0.get(r).map(|v| v.0)
                }
            },
            &|r| disk.0.get(r).map(|v| v.1.clone()),
        );
        assert_eq!(
            plan.actions,
            vec![
                Action::Prune {
                    rel: rp(&p("clean.arc"))
                },
                Action::KeepModified {
                    rel: rp(&p("edited.arc"))
                },
                Action::KeepModified {
                    rel: rp(&p("unreadable.arc"))
                },
                Action::Prune { rel: rp(MARKER) },
            ]
        );
        assert!(plan.keep_backup.is_empty());

        // A marker that is already gone is not pruned.
        disk.0.remove(&rp(MARKER));
        assert!(!disk
            .plan_remove(&old)
            .actions
            .contains(&Action::Prune { rel: rp(MARKER) }));
    }

    #[test]
    fn hashes_are_requested_only_for_dropped_files() {
        let old = marker(&[("kept.arc", 10, "h1"), ("gone.arc", 5, "h3")]);
        let new = marker(&[("kept.arc", 12, "n1")]);
        let asked = std::cell::RefCell::new(Vec::new());
        let _ = install(&new, &rp(MARKER), Some(&old), &|_| Some(5), &|r| {
            asked.borrow_mut().push(r.clone());
            Some("h3".to_string())
        });
        assert_eq!(asked.into_inner(), vec![rp(&p("gone.arc"))]);
    }
}
