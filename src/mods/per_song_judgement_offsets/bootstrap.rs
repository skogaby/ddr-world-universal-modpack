//! Boot-time bootstrap for Per-Song Judgement Offsets (design §Components →
//! musicdb crawl; plan Step 3).
//!
//! A fire-and-forget background thread that:
//!
//! 1. resolves the musicdb text the game actually parses — the LayeredFS
//!    merge cache when fragments exist, else a whole-file mod override, else
//!    the stock file served straight from AVS (arc-transparent, kbin-safe);
//! 2. append-merges any missing basenames into `judgement_offsets.csv`
//!    (CWD-relative, beside `mod-config.json`) — existing rows are never
//!    modified, and the file is created when absent;
//! 3. loads the CSV baseline into the [`store`](super::store).
//!
//! It also hosts the coalesced background CSV writer used by the options-menu
//! edit path (`queue_csv_upsert`) so per-row upserts never touch the render
//! thread. All failures degrade to stock behavior with one WARN.

use std::sync::mpsc;
use std::sync::OnceLock;

use super::csv;
use super::musicdb_scan;
use super::store;
use crate::services::avs_layeredfs::{kbin, mod_paths};
use crate::{log_info, log_warn};

/// Runtime CSV path — CWD-relative like `mod-config.json`.
pub const CSV_PATH: &str = "judgement_offsets.csv";

/// The stock archive carrying musicdb (CWD-relative, like every game data
/// path).
const STARTUP_ARC_PATH: &str = "./data/arc/startup.arc";
const MUSICDB_ARC_ENTRY: &str = "data/gamedata/musicdb.xml";
/// LayeredFS-normalized musicdb paths (whole-file override + merge
/// fragments).
const MUSICDB_NORM_PATH: &str = "gamedata/musicdb.xml";
const MUSICDB_FRAGMENT_PATH: &str = "gamedata/musicdb.merged.xml";

/// How long to keep polling for the mod-folder index before giving up
/// (LayeredFS init precedes mod enable, so this covers only a pathological
/// race; the disk reads themselves have no game dependency at all).
const CRAWL_ATTEMPTS: u32 = 3;
const CRAWL_RETRY_MS: u64 = 1_000;

/// One CSV cell change from the options menu (side 0/1, `None` = blank).
struct Upsert {
    code: String,
    side: usize,
    value: Option<i8>,
}

static UPSERT_TX: OnceLock<mpsc::Sender<Upsert>> = OnceLock::new();
static STARTED: OnceLock<()> = OnceLock::new();

/// Spawn the bootstrap thread (idempotent — later `enable()` calls no-op).
pub fn start() {
    if STARTED.set(()).is_err() {
        return;
    }
    if std::thread::Builder::new()
        .name("judgement-offsets".into())
        .spawn(|| {
            let result = std::panic::catch_unwind(run_bootstrap);
            if result.is_err() {
                log_warn!("judgement_offsets: bootstrap thread panicked — offsets unavailable");
            }
        })
        .is_err()
    {
        log_warn!("judgement_offsets: failed to spawn bootstrap thread");
    }
}

/// Queue an options-menu edit for persistence into the CSV. Safe to call
/// from any thread; drops (with one implicit WARN from the worker's absence)
/// if the writer never started.
pub fn queue_csv_upsert(code: String, side: usize, value: Option<i8>) {
    if let Some(tx) = UPSERT_TX.get() {
        let _ = tx.send(Upsert { code, side, value });
    }
}

fn run_bootstrap() {
    // 1. Crawl the merged musicdb (retrying while AVS spins up).
    let crawl = crawl_basenames();

    // 2. Read (or start) the CSV and append missing codes.
    let csv_text = std::fs::read_to_string(CSV_PATH).unwrap_or_default();
    let (mut doc, stats) = csv::parse(&csv_text);
    if !stats.is_clean() {
        log_warn!(
            "judgement_offsets: {}: {} clamped, {} skipped, {} duplicate line(s) (lines {:?}) — bad lines are dropped on the next rewrite",
            CSV_PATH,
            stats.clamped,
            stats.skipped,
            stats.duplicates,
            stats.bad_lines
        );
    }
    match &crawl {
        Some(names) => {
            let appended = doc.append_missing(names.iter().map(String::as_str));
            if appended > 0 || csv_text.is_empty() {
                if write_csv(&doc) {
                    log_info!(
                        "judgement_offsets: {} — {} song(s) known, {} appended",
                        CSV_PATH,
                        doc.rows().len(),
                        appended
                    );
                }
            } else {
                log_info!(
                    "judgement_offsets: {} up to date ({} songs)",
                    CSV_PATH,
                    doc.rows().len()
                );
            }
        }
        None => {
            log_warn!(
                "judgement_offsets: musicdb crawl failed — using existing {} as-is ({} rows)",
                CSV_PATH,
                doc.rows().len()
            );
            if csv_text.is_empty() {
                // Guarantee the file exists for the operator even without a
                // crawl (header-only).
                write_csv(&doc);
            }
        }
    }

    // 3. Baseline into the store (arms it).
    store::with_store(|s| s.load_baseline(&doc));
    log_info!(
        "judgement_offsets: baseline loaded ({} rows)",
        doc.rows().len()
    );

    // 4. Become the CSV writer: serve upserts for the rest of the process
    //    lifetime, coalescing bursts into one read-modify-write per drain.
    let (tx, rx) = mpsc::channel::<Upsert>();
    if UPSERT_TX.set(tx).is_err() {
        return; // impossible (single bootstrap), but never double-serve
    }
    let mut warned_write = false;
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        while let Ok(next) = rx.try_recv() {
            batch.push(next);
        }
        // Re-read so operator hand-edits between writes are preserved.
        let text = std::fs::read_to_string(CSV_PATH).unwrap_or_default();
        let (mut doc, _) = csv::parse(&text);
        for u in &batch {
            doc.upsert(&u.code, u.side, u.value);
        }
        if !write_csv(&doc) && !warned_write {
            warned_write = true;
        }
    }
}

/// Resolve the musicdb basename union from DISK — no AVS calls (the AVS
/// trampolines only work for in-hook game-thread callers; this runs on our
/// own thread — cabinet-diagnosed 2026-08-18). Union = base musicdb
/// (whole-file mod override if present, else the stock file out of
/// startup.arc) plus every mod's `musicdb.merged.xml` fragment — the same
/// resolution order the LayeredFS open hook applies, so custom songs are
/// included.
fn crawl_basenames() -> Option<Vec<String>> {
    for attempt in 0..CRAWL_ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(CRAWL_RETRY_MS));
        }

        // Base document: first mod's whole-file override wins, else stock.
        let base = mod_paths::find_first_modfile(MUSICDB_NORM_PATH)
            .and_then(|p| read_xml_file(&p))
            .or_else(read_stock_musicdb);

        // Fragment files from every mod folder (custom songs).
        let fragments: Vec<String> = mod_paths::find_all_modfile(MUSICDB_FRAGMENT_PATH)
            .iter()
            .filter_map(|p| read_xml_file(p))
            .collect();

        let mut names = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for text in base.iter().chain(fragments.iter()) {
            for name in musicdb_scan::scan_basenames(text) {
                if seen.insert(name.clone()) {
                    names.push(name);
                }
            }
        }
        if !names.is_empty() {
            log_info!(
                "judgement_offsets: musicdb crawl found {} song(s) ({} fragment file(s), attempt {})",
                names.len(),
                fragments.len(),
                attempt + 1
            );
            return Some(names);
        }
    }
    None
}

/// Read an on-disk XML file, decoding binary kbin when present.
fn read_xml_file(path: &str) -> Option<String> {
    let buf = std::fs::read(path).ok()?;
    xml_bytes_to_string(buf)
}

/// Extract `data/gamedata/musicdb.xml` from the stock startup.arc on disk
/// (AVSLZ decompression handled by `core::arc::extract`).
fn read_stock_musicdb() -> Option<String> {
    let arc_bytes = std::fs::read(STARTUP_ARC_PATH).ok()?;
    let entries = crate::core::arc::parse(&arc_bytes)?;
    let entry = entries.iter().find(|e| e.path == MUSICDB_ARC_ENTRY)?;
    let data = crate::core::arc::extract(&arc_bytes, entry)?;
    xml_bytes_to_string(data)
}

fn xml_bytes_to_string(buf: Vec<u8>) -> Option<String> {
    if buf.first() == Some(&0xA0) {
        // Binary kbin property format.
        return kbin::reader::decode_to_string(&buf).ok();
    }
    String::from_utf8(buf).ok()
}

/// Serialize + write via a temp file and rename (best-effort atomicity).
fn write_csv(doc: &csv::CsvDoc) -> bool {
    let tmp = format!("{CSV_PATH}.tmp");
    let text = csv::serialize(doc);
    let ok = std::fs::write(&tmp, text.as_bytes())
        .and_then(|_| std::fs::rename(&tmp, CSV_PATH))
        .is_ok();
    if !ok {
        log_warn!("judgement_offsets: failed to write {}", CSV_PATH);
        let _ = std::fs::remove_file(&tmp);
    }
    ok
}
