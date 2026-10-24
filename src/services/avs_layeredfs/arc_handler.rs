//! ARC mod handler — overlays mod files onto game `.arc` archives.
//!
//! Mirrors upstream ifs_layeredfs's `handle_arc`: when an `.arc` is opened,
//! look for a sibling `<arc with .arc → _arc>` mod folder. Walk it for:
//!   1. Plain files (overlays) — repack into a cached copy of the arc.
//!   2. `*_ifs` subdirectories (inner-IFS mods) — register the basename with
//!      the demangler so the game's later ramfs mount of the extracted inner
//!      ifs maps back to the arc-qualified mod path our LayeredFS lookups use.
//!
//! Member transforms: a registered `(arc, member) -> fn(&[u8]) -> Option<Vec<u8>>`
//! rewrites one member of the repacked arc AFTER overlays (so an operator's
//! full-file overlay keeps working underneath, e.g. an omnimix `musicdb.xml`
//! beneath StepMania-injected entries). An arc with a registered transform is
//! repacked even when no mod folder overlays it. Pre-repack hooks run first,
//! inside the game's open of that arc (the StepMania XML-arena patch rides
//! the `startup.arc` open this way).
//!
//! The cached arc is written uncompressed (compression=0, 64-byte aligned).
//! Cache invalidation uses the shared `CacheHasher` against the original arc's
//! AVS size+mtime, every overlay file path+mtime, and every registered
//! fingerprint string for the arc (e.g. an injected-song index digest).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::core::arc::ArcArchive;
use crate::{log_info, log_warn};

use super::cache_hasher::{CacheHasher, CACHE_FOLDER};
use super::file_hooks;
use super::mod_paths;
use super::ramfs_demangler;
use super::shader_synthesis;
use super::xml_merger;
use std::sync::Mutex;

// ── Member transforms, fingerprints, pre-repack hooks ─────────────────

/// Rewrites one arc member's bytes (post-overlay). `None` = leave unchanged.
pub type MemberTransform = fn(&[u8]) -> Option<Vec<u8>>;
/// Extra cache-key material for an arc (e.g. an index digest). `None` = no
/// contribution this boot.
pub type ArcFingerprint = fn() -> Option<String>;
/// Runs inside the game's open of the arc, before cache check and repack.
pub type PreRepackHook = fn();

struct MemberTransformEntry {
    arc_norm_path: String,
    member_path: String,
    transform: MemberTransform,
}

static MEMBER_TRANSFORMS: Mutex<Vec<MemberTransformEntry>> = Mutex::new(Vec::new());
static ARC_FINGERPRINTS: Mutex<Vec<(String, ArcFingerprint)>> = Mutex::new(Vec::new());
static PRE_REPACK_HOOKS: Mutex<Vec<(String, PreRepackHook)>> = Mutex::new(Vec::new());

/// Set once the game's open of `arc/startup.arc` has passed through
/// `handle_arc`. The music DB is parsed out of that arc, so anything that
/// injects music-DB content (member transforms) can only have landed if this
/// is true by the time the game analyses its songs.
static STARTUP_ARC_SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether LayeredFS has seen the game's `startup.arc` open this boot.
pub fn startup_arc_seen() -> bool {
    STARTUP_ARC_SEEN.load(std::sync::atomic::Ordering::Acquire)
}

/// Register a member transform for `arc_norm_path` (e.g. `arc/startup.arc`)
/// / `member_path` (e.g. `data/gamedata/musicdb.xml`). Safe before init.
pub fn register_member_transform(
    arc_norm_path: &str,
    member_path: &str,
    transform: MemberTransform,
) {
    if let Ok(mut t) = MEMBER_TRANSFORMS.lock() {
        t.push(MemberTransformEntry {
            arc_norm_path: arc_norm_path.to_ascii_lowercase(),
            member_path: member_path.to_string(),
            transform,
        });
    }
}

/// Register extra cache-key material for `arc_norm_path`.
pub fn register_arc_fingerprint(arc_norm_path: &str, fingerprint: ArcFingerprint) {
    if let Ok(mut f) = ARC_FINGERPRINTS.lock() {
        f.push((arc_norm_path.to_ascii_lowercase(), fingerprint));
    }
}

/// Register a hook that runs inside the game's open of `arc_norm_path`.
pub fn register_pre_repack_hook(arc_norm_path: &str, hook: PreRepackHook) {
    if let Ok(mut h) = PRE_REPACK_HOOKS.lock() {
        h.push((arc_norm_path.to_ascii_lowercase(), hook));
    }
}

fn transforms_for(arc_lower: &str) -> Vec<(String, MemberTransform)> {
    MEMBER_TRANSFORMS
        .lock()
        .map(|t| {
            t.iter()
                .filter(|e| e.arc_norm_path == arc_lower)
                .map(|e| (e.member_path.clone(), e.transform))
                .collect()
        })
        .unwrap_or_default()
}

fn fingerprints_for(arc_lower: &str) -> Vec<String> {
    ARC_FINGERPRINTS
        .lock()
        .map(|f| {
            f.iter()
                .filter(|(a, _)| a == arc_lower)
                .filter_map(|(_, fp)| fp())
                .collect()
        })
        .unwrap_or_default()
}

fn run_pre_repack_hooks(arc_lower: &str) {
    let hooks: Vec<PreRepackHook> = PRE_REPACK_HOOKS
        .lock()
        .map(|h| {
            h.iter()
                .filter(|(a, _)| a == arc_lower)
                .map(|(_, hook)| *hook)
                .collect()
        })
        .unwrap_or_default();
    for hook in hooks {
        hook();
    }
}

/// Result of scanning a mod's `*_arc` folder.
struct ArcModScan {
    /// Per-entry overrides. Keyed by relative path within the arc (e.g.
    /// "shader/foo.bin"); value is the absolute filesystem path of the mod
    /// file to read at pack time.
    files: BTreeMap<String, String>,
    /// Inner-ifs paths inside the arc, derived from `*_ifs` mod subdirs.
    /// Paths are arc-relative with the dot restored: a mod dir
    /// `sub/inner_ifs/` yields `sub/inner.ifs`. Stored in a set so multiple
    /// mods declaring the same inner ifs dedupe.
    inner_ifs_paths: BTreeSet<String>,
}

impl ArcModScan {
    fn is_empty(&self) -> bool {
        self.files.is_empty() && self.inner_ifs_paths.is_empty()
    }
}

/// Try to apply ARC mods for `norm_path` (a normalized arc path, e.g.
/// "arc/bm2d/foo.arc"). Returns the cache path to redirect the open to,
/// or None if there are no mods touching this arc.
///
/// `original_path` is the path the caller would have opened (post-mangling)
/// — used to load the original arc bytes through AVS when overlays exist.
pub fn handle_arc(norm_path: &str, original_path: &str) -> Option<String> {
    let arc_mod_path = norm_path.replace(".arc", "_arc");
    let arc_lower = norm_path.to_ascii_lowercase();
    if arc_lower == "arc/startup.arc" {
        STARTUP_ARC_SEEN.store(true, std::sync::atomic::Ordering::Release);
    }
    run_pre_repack_hooks(&arc_lower);
    let transforms = transforms_for(&arc_lower);

    // Runtime-synthesized shader containers (extended gs_screencommand_*
    // GSPW files built from the stock arc + committed mod blobs) ride the
    // same repack path as regular overlay files.
    let synth_entries = if norm_path == "arc/shader.arc" {
        shader_synthesis::synthesize(original_path)
    } else {
        Vec::new()
    };

    // No mod folder for this arc, nothing synthesized and no member transform
    // → nothing to do.
    let has_mod_folder = mod_paths::find_first_modfolder(&arc_mod_path).is_some();
    if !has_mod_folder && synth_entries.is_empty() && transforms.is_empty() {
        return None;
    }

    let mut scan = if has_mod_folder {
        scan_arc_mod_folder(&arc_mod_path)
    } else {
        ArcModScan {
            files: BTreeMap::new(),
            inner_ifs_paths: BTreeSet::new(),
        }
    };
    // Synthesized entries join the overlay set; an operator's explicit
    // overlay of the same entry name wins over synthesis.
    for e in synth_entries {
        scan.files.entry(e.entry_name).or_insert(e.file_path);
    }
    if scan.is_empty() && transforms.is_empty() {
        log_info!(
            "LayeredFS: arc: mod folder for '{}' has no files, skipping",
            arc_mod_path
        );
        return None;
    }

    // Register inner-IFS basenames so the demangler can resolve mounts that
    // happen after the game extracts an inner ifs from the arc. Do this even
    // if there are no top-level overlay files — the game still needs to see
    // the inner-IFS modifications.
    for inner_rel in &scan.inner_ifs_paths {
        let basename = inner_rel
            .rfind('/')
            .map(|p| &inner_rel[p + 1..])
            .unwrap_or(inner_rel.as_str());
        let demangled = format!("data/{}/{}", arc_mod_path, inner_rel);
        ramfs_demangler::register_arc_inner_ifs(basename, &demangled);
    }

    // Inner-IFS-only case: nothing to repack, just let the game open the
    // original arc and let the demangler do its thing on the inner mount.
    if scan.files.is_empty() && transforms.is_empty() {
        return None;
    }

    let out = format!("{}/{}", CACHE_FOLDER, norm_path);
    let out_hashed = format!("{}.hashed", out);

    // Cache-hash inputs: the original arc's AVS size+mtime (host `metadata`
    // cannot see an AVS path — before 2026-10 a changed stock arc never
    // invalidated), every overlay file, the transform member names and every
    // registered fingerprint.
    let mut hasher = CacheHasher::new(&out_hashed);
    match file_hooks::avs_stat(original_path) {
        Some((size, mtime)) => hasher.add_str(&format!("{}:{}:{}", original_path, size, mtime)),
        None => hasher.add(original_path),
    }
    for path in scan.files.values() {
        hasher.add(path);
    }
    for (member, _) in &transforms {
        hasher.add_str(&format!("transform:{}", member));
    }
    for fp in fingerprints_for(&arc_lower) {
        hasher.add_str(&fp);
    }
    hasher.finish();

    if hasher.matches() && Path::new(&out).is_file() {
        log_info!("LayeredFS: arc cache up to date for {}", norm_path);
        return Some(out);
    }
    log_info!("LayeredFS: arc: regenerating cache for {}", norm_path);

    // Load the original arc through AVS. If it doesn't exist, build from
    // scratch — matches upstream's behavior for arc files that aren't in
    // the base game.
    let mut arc = match xml_merger::load_bytes_from_avs_path(original_path) {
        Some(bytes) => match ArcArchive::from_bytes(&bytes) {
            Some(a) => a,
            None => {
                log_warn!(
                    "LayeredFS: arc: load failed for {}, aborting modding",
                    original_path
                );
                return None;
            }
        },
        None => {
            log_info!(
                "LayeredFS: arc: no original file, creating from scratch: \"{}\"",
                norm_path
            );
            ArcArchive::empty()
        }
    };

    if let Some(parent) = out.rsplit_once('/').map(|(p, _)| p) {
        if !mod_paths::mkdir_p(parent) {
            log_warn!(
                "LayeredFS: arc: couldn't create output cache folder '{}'",
                parent
            );
            return None;
        }
    }

    for (name, path) in &scan.files {
        match std::fs::read(path) {
            Ok(data) => arc.add_or_replace(name.clone(), data),
            Err(e) => {
                log_warn!("LayeredFS: arc: couldn't read mod file '{}': {}", path, e);
            }
        }
    }

    // Member transforms run AFTER overlays: an operator's full-file overlay
    // is the input the transform rewrites.
    for (member, transform) in &transforms {
        let Some(current) = arc.files.get(member).map(|v| v.as_slice()) else {
            log_warn!(
                "LayeredFS: arc: transform target '{}' not in {} — skipped",
                member,
                norm_path
            );
            continue;
        };
        match transform(current) {
            Some(bytes) => {
                log_info!(
                    "LayeredFS: arc: transformed '{}' in {} ({} -> {} bytes)",
                    member,
                    norm_path,
                    current.len(),
                    bytes.len()
                );
                arc.add_or_replace(member.clone(), bytes);
            }
            None => log_info!(
                "LayeredFS: arc: transform for '{}' in {} made no change",
                member,
                norm_path
            ),
        }
    }

    if !arc.save(&out) {
        log_warn!("LayeredFS: arc: couldn't write output '{}'", out);
        return None;
    }

    hasher.commit();
    Some(out)
}

// ── Scanning ─────────────────────────────────────────────────────────

fn scan_arc_mod_folder(folder: &str) -> ArcModScan {
    let mut out = ArcModScan {
        files: BTreeMap::new(),
        inner_ifs_paths: BTreeSet::new(),
    };
    for mod_dir in mod_paths::available_mods() {
        let root = format!("{}/{}", mod_dir, folder);
        scan_arc_mod_onefolder(&mut out, &root, "");
    }
    out
}

fn scan_arc_mod_onefolder(out: &mut ArcModScan, folder: &str, rel_prefix: &str) {
    let entries = match std::fs::read_dir(folder) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "." || name == ".." {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        if meta.is_dir() {
            if name.to_lowercase().ends_with("_ifs") {
                // Convert inner_ifs → inner.ifs, prepend any prefix.
                let stem = &name[..name.len() - 4];
                let inner_rel = format!("{}{}.ifs", rel_prefix, stem);
                out.inner_ifs_paths.insert(inner_rel);
                continue;
            }
            let next_folder = format!("{}/{}", folder, name);
            let next_prefix = format!("{}{}/", rel_prefix, name);
            scan_arc_mod_onefolder(out, &next_folder, &next_prefix);
        } else {
            let rel = format!("{}{}", rel_prefix, name);
            // First mod wins (matches upstream's iteration over available_mods()).
            out.files
                .entry(rel)
                .or_insert_with(|| format!("{}/{}", folder, name));
        }
    }
}

// ── Diagnostic musicdb fragment (developer aid) ──────────────────────
//
// `diagnostics.musicdb_fragment` names a file of `<music>` elements that the
// `startup.arc` repack appends to `data/gamedata/musicdb.xml` just before
// `</mdb>`. It exercises the member-transform path end to end without any
// mod code and lets an operator hand-inject entries for testing. The
// StepMania pack loader registers its own transform the same way.

static MUSICDB_FRAGMENT_PATH: Mutex<String> = Mutex::new(String::new());

/// Register the diagnostic fragment transform when the config names a file.
/// Call before `avs_layeredfs::init()`.
pub fn register_diagnostic_musicdb_fragment(path: &str) {
    if path.trim().is_empty() {
        return;
    }
    if let Ok(mut p) = MUSICDB_FRAGMENT_PATH.lock() {
        *p = path.trim().to_string();
    }
    register_member_transform(
        "arc/startup.arc",
        "data/gamedata/musicdb.xml",
        musicdb_fragment_transform,
    );
    register_arc_fingerprint("arc/startup.arc", musicdb_fragment_fingerprint);
    log_info!(
        "LayeredFS: diagnostic musicdb fragment registered: {}",
        path.trim()
    );
}

fn musicdb_fragment_path() -> Option<String> {
    MUSICDB_FRAGMENT_PATH
        .lock()
        .ok()
        .map(|p| p.clone())
        .filter(|p| !p.is_empty())
}

fn musicdb_fragment_fingerprint() -> Option<String> {
    let path = musicdb_fragment_path()?;
    let meta = std::fs::metadata(&path).ok();
    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let mtime = meta
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some(format!("musicdb_fragment:{}:{}:{}", path, size, mtime))
}

/// Pure: insert `fragment` before the final `</mdb>` of `xml`.
pub fn append_musicdb_fragment(xml: &[u8], fragment: &[u8]) -> Option<Vec<u8>> {
    let needle = b"</mdb>";
    let pos = xml.windows(needle.len()).rposition(|w| w == needle)?;
    let mut out = Vec::with_capacity(xml.len() + fragment.len() + 1);
    out.extend_from_slice(&xml[..pos]);
    out.extend_from_slice(fragment);
    if !fragment.ends_with(b"\n") {
        out.push(b'\n');
    }
    out.extend_from_slice(&xml[pos..]);
    Some(out)
}

fn musicdb_fragment_transform(xml: &[u8]) -> Option<Vec<u8>> {
    let path = musicdb_fragment_path()?;
    let fragment = match std::fs::read(&path) {
        Ok(f) => f,
        Err(e) => {
            log_warn!("LayeredFS: musicdb fragment '{}' unreadable: {}", path, e);
            return None;
        }
    };
    let out = append_musicdb_fragment(xml, &fragment);
    if out.is_none() {
        log_warn!("LayeredFS: musicdb.xml has no </mdb> — fragment not applied");
    }
    out
}
