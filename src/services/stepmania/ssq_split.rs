//! The StepMania consumer of `services::ssq_path` (priority 10): answers the
//! game's `build_ssq_path(b, d)` for INJECTED basenames from the index.
//!
//! A song whose charts share one timing is one `ssq/<b>.ssq` → `Choice::Base`
//! for every slot. A song with per-chart timing has one `ssq/<b>_N.ssq` per
//! level (`N = level + 1`, written by `core::ssq::writer`) and no base file;
//! `SongEntry::split_levels` records which `N` exist and the levels each
//! holds, and `SongEntry::split_file_for(d)` applies Rule A over it (the
//! highest `N ≤ d + 1` holding level `d`) → `Choice::Split(N)`, else `Base`.
//! Non-injected basenames → `None` so the chain continues (split-ssq
//! auto-discovery, then the game's own table).
//!
//! Panic-free: a map lookup and an array walk. One INFO per injected split
//! song the first time the game asks about it (cap 32).

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::log_info;
use crate::services::ssq_path::{self, Choice};

use super::index;

/// Chain position: ahead of split-ssq auto-discovery (20).
pub const PRIORITY: u8 = 10;

static REGISTERED: AtomicBool = AtomicBool::new(false);
static ANNOUNCED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
const ANNOUNCE_CAP: usize = 32;

/// Register with the service (once). `false` when the service is unavailable.
pub fn register() -> bool {
    if REGISTERED.load(Ordering::Acquire) {
        return true;
    }
    if !ssq_path::register(PRIORITY, resolve) {
        return false;
    }
    REGISTERED.store(true, Ordering::Release);
    true
}

/// Whether the resolver is in the chain.
pub fn is_registered() -> bool {
    REGISTERED.load(Ordering::Acquire)
}

fn resolve(basename: &[u8], d: usize) -> Option<Choice> {
    let b = std::str::from_utf8(basename).ok()?;
    let entry = index::entry_by_basename(b)?;
    if !entry.has_split_files() {
        return Some(Choice::Base);
    }
    let choice = match entry.split_file_for(d) {
        Some(n) => Choice::Split(n),
        None => Choice::Base,
    };
    announce_once(
        &entry.basename,
        &entry.pack,
        &entry.folder,
        &entry.split_levels,
    );
    Some(choice)
}

fn announce_once(basename: &str, pack: &str, folder: &str, split: &[u8; 5]) {
    let Ok(mut guard) = ANNOUNCED.lock() else {
        return;
    };
    let seen = guard.get_or_insert_with(HashSet::new);
    if seen.len() >= ANNOUNCE_CAP || !seen.insert(basename.to_string()) {
        return;
    }
    let files: Vec<String> = split
        .iter()
        .enumerate()
        .filter(|(_, m)| **m != 0)
        .map(|(i, _)| format!("{basename}_{}.ssq", i + 1))
        .collect();
    log_info!(
        "SsqPath: StepMania '{}/{}' ({}) served from split files {}",
        pack,
        folder,
        basename,
        files.join(" ")
    );
}
