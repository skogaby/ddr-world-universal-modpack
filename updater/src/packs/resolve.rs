//! What a run has to do about packs (optional content packs design §4.6) — a
//! pure function of the catalog, the selection, the installed markers and the
//! state of the recorded files on disk.
//!
//! A run without the chooser acts only on SELECTED packs: it installs the
//! missing ones, replaces other revisions, repairs damaged ones, and removes
//! a selected pack the release no longer offers. An installed pack that is
//! not selected is left exactly as it is.

use std::collections::BTreeMap;

use super::catalog::Catalog;
use super::megabytes;
use super::state::{Marker, Selection, SelectionFile};
use crate::relpath::RelPath;

/// Why a selected pack needs installing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    NotInstalled,
    /// The marker names another revision than the catalog.
    NewRevision,
    /// The right revision, but recorded files are missing or not the
    /// recorded ones.
    Damaged {
        missing: usize,
        changed: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Work {
    pub id: String,
    pub reason: Reason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// The selection this run works with.
    pub selection: Selection,
    /// The selection differs from the file and should be saved.
    pub save_selection: bool,
    /// Selected catalog packs to install, in catalog order.
    pub install: Vec<Work>,
    /// Selected, installed, and no longer offered by the release.
    pub remove: Vec<String>,
    /// Selected ids that are neither in the catalog nor installed (typos).
    pub unknown: Vec<String>,
}

/// `size` reports a path's size on disk (`None` = no file); `hash` its
/// SHA-256 and is consulted only with `force`.
pub fn resolve(
    catalog: &Catalog,
    selection_file: &SelectionFile,
    markers: &BTreeMap<String, Marker>,
    size: &dyn Fn(&RelPath) -> Option<u64>,
    hash: &dyn Fn(&RelPath) -> Option<String>,
    force: bool,
) -> Resolution {
    let mut save_selection = false;
    let mut selection = match selection_file {
        SelectionFile::Loaded(s) => s.clone(),
        // Nothing chosen yet: whatever is installed (unpacked by hand, or a
        // full image) is what the user has. Recorded only when there is
        // something to record, and never over a file that could not be read.
        SelectionFile::Absent | SelectionFile::Unreadable(_) => {
            let mut s = Selection::new();
            s.packs = catalog
                .packs
                .iter()
                .filter(|p| markers.contains_key(&p.id))
                .map(|p| p.id.clone())
                .collect();
            save_selection = matches!(selection_file, SelectionFile::Absent) && !s.packs.is_empty();
            s
        }
    };

    // Packs this release added, for a user who asked for future packs.
    if selection.install_new {
        for pack in &catalog.packs {
            if !selection.is_known(&pack.id) {
                selection.known.push(pack.id.clone());
                if !selection.is_selected(&pack.id) {
                    selection.packs.push(pack.id.clone());
                }
                save_selection = true;
            }
        }
    }

    // Selected packs the release no longer offers.
    let mut remove = Vec::new();
    let mut unknown = Vec::new();
    let mut kept = Vec::new();
    for id in std::mem::take(&mut selection.packs) {
        if catalog.get(&id).is_some() {
            kept.push(id);
        } else if markers.contains_key(&id) {
            remove.push(id);
            save_selection = true;
        } else {
            // Left in the file: it may be a typo the user wants to see.
            unknown.push(id.clone());
            kept.push(id);
        }
    }
    selection.packs = kept;

    let mut install = Vec::new();
    for pack in &catalog.packs {
        if !selection.is_selected(&pack.id) {
            continue;
        }
        let reason = match markers.get(&pack.id) {
            None => Some(Reason::NotInstalled),
            Some(m) if m.content_id != pack.content_id => Some(Reason::NewRevision),
            Some(m) => damage(m, size, hash, force),
        };
        if let Some(reason) = reason {
            install.push(Work {
                id: pack.id.clone(),
                reason,
            });
        }
    }

    Resolution {
        selection,
        save_selection,
        install,
        remove,
        unknown,
    }
}

/// Presence and size of every recorded file; with `force`, the hash too.
fn damage(
    marker: &Marker,
    size: &dyn Fn(&RelPath) -> Option<u64>,
    hash: &dyn Fn(&RelPath) -> Option<String>,
    force: bool,
) -> Option<Reason> {
    let (mut missing, mut changed) = (0, 0);
    for (rel, record) in &marker.files {
        match size(rel) {
            None => missing += 1,
            Some(bytes) if bytes != record.size => changed += 1,
            Some(_) if force => {
                if !hash(rel).is_some_and(|h| h.eq_ignore_ascii_case(&record.sha256)) {
                    changed += 1;
                }
            }
            Some(_) => {}
        }
    }
    (missing + changed > 0).then_some(Reason::Damaged { missing, changed })
}

/// The one-line summary every run prints before the pack prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// Catalog packs with a marker.
    pub installed: usize,
    pub available: usize,
    /// Download size of every catalog pack.
    pub total_zip_bytes: u64,
    /// `<title> <group>` of packs the user has not been shown yet.
    pub new: Vec<String>,
    /// Selected packs still waiting to be installed.
    pub pending: usize,
}

pub fn status(
    catalog: &Catalog,
    selection: &Selection,
    markers: &BTreeMap<String, Marker>,
    pending: usize,
) -> Status {
    // "New" needs a baseline: before the user has seen the list once, every
    // pack is simply available.
    let new = if selection.known.is_empty() {
        Vec::new()
    } else {
        catalog
            .packs
            .iter()
            .filter(|p| {
                !selection.is_known(&p.id)
                    && !selection.is_selected(&p.id)
                    && !markers.contains_key(&p.id)
            })
            .map(|p| format!("{} {}", p.title, p.group.to_lowercase()))
            .collect()
    };
    Status {
        installed: catalog
            .packs
            .iter()
            .filter(|p| markers.contains_key(&p.id))
            .count(),
        available: catalog.packs.len(),
        total_zip_bytes: catalog.packs.iter().map(|p| p.zip_size).sum(),
        new,
        pending,
    }
}

impl Status {
    pub fn line(&self) -> String {
        let mut line = if self.installed == 0 && self.pending == 0 {
            format!(
                "Optional packs: none installed ({} available, {} to download)",
                self.available,
                megabytes(self.total_zip_bytes)
            )
        } else {
            format!(
                "Optional packs: {} of {} installed",
                self.installed, self.available
            )
        };
        if !self.new.is_empty() {
            const SHOWN: usize = 3;
            let names: Vec<&str> = self.new.iter().take(SHOWN).map(String::as_str).collect();
            line.push_str(&format!("; new in this release: {}", names.join(", ")));
            if self.new.len() > SHOWN {
                line.push_str(&format!(" and {} more", self.new.len() - SHOWN));
            }
        }
        if self.pending > 0 {
            line.push_str(&format!("; {} waiting to download", self.pending));
        }
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packs::catalog::tests::{entry, test_pack};
    use crate::packs::catalog::{parse, Pack};
    use crate::packs::selection_of;
    use crate::packs::state::{parse_marker, tests::marker_json};
    use std::collections::HashMap;

    fn rp(s: &str) -> RelPath {
        RelPath::new(s).unwrap()
    }

    /// Packs `a`, `b`, `c` at revision `r1` (content ids `aaaa…`, `bbbb…`, …).
    fn catalog() -> Catalog {
        let packs: Vec<Pack> = ["a", "b", "c"]
            .iter()
            .map(|id| test_pack(id, &id.to_uppercase(), &id.repeat(64)))
            .collect();
        Catalog { packs }
    }

    fn file(id: &str) -> String {
        format!(
            "data_mods/custom_models/dancers/{}/x.arc",
            id.to_uppercase()
        )
    }

    /// A marker for `id` at `content_id` recording one 10-byte file.
    fn marker(id: &str, content_id: &str) -> Marker {
        parse_marker(&marker_json(id, content_id, &[(&file(id), 10, "h")]), id).unwrap()
    }

    fn markers(rows: &[(&str, &str)]) -> BTreeMap<String, Marker> {
        rows.iter()
            .map(|(id, cid)| (id.to_string(), marker(id, cid)))
            .collect()
    }

    fn current(ids: &[&str]) -> BTreeMap<String, Marker> {
        ids.iter()
            .map(|id| (id.to_string(), marker(id, &id.repeat(64))))
            .collect()
    }

    /// Every recorded file present at its recorded size and hash.
    fn intact(_: &RelPath) -> Option<u64> {
        Some(10)
    }

    fn run(sel: &SelectionFile, markers: &BTreeMap<String, Marker>) -> Resolution {
        resolve(
            &catalog(),
            sel,
            markers,
            &intact,
            &|_| Some("h".into()),
            false,
        )
    }

    fn ids(work: &[Work]) -> Vec<&str> {
        work.iter().map(|w| w.id.as_str()).collect()
    }

    #[test]
    fn nothing_chosen_and_nothing_installed_does_nothing() {
        let r = run(&SelectionFile::Absent, &BTreeMap::new());
        assert!(r.install.is_empty() && r.remove.is_empty() && r.unknown.is_empty());
        assert!(!r.save_selection);
        assert!(r.selection.packs.is_empty());
    }

    #[test]
    fn without_a_selection_file_the_installed_packs_become_the_selection() {
        // `b` unpacked by hand at an old revision; `zz` is not offered at all.
        let m = markers(&[("b", "old"), ("zz", "x")]);
        let r = run(&SelectionFile::Absent, &m);
        assert_eq!(r.selection.packs, ["b"]);
        assert!(r.save_selection);
        assert_eq!(
            r.install,
            vec![Work {
                id: "b".into(),
                reason: Reason::NewRevision
            }]
        );
        assert!(r.remove.is_empty(), "an unselected stranger is left alone");

        // An unreadable file gives the same working selection but is never
        // overwritten by an unattended run.
        let r = run(&SelectionFile::Unreadable("bad".into()), &m);
        assert_eq!(r.selection.packs, ["b"]);
        assert!(!r.save_selection);
    }

    #[test]
    fn selected_packs_are_installed_updated_or_left_alone() {
        let sel = SelectionFile::Loaded(selection_of(&["a", "b", "c"]));
        let m = markers(&[("b", &"b".repeat(64)), ("c", "old revision")]);
        let r = run(&sel, &m);
        assert_eq!(
            r.install,
            vec![
                Work {
                    id: "a".into(),
                    reason: Reason::NotInstalled
                },
                Work {
                    id: "c".into(),
                    reason: Reason::NewRevision
                },
            ]
        );
        assert!(!r.save_selection);
    }

    #[test]
    fn installed_but_unselected_packs_are_not_touched() {
        let sel = SelectionFile::Loaded(selection_of(&["a"]));
        // `b` is installed at an old revision and damaged, but not selected.
        let m = markers(&[("a", &"a".repeat(64)), ("b", "old")]);
        let r = resolve(&catalog(), &sel, &m, &|_| None, &|_| None, true);
        assert_eq!(ids(&r.install), ["a"], "only the selected pack is repaired");
        assert!(r.remove.is_empty());
    }

    #[test]
    fn damage_is_presence_and_size_unless_forced() {
        let sel = SelectionFile::Loaded(selection_of(&["a", "b", "c"]));
        let m = current(&["a", "b", "c"]);
        let disk: HashMap<RelPath, (u64, &str)> = HashMap::from([
            // a: missing. b: wrong size. c: right size, different bytes.
            (rp(&file("b")), (7, "h")),
            (rp(&file("c")), (10, "edited")),
        ]);
        let size = |r: &RelPath| disk.get(r).map(|v| v.0);
        let hash = |r: &RelPath| disk.get(r).map(|v| v.1.to_string());

        let quick = resolve(&catalog(), &sel, &m, &size, &hash, false);
        assert_eq!(
            quick.install,
            vec![
                Work {
                    id: "a".into(),
                    reason: Reason::Damaged {
                        missing: 1,
                        changed: 0
                    }
                },
                Work {
                    id: "b".into(),
                    reason: Reason::Damaged {
                        missing: 0,
                        changed: 1
                    }
                },
            ],
            "a same-size edit is not seen without --force"
        );
        let forced = resolve(&catalog(), &sel, &m, &size, &hash, true);
        assert_eq!(ids(&forced.install), ["a", "b", "c"]);
        assert_eq!(
            forced.install[2].reason,
            Reason::Damaged {
                missing: 0,
                changed: 1
            }
        );
    }

    #[test]
    fn hashes_are_not_taken_without_force() {
        let sel = SelectionFile::Loaded(selection_of(&["a"]));
        let r = resolve(
            &catalog(),
            &sel,
            &current(&["a"]),
            &intact,
            &|_| panic!("no hashing on an ordinary run"),
            false,
        );
        assert!(r.install.is_empty());
    }

    #[test]
    fn future_packs_are_adopted_only_when_asked_for() {
        let mut s = selection_of(&["a"]);
        s.known = vec!["a".into(), "b".into()];
        let installed = current(&["a"]);

        // Not asked for: `c` is new but stays unselected.
        let r = run(&SelectionFile::Loaded(s.clone()), &installed);
        assert!(r.install.is_empty() && !r.save_selection);
        assert_eq!(r.selection, s);

        // Asked for: `c` (unknown) is adopted; `b` (known, declined) is not.
        s.install_new = true;
        let r = run(&SelectionFile::Loaded(s), &installed);
        assert_eq!(r.selection.packs, ["a", "c"]);
        assert_eq!(r.selection.known, ["a", "b", "c"]);
        assert!(r.save_selection);
        assert_eq!(
            r.install,
            vec![Work {
                id: "c".into(),
                reason: Reason::NotInstalled
            }]
        );
    }

    #[test]
    fn a_selected_pack_the_release_dropped_is_removed_and_typos_are_reported() {
        let sel = SelectionFile::Loaded(selection_of(&["a", "gone", "typo"]));
        let m = markers(&[("a", &"a".repeat(64)), ("gone", "x")]);
        let r = run(&sel, &m);
        assert_eq!(r.remove, ["gone"]);
        assert_eq!(r.unknown, ["typo"]);
        assert_eq!(
            r.selection.packs,
            ["a", "typo"],
            "the typo stays for the user to see"
        );
        assert!(r.save_selection);
        assert!(r.install.is_empty());
    }

    #[test]
    fn status_lines() {
        let c = catalog();
        let total = megabytes(300);
        let none = status(&c, &Selection::new(), &BTreeMap::new(), 0);
        assert_eq!(
            none.line(),
            format!("Optional packs: none installed (3 available, {total} to download)")
        );

        let mut s = selection_of(&["a"]);
        let some = status(&c, &s, &current(&["a"]), 0);
        assert_eq!(some.line(), "Optional packs: 1 of 3 installed");
        assert!(
            some.new.is_empty(),
            "no baseline yet, so nothing is called new"
        );

        s.known = vec!["a".into(), "b".into()];
        let with_new = status(&c, &s, &current(&["a"]), 2);
        assert_eq!(
            with_new.line(),
            "Optional packs: 1 of 3 installed; new in this release: C dancers; 2 waiting to download"
        );

        // Selected but not yet installed: pending, not "none installed".
        let pending = status(&c, &selection_of(&["a"]), &BTreeMap::new(), 1);
        assert_eq!(
            pending.line(),
            "Optional packs: 0 of 3 installed; 1 waiting to download"
        );
    }

    #[test]
    fn status_names_at_most_three_new_packs() {
        let packs: Vec<serde_json::Value> = (1..=6)
            .map(|i| {
                let mut e = entry(
                    &format!("p{i}"),
                    &format!("data_mods/custom_models/dancers/P{i}"),
                );
                e["title"] = serde_json::json!(format!("P{i}"));
                e
            })
            .collect();
        let c = parse(
            &serde_json::json!({"schema": 1, "packs": packs}).to_string(),
            false,
        )
        .unwrap();
        let mut s = selection_of(&["p1"]);
        s.known = vec!["p1".into()];
        let line = status(&c, &s, &BTreeMap::new(), 0).line();
        assert!(
            line.ends_with("new in this release: P2 dancers, P3 dancers, P4 dancers and 2 more"),
            "{line}"
        );
    }
}
