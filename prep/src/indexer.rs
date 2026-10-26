//! The song index — walks `stepmania/Songs/<Pack>/<Song>/`, converts each
//! song on a worker pool and materialises the DLL's boot-time artefacts.
//! This is the hook's former in-game `index.rs` coordinator, now the tool's;
//! the DLL only reads what this writes.
//!
//! ## Cold path (per song, on a worker — [`convert_song`])
//! simfile (`.ssc` over `.sm`, ≤ 4 MiB) → `core::sm::parse_bytes` →
//! `#SELECTABLE:NO` rejects → audio (`#MUSIC` else the folder's ogg/mp3/wav,
//! ≤ 256 MiB, ≤ 60 min by probe) → trial `write_song` (lead-in 0) →
//! `choose_lead_in` from its timings → `write_song` with the chosen lead-in →
//! `check_ssq` on EVERY file of the bundle (one `<b>.ssq`, or one `<b>_N.ssq`
//! per level when the charts carry different timing) → a slot is declared
//! only when its file passed → `ssq/<b>[_N].ssq`, `xsb/<b>.xsb`, `tn/<b>.dds`
//! (art composed per `convert::resolve_art`, placeholder when nothing
//! decodes) → `SongEntry`.
//!
//! ## Warm path
//! `index.bin` holds every entry with a fingerprint over the `(path, size,
//! mtime)` of simfile + audio + art and `model::FORMAT_VERSION`. A song whose
//! fingerprint and identity match and whose artefacts exist is reused
//! without touching audio; a changed/new song re-converts; a vanished song's
//! artefacts are deleted. Rejections are cached against a stamp of the whole
//! song folder and re-reported without re-parsing.
//!
//! ## Checkpoints
//! The coordinator rewrites `index.bin` (atomically) every
//! `Settings::checkpoint_every` converted songs or `checkpoint_secs`
//! seconds, with the reused records plus everything converted so far. A
//! checkpoint is an ordinary, valid `index.bin` that simply lacks the songs
//! not yet reached — an interrupted run's next run reuses every record in
//! it. Workers never touch `index.bin`.
//!
//! Every per-song step is panic-contained: one bad pack file costs one
//! rejection, not the run. Progress reaches the caller through [`Sink`].

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::core::art;
use crate::core::audio::lead_in::{choose_lead_in, LeadIn};
use crate::core::audio::preview::PreviewWindow;
use crate::core::audio::{probe, AudioInfo};
use crate::core::sm;
use crate::core::ssq::check::{check_ssq, CheckFailure};
use crate::core::ssq::writer::{self, WriteOptions};
use crate::core::xact::xsb;
use crate::services::stepmania::cache;
use crate::services::stepmania::convert;
use crate::services::stepmania::identity::{self, Identity};
use crate::services::stepmania::model::{
    self, ArtMeta, AudioMeta, IndexFile, RejectEntry, SongEntry,
};
use crate::services::stepmania::records::{
    artefacts_present, folder_stamp, rel_to_root, song_fingerprint, warm_fingerprint,
};
use crate::{log_info, log_warn};

/// Largest audio file converted.
pub const MAX_AUDIO_BYTES: u64 = 256 * 1024 * 1024;
/// Longest song accepted.
pub const MAX_DURATION_S: f64 = 60.0 * 60.0;
/// Largest art source read.
pub const MAX_ART_BYTES: u64 = 16 * 1024 * 1024;

/// What a run works with.
#[derive(Debug, Clone)]
pub struct Settings {
    /// `stepmania/Songs` (relative to the game folder, which is the cwd).
    pub root: PathBuf,
    /// Worker count; `None` = CPU count clamped to 2..=6.
    pub workers: Option<usize>,
    /// Checkpoint `index.bin` after this many conversions …
    pub checkpoint_every: usize,
    /// … or this many seconds, whichever comes first.
    pub checkpoint_secs: u64,
}

impl Settings {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            workers: None,
            checkpoint_every: 250,
            checkpoint_secs: 15,
        }
    }
}

/// Per-pack tallies, reported when every song of the pack is accounted for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PackStats {
    pub total: usize,
    pub reused: usize,
    pub converted: usize,
    pub rejected: usize,
}

/// Progress events. All methods have no-op defaults.
pub trait Sink: Sync {
    /// Discovery finished: how many packs / songs / already-cached songs.
    fn discovered(&self, _packs: usize, _songs: usize, _to_convert: usize) {}
    fn pack_done(&self, _pack: &str, _stats: &PackStats) {}
    fn rejected(&self, _pack: &str, _folder: &str, _reason: &str) {}
    /// A note about a converted song that is not a rejection (hidden charts,
    /// a scroll-table conflict).
    fn note(&self, _pack: &str, _folder: &str, _what: &str) {}
    fn converted(&self, _entry: &SongEntry) {}
    fn checkpoint(&self, _songs_in_file: usize) {}
    /// Polled by workers before taking a job; `true` stops the run early
    /// (the final `index.bin` is still written).
    fn should_abort(&self) -> bool {
        false
    }
}

/// A `Sink` that ignores everything.
pub struct Silent;
impl Sink for Silent {}

/// What a run produced.
#[derive(Debug)]
pub struct Outcome {
    /// Exactly what `index.bin` now holds.
    pub file: IndexFile,
    pub packs: usize,
    pub converted: usize,
    pub reused: usize,
    pub rejected: usize,
    pub cold: bool,
    pub aborted: bool,
    pub workers: usize,
    pub elapsed: Duration,
}

/// One discovered song.
#[derive(Debug, Clone)]
pub struct Job {
    pub pack: String,
    pub folder: String,
    pub key: String,
    pub song_dir: PathBuf,
    pub pack_dir: PathBuf,
    pub simfile: PathBuf,
    pub identity: Identity,
}

impl Job {
    /// A job for one song folder with a caller-supplied identity (the dry
    /// run). `None` when the folder holds no simfile.
    pub fn for_dir(song_dir: &Path, identity: Identity) -> Option<Job> {
        let name = |p: Option<&Path>| {
            p.and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string()
        };
        let simfile = convert::find_simfile(song_dir)?;
        let pack_dir = song_dir.parent()?.to_path_buf();
        let pack = name(Some(&pack_dir));
        let folder = name(Some(song_dir));
        Some(Job {
            key: identity::key(&pack, &folder),
            pack,
            folder,
            song_dir: song_dir.to_path_buf(),
            pack_dir,
            simfile,
            identity,
        })
    }
}

/// Where a conversion writes its artefacts.
pub trait ArtefactPaths: Sync {
    fn ssq(&self, basename: &str) -> PathBuf;
    fn ssq_split(&self, basename: &str, n: u8) -> PathBuf;
    fn xsb(&self, basename: &str) -> PathBuf;
    fn tn(&self, basename: &str) -> PathBuf;
    /// Remove every `<b>.ssq` / `<b>_N.ssq` before a re-conversion.
    fn remove_ssq_files(&self, basename: &str);
}

/// The DLL's cache layout (`cache.rs`).
pub struct CachePaths;

impl ArtefactPaths for CachePaths {
    fn ssq(&self, b: &str) -> PathBuf {
        cache::ssq_path(b)
    }
    fn ssq_split(&self, b: &str, n: u8) -> PathBuf {
        cache::ssq_split_path(b, n)
    }
    fn xsb(&self, b: &str) -> PathBuf {
        cache::xsb_path(b)
    }
    fn tn(&self, b: &str) -> PathBuf {
        cache::tn_path(b)
    }
    fn remove_ssq_files(&self, b: &str) {
        cache::remove_ssq_files(b);
    }
}

/// Everything in one flat directory (`--dry-run --out`).
pub struct FlatPaths(pub PathBuf);

impl ArtefactPaths for FlatPaths {
    fn ssq(&self, b: &str) -> PathBuf {
        self.0.join(format!("{b}.ssq"))
    }
    fn ssq_split(&self, b: &str, n: u8) -> PathBuf {
        self.0.join(format!("{b}_{n}.ssq"))
    }
    fn xsb(&self, b: &str) -> PathBuf {
        self.0.join(format!("{b}.xsb"))
    }
    fn tn(&self, b: &str) -> PathBuf {
        self.0.join(format!("{b}.dds"))
    }
    fn remove_ssq_files(&self, b: &str) {
        let _ = fs::remove_file(self.ssq(b));
        for n in 1..=5u8 {
            let _ = fs::remove_file(self.ssq_split(b, n));
        }
    }
}

/// How the lead-in is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeadInPolicy {
    /// The runtime rule (`choose_lead_in`).
    Auto,
    /// None (SSQ byte-identical to the ddr-chart-tools CLI).
    Zero,
    /// At least this many ms, rounded up to whole ADPCM blocks.
    AtLeastMs(u32),
}

#[derive(Debug, Clone, Copy)]
pub struct ConvertOptions {
    pub lead_in: LeadInPolicy,
    pub max_audio_bytes: u64,
    /// Require the runtime's 6-char `z…` basename shape (the index); the dry
    /// run accepts any valid XACT cue name.
    pub strict_identity: bool,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self {
            lead_in: LeadInPolicy::Auto,
            max_audio_bytes: MAX_AUDIO_BYTES,
            strict_identity: true,
        }
    }
}

/// One file of the written bundle.
#[derive(Debug)]
pub struct FileReport {
    pub index: u8,
    pub name: String,
    pub bytes: Vec<u8>,
    /// `(slot, level)` per chart in the file.
    pub charts: Vec<(u8, u8)>,
    pub check: Result<(), Vec<CheckFailure>>,
}

/// A converted song plus the intermediates a report wants.
#[derive(Debug)]
pub struct Converted {
    pub entry: SongEntry,
    pub song: sm::model::Song,
    pub audio_path: PathBuf,
    pub audio: Vec<u8>,
    pub info: AudioInfo,
    /// Timings of the lead-in-0 bundle the lead-in rule read.
    pub td0_ms: i32,
    pub first_note_ms: i32,
    pub lead_in: LeadIn,
    /// Every file of the final bundle, written or not (`check` says which).
    pub files: Vec<FileReport>,
    /// `(label, reason)` charts the writer rejected.
    pub bundle_rejects: Vec<(String, String)>,
    /// Files hidden because their self-check failed (`name: reasons`).
    pub hidden: Vec<String>,
    pub scroll_conflict: Option<String>,
}

// ---------------------------------------------------------------------
// Coordinator
// ---------------------------------------------------------------------

/// Index `settings.root` into the cache. Errors only when the cache layout
/// cannot be created; everything per song is a rejection.
pub fn run(settings: &Settings, sink: &dyn Sink) -> Result<Outcome, String> {
    let started = Instant::now();
    if !cache::ensure_layout() {
        return Err("cache layout unavailable (data_mods/_cache/stepmania)".into());
    }
    let swept = cache::sweep_tmp_files();
    if swept > 0 {
        log_info!("swept {} stale .tmp file(s)", swept);
    }

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

    let (jobs, packs) = discover(&settings.root, sink);

    // Vanished songs: drop their artefacts.
    let live_keys: HashSet<&str> = jobs.iter().map(|j| j.key.as_str()).collect();
    for (key, entry) in &cached_songs {
        if !live_keys.contains(key.as_str()) {
            cache::remove_song_artefacts(&entry.basename);
            log_info!(
                "'{}/{}' removed -- artefacts deleted",
                entry.pack,
                entry.folder
            );
        }
    }

    // Warm reuse / cached rejects / work queue.
    let mut tally: HashMap<String, PackStats> = HashMap::new();
    for job in &jobs {
        tally.entry(job.pack.clone()).or_default().total += 1;
    }
    let mut file_songs: Vec<SongEntry> = Vec::with_capacity(jobs.len());
    let mut file_rejects: Vec<RejectEntry> = Vec::new();
    let mut queue: VecDeque<Job> = VecDeque::new();
    let (mut reused, mut rejected) = (0usize, 0usize);
    // Packs completed by the warm pass alone are reported after `discovered`.
    let deferred = DeferredPacks::default();
    let total_songs = jobs.len();
    for job in jobs {
        if let Some(entry) = cached_songs.remove(&job.key) {
            let same_identity =
                entry.basename == job.identity.basename && entry.mcode == job.identity.mcode;
            let root = &settings.root;
            let fp = warm_fingerprint(root, &job.song_dir, &job.simfile, &entry);
            if same_identity && fp == Some(entry.fingerprint) && artefacts_present(&entry) {
                if entry.audio_failed {
                    sink.rejected(
                        &job.pack,
                        &job.folder,
                        "audio bank build failed on an earlier boot (fix the audio file to retry)",
                    );
                    rejected += 1;
                    pack_account(&mut tally, &job.pack, &deferred, |s| s.rejected += 1);
                    file_songs.push(entry);
                    continue;
                }
                reused += 1;
                pack_account(&mut tally, &job.pack, &deferred, |s| s.reused += 1);
                file_songs.push(entry);
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
                sink.rejected(&job.pack, &job.folder, &rej.reason);
                rejected += 1;
                pack_account(&mut tally, &job.pack, &deferred, |s| s.rejected += 1);
                file_rejects.push(rej.clone());
                continue;
            }
        }
        queue.push_back(job);
    }

    let to_convert = queue.len();
    sink.discovered(packs, total_songs, to_convert);
    for (pack, stats) in deferred.take() {
        sink.pack_done(&pack, &stats);
    }
    let workers = settings
        .workers
        .unwrap_or_else(default_workers)
        .max(1)
        .min(to_convert.max(1));

    // Workers pull jobs and send results; the coordinator (this thread,
    // inside the scope) tallies, checkpoints and reports.
    let queue = Mutex::new(queue);
    let (tx, rx) = mpsc::channel::<(Job, Result<Converted, String>)>();
    let opts = ConvertOptions::default();
    let paths = CachePaths;
    let mut converted = 0usize;
    let mut since_checkpoint = 0usize;
    let mut last_checkpoint = Instant::now();
    let mut aborted = false;
    let ckpt_every = settings.checkpoint_every.max(1);
    let ckpt_secs = Duration::from_secs(settings.checkpoint_secs.max(1));

    std::thread::scope(|scope| {
        for _ in 0..workers {
            let tx = tx.clone();
            let queue = &queue;
            let root = settings.root.as_path();
            let paths = &paths;
            scope.spawn(move || loop {
                if sink.should_abort() {
                    break;
                }
                let job = match queue.lock() {
                    Ok(mut q) => q.pop_front(),
                    Err(_) => None,
                };
                let Some(job) = job else {
                    break;
                };
                let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
                    convert_song(root, &job, &opts, paths)
                }))
                .unwrap_or_else(|_| Err("conversion panicked".to_string()));
                if tx.send((job, outcome)).is_err() {
                    break;
                }
            });
        }
        drop(tx);

        // Receive until every worker has hung up.
        for (job, outcome) in rx {
            match outcome {
                Ok(c) => {
                    for h in &c.hidden {
                        sink.note(
                            &job.pack,
                            &job.folder,
                            &format!("SSQ self-check failed for {h} -- its charts are hidden"),
                        );
                    }
                    if let Some(why) = &c.scroll_conflict {
                        sink.note(&job.pack, &job.folder, &format!("scroll gimmicks: {why}"));
                    }
                    sink.converted(&c.entry);
                    converted += 1;
                    since_checkpoint += 1;
                    pack_account(&mut tally, &job.pack, sink, |s| s.converted += 1);
                    file_songs.push(c.entry);
                }
                Err(reason) => {
                    sink.rejected(&job.pack, &job.folder, &reason);
                    rejected += 1;
                    pack_account(&mut tally, &job.pack, sink, |s| s.rejected += 1);
                    file_rejects.push(RejectEntry {
                        pack: job.pack.clone(),
                        folder: job.folder.clone(),
                        fingerprint: folder_stamp(&job.song_dir),
                        reason,
                    });
                }
            }
            if since_checkpoint > 0
                && (since_checkpoint >= ckpt_every || last_checkpoint.elapsed() >= ckpt_secs)
            {
                write_index(&mut file_songs, &mut file_rejects);
                sink.checkpoint(file_songs.len());
                since_checkpoint = 0;
                last_checkpoint = Instant::now();
            }
        }
        // Jobs still queued after the workers stopped = an abort.
        aborted = queue.lock().map(|q| !q.is_empty()).unwrap_or(false);
    });

    let written = write_index(&mut file_songs, &mut file_rejects);
    if !written {
        log_warn!("could not write index.bin -- the next run re-converts");
    }
    Ok(Outcome {
        file: IndexFile {
            songs: file_songs,
            rejects: file_rejects,
        },
        packs,
        converted,
        reused,
        rejected,
        cold,
        aborted,
        workers,
        elapsed: started.elapsed(),
    })
}

/// CPU count clamped to 2..=6 (the DLL's former pool size).
#[must_use]
pub fn default_workers() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .clamp(2, 6)
}

/// Collects `pack_done` events fired before `discovered`.
#[derive(Default)]
struct DeferredPacks(Mutex<Vec<(String, PackStats)>>);

impl DeferredPacks {
    fn take(&self) -> Vec<(String, PackStats)> {
        self.0
            .lock()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default()
    }
}

impl Sink for DeferredPacks {
    fn pack_done(&self, pack: &str, stats: &PackStats) {
        if let Ok(mut v) = self.0.lock() {
            v.push((pack.to_string(), *stats));
        }
    }
}

/// Apply `f` to the pack's tally and report the pack when complete.
fn pack_account(
    tally: &mut HashMap<String, PackStats>,
    pack: &str,
    sink: &dyn Sink,
    f: impl FnOnce(&mut PackStats),
) {
    let Some(stats) = tally.get_mut(pack) else {
        return;
    };
    f(stats);
    if stats.reused + stats.converted + stats.rejected == stats.total {
        sink.pack_done(pack, stats);
    }
}

/// Write `index` as `index.bin` (sorted, atomic). The measurement pass uses
/// this to persist `audio_failed` flags.
pub fn write_index_file(index: &IndexFile) -> bool {
    let mut songs = index.songs.clone();
    let mut rejects = index.rejects.clone();
    write_index(&mut songs, &mut rejects)
}

/// Sort and write `index.bin` atomically; an identical file on disk is left
/// alone (a warm run writes nothing). `true` when the file is up to date.
fn write_index(songs: &mut Vec<SongEntry>, rejects: &mut Vec<RejectEntry>) -> bool {
    songs.sort_by(|a, b| a.key().cmp(&b.key()));
    rejects.sort_by(|a, b| (&a.pack, &a.folder).cmp(&(&b.pack, &b.folder)));
    let file = IndexFile {
        songs: std::mem::take(songs),
        rejects: std::mem::take(rejects),
    };
    let bytes = model::serialize_index(&file);
    let path = cache::index_path();
    let ok = match fs::read(&path) {
        Ok(existing) if existing == bytes => true,
        _ => cache::write_atomic(&path, &bytes),
    };
    *songs = file.songs;
    *rejects = file.rejects;
    ok
}

/// Walk `<root>/<Pack>/<Song>` and assign identities. Returns the jobs and
/// the number of packs holding at least one song.
pub fn discover(root: &Path, sink: &dyn Sink) -> (Vec<Job>, usize) {
    let mut found: Vec<(String, String, PathBuf, PathBuf, PathBuf)> = Vec::new();
    let mut packs = 0usize;
    let Ok(rd) = fs::read_dir(root) else {
        log_warn!("cannot read {} -- no songs", root.display());
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
            sink.rejected(&pack, &folder, "no free identity (probing exhausted)");
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

// ---------------------------------------------------------------------
// Per-song conversion
// ---------------------------------------------------------------------

/// Convert one song and write its artefacts through `paths`. Does not log:
/// the caller reports from the returned [`Converted`].
pub fn convert_song(
    root: &Path,
    job: &Job,
    opts: &ConvertOptions,
    paths: &dyn ArtefactPaths,
) -> Result<Converted, String> {
    let b = job.identity.basename.as_str();
    if !xsb::is_valid_name(b) || (opts.strict_identity && !identity::is_well_formed_basename(b)) {
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
    if audio_len > opts.max_audio_bytes {
        return Err(format!(
            "audio is {} MiB > {} MiB cap",
            audio_len / (1024 * 1024),
            opts.max_audio_bytes / (1024 * 1024)
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
    let lead_in: LeadIn = match opts.lead_in {
        LeadInPolicy::Zero => LeadIn::ZERO,
        LeadInPolicy::AtLeastMs(ms) => LeadIn::at_least_ms(ms, rate),
        LeadInPolicy::Auto => choose_lead_in(
            -f64::from(td0) / 1000.0,
            f64::from(first_note_ms) / 1000.0,
            rate,
        ),
    };
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
    // Self-check every file; a slot is declared only when its file passed.
    let mut passed: Vec<&writer::SsqFile> = Vec::new();
    let mut hidden: Vec<String> = Vec::new();
    let mut files: Vec<FileReport> = Vec::with_capacity(bundle.files.len());
    for file in &bundle.files {
        let slots: Vec<u8> = file.charts.iter().map(|c| c.0).collect();
        let name = convert::ssq_file_name(b, file.index);
        let check = check_ssq(&file.bytes, &slots);
        match &check {
            Ok(()) => passed.push(file),
            Err(failures) => {
                let why: Vec<String> = failures.iter().map(|f| format!("{f:?}")).collect();
                hidden.push(format!("{name}: {}", why.join("; ")));
            }
        }
        files.push(FileReport {
            index: file.index,
            name,
            bytes: file.bytes.clone(),
            charts: file.charts.clone(),
            check,
        });
    }
    if passed.is_empty() {
        return Err(format!("SSQ self-check failed ({})", hidden.join(" / ")));
    }
    let declared = convert::declare(&passed);
    if declared.is_empty() {
        return Err("no declared slot".into());
    }
    let levels = declared.levels;
    let split_levels = declared.split_levels;

    // Display-only scroll gimmicks of the committed charts.
    let scroll_res = convert::committed_scroll_gimmicks_for_slots(&song, &declared.slots);
    // Files are in level order; for a split song the lowest level's file
    // sets beat 0.
    let beat0_ms = passed.first().map(|f| convert::beat0_ms(f)).unwrap_or(0);

    // Art (resolved now for the fingerprint; composed for the thumbnail).
    let art_pick = convert::resolve_art(&job.song_dir, &job.pack_dir, &song);
    let art_path = art_pick.as_ref().map(|p| p.path.clone());
    let fingerprint = song_fingerprint(root, &job.simfile, &audio_path, art_path.as_deref())
        .ok_or("could not stat the song's files")?;

    // Materialise.
    let xsb_bytes = xsb::write_song_profile(b).map_err(|e| format!("xsb: {e}"))?;
    paths.remove_ssq_files(b);
    for file in &passed {
        let path = if file.index == 0 {
            paths.ssq(b)
        } else {
            paths.ssq_split(b, file.index)
        };
        if !cache::write_atomic(&path, &file.bytes) {
            return Err(format!(
                "could not write {}",
                convert::ssq_file_name(b, file.index)
            ));
        }
    }
    if !cache::write_atomic(&paths.xsb(b), &xsb_bytes) {
        return Err("could not write the XSB".into());
    }
    let (square, art_kind) = compose_art(art_pick.as_ref());
    let dds = art::thumbnail_dds(&square);
    if !cache::write_atomic(&paths.tn(b), &dds) {
        return Err("could not write the thumbnail".into());
    }

    let window = PreviewWindow::for_audio(song.sample_start_s, song.sample_length_s, &info);
    let title = convert::display_title(&song);
    let (bpm_min, bpm_max) = convert::bpm_range(&song);
    let entry = SongEntry {
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
    };
    Ok(Converted {
        entry,
        song,
        audio_path,
        audio: audio_bytes,
        info,
        td0_ms: td0,
        first_note_ms,
        lead_in,
        files,
        bundle_rejects: bundle
            .rejects
            .iter()
            .map(|(label, reason)| (label.clone(), reason.to_string()))
            .collect(),
        hidden,
        scroll_conflict: scroll_res.conflict,
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

/// Read a file no larger than `cap` bytes.
pub fn read_capped(path: &Path, cap: u64) -> Option<Vec<u8>> {
    let (size, _) = cache::stat(path)?;
    if size > cap {
        return None;
    }
    fs::read(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_paths_name_like_the_cache() {
        let p = FlatPaths(PathBuf::from("/o"));
        assert_eq!(p.ssq("zabcde"), PathBuf::from("/o/zabcde.ssq"));
        assert_eq!(p.ssq_split("zabcde", 3), PathBuf::from("/o/zabcde_3.ssq"));
        assert_eq!(p.xsb("zabcde"), PathBuf::from("/o/zabcde.xsb"));
        assert_eq!(p.tn("zabcde"), PathBuf::from("/o/zabcde.dds"));
    }

    #[test]
    fn job_for_dir_derives_pack_and_folder() {
        let dir = std::env::temp_dir().join(format!("ddr_idx_job_{}", std::process::id()));
        let song = dir.join("Pack A").join("Song B");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&song).unwrap();
        assert!(Job::for_dir(&song, identity::derive("x", 0)).is_none());
        fs::write(song.join("b.sm"), b"#TITLE:b;").unwrap();
        let j = Job::for_dir(&song, identity::derive("x", 0)).unwrap();
        assert_eq!((j.pack.as_str(), j.folder.as_str()), ("Pack A", "Song B"));
        assert_eq!(j.key, identity::key("Pack A", "Song B"));
        assert!(j.simfile.ends_with("b.sm"));
        let _ = fs::remove_dir_all(&dir);
    }
}
