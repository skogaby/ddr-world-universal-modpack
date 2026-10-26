//! Auto-sync measurement — the hook's former in-game sweep, now the tool's
//! and parallel.
//!
//! For every song the game will inject (the ceiling's kept set) that has no
//! `sync.bin` record for its current source fingerprint, the simfile is
//! re-parsed with `core::sm`, the audio fully decoded, one onset envelope
//! computed and every materialised SSQ file measured through
//! `core::sync::measure`; `sync_state::decide_song` reduces the files to the
//! song's ONE record (`Measured{delta}` or `Refused{reason}`), which the DLL
//! applies at play time through the judgement offset. A song whose audio
//! fails to decode is additionally marked `audio_failed` in `index.bin` so
//! the game rejects it (the DLL never writes that flag itself).
//!
//! `sync.bin` is rewritten (atomically) every `checkpoint_every` songs and
//! at the end; stale records (song gone or fingerprint changed) are dropped
//! at load exactly as the DLL's loader does.

use std::collections::HashMap;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::core::audio::decode::decode_whole;
use crate::core::audio::probe;
use crate::core::sm;
use crate::core::sync::measure::SongAnalysis;
use crate::core::sync::{AudioView, Outcome, Params};
use crate::indexer::{read_capped, MAX_AUDIO_BYTES};
use crate::services::stepmania::cache;
use crate::services::stepmania::convert;
use crate::services::stepmania::model::{IndexFile, SongEntry};
use crate::services::stepmania::records::song_dir_of;
use crate::services::stepmania::sync_state::{
    decide_song, FileResult, SyncRecord, SyncState, SyncTable,
};
use crate::{log_debug, log_info, log_warn};

/// Longest audio measured (the whole song is decoded into memory: 4 bytes
/// per frame plus the analysis' mono copy).
pub const MAX_MEASURE_S: f64 = 20.0 * 60.0;
/// Rough CPU cost per song, for the estimate line.
pub const SECONDS_PER_SONG: f64 = 0.8;

#[derive(Debug, Clone, Copy)]
pub struct Settings {
    pub workers: usize,
    pub checkpoint_every: usize,
}

/// Progress events. All methods have no-op defaults.
pub trait Sink: Sync {
    fn started(&self, _eligible: usize, _known: usize, _estimate: Duration) {}
    fn song_done(&self, _entry: &SongEntry, _summary: &str) {}
    fn checkpoint(&self, _records: usize) {}
    fn should_abort(&self) -> bool {
        false
    }
}

pub struct Silent;
impl Sink for Silent {}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    /// Songs the pass set out to measure.
    pub eligible: usize,
    /// Kept songs that already had a current record.
    pub known: usize,
    pub measured: usize,
    pub refused: usize,
    /// Refused because the audio did not decode — marked `audio_failed`.
    pub failed: usize,
    pub aborted: bool,
}

/// The per-file results of one song's measurement.
pub struct SongMeasurement {
    pub files: Vec<FileResult>,
    /// `(file index, measured_ms)` for the files the estimator could measure.
    pub measured_ms: Vec<(u8, f64)>,
    pub detail: String,
}

/// `(file index, cache path)` of every SSQ file the song materialised.
pub fn files_of(entry: &SongEntry) -> Vec<(u8, PathBuf)> {
    if entry.has_split_files() {
        (1..=5u8)
            .filter(|&n| {
                entry
                    .split_levels
                    .get(usize::from(n - 1))
                    .is_some_and(|&m| m != 0)
            })
            .map(|n| (n, cache::ssq_split_path(&entry.basename, n)))
            .collect()
    } else {
        vec![(0, cache::ssq_path(&entry.basename))]
    }
}

/// Measure every file of `entry` against its audio. `Err` is a song-level
/// refusal reason (`no_simfile`, `decode_failed`, `too_long`). Read-only.
pub fn measure_song(root: &Path, entry: &SongEntry) -> Result<SongMeasurement, &'static str> {
    let files = files_of(entry);
    let song_dir = song_dir_of(root, entry);
    let simfile = convert::find_simfile(&song_dir).ok_or("no_simfile")?;
    let sim_bytes = std::fs::read(&simfile).map_err(|_| "no_simfile")?;
    let hint = simfile
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("x.sm")
        .to_string();
    let song = sm::parse_bytes(&hint, &sim_bytes).ok_or("no_simfile")?;
    let audio_path = song_dir.join(&entry.audio.rel_path);
    let audio_bytes = read_capped(&audio_path, MAX_AUDIO_BYTES).ok_or("decode_failed")?;
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
            "sync '{}/{}' {} {}",
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
    Ok(SongMeasurement {
        files: results,
        measured_ms,
        detail: format!(
            "{} events, {} file(s), analysed in {} ms",
            events,
            files.len(),
            analysis_ms
        ),
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

/// One-line summary of a decided song for the sink.
fn summarize(files: &[FileResult], measured: &[(u8, f64)]) -> (SyncRecordState, String) {
    let Some(decision) = decide_song(files) else {
        return (
            SyncRecordState::Refused("no_files".into()),
            "refused (no_files)".into(),
        );
    };
    let picked_ms = measured
        .iter()
        .find(|(i, _)| *i == decision.picked)
        .map(|(_, m)| *m);
    let text = match (&decision.state, picked_ms) {
        (SyncState::Measured { delta_ms }, Some(m)) => {
            format!("measured {m:+.2} ms -> delta {delta_ms} ms")
        }
        (SyncState::Measured { delta_ms }, None) => format!("measured -> delta {delta_ms} ms"),
        (SyncState::Refused { reason }, _) => format!("refused ({reason}) -- plays as authored"),
    };
    let text = if decision.disagree {
        format!("{text}; split files disagree by more than 2 ms")
    } else {
        text
    };
    (SyncRecordState::Decided(decision.state), text)
}

enum SyncRecordState {
    Decided(SyncState),
    Refused(String),
}

impl SyncRecordState {
    fn into_state(self) -> SyncState {
        match self {
            Self::Decided(s) => s,
            Self::Refused(reason) => SyncState::Refused { reason },
        }
    }
}

/// Measure the kept songs that lack a current record; update `sync.bin`;
/// mark decode failures `audio_failed` in `index` (the caller rewrites
/// `index.bin` when `Stats::failed > 0`).
pub fn run(
    root: &Path,
    index: &mut IndexFile,
    kept: &[usize],
    settings: &Settings,
    sink: &dyn Sink,
) -> Stats {
    let mut stats = Stats::default();
    let fingerprints: HashMap<String, u64> = index
        .songs
        .iter()
        .map(|e| (e.basename.clone(), e.fingerprint))
        .collect();
    let mut table = SyncTable::parse_or_empty(std::fs::read(cache::sync_path()).ok().as_deref());
    let dropped = table.drop_stale(&fingerprints);
    if dropped > 0 {
        log_info!("sync.bin: {dropped} stale record(s) dropped");
    }

    let mut queue: Vec<SongEntry> = Vec::new();
    for &i in kept {
        let Some(e) = index.songs.get(i) else {
            continue;
        };
        if e.audio_failed {
            continue;
        }
        if table.current(&e.basename, e.fingerprint).is_some() {
            stats.known += 1;
            continue;
        }
        queue.push(e.clone());
    }
    stats.eligible = queue.len();
    let workers = settings.workers.max(1).min(queue.len().max(1));
    let estimate = Duration::from_secs_f64(queue.len() as f64 * SECONDS_PER_SONG / workers as f64);
    sink.started(stats.eligible, stats.known, estimate);
    if queue.is_empty() {
        save(&table);
        return stats;
    }

    queue.reverse(); // pop() takes the first by kept order
    let queue = Mutex::new(queue);
    let (tx, rx) = mpsc::channel::<(SongEntry, Result<SongMeasurement, &'static str>)>();
    let ckpt_every = settings.checkpoint_every.max(1);
    let mut since = 0usize;
    let mut failed_basenames: Vec<String> = Vec::new();

    std::thread::scope(|scope| {
        for _ in 0..workers {
            let tx = tx.clone();
            let queue = &queue;
            scope.spawn(move || loop {
                if sink.should_abort() {
                    break;
                }
                let entry = match queue.lock() {
                    Ok(mut q) => q.pop(),
                    Err(_) => None,
                };
                let Some(entry) = entry else {
                    break;
                };
                let outcome = panic::catch_unwind(AssertUnwindSafe(|| measure_song(root, &entry)))
                    .unwrap_or(Err("panicked"));
                if tx.send((entry, outcome)).is_err() {
                    break;
                }
            });
        }
        drop(tx);
        for (entry, outcome) in rx {
            let (state, summary) = match outcome {
                Ok(m) => summarize(&m.files, &m.measured_ms),
                Err(reason) => (
                    SyncRecordState::Refused(reason.to_string()),
                    format!("refused ({reason}) -- plays as authored"),
                ),
            };
            if matches!(&state, SyncRecordState::Refused(r) if r == "decode_failed") {
                failed_basenames.push(entry.basename.clone());
                stats.failed += 1;
            }
            let state = state.into_state();
            match &state {
                SyncState::Measured { .. } => stats.measured += 1,
                SyncState::Refused { .. } => stats.refused += 1,
            }
            if !table.insert(
                &entry.basename,
                SyncRecord {
                    source_fingerprint: entry.fingerprint,
                    state,
                },
            ) {
                log_warn!(
                    "sync '{}/{}': result not storable (key/reason) -- will be re-measured",
                    entry.pack,
                    entry.folder
                );
            }
            sink.song_done(&entry, &summary);
            since += 1;
            if since >= ckpt_every {
                save(&table);
                sink.checkpoint(table.len());
                since = 0;
            }
        }
        stats.aborted = queue.lock().map(|q| !q.is_empty()).unwrap_or(false);
    });

    save(&table);
    for s in &mut index.songs {
        if failed_basenames.contains(&s.basename) {
            s.audio_failed = true;
        }
    }
    stats
}

fn save(table: &SyncTable) {
    if !cache::write_atomic(&cache::sync_path(), &table.serialize()) {
        log_warn!("could not save sync.bin -- results will be re-measured");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_of_names_split_files_only_for_declared_levels() {
        let mut e = SongEntry {
            basename: "zabcde".into(),
            ..SongEntry::default()
        };
        assert_eq!(files_of(&e).len(), 1);
        assert!(files_of(&e)[0].1.ends_with("ssq/zabcde.ssq"));
        e.split_levels = [0, 3, 0, 9, 0];
        let f = files_of(&e);
        assert_eq!(f.iter().map(|(i, _)| *i).collect::<Vec<_>>(), vec![2, 4]);
        assert!(f[0].1.ends_with("ssq/zabcde_2.ssq"));
    }

    #[test]
    fn summary_text() {
        let (st, text) = summarize(
            &[FileResult {
                index: 0,
                outcome: Ok(4),
            }],
            &[(0, 3.6)],
        );
        assert!(matches!(
            st.into_state(),
            SyncState::Measured { delta_ms: 4 }
        ));
        assert_eq!(text, "measured +3.60 ms -> delta 4 ms");
        let (st, text) = summarize(
            &[FileResult {
                index: 0,
                outcome: Err("too_few_events".into()),
            }],
            &[],
        );
        assert!(matches!(st.into_state(), SyncState::Refused { .. }));
        assert!(text.starts_with("refused (too_few_events)"));
    }
}
