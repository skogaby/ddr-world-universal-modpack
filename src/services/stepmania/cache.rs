//! `./data_mods/_cache/stepmania/` — layout, atomic writes and the
//! byte-budgeted LRU over the on-demand artefacts (design §5.3).
//!
//! ```text
//! index.bin                 fingerprint-guarded SongEntry table (model.rs)
//! sync.bin                  auto-sync ledger: song → {fingerprint, delta | refusal} (sync_state.rs)
//! ssq/<b>.ssq               complete, atomically replaced (one-timing songs)
//! ssq/<b>_N.ssq             per-level split files (per-chart timing songs; N = level + 1)
//! xsb/<b>.xsb
//! tn/<b>.dds                per-song thumbnail blob (re-packed into the arc)
//! thumbnail/all.arc         ONE arc for the STEPMANIA series, built by the tool (+ all.digest)
//! jacket/<b>_jk.arc         LRU (+ <b>_jk.ph marker when it holds the placeholder)
//! xwb/<b>.xwb               full bank, LRU
//! xwb/<b>.xwb.part          full bank being written progressively (ledgered under the final key)
//! xwb/<b>.p.xwb             preview variant (1-block silent main + real preview), LRU
//! xwb/_silent/<b>.xwb       silent fallback, tiny
//! lru.bin                   touch order + sizes
//! ```
//!
//! Every artefact is written to `<path>.tmp` and renamed into place, so a
//! file AT its final path is always complete — the resolver never serves a
//! half-written bank, and a crash mid-write leaves only a `.tmp` that the
//! next boot sweeps. The one deliberate exception is the progressive full
//! bank (`progressive.rs`): its `.part` is served while incomplete, with the
//! read hook holding reads at the writer's watermark; it too is swept at
//! boot. The whole directory is deletable: everything in it is
//! regenerated from the packs.
//!
//! The LRU (`model::LruTable`) covers `xwb/` + `jacket/` against
//! `audio_cache_mb`: touch on serve, evict-on-insert, persisted after each
//! change (tiny file). At boot it is reconciled with the files actually
//! present so an unclean shutdown cannot leak space.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::model::LruTable;
use crate::{log_info, log_warn};

/// Cache root, relative to the game directory like every other `_cache` path.
pub const CACHE_ROOT: &str = "./data_mods/_cache/stepmania";

#[must_use]
pub fn root() -> PathBuf {
    PathBuf::from(CACHE_ROOT)
}

#[must_use]
pub fn index_path() -> PathBuf {
    root().join("index.bin")
}

#[must_use]
pub fn lru_path() -> PathBuf {
    root().join("lru.bin")
}

/// `sync.bin` — the auto-sync ledger (`sync_state.rs`).
#[must_use]
pub fn sync_path() -> PathBuf {
    root().join("sync.bin")
}

#[must_use]
pub fn ssq_path(basename: &str) -> PathBuf {
    root().join("ssq").join(format!("{basename}.ssq"))
}

/// `ssq/<b>_N.ssq` — one per-chart-timing split file (`N` = 1..=5).
#[must_use]
pub fn ssq_split_path(basename: &str, n: u8) -> PathBuf {
    root().join("ssq").join(format!("{basename}_{n}.ssq"))
}

/// Delete `ssq/<b>.ssq` and every `ssq/<b>_N.ssq` (before a re-conversion
/// materialises a possibly different file set). Missing files are not errors.
pub fn remove_ssq_files(basename: &str) {
    let _ = fs::remove_file(ssq_path(basename));
    for n in 1..=5u8 {
        let _ = fs::remove_file(ssq_split_path(basename, n));
    }
}

#[must_use]
pub fn xsb_path(basename: &str) -> PathBuf {
    root().join("xsb").join(format!("{basename}.xsb"))
}

#[must_use]
pub fn tn_path(basename: &str) -> PathBuf {
    root().join("tn").join(format!("{basename}.dds"))
}

#[must_use]
pub fn thumbnail_arc_path() -> PathBuf {
    root().join("thumbnail").join("all.arc")
}

#[must_use]
pub fn thumbnail_digest_path() -> PathBuf {
    root().join("thumbnail").join("all.digest")
}

/// Which bank a `sound/win/dance/<b>.xwb` open gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BankVariant {
    /// Whole song as entry 0, preview window as entry 1 (gameplay).
    Full,
    /// One silent block as entry 0, preview window as entry 1 (wheel).
    Preview,
    /// One silent block in both entries (fallback).
    Silent,
}

impl BankVariant {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Preview => "preview",
            Self::Silent => "silent",
        }
    }
}

/// Cache-relative key of a bank (also the LRU key for the LRU-managed ones).
#[must_use]
pub fn xwb_key(basename: &str, variant: BankVariant) -> String {
    match variant {
        BankVariant::Full => format!("xwb/{basename}.xwb"),
        BankVariant::Preview => format!("xwb/{basename}.p.xwb"),
        BankVariant::Silent => format!("xwb/_silent/{basename}.xwb"),
    }
}

#[must_use]
pub fn xwb_path(basename: &str, variant: BankVariant) -> PathBuf {
    root().join(xwb_key(basename, variant))
}

#[must_use]
pub fn jacket_key(basename: &str) -> String {
    format!("jacket/{basename}_jk.arc")
}

#[must_use]
pub fn jacket_path(basename: &str) -> PathBuf {
    root().join(jacket_key(basename))
}

/// Marker next to a jacket arc that holds the placeholder (so a later
/// request from a worker thread upgrades it to the composed art).
#[must_use]
pub fn jacket_placeholder_marker(basename: &str) -> PathBuf {
    root().join("jacket").join(format!("{basename}_jk.ph"))
}

/// The path string the resolver hands LayeredFS (forward slashes, `./`-rooted
/// like every other cache path the open hook redirects to).
#[must_use]
pub fn to_game_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Create every cache directory. `false` when the root cannot be created.
pub fn ensure_layout() -> bool {
    let dirs = [
        "",
        "ssq",
        "xsb",
        "tn",
        "thumbnail",
        "jacket",
        "xwb",
        "xwb/_silent",
    ];
    for d in dirs {
        let p = if d.is_empty() { root() } else { root().join(d) };
        if let Err(e) = fs::create_dir_all(&p) {
            log_warn!(
                "StepMania: cannot create cache dir {}: {}",
                to_game_path(&p),
                e
            );
            return false;
        }
    }
    true
}

/// Write `bytes` to `<path>.tmp` and rename it into place. Returns `false`
/// (with one WARN) on failure; the destination is untouched then.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> bool {
    let tmp = tmp_path(path);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Err(e) = fs::write(&tmp, bytes) {
        log_warn!("StepMania: write {} failed: {}", to_game_path(&tmp), e);
        let _ = fs::remove_file(&tmp);
        return false;
    }
    commit_tmp(path)
}

/// `<path>.tmp` for a destination.
#[must_use]
pub fn tmp_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".tmp");
    PathBuf::from(s)
}

/// Rename `<path>.tmp` over `path` (Windows cannot rename over an existing
/// file, so the old one is removed first; a failed remove — the game holds
/// the file open — leaves the old complete file in place and reports false).
pub fn commit_tmp(path: &Path) -> bool {
    let tmp = tmp_path(path);
    if path.exists() {
        if let Err(e) = fs::remove_file(path) {
            log_warn!(
                "StepMania: cannot replace {} (in use?): {}",
                to_game_path(path),
                e
            );
            let _ = fs::remove_file(&tmp);
            return false;
        }
    }
    match fs::rename(&tmp, path) {
        Ok(()) => true,
        Err(e) => {
            log_warn!(
                "StepMania: rename {} -> {} failed: {}",
                to_game_path(&tmp),
                to_game_path(path),
                e
            );
            let _ = fs::remove_file(&tmp);
            false
        }
    }
}

/// Delete every stale `*.tmp` and `*.part` under the cache (an earlier boot
/// died mid-write; a progressive bank never survives a boot). Returns how
/// many were removed.
pub fn sweep_tmp_files() -> usize {
    let mut removed = 0;
    for sub in [
        "ssq",
        "xsb",
        "tn",
        "thumbnail",
        "jacket",
        "xwb",
        "xwb/_silent",
        "",
    ] {
        let dir = if sub.is_empty() {
            root()
        } else {
            root().join(sub)
        };
        let Ok(rd) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().is_some_and(|e| e == "tmp" || e == "part")
                && fs::remove_file(&p).is_ok()
            {
                removed += 1;
            }
        }
    }
    removed
}

/// Remove every per-song artefact of `basename` (used when a song vanished
/// or its identity changed). Missing files are not errors.
pub fn remove_song_artefacts(basename: &str) {
    remove_ssq_files(basename);
    let paths = [
        xsb_path(basename),
        tn_path(basename),
        xwb_path(basename, BankVariant::Full),
        xwb_path(basename, BankVariant::Preview),
        xwb_path(basename, BankVariant::Silent),
        jacket_path(basename),
        jacket_placeholder_marker(basename),
    ];
    for p in paths {
        let _ = fs::remove_file(&p);
    }
    with_lru(|lru| {
        lru.remove(&xwb_key(basename, BankVariant::Full));
        lru.remove(&xwb_key(basename, BankVariant::Preview));
        lru.remove(&jacket_key(basename));
    });
}

/// `(size, mtime secs)` of a file, or `None`.
#[must_use]
pub fn stat(path: &Path) -> Option<(u64, u64)> {
    let meta = fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some((meta.len(), mtime))
}

// ---------------------------------------------------------------------
// LRU
// ---------------------------------------------------------------------

struct LruState {
    table: LruTable,
    budget_bytes: u64,
    loaded: bool,
}

static LRU: Mutex<LruState> = Mutex::new(LruState {
    table: LruTable::new(),
    budget_bytes: 1024 * 1024 * 1024,
    loaded: false,
});

/// Load `lru.bin`, reconcile it with the files on disk and set the budget.
/// Called once from the index coordinator (before any serve can happen —
/// nothing is injected before the index is ready).
pub fn init_lru(audio_cache_mb: u32) {
    let budget = u64::from(audio_cache_mb.max(1)) * 1024 * 1024;
    let mut table = fs::read(lru_path())
        .ok()
        .and_then(|b| LruTable::parse(&b))
        .unwrap_or_default();
    let mut on_disk: HashMap<String, u64> = HashMap::new();
    let families: [(&str, fn(&str) -> bool); 2] = [
        ("xwb", |n| n.ends_with(".xwb")),
        ("jacket", |n| n.ends_with("_jk.arc")),
    ];
    for (sub, filter) in families {
        let Ok(rd) = fs::read_dir(root().join(sub)) else {
            continue;
        };
        for entry in rd.flatten() {
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if filter(&name) {
                on_disk.insert(format!("{sub}/{name}"), meta.len());
            }
        }
    }
    table.reconcile(&on_disk);
    let total = table.total_bytes();
    if let Ok(mut st) = LRU.lock() {
        st.table = table;
        st.budget_bytes = budget;
        st.loaded = true;
    }
    log_info!(
        "StepMania: audio/jacket cache {} MiB used of {} MiB budget ({} files)",
        total / (1024 * 1024),
        audio_cache_mb,
        on_disk.len()
    );
    save_lru();
}

fn with_lru<R>(f: impl FnOnce(&mut LruTable) -> R) -> Option<R> {
    let mut st = LRU.lock().ok()?;
    Some(f(&mut st.table))
}

/// Serialises `lru.bin` rewrites — worker jobs finish concurrently and
/// `write_atomic` shares one `.tmp` name.
static LRU_SAVE: Mutex<()> = Mutex::new(());

fn save_lru() {
    let bytes = match LRU.lock() {
        Ok(st) if st.loaded => st.table.serialize(),
        _ => return,
    };
    let _guard = LRU_SAVE.lock().unwrap_or_else(|p| p.into_inner());
    let _ = write_atomic(&lru_path(), &bytes);
}

/// Record a serve of an LRU-managed key.
pub fn lru_touch(key: &str) {
    let touched = with_lru(|t| t.touch(key)).unwrap_or(false);
    if touched {
        save_lru();
    }
}

/// Record a freshly written LRU-managed artefact and delete whatever the
/// budget evicts (the files named by the ledger; a file the game still holds
/// open stays and is re-adopted at the next boot's reconcile). A full bank
/// still being written progressively is ledgered under its FINAL key from
/// creation, so a victim absent at its final path is looked for at
/// `<key>.part`.
pub fn lru_insert(key: &str, bytes: u64) {
    let evicted = match LRU.lock() {
        Ok(mut st) => {
            let budget = st.budget_bytes;
            st.table.insert(key, bytes, budget)
        }
        Err(_) => Vec::new(),
    };
    for victim in &evicted {
        let p = root().join(victim);
        let mut removed = fs::remove_file(&p);
        if removed
            .as_ref()
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
        {
            let mut part = p.as_os_str().to_owned();
            part.push(".part");
            removed = fs::remove_file(PathBuf::from(part));
        }
        match removed {
            Ok(()) => {
                if victim.ends_with("_jk.arc") {
                    let marker = p.with_extension("ph");
                    let _ = fs::remove_file(marker);
                }
                log_info!("StepMania: cache evicted {}", victim);
            }
            Err(e) => log_warn!("StepMania: cache evict {} failed: {}", victim, e),
        }
    }
    save_lru();
}

/// Forget `key` (its file was abandoned by the writer).
pub fn lru_remove(key: &str) {
    let removed = with_lru(|t| t.remove(key)).unwrap_or(false);
    if removed {
        save_lru();
    }
}

/// Whether the LRU knows `key` (a complete file under management).
#[must_use]
pub fn lru_contains(key: &str) -> bool {
    with_lru(|t| t.contains(key)).unwrap_or(false)
}
