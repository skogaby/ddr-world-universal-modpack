//! Custom dancers & stages from `data_mods` — the IMPURE half: walk the ONE
//! custom-models base (`data_mods/custom_models/dancers/` and `/stages/`) —
//! three levels: `<kind>/<Source>/<Friendly>/<model>` with the source and
//! friendly levels each optional (`sources::dir_role`; design 2026-09-30) —
//! turn every MODEL FOLDER into a cache arc the engine can open (nobody packs
//! arcs by hand — the `LayeredFS` `arc_handler` convention: `ArcArchive`
//! into `data_mods/_cache/`, a `CacheHasher` fingerprint over the member
//! paths + mtimes), read ready `.arc` headers and the sidecar rlists, hand
//! the listing to the pure planner (`custom_content::plan`), mount the
//! accepted arcs in `scene3d::arc_set` and log the outcome. Never touches the
//! engine; every failure is one WARN and the stock tables are untouched.
//!
//! ## Cost model (2026-10-04 — the content must be free to grow)
//!
//! Measured under CrossOver on the maintainer's install (542 models, 2,217
//! directories, 19,993 files): a directory enumeration ≈ 0.43 ms, but a
//! per-file `fs::metadata` ≈ 0.40 ms and every small open + read ≈ 0.36 ms —
//! the old scan stat'ed every input file (8.1 s) and read one `.hashed`
//! sidecar + stat'ed one cache arc per model (0.4 s), on the init thread,
//! ahead of every later mod and the splash. Now a warm boot does nothing per
//! FILE:
//!
//! - every directory is listed exactly ONCE, and file mtimes/sizes come out of
//!   that listing (`DirEntry::metadata` is free on Windows: FindNextFile
//!   already returned them) — the fingerprint is byte-identical to the old
//!   per-file-stat one, so existing cache arcs stay valid;
//! - the cache directory is listed once and `scan.idx` (`scan_index.rs`)
//!   remembers each packed folder's fingerprint + arc size, each ready arc's
//!   member list and each sidecar's rows, keyed by the listing stamps — no
//!   `.hashed` read, no `is_file`, no header read, no sidecar read while the
//!   stamps hold (a missing index entry falls back to the `.hashed` sidecar,
//!   which is still written for older DLLs);
//! - cache arcs no folder references any more (renamed / removed models) are
//!   pruned, so the cache cannot outgrow the content.
//!
//! What remains is linear in DIRECTORIES (≈ 0.43 ms each): a model shipped as
//! a ready `.arc` (`scripts/pack_custom_models.py`) costs one entry in its
//! friendly folder's listing instead of its whole folder tree. And the walk
//! itself is taken off the init thread: [`prefetch`] (called by `lib.rs`
//! right after `early_apply`, i.e. after every boot race) runs the listing on
//! its own thread while the services and the other mods initialise;
//! [`discover_and_mount`] joins it (or walks synchronously when nothing was
//! prefetched — a live enable from the menu).
//!
//! Host `std::fs` on repo-relative paths (the process CWD is the game's
//! `contents/`, the `preview/badge.rs` / `assist_tick` fixed-path
//! convention); the prefetch thread runs nothing but this file's std code.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

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
use super::scan_index::{self, Cached, Packed, ScanIndex, Stamp};
use super::sources::{dir_role, has_model_content, is_model_folder_name, DirRole};

/// The single base every custom dancer / stage lives under (design 2026-09-22
/// D2, maintainer: one folder, not one LayeredFS pack per character).
pub const CUSTOM_MODELS_DIR: &str = "./data_mods/custom_models";

/// Where model folders are packed for the engine (`<name>-<hash8>.arc` +
/// `.hashed` fingerprint beside it; delete the folder to force a repack).
const CACHE_SUBDIR: &str = "custom_models";

/// The scan index inside the cache directory (`scan_index.rs`).
const INDEX_FILE: &str = "scan.idx";

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
    let listing = take_prefetched(base).unwrap_or_else(|| list_content_contained(base));
    let arcs_seen = listing
        .dancer_dirs
        .iter()
        .chain(listing.stage_dirs.iter())
        .map(|d| d.arcs.len())
        .sum::<usize>();
    remember_members(&listing);
    let plan = plan(&listing.dancer_dirs, &listing.stage_dirs, stock);
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
        } else if listing.base_present {
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
        let sources: Vec<String> = plan
            .source_counts()
            .iter()
            .map(|(s, n)| format!("{} {}", s.label, n))
            .collect();
        log_info!(
            "BackgroundDancers: custom content -- {} dancer(s) + {} stage(s) from {} in {} source(s): {}; {} arc(s) mounted",
            plan.dancers.len(),
            plan.stages.len(),
            base,
            sources.len(),
            sources.join(", "),
            plan.mounts.len()
        );
    }
    plan
}

// ---------------------------------------------------------------------------
// Prefetch
// ---------------------------------------------------------------------------

/// The listing half of discovery — everything the planner consumes, built
/// without the stock tables (so it can run ahead of them).
pub struct ContentListing {
    /// The base it was taken from (a prefetch is only used for the same base).
    base: String,
    dancer_dirs: Vec<PackDir>,
    stage_dirs: Vec<PackDir>,
    base_present: bool,
}

static PREFETCH: Mutex<Option<JoinHandle<ContentListing>>> = Mutex::new(None);

/// Start listing [`CUSTOM_MODELS_DIR`] on a background thread. `lib.rs` calls
/// this right after `early_apply` (after every boot race — the walk competes
/// for wineserver time, and nothing on the race path may scale with the
/// content) when Background Dancers and its `custom_content` toggle are on in
/// the config; the mod's enable joins it. Idempotent; a spawn failure just
/// leaves the enable to walk synchronously.
pub fn prefetch() {
    let Ok(mut slot) = PREFETCH.lock() else {
        return;
    };
    if slot.is_some() {
        return;
    }
    match std::thread::Builder::new()
        .name("bd-custom-scan".into())
        .spawn(|| list_content(CUSTOM_MODELS_DIR))
    {
        Ok(handle) => *slot = Some(handle),
        Err(e) => log_warn!(
            "BackgroundDancers: custom content prefetch thread not started ({}) -- the enable scans inline",
            e
        ),
    }
}

/// The prefetched listing for `base`, joining the thread if it is still
/// running. `None` when nothing was prefetched (or for another base). A
/// prefetch that PANICKED (logged by the panic hook) yields an EMPTY listing:
/// re-running the same walk inline would only repeat the panic on the init
/// thread, so the custom content is skipped for this boot instead.
fn take_prefetched(base: &str) -> Option<ContentListing> {
    let handle = PREFETCH.lock().ok()?.take()?;
    let waited = Instant::now();
    let finished = handle.is_finished();
    match handle.join() {
        Ok(listing) if listing.base == base => {
            if !finished {
                log_info!(
                    "BackgroundDancers: custom content -- waited {} ms for the prefetched scan",
                    waited.elapsed().as_millis()
                );
            }
            Some(listing)
        }
        Ok(_) => None,
        Err(_) => {
            log_warn!(
                "BackgroundDancers: custom content prefetch thread panicked -- custom dancers/stages skipped this boot"
            );
            Some(ContentListing::empty(base))
        }
    }
}

/// [`list_content`] with a panic contained (the inline path runs on the init
/// thread, which must survive whatever the content holds): a panic ⇒ WARN +
/// an empty listing.
fn list_content_contained(base: &str) -> ContentListing {
    std::panic::catch_unwind(|| list_content(base)).unwrap_or_else(|_| {
        log_warn!(
            "BackgroundDancers: custom content scan panicked -- custom dancers/stages skipped this boot"
        );
        ContentListing::empty(base)
    })
}

impl ContentListing {
    fn empty(base: &str) -> Self {
        ContentListing {
            base: base.to_string(),
            dancer_dirs: Vec::new(),
            stage_dirs: Vec::new(),
            base_present: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Arc members for later consumers
// ---------------------------------------------------------------------------

/// Member lists of every arc the last discovery saw, by filesystem path —
/// `lifecycle::scan_screen_stages` asks for the custom stages' members right
/// after discovery instead of re-reading their headers. Dropped by
/// [`release_known_members`].
static KNOWN_MEMBERS: Mutex<Option<HashMap<String, Vec<String>>>> = Mutex::new(None);

fn remember_members(listing: &ContentListing) {
    let map: HashMap<String, Vec<String>> = listing
        .dancer_dirs
        .iter()
        .chain(listing.stage_dirs.iter())
        .flat_map(|d| d.arcs.iter())
        .filter_map(|a| a.members.clone().map(|m| (a.path.clone(), m)))
        .collect();
    if let Ok(mut k) = KNOWN_MEMBERS.lock() {
        *k = Some(map);
    }
}

/// The member list discovery recorded for the arc at `path`, if any.
pub(super) fn known_members(path: &str) -> Option<Vec<String>> {
    KNOWN_MEMBERS.lock().ok()?.as_ref()?.get(path).cloned()
}

/// Free the [`known_members`] map (the tables are built).
pub(super) fn release_known_members() {
    if let Ok(mut k) = KNOWN_MEMBERS.lock() {
        *k = None;
    }
}

// ---------------------------------------------------------------------------
// The listing
// ---------------------------------------------------------------------------

/// One regular file of a directory listing, with the stamp the listing
/// already carries.
struct Listed {
    path: PathBuf,
    name: String,
    modified: Option<SystemTime>,
    stamp: Stamp,
}

/// One directory listing: files and subdirectories, each sorted by name so the
/// plan is deterministic; dot-entries skipped.
struct Listing {
    files: Vec<Listed>,
    dirs: Vec<PathBuf>,
}

/// Mutable state of one scan: the index as loaded, the index as rebuilt (only
/// what this scan saw — pruning is implicit), the cache directory listing and
/// counters for the summary line.
struct Scan {
    cache_dir: String,
    old: ScanIndex,
    new: ScanIndex,
    /// Cache directory listing: file name → size.
    cached_files: HashMap<String, u64>,
    /// Cache file names this scan references (arcs + their `.hashed`).
    referenced: HashSet<String>,
    dirs_listed: usize,
    folders: usize,
    repacked: usize,
    migrated: usize,
    arcs: usize,
    headers_read: usize,
    sidecars: usize,
    sidecars_read: usize,
}

fn stamp_of(meta: &fs::Metadata) -> (Option<SystemTime>, Stamp) {
    let modified = meta.modified().ok();
    let mtime_ns = modified
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    (
        modified,
        Stamp {
            size: meta.len(),
            mtime_ns,
        },
    )
}

impl Scan {
    fn list_dir(&mut self, dir: &Path) -> Listing {
        self.dirs_listed += 1;
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
                    // Free on Windows (the FindNextFile record); one lstat on
                    // a Unix host.
                    let (modified, stamp) = match e.metadata() {
                        Ok(m) => stamp_of(&m),
                        Err(_) => (None, Stamp::default()),
                    };
                    files.push(Listed {
                        path: e.path(),
                        name,
                        modified,
                        stamp,
                    });
                }
            }
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        dirs.sort();
        Listing { files, dirs }
    }
}

/// Walk `base` (both kinds), packing stale model folders, and persist the
/// scan index. Thread-agnostic (std only).
fn list_content(base: &str) -> ContentListing {
    let started = Instant::now();
    let cache_dir = format!("{}/{}", CACHE_FOLDER, CACHE_SUBDIR);
    let mut scan = Scan {
        old: load_index(&cache_dir),
        new: ScanIndex::default(),
        cached_files: HashMap::new(),
        referenced: HashSet::new(),
        cache_dir,
        dirs_listed: 0,
        folders: 0,
        repacked: 0,
        migrated: 0,
        arcs: 0,
        headers_read: 0,
        sidecars: 0,
        sidecars_read: 0,
    };
    let cache_dir_path = PathBuf::from(&scan.cache_dir);
    let cache_listing = scan.list_dir(&cache_dir_path);
    scan.cached_files = cache_listing
        .files
        .into_iter()
        .map(|f| (f.name, f.stamp.size))
        .collect();

    let mut dancer_dirs = Vec::new();
    let mut stage_dirs = Vec::new();
    let mut base_present = false;
    for kind in [ContentKind::Dancer, ContentKind::Stage] {
        let root = format!("{}/{}", base, kind.dir_name());
        if !Path::new(&root).is_dir() {
            continue;
        }
        base_present = true;
        let dirs = walk_kind_root(&root, &mut scan);
        match kind {
            ContentKind::Dancer => dancer_dirs.extend(dirs),
            ContentKind::Stage => stage_dirs.extend(dirs),
        }
    }

    // Persist the index only when something it records changed; prune cache
    // files nothing references (only for the real base — another base must
    // not delete the real content's cache).
    let index_written = if scan.new != scan.old {
        save_index(&scan.cache_dir, &scan.new)
    } else {
        false
    };
    let (pruned, pruned_bytes) = if base == CUSTOM_MODELS_DIR && base_present {
        prune_orphans(&scan)
    } else {
        (0, 0)
    };
    log_info!(
        "BackgroundDancers: custom content scan -- {} ms: {} dir(s) listed; {} model folder(s) ({} repacked, {} validated via .hashed); {} ready arc(s) ({} header(s) read); {} sidecar(s) ({} read); index {}{}",
        started.elapsed().as_millis(),
        scan.dirs_listed,
        scan.folders,
        scan.repacked,
        scan.migrated,
        scan.arcs,
        scan.headers_read,
        scan.sidecars,
        scan.sidecars_read,
        if index_written { "rewritten" } else { "unchanged" },
        if pruned > 0 {
            format!(
                "; pruned {} orphaned cache file(s) ({} MiB)",
                pruned,
                pruned_bytes / (1024 * 1024)
            )
        } else {
            String::new()
        }
    );
    ContentListing {
        base: base.to_string(),
        dancer_dirs,
        stage_dirs,
        base_present,
    }
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn names(paths: &[PathBuf]) -> Vec<String> {
    paths.iter().map(|p| file_name(p)).collect()
}

fn file_names(files: &[Listed]) -> Vec<String> {
    files.iter().map(|f| f.name.clone()).collect()
}

/// Split a listing's directories into model folders and the rest.
fn partition_models(dirs: Vec<PathBuf>) -> (Vec<PathBuf>, Vec<PathBuf>) {
    dirs.into_iter()
        .partition(|d| is_model_folder_name(&file_name(d)))
}

/// The `dancers/` or `stages/` root, three levels deep: the root itself (flat
/// arcs and model folders ⇒ the implicit CUSTOM source), then every non-model
/// subdirectory classified by `sources::dir_role` — a SOURCE folder (it holds
/// at least one friendly folder) yields one [`PackDir`] for its own flat
/// content plus one per friendly folder inside it, all tagged with the
/// source; a FRIENDLY folder (models directly inside — the pre-2026-09-30
/// layout) yields one untagged [`PackDir`]; anything else is ignored. Every
/// directory is listed exactly once: a child's listing decides its role AND
/// feeds its `PackDir` — deterministic from names alone.
fn walk_kind_root(root: &str, scan: &mut Scan) -> Vec<PackDir> {
    let top = scan.list_dir(Path::new(root));
    let (model_dirs, other_dirs) = partition_models(top.dirs);
    let mut out = vec![read_pack_dir(
        root,
        None,
        None,
        &top.files,
        &model_dirs,
        scan,
    )];
    for sub in other_dirs {
        let name = file_name(&sub);
        let sub_listing = scan.list_dir(&sub);
        let (sub_models, sub_others) = partition_models(sub_listing.dirs);
        // A friendly child is a non-model directory with model content.
        let mut friendly_children: Vec<(PathBuf, Listing)> = Vec::new();
        for child in sub_others {
            let listing = scan.list_dir(&child);
            if has_model_content(&file_names(&listing.files), &names(&listing.dirs)) {
                friendly_children.push((child, listing));
            }
        }
        let own_content = has_model_content(&file_names(&sub_listing.files), &names(&sub_models));
        match dir_role(false, !friendly_children.is_empty(), own_content) {
            DirRole::Source => {
                let source = Some(name.clone());
                out.push(read_pack_dir(
                    &sub.to_string_lossy(),
                    None,
                    source.clone(),
                    &sub_listing.files,
                    &sub_models,
                    scan,
                ));
                for (friendly, listing) in friendly_children {
                    let (friendly_models, _) = partition_models(listing.dirs);
                    out.push(read_pack_dir(
                        &friendly.to_string_lossy(),
                        Some(file_name(&friendly)),
                        source.clone(),
                        &listing.files,
                        &friendly_models,
                        scan,
                    ));
                }
            }
            DirRole::Friendly => out.push(read_pack_dir(
                &sub.to_string_lossy(),
                Some(name),
                None,
                &sub_listing.files,
                &sub_models,
                scan,
            )),
            DirRole::Model | DirRole::Ignored => {}
        }
    }
    out
}

/// One directory's arcs (ready `.arc` files header-walked, model folders
/// packed into cache arcs) and sidecar rows.
fn read_pack_dir(
    dir: &str,
    folder: Option<String>,
    source: Option<String>,
    files: &[Listed],
    model_dirs: &[PathBuf],
    scan: &mut Scan,
) -> PackDir {
    let mut pack = PackDir {
        dir: dir.replace('\\', "/"),
        folder,
        source,
        ..Default::default()
    };
    for file in files {
        let fs_path = file.path.to_string_lossy().replace('\\', "/");
        if let Some((list, text)) = classify_sidecar_name(&file.name) {
            let rows = sidecar_rows(&fs_path, file.stamp, text, scan);
            match list {
                SidecarList::Chara => pack.chara_rows.extend(rows),
                SidecarList::Map => pack.map_rows.extend(rows),
                SidecarList::Camera => pack.camera_rows.extend(rows),
            }
            continue;
        }
        match classify_arc_name(&file.name) {
            ArcRole::Body { .. } | ArcRole::Part { .. } | ArcRole::Stage { .. } => {
                pack.arcs.push(ArcFile {
                    name: file.name.clone(),
                    members: ready_arc_members(&fs_path, file.stamp, scan),
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
        let name = file_name(dir);
        let role = classify_folder_name(&name);
        if !matches!(
            role,
            ArcRole::Body { .. } | ArcRole::Part { .. } | ArcRole::Stage { .. }
        ) {
            continue; // `mapset_*_g` folders: never loaded, like the `_g` arcs
        }
        if let Some(arc) = pack_model_folder(dir, &name, &role, scan) {
            pack.arcs.push(arc);
        }
    }
    pack
}

/// Every regular file under `dir` (recursive, dot-entries skipped) as
/// `(folder-relative path with '/', listed file)`, sorted.
fn walk_files(dir: &Path, scan: &mut Scan) -> Vec<(String, Listed)> {
    fn rec(base: &Path, cur: &Path, scan: &mut Scan, out: &mut Vec<(String, Listed)>) {
        let listing = scan.list_dir(cur);
        for f in listing.files {
            if let Ok(rel) = f.path.strip_prefix(base) {
                out.push((rel.to_string_lossy().replace('\\', "/"), f));
            }
        }
        for d in listing.dirs {
            rec(base, &d, scan, out);
        }
    }
    let mut out = Vec::new();
    rec(dir, dir, scan, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.path.cmp(&b.1.path)));
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
fn pack_model_folder(dir: &Path, name: &str, role: &ArcRole, scan: &mut Scan) -> Option<ArcFile> {
    scan.folders += 1;
    let source = dir.to_string_lossy().replace('\\', "/");
    let files = walk_files(dir, scan);
    if files.is_empty() {
        log_warn!(
            "BackgroundDancers: custom content -- {}: model folder is empty -- skipped",
            source
        );
        return None;
    }
    let members: Vec<(String, Listed)> = files
        .into_iter()
        .map(|(rel, file)| (folder_member_path(role, name, &rel), file))
        .collect();
    let arc_name = format!(
        "{}-{:08x}.arc",
        name,
        (fnv1a64(&source) & 0xFFFF_FFFF) as u32
    );
    let hashed_name = format!("{arc_name}.hashed");
    let out = format!("{}/{}", scan.cache_dir, arc_name);
    let out_hashed = format!("{}/{}", scan.cache_dir, hashed_name);
    scan.referenced.insert(arc_name.clone());
    scan.referenced.insert(hashed_name);
    // Fingerprint: the source path, then every member path + the input file's
    // path and mtime (from the listing — the same bytes `CacheHasher::add`
    // folds after its own stat) — a rename, an added/removed file or an edit
    // all invalidate; nothing is read unless the cache is stale.
    let mut hasher = CacheHasher::deferred(&out_hashed);
    hasher.add_str(&source);
    for (member, file) in &members {
        hasher.add_str(member);
        hasher.add_stamped(&file.path.to_string_lossy(), file.modified);
    }
    hasher.finish();
    let hash = hasher.new_hash();
    let member_names: Vec<String> = members.iter().map(|(m, _)| m.clone()).collect();
    let arc = |path: String| ArcFile {
        name: format!("{name}.arc"),
        path,
        source: source.clone(),
        members: Some(member_names.clone()),
    };

    // Fresh? The cache listing must hold the arc; the index (or, for a folder
    // the index has never seen, the `.hashed` sidecar) must vouch for it.
    if let Some(&listed_size) = scan.cached_files.get(&arc_name) {
        let fresh = match scan.old.packed_fresh(&arc_name, hash, listed_size) {
            Some(fresh) => fresh,
            None => {
                hasher.load_existing();
                scan.migrated += 1;
                hasher.matches()
            }
        };
        if fresh {
            scan.new.packed.insert(
                arc_name,
                Packed {
                    hash,
                    arc_size: listed_size,
                },
            );
            return Some(arc(out));
        }
    }

    if !mod_paths::mkdir_p(&scan.cache_dir) {
        log_warn!(
            "BackgroundDancers: custom content -- cannot create {} -- {} skipped",
            scan.cache_dir,
            source
        );
        return None;
    }
    let mut archive = arcfile::ArcArchive::empty();
    let mut total = 0usize;
    for (member, file) in &members {
        match fs::read(&file.path) {
            Ok(bytes) => {
                total += bytes.len();
                archive.add_or_replace(member.clone(), bytes);
            }
            Err(e) => {
                log_warn!(
                    "BackgroundDancers: custom content -- {}: cannot read {} ({}) -- folder skipped",
                    source,
                    file.path.to_string_lossy(),
                    e
                );
                return None;
            }
        }
    }
    let bytes = archive.to_bytes();
    drop(archive);
    if let Err(e) = fs::write(&out, &bytes) {
        log_warn!(
            "BackgroundDancers: custom content -- cannot write {} ({}) -- {} skipped",
            out,
            e,
            source
        );
        return None;
    }
    hasher.commit();
    scan.repacked += 1;
    scan.new.packed.insert(
        arc_name,
        Packed {
            hash,
            arc_size: bytes.len() as u64,
        },
    );
    log_info!(
        "BackgroundDancers: custom content -- packed {} ({} file(s), {} KiB) into {}",
        source,
        members.len(),
        total / 1024,
        out
    );
    Some(arc(out))
}

/// A ready arc's member list: from the index while its listing stamp holds,
/// else from its header (recorded for the next boot when readable).
fn ready_arc_members(path: &str, stamp: Stamp, scan: &mut Scan) -> Option<Vec<String>> {
    scan.arcs += 1;
    if let Some(members) = scan.old.arc_members(path, stamp) {
        let members = members.clone();
        scan.new.arcs.insert(
            path.to_string(),
            Cached {
                stamp,
                value: members.clone(),
            },
        );
        return Some(members);
    }
    scan.headers_read += 1;
    let members = read_arc_members(path)?;
    if stamp.mtime_ns != 0 {
        scan.new.arcs.insert(
            path.to_string(),
            Cached {
                stamp,
                value: members.clone(),
            },
        );
    }
    Some(members)
}

/// A sidecar's rows: from the index while its listing stamp holds, else read
/// and parsed (and recorded — an unreadable / unparsable sidecar is not, so
/// its WARN repeats until it is fixed).
fn sidecar_rows(path: &str, stamp: Stamp, text: bool, scan: &mut Scan) -> Vec<rlist::Row> {
    scan.sidecars += 1;
    if let Some(rows) = scan.old.sidecar_rows(path, stamp) {
        let rows = rows.clone();
        scan.new.sidecars.insert(
            path.to_string(),
            Cached {
                stamp,
                value: rows.clone(),
            },
        );
        return rows;
    }
    scan.sidecars_read += 1;
    let Some(rows) = read_sidecar(path, text) else {
        return Vec::new();
    };
    if stamp.mtime_ns != 0 {
        scan.new.sidecars.insert(
            path.to_string(),
            Cached {
                stamp,
                value: rows.clone(),
            },
        );
    }
    rows
}

/// Sidecar rows: binary MRL0 through the shared codec, the `.rlist.txt`
/// twin through the text grammar. Unreadable / unparsable ⇒ one WARN + `None`.
fn read_sidecar(path: &str, text: bool) -> Option<Vec<rlist::Row>> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            log_warn!(
                "BackgroundDancers: custom content -- {}: unreadable ({}) -- sidecar ignored",
                path,
                e
            );
            return None;
        }
    };
    if text {
        return Some(parse_text_rlist(&String::from_utf8_lossy(&bytes)));
    }
    match rlist::parse(&bytes) {
        Ok(rows) => Some(rows),
        Err(e) => {
            log_warn!(
                "BackgroundDancers: custom content -- {}: not an MRL0 rlist ({:?}) -- sidecar ignored",
                path,
                e
            );
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Index persistence + cache pruning
// ---------------------------------------------------------------------------

fn load_index(cache_dir: &str) -> ScanIndex {
    match fs::read(format!("{cache_dir}/{INDEX_FILE}")) {
        Ok(bytes) => scan_index::decode(&String::from_utf8_lossy(&bytes)),
        Err(_) => ScanIndex::default(),
    }
}

/// Write the index atomically (temp file + rename). `true` when written.
fn save_index(cache_dir: &str, index: &ScanIndex) -> bool {
    if !mod_paths::mkdir_p(cache_dir) {
        return false;
    }
    let path = format!("{cache_dir}/{INDEX_FILE}");
    let tmp = format!("{path}.tmp");
    let text = scan_index::encode(index);
    if let Err(e) = fs::write(&tmp, text.as_bytes()) {
        log_warn!(
            "BackgroundDancers: custom content -- cannot write {} ({}) -- the next boot re-validates via .hashed",
            tmp,
            e
        );
        return false;
    }
    if let Err(e) = fs::rename(&tmp, &path) {
        log_warn!(
            "BackgroundDancers: custom content -- cannot replace {} ({})",
            path,
            e
        );
        let _ = fs::remove_file(&tmp);
        return false;
    }
    true
}

/// Delete cache arcs (+ `.hashed`) under the cache directory that no model
/// folder referenced this scan — renamed or removed content. Only files of the
/// scanner's own naming (`*.arc`, `*.arc.hashed`) are touched. Returns
/// `(files, bytes)` removed.
fn prune_orphans(scan: &Scan) -> (usize, u64) {
    let mut files = 0usize;
    let mut bytes = 0u64;
    for (name, size) in &scan.cached_files {
        let lower = name.to_ascii_lowercase();
        if !(lower.ends_with(".arc") || lower.ends_with(".arc.hashed")) {
            continue;
        }
        if scan.referenced.contains(name) {
            continue;
        }
        let path = format!("{}/{}", scan.cache_dir, name);
        match fs::remove_file(&path) {
            Ok(()) => {
                files += 1;
                bytes += size;
            }
            Err(e) => log_warn!(
                "BackgroundDancers: custom content -- cannot prune orphaned {} ({})",
                path,
                e
            ),
        }
    }
    (files, bytes)
}

// ---------------------------------------------------------------------------
// Arc headers
// ---------------------------------------------------------------------------

/// The member paths of an arc from its header (a bounded prefix read, the
/// whole file only when the string table runs past it). `None` for anything
/// that is not a parseable arc.
pub(super) fn read_arc_members(path: &str) -> Option<Vec<String>> {
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
