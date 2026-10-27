//! The song index as the game sees it — a READER of the cache the prep tool
//! (`prep/`, `ddr_world_hook_prep.exe`) wrote. The DLL never discovers,
//! converts or measures a song; the only way a StepMania song reaches the
//! game is through a `data_mods/_cache/stepmania/index.bin` the tool
//! produced beforehand.
//!
//! ## Boot
//! [`load`] (step 0', `early_init`) parses `index.bin` synchronously — a few
//! ms for thousands of records — and publishes every record to the lookup
//! maps; the `startup.arc` open injects from that set WITHOUT waiting (a
//! 14,000-song library costs the open nothing beyond the transform itself).
//! [`validate_async`] re-checks each record on read-only, below-normal
//! worker threads against the disk (`records.rs`: the three artefacts exist,
//! the simfile/audio/art stamps still hash to the record's fingerprint —
//! `validation.rs` decides); when the pass finishes, the records that failed
//! are removed from the maps and handed to `super::on_validation_complete`,
//! which hides them in the music DB (corruption flag → gone at the next
//! wheel rebuild) with one WARN naming the tool. The normal case — the tool
//! just ran — therefore costs the boot nothing; the stale case converges a
//! few seconds after boot instead of blocking it.
//!
//! Nothing here writes `index.bin`. A bank that fails to decode at play is
//! flagged for the SESSION only ([`mark_audio_failed_session`]); the tool's
//! measurement pass finds and records the same failures offline.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use crate::core::art;
use crate::core::art::source::SourceKind;
use crate::{log_info, log_warn};

use super::cache;
use super::convert;
use super::model::{self, SongEntry};
use super::records::{artefacts_present, song_dir_of, warm_fingerprint};
use super::validation::{self, DropTally, Verdict};

/// Largest audio file the providers read.
pub const MAX_AUDIO_BYTES: u64 = 256 * 1024 * 1024;
/// Largest art source read.
pub const MAX_ART_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Default)]
struct State {
    validated: bool,
    /// The loaded records; the failed ones are removed when validation
    /// completes.
    songs: Vec<Arc<SongEntry>>,
    /// Loaded records the validation workers have not reached yet.
    pending: Vec<Arc<SongEntry>>,
    /// Records that failed validation (removed from `songs` at completion).
    dropped: Vec<Arc<SongEntry>>,
    tally: DropTally,
    packs: usize,
    rejected: usize,
    /// Records `index.bin` held.
    loaded: usize,
}

static STATE: Mutex<State> = Mutex::new(State {
    validated: false,
    songs: Vec::new(),
    pending: Vec::new(),
    dropped: Vec::new(),
    tally: DropTally {
        artefacts: 0,
        sources: 0,
        changed: 0,
    },
    packs: 0,
    rejected: 0,
    loaded: 0,
});
static BY_BASENAME: RwLock<Option<HashMap<String, Arc<SongEntry>>>> = RwLock::new(None);
static BY_MCODE: RwLock<Option<HashMap<u32, Arc<SongEntry>>>> = RwLock::new(None);
/// Basenames whose bank build failed this session (never persisted).
static AUDIO_FAILED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// A point-in-time view of the index: every loaded record until validation
/// completes, then the records that passed.
#[derive(Clone, Debug)]
pub struct IndexSnapshot {
    pub songs: Arc<Vec<Arc<SongEntry>>>,
    /// The validation pass has finished.
    pub validated: bool,
    pub packs: usize,
    /// Rejections the tool cached (reported by the tool, counted here).
    pub rejected: usize,
    /// Records dropped by validation this boot.
    pub dropped: usize,
    /// Records `index.bin` held.
    pub loaded: usize,
}

impl IndexSnapshot {
    /// Plain entries (for the pure transform).
    #[must_use]
    pub fn entries(&self) -> Vec<SongEntry> {
        self.songs.iter().map(|s| (**s).clone()).collect()
    }
}

fn snapshot_locked(st: &State) -> IndexSnapshot {
    IndexSnapshot {
        songs: Arc::new(st.songs.clone()),
        validated: st.validated,
        packs: st.packs,
        rejected: st.rejected,
        dropped: st.tally.total(),
        loaded: st.loaded,
    }
}

/// The index as it is right now (the loaded set until validation finishes).
#[must_use]
pub fn snapshot() -> IndexSnapshot {
    match STATE.lock() {
        Ok(st) => snapshot_locked(&st),
        Err(p) => snapshot_locked(&p.into_inner()),
    }
}

#[must_use]
pub fn is_validated() -> bool {
    STATE.lock().map(|s| s.validated).unwrap_or(false)
}

/// The entry for an injected basename.
#[must_use]
pub fn entry_by_basename(basename: &str) -> Option<Arc<SongEntry>> {
    BY_BASENAME.read().ok()?.as_ref()?.get(basename).cloned()
}

#[must_use]
pub fn has_basename(basename: &str) -> bool {
    BY_BASENAME
        .read()
        .ok()
        .and_then(|m| m.as_ref().map(|m| m.contains_key(basename)))
        .unwrap_or(false)
}

#[must_use]
pub fn has_mcode(mcode: u32) -> bool {
    BY_MCODE
        .read()
        .ok()
        .and_then(|m| m.as_ref().map(|m| m.contains_key(&mcode)))
        .unwrap_or(false)
}

/// The entry for an injected mcode (the wheel-card → prefetch mapping).
#[must_use]
pub fn entry_by_mcode(mcode: u32) -> Option<Arc<SongEntry>> {
    BY_MCODE.read().ok()?.as_ref()?.get(&mcode).cloned()
}

fn publish(entry: &Arc<SongEntry>) {
    if let Ok(mut m) = BY_BASENAME.write() {
        m.get_or_insert_with(HashMap::new)
            .insert(entry.basename.clone(), entry.clone());
    }
    if let Ok(mut m) = BY_MCODE.write() {
        m.get_or_insert_with(HashMap::new)
            .insert(entry.mcode, entry.clone());
    }
}

fn unpublish(entry: &SongEntry) {
    if let Ok(mut m) = BY_BASENAME.write() {
        if let Some(m) = m.as_mut() {
            m.remove(&entry.basename);
        }
    }
    if let Ok(mut m) = BY_MCODE.write() {
        if let Some(m) = m.as_mut() {
            m.remove(&entry.mcode);
        }
    }
}

/// Flag a song whose bank build failed, for this session: the resolver
/// serves silence for it from now on and WARNs once. Never written to disk
/// — the prep tool's measurement pass records real decode failures.
pub fn mark_audio_failed_session(basename: &str) {
    if let Ok(mut set) = AUDIO_FAILED.lock() {
        set.get_or_insert_with(HashSet::new)
            .insert(basename.to_string());
    }
}

#[must_use]
pub fn is_audio_failed_session(basename: &str) -> bool {
    AUDIO_FAILED
        .lock()
        .ok()
        .and_then(|s| s.as_ref().map(|s| s.contains(basename)))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------
// Load + validate
// ---------------------------------------------------------------------

/// Where the records came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadSource {
    Loaded,
    /// No `index.bin` — the tool has never run here.
    Missing,
    /// `index.bin` exists but does not parse.
    Corrupt,
}

#[derive(Debug, Clone, Copy)]
pub struct LoadReport {
    pub source: LoadSource,
    pub songs: usize,
    pub rejects: usize,
}

/// Parse `index.bin` and publish every record (unvalidated). Milliseconds.
pub fn load() -> LoadReport {
    let path = cache::index_path();
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(_) => {
            settle_empty();
            return LoadReport {
                source: LoadSource::Missing,
                songs: 0,
                rejects: 0,
            };
        }
    };
    let Some(file) = model::parse_index(&bytes) else {
        settle_empty();
        return LoadReport {
            source: LoadSource::Corrupt,
            songs: 0,
            rejects: 0,
        };
    };
    let rejects = file.rejects.len();
    // Rejected-on-an-earlier-run songs stay out; the tool reports them.
    let songs: Vec<Arc<SongEntry>> = file
        .songs
        .into_iter()
        .filter(|s| !s.audio_failed)
        .map(Arc::new)
        .collect();
    let packs: HashSet<&str> = songs.iter().map(|s| s.pack.as_str()).collect();
    for s in &songs {
        publish(s);
    }
    let n = songs.len();
    if let Ok(mut st) = STATE.lock() {
        st.packs = packs.len();
        st.rejected = rejects;
        st.loaded = n;
        // The workers `pop` the queue; reversed so they go in file order.
        let mut pending = songs.clone();
        pending.reverse();
        st.pending = pending;
        st.songs = songs;
        st.validated = n == 0;
    }
    LoadReport {
        source: LoadSource::Loaded,
        songs: n,
        rejects,
    }
}

/// No records: the index is validated as soon as it is loaded.
fn settle_empty() {
    if let Ok(mut st) = STATE.lock() {
        st.validated = true;
    }
}

/// Validation worker count: CPU count clamped to 2..=6 (stat-bound work;
/// under Wine a stat is several ms).
fn validation_workers() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .clamp(2, 6)
}

/// Workers still running; the last one to finish settles the state.
static VALIDATORS_ACTIVE: AtomicUsize = AtomicUsize::new(0);
static VALIDATION_STARTED: Mutex<Option<Instant>> = Mutex::new(None);

fn lower_priority() {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
        };
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

/// Spawn the read-only validation workers over the loaded records. Nothing
/// waits on them; no-op when nothing was loaded.
pub fn validate_async(root: &Path) {
    if is_validated() {
        return;
    }
    let pending = STATE.lock().map(|st| st.pending.len()).unwrap_or(0);
    let workers = validation_workers().min(pending.max(1));
    if let Ok(mut s) = VALIDATION_STARTED.lock() {
        *s = Some(Instant::now());
    }
    VALIDATORS_ACTIVE.store(workers, Ordering::Release);
    let mut spawned = 0usize;
    for i in 0..workers {
        let root = root.to_path_buf();
        let ok = std::thread::Builder::new()
            .name(format!("stepmania-validate-{i}"))
            .spawn(move || {
                lower_priority();
                if panic::catch_unwind(AssertUnwindSafe(|| validate(&root))).is_err() {
                    log_warn!(
                        "StepMania: a validation worker panicked -- the records it had not reached stay as loaded"
                    );
                }
                validator_done();
            })
            .is_ok();
        if ok {
            spawned += 1;
        }
    }
    for _ in spawned..workers {
        validator_done(); // threads that never started
    }
    if spawned == 0 {
        log_warn!("StepMania: could not start the validation threads -- records stay as loaded");
    }
}

/// A worker finished; the last one removes the failed records, settles the
/// state, logs, and hands the dropped mcodes to the service for hiding.
fn validator_done() {
    if VALIDATORS_ACTIVE.fetch_sub(1, Ordering::AcqRel) != 1 {
        return;
    }
    let (dropped, kept, tally) = {
        let Ok(mut st) = STATE.lock() else {
            return;
        };
        if st.validated {
            return;
        }
        let dropped = std::mem::take(&mut st.dropped);
        if !dropped.is_empty() {
            let gone: HashSet<&str> = dropped.iter().map(|e| e.basename.as_str()).collect();
            st.songs.retain(|e| !gone.contains(e.basename.as_str()));
        }
        st.pending = Vec::new();
        st.validated = true;
        (dropped, st.songs.len(), st.tally)
    };
    let elapsed_ms = VALIDATION_STARTED
        .lock()
        .ok()
        .and_then(|s| *s)
        .map(|s| s.elapsed().as_millis())
        .unwrap_or(0);
    if !tally.is_empty() {
        log_warn!("StepMania: {}", tally);
    }
    log_info!(
        "StepMania: index validated -- {} song(s) kept, {} dropped, {} ms on {} thread(s)",
        kept,
        dropped.len(),
        elapsed_ms,
        validation_workers()
    );
    let mcodes: Vec<u32> = dropped.iter().map(|e| e.mcode).collect();
    super::on_validation_complete(&mcodes);
}

/// Take the next unvalidated record, or `None` when the queue is empty.
fn next_pending() -> Option<Arc<SongEntry>> {
    STATE.lock().ok()?.pending.pop()
}

/// One worker's loop: validate records until the queue is empty.
fn validate(root: &Path) {
    while let Some(entry) = next_pending() {
        let song_dir = song_dir_of(root, &entry);
        let simfile: Option<PathBuf> = convert::find_simfile(&song_dir);
        let probe = validation::Probe {
            artefacts_present: artefacts_present(&entry),
            simfile_found: simfile.is_some(),
            fingerprint_now: simfile
                .as_deref()
                .and_then(|sf| warm_fingerprint(root, &song_dir, sf, &entry)),
        };
        if let Verdict::Dropped(cause) = validation::verdict(&probe, entry.fingerprint) {
            unpublish(&entry);
            if let Ok(mut st) = STATE.lock() {
                st.tally.record(cause);
                st.dropped.push(entry);
            }
        }
    }
}

// ---------------------------------------------------------------------
// Art + file helpers used by the providers
// ---------------------------------------------------------------------

/// Compose the jacket square for a pick (placeholder when it does not
/// decode); returns the square and the kind tag recorded in the index.
pub fn compose_art(pick: Option<&convert::ArtPick>) -> (image::RgbaImage, u8) {
    let Some(pick) = pick else {
        return (art::placeholder_jacket(), 0);
    };
    let Some(bytes) = read_capped(&pick.path, MAX_ART_BYTES) else {
        return (art::placeholder_jacket(), 0);
    };
    let Some(img) = art::jacket::decode(&bytes) else {
        return (art::placeholder_jacket(), 0);
    };
    let kind = pick.kind.unwrap_or_else(|| {
        let (w, h) = img.dimensions();
        art::source::classify_aspect(w, h)
    });
    (art::jacket::square_from(kind, &img), kind.tag())
}

/// Compose from an index record's art fields (the jacket provider's path).
pub fn compose_art_for_entry(root: &Path, entry: &SongEntry) -> image::RgbaImage {
    let Some(source) = entry.art.source.as_ref() else {
        return art::placeholder_jacket();
    };
    let pick = convert::ArtPick {
        kind: SourceKind::from_tag(entry.art.kind),
        path: root.join(source),
    };
    compose_art(Some(&pick)).0
}

/// Read a file no larger than `cap` bytes.
pub fn read_capped(path: &Path, cap: u64) -> Option<Vec<u8>> {
    let (size, _) = cache::stat(path)?;
    if size > cap {
        return None;
    }
    fs::read(path).ok()
}
