//! Auto-sync, the apply side: the ledger (`sync.bin`), the live per-song
//! delta map with its freeze rule, the on-decide measurer and the two
//! bounded holds. Nothing here writes a song file — a correction is one
//! number, handed to `per_song_judgement_offsets` at play time (design
//! §4.5 as reworked 2026-10-24).
//!
//! ```text
//! sweep / on-decide measurer ──publish──► LiveMap {delta, frozen} ──► ledger (sync.bin)
//!                                              │
//! scene 25→26 (committed song)  ─request─►  on-decide queue (normal-priority thread)
//! full-bank request (stage load) ─hold ≤ 5 s─┤   (also the fallback request)
//! first judge dispatch (per side) ─hold ≤ 3 s─┴─► freeze → delta → compose(entry, stock, delta)
//! ```
//!
//! ## Live value and freeze ([`super::sync_state::LiveMap`])
//! At [`init`] every valid `sync.bin` record seeds the map unfrozen
//! (`Measured` → `Some(d)`, `Refused` → `None`); the sweep seeds the rest
//! once the index is complete ([`sync_index`]). A play's first
//! [`delta_for_play`] freezes the song's entry with whatever it has — the
//! measured delta, `None` for a refusal, or `None` when the measurement did
//! not arrive in time — and every later caller (the other side, a replay)
//! gets that value. A measurement that lands after the freeze only reaches
//! the ledger (INFO `measured late; applies from the next boot`).
//!
//! ## On-decide measurement
//! [`request_on_decide`] (the scheduler's 25→26 transition with the dwell
//! basename; `stepmania::on_gameplay_load` as the fallback for paths
//! without a highlight) queues an unmeasured, non-`audio_failed` song for a
//! dedicated NORMAL-priority thread (`stepmania-sync-decide`), which runs
//! `sync_sweep::measure_song` under `catch_unwind` and publishes the result
//! (a panic publishes `Refused{panicked}` so waiters stop waiting). The
//! in-flight set is shared with the sweep: a song is never measured twice
//! concurrently, and the sweep does not START a new song while an on-decide
//! request is pending ([`on_decide_pending`]).
//!
//! ## Holds (both only for an injected song with a measurement pending)
//! - **Stage load** — `providers::serve_full_bank` calls
//!   [`hold_for_measurement`] (≤ [`STAGE_LOAD_HOLD`]) on the FileManager
//!   I/O worker before serving the bank; the preview variant, SSQ opens,
//!   jackets and `chart_length` are never held.
//! - **First judge** — the provider ([`delta_provider`]) waits ≤
//!   [`FIRST_JUDGE_HOLD`] on the game thread.
//!
//! On expiry the song is frozen as authored (one WARN); the measurement
//! still completes into the ledger for the next boot.
//!
//! `auto_sync: false`: [`init`] does nothing, the provider is never
//! registered, [`request_on_decide`] returns at once, nothing is written.

use std::collections::{HashMap, HashSet, VecDeque};
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use crate::mods::per_song_judgement_offsets::override_hook;
use crate::{log_info, log_warn};

use super::cache;
use super::index;
use super::model::SongEntry;
use super::sync_state::{
    decide_song, FileResult, LiveMap, Publish, SyncRecord, SyncState, SyncTable,
};
use super::sync_sweep;

/// Longest the stage load (full-bank request) waits for a measurement.
pub const STAGE_LOAD_HOLD: Duration = Duration::from_secs(5);
/// Longest a side's first judge dispatch waits for a measurement.
pub const FIRST_JUDGE_HOLD: Duration = Duration::from_secs(3);

#[derive(Default)]
struct Apply {
    /// The ledger (`None` until loaded).
    table: Option<SyncTable>,
    live: LiveMap,
    /// Basenames being measured right now (sweep or on-decide).
    in_flight: HashSet<String>,
    /// On-decide requests not started yet.
    queued: VecDeque<String>,
    /// The on-decide thread's current song.
    deciding: Option<String>,
    thread_started: bool,
}

static STATE: OnceLock<Mutex<Apply>> = OnceLock::new();
/// Notified on every publish, in-flight change and queue push.
static CHANGED: Condvar = Condvar::new();
static INITIALISED: AtomicBool = AtomicBool::new(false);
/// Serialises `sync.bin` rewrites (`write_atomic` shares one `.tmp` name).
static SAVE_LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, Apply> {
    STATE
        .get_or_init(|| Mutex::new(Apply::default()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

fn table_of(st: &mut Apply) -> &mut SyncTable {
    st.table.get_or_insert_with(|| {
        SyncTable::parse_or_empty(std::fs::read(cache::sync_path()).ok().as_deref())
    })
}

/// Seed the live map from the ledger record of `basename` when the index
/// knows the song under the same fingerprint. `true` when an entry exists
/// afterwards.
fn seed_from_table(st: &mut Apply, basename: &str, fingerprint: u64) -> bool {
    if st.live.is_measured(basename) {
        return true;
    }
    let delta = table_of(st)
        .current(basename, fingerprint)
        .map(SyncRecord::live_delta);
    match delta {
        Some(d) => {
            st.live.seed(basename, d);
            true
        }
        None => false,
    }
}

/// Mod enable (`stepmania::start_sync_sweep`): load the ledger, seed the
/// live map from every record the index already confirms, register the
/// delta provider with the per-song mod. `false` when `auto_sync` is off.
/// Idempotent.
pub fn init() -> bool {
    if !super::auto_sync_enabled() {
        return false;
    }
    if INITIALISED.swap(true, Ordering::AcqRel) {
        return true;
    }
    let (records, seeded) = {
        let mut st = lock();
        let records: Vec<(String, u64)> = table_of(&mut st)
            .iter()
            .map(|(b, r)| (b.to_string(), r.source_fingerprint))
            .collect();
        let mut seeded = 0usize;
        for (b, fp) in &records {
            if index::entry_by_basename(b).is_some_and(|e| e.fingerprint == *fp)
                && seed_from_table(&mut st, b, *fp)
            {
                seeded += 1;
            }
        }
        (records.len(), seeded)
    };
    override_hook::set_song_delta_provider(delta_provider);
    log_info!(
        "StepMania[sync]: sync.bin holds {} song(s); {} live for this boot (the rest once the index completes); judgement-offset provider registered",
        records,
        seeded
    );
    true
}

/// Index complete (the sweep's first act): drop stale records, seed every
/// remaining record into the live map. Returns `(dropped, seeded)`.
pub(super) fn sync_index() -> (usize, usize) {
    let fingerprints: HashMap<String, u64> = index::snapshot()
        .songs
        .iter()
        .map(|e| (e.basename.clone(), e.fingerprint))
        .collect();
    let (dropped, seeded) = {
        let mut st = lock();
        let dropped = table_of(&mut st).drop_stale(&fingerprints);
        let records: Vec<(String, Option<i8>)> = table_of(&mut st)
            .iter()
            .map(|(b, r)| (b.to_string(), r.live_delta()))
            .collect();
        let mut seeded = 0usize;
        for (b, d) in records {
            if st.live.seed(&b, d) {
                seeded += 1;
            }
        }
        (dropped, seeded)
    };
    if dropped > 0 {
        save_table();
    }
    CHANGED.notify_all();
    (dropped, seeded)
}

fn save_table() {
    let bytes = {
        let mut st = lock();
        table_of(&mut st).serialize()
    };
    let _guard = SAVE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    if !cache::write_atomic(&cache::sync_path(), &bytes) {
        log_warn!("StepMania[sync]: could not save sync.bin -- results will be re-measured");
    }
}

/// Measured already (live entry present — including frozen as authored)?
#[must_use]
pub(super) fn is_measured(entry: &SongEntry) -> bool {
    let mut st = lock();
    seed_from_table(&mut st, &entry.basename, entry.fingerprint)
}

/// Claim `basename` for measurement; `false` when another measurer has it.
pub(super) fn begin_measurement(basename: &str) -> bool {
    lock().in_flight.insert(basename.to_string())
}

pub(super) fn end_measurement(basename: &str) {
    lock().in_flight.remove(basename);
    CHANGED.notify_all();
}

/// An on-decide request is queued or running: the sweep must not start a
/// new song.
#[must_use]
pub(super) fn on_decide_pending() -> bool {
    let st = lock();
    !st.queued.is_empty() || st.deciding.is_some()
}

fn pending(st: &Apply, basename: &str) -> bool {
    st.in_flight.contains(basename)
        || st.deciding.as_deref() == Some(basename)
        || st.queued.iter().any(|b| b == basename)
}

/// A measurement result for `entry`: decide the song's record from its
/// files, store + save it, publish the live value, log one INFO.
/// `measured` is `(file index, measured_ms)` and `detail` a suffix for the
/// log line. `true` when the song ended up `Measured`.
pub(super) fn publish_song(
    entry: &SongEntry,
    files: &[FileResult],
    measured: &[(u8, f64)],
    detail: &str,
) -> bool {
    let Some(decision) = decide_song(files) else {
        return false;
    };
    let delta = match decision.state {
        SyncState::Measured { delta_ms } => Some(delta_ms),
        SyncState::Refused { .. } => None,
    };
    let record = SyncRecord {
        source_fingerprint: entry.fingerprint,
        state: decision.state.clone(),
    };
    let published = {
        let mut st = lock();
        if !table_of(&mut st).insert(&entry.basename, record) {
            log_warn!(
                "StepMania[sync]: '{}/{}' result not storable (key/reason) -- kept for this boot only",
                entry.pack,
                entry.folder
            );
        }
        st.live.publish(&entry.basename, delta)
    };
    CHANGED.notify_all();
    save_table();
    let picked_ms = measured
        .iter()
        .find(|(i, _)| *i == decision.picked)
        .map(|(_, m)| *m);
    let suffix = if detail.is_empty() {
        String::new()
    } else {
        format!(" ({detail})")
    };
    match (&decision.state, picked_ms) {
        (SyncState::Measured { delta_ms }, Some(m)) => log_info!(
            "StepMania[sync]: '{}/{}' measured {:+.2} ms -> delta {} ms{}",
            entry.pack,
            entry.folder,
            m,
            delta_ms,
            suffix
        ),
        (SyncState::Measured { delta_ms }, None) => log_info!(
            "StepMania[sync]: '{}/{}' measured -> delta {} ms{}",
            entry.pack,
            entry.folder,
            delta_ms,
            suffix
        ),
        (SyncState::Refused { reason }, _) => log_info!(
            "StepMania[sync]: '{}/{}' refused ({}) -- plays as authored{}",
            entry.pack,
            entry.folder,
            reason,
            suffix
        ),
    }
    if decision.disagree {
        log_warn!(
            "StepMania[sync]: '{}/{}' split files disagree by more than 2 ms -- using {}'s delta for the whole song",
            entry.pack,
            entry.folder,
            file_label(decision.picked)
        );
    }
    if published == Publish::Late {
        log_info!(
            "StepMania[sync]: '{}/{}' measured late; applies from the next boot",
            entry.pack,
            entry.folder
        );
    }
    delta.is_some()
}

fn file_label(index: u8) -> String {
    if index == 0 {
        "<b>.ssq".to_string()
    } else {
        format!("<b>_{index}.ssq")
    }
}

/// Persist a refusal that is not the estimator's (decode failure, panic …).
pub(super) fn refuse_song(entry: &SongEntry, reason: &str) {
    publish_song(
        entry,
        &[FileResult {
            index: 0,
            outcome: Err(reason.to_string()),
        }],
        &[],
        "",
    );
}

// ---------------------------------------------------------------------
// On-decide measurement
// ---------------------------------------------------------------------

/// The song was committed from the wheel (or its full bank was requested):
/// measure it now unless it is measured, refused, `audio_failed`, or
/// already pending. Game-thread cheap: one lock, a map lookup, a queue push.
pub fn request_on_decide(basename: &str) {
    if !INITIALISED.load(Ordering::Acquire) {
        return;
    }
    let Some(entry) = index::entry_by_basename(basename) else {
        return;
    };
    if entry.audio_failed {
        return;
    }
    let mut st = lock();
    if seed_from_table(&mut st, basename, entry.fingerprint) || pending(&st, basename) {
        return;
    }
    st.queued.push_back(basename.to_string());
    ensure_thread(&mut st);
    drop(st);
    CHANGED.notify_all();
    log_info!(
        "StepMania[sync]: '{}/{}' committed before it was measured -- measuring now (stage load may wait up to {} s)",
        entry.pack,
        entry.folder,
        STAGE_LOAD_HOLD.as_secs()
    );
}

fn ensure_thread(st: &mut Apply) {
    if st.thread_started {
        return;
    }
    st.thread_started = true;
    let spawned = std::thread::Builder::new()
        .name("stepmania-sync-decide".into())
        .stack_size(4 << 20)
        .spawn(decide_main);
    if let Err(e) = spawned {
        st.thread_started = false;
        log_warn!(
            "StepMania[sync]: on-decide measurer failed to start: {} -- songs play as authored until the sweep reaches them",
            e
        );
    }
}

/// Normal priority on purpose: the player is waiting on this one.
fn decide_main() {
    loop {
        let basename = {
            let mut st = lock();
            loop {
                if let Some(b) = st.queued.pop_front() {
                    st.deciding = Some(b.clone());
                    break b;
                }
                st = CHANGED.wait(st).unwrap_or_else(|p| p.into_inner());
            }
        };
        measure_on_decide(&basename);
        lock().deciding = None;
        CHANGED.notify_all();
    }
}

fn measure_on_decide(basename: &str) {
    let Some(entry) = index::entry_by_basename(basename) else {
        return;
    };
    if is_measured(&entry) || !begin_measurement(basename) {
        return; // measured meanwhile, or the sweep has it (waiters wait on it)
    }
    let started = Instant::now();
    let r = panic::catch_unwind(AssertUnwindSafe(|| sync_sweep::measure_song(&entry)));
    match r {
        Ok(Ok(m)) => {
            publish_song(&entry, &m.files, &m.measured_ms, &m.detail);
        }
        Ok(Err(reason)) => refuse_song(&entry, reason),
        Err(_) => {
            log_warn!(
                "StepMania[sync]: '{}/{}' on-decide measurement panicked",
                entry.pack,
                entry.folder
            );
            refuse_song(&entry, "panicked");
        }
    }
    end_measurement(basename);
    log_info!(
        "StepMania[sync]: '{}/{}' on-decide measurement done in {} ms",
        entry.pack,
        entry.folder,
        started.elapsed().as_millis()
    );
}

// ---------------------------------------------------------------------
// Holds
// ---------------------------------------------------------------------

/// Wait (≤ `max`) while `basename` has a measurement pending and no live
/// entry. Returns the guard, and whether the song is measured now.
fn wait_measured<'a>(
    mut st: MutexGuard<'a, Apply>,
    basename: &str,
    max: Duration,
) -> (MutexGuard<'a, Apply>, bool) {
    let deadline = Instant::now() + max;
    loop {
        if st.live.is_measured(basename) {
            return (st, true);
        }
        if !pending(&st, basename) {
            return (st, false);
        }
        let now = Instant::now();
        if now >= deadline {
            return (st, false);
        }
        let (guard, _) = CHANGED
            .wait_timeout(st, deadline - now)
            .unwrap_or_else(|p| p.into_inner());
        st = guard;
    }
}

fn warn_not_in_time(basename: &str) {
    let label = index::entry_by_basename(basename)
        .map(|e| format!("{}/{}", e.pack, e.folder))
        .unwrap_or_else(|| basename.to_string());
    log_warn!(
        "StepMania[sync]: '{}' not measured in time -- played as authored this session (result applies from next boot)",
        label
    );
}

/// Stage-load hold (`providers::serve_full_bank`, FileManager I/O worker):
/// wait ≤ `max` for a pending measurement of `basename`. Nothing pending,
/// or already measured ⇒ returns at once. On expiry the song is frozen as
/// authored (one WARN) so the later judge-time provider does not wait again.
pub(super) fn hold_for_measurement(basename: &str, max: Duration) {
    if !INITIALISED.load(Ordering::Acquire) {
        return;
    }
    let st = lock();
    if st.live.is_measured(basename) || !pending(&st, basename) {
        return;
    }
    let started = Instant::now();
    let (mut st, measured) = wait_measured(st, basename, max);
    if measured {
        drop(st);
        log_info!(
            "StepMania[sync]: stage load for '{}' held {} ms for its measurement",
            basename,
            started.elapsed().as_millis()
        );
        return;
    }
    st.live.freeze(basename);
    drop(st);
    warn_not_in_time(basename);
}

/// The per-song provider (`override_hook::set_song_delta_provider`): the
/// delta the current play uses for `basename`, freezing it for the
/// session. Waits ≤ `wait` for a pending measurement. `None` for a
/// non-injected basename, a refused song, or a song measured too late.
pub fn delta_for_play(basename: &str, wait: Duration) -> Option<i8> {
    if !INITIALISED.load(Ordering::Acquire) {
        return None;
    }
    let entry = index::entry_by_basename(basename)?;
    let mut st = lock();
    seed_from_table(&mut st, basename, entry.fingerprint);
    if let Some(e) = st.live.get(basename) {
        return if e.frozen {
            e.delta
        } else {
            st.live.freeze(basename)
        };
    }
    let was_pending = pending(&st, basename);
    let (mut st, measured) = wait_measured(st, basename, wait);
    let delta = st.live.freeze(basename);
    drop(st);
    if !measured && was_pending {
        warn_not_in_time(basename);
    }
    delta
}

fn delta_provider(code: &str) -> Option<i8> {
    delta_for_play(code, FIRST_JUDGE_HOLD)
}
