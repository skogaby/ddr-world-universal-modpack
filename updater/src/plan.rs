//! The file-action plan (design §4.10): a pure function from "what the release
//! contains", "what the previous release installed" and "what is on disk" to
//! the ordered list of actions the transactional executor performs.
//!
//! Rules (§2.3): every staged file is release-owned and written unconditionally;
//! a path in the previous manifest but absent from the new release is pruned
//! only when its on-disk hash still equals the recorded one; anything in
//! neither set is never touched. The two merged files and the updater's own
//! executable are handled by their own actions.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::manifest::Manifest;
use crate::relpath::RelPath;

/// The user-owned files that are merged rather than overwritten.
pub const MERGED_FILES: [&str; 2] = ["mod-config.json", "judgement_offsets.csv"];
/// The running image; never written or pruned by the plan.
pub const UPDATER_EXE: &str = "ddr_world_hook_updater.exe";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    /// Move `stage/rel` over `root/rel` (backing up the existing file first).
    Write { rel: RelPath, existed: bool },
    /// Move `root/rel` into `backup/` (dropped by the release, unchanged locally).
    Prune { rel: RelPath },
    /// Dropped by the release but modified locally: keep, report only.
    KeepModified { rel: RelPath },
    /// Write merged bytes over `root/rel` (backing up the existing file first).
    WriteMerged { rel: RelPath, existed: bool },
    /// Replace the updater's own executable by rename-swap.
    SelfUpdate { rel: RelPath },
    /// Write the install manifest (always last).
    WriteManifest,
}

/// What the probe found at a path in the game folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiskState {
    Missing,
    Present { sha256: String },
}

/// Bytes to write for the two merged files, when they need writing at all.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MergedFiles {
    pub config: Option<Vec<u8>>,
    pub csv: Option<Vec<u8>>,
}

impl MergedFiles {
    fn bytes_for(&self, name: &str) -> Option<&Vec<u8>> {
        match name {
            "mod-config.json" => self.config.as_ref(),
            "judgement_offsets.csv" => self.csv.as_ref(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub actions: Vec<Action>,
    /// The `files` map of the manifest to write on success.
    pub new_manifest_files: BTreeMap<RelPath, String>,
}

#[cfg(test)]
impl Plan {
    pub fn count<F: Fn(&Action) -> bool>(&self, pred: F) -> usize {
        self.actions.iter().filter(|a| pred(a)).count()
    }
}

fn is_merged(rel: &RelPath) -> bool {
    MERGED_FILES.contains(&rel.as_str())
}

fn is_updater_exe(rel: &RelPath) -> bool {
    rel.as_str() == UPDATER_EXE
}

/// Build the plan. `stage_hashes` covers every entry of `stage_files`; `probe`
/// is consulted only for staged paths and previous-manifest paths, so files in
/// neither set can never be affected.
pub fn build(
    stage_files: &[RelPath],
    stage_hashes: &BTreeMap<RelPath, String>,
    previous: Option<&Manifest>,
    probe: &dyn Fn(&RelPath) -> DiskState,
    merged: &MergedFiles,
) -> Plan {
    let mut actions = Vec::new();
    let mut new_manifest_files = BTreeMap::new();

    // 1. Release-owned files.
    for rel in stage_files {
        if is_merged(rel) || is_updater_exe(rel) {
            continue;
        }
        let existed = probe(rel) != DiskState::Missing;
        actions.push(Action::Write {
            rel: rel.clone(),
            existed,
        });
        if let Some(h) = stage_hashes.get(rel) {
            new_manifest_files.insert(rel.clone(), h.clone());
        }
    }

    // 2. Previously shipped, now dropped.
    if let Some(prev) = previous {
        for (rel, recorded) in &prev.files {
            if stage_files.contains(rel) || is_merged(rel) || is_updater_exe(rel) {
                continue;
            }
            match probe(rel) {
                DiskState::Missing => {}
                DiskState::Present { sha256 } if sha256.eq_ignore_ascii_case(recorded) => {
                    actions.push(Action::Prune { rel: rel.clone() });
                }
                DiskState::Present { .. } => {
                    actions.push(Action::KeepModified { rel: rel.clone() });
                }
            }
        }
    }

    // 3. Merged files.
    for name in MERGED_FILES {
        if merged.bytes_for(name).is_some() {
            let rel = RelPath::new(name).expect("merged file names are valid RelPaths");
            let existed = probe(&rel) != DiskState::Missing;
            actions.push(Action::WriteMerged { rel, existed });
        }
    }

    // 4. The updater itself, when the release carries a different build.
    let exe = RelPath::new(UPDATER_EXE).expect("valid RelPath");
    if let Some(staged_hash) = stage_files
        .contains(&exe)
        .then(|| stage_hashes.get(&exe))
        .flatten()
    {
        let differs = match probe(&exe) {
            DiskState::Missing => true,
            DiskState::Present { sha256 } => !sha256.eq_ignore_ascii_case(staged_hash),
        };
        if differs {
            actions.push(Action::SelfUpdate { rel: exe.clone() });
        }
        // Recorded either way: the folder's exe is (or becomes) this build.
        new_manifest_files.insert(exe, staged_hash.clone());
    }

    // 5. Manifest, always last.
    actions.push(Action::WriteManifest);

    Plan {
        actions,
        new_manifest_files,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn rp(s: &str) -> RelPath {
        RelPath::new(s).unwrap()
    }

    fn hashes(pairs: &[(&str, &str)]) -> BTreeMap<RelPath, String> {
        pairs.iter().map(|(k, v)| (rp(k), v.to_string())).collect()
    }

    fn probe_from(map: HashMap<RelPath, DiskState>) -> impl Fn(&RelPath) -> DiskState {
        move |rel| map.get(rel).cloned().unwrap_or(DiskState::Missing)
    }

    fn present(h: &str) -> DiskState {
        DiskState::Present {
            sha256: h.to_string(),
        }
    }

    fn previous(files: &[(&str, &str)]) -> Manifest {
        Manifest::new("v1.0", None, "old.zip", "00", hashes(files))
    }

    #[test]
    fn first_run_writes_everything_release_owned_and_prunes_nothing() {
        let stage = vec![
            rp("ddr_world_hook.dll"),
            rp("data_mods/a.png"),
            rp("mod-config.json"),
            rp("judgement_offsets.csv"),
            rp("ddr_world_hook_updater.exe"),
            rp("README.md"),
        ];
        let stage_hashes = hashes(&[
            ("ddr_world_hook.dll", "d1"),
            ("data_mods/a.png", "a1"),
            ("mod-config.json", "c1"),
            ("judgement_offsets.csv", "j1"),
            ("ddr_world_hook_updater.exe", "u1"),
            ("README.md", "r1"),
        ]);
        let mut disk = HashMap::new();
        disk.insert(rp("ddr_world_hook.dll"), present("old"));
        disk.insert(rp("data_mods/user_pack/song.ssq"), present("user")); // unknown file
        let plan = build(
            &stage,
            &stage_hashes,
            None,
            &probe_from(disk),
            &MergedFiles::default(),
        );

        assert_eq!(
            plan.actions,
            vec![
                Action::Write {
                    rel: rp("ddr_world_hook.dll"),
                    existed: true
                },
                Action::Write {
                    rel: rp("data_mods/a.png"),
                    existed: false
                },
                Action::Write {
                    rel: rp("README.md"),
                    existed: false
                },
                Action::SelfUpdate {
                    rel: rp("ddr_world_hook_updater.exe")
                },
                Action::WriteManifest,
            ]
        );
        assert_eq!(
            plan.new_manifest_files,
            hashes(&[
                ("ddr_world_hook.dll", "d1"),
                ("data_mods/a.png", "a1"),
                ("README.md", "r1"),
                ("ddr_world_hook_updater.exe", "u1")
            ])
        );
    }

    #[test]
    fn second_run_prunes_unchanged_keeps_modified_ignores_missing() {
        let stage = vec![rp("ddr_world_hook.dll")];
        let stage_hashes = hashes(&[("ddr_world_hook.dll", "d2")]);
        let prev = previous(&[
            ("ddr_world_hook.dll", "d1"),
            ("data_mods/gone_unchanged.png", "g1"),
            ("data_mods/gone_modified.png", "m1"),
            ("data_mods/gone_missing.png", "x1"),
        ]);
        let mut disk = HashMap::new();
        disk.insert(rp("ddr_world_hook.dll"), present("d1"));
        disk.insert(rp("data_mods/gone_unchanged.png"), present("g1"));
        disk.insert(rp("data_mods/gone_modified.png"), present("changed"));
        let plan = build(
            &stage,
            &stage_hashes,
            Some(&prev),
            &probe_from(disk),
            &MergedFiles::default(),
        );
        assert_eq!(
            plan.actions,
            vec![
                Action::Write {
                    rel: rp("ddr_world_hook.dll"),
                    existed: true
                },
                // Previous-manifest paths are visited in BTreeMap (sorted) order.
                Action::KeepModified {
                    rel: rp("data_mods/gone_modified.png")
                },
                Action::Prune {
                    rel: rp("data_mods/gone_unchanged.png")
                },
                Action::WriteManifest,
            ]
        );
    }

    #[test]
    fn merged_files_and_updater_exe_are_never_pruned_or_written_as_release_files() {
        let stage = vec![rp("ddr_world_hook.dll")];
        let stage_hashes = hashes(&[("ddr_world_hook.dll", "d1")]);
        // A (hypothetical) previous manifest that listed them: still untouched.
        let prev = previous(&[
            ("mod-config.json", "c0"),
            ("judgement_offsets.csv", "j0"),
            ("ddr_world_hook_updater.exe", "u0"),
        ]);
        let mut disk = HashMap::new();
        disk.insert(rp("mod-config.json"), present("c0"));
        disk.insert(rp("judgement_offsets.csv"), present("j0"));
        disk.insert(rp("ddr_world_hook_updater.exe"), present("u0"));
        let plan = build(
            &stage,
            &stage_hashes,
            Some(&prev),
            &probe_from(disk),
            &MergedFiles::default(),
        );
        assert_eq!(
            plan.actions,
            vec![
                Action::Write {
                    rel: rp("ddr_world_hook.dll"),
                    existed: false
                },
                Action::WriteManifest
            ]
        );
    }

    #[test]
    fn merged_bytes_produce_write_merged_with_existed_flag_in_fixed_order() {
        let stage = vec![
            rp("ddr_world_hook.dll"),
            rp("judgement_offsets.csv"),
            rp("mod-config.json"),
        ];
        let stage_hashes = hashes(&[
            ("ddr_world_hook.dll", "d1"),
            ("judgement_offsets.csv", "j1"),
            ("mod-config.json", "c1"),
        ]);
        let mut disk = HashMap::new();
        disk.insert(rp("mod-config.json"), present("c-user"));
        let merged = MergedFiles {
            config: Some(b"{}".to_vec()),
            csv: Some(b"code,p1_offset,p2_offset\n".to_vec()),
        };
        let plan = build(&stage, &stage_hashes, None, &probe_from(disk), &merged);
        assert_eq!(
            plan.actions,
            vec![
                Action::Write {
                    rel: rp("ddr_world_hook.dll"),
                    existed: false
                },
                Action::WriteMerged {
                    rel: rp("mod-config.json"),
                    existed: true
                },
                Action::WriteMerged {
                    rel: rp("judgement_offsets.csv"),
                    existed: false
                },
                Action::WriteManifest,
            ]
        );
        assert!(!plan.new_manifest_files.contains_key(&rp("mod-config.json")));

        // No merged bytes → no merged actions.
        let plan = build(
            &stage,
            &stage_hashes,
            None,
            &|_| DiskState::Missing,
            &MergedFiles::default(),
        );
        assert!(plan.count(|a| matches!(a, Action::WriteMerged { .. })) == 0);
    }

    #[test]
    fn probe_is_never_asked_about_unrelated_paths() {
        let stage = vec![rp("ddr_world_hook.dll")];
        let stage_hashes = hashes(&[("ddr_world_hook.dll", "d1")]);
        let prev = previous(&[("data_mods/old.png", "o1")]);
        let asked = std::cell::RefCell::new(Vec::new());
        let probe = |rel: &RelPath| {
            asked.borrow_mut().push(rel.clone());
            DiskState::Missing
        };
        let _ = build(
            &stage,
            &stage_hashes,
            Some(&prev),
            &probe,
            &MergedFiles::default(),
        );
        let mut asked = asked.into_inner();
        asked.sort();
        assert_eq!(
            asked,
            vec![rp("data_mods/old.png"), rp("ddr_world_hook.dll")]
        );
    }

    #[test]
    fn action_serde_round_trip_for_the_journal() {
        let actions = vec![
            Action::Write {
                rel: rp("a"),
                existed: true,
            },
            Action::Prune { rel: rp("b") },
            Action::WriteMerged {
                rel: rp("mod-config.json"),
                existed: false,
            },
            Action::SelfUpdate {
                rel: rp("ddr_world_hook_updater.exe"),
            },
            Action::WriteManifest,
        ];
        let json = serde_json::to_string(&actions).unwrap();
        let back: Vec<Action> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, actions);
    }
}
