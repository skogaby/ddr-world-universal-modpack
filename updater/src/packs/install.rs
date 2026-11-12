//! One pack transaction (optional content packs design §4.7–§4.9): obtain the
//! zip, check it is the catalog's, extract it, plan and apply — or remove an
//! installed pack. Each pack is its own journaled transaction with its own
//! backup subfolder, so a failed pack rolls back alone and never disturbs the
//! base release or another pack.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use super::catalog::Pack;
use super::plan;
use super::space;
use super::state::Marker;
use crate::apply::{self, ApplyError, Session, Summary, Transaction};
use crate::archive;
use crate::download::{self, Target};
use crate::fault::Fault;
use crate::gamedir::GameDir;
use crate::github::NetError;
use crate::manifest;
use crate::plan::MergedFiles;
use crate::relpath::RelPath;

/// Why a pack was not installed (or removed).
#[derive(Debug)]
pub enum PackError {
    /// No zip could be had this run; nothing is wrong with the pack. `network`
    /// says the cause was the connection, so further downloads are pointless.
    Unavailable { why: String, network: bool },
    /// The zip, or the archive inside it, is not the pack the catalog names.
    Rejected(String),
    /// Not enough room on the game folder's volume.
    NoSpace { short_by: u64 },
    /// The transaction failed and was undone.
    RolledBack(String),
    /// The transaction failed and could not be fully undone.
    RollbackFailed {
        cause: String,
        restore_errors: Vec<String>,
    },
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackError::Unavailable { why, .. } => write!(f, "{why}"),
            PackError::Rejected(why) => write!(f, "{why}"),
            PackError::NoSpace { short_by } => write!(
                f,
                "not enough free disk space ({} short)",
                super::megabytes(*short_by)
            ),
            PackError::RolledBack(cause) => write!(f, "{cause}; the change was rolled back"),
            PackError::RollbackFailed { cause, .. } => {
                write!(f, "{cause}, and the rollback did not fully succeed")
            }
        }
    }
}

impl From<ApplyError> for PackError {
    fn from(e: ApplyError) -> Self {
        match e {
            ApplyError::RolledBack(cause) => PackError::RolledBack(cause),
            ApplyError::RollbackFailed {
                cause,
                restore_errors,
            } => PackError::RollbackFailed {
                cause,
                restore_errors,
            },
        }
    }
}

/// Where pack zips come from.
pub struct Sources<'a> {
    /// `--packs-dir`: a folder of pack zips, looked up by asset name first.
    pub packs_dir: Option<&'a Path>,
    /// The HTTP agent, or `None` when the network must not be used this run.
    pub agent: Option<&'a ureq::Agent>,
}

/// A zip ready to extract.
pub struct Obtained {
    pub path: PathBuf,
    /// Downloaded into the work folder (delete after use), as opposed to a
    /// file in the user's `--packs-dir` (never touched).
    pub downloaded: bool,
    /// Bytes reused from an earlier, interrupted download.
    pub resumed_from: u64,
}

fn size_of(game: &GameDir) -> impl Fn(&RelPath) -> Option<u64> + '_ {
    move |rel| {
        fs::metadata(rel.to_path(&game.root))
            .ok()
            .filter(|m| m.is_file())
            .map(|m| m.len())
    }
}

fn hash_of(game: &GameDir) -> impl Fn(&RelPath) -> Option<String> + '_ {
    move |rel| manifest::sha256_file(&rel.to_path(&game.root)).ok()
}

/// Size and digest of a local zip against the catalog entry.
pub fn verify_zip(zip: &Path, pack: &Pack) -> Result<(), String> {
    let size = fs::metadata(zip)
        .map_err(|e| format!("cannot read {}: {e}", zip.display()))?
        .len();
    if size != pack.zip_size {
        return Err(format!(
            "{} is {size} bytes, the catalog says {}",
            pack.asset, pack.zip_size
        ));
    }
    let sha =
        manifest::sha256_file(zip).map_err(|e| format!("cannot read {}: {e}", zip.display()))?;
    if !sha.eq_ignore_ascii_case(&pack.zip_sha256) {
        return Err(format!(
            "{} does not match the catalog's digest",
            pack.asset
        ));
    }
    Ok(())
}

/// Get the pack's zip: from `--packs-dir` when it is there, else by download
/// (resuming an interrupted one). The free-space check comes first.
pub fn obtain(
    game: &GameDir,
    pack: &Pack,
    sources: &Sources<'_>,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Obtained, PackError> {
    let local = sources
        .packs_dir
        .map(|dir| dir.join(&pack.asset))
        .filter(|p| p.is_file());
    let free = space::free_bytes(&game.root);

    if let Some(path) = local {
        if let Some(short_by) = space::shortfall(free, pack.installed_size) {
            return Err(PackError::NoSpace { short_by });
        }
        verify_zip(&path, pack).map_err(PackError::Rejected)?;
        return Ok(Obtained {
            path,
            downloaded: false,
            resumed_from: 0,
        });
    }

    let Some(agent) = sources.agent else {
        return Err(PackError::Unavailable {
            why: "GitHub could not be reached this run".to_string(),
            network: true,
        });
    };
    let dir = game.packs_dir();
    let have = fs::metadata(dir.join(format!("{}.part", pack.asset)))
        .map(|m| m.len())
        .unwrap_or(0);
    let need = pack.zip_size.saturating_sub(have) + pack.installed_size;
    if let Some(short_by) = space::shortfall(free, need) {
        return Err(PackError::NoSpace { short_by });
    }
    let target = Target {
        name: pack.asset.clone(),
        url: pack.url.clone(),
        size: pack.zip_size,
        sha256: Some(pack.zip_sha256.clone()),
    };
    match download::fetch(agent, &target, &dir, true, progress) {
        Ok(d) => Ok(Obtained {
            path: d.path,
            downloaded: true,
            resumed_from: d.resumed_from,
        }),
        Err(NetError::Verify(why)) => Err(PackError::Rejected(why)),
        Err(e @ NetError::Status(..)) => Err(PackError::Unavailable {
            why: e.to_string(),
            network: false,
        }),
        Err(e) => Err(PackError::Unavailable {
            why: e.to_string(),
            network: true,
        }),
    }
}

/// Install `pack` from `zip` over whatever revision `old` records.
pub fn install(
    game: &GameDir,
    session: &Session,
    pack: &Pack,
    zip: &Path,
    old: Option<&Marker>,
    fault: Fault,
) -> Result<Summary, PackError> {
    let staged = match archive::extract_pack(zip, &game.stage_dir(), pack) {
        Ok(s) => s,
        Err(e) => {
            let _ = fs::remove_dir_all(game.stage_dir());
            return Err(PackError::Rejected(e.to_string()));
        }
    };
    let plan = plan::install(
        &staged.marker,
        &pack.marker_path(),
        old,
        &size_of(game),
        &hash_of(game),
    );
    let result = apply::execute(
        game,
        session,
        &Transaction {
            label: &pack.id,
            backup_subdir: &pack.id,
            actions: &plan.actions,
            stage_root: &staged.root,
            merged: &MergedFiles::default(),
            manifest: None,
            keep_backup: Some(&plan.keep_backup),
        },
        fault,
    );
    Ok(result?)
}

/// Remove the installed pack `marker` records.
pub fn remove(
    game: &GameDir,
    session: &Session,
    marker: &Marker,
    fault: Fault,
) -> Result<Summary, PackError> {
    let Some(marker_rel) = super::catalog::marker_path(&marker.id) else {
        return Err(PackError::Rejected(format!(
            "{:?} is not a pack id",
            marker.id
        )));
    };
    let plan = plan::remove(marker, &marker_rel, &size_of(game), &hash_of(game));
    let result = apply::execute(
        game,
        session,
        &Transaction {
            label: &marker.id,
            backup_subdir: &marker.id,
            actions: &plan.actions,
            stage_root: &game.stage_dir(),
            merged: &MergedFiles::default(),
            manifest: None,
            keep_backup: Some(&plan.keep_backup),
        },
        fault,
    );
    Ok(result?)
}

/// Model names (`pl_<key>`, `mapset_<key>`) the pack just installed that
/// ALSO exist elsewhere under the same kind folder, as an `.arc` or as a
/// model folder. The DLL keys models by that name and uses the first it
/// finds; the updater only reports it.
pub fn name_collisions(game_root: &Path, pack: &Pack, marker: &Marker) -> Vec<String> {
    let Some(kind_root) = pack.root.parent() else {
        return Vec::new();
    };
    let prefix = format!("{}/", pack.root.as_str());
    let names: Vec<String> = marker
        .files
        .keys()
        .filter_map(|rel| rel.as_str().rsplit('/').next())
        .filter_map(|name| {
            name.to_ascii_lowercase()
                .strip_suffix(".arc")
                .map(str::to_string)
        })
        .filter(|stem| stem.starts_with("pl_") || stem.starts_with("mapset_"))
        .collect();
    let mut found = Vec::new();
    // Models sit at most three levels below the kind folder
    // (<Source>/<Friendly>/<model>).
    walk(
        &kind_root.to_path(game_root),
        kind_root.as_str(),
        3,
        &mut |rel, is_dir| {
            if rel.starts_with(&prefix) {
                return;
            }
            let name = rel.rsplit('/').next().unwrap_or(rel).to_ascii_lowercase();
            let stem = if is_dir {
                name.as_str()
            } else {
                match name.strip_suffix(".arc") {
                    Some(s) => s,
                    None => return,
                }
            };
            if names.iter().any(|n| n == stem) {
                found.push(rel.to_string());
            }
        },
    );
    found.sort();
    found
}

/// Visit every entry up to `depth` levels below `dir` (dot-entries skipped,
/// as the DLL's scanner does).
fn walk(dir: &Path, rel: &str, depth: usize, visit: &mut dyn FnMut(&str, bool)) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let child = format!("{rel}/{name}");
        let is_dir = entry.path().is_dir();
        visit(&child, is_dir);
        if is_dir && depth > 1 {
            walk(&entry.path(), &child, depth - 1, visit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packs::catalog::tests::test_pack;
    use crate::packs::state::parse_marker;
    use crate::packs::state::tests::{marker_json, Temp};

    fn put(root: &Path, rel: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
    }

    #[test]
    fn collisions_are_found_outside_the_pack_only() {
        let t = Temp::new();
        let pack = test_pack("dancers-mix", "Mix", &"c".repeat(64));
        let base = "data_mods/custom_models/dancers";
        let marker = parse_marker(
            &marker_json(
                "dancers-mix",
                &"c".repeat(64),
                &[
                    (&format!("{base}/Mix/Afro/pl_afro00.arc"), 1, "h"),
                    (&format!("{base}/Mix/Afro/pl_afro00_head00.arc"), 1, "h"),
                    (
                        &format!("{base}/Mix/Afro/chara_resources.rlist.txt"),
                        1,
                        "h",
                    ),
                    (&format!("{base}/Mix/Lady/pl_lady00.arc"), 1, "h"),
                ],
            ),
            "dancers-mix",
        )
        .unwrap();
        // The pack's own files, and unrelated user content: no collision.
        put(&t.0, &format!("{base}/Mix/Afro/pl_afro00.arc"));
        put(&t.0, &format!("{base}/Mine/Robot/pl_robot00.arc"));
        put(
            &t.0,
            &format!("{base}/Mine/Robot/chara_resources.rlist.txt"),
        );
        assert!(name_collisions(&t.0, &pack, &marker).is_empty());

        // Same model name elsewhere: as an arc, as a model folder (any case),
        // and in a dot-directory (which the DLL never scans).
        put(&t.0, &format!("{base}/Custom/My Afro/PL_AFRO00.arc"));
        put(&t.0, &format!("{base}/Old Afro/pl_afro00/pl_afro00.model"));
        put(&t.0, &format!("{base}/.hidden/pl_lady00.arc"));
        put(&t.0, "data_mods/custom_models/stages/X/pl_lady00.arc");
        assert_eq!(
            name_collisions(&t.0, &pack, &marker),
            [
                format!("{base}/Custom/My Afro/PL_AFRO00.arc"),
                format!("{base}/Old Afro/pl_afro00"),
            ]
        );
    }

    #[test]
    fn verify_zip_checks_size_then_digest() {
        let t = Temp::new();
        let zip = t.0.join("p.zip");
        fs::write(&zip, b"abc").unwrap();
        let mut pack = test_pack("dancers-mix", "Mix", &"c".repeat(64));
        pack.zip_size = 3;
        pack.zip_sha256 = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into();
        assert_eq!(verify_zip(&zip, &pack), Ok(()));
        pack.zip_size = 4;
        assert!(verify_zip(&zip, &pack).unwrap_err().contains("is 3 bytes"));
        pack.zip_size = 3;
        pack.zip_sha256 = "0".repeat(64);
        assert!(verify_zip(&zip, &pack).unwrap_err().contains("digest"));
        assert!(verify_zip(&t.0.join("absent.zip"), &pack).is_err());
    }

    #[test]
    fn obtain_without_any_source_is_unavailable_and_blames_the_network() {
        let t = Temp::new();
        let game = GameDir::for_test(&t.0);
        let pack = test_pack("dancers-mix", "Mix", &"c".repeat(64));
        let none = Sources {
            packs_dir: Some(&t.0),
            agent: None,
        };
        match obtain(&game, &pack, &none, &mut |_, _| {}) {
            Err(PackError::Unavailable { network: true, .. }) => {}
            other => panic!("unexpected: {:?}", other.map(|o| o.path)),
        }
    }
}
