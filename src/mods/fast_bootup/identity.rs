//! File-identity resolution + the background cache verifier for the
//! ultrafast-boot step-data cache (design §Components → `identity.rs`).
//!
//! Two responsibilities, both OFF the game thread:
//!
//! 1. [`resolve`] maps a registered game path (`data/mdb_apx/ssq/x.ssq`) to
//!    its current [`Identity`] — the LayeredFS-resolved backing file's
//!    size+mtime, or [`Identity::Absent`] — using host `std::fs` (never AVS,
//!    which is game-thread-only) and the mod-folder override precedence via
//!    `avs_layeredfs::mod_paths` (the established `chart_length` pattern).
//! 2. [`spawn_verifier`] runs at mod enable: it reads + parses the bin, then
//!    resolves + stats every cached file and publishes a
//!    `game_path → verified?` verdict map behind a ready flag, plus stashes
//!    the loaded [`CacheFile`] for the completion-time writer to merge over.
//!
//! The verifier races a ~100 ms stat sweep against the ~1 s before the boot
//! actor's first `onUpdate`, so the map is ready by the time the pass starts
//! (design §Error Handling: if not ready, the pass treats every item as a
//! miss — still correct, just no replay that boot).
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
    let mod_hit =
        mod_paths::find_first_modfile(&rel).and_then(|p| stat_file(&p).map(|(s, m)| (p, s, m)));
    let stock_path = format!("data/{}", rel);
    let stock_hit = stat_file(&stock_path).map(|(s, m)| (stock_path.clone(), s, m));
    cache::resolve_identity(mod_hit, stock_hit)
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

    // Build the replay index (verdicts + payloads-by-difficulty) off-thread.
    let mut index: HashMap<String, ReplayFile> = HashMap::with_capacity(loaded.entries.len());
    let mut hits = 0usize;
    for e in &loaded.entries {
        let current = resolve(&e.game_path);
        let verified = cache::identity_matches(&e.identity, &current);
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
