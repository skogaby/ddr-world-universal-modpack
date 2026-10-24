//! The song index — walks `./stepmania/Songs/<Pack>/<Song>/`, converts each
//! song on a small worker pool and materialises the boot-time artefacts
//! (design §4.7 `index.rs`, §5.1, §5.3).
//!
//! ## Cold path (per song, on a worker)
//! simfile (`.ssc` over `.sm`, ≤ 4 MiB) → `core::sm::parse_bytes` →
//! `#SELECTABLE:NO` rejects → audio (`#MUSIC` else the folder's ogg/mp3/wav,
//! ≤ 256 MiB, ≤ 60 min by probe) → trial `write_song` (lead-in 0) →
//! `choose_lead_in` from its timings → `write_song` with the chosen lead-in →
//! `check_ssq` on EVERY file of the bundle (one `<b>.ssq`, or one `<b>_N.ssq`
//! per level when the charts carry different timing — `ssq_path` answers
//! the game's builder for those) → a slot is declared only when its file
//! passed → `ssq/<b>[_N].ssq` (as converted — auto-sync never changes a
//! file; `sync_state.rs`), `xsb/<b>.xsb`, `tn/<b>.dds` (art composed per
//! `convert::resolve_art`, placeholder when nothing decodes) → `SongEntry`
//! (with `split_levels` for the resolver).
//!
//! ## Warm path
//! `index.bin` holds every entry with a fingerprint over the `(path, size,
//! mtime)` of simfile + audio + art and `model::FORMAT_VERSION`. A song whose
//! fingerprint and identity match and whose three artefacts exist is reused
//! without touching audio; a changed/new song re-converts; a vanished song's
//! artefacts are deleted. Rejections are cached against a stamp of the whole
//! song folder, so a warm boot re-WARNs them without re-parsing. Identities
//! come from `identity::assign` over every discovered key up front, so the
//! probing order is stable across boots.
//!
//! ## Publication
//! Results land in a process-wide state as they finish; `wait_ready`
//! blocks (bounded) until the coordinator marks the index complete. The
//! `startup.arc` transform reads whatever finished by its deadline. The
//! `basename → entry` map the resolver consults is filled as entries land.
//!
//! Workers use `std::fs` only (never AVS — the hooks are live and the
//! FileManager thread must not be contended) and every per-song step is
//! panic-contained: one bad pack file costs one rejection, not the boot.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant};

use crate::core::art;
use crate::core::art::source::SourceKind;
use crate::core::audio::lead_in::{choose_lead_in, LeadIn};
use crate::core::audio::preview::PreviewWindow;
use crate::core::audio::{probe, AudioInfo};
use crate::core::sm;
use crate::core::ssq::check::check_ssq;
use crate::core::ssq::writer::{self, WriteOptions};
use crate::core::xact::xsb;
use crate::{log_info, log_warn};

use super::cache;
use super::convert;
use super::identity::{self, Identity};
use super::model::{self, ArtMeta, AudioMeta, FileStamp, IndexFile, RejectEntry, SongEntry};

/// Largest audio file converted.
pub const MAX_AUDIO_BYTES: u64 = 256 * 1024 * 1024;
/// Longest song accepted.
pub const MAX_DURATION_S: f64 = 60.0 * 60.0;
/// Largest art source read.
pub const MAX_ART_BYTES: u64 = 16 * 1024 * 1024;

/// What `early_init` resolved for the coordinator.
#[derive(Debug, Clone)]
pub struct Settings {
    /// `./stepmania/Songs`.
    pub root: PathBuf,
    pub audio_cache_mb: u32,
}

#[derive(Default)]
struct State {
    complete: bool,
    songs: Vec<Arc<SongEntry>>,
    packs: usize,
    rejected: usize,
    converted: usize,
    reused: usize,
    elapsed_ms: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    complete: false,
    songs: Vec::new(),
    packs: 0,
    rejected: 0,
    converted: 0,
    reused: 0,
    elapsed_ms: 0,
});
static READY: Condvar = Condvar::new();
static BY_BASENAME: RwLock<Option<HashMap<String, Arc<SongEntry>>>> = RwLock::new(None);
static BY_MCODE: RwLock<Option<HashMap<u32, Arc<SongEntry>>>> = RwLock::new(None);
/// Serialises `index.bin` rewrites (completion + `mark_audio_failed`).
static INDEX_FILE: Mutex<Option<IndexFile>> = Mutex::new(None);

/// A point-in-time view of the index.
#[derive(Clone, Debug)]
pub struct IndexSnapshot {
    pub songs: Arc<Vec<Arc<SongEntry>>>,
    pub complete: bool,
    pub packs: usize,
    pub rejected: usize,
    pub converted: usize,
    pub reused: usize,
    pub elapsed_ms: u64,
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
        complete: st.complete,
        packs: st.packs,
        rejected: st.rejected,
        converted: st.converted,
        reused: st.reused,
        elapsed_ms: st.elapsed_ms,
    }
}

/// The index as it is right now (possibly incomplete).
#[must_use]
pub fn snapshot() -> IndexSnapshot {
    match STATE.lock() {
        Ok(st) => snapshot_locked(&st),
        Err(p) => snapshot_locked(&p.into_inner()),
    }
}

#[must_use]
pub fn is_complete() -> bool {
    STATE.lock().map(|s| s.complete).unwrap_or(false)
}

/// Block until the coordinator marks the index complete or `deadline`
/// passes; returns the snapshot either way (`complete` tells which).
#[must_use]
pub fn wait_ready(deadline: Duration) -> IndexSnapshot {
    let Ok(guard) = STATE.lock() else {
        return snapshot();
    };
    let (st, _) = READY
        .wait_timeout_while(guard, deadline, |s| !s.complete)
        .unwrap_or_else(|p| p.into_inner());
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

fn publish_entry(entry: Arc<SongEntry>, reused: bool) {
    if let Ok(mut m) = BY_BASENAME.write() {
        m.get_or_insert_with(HashMap::new)
            .insert(entry.basename.clone(), entry.clone());
    }
    if let Ok(mut m) = BY_MCODE.write() {
        m.get_or_insert_with(HashMap::new)
            .insert(entry.mcode, entry.clone());
    }
    if let Ok(mut st) = STATE.lock() {
        st.songs.push(entry);
        if reused {
            st.reused += 1;
        } else {
            st.converted += 1;
        }
    }
}

fn publish_reject(pack: &str, folder: &str, reason: &str) {
    log_warn!("StepMania: rejected '{}/{}': {}", pack, folder, reason);
    if let Ok(mut st) = STATE.lock() {
        st.rejected += 1;
    }
}

/// Flag a song whose bank build failed so the next boot rejects it until
/// its inputs change; rewrites `index.bin`.
pub fn mark_audio_failed(basename: &str) {
    let Ok(mut file) = INDEX_FILE.lock() else {
        return;
    };
    let Some(file) = file.as_mut() else {
        return;
    };
    let mut changed = false;
    for s in &mut file.songs {
        if s.basename == basename && !s.audio_failed {
            s.audio_failed = true;
            changed = true;
        }
    }
    if changed {
        let _ = cache::write_atomic(&cache::index_path(), &model::serialize_index(file));
    }
}

// ---------------------------------------------------------------------
// Coordinator
// ---------------------------------------------------------------------

struct Job {
    pack: String,
    folder: String,
    key: String,
    song_dir: PathBuf,
    pack_dir: PathBuf,
    simfile: PathBuf,
    identity: Identity,
}

/// Spawn the coordinator thread. Returns `false` if the thread could not
/// be spawned (the index then stays empty and nothing is injected).
pub fn spawn(settings: Arc<Settings>) -> bool {
    std::thread::Builder::new()
        .name("stepmania-index".into())
        .spawn(move || {
            let result = panic::catch_unwind(AssertUnwindSafe(|| run(&settings)));
            if result.is_err() {
                log_warn!("StepMania: index coordinator panicked -- injecting what finished");
            }
            finish();
        })
        .is_ok()
}

fn finish() {
    if let Ok(mut st) = STATE.lock() {
        st.complete = true;
    }
    READY.notify_all();
    super::on_index_complete();
}

fn run(settings: &Settings) {
    let started = Instant::now();
    if !cache::ensure_layout() {
        log_warn!("StepMania: cache layout unavailable -- no songs injected this boot");
        return;
    }
    let swept = cache::sweep_tmp_files();
    if swept > 0 {
        log_info!("StepMania: swept {} stale .tmp file(s)", swept);
    }
    cache::init_lru(settings.audio_cache_mb);

    let cached = fs::read(cache::index_path())
        .ok()
        .and_then(|b| model::parse_index(&b));
    let cold = cached.is_none();
    let cached = cached.unwrap_or_default();
    let mut cached_songs: HashMap<String, SongEntry> = cached
        .songs
        .into_iter()
        .map(|s| (identity::key(&s.pack, &s.folder), s))
        .collect();
    let cached_rejects: HashMap<String, RejectEntry> = cached
        .rejects
        .into_iter()
        .map(|r| (identity::key(&r.pack, &r.folder), r))
        .collect();

    let (jobs, packs) = discover(&settings.root);
    if let Ok(mut st) = STATE.lock() {
        st.packs = packs;
    }

    // Vanished songs: drop their artefacts.
    let live_keys: HashSet<&str> = jobs.iter().map(|j| j.key.as_str()).collect();
    for (key, entry) in &cached_songs {
        if !live_keys.contains(key.as_str()) {
            cache::remove_song_artefacts(&entry.basename);
            log_info!(
                "StepMania: '{}/{}' removed -- artefacts deleted",
                entry.pack,
                entry.folder
            );
        }
    }

    // Warm reuse / cached rejects / work queue.
    let mut file_songs: Vec<SongEntry> = Vec::with_capacity(jobs.len());
    let mut file_rejects: Vec<RejectEntry> = Vec::new();
    let mut queue: VecDeque<Job> = VecDeque::new();
    for job in jobs {
        if let Some(entry) = cached_songs.remove(&job.key) {
            let same_identity =
                entry.basename == job.identity.basename && entry.mcode == job.identity.mcode;
            let fp = warm_fingerprint(&job, &entry);
            if same_identity && fp == Some(entry.fingerprint) && artefacts_present(&entry) {
                if entry.audio_failed {
                    publish_reject(
                        &job.pack,
                        &job.folder,
                        "audio bank build failed on an earlier boot (fix the audio file to retry)",
                    );
                    file_songs.push(entry);
                    continue;
                }
                let entry = Arc::new(entry);
                file_songs.push((*entry).clone());
                publish_entry(entry, true);
                continue;
            }
            if !same_identity {
                cache::remove_song_artefacts(&entry.basename);
            }
            queue.push_back(job);
            continue;
        }
        if let Some(rej) = cached_rejects.get(&job.key) {
            if folder_stamp(&job.song_dir) == rej.fingerprint {
                publish_reject(&job.pack, &job.folder, &rej.reason);
                file_rejects.push(rej.clone());
                continue;
            }
        }
        queue.push_back(job);
    }

    let to_convert = queue.len();
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .clamp(2, 6)
        .min(to_convert.max(1));
    let queue = Mutex::new(queue);
    let results: Mutex<(Vec<SongEntry>, Vec<RejectEntry>)> = Mutex::new((Vec::new(), Vec::new()));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let job = match queue.lock() {
                    Ok(mut q) => q.pop_front(),
                    Err(_) => None,
                };
                let Some(job) = job else {
                    break;
                };
                let outcome =
                    panic::catch_unwind(AssertUnwindSafe(|| convert_song(&settings.root, &job)))
                        .unwrap_or_else(|_| Err("conversion panicked".to_string()));
                match outcome {
                    Ok(entry) => {
                        let entry = Arc::new(entry);
                        if let Ok(mut r) = results.lock() {
                            r.0.push((*entry).clone());
                        }
                        publish_entry(entry, false);
                    }
                    Err(reason) => {
                        publish_reject(&job.pack, &job.folder, &reason);
                        if let Ok(mut r) = results.lock() {
                            r.1.push(RejectEntry {
                                pack: job.pack.clone(),
                                folder: job.folder.clone(),
                                fingerprint: folder_stamp(&job.song_dir),
                                reason,
                            });
                        }
                    }
                }
            });
        }
    });
    if let Ok(r) = results.into_inner() {
        file_songs.extend(r.0);
        file_rejects.extend(r.1);
    }

    file_songs.sort_by(|a, b| a.key().cmp(&b.key()));
    file_rejects.sort_by(|a, b| (&a.pack, &a.folder).cmp(&(&b.pack, &b.folder)));
    let index_file = IndexFile {
        songs: file_songs,
        rejects: file_rejects,
    };
    if !cache::write_atomic(&cache::index_path(), &model::serialize_index(&index_file)) {
        log_warn!("StepMania: could not write index.bin -- next boot re-converts");
    }
    if let Ok(mut f) = INDEX_FILE.lock() {
        *f = Some(index_file);
    }

    let elapsed = started.elapsed().as_millis() as u64;
    let (songs, rejected, converted, reused) = match STATE.lock() {
        Ok(mut st) => {
            st.elapsed_ms = elapsed;
            (st.songs.len(), st.rejected, st.converted, st.reused)
        }
        Err(_) => (0, 0, 0, 0),
    };
    log_info!(
        "StepMania: index complete -- {} songs, {} packs, {} rejected ({}; {} converted, {} reused), {} ms, {} worker(s)",
        songs,
        packs,
        rejected,
        if cold { "cold" } else { "warm" },
        converted,
        reused,
        elapsed,
        workers
    );
}

/// Walk `<root>/<Pack>/<Song>` and assign identities. Returns the jobs and
/// the number of packs holding at least one song.
fn discover(root: &Path) -> (Vec<Job>, usize) {
    let mut found: Vec<(String, String, PathBuf, PathBuf, PathBuf)> = Vec::new();
    let mut packs = 0usize;
    let Ok(rd) = fs::read_dir(root) else {
        log_warn!(
            "StepMania: cannot read {} -- no songs this boot",
            cache::to_game_path(root)
        );
        return (Vec::new(), 0);
    };
    let mut pack_dirs: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter(|p| {
            !p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.') || n.starts_with('_'))
        })
        .collect();
    pack_dirs.sort();
    for pack_dir in pack_dirs {
        let Some(pack) = pack_dir
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        let Ok(rd) = fs::read_dir(&pack_dir) else {
            continue;
        };
        let mut song_dirs: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        song_dirs.sort();
        let mut n = 0usize;
        for song_dir in song_dirs {
            let Some(folder) = song_dir
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_string)
            else {
                continue;
            };
            let Some(simfile) = convert::find_simfile(&song_dir) else {
                continue;
            };
            found.push((pack.clone(), folder, song_dir, pack_dir.clone(), simfile));
            n += 1;
        }
        if n > 0 {
            packs += 1;
        }
    }
    let keys: Vec<String> = found
        .iter()
        .map(|(pack, folder, ..)| identity::key(pack, folder))
        .collect();
    let assigned: HashMap<String, Identity> = identity::assign(&keys).into_iter().collect();
    let mut jobs = Vec::with_capacity(found.len());
    for (pack, folder, song_dir, pack_dir, simfile) in found {
        let key = identity::key(&pack, &folder);
        let Some(id) = assigned.get(&key).cloned() else {
            publish_reject(&pack, &folder, "no free identity (probing exhausted)");
            continue;
        };
        jobs.push(Job {
            pack,
            folder,
            key,
            song_dir,
            pack_dir,
            simfile,
            identity: id,
        });
    }
    (jobs, packs)
}

fn stamp_of<'a>(path_rel: &'a str, path: &Path) -> Option<FileStamp<'a>> {
    let (size, mtime_secs) = cache::stat(path)?;
    Some(FileStamp {
        path: path_rel,
        size,
        mtime_secs,
    })
}

/// Relative (to the songs root, forward slashes) rendering of a path.
fn rel_to_root(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Fingerprint of simfile + audio + art stamps.
fn song_fingerprint(root: &Path, simfile: &Path, audio: &Path, art: Option<&Path>) -> Option<u64> {
    let sim_rel = rel_to_root(root, simfile);
    let audio_rel = rel_to_root(root, audio);
    let art_rel = art.map(|p| rel_to_root(root, p));
    let mut stamps = vec![stamp_of(&sim_rel, simfile)?, stamp_of(&audio_rel, audio)?];
    if let (Some(rel), Some(path)) = (art_rel.as_deref(), art) {
        stamps.push(stamp_of(rel, path)?);
    }
    Some(model::fingerprint(&stamps))
}

/// The cached entry's fingerprint recomputed from today's files.
fn warm_fingerprint(job: &Job, entry: &SongEntry) -> Option<u64> {
    // The simfile the index picked may differ (an .ssc added next to the .sm).
    let audio = job.song_dir.join(&entry.audio.rel_path);
    let root = job.song_dir.parent()?.parent()?;
    let art = entry.art.source.as_ref().map(|s| root.join(s));
    song_fingerprint(root, &job.simfile, &audio, art.as_deref())
}

fn artefacts_present(entry: &SongEntry) -> bool {
    let ssq_ok = if entry.has_split_files() {
        entry
            .split_levels
            .iter()
            .enumerate()
            .filter(|(_, m)| **m != 0)
            .all(|(i, _)| cache::ssq_split_path(&entry.basename, i as u8 + 1).is_file())
    } else {
        cache::ssq_path(&entry.basename).is_file()
    };
    ssq_ok
        && cache::xsb_path(&entry.basename).is_file()
        && cache::tn_path(&entry.basename).is_file()
}

/// Stamp of every file directly in a song folder (names, sizes, mtimes).
fn folder_stamp(dir: &Path) -> u64 {
    let mut h = model::fnv1a64(&model::FORMAT_VERSION.to_le_bytes());
    for p in convert::list_files(dir) {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let (size, mtime) = cache::stat(&p).unwrap_or((0, 0));
        h = model::fnv1a64_continue(h, name.as_bytes());
        h = model::fnv1a64_continue(h, &size.to_le_bytes());
        h = model::fnv1a64_continue(h, &mtime.to_le_bytes());
    }
    h
}

// ---------------------------------------------------------------------
// Per-song conversion
// ---------------------------------------------------------------------

fn convert_song(root: &Path, job: &Job) -> Result<SongEntry, String> {
    let b = job.identity.basename.as_str();
    if !xsb::is_valid_name(b) || !identity::is_well_formed_basename(b) {
        return Err(format!("derived basename {b:?} is not usable"));
    }
    let sim_bytes = fs::read(&job.simfile).map_err(|e| format!("read simfile: {e}"))?;
    let hint = job
        .simfile
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("x.sm");
    let song = sm::parse_bytes(hint, &sim_bytes).ok_or("simfile exceeds the 4 MiB cap")?;
    if !song.selectable {
        return Err("#SELECTABLE:NO".into());
    }
    if song.charts.is_empty() {
        let why: Vec<String> = song
            .rejected
            .iter()
            .map(|r| format!("{} {}: {}", r.stepstype, r.difficulty, r.reason))
            .collect();
        return Err(format!("no chart parsed ({})", why.join("; ")));
    }

    // Charts, trial write.
    let bundle0 = writer::write_song(&song, &WriteOptions::default()).map_err(|e| {
        let writer::WriteError::NothingPlayable(rejects) = &e;
        let why: Vec<String> = rejects
            .iter()
            .map(|(label, reason)| format!("{label}: {reason}"))
            .collect();
        format!("no playable chart ({})", why.join("; "))
    })?;

    // Audio.
    let audio_path = convert::find_audio(&job.song_dir, &song.music)
        .ok_or_else(|| format!("audio not found (#MUSIC {:?})", song.music))?;
    let (audio_len, _) = cache::stat(&audio_path).ok_or("audio unreadable")?;
    if audio_len > MAX_AUDIO_BYTES {
        return Err(format!(
            "audio is {} MiB > {} MiB cap",
            audio_len / (1024 * 1024),
            MAX_AUDIO_BYTES / (1024 * 1024)
        ));
    }
    let audio_bytes = fs::read(&audio_path).map_err(|e| format!("read audio: {e}"))?;
    let info: AudioInfo = probe::probe(&audio_bytes).map_err(|e| format!("audio: {e}"))?;
    if info.frames_exact == 0 || info.target_frames() == 0 {
        return Err("audio has no frames".into());
    }
    if info.duration_s() > MAX_DURATION_S {
        return Err(format!(
            "audio is {:.0} s > {:.0} s cap",
            info.duration_s(),
            MAX_DURATION_S
        ));
    }
    let rate = info.target_rate();

    // Lead-in, final write.
    let (td0, first_note_ms) = convert::chart_timings(&bundle0);
    let lead_in: LeadIn = choose_lead_in(
        -f64::from(td0) / 1000.0,
        f64::from(first_note_ms) / 1000.0,
        rate,
    );
    let bundle = if lead_in.ms == 0 {
        bundle0
    } else {
        writer::write_song(
            &song,
            &WriteOptions {
                lead_in_ms: lead_in.ms,
                ..WriteOptions::default()
            },
        )
        .map_err(|e| e.to_string())?
    };
    // Self-check every file — one `<b>.ssq`, or one `<b>_N.ssq` per level
    // when the charts carry different timing (`ssq_path` serves those). A
    // slot is declared only when its file passed; a failing file costs its
    // charts, not the song.
    let mut passed: Vec<&writer::SsqFile> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    for file in &bundle.files {
        let slots: Vec<u8> = file.charts.iter().map(|c| c.0).collect();
        match check_ssq(&file.bytes, &slots) {
            Ok(()) => passed.push(file),
            Err(failures) => {
                let why: Vec<String> = failures.iter().map(|f| format!("{f:?}")).collect();
                failed.push(format!(
                    "{}: {}",
                    convert::ssq_file_name(b, file.index),
                    why.join("; ")
                ));
            }
        }
    }
    if passed.is_empty() {
        return Err(format!("SSQ self-check failed ({})", failed.join(" / ")));
    }
    for f in &failed {
        log_warn!(
            "StepMania: '{}/{}' SSQ self-check failed for {} -- its charts are hidden",
            job.pack,
            job.folder,
            f
        );
    }
    let declared = convert::declare(&passed);
    if declared.is_empty() {
        return Err("no declared slot".into());
    }
    let levels = declared.levels;
    let split_levels = declared.split_levels;

    // Display-only scroll gimmicks of the committed charts (Step 9).
    let scroll_res = convert::committed_scroll_gimmicks_for_slots(&song, &declared.slots);
    if let Some(why) = &scroll_res.conflict {
        log_warn!(
            "StepMania: '{}/{}' scroll gimmicks: {}",
            job.pack,
            job.folder,
            why
        );
    }
    // Files are in level order; for a split song the lowest level's file
    // sets beat 0 (the renderer seam has no difficulty context either way).
    let beat0_ms = passed.first().map(|f| convert::beat0_ms(f)).unwrap_or(0);

    // Art (resolved now for the fingerprint; composed for the thumbnail).
    let art_pick = convert::resolve_art(&job.song_dir, &job.pack_dir, &song);
    let art_path = art_pick.as_ref().map(|p| p.path.clone());
    let fingerprint = song_fingerprint(root, &job.simfile, &audio_path, art_path.as_deref())
        .ok_or("could not stat the song's files")?;

    // Materialise.
    let xsb_bytes = xsb::write_song_profile(b).map_err(|e| format!("xsb: {e}"))?;
    cache::remove_ssq_files(b);
    for file in &passed {
        let path = if file.index == 0 {
            cache::ssq_path(b)
        } else {
            cache::ssq_split_path(b, file.index)
        };
        if !cache::write_atomic(&path, &file.bytes) {
            return Err(format!(
                "could not write {}",
                convert::ssq_file_name(b, file.index)
            ));
        }
    }
    if split_levels.iter().any(|&m| m != 0) {
        let names: Vec<String> = passed
            .iter()
            .map(|f| convert::ssq_file_name(b, f.index))
            .collect();
        log_info!(
            "StepMania: '{}/{}' per-chart timing -- split files {} materialised (slots {:?})",
            job.pack,
            job.folder,
            names.join(" "),
            declared.slots
        );
    }
    if !cache::write_atomic(&cache::xsb_path(b), &xsb_bytes) {
        return Err("could not write the XSB".into());
    }
    let (square, art_kind) = compose_art(art_pick.as_ref());
    let dds = art::thumbnail_dds(&square);
    if !cache::write_atomic(&cache::tn_path(b), &dds) {
        return Err("could not write the thumbnail".into());
    }

    let window = PreviewWindow::for_audio(song.sample_start_s, song.sample_length_s, &info);
    let title = convert::display_title(&song);
    let (bpm_min, bpm_max) = convert::bpm_range(&song);
    Ok(SongEntry {
        pack: job.pack.clone(),
        folder: job.folder.clone(),
        basename: b.to_string(),
        mcode: job.identity.mcode,
        title: if title.is_empty() {
            job.folder.clone()
        } else {
            title
        },
        title_sort: convert::sort_title(&song),
        artist: convert::display_artist(&song),
        bpm_min,
        bpm_max,
        levels,
        audio: AudioMeta {
            rel_path: rel_to_root(&job.song_dir, &audio_path),
            rate,
            frames: info.target_frames(),
            lead_in_frames: lead_in.frames,
            lead_in_ms: lead_in.ms,
            preview_start: window.start_frame,
            preview_len: window.len_frames,
        },
        art: ArtMeta {
            source: art_path.as_deref().map(|p| rel_to_root(root, p)),
            kind: art_kind,
        },
        fingerprint,
        audio_failed: false,
        scroll: scroll_res.gimmicks,
        beat0_ms,
        split_levels,
    })
}

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
