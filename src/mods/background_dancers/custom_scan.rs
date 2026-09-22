//! Custom dancers & stages from `data_mods` — the IMPURE half: walk the ONE
//! custom-models base (`data_mods/custom_models/dancers/` and `/stages/`),
//! turn every MODEL FOLDER into a cache arc the engine can open (nobody packs
//! arcs by hand — the `LayeredFS` `arc_handler` convention: `ArcArchive`
//! into `data_mods/_cache/`, `CacheHasher` on the member paths + mtimes so
//! later boots only stat), read ready `.arc` headers and the sidecar rlists,
//! hand the listing to the pure planner (`custom_content::plan`), mount the
//! accepted arcs in `scene3d::arc_set` and log the outcome. Runs once from
//! `lifecycle::init_tables` (the enabling thread; host `std::fs` on
//! repo-relative paths — the process CWD is the game's `contents/`, the
//! `preview/badge.rs` / `assist_tick` fixed-path convention). Never touches
//! the engine; every failure is one WARN and the stock tables are untouched.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::core::anm::rlist;
use crate::core::arc as arcfile;
use crate::services::avs_layeredfs::cache_hasher::{CacheHasher, CACHE_FOLDER};
use crate::services::avs_layeredfs::mod_paths;
use crate::services::scene3d::arc_set;
use crate::{log_info, log_warn};

use super::custom_content::{
    classify_arc_name, classify_folder_name, classify_sidecar_name, folder_member_path,
    parse_text_rlist, plan, ArcFile, ArcRole, ContentKind, PackDir, Plan, SidecarList,
    StockContext,
};

/// The single base every custom dancer / stage lives under (design 2026-09-22
/// D2, maintainer: one folder, not one LayeredFS pack per character).
pub const CUSTOM_MODELS_DIR: &str = "./data_mods/custom_models";

/// Where model folders are packed for the engine (`<name>-<hash8>.arc` +
/// `.hashed` fingerprint beside it; delete the folder to force a repack).
const CACHE_SUBDIR: &str = "custom_models";

/// Bytes of an arc read for the header walk before falling back to the whole
/// file (header + cue table + string table sit at the front, well under this).
const HEADER_PREFIX: usize = 64 * 1024;

/// Discover the custom content under [`CUSTOM_MODELS_DIR`] and mount the
/// accepted arcs. Returns the plan (candidates + labels) for the caller to
/// append to the stock tables.
pub fn discover_and_mount(stock: &StockContext) -> Plan {
    discover_and_mount_in(CUSTOM_MODELS_DIR, stock)
}

/// [`discover_and_mount`] over an explicit base directory.
pub fn discover_and_mount_in(base: &str, stock: &StockContext) -> Plan {
    let mut dancer_dirs = Vec::new();
    let mut stage_dirs = Vec::new();
    let mut base_present = false;
    for kind in [ContentKind::Dancer, ContentKind::Stage] {
        let root = format!("{}/{}", base, kind.dir_name());
        if !Path::new(&root).is_dir() {
            continue;
        }
        base_present = true;
        let dirs = walk_kind_root(&root);
        match kind {
            ContentKind::Dancer => dancer_dirs.extend(dirs),
            ContentKind::Stage => stage_dirs.extend(dirs),
        }
    }
    let arcs_seen = dancer_dirs
        .iter()
        .chain(stage_dirs.iter())
        .map(|d| d.arcs.len())
        .sum::<usize>();
    let plan = plan(&dancer_dirs, &stage_dirs, stock);
    for w in &plan.warnings {
        log_warn!("BackgroundDancers: custom content -- {}", w);
    }
    for n in &plan.notes {
        log_info!("BackgroundDancers: {}", n);
    }
    for (game_rel, fs_path) in &plan.mounts {
        arc_set::mount(game_rel, fs_path);
    }
    if plan.is_empty() {
        if arcs_seen > 0 {
            log_warn!(
                "BackgroundDancers: custom content -- {} arc(s) under {}/{{dancers,stages}} but none was accepted (see the warnings above)",
                arcs_seen,
                base
            );
        } else if base_present {
            log_info!(
                "BackgroundDancers: custom content ON -- {}/{{dancers,stages}} present but empty",
                base
            );
        } else {
            log_info!(
                "BackgroundDancers: custom content ON -- no {}/dancers or /stages folder (nothing installed)",
                base
            );
        }
    } else {
        log_info!(
            "BackgroundDancers: custom content -- {} dancer(s) + {} stage(s) from {}, {} arc(s) mounted",
            plan.dancers.len(),
            plan.stages.len(),
            base,
            plan.mounts.len()
        );
    }
    plan
}

/// A directory listing split into files and subdirectories, each sorted by
/// name so the plan is deterministic; dot-entries skipped.
fn list_dir(dir: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                dirs.push(e.path());
            } else if ft.is_file() {
                files.push(e.path());
            }
        }
    }
    files.sort();
    dirs.sort();
    (files, dirs)
}

fn is_model_folder(name: &str) -> bool {
    matches!(
        classify_folder_name(name),
        ArcRole::Body { .. }
            | ArcRole::Part { .. }
            | ArcRole::Stage { .. }
            | ArcRole::GoldStage { .. }
    )
}

/// The `dancers/` or `stages/` root: the root itself (flat arcs and model
/// folders) plus one [`PackDir`] per friendly-name subfolder (any subfolder
/// whose name is NOT itself a model folder), each holding that folder's
/// arcs, model folders and sidecars.
fn walk_kind_root(root: &str) -> Vec<PackDir> {
    let root_path = Path::new(root);
    let (files, dirs) = list_dir(root_path);
    let (model_dirs, friendly_dirs): (Vec<PathBuf>, Vec<PathBuf>) =
        dirs.into_iter().partition(|d| {
            d.file_name()
                .map(|n| is_model_folder(&n.to_string_lossy()))
                .unwrap_or(false)
        });
    let mut out = vec![read_pack_dir(root, None, &files, &model_dirs)];
    for sub in friendly_dirs {
        let folder = sub
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (inner_files, inner_dirs) = list_dir(&sub);
        let inner_models: Vec<PathBuf> = inner_dirs
            .into_iter()
            .filter(|d| {
                d.file_name()
                    .map(|n| is_model_folder(&n.to_string_lossy()))
                    .unwrap_or(false)
            })
            .collect();
        out.push(read_pack_dir(
            &sub.to_string_lossy(),
            Some(folder),
            &inner_files,
            &inner_models,
        ));
    }
    out
}

/// One directory's arcs (ready `.arc` files header-walked, model folders
/// packed into cache arcs) and sidecar rows.
fn read_pack_dir(
    dir: &str,
    folder: Option<String>,
    files: &[PathBuf],
    model_dirs: &[PathBuf],
) -> PackDir {
    let mut pack = PackDir {
        dir: dir.replace('\\', "/"),
        folder,
        ..Default::default()
    };
    for path in files {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let fs_path = path.to_string_lossy().replace('\\', "/");
        if let Some((list, text)) = classify_sidecar_name(&name) {
            let rows = read_sidecar(&fs_path, text);
            match list {
                SidecarList::Chara => pack.chara_rows.extend(rows),
                SidecarList::Map => pack.map_rows.extend(rows),
                SidecarList::Camera => pack.camera_rows.extend(rows),
            }
            continue;
        }
        match classify_arc_name(&name) {
            ArcRole::Body { .. } | ArcRole::Part { .. } | ArcRole::Stage { .. } => {
                pack.arcs.push(ArcFile {
                    name,
                    members: read_arc_members(&fs_path),
                    source: fs_path.clone(),
                    path: fs_path,
                });
            }
            // `_g` variants and the shadow quad are never candidates; other
            // files in the folder are the author's business.
            ArcRole::GoldStage { .. } | ArcRole::Shadow | ArcRole::Other => {}
        }
    }
    for dir in model_dirs {
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let role = classify_folder_name(&name);
        if !matches!(
            role,
            ArcRole::Body { .. } | ArcRole::Part { .. } | ArcRole::Stage { .. }
        ) {
            continue; // `mapset_*_g` folders: never loaded, like the `_g` arcs
        }
        if let Some(arc) = pack_model_folder(dir, &name, &role) {
            pack.arcs.push(arc);
        }
    }
    pack
}

/// Every regular file under `dir` (recursive, dot-entries skipped) as
/// `(folder-relative path with '/', filesystem path)`, sorted.
fn walk_files(dir: &Path) -> Vec<(String, PathBuf)> {
    fn rec(base: &Path, cur: &Path, out: &mut Vec<(String, PathBuf)>) {
        let (files, dirs) = list_dir(cur);
        for f in files {
            if let Ok(rel) = f.strip_prefix(base) {
                out.push((rel.to_string_lossy().replace('\\', "/"), f.clone()));
            }
        }
        for d in dirs {
            rec(base, &d, out);
        }
    }
    let mut out = Vec::new();
    rec(dir, dir, &mut out);
    out.sort();
    out
}

/// FNV-1a 64 over a string — the cache-file name discriminator (two model
/// folders with the same name in different friendly folders must not share
/// a cache file; the planner still refuses the duplicate key).
fn fnv1a64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Pack a model folder into its cache arc (or reuse the up-to-date one) and
/// describe it as an [`ArcFile`] whose members come from the folder walk.
/// `None` (+ WARN) when the folder is empty or the cache cannot be written.
fn pack_model_folder(dir: &Path, name: &str, role: &ArcRole) -> Option<ArcFile> {
    let source = dir.to_string_lossy().replace('\\', "/");
    let files = walk_files(dir);
    if files.is_empty() {
        log_warn!(
            "BackgroundDancers: custom content -- {}: model folder is empty -- skipped",
            source
        );
        return None;
    }
    let members: Vec<(String, PathBuf)> = files
        .into_iter()
        .map(|(rel, path)| (folder_member_path(role, name, &rel), path))
        .collect();
    let out = format!(
        "{}/{}/{}-{:08x}.arc",
        CACHE_FOLDER,
        CACHE_SUBDIR,
        name,
        (fnv1a64(&source) & 0xFFFF_FFFF) as u32
    );
    let out_hashed = format!("{out}.hashed");
    // Fingerprint: every member path + the source file's path and mtime (the
    // hasher folds mtimes itself) — a rename, an added/removed file or an
    // edit all invalidate; nothing is read unless the cache is stale.
    let mut hasher = CacheHasher::new(&out_hashed);
    hasher.add_str(&source);
    for (member, path) in &members {
        hasher.add_str(member);
        hasher.add(&path.to_string_lossy());
    }
    hasher.finish();
    let member_names: Vec<String> = members.iter().map(|(m, _)| m.clone()).collect();
    if hasher.matches() && Path::new(&out).is_file() {
        return Some(ArcFile {
            name: format!("{name}.arc"),
            path: out,
            source,
            members: Some(member_names),
        });
    }
    if let Some(parent) = out.rsplit_once('/').map(|(p, _)| p) {
        if !mod_paths::mkdir_p(parent) {
            log_warn!(
                "BackgroundDancers: custom content -- cannot create {} -- {} skipped",
                parent,
                source
            );
            return None;
        }
    }
    let mut arc = arcfile::ArcArchive::empty();
    let mut total = 0usize;
    for (member, path) in &members {
        match fs::read(path) {
            Ok(bytes) => {
                total += bytes.len();
                arc.add_or_replace(member.clone(), bytes);
            }
            Err(e) => {
                log_warn!(
                    "BackgroundDancers: custom content -- {}: cannot read {} ({}) -- folder skipped",
                    source,
                    path.to_string_lossy(),
                    e
                );
                return None;
            }
        }
    }
    if !arc.save(&out) {
        log_warn!(
            "BackgroundDancers: custom content -- cannot write {} -- {} skipped",
            out,
            source
        );
        return None;
    }
    hasher.commit();
    log_info!(
        "BackgroundDancers: custom content -- packed {} ({} file(s), {} KiB) into {}",
        source,
        members.len(),
        total / 1024,
        out
    );
    Some(ArcFile {
        name: format!("{name}.arc"),
        path: out,
        source,
        members: Some(member_names),
    })
}

/// Sidecar rows: binary MRL0 through the shared codec, the `.rlist.txt`
/// twin through the text grammar. Unreadable ⇒ one WARN + no rows.
fn read_sidecar(path: &str, text: bool) -> Vec<rlist::Row> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            log_warn!(
                "BackgroundDancers: custom content -- {}: unreadable ({}) -- sidecar ignored",
                path,
                e
            );
            return Vec::new();
        }
    };
    if text {
        return parse_text_rlist(&String::from_utf8_lossy(&bytes));
    }
    match rlist::parse(&bytes) {
        Ok(rows) => rows,
        Err(e) => {
            log_warn!(
                "BackgroundDancers: custom content -- {}: not an MRL0 rlist ({:?}) -- sidecar ignored",
                path,
                e
            );
            Vec::new()
        }
    }
}

/// The member paths of an arc from its header (a bounded prefix read, the
/// whole file only when the string table runs past it). `None` for anything
/// that is not a parseable arc.
fn read_arc_members(path: &str) -> Option<Vec<String>> {
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len() as usize;
    let want = len.min(HEADER_PREFIX);
    let mut buf = vec![0u8; want];
    file.read_exact(&mut buf).ok()?;
    if let Some(entries) = header_fits(&buf) {
        return Some(entries);
    }
    // Not an arc at all ⇒ done (quietly: the planner WARNs per file).
    if buf.len() >= 4 && u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) != ARC_MAGIC {
        return None;
    }
    if len > want {
        let rest = fs::read(path).ok()?;
        if let Some(entries) = arcfile::parse(&rest) {
            return Some(entries.into_iter().map(|e| e.path).collect());
        }
    }
    None
}

/// Konami ARC magic (`core::arc::ARC_MAGIC`, private there).
const ARC_MAGIC: u32 = 0x1975_1120;

/// Parse the header from a prefix WITHOUT tripping `arc::parse`'s corrupt-arc
/// WARN when the string table lies past the prefix: the magic and the cue
/// table's string offsets are checked here first. `None` = not an arc, or
/// the prefix is too short (the caller then reads the whole file).
fn header_fits(buf: &[u8]) -> Option<Vec<String>> {
    if buf.len() < 16 {
        return None;
    }
    if u32::from_le_bytes(buf[0..4].try_into().ok()?) != ARC_MAGIC {
        return None;
    }
    let count = u32::from_le_bytes(buf[8..12].try_into().ok()?) as usize;
    let cue_end = 16usize.checked_add(count.checked_mul(16)?)?;
    if cue_end > buf.len() {
        return None;
    }
    for i in 0..count {
        let off = 16 + i * 16;
        let path_offset = u32::from_le_bytes(buf[off..off + 4].try_into().ok()?) as usize;
        // The string must start and end (NUL) inside the prefix.
        if path_offset >= buf.len() || !buf[path_offset..].contains(&0) {
            return None;
        }
    }
    arcfile::parse(buf).map(|entries| entries.into_iter().map(|e| e.path).collect())
}
