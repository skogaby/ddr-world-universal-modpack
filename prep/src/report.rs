//! Console reporting for the default run: the indexer's progress sink, the
//! injection-ceiling report and the closing summary. Everything goes through
//! the `log_*` macros so the log file matches the console.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::indexer::{Outcome, PackStats, Sink};
use crate::measure;
use crate::services::stepmania::ceiling::{self, Budget, Selection};
use crate::services::stepmania::model::SongEntry;
use crate::{log_debug, log_info, log_warn};

/// The indexer's progress on the console.
#[derive(Default)]
pub struct ConsoleSink {
    packs_total: AtomicUsize,
    packs_done: AtomicUsize,
    converted: AtomicUsize,
    /// Test hook: stop after this many conversions.
    pub abort_after: Option<usize>,
}

impl ConsoleSink {
    pub fn new(abort_after: Option<usize>) -> Self {
        Self {
            abort_after,
            ..Self::default()
        }
    }
}

impl Sink for ConsoleSink {
    fn should_abort(&self) -> bool {
        self.abort_after
            .is_some_and(|n| self.converted.load(Ordering::Relaxed) >= n)
    }

    fn discovered(&self, packs: usize, songs: usize, to_convert: usize) {
        self.packs_total.store(packs, Ordering::Relaxed);
        if to_convert == 0 {
            log_info!("{songs} song(s) in {packs} pack(s); everything is already cached");
        } else {
            log_info!(
                "{songs} song(s) in {packs} pack(s); {to_convert} to convert (~{} s at 25 ms/song)",
                (to_convert * 25).div_ceil(1000)
            );
        }
    }

    fn pack_done(&self, pack: &str, stats: &PackStats) {
        let done = self.packs_done.fetch_add(1, Ordering::Relaxed) + 1;
        let total = self.packs_total.load(Ordering::Relaxed);
        log_info!(
            "[{done:>3}/{total:<3}] {pack} -- {} song(s): {} reused, {} converted, {} rejected",
            stats.total,
            stats.reused,
            stats.converted,
            stats.rejected
        );
    }

    fn rejected(&self, pack: &str, folder: &str, reason: &str) {
        log_warn!("rejected '{pack}/{folder}': {reason}");
    }

    fn note(&self, pack: &str, folder: &str, what: &str) {
        log_warn!("'{pack}/{folder}': {what}");
    }

    fn converted(&self, entry: &SongEntry) {
        self.converted.fetch_add(1, Ordering::Relaxed);
        log_debug!(
            "converted '{}/{}' -> {} (mcode {})",
            entry.pack,
            entry.folder,
            entry.basename,
            entry.mcode
        );
    }

    fn checkpoint(&self, songs_in_file: usize) {
        log_debug!("checkpoint: index.bin holds {songs_in_file} song(s)");
    }
}

/// The measurement pass on the console. No up-front estimate (per-song
/// cost varies too much with machine, codec and length to guess); instead
/// the progress line carries the remaining time extrapolated from the
/// throughput so far, every 100 songs and at least every ~10 s.
pub struct MeasureConsole {
    total: AtomicUsize,
    done: AtomicUsize,
    started: std::sync::Mutex<Option<std::time::Instant>>,
    last_report: std::sync::Mutex<Option<std::time::Instant>>,
}

impl Default for MeasureConsole {
    fn default() -> Self {
        Self {
            total: AtomicUsize::new(0),
            done: AtomicUsize::new(0),
            started: std::sync::Mutex::new(None),
            last_report: std::sync::Mutex::new(None),
        }
    }
}

/// Progress heartbeat interval.
const PROGRESS_EVERY: std::time::Duration = std::time::Duration::from_secs(10);

impl measure::Sink for MeasureConsole {
    fn started(&self, eligible: usize, known: usize) {
        self.total.store(eligible, Ordering::Relaxed);
        let now = std::time::Instant::now();
        if let Ok(mut s) = self.started.lock() {
            *s = Some(now);
        }
        if let Ok(mut l) = self.last_report.lock() {
            *l = Some(now);
        }
        if eligible == 0 {
            log_info!("auto-sync: every injected song is measured already ({known} record(s))");
        } else {
            log_info!("auto-sync: measuring {eligible} song(s) ({known} already measured) ...");
        }
    }

    fn song_done(&self, entry: &SongEntry, summary: &str) {
        let done = self.done.fetch_add(1, Ordering::Relaxed) + 1;
        let total = self.total.load(Ordering::Relaxed);
        log_debug!(
            "[{done:>4}/{total:<4}] sync '{}/{}': {summary}",
            entry.pack,
            entry.folder
        );
        let now = std::time::Instant::now();
        let due = self
            .last_report
            .lock()
            .ok()
            .and_then(|l| *l)
            .is_none_or(|l| now.duration_since(l) >= PROGRESS_EVERY);
        if done == total || done % 100 == 0 || due {
            if let Ok(mut l) = self.last_report.lock() {
                *l = Some(now);
            }
            let elapsed = self
                .started
                .lock()
                .ok()
                .and_then(|s| *s)
                .map(|s| now.duration_since(s))
                .unwrap_or_default();
            if done == total {
                log_info!(
                    "auto-sync: {done}/{total} measured ({})",
                    human_duration(elapsed)
                );
            } else {
                let per_song = elapsed.as_secs_f64() / done as f64;
                let left = std::time::Duration::from_secs_f64(per_song * (total - done) as f64);
                log_info!(
                    "auto-sync: {done}/{total} measured ({} elapsed, about {} left)",
                    human_duration(elapsed),
                    human_duration(left)
                );
            }
        }
    }

    fn checkpoint(&self, records: usize) {
        log_debug!("checkpoint: sync.bin holds {records} record(s)");
    }
}

/// `about 2 min` / `about 45 s` / `about 1.5 h`.
pub fn human_duration(d: std::time::Duration) -> String {
    let s = d.as_secs_f64();
    if s < 90.0 {
        format!("{} s", s.round() as u64)
    } else if s < 5400.0 {
        format!("{} min", (s / 60.0).round() as u64)
    } else {
        format!("{:.1} h", s / 3600.0)
    }
}

/// The closing line of the measurement pass.
pub fn measure_summary(st: &measure::Stats, elapsed: std::time::Duration) -> String {
    format!(
        "auto-sync {} -- {} measured, {} refused ({} audio failed, now rejected), {} already known, {}",
        if st.aborted { "INTERRUPTED" } else { "complete" },
        st.measured,
        st.refused,
        st.failed,
        st.known,
        human_duration(elapsed)
    )
}

/// The closing line of the index pass.
pub fn index_summary(o: &Outcome) -> String {
    format!(
        "index {} -- {} song(s), {} pack(s), {} rejected ({}; {} converted, {} reused), {:.1} s, {} worker(s)",
        if o.aborted { "INTERRUPTED" } else { "complete" },
        o.file.songs.len(),
        o.packs,
        o.rejected,
        if o.cold { "cold" } else { "warm" },
        o.converted,
        o.reused,
        o.elapsed.as_secs_f64(),
        o.workers
    )
}

/// What the game will inject from `entries`, as console lines, plus the
/// selection itself (the measurement pass works on the same kept set).
pub fn ceiling_report(entries: &[SongEntry], song_limit_enabled: bool) -> (Vec<String>, Selection) {
    let budget = Budget {
        arena_bytes: ceiling::EXPANDED_ARENA_BYTES,
        max_songs: ceiling::MAX_INJECTED_SONGS,
    };
    let sel = ceiling::select(entries, &budget);
    let mut lines = Vec::new();
    if entries.is_empty() {
        lines.push("the game will inject no StepMania songs (none indexed)".to_string());
        return (lines, sel);
    }
    if !song_limit_enabled {
        lines.push(format!(
            "Song Limit Expansion is OFF in mod-config.json: the game injects NONE of the {} indexed song(s) until it is on",
            entries.len()
        ));
    }
    if sel.over_count == 0 {
        lines.push(format!(
            "the game will inject all {} indexed song(s) (ceiling {})",
            sel.kept.len(),
            budget.max_songs
        ));
        return (lines, sel);
    }
    lines.push(format!(
        "{} indexed song(s) exceed the {}-song ceiling: the game injects the first {} by pack/folder name and leaves {} out",
        entries.len(),
        budget.max_songs,
        sel.kept.len(),
        sel.over_count
    ));
    // Per pack: kept / total, for the packs that lost songs.
    let mut per_pack: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for e in entries {
        per_pack.entry(e.pack.as_str()).or_default().1 += 1;
    }
    for &i in &sel.kept {
        if let Some(e) = entries.get(i) {
            per_pack.entry(e.pack.as_str()).or_default().0 += 1;
        }
    }
    for (pack, (kept, total)) in per_pack {
        if kept < total {
            lines.push(format!(
                "  {pack}: {} of {total} left out{}",
                total - kept,
                if kept == 0 { " (whole pack)" } else { "" }
            ));
        }
    }
    lines.push("  remove or rename packs to choose which ones the game shows".to_string());
    (lines, sel)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(pack: &str, folder: &str, n: u32) -> SongEntry {
        SongEntry {
            pack: pack.into(),
            folder: folder.into(),
            basename: format!("z{n:05}"),
            mcode: 9_000_000 + n,
            levels: [0, 0, 0, 0, 5, 0, 0, 0, 0, 0],
            ..SongEntry::default()
        }
    }

    #[test]
    fn durations_read_naturally() {
        use std::time::Duration;
        assert_eq!(human_duration(Duration::from_secs(45)), "45 s");
        assert_eq!(human_duration(Duration::from_secs(600)), "10 min");
        assert_eq!(human_duration(Duration::from_secs(7200)), "2.0 h");
    }

    #[test]
    fn report_under_the_ceiling() {
        let e = [entry("A", "1", 1), entry("B", "2", 2)];
        let (lines, sel) = ceiling_report(&e, true);
        assert_eq!(sel.kept.len(), 2);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("all 2 indexed"));
        let (lines, _) = ceiling_report(&e, false);
        assert!(lines[0].contains("Song Limit Expansion is OFF"));
        let (lines, _) = ceiling_report(&[], true);
        assert!(lines[0].contains("none indexed"));
    }

    #[test]
    fn report_names_the_packs_left_out() {
        // 4,010 songs: pack "A" 4,000 + pack "B" 10 → B wholly left out.
        let mut e: Vec<SongEntry> = (0..4_000u32)
            .map(|i| entry("A", &format!("{i:05}"), i))
            .collect();
        e.extend((0..10u32).map(|i| entry("B", &format!("{i}"), 5_000 + i)));
        let (lines, sel) = ceiling_report(&e, true);
        assert_eq!(sel.kept.len(), ceiling::MAX_INJECTED_SONGS);
        assert!(lines[0].contains("leaves 10 out"));
        assert!(lines
            .iter()
            .any(|l| l.contains("B: 10 of 10 left out (whole pack)")));
        assert!(!lines.iter().any(|l| l.starts_with("  A:")));
    }
}
