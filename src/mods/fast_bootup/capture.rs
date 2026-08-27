//! Boot-time capture of the game's own analyzer outputs for the
//! ultrafast-boot step-data cache (design §Components → `capture.rs`).
//!
//! A post-original subscriber on the shared `services::analyze_hook`
//! dispatcher records each Analyze call's `result[14]` / `radar[5]` / return
//! byte — but ONLY while a boot pass is active AND the call originated from
//! the boot actor's own `onUpdate` (both gates below). Gameplay Analyze
//! calls (a different call site, outside `IN_HOOK`) never capture.
//!
//! Keying: `update_hook` stashes the current work item's `{game_path,
//! difficulty}` immediately before each stock `hook.call`; the subscriber
//! fills the two mode slots (0/1); [`harvest`] (after the call) folds them
//! into the per-file capture [`STORE`]. A file shared by up to five
//! difficulty items accumulates up to ten payloads. At completion the
//! [`spawn_writer`] thread merges the fresh store over the loaded cache,
//! stats each fresh file's identity (host `std::fs`, off the game thread),
//! serializes, and atomically replaces the bin.
//!
//! Replay is not wired here — this is the capture-only half. Everything is
//! panic-contained (the dispatcher wraps the subscriber in `catch_unwind`;
//! the writer thread wraps its body) and fail-open.

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use super::cache::{self, CacheFile, FileEntry, SlotPayload};
use super::identity;
use crate::services::analyze_hook::AnalyzeArgs;
use crate::services::avs_layeredfs::mod_paths;
use crate::{log_info, log_warn};

/// One Analyze call's captured outputs (one mode of one difficulty).
struct SlotCap {
    ret: u8,
    result: [i32; 14],
    radar: [i32; 5],
}

/// The work item currently being processed stock, plus its two mode slots
/// filled by the subscriber during `hook.call`.
struct Pending {
    game_path: String,
    difficulty: i32,
    slots: [Option<SlotCap>; 2],
}

/// True only during the boot pass (between the first hooked `onUpdate` and
/// completion). The subscriber's outer gate.
static BOOT_ACTIVE: AtomicBool = AtomicBool::new(false);
/// The current stock item's stash (game-thread only, but a `Mutex` avoids a
/// `static mut` and the subscriber reads it from the same thread).
static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
/// Fresh captures, keyed by registered game path. Drained by the writer.
static STORE: Lazy<Mutex<HashMap<String, Vec<SlotPayload>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Register the capture subscriber on the shared Analyze dispatcher. Safe to
/// call even if the dispatcher isn't armed (the subscriber then never fires).
pub fn register() {
    crate::services::analyze_hook::register_post(on_analyze);
}

/// Open the capture window (first hooked `onUpdate`).
pub fn begin() {
    BOOT_ACTIVE.store(true, Ordering::Release);
}

/// Close the capture window (completion / disable).
pub fn end() {
    BOOT_ACTIVE.store(false, Ordering::Release);
    if let Ok(mut g) = PENDING.lock() {
        *g = None;
    }
}

/// True while capturing.
pub fn is_active() -> bool {
    BOOT_ACTIVE.load(Ordering::Acquire)
}

/// Stash the item about to be processed stock (before `hook.call`). Both
/// mode slots reset; the subscriber fills them during the call.
pub fn set_item(game_path: String, difficulty: i32) {
    if let Ok(mut g) = PENDING.lock() {
        *g = Some(Pending {
            game_path,
            difficulty,
            slots: [None, None],
        });
    }
}

/// Drop the pending item without harvesting (e.g. an item with no resolvable
/// game path — entry_index ≤ 0). Keeps a stray subscriber write from being
/// mis-attributed to the previous item.
pub fn clear_item() {
    if let Ok(mut g) = PENDING.lock() {
        *g = None;
    }
}

/// Fold the just-processed item's captured slots into the store (after
/// `hook.call`).
pub fn harvest() {
    let taken = PENDING.lock().ok().and_then(|mut g| g.take());
    let p = match taken {
        Some(p) => p,
        None => return,
    };
    let mut payloads: Vec<SlotPayload> = Vec::with_capacity(2);
    for (mode, slot) in p.slots.iter().enumerate() {
        if let Some(s) = slot {
            payloads.push(SlotPayload {
                difficulty: p.difficulty as u8,
                mode: mode as u8,
                ret: s.ret,
                result: s.result,
                radar: s.radar,
            });
        }
    }
    if payloads.is_empty() {
        return;
    }
    if let Ok(mut store) = STORE.lock() {
        store.entry(p.game_path).or_default().extend(payloads);
    }
}

/// Number of distinct files captured so far (for the completion log).
pub fn store_len() -> usize {
    STORE.lock().map(|s| s.len()).unwrap_or(0)
}

/// The post-original Analyze subscriber. Reads the freshly-filled output
/// blocks into the pending item's mode slot. No allocation, no logging.
fn on_analyze(args: &AnalyzeArgs, ret: u8) {
    if !BOOT_ACTIVE.load(Ordering::Acquire) {
        return;
    }
    // Only capture the boot actor's own Analyze calls (belt-and-suspenders
    // against any Analyze that isn't ours during the window).
    if !super::in_hook() {
        return;
    }
    let mode = args.mode;
    if mode != 0 && mode != 1 {
        return;
    }
    if args.result.is_null() || args.radar.is_null() {
        return;
    }
    let mut result = [0i32; 14];
    let mut radar = [0i32; 5];
    // Safety: the pointers are the game's caller-owned output blocks, fully
    // written by the original Analyze (which ran before this subscriber).
    unsafe {
        let rp = args.result as *const i32;
        for (i, v) in result.iter_mut().enumerate() {
            *v = rp.add(i).read_unaligned();
        }
        let dp = args.radar as *const i32;
        for (i, v) in radar.iter_mut().enumerate() {
            *v = dp.add(i).read_unaligned();
        }
    }
    if let Ok(mut g) = PENDING.lock() {
        if let Some(p) = g.as_mut() {
            p.slots[mode as usize] = Some(SlotCap { ret, result, radar });
        }
    }
}

/// Spawn the completion-time writer thread: drain the store, merge fresh over
/// the loaded cache (fresh wins), stat each fresh file's identity, serialize,
/// and atomically replace the bin. No-op if nothing was captured (a full
/// cache-hit boot re-captures nothing and must not clobber the good cache).
pub fn spawn_writer(stamp: u32, size: u32) {
    let fresh_map = match STORE.lock() {
        Ok(mut s) => std::mem::take(&mut *s),
        Err(_) => return,
    };
    if fresh_map.is_empty() {
        return;
    }
    let fresh_count = fresh_map.len();
    let spawned = std::thread::Builder::new()
        .name("fast-bootup-writer".into())
        .spawn(move || {
            if std::panic::catch_unwind(move || run_writer(fresh_map, stamp, size, fresh_count))
                .is_err()
            {
                log_warn!("FastBootup: cache writer panicked -- cache left unchanged");
            }
        })
        .is_ok();
    if !spawned {
        log_warn!("FastBootup: failed to spawn cache writer thread");
    }
}

fn run_writer(
    fresh_map: HashMap<String, Vec<SlotPayload>>,
    stamp: u32,
    size: u32,
    fresh_count: usize,
) {
    // Build fresh entries, stat'ing identity here (writer thread) — never on
    // the game thread. Payloads sorted (difficulty, mode) for deterministic,
    // diff-friendly output.
    let mut fresh: Vec<FileEntry> = Vec::with_capacity(fresh_map.len());
    for (game_path, mut payloads) in fresh_map {
        payloads.sort_by_key(|p| (p.difficulty, p.mode));
        let identity = identity::resolve(&game_path);
        fresh.push(FileEntry {
            game_path,
            identity,
            payloads,
        });
    }

    let loaded: CacheFile = identity::take_loaded_cache();
    let merged = cache::merge(loaded, fresh);
    let entry_count = merged.entries.len();
    let bytes = cache::serialize(&merged, stamp, size);

    mod_paths::mkdir_p(identity::CACHE_DIR);
    let tmp = format!("{}.tmp", identity::CACHE_FILE);
    let ok = std::fs::write(&tmp, &bytes)
        .and_then(|_| std::fs::rename(&tmp, identity::CACHE_FILE))
        .is_ok();
    if ok {
        log_info!(
            "FastBootup: wrote step-data cache -- {} entries ({} fresh), {} bytes",
            entry_count,
            fresh_count,
            bytes.len()
        );
    } else {
        log_warn!(
            "FastBootup: failed to write step-data cache to {}",
            identity::CACHE_FILE
        );
        let _ = std::fs::remove_file(&tmp);
    }
}
