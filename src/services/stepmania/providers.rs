//! The LayeredFS resolver for injected songs — maps the five per-song game
//! paths to cache files — plus the build bodies the scheduler runs (design
//! §3.4, §4.7 `providers.rs`, §5.6).
//!
//! Registered through `mod_paths::register_resolver`, so it runs on every
//! intercepted open/lstat/convert_path AFTER the operator's mod folders and
//! must be cheap for anything that is not ours: four prefix tests, then a
//! `basename → entry` map lookup. Paths served:
//!
//! | game path | served from |
//! |---|---|
//! | `mdb_apx/ssq/<b>.ssq` | `ssq/<b>.ssq` (materialised at index time) |
//! | `mdb_apx/ssq/<b>_N.ssq` | `ssq/<b>_N.ssq` (per-chart-timing split file; `ssq_path` named it) |
//! | `sound/win/dance/<b>.xsb` | `xsb/<b>.xsb` (index time) |
//! | `sound/win/dance/<b>.xwb` | `xwb/<b>.xwb` (full) or `xwb/<b>.p.xwb` (preview) — see below |
//! | `arc/jacket/<b>_jk.arc` | `jacket/<b>_jk.arc` |
//! | `arc/thumbnail/jacket_thumbnails_{ja,ua}_<S>.arc` | `thumbnail/<S>.arc` for our series S only |
//!
//! Bank variant: preview while `scene_manager` reports the song wheel
//! (scene 25 — the wheel-settle preview reads the whole XWB), full otherwise
//! (the gameplay load).
//!
//! **Preview / jacket**: normally already built by the scheduler from the
//! wheel-card events (`scheduler.rs`). When not, the open enqueues the job
//! at highlighted priority and — on the FileManager's I/O worker (cabinet
//! fact E2; never the render thread) — waits ≤ 300 ms for the ticket, then
//! falls back: a 1-block silent bank (`xwb/_silent/<b>.xwb`) or the
//! placeholder jacket arc (`.ph` marker beside it, upgraded by the next
//! worker request).
//!
//! **Full bank**: never blocks the open on the ENCODE. One bounded hold
//! exists: an injected song whose auto-sync measurement is still running
//! (picked before the sweep reached it) waits ≤ 5 s for it first
//! (`sync_apply::hold_for_measurement`, on the FileManager I/O worker —
//! never on the render thread; expiry plays the song as authored for the
//! session), so the judgement offset the stage uses is decided before the
//! first judge. If `xwb/<b>.xwb` exists it is
//! served; otherwise the progressive `.part` is served — created HERE,
//! synchronously, when no job has started it yet (audio read + probe +
//! pre-size + header: a few ms), then encoded on a worker at gameplay-load
//! priority. The `.part` has the complete header from its first byte; the
//! preview entry (file tail) is written first because XACT primes every
//! entry at bank prepare, then the main entry in file order behind a
//! watermark the LayeredFS read hook honours (`progressive.rs`). The final
//! file equals a `write_complete` build byte for byte.
//!
//! Builds are panic-contained; a failed build serves the fallback and flags
//! the song `audio_failed` for the next boot. Every full-variant request —
//! ours or stock — is reported to `taint.rs`, which level-writes the score
//! taint from it. Banks and jackets live under the `audio_cache_mb` LRU
//! (`.part` files count from creation).

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Seek, SeekFrom, Write};
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::core::art;
use crate::core::audio::decode::{Decoder, TargetFormat};
use crate::core::audio::preview::{PreviewDecoder, PreviewWindow};
use crate::core::audio::{probe, AudioInfo, BLOCK_FRAMES};
use crate::core::xact::adpcm;
use crate::core::xact::song_bank::{self, SongBankPlan, SongBankWriter};
use crate::services::{scene_manager, widget_renderer};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

use super::cache::{self, BankVariant};
use super::index;
use super::model::SongEntry;
use super::progressive::{self, ProgressiveFile, Staged};
use super::scheduler::{self, Ticket, RESOLVER_WAIT};
use super::scheduler_logic::{JobKind, Urgency};
use super::sync_apply;
use super::taint;

const SSQ_PREFIX: &str = "mdb_apx/ssq/";
const DANCE_PREFIX: &str = "sound/win/dance/";
const JACKET_PREFIX: &str = "arc/jacket/";
const THUMB_PREFIX: &str = "arc/thumbnail/jacket_thumbnails_";

/// Bytes buffered before the progressive writer flushes and publishes the
/// watermark (≈ 2.7 s of 44.1 kHz ADPCM; the game reads 1 MiB chunks).
const PROGRESSIVE_FLUSH_BYTES: usize = 128 * 1024;

/// Serialises the tiny fallback writes (silent bank, placeholder jacket)
/// and the full-bank begin (check-active → prepare → register).
static SILENT_LOCK: Mutex<()> = Mutex::new(());
static JACKET_LOCK: Mutex<()> = Mutex::new(());
static BEGIN_LOCK: Mutex<()> = Mutex::new(());
static WARNED_RENDER_THREAD: AtomicBool = AtomicBool::new(false);
static WARNED_PREVIEW_LATE: AtomicBool = AtomicBool::new(false);
static WARNED_JACKET_LATE: AtomicBool = AtomicBool::new(false);
static WARNED_PANIC: AtomicBool = AtomicBool::new(false);
/// `(basename, mode)` of the last full-bank serve logged (lstat + open +
/// convert_path of one load collapse into one INFO).
static LAST_FULL_SERVE: Mutex<Option<(String, &'static str)>> = Mutex::new(None);

/// The `mod_paths::Resolver`.
pub fn resolve(norm_path: &str) -> Option<String> {
    match panic::catch_unwind(AssertUnwindSafe(|| resolve_inner(norm_path))) {
        Ok(r) => r,
        Err(_) => {
            if !WARNED_PANIC.swap(true, Ordering::AcqRel) {
                log_warn!(
                    "StepMania: resolver panicked on '{}' -- serving nothing for it (further panics silent)",
                    norm_path
                );
            }
            None
        }
    }
}

fn strip_prefix_ci<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    let head = path.get(..prefix.len())?;
    if head.eq_ignore_ascii_case(prefix) {
        path.get(prefix.len()..)
    } else {
        None
    }
}

fn strip_suffix_ci<'a>(s: &'a str, suffix: &str) -> Option<&'a str> {
    if s.len() < suffix.len() {
        return None;
    }
    let (head, tail) = s.split_at(s.len() - suffix.len());
    if tail.eq_ignore_ascii_case(suffix) {
        Some(head)
    } else {
        None
    }
}

fn resolve_inner(norm_path: &str) -> Option<String> {
    if !super::serving() {
        return None;
    }
    if let Some(rest) = strip_prefix_ci(norm_path, SSQ_PREFIX) {
        let stem = strip_suffix_ci(rest, ".ssq")?;
        if let Some(entry) = index::entry_by_basename(stem) {
            return existing(&cache::ssq_path(&entry.basename));
        }
        // `<b>_N.ssq` — a per-chart-timing split file (`ssq_path` answered
        // `Split(N)` for an injected basename).
        let (b, n) = split_ssq_name(stem)?;
        let entry = index::entry_by_basename(b)?;
        if entry
            .split_levels
            .get(usize::from(n - 1))
            .copied()
            .unwrap_or(0)
            == 0
        {
            return None;
        }
        return existing(&cache::ssq_split_path(&entry.basename, n));
    }
    if let Some(rest) = strip_prefix_ci(norm_path, DANCE_PREFIX) {
        if let Some(b) = strip_suffix_ci(rest, ".xsb") {
            let entry = index::entry_by_basename(b)?;
            return existing(&cache::xsb_path(&entry.basename));
        }
        if let Some(b) = strip_suffix_ci(rest, ".xwb") {
            if b.contains('/') {
                return None;
            }
            let variant = current_variant();
            let Some(entry) = index::entry_by_basename(b) else {
                taint::note_bank_request(b, false, variant);
                return None;
            };
            taint::note_bank_request(&entry.basename, true, variant);
            return serve_bank(&entry, variant);
        }
        return None;
    }
    if let Some(rest) = strip_prefix_ci(norm_path, JACKET_PREFIX) {
        let b = strip_suffix_ci(rest, "_jk.arc")?;
        let entry = index::entry_by_basename(b)?;
        return serve_jacket(&entry);
    }
    if let Some(rest) = strip_prefix_ci(norm_path, THUMB_PREFIX) {
        // `{ja|ua}_<N>.arc`
        let stem = strip_suffix_ci(rest, ".arc")?;
        let (_region, n) = stem.split_once('_')?;
        let n: u8 = n.parse().ok()?;
        let series = super::registered_series()?;
        if n != series {
            return None;
        }
        return existing(&cache::thumbnail_arc_path(series));
    }
    None
}

fn existing(path: &Path) -> Option<String> {
    if path.is_file() {
        Some(cache::to_game_path(path))
    } else {
        None
    }
}

/// `<b>_N` → `(b, N)` for `N` in `1..=5`; anything else `None`.
fn split_ssq_name(stem: &str) -> Option<(&str, u8)> {
    let (b, tail) = stem.rsplit_once('_')?;
    if b.is_empty() || tail.len() != 1 {
        return None;
    }
    let n = tail.bytes().next()?;
    if !(b'1'..=b'5').contains(&n) {
        return None;
    }
    Some((b, n - b'0'))
}

/// Preview on the song wheel, full everywhere else (scene manager unknown
/// ⇒ full: a wrong full is a slower preview, a wrong preview is a silent
/// stage).
fn current_variant() -> BankVariant {
    if scene_manager::is_available() && scene_manager::current_scene() == scene::SONG_SELECT {
        BankVariant::Preview
    } else {
        BankVariant::Full
    }
}

// ---------------------------------------------------------------------
// Shared preparation
// ---------------------------------------------------------------------

/// Everything a bank build needs from the source audio.
struct Prepared {
    audio: Vec<u8>,
    info: AudioInfo,
    window: PreviewWindow,
}

fn prepare(entry: &SongEntry) -> Result<Prepared, String> {
    let root = super::songs_root().ok_or("songs root unavailable")?;
    let audio_path = root
        .join(&entry.pack)
        .join(&entry.folder)
        .join(&entry.audio.rel_path);
    let audio = index::read_capped(&audio_path, index::MAX_AUDIO_BYTES)
        .ok_or_else(|| format!("audio {} unreadable or over the cap", entry.audio.rel_path))?;
    let info = probe::probe(&audio).map_err(|e| format!("probe: {e}"))?;
    let rate = info.target_rate();
    let stored = PreviewWindow {
        start_frame: entry.audio.preview_start,
        len_frames: entry.audio.preview_len,
        rate: entry.audio.rate,
    };
    // The index's window is authoritative unless the file changed under it.
    let window = if stored.rate == rate
        && stored.len_frames >= BLOCK_FRAMES as u64
        && stored.start_frame + stored.len_frames <= info.target_frames().max(BLOCK_FRAMES as u64)
    {
        stored
    } else {
        PreviewWindow::for_audio(None, None, &info)
    };
    Ok(Prepared {
        audio,
        info,
        window,
    })
}

fn plan_for(entry: &SongEntry, p: &Prepared, variant: BankVariant) -> Result<SongBankPlan, String> {
    let frames_main = match variant {
        BankVariant::Full => entry.audio.lead_in_frames + p.info.target_frames(),
        BankVariant::Preview | BankVariant::Silent => BLOCK_FRAMES as u64,
    };
    SongBankPlan::new(
        &entry.basename,
        p.info.target_rate(),
        frames_main,
        p.window.len_frames,
    )
    .map_err(|e| format!("bank plan: {e}"))
}

// ---------------------------------------------------------------------
// Banks — resolver side
// ---------------------------------------------------------------------

fn serve_bank(entry: &Arc<SongEntry>, variant: BankVariant) -> Option<String> {
    match variant {
        BankVariant::Full => serve_full_bank(entry),
        BankVariant::Preview | BankVariant::Silent => serve_preview_bank(entry),
    }
}

fn serve_preview_bank(entry: &Arc<SongEntry>) -> Option<String> {
    let b = entry.basename.as_str();
    let key = cache::xwb_key(b, BankVariant::Preview);
    let path = cache::xwb_path(b, BankVariant::Preview);
    if path.is_file() {
        cache::lru_touch(&key);
        return Some(cache::to_game_path(&path));
    }
    let ticket = scheduler::enqueue(JobKind::Preview, b, Urgency::HighlightedPreview, false);
    if widget_renderer::is_render_thread() {
        if !WARNED_RENDER_THREAD.swap(true, Ordering::AcqRel) {
            log_warn!(
                "StepMania: bank for '{}' requested on the render thread -- serving the silent variant (further cases silent)",
                b
            );
        }
        return serve_silent(b);
    }
    let started = Instant::now();
    ticket.wait(RESOLVER_WAIT);
    if path.is_file() {
        cache::lru_touch(&key);
        log_info!(
            "StepMania: preview bank '{}' served after a {} ms wait on its job",
            b,
            started.elapsed().as_millis()
        );
        return Some(cache::to_game_path(&path));
    }
    if !WARNED_PREVIEW_LATE.swap(true, Ordering::AcqRel) {
        log_warn!(
            "StepMania: preview bank '{}' not ready within {} ms -- serving the silent variant (further cases silent)",
            b,
            RESOLVER_WAIT.as_millis()
        );
    }
    serve_silent(b)
}

fn serve_full_bank(entry: &Arc<SongEntry>) -> Option<String> {
    let b = entry.basename.as_str();
    // Hold (bounded) for an in-flight auto-sync measurement — the I/O
    // worker only; the render thread never waits here.
    if !widget_renderer::is_render_thread() {
        sync_apply::hold_for_measurement(b, sync_apply::STAGE_LOAD_HOLD);
    }
    progressive::retry_pending_renames();
    let key = cache::xwb_key(b, BankVariant::Full);
    let path = cache::xwb_path(b, BankVariant::Full);
    if path.is_file() {
        cache::lru_touch(&key);
        note_full_serve(b, "complete", None);
        return Some(cache::to_game_path(&path));
    }
    if let Some(h) = progressive::active(b) {
        return Some(serve_progressive(&h));
    }
    if widget_renderer::is_render_thread() {
        if !WARNED_RENDER_THREAD.swap(true, Ordering::AcqRel) {
            log_warn!(
                "StepMania: bank for '{}' requested on the render thread -- serving the silent variant (further cases silent)",
                b
            );
        }
        return serve_silent(b);
    }
    let started = Instant::now();
    match begin_full_bank(entry) {
        Ok(Begun::Complete) => {
            cache::lru_touch(&key);
            note_full_serve(b, "complete", None);
            existing(&path).or_else(|| serve_silent(b))
        }
        Ok(Begun::Active(h)) => {
            scheduler::enqueue(JobKind::FullBank, b, Urgency::GameplayLoad, false);
            cache::lru_touch(&key);
            let served = serve_progressive(&h);
            log_info!(
                "StepMania: full bank '{}' progressive file created on open in {} ms",
                b,
                started.elapsed().as_millis()
            );
            Some(served)
        }
        Err(e) => {
            log_warn!(
                "StepMania: full bank for '{}' cannot start: {} -- serving silence; song flagged audio_failed for the next boot",
                b,
                e
            );
            index::mark_audio_failed(b);
            serve_silent(b)
        }
    }
}

fn serve_progressive(h: &ProgressiveFile) -> String {
    h.served.store(true, Ordering::Release);
    let p = h.progress();
    note_full_serve(
        &h.basename,
        if p.done {
            "complete (deferred rename)"
        } else {
            "progressive"
        },
        Some(p.watermark),
    );
    h.serve_path().to_string()
}

fn note_full_serve(basename: &str, mode: &'static str, watermark: Option<u64>) {
    let changed = match LAST_FULL_SERVE.lock() {
        Ok(mut last) => {
            let changed = last
                .as_ref()
                .is_none_or(|(b, m)| b != basename || *m != mode);
            if changed {
                *last = Some((basename.to_string(), mode));
            }
            changed
        }
        Err(_) => true,
    };
    if changed {
        match watermark {
            Some(w) => log_info!(
                "StepMania: full bank '{}' served {} (watermark {} KiB)",
                basename,
                mode,
                w / 1024
            ),
            None => log_info!("StepMania: full bank '{}' served {}", basename, mode),
        }
    }
}

/// The 1-block silent bank (built on demand, outside the LRU — a few KiB).
fn serve_silent(basename: &str) -> Option<String> {
    let _guard = SILENT_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let path = cache::xwb_path(basename, BankVariant::Silent);
    if path.is_file() {
        return Some(cache::to_game_path(&path));
    }
    let plan = SongBankPlan::new(
        basename,
        crate::core::audio::FALLBACK_RATE,
        BLOCK_FRAMES as u64,
        BLOCK_FRAMES as u64,
    )
    .ok()?;
    let tmp = cache::tmp_path(&path);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let file = File::create(&tmp).ok()?;
    let sink = song_bank::write_complete(
        BufWriter::new(file),
        &plan,
        std::iter::empty::<[i16; BLOCK_FRAMES * 2]>(),
        std::iter::empty::<[i16; BLOCK_FRAMES * 2]>(),
    )
    .ok()?;
    drop(sink.into_inner().ok()?);
    if !cache::commit_tmp(&path) {
        return None;
    }
    Some(cache::to_game_path(&path))
}

// ---------------------------------------------------------------------
// Banks — job bodies (scheduler workers)
// ---------------------------------------------------------------------

/// Build `xwb/<b>.p.xwb`. Returns the log detail.
pub(super) fn run_preview_job(entry: &SongEntry) -> Result<String, String> {
    let b = entry.basename.as_str();
    let key = cache::xwb_key(b, BankVariant::Preview);
    let path = cache::xwb_path(b, BankVariant::Preview);
    if path.is_file() {
        return Ok("already present".into());
    }
    match build_preview(entry, &path) {
        Ok(report) => {
            cache::lru_insert(&key, report.bytes);
            if let Some(w) = report.decode_warning {
                log_warn!("StepMania: '{}' audio decode: {}", b, w);
                index::mark_audio_failed(b);
            }
            Ok(format!("{} KiB", report.bytes / 1024))
        }
        Err(e) => {
            index::mark_audio_failed(b);
            Err(e)
        }
    }
}

struct BuildReport {
    bytes: u64,
    decode_warning: Option<String>,
}

fn build_preview(entry: &SongEntry, path: &Path) -> Result<BuildReport, String> {
    let p = prepare(entry)?;
    let plan = plan_for(entry, &p, BankVariant::Preview)?;
    let tmp = cache::tmp_path(path);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let file = File::create(&tmp).map_err(|e| format!("create: {e}"))?;
    let sink = BufWriter::with_capacity(1 << 20, file);
    let mut preview_dec = PreviewDecoder::open(&p.audio, &p.info, &p.window)
        .map_err(|e| format!("open preview: {e}"))?;
    let sink = song_bank::write_complete(
        sink,
        &plan,
        std::iter::empty::<[i16; BLOCK_FRAMES * 2]>(),
        preview_dec.by_ref(),
    )
    .map_err(|e| format!("xwb: {e}"))?;
    let decode_warning = preview_dec.failed().map(|e| format!("preview: {e}"));
    let file = sink.into_inner().map_err(|e| format!("flush: {e}"))?;
    file.sync_all().map_err(|e| format!("sync: {e}"))?;
    drop(file);
    if !cache::commit_tmp(path) {
        return Err("could not move the bank into place".into());
    }
    Ok(BuildReport {
        bytes: plan.total_len(),
        decode_warning,
    })
}

/// What `begin_full_bank` found or made.
pub(super) enum Begun {
    /// `xwb/<b>.xwb` already exists.
    Complete,
    /// A progressive handle (new or already active).
    Active(Arc<ProgressiveFile>),
}

/// Make sure the full bank for `entry` is complete or in progress: when
/// neither, read + probe the audio, create the pre-sized `.part` with its
/// header and register the handle (the encode is NOT started here — the
/// scheduler job does that). Serialised; idempotent.
pub(super) fn begin_full_bank(entry: &SongEntry) -> Result<Begun, String> {
    let _guard = BEGIN_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let b = entry.basename.as_str();
    if let Some(h) = progressive::active(b) {
        return Ok(Begun::Active(h));
    }
    let final_path = cache::xwb_path(b, BankVariant::Full);
    if final_path.is_file() {
        return Ok(Begun::Complete);
    }
    let p = prepare(entry)?;
    let plan = plan_for(entry, &p, BankVariant::Full)?;
    let part = progressive::part_path(&final_path);
    if let Some(parent) = final_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&part)
        .map_err(|e| format!("create {}: {e}", cache::to_game_path(&part)))?;
    file.set_len(plan.total_len())
        .map_err(|e| format!("pre-size: {e}"))?;
    let header = plan.header_bytes();
    file.seek(SeekFrom::Start(0))
        .and_then(|_| file.write_all(&header))
        .and_then(|()| file.flush())
        .map_err(|e| format!("header: {e}"))?;
    cache::lru_insert(&cache::xwb_key(b, BankVariant::Full), plan.total_len());
    let staged = Staged {
        audio: p.audio,
        info: p.info,
        window: p.window,
        file,
    };
    let h = progressive::register(b, &final_path, plan, header.len() as u64, staged);
    Ok(Begun::Active(h))
}

/// The scheduler's full-bank job: begin if needed, then encode.
pub(super) fn run_full_bank_job(entry: &SongEntry, ticket: &Ticket) -> Result<String, String> {
    match begin_full_bank(entry)? {
        Begun::Complete => Ok("already complete".into()),
        Begun::Active(h) => encode_full_bank(entry, &h, ticket),
    }
}

/// Encode into an active progressive file: preview entry (tail) first,
/// then the main entry in file order behind the watermark, then the
/// preview blocks again through the writer (identical bytes) so the file
/// matches `write_complete`. Cooperative cancel between blocks while nobody
/// has opened the file.
fn encode_full_bank(
    entry: &SongEntry,
    h: &Arc<ProgressiveFile>,
    ticket: &Ticket,
) -> Result<String, String> {
    let b = entry.basename.as_str();
    let Some(staged) = h.staged.lock().ok().and_then(|mut s| s.take()) else {
        return Ok("encode already in progress".into());
    };
    let Staged {
        audio,
        info,
        window,
        mut file,
    } = staged;
    let plan = h.plan.clone();
    let abort_requested =
        || h.should_abort() || (ticket.cancelled() && !h.served.load(Ordering::Acquire));
    if abort_requested() {
        abandon_full_bank(h, "cancelled before encoding started");
        return Err("cancelled".into());
    }

    let result = (|| -> Result<Option<String>, String> {
        // 1. Preview entry — the file tail, written first.
        let mut preview_dec = PreviewDecoder::open(&audio, &info, &window)
            .map_err(|e| format!("open preview: {e}"))?;
        let preview_blocks = plan.blocks(song_bank::PREVIEW_ENTRY) as usize;
        let mut preview_bytes: Vec<u8> =
            Vec::with_capacity(preview_blocks * song_bank::BLOCK_BYTES);
        let mut block = Vec::with_capacity(song_bank::BLOCK_BYTES);
        let mut n = 0usize;
        for pcm in preview_dec.by_ref() {
            if n >= preview_blocks {
                break;
            }
            block.clear();
            adpcm::encode_block(&pcm, plan.wave_format(), &mut block)
                .map_err(|e| format!("encode preview: {e}"))?;
            preview_bytes.extend_from_slice(&block);
            n += 1;
        }
        if n < preview_blocks {
            block.clear();
            adpcm::encode_block(&[0i16; BLOCK_FRAMES * 2], plan.wave_format(), &mut block)
                .map_err(|e| format!("encode silence: {e}"))?;
            for _ in n..preview_blocks {
                preview_bytes.extend_from_slice(&block);
            }
        }
        let mut decode_warning = preview_dec.failed().map(|e| format!("preview: {e}"));
        file.seek(SeekFrom::Start(
            plan.entry_data_offset(song_bank::PREVIEW_ENTRY),
        ))
        .and_then(|_| file.write_all(&preview_bytes))
        .and_then(|()| file.flush())
        .map_err(|e| format!("write preview entry: {e}"))?;
        h.tail_ready.store(true, Ordering::Release);
        if abort_requested() {
            return Err("cancelled".into());
        }

        // 2. Main entry, in file order, behind the watermark.
        let target = TargetFormat {
            rate: plan.rate(),
            lead_in_frames: entry.audio.lead_in_frames,
            total_frames: plan.frames(song_bank::MAIN_ENTRY),
        };
        let mut main_dec =
            Decoder::open(&audio, &info, target).map_err(|e| format!("open main: {e}"))?;
        let sink = WatermarkSink::new(file, &h.watermark);
        let mut writer = SongBankWriter::new(sink, &plan).map_err(|e| format!("xwb: {e}"))?;
        let mut since_check = 0u32;
        for pcm in main_dec.by_ref() {
            if writer.position().0 != song_bank::MAIN_ENTRY {
                break;
            }
            writer.append_pcm(&pcm).map_err(|e| format!("xwb: {e}"))?;
            since_check += 1;
            if since_check >= 64 {
                since_check = 0;
                if abort_requested() {
                    return Err("cancelled".into());
                }
            }
        }
        if writer.position().0 == song_bank::MAIN_ENTRY {
            writer
                .pad_entry_with_silence()
                .map_err(|e| format!("xwb: {e}"))?;
        }
        if decode_warning.is_none() {
            if let Some(e) = main_dec.failed() {
                decode_warning = Some(format!("main: {e}"));
            }
        }
        // 3. Preview blocks again through the writer (same bytes, same place).
        for chunk in preview_bytes.chunks(song_bank::BLOCK_BYTES) {
            writer
                .append_encoded(chunk)
                .map_err(|e| format!("xwb: {e}"))?;
        }
        let mut sink = writer.finish().map_err(|e| format!("xwb: {e}"))?;
        sink.flush().map_err(|e| format!("flush: {e}"))?;
        let file = sink.into_inner();
        file.sync_all().map_err(|e| format!("sync: {e}"))?;
        drop(file);
        Ok(decode_warning)
    })();

    match result {
        Ok(decode_warning) => {
            progressive::complete(h);
            if let Some(w) = decode_warning {
                log_warn!(
                    "StepMania: '{}' audio decode: {} -- rest of the bank silent; flagged audio_failed",
                    b,
                    w
                );
                index::mark_audio_failed(b);
            }
            Ok(format!(
                "{} KiB{}",
                plan.total_len() / 1024,
                if h.rename_pending.load(Ordering::Acquire) {
                    ", rename deferred"
                } else {
                    ""
                }
            ))
        }
        Err(e) if e == "cancelled" => {
            abandon_full_bank(h, "highlight moved on");
            Err(e)
        }
        Err(e) => {
            abandon_full_bank(h, "encode failed");
            index::mark_audio_failed(b);
            Err(e)
        }
    }
}

fn abandon_full_bank(h: &ProgressiveFile, why: &str) {
    cache::lru_remove(&cache::xwb_key(&h.basename, BankVariant::Full));
    progressive::abandon(h, why);
}

/// Buffered sequential sink over the `.part` file that publishes the
/// contiguous byte count reaching the OS (monotonic) after every flush —
/// what the read hook waits on. `SongBankWriter` only ever seeks to 0
/// (header) and then writes forward.
struct WatermarkSink<'a> {
    file: File,
    buf: Vec<u8>,
    pos: u64,
    watermark: &'a AtomicU64,
}

impl<'a> WatermarkSink<'a> {
    fn new(file: File, watermark: &'a AtomicU64) -> Self {
        Self {
            file,
            buf: Vec::with_capacity(PROGRESSIVE_FLUSH_BYTES + song_bank::BLOCK_BYTES),
            pos: 0,
            watermark,
        }
    }

    fn flush_buf(&mut self) -> io::Result<()> {
        if !self.buf.is_empty() {
            self.file.write_all(&self.buf)?;
            self.pos += self.buf.len() as u64;
            self.buf.clear();
        }
        self.watermark.fetch_max(self.pos, Ordering::AcqRel);
        Ok(())
    }

    fn into_inner(self) -> File {
        self.file
    }
}

impl Write for WatermarkSink<'_> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(data);
        if self.buf.len() >= PROGRESSIVE_FLUSH_BYTES {
            self.flush_buf()?;
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flush_buf()?;
        self.file.flush()
    }
}

impl Seek for WatermarkSink<'_> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.flush_buf()?;
        let new = self.file.seek(pos)?;
        self.pos = new;
        Ok(new)
    }
}

// ---------------------------------------------------------------------
// Jackets
// ---------------------------------------------------------------------

fn serve_jacket(entry: &Arc<SongEntry>) -> Option<String> {
    let b = entry.basename.as_str();
    let key = cache::jacket_key(b);
    let path = cache::jacket_path(b);
    let marker = cache::jacket_placeholder_marker(b);
    if path.is_file() && !marker.is_file() {
        cache::lru_touch(&key);
        return Some(cache::to_game_path(&path));
    }
    let ticket = scheduler::enqueue(JobKind::Jacket, b, Urgency::HighlightedJacket, false);
    if !widget_renderer::is_render_thread() {
        ticket.wait(RESOLVER_WAIT);
        if path.is_file() && !marker.is_file() {
            cache::lru_touch(&key);
            return Some(cache::to_game_path(&path));
        }
        if !WARNED_JACKET_LATE.swap(true, Ordering::AcqRel) {
            log_warn!(
                "StepMania: jacket '{}' not ready within {} ms -- serving the placeholder (further cases silent)",
                b,
                RESOLVER_WAIT.as_millis()
            );
        }
    }
    // Placeholder arc (tiny) + `.ph` marker; the job upgrades it later.
    let _guard = JACKET_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    if path.is_file() {
        return Some(cache::to_game_path(&path));
    }
    let arc = art::jacket_arc(b, art::PLACEHOLDER_JACKET_PNG.to_vec());
    if !cache::write_atomic(&path, &arc) {
        return None;
    }
    let _ = std::fs::write(&marker, b"placeholder");
    cache::lru_insert(&key, arc.len() as u64);
    Some(cache::to_game_path(&path))
}

/// Compose `jacket/<b>_jk.arc` (or upgrade its placeholder). Returns the
/// log detail.
pub(super) fn run_jacket_job(entry: &SongEntry) -> Result<String, String> {
    let b = entry.basename.as_str();
    let key = cache::jacket_key(b);
    let path = cache::jacket_path(b);
    let marker = cache::jacket_placeholder_marker(b);
    if path.is_file() && !marker.is_file() {
        return Ok("already present".into());
    }
    let composed = panic::catch_unwind(AssertUnwindSafe(|| {
        let root = super::songs_root()?;
        let square = index::compose_art_for_entry(&root, entry);
        art::jacket::encode_png(&square)
    }));
    let (png, fallback) = match composed {
        Ok(Some(png)) => (png, false),
        _ => (art::PLACEHOLDER_JACKET_PNG.to_vec(), true),
    };
    let arc = art::jacket_arc(b, png);
    let _guard = JACKET_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    if !cache::write_atomic(&path, &arc) {
        return Err("could not write the jacket arc".into());
    }
    let _ = std::fs::remove_file(&marker);
    cache::lru_insert(&key, arc.len() as u64);
    Ok(format!(
        "{} KiB{}",
        arc.len() / 1024,
        if fallback { ", placeholder art" } else { "" }
    ))
}
