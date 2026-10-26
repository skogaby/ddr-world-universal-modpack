//! The song index as the game sees it — a READER of the cache the prep tool
//! (`prep/`, `ddr_world_hook_prep.exe`) wrote. The DLL never discovers,
//! converts or measures a song; the only way a StepMania song reaches the
//! game is through a `data_mods/_cache/stepmania/index.bin` the tool
//! produced beforehand.
//!
//! ## Boot
//! [`load`] (step 0', `early_init`) parses `index.bin` synchronously — a few
//! ms for thousands of records — and publishes every record to the lookup
//! maps so the resolver and the mods can answer at once. [`validate_async`]
//! then re-checks each record on a read-only thread against the disk
//! (`records.rs`: the three artefacts exist, the simfile/audio/art stamps
//! still hash to the record's fingerprint — `validation.rs` decides) and
//! removes the failures; the `startup.arc` fingerprint waits for it through
//! [`wait_validated`] (bounded; records not reached by the cap are dropped
//! too) so the music DB never lists a song whose chart or audio changed
//! under the cache. One WARN per boot tallies what was dropped and names
//! the tool.
//!
//! Nothing here writes `index.bin`. A bank that fails to decode at play is
//! flagged for the SESSION only ([`mark_audio_failed_session`]); the tool's
//! measurement pass finds and records the same failures offline.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant};

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
    /// Records validated and kept so far (final once `validated`).
    songs: Vec<Arc<SongEntry>>,
    /// Loaded records the validation thread has not reached yet.
    pending: Vec<Arc<SongEntry>>,
    tally: DropTally,
    packs: usize,
    rejected: usize,
}

static STATE: Mutex<State> = Mutex::new(State {
    validated: false,
    songs: Vec::new(),
    pending: Vec::new(),
    tally: DropTally {
        artefacts: 0,
        sources: 0,
        changed: 0,
        unvalidated: 0,
    },
    packs: 0,
    rejected: 0,
});
static READY: Condvar = Condvar::new();
static BY_BASENAME: RwLock<Option<HashMap<String, Arc<SongEntry>>>> = RwLock::new(None);
static BY_MCODE: RwLock<Option<HashMap<u32, Arc<SongEntry>>>> = RwLock::new(None);
/// Basenames whose bank build failed this session (never persisted).
static AUDIO_FAILED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// A point-in-time view of the index.
#[derive(Clone, Debug)]
pub struct IndexSnapshot {
    pub songs: Arc<Vec<Arc<SongEntry>>>,
    /// The validation pass has finished (or been given up on).
    pub validated: bool,
    pub packs: usize,
    /// Rejections the tool cached (reported by the tool, counted here).
    pub rejected: usize,
    /// Records dropped by validation this boot.
    pub dropped: usize,
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
    }
}

/// The index as it is right now (possibly not yet validated).
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

/// Block until validation finishes or `cap` passes; records the thread had
/// not reached by then are dropped (counted in the WARN) so the snapshot
/// returned is final for this boot either way.
#[must_use]
pub fn wait_validated(cap: Duration) -> IndexSnapshot {
    let Ok(guard) = STATE.lock() else {
        return snapshot();
    };
    let (mut st, _) = READY
        .wait_timeout_while(guard, cap, |s| !s.validated)
        .unwrap_or_else(|p| p.into_inner());
    if !st.validated {
        // The thread may still be running; it finds nothing left to take.
        let pending: Vec<Arc<SongEntry>> = std::mem::take(&mut st.pending);
        for e in &pending {
            unpublish(e);
        }
        st.tally.unvalidated += pending.len();
        st.validated = true;
        if !st.tally.is_empty() {
            log_warn!(
                "StepMania: validation did not finish within {} s -- {}",
                cap.as_secs(),
                st.tally
            );
        }
        READY.notify_all();
    }
    snapshot_locked(&st)
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
    let mut songs: Vec<Arc<SongEntry>> = file
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
        // Reverse so `pop` validates in file (pack/folder) order.
        songs.reverse();
        st.pending = songs;
        st.validated = n == 0;
    }
    if n == 0 {
        READY.notify_all();
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
    READY.notify_all();
}

/// Spawn the read-only validation thread over the loaded records. No-op
/// when nothing was loaded (then the index is already "validated").
pub fn validate_async(root: &Path) {
    if is_validated() {
        return;
    }
    let root = root.to_path_buf();
    let spawned = std::thread::Builder::new()
        .name("stepmania-validate".into())
        .spawn(move || {
            let result = panic::catch_unwind(AssertUnwindSafe(|| validate(&root)));
            if result.is_err() {
                log_warn!(
                    "StepMania: validation panicked -- dropping every record not yet checked"
                );
                let _ = wait_validated(Duration::ZERO);
            }
        })
        .is_ok();
    if !spawned {
        log_warn!("StepMania: could not start the validation thread -- nothing injected this boot");
        let _ = wait_validated(Duration::ZERO);
    }
}

/// Take the next unvalidated record, or `None` when the queue is empty (or
/// the wait expired and emptied it).
fn next_pending() -> Option<Arc<SongEntry>> {
    STATE.lock().ok()?.pending.pop()
}

fn validate(root: &Path) {
    let started = Instant::now();
    let (mut kept, mut dropped) = (0usize, 0usize);
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
        let verdict = validation::verdict(&probe, entry.fingerprint);
        if let Verdict::Dropped(_) = verdict {
            unpublish(&entry);
        }
        let Ok(mut st) = STATE.lock() else {
            return;
        };
        if st.validated {
            // The wait expired first and settled the state; the record in
            // flight was already counted as unvalidated — take it out too.
            drop(st);
            unpublish(&entry);
            return;
        }
        match verdict {
            Verdict::Keep => {
                st.songs.push(entry);
                kept += 1;
            }
            Verdict::Dropped(cause) => {
                st.tally.record(cause);
                dropped += 1;
            }
        }
    }
    let Ok(mut st) = STATE.lock() else {
        return;
    };
    if st.validated {
        return;
    }
    st.validated = true;
    let tally = st.tally;
    drop(st);
    READY.notify_all();
    if !tally.is_empty() {
        log_warn!("StepMania: {}", tally);
    }
    log_info!(
        "StepMania: index validated -- {} song(s) kept, {} dropped, {} ms",
        kept,
        dropped,
        started.elapsed().as_millis()
    );
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
