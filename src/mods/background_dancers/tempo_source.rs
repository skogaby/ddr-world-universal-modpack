//! The tempo map's engine-side source: resolve the live song's SSQ
//! (LayeredFS-aware, split files included), read it on a std thread and
//! turn its tempo chunk into a [`TempoMap`](super::tempo::TempoMap).
//!
//! The song identity is the live `DancePlaySequence`'s basename
//! (`song_reset::live_dps_basename`), keyed on the DPS pointer so a fresh
//! DPS (finish-path quick restart, course stage) re-resolves; the SSQ is
//! read from disk exactly like `services::chart_length` does (mod folders
//! before `data/mdb_apx/ssq/`), trying the unsplit file then `_1`..`_5`
//! (every split file of a song carries the same tempo chunk).

use std::sync::{Arc, Mutex};

use crate::core::ssq::timing::TempoConverter;
use crate::services::avs_layeredfs::mod_paths;

use super::tempo::{nodes_from_ssq_pairs, TempoMap, TempoOptions};

/// Result slot the resolver thread fills.
pub type TempoSlot = Arc<Mutex<Option<Result<TempoMap, String>>>>;

/// Candidate SSQ files for a basename, in preference order.
fn candidates(basename: &str) -> Vec<String> {
    let mut v = vec![format!("mdb_apx/ssq/{basename}.ssq")];
    for n in 1..=5 {
        v.push(format!("mdb_apx/ssq/{basename}_{n}.ssq"));
    }
    v
}

/// Read the first SSQ that exists (mod folders first, then stock).
fn read_ssq(basename: &str) -> Result<(String, Vec<u8>), String> {
    for rel in candidates(basename) {
        let path = mod_paths::find_first_modfile(&rel).unwrap_or_else(|| format!("data/{rel}"));
        if let Ok(bytes) = std::fs::read(&path) {
            return Ok((path, bytes));
        }
    }
    Err(format!("no SSQ found for '{basename}' (unsplit or _1.._5)"))
}

/// Blocking: resolve + read + parse. No engine calls.
pub fn resolve(basename: &str, opts: TempoOptions) -> Result<TempoMap, String> {
    let (path, bytes) = read_ssq(basename)?;
    let conv = TempoConverter::from_ssq(&bytes).ok_or_else(|| format!("{path}: no tempo chunk"))?;
    let pairs: Vec<(i32, i32)> = conv.entries().collect();
    let nodes = nodes_from_ssq_pairs(&pairs, conv.tps());
    TempoMap::new(nodes, opts).ok_or_else(|| format!("{path}: tempo chunk unusable"))
}

/// Spawn the resolver thread; the caller polls the returned slot.
pub fn spawn(basename: String, opts: TempoOptions) -> TempoSlot {
    let slot: TempoSlot = Arc::new(Mutex::new(None));
    let out = Arc::clone(&slot);
    let spawned = std::thread::Builder::new()
        .name("bg-dancers-tempo".into())
        .spawn(move || {
            let r = resolve(&basename, opts);
            if let Ok(mut g) = out.lock() {
                *g = Some(r);
            }
        });
    if spawned.is_err() {
        if let Ok(mut g) = slot.lock() {
            *g = Some(Err("tempo thread could not be spawned".into()));
        }
    }
    slot
}
