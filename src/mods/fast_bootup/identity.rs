//! File-identity resolution + the background cache verifier for the
//! ultrafast-boot step-data cache (design §Components → `identity.rs`).
//!
//! Two responsibilities, both OFF the game thread:
//!
//! 1. [`resolve`] maps a registered game path (`data/mdb_apx/ssq/x.ssq`) to
//!    its current [`Identity`]: for an injected StepMania SSQ the index
//!    record's source fingerprint ([`Identity::Indexed`], no I/O); otherwise
//!    the LayeredFS-resolved backing file's size+mtime, or
//!    [`Identity::Absent`] — using host `std::fs` (never AVS, which is
//!    game-thread-only) and the mod-folder override precedence via
//!    `avs_layeredfs::mod_paths` (the established `chart_length` pattern).
//! 2. [`spawn_verifier`] runs at mod enable: it reads + parses the bin, then
//!    resolves + stats every cached file and publishes a
//!    `game_path → verified?` verdict map behind a ready flag, plus stashes
//!    the loaded [`CacheFile`] for the completion-time writer to merge over.
//!
//! The verifier races its stat sweep (~100 ms for the stock library, four
//! workers) against the ~1 s before the boot actor's first `onUpdate`, so
//! the map is normally ready by the time the pass starts. When it is not —
//! a 14k-song library is ~15k stats, a minute or more under Wine — the pass
//! starts with every item a miss and `mod.rs::late_replan` amends the plan
//! for the files still ahead of the cursor the moment the map publishes.
//!
//! Fail-open: a missing/empty/corrupt bin publishes an empty cache + empty
//! verdicts (full stock boot + rebuild). Pure identity/merge logic lives in
//! [`super::cache`] so it stays host-testable.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

use super::cache::{self, CacheFile, CacheLoad, Identity, SlotPayload};
use crate::services::avs_layeredfs::mod_paths;
use crate::services::stepmania::index as stepmania_index;
use crate::{log_info, log_warn};

/// Cache directory + file (CWD-relative, like every other `data_mods/_cache`
/// consumer; the game's CWD is `contents/`).
pub const CACHE_DIR: &str = "./data_mods/_cache/step_data";
pub const CACHE_FILE: &str = "./data_mods/_cache/step_data/v1.bin";

/// A cached file's replay view: whether its identity still verifies, plus its
/// payloads indexed by difficulty → [mode0, mode1] for O(1) replay lookup.
struct ReplayFile {
    verified: bool,
    slots: HashMap<u8, [Option<SlotPayload>; 2]>,
}

static STARTED: OnceLock<()> = OnceLock::new();
static READY: AtomicBool = AtomicBool::new(false);
/// The published replay index (verdicts + payloads), built off-thread so the
/// game thread never stats or re-parses. Populated once by the verifier.
static REPLAY_INDEX: OnceLock<HashMap<String, ReplayFile>> = OnceLock::new();
/// The raw loaded cache, stashed for the completion-time writer's merge.
static LOADED_CACHE: Mutex<Option<CacheFile>> = Mutex::new(None);

/// Resolve a registered game path's current identity via host `std::fs`.
/// Mod-folder override wins over the stock `data/…` path (pure precedence in
/// [`cache::resolve_identity`]); neither existing ⇒ [`Identity::Absent`].
pub fn resolve(game_path: &str) -> Identity {
    let rel = match cache::normalize_ssq_rel(game_path) {
        Some(r) => r,
        None => return Identity::Absent,
    };
    // Injected StepMania SSQs: the index record's source fingerprint is the
    // identity (see `cache::Identity::Indexed`) — no stat.
    if let Some(basename) = cache::indexed_basename(&rel, stepmania_index::has_basename) {
        if let Some(entry) = stepmania_index::entry_by_basename(&basename) {
            return Identity::Indexed {
                basename,
                fingerprint: entry.fingerprint,
            };
        }
    }
    let mod_hit =
        mod_paths::find_first_modfile(&rel).and_then(|p| stat_file(&p).map(|(s, m)| (p, s, m)));
    // The override wins, so the stock stat is only paid when there is none
    // (every injected StepMania SSQ resolves to its cache file; stat'ing a
    // stock path that cannot exist doubled the verifier's work at 14k).
    let stock_hit = if mod_hit.is_some() {
        None
    } else {
        let stock_path = format!("data/{}", rel);
        stat_file(&stock_path).map(|(s, m)| (stock_path.clone(), s, m))
    };
    cache::resolve_identity(mod_hit, stock_hit)
}

/// The host path backing a registered game path — the same precedence as
/// [`resolve`] (mod folder / LayeredFS resolvers, then stock) — or `None`.
pub fn resolve_path(game_path: &str) -> Option<String> {
    let rel = cache::normalize_ssq_rel(game_path)?;
    if let Some(p) = mod_paths::find_first_modfile(&rel) {
        if stat_file(&p).is_some() {
            return Some(p);
        }
    }
    let stock_path = format!("data/{}", rel);
    stat_file(&stock_path).map(|_| stock_path)
}

/// `(size, mtime_secs)` for a host path, or `None` if it can't be stat'd.
fn stat_file(path: &str) -> Option<(u64, u64)> {
    let md = std::fs::metadata(path).ok()?;
    let mtime = md
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some((md.len(), mtime))
}

/// Spawn the one-shot verifier thread (idempotent). `stamp`/`size` are the
/// loaded gamemdx module's PE TimeDateStamp / SizeOfImage — the cache header
/// invalidators.
pub fn spawn_verifier(stamp: u32, size: u32) {
    if STARTED.set(()).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("fast-bootup-cache".into())
        .spawn(move || {
            if std::panic::catch_unwind(|| run_verify(stamp, size)).is_err() {
                log_warn!("FastBootup: cache verifier panicked -- treating cache as empty");
                READY.store(true, Ordering::Release);
            }
        })
        .is_ok();
    if !spawned {
        log_warn!("FastBootup: failed to spawn cache verifier -- capture-only, no would-hit stats");
        READY.store(true, Ordering::Release);
    }
}

fn run_verify(stamp: u32, size: u32) {
    let bytes = std::fs::read(CACHE_FILE).unwrap_or_default();
    let loaded = match cache::parse(&bytes, stamp, size) {
        CacheLoad::Loaded(c) => c,
        CacheLoad::Empty { reason } => {
            if !bytes.is_empty() {
                // A present-but-unusable bin: one WARN + full rebuild (NFR-1b).
                log_warn!(
                    "FastBootup: step-data cache unusable ({}) -- full rebuild this boot",
                    reason
                );
            }
            CacheFile::default()
        }
    };

    // Resolve + stat every cached file on a few worker threads: one stat is
    // ~3–10 ms under Wine, and a 14k-song library has ~15k cached files
    // (single-threaded that is ~100 s — the pass starts ~10 s in).
    let verdicts = verify_parallel(&loaded.entries);

    // Build the replay index (verdicts + payloads-by-difficulty) off-thread.
    let mut index: HashMap<String, ReplayFile> = HashMap::with_capacity(loaded.entries.len());
    let mut hits = 0usize;
    for (e, &verified) in loaded.entries.iter().zip(&verdicts) {
        if verified {
            hits += 1;
        }
        let mut slots: HashMap<u8, [Option<SlotPayload>; 2]> = HashMap::new();
        for p in &e.payloads {
            if p.mode < 2 {
                let arr = slots.entry(p.difficulty).or_insert([None, None]);
                arr[p.mode as usize] = Some(*p);
            }
        }
        index.insert(e.game_path.clone(), ReplayFile { verified, slots });
    }

    let total = loaded.entries.len();
    if let Ok(mut g) = LOADED_CACHE.lock() {
        *g = Some(loaded);
    }
    let _ = REPLAY_INDEX.set(index);
    READY.store(true, Ordering::Release);
    log_info!(
        "FastBootup: step-data cache verifier ready -- {} cached file(s), {} verified",
        total,
        hits
    );
}

/// Workers for the identity sweep: a handful is plenty (the cost is
/// per-stat latency, not CPU), and the StepMania validation sweep runs its
/// own threads alongside.
const VERIFY_WORKERS: usize = 4;

/// `identity_matches` for every entry, in entry order, computed by
/// [`VERIFY_WORKERS`] threads over interleaved slices. Falls back to the
/// calling thread when a worker cannot be spawned.
fn verify_parallel(entries: &[cache::FileEntry]) -> Vec<bool> {
    let n = entries.len();
    if n < VERIFY_WORKERS * 64 {
        return entries
            .iter()
            .map(|e| cache::identity_matches(&e.identity, &resolve(&e.game_path)))
            .collect();
    }
    let mut out = vec![false; n];
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(VERIFY_WORKERS);
        for w in 0..VERIFY_WORKERS {
            let entries = &entries;
            let h = std::thread::Builder::new()
                .name(format!("fast-bootup-verify-{w}"))
                .spawn_scoped(scope, move || {
                    let mut local: Vec<(usize, bool)> = Vec::with_capacity(n / VERIFY_WORKERS + 1);
                    let mut i = w;
                    while i < n {
                        let e = &entries[i];
                        local.push((
                            i,
                            cache::identity_matches(&e.identity, &resolve(&e.game_path)),
                        ));
                        i += VERIFY_WORKERS;
                    }
                    local
                });
            match h {
                Ok(h) => handles.push(h),
                Err(_) => {
                    // No thread: do this worker's share inline.
                    let mut i = w;
                    while i < n {
                        let e = &entries[i];
                        out[i] = cache::identity_matches(&e.identity, &resolve(&e.game_path));
                        i += VERIFY_WORKERS;
                    }
                }
            }
        }
        for h in handles {
            if let Ok(local) = h.join() {
                for (i, v) in local {
                    out[i] = v;
                }
            }
        }
    });
    out
}

/// True once the verifier has published its replay index.
pub fn is_ready() -> bool {
    READY.load(Ordering::Acquire)
}

/// Cache verdict for a game path: `true` = identity verified (replayable),
/// `false` = miss/unknown. Meaningful only once [`is_ready`].
pub fn verdict(game_path: &str) -> bool {
    REPLAY_INDEX
        .get()
        .and_then(|m| m.get(game_path))
        .map(|rf| rf.verified)
        .unwrap_or(false)
}

/// True iff this (file, difficulty) is fully replayable: identity verified
/// AND both mode payloads present. The per-item cache-hit predicate.
pub fn item_replayable(game_path: &str, difficulty: u8) -> bool {
    REPLAY_INDEX
        .get()
        .and_then(|m| m.get(game_path))
        .map(|rf| {
            rf.verified
                && rf
                    .slots
                    .get(&difficulty)
                    .map(|a| a[0].is_some() && a[1].is_some())
                    .unwrap_or(false)
        })
        .unwrap_or(false)
}

/// The two mode payloads (0, 1) for a replayable (file, difficulty), or
/// `None` if either is missing. `SlotPayload` is `Copy`, so this is a cheap
/// by-value fetch on the replay hot path.
pub fn replay_payloads(game_path: &str, difficulty: u8) -> Option<[SlotPayload; 2]> {
    let rf = REPLAY_INDEX.get()?.get(game_path)?;
    let a = rf.slots.get(&difficulty)?;
    Some([a[0]?, a[1]?])
}

/// Take the loaded cache for the completion-time writer to merge fresh
/// captures over. Returns an empty cache if the verifier hasn't stashed one
/// (in which case fresh captures fully rebuild it — correct, since a
/// not-ready verifier means every item went stock + captured).
pub fn take_loaded_cache() -> CacheFile {
    LOADED_CACHE
        .lock()
        .ok()
        .and_then(|mut g| g.take())
        .unwrap_or_default()
}
