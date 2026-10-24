//! Auto-sync sweep — measures every injected song once per source
//! fingerprint against its audio, on a below-normal-priority thread after
//! boot, and hands the result to `sync_apply` (ledger + live value). It
//! measures only: no SSQ or audio file is ever written — the correction is
//! applied at play time through the game's judgement offset (design §4.5
//! as reworked 2026-10-24; `sync_apply.rs`). Gated by
//! `stepmania_packs.auto_sync`: with it off no thread starts.
//!
//! ## Thread
//! One thread at `THREAD_PRIORITY_BELOW_NORMAL`, started by the mod's
//! `enable` (`stepmania::start_sync_sweep`). **Start signal**: the index
//! has completed AND `scene_manager::current_scene() ≥ WARNING_SPLASH`
//! (the boot pass — `CheckStepDataActor` opening every SSQ — runs in
//! scenes 0/1), then a short settle. Without `scene_manager` the sweep
//! never starts (it could not stay out of gameplay; on-decide measurement
//! and the provider still work). Between songs it waits while the scene is
//! 26/27/28 (stage interstitial, stage indicator, gameplay) and while an
//! on-decide measurement is queued or running (`sync_apply`), so a
//! player's song is never behind a sweep song. Each song's work runs under
//! `catch_unwind`; a panic publishes `Refused{panicked}`.
//!
//! ## Per song ([`measure_song`])
//! Skipped when `audio_failed` or when a valid record exists (live entry).
//! Otherwise the simfile is re-parsed with `core::sm`, the audio re-decoded
//! with `decode_whole` (the full-bank build's decode is NOT shared — the
//! progressive writer streams blocks and never holds the whole song), one
//! onset envelope is computed, and every materialised file (`<b>.ssq`, or
//! each `<b>_N.ssq`) is measured through `core::sync::measure` — the same
//! path `tools/sm_pack_check --auto-sync` uses (lead-in subtracted from the
//! file's anchors). `sync_apply::publish_song` reduces the files to the
//! song's one delta (`sync_state::decide_song`) and logs one INFO per song;
//! one summary per pass.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::core::audio::decode::decode_whole;
use crate::core::audio::probe;
use crate::core::sm;
use crate::core::sync::measure::SongAnalysis;
use crate::core::sync::{AudioView, Outcome, Params};
use crate::mods::per_song_judgement_offsets;
use crate::services::scene_manager;
use crate::types::scenes::scene;
use crate::{log_debug, log_info, log_warn};

use super::convert;
use super::index;
use super::model::SongEntry;
use super::sync_apply;
use super::sync_state::FileResult;

/// Poll interval while the scene is 26–28 / before the start signal / while
/// an on-decide measurement is pending.
const PAUSE_POLL: Duration = Duration::from_millis(250);
/// Settle after the start signal before the first measurement.
const START_SETTLE: Duration = Duration::from_secs(5);
/// Longest audio measured (the whole song is decoded into memory: 4 bytes
/// per frame plus the analysis' mono copy).
const MAX_MEASURE_S: f64 = 20.0 * 60.0;

static STARTED: AtomicBool = AtomicBool::new(false);

/// Start the sweep thread (once). `false` when not started: `auto_sync`
/// off, `scene_manager` unavailable, or the spawn failed.
pub fn start() -> bool {
    if !super::auto_sync_enabled() {
        return false;
    }
    if !scene_manager::is_available() {
        log_warn!(
            "StepMania[sync]: scene_manager unavailable -- auto-sync sweep not started (it could not stay out of gameplay); songs are measured when picked"
        );
        return false;
    }
    if STARTED.swap(true, Ordering::AcqRel) {
        return true;
    }
    let spawned = std::thread::Builder::new()
        .name("stepmania-sync".into())
        .stack_size(4 << 20)
        .spawn(|| {
            lower_priority();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run));
            if r.is_err() {
                log_warn!("StepMania[sync]: sweep panicked -- stopped for this boot");
            }
        });
    match spawned {
        Ok(_) => {
            log_info!(
                "StepMania[sync]: auto-sync sweep armed (below-normal priority; starts after boot, paused in scenes 26-28)"
            );
            true
        }
        Err(e) => {
            STARTED.store(false, Ordering::Release);
            log_warn!("StepMania[sync]: sweep thread failed to start: {}", e);
            false
        }
    }
}

fn lower_priority() {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
        };
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

fn in_stage(scene_id: i32) -> bool {
    matches!(
        scene_id,
        scene::SONG_TO_STAGE_INTERSTITIAL | scene::STAGE_INDICATOR | scene::GAMEPLAY
    )
}

/// Block while the game is in a stage (26–28) or an on-decide measurement
/// is pending.
fn wait_until_clear() {
    while in_stage(scene_manager::current_scene()) || sync_apply::on_decide_pending() {
        std::thread::sleep(PAUSE_POLL);
    }
}

#[derive(Default)]
struct PassStats {
    measured: usize,
    refused: usize,
    failed: usize,
    skipped: usize,
}

fn run() {
    // Index complete (bounded waits, re-armed until it is).
    loop {
        if index::wait_ready(Duration::from_secs(60)).complete {
            break;
        }
    }
    // Boot pass over.
    while scene_manager::current_scene() < scene::WARNING_SPLASH {
        std::thread::sleep(PAUSE_POLL);
    }
    std::thread::sleep(START_SETTLE);
    if !per_song_judgement_offsets::is_active() {
        log_warn!(
            "StepMania[sync]: per-song-judgement-offsets is not active -- auto-sync measures but cannot be applied (songs play as authored)"
        );
    }
    let (dropped, seeded) = sync_apply::sync_index();
    if dropped > 0 || seeded > 0 {
        log_info!(
            "StepMania[sync]: ledger synced with the index -- {} stale record(s) dropped, {} loaded",
            dropped,
            seeded
        );
    }
    let entries = super::injected_entries();
    if entries.is_empty() {
        log_info!("StepMania[sync]: no injected songs -- sweep idle");
        return;
    }
    let started = Instant::now();
    let mut stats = PassStats::default();
    let mut queue: VecDeque<Arc<SongEntry>> = entries.iter().cloned().collect();
    while let Some(entry) = queue.pop_front() {
        if entry.audio_failed || sync_apply::is_measured(&entry) {
            stats.skipped += 1;
            continue;
        }
        wait_until_clear();
        // An on-decide measurement may have covered it while we waited.
        if sync_apply::is_measured(&entry) || !sync_apply::begin_measurement(&entry.basename) {
            stats.skipped += 1;
            continue;
        }
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| measure_song(&entry)));
        match r {
            Ok(Ok(m)) => {
                if sync_apply::publish_song(&entry, &m.files, &m.measured_ms, &m.detail) {
                    stats.measured += 1;
                } else {
                    stats.refused += 1;
                }
            }
            Ok(Err(reason)) => {
                stats.refused += 1;
                sync_apply::refuse_song(&entry, reason);
            }
            Err(_) => {
                stats.failed += 1;
                log_warn!(
                    "StepMania[sync]: '{}/{}' measurement panicked -- refused this boot",
                    entry.pack,
                    entry.folder
                );
                sync_apply::refuse_song(&entry, "panicked");
            }
        }
        sync_apply::end_measurement(&entry.basename);
    }
    log_info!(
        "StepMania[sync]: pass complete -- {} song(s) measured, {} refused, {} failed, {} already known; {} ms",
        stats.measured,
        stats.refused,
        stats.failed,
        stats.skipped,
        started.elapsed().as_millis()
    );
}

/// The per-file results of one song's measurement.
pub(super) struct SongMeasurement {
    pub files: Vec<FileResult>,
    /// `(file index, measured_ms)` for the files the estimator could
    /// measure.
    pub measured_ms: Vec<(u8, f64)>,
    /// Log detail (events, files, analysis time).
    pub detail: String,
}

/// `(file index, cache path)` of every SSQ file the song materialised.
fn files_of(entry: &SongEntry) -> Vec<(u8, PathBuf)> {
    if entry.has_split_files() {
        (1..=5u8)
            .filter(|&n| {
                entry
                    .split_levels
                    .get(usize::from(n - 1))
                    .is_some_and(|&m| m != 0)
            })
            .map(|n| (n, super::cache::ssq_split_path(&entry.basename, n)))
            .collect()
    } else {
        vec![(0, super::cache::ssq_path(&entry.basename))]
    }
}

/// Measure every file of `entry` against its audio (module docs). `Err` is
/// a song-level refusal reason (`no_simfile`, `decode_failed`, `too_long`).
/// Read-only: nothing under the cache is written here.
pub(super) fn measure_song(entry: &SongEntry) -> Result<SongMeasurement, &'static str> {
    let files = files_of(entry);
    let root = super::songs_root().ok_or("no_songs_root")?;
    let song_dir = root.join(&entry.pack).join(&entry.folder);
    let simfile = convert::find_simfile(&song_dir).ok_or("no_simfile")?;
    let sim_bytes = std::fs::read(&simfile).map_err(|_| "no_simfile")?;
    let hint = simfile
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("x.sm")
        .to_string();
    let song = sm::parse_bytes(&hint, &sim_bytes).ok_or("no_simfile")?;
    let audio_path = song_dir.join(&entry.audio.rel_path);
    let audio_bytes =
        index::read_capped(&audio_path, index::MAX_AUDIO_BYTES).ok_or("decode_failed")?;
    let info = probe::probe(&audio_bytes).map_err(|_| "decode_failed")?;
    if info.duration_s() > MAX_MEASURE_S {
        return Err("too_long");
    }
    let rate = info.target_rate();
    if rate == 0 || rate != entry.audio.rate {
        return Err("decode_failed");
    }
    let started = Instant::now();
    let pcm = decode_whole(&audio_bytes, &info).map_err(|_| "decode_failed")?;
    drop(audio_bytes);
    let analysis = SongAnalysis::new(&AudioView {
        samples: &pcm,
        sample_rate: rate,
        channels: 2,
    });
    drop(pcm);
    let analysis_ms = started.elapsed().as_millis();
    let lead_in_s = entry.audio.lead_in_frames as f64 / f64::from(rate);
    let mut results = Vec::with_capacity(files.len());
    let mut measured_ms = Vec::with_capacity(files.len());
    let mut events = 0usize;
    for (index, path) in &files {
        let Ok(bytes) = std::fs::read(path) else {
            results.push(FileResult {
                index: *index,
                outcome: Err("unreadable".to_string()),
            });
            continue;
        };
        let m = analysis.measure(&song, &bytes, lead_in_s, &Params::default());
        let e = &m.estimate;
        if let Some(ms) = e.measured_ms {
            measured_ms.push((*index, ms));
        }
        events = events.max(e.events_used);
        let outcome = match e.outcome {
            Outcome::Apply { delta_ms } => Ok(delta_ms),
            Outcome::Unchanged => Ok(0),
            Outcome::Refused(r) => Err(r.key().to_string()),
        };
        log_debug!(
            "StepMania[sync]: '{}/{}' {} {}",
            entry.pack,
            entry.folder,
            convert::ssq_file_name(&entry.basename, *index),
            describe_estimate(e)
        );
        results.push(FileResult {
            index: *index,
            outcome,
        });
    }
    let detail = format!(
        "{} events, {} file(s), analysed in {} ms",
        events,
        files.len(),
        analysis_ms
    );
    Ok(SongMeasurement {
        files: results,
        measured_ms,
        detail,
    })
}

fn describe_estimate(e: &crate::core::sync::SyncEstimate) -> String {
    let measured = match (e.measured_ms, e.correction_ms) {
        (Some(ms), Some(c)) => format!("measured {ms:+.2} ms (correction {c:+.2} ms) -> "),
        _ => String::new(),
    };
    let tail = match e.outcome {
        Outcome::Apply { delta_ms } => format!("delta {delta_ms:+} ms"),
        Outcome::Unchanged => "in sync".to_string(),
        Outcome::Refused(r) => format!("refused ({})", r.key()),
    };
    format!("{measured}{tail}, {} events", e.events_used)
}
