//! Auto-sync, the apply side: the ledger (`sync.bin`, written by the prep
//! tool) and the live per-song delta map with its freeze rule. Nothing here
//! measures or writes a file — a correction is one number, handed to
//! `per_song_judgement_offsets` at play time.
//!
//! ```text
//! sync.bin (tool) ──load at enable──► LiveMap {delta, frozen}
//!                                          │
//! first judge dispatch (per side) ─────────┴─► freeze → delta → compose(entry, stock, delta)
//! ```
//!
//! ## Live value and freeze ([`super::sync_state::LiveMap`])
//! At [`init`] every `sync.bin` record whose fingerprint the (validated)
//! index confirms seeds the map unfrozen (`Measured` → `Some(d)`, `Refused`
//! → `None`). A play's first [`delta_for_play`] freezes the song's entry
//! with whatever it has — the measured delta, or `None` for a refusal or a
//! song the tool has not measured (it plays as authored) — and every later
//! caller (the other side, a replay) gets that value, so two plays of one
//! song in one session never differ.
//!
//! `auto_sync: false`: [`init`] does nothing and the provider is never
//! registered.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::log_info;
use crate::mods::per_song_judgement_offsets::override_hook;

use super::cache;
use super::index;
use super::sync_state::{LiveMap, SyncRecord, SyncTable};

#[derive(Default)]
struct Apply {
    /// The ledger (`None` until loaded).
    table: Option<SyncTable>,
    live: LiveMap,
}

static STATE: OnceLock<Mutex<Apply>> = OnceLock::new();
static INITIALISED: AtomicBool = AtomicBool::new(false);

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

/// Mod enable (`stepmania::init_sync`): load the ledger, seed the live map
/// from every record the index confirms, register the delta provider with
/// the per-song mod, and say how many injected songs have no record.
/// `false` when `auto_sync` is off. Idempotent.
pub fn init() -> bool {
    if !super::auto_sync_enabled() {
        return false;
    }
    if INITIALISED.swap(true, Ordering::AcqRel) {
        return true;
    }
    let songs = index::snapshot().songs;
    let (records, seeded) = {
        let mut st = lock();
        let records = table_of(&mut st).len();
        let mut seeded = 0usize;
        for e in songs.iter() {
            if seed_from_table(&mut st, &e.basename, e.fingerprint) {
                seeded += 1;
            }
        }
        (records, seeded)
    };
    override_hook::set_song_delta_provider(delta_provider);
    let unmeasured = songs.len().saturating_sub(seeded);
    log_info!(
        "StepMania[sync]: sync.bin holds {} record(s); {} of {} indexed song(s) live{}; judgement-offset provider registered",
        records,
        seeded,
        songs.len(),
        if unmeasured > 0 {
            format!(" ({unmeasured} without a record play as authored -- run {} to measure them)", super::PREP_TOOL)
        } else {
            String::new()
        }
    );
    true
}

/// The per-song provider (`override_hook::set_song_delta_provider`): the
/// delta the current play uses for `basename`, freezing it for the session.
/// `None` for a non-injected basename, a refused song, or a song without a
/// record.
pub fn delta_for_play(basename: &str) -> Option<i8> {
    if !INITIALISED.load(Ordering::Acquire) {
        return None;
    }
    let entry = index::entry_by_basename(basename)?;
    let mut st = lock();
    seed_from_table(&mut st, basename, entry.fingerprint);
    if let Some(e) = st.live.get(basename) {
        if e.frozen {
            return e.delta;
        }
    }
    st.live.freeze(basename)
}

fn delta_provider(code: &str) -> Option<i8> {
    delta_for_play(code)
}
