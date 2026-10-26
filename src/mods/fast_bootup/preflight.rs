//! Background pre-flight of injected (StepMania) SSQ files.
//!
//! The quarantine check (`quarantine.rs`, `mod.rs` → `preflight_injected`)
//! reads and walks every injected file the step-data cache cannot vouch
//! for. Done at plan build that is game-thread I/O on the loading screen —
//! 636 files took 2.9 s on a cold-cache boot. This thread does the same
//! check right after mod enable, from the songs' index records (no work list
//! needed), and publishes one verdict per file name as it goes; the plan
//! build only looks verdicts up and checks inline just the files the thread
//! has not reached yet. It never waits: a slow thread costs the old inline
//! time for its unfinished tail, never more.
//!
//! The file's identity cannot change between the two (the prep tool wrote
//! the SSQs before the game started and nothing in the game ever rewrites
//! them), so a verdict taken here is the verdict the plan would have reached.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::{identity, quarantine};
use crate::{log_info, log_warn};

/// Registered game-path prefix of every injected SSQ.
const SSQ_GAME_DIR: &str = "data/mdb_apx/ssq/";
/// Largest SSQ read (stock files are < 1 MiB).
pub const MAX_BYTES: u64 = 32 * 1024 * 1024;
/// How long the thread waits for the `startup.arc` injection to publish
/// its entries before giving up (the plan build then checks inline).
const INJECTION_WAIT: Duration = Duration::from_secs(120);

static STARTED: OnceLock<()> = OnceLock::new();
/// Set by the plan build; the thread stops (its results are no longer read).
static PLAN_STARTED: AtomicBool = AtomicBool::new(false);
/// File name (`<b>.ssq` / `<b>_N.ssq`) → `None` = passed, `Some(reason)`.
static VERDICTS: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();

fn verdicts() -> &'static Mutex<HashMap<String, Option<String>>> {
    VERDICTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Check one injected file: resolve it the way the game's open will, read
/// it, and walk it against `slots`. `None` = safe to analyse.
pub fn check_file(game_path: &str, slots: &[u8]) -> Option<String> {
    let Some(host) = identity::resolve_path(game_path) else {
        return Some("SSQ file not found".to_string());
    };
    let Some(bytes) =
        crate::services::stepmania::index::read_capped(std::path::Path::new(&host), MAX_BYTES)
    else {
        return Some(format!("cannot read {host}"));
    };
    crate::core::ssq::check::check_ssq(&bytes, slots)
        .err()
        .map(|fs| {
            fs.iter()
                .take(3)
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        })
}

/// The background verdict for `file_name`, if the thread has reached it.
pub fn lookup(file_name: &str) -> Option<Option<String>> {
    verdicts().lock().ok()?.get(file_name).cloned()
}

/// The plan build has started: the thread's remaining work is moot.
pub fn mark_plan_started() {
    PLAN_STARTED.store(true, Ordering::Release);
}

/// Spawn the one-shot pre-flight thread (idempotent; no-op without packs).
pub fn spawn() {
    if !crate::services::stepmania::is_armed() || STARTED.set(()).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("fast-bootup-preflight".into())
        .spawn(|| {
            if std::panic::catch_unwind(run).is_err() {
                log_warn!(
                    "FastBootup: background pre-flight panicked -- the plan build checks inline"
                );
            }
        })
        .is_ok();
    if !spawned {
        log_warn!(
            "FastBootup: could not spawn the background pre-flight -- the plan build checks inline"
        );
    }
}

fn run() {
    // The entries exist once the startup.arc transform injected them.
    let deadline = Instant::now() + INJECTION_WAIT;
    let entries = loop {
        let entries = crate::services::stepmania::injected_entries();
        if !entries.is_empty() {
            break entries;
        }
        if PLAN_STARTED.load(Ordering::Acquire) || Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    };

    let started = Instant::now();
    let (mut checked, mut failing) = (0usize, 0usize);
    for entry in entries.iter() {
        if PLAN_STARTED.load(Ordering::Acquire) {
            log_info!(
                "FastBootup: background pre-flight overtaken by the boot pass after {} file(s) in {} ms -- the rest are checked inline",
                checked,
                started.elapsed().as_millis()
            );
            return;
        }
        let files =
            quarantine::entry_files(&entry.basename, &entry.levels, |d| entry.split_file_for(d));
        for (name, slots) in files {
            let verdict = check_file(&format!("{SSQ_GAME_DIR}{name}"), &slots);
            failing += usize::from(verdict.is_some());
            checked += 1;
            if let Ok(mut m) = verdicts().lock() {
                m.insert(name, verdict);
            }
        }
    }
    log_info!(
        "FastBootup: background pre-flight -- {} injected file(s) checked in {} ms, {} failing",
        checked,
        started.elapsed().as_millis(),
        failing
    );
}
