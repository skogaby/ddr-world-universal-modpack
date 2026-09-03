//! Fast Bootup Mod — Accelerates the loading screen by processing
//! all ready step-data items per frame instead of one, and caching the
//! boot-time chart analysis so unchanged charts skip loading entirely.
//!
//! ## Two layers of acceleration
//!
//! 1. **Loader pacing + batch processing** (the original mod): the game
//!    processes one SSQ per frame and its loader opens only 4 files per pump.
//!    We re-invoke the original `onUpdate` in a tight per-frame loop and raise
//!    the loader's open cap (`mgr+0x70`, 4→64) during the boot pass, so any
//!    charts that must load run at device speed. Restored at completion.
//! 2. **Boot analysis cache** (the ultrafast-boot refactor): the boot pass
//!    computes wheel metadata absent from `musicdb.xml` (min/core/max BPM,
//!    note counts → EX score, shock/variable-BPM flags, groove-radar maxima)
//!    by loading + analyzing every SSQ. That output lands in a small,
//!    enumerable write-set (music-DB entry fields + actor accumulators). We
//!    capture those outputs per (file × difficulty × mode) at the Analyze
//!    boundary on the first boot and persist them to
//!    `data_mods/_cache/step_data/v1.bin`; on later boots we skip the file
//!    reads AND the analysis for unchanged charts, injecting the cached values
//!    directly and retiring the loader records through the game's own release
//!    machinery. The multi-second SSQ window collapses to the single final
//!    work item.
//!
//! ### Cache module layout
//!
//! - `cache.rs` — pure bin format (parse/serialize, fail-open) + the pure
//!   identity/merge helpers (`normalize_ssq_rel`, `resolve_identity`,
//!   `identity_matches`, `merge`). Host-tested.
//! - `identity.rs` — host-`std::fs` file resolution (LayeredFS override
//!   precedence via `mod_paths`) + the off-thread verifier that reads the bin
//!   and publishes a replay index (verdict + payloads) before the boot pass.
//! - `capture.rs` — the boot-gated Analyze post-subscriber (shared
//!   `services::analyze_hook` dispatcher), the per-file capture store, and the
//!   completion-time writer thread (merge fresh over loaded, stat identities
//!   off-thread, atomic tmp+rename).
//! - `replay.rs` — pure transcription of onUpdate's post-Analyze arithmetic
//!   (`compute_slot`) + the actor radar fold (`fold_radar`). Host-tested.
//! - `plan.rs` — the pure boot-plan / flip-eligibility invariants. Host-tested.
//! - `mod.rs` (this file) — the `update_hook` orchestration: build the plan +
//!   flip records on the first call, then per cursor item either Replay
//!   (apply cached writes + accumulators + release + cursor + percent) or
//!   Stock (the existing gated path + capture).
//!
//! ### Safety invariants (why the cache is sound)
//!
//! - The **final work item is always Stock**, so the game's own completion
//!   block (copy actor accumulators → global config, set the done flag, walk
//!   the parent chain) runs natively on top of the accumulator state our
//!   replays built up.
//! - A file record is **flipped 1→6** (the stock "complete, empty" shape) only
//!   when *every* item referencing it is Replay AND it is still status 1;
//!   otherwise it loads and is processed stock. This structurally prevents a
//!   stock item ever analyzing an emptied buffer — which would zero its
//!   results AND trip the game's `ME1529 / FILE CORRUPTION ERROR` reporter, a
//!   hard boot blocker on real hardware. **Replay writes only the music-DB
//!   `+0x1B0` corruption FLAG byte; it never calls that reporter.**
//! - Cache identity per file = registered game path + LayeredFS-resolved
//!   backing file + size + mtime; the header additionally carries the gamemdx
//!   PE stamp/size, so a game update invalidates the whole cache.
//! - Everything fails open: a missing/corrupt bin, an unresolved derivation,
//!   or any per-item failure degrades to the plain loader-acceleration
//!   behavior (one WARN per class), never a crash or a wrong-metadata boot.
//!
//! ## What it hooks
//!
//! `CheckStepDataActor::onUpdate` (`check_step_data_update`, gamemdx vtable[6]).
//! Stock, the game calls this once per frame; each call processes at most one
//! step-data (SSQ) entry whose async load has completed, then advances an
//! internal cursor. On a real cabinet the full step-data preload takes minutes.
//! This mod re-invokes the original in a tight loop so every *ready* entry is
//! processed in a single frame — collapsing the multi-minute boot.
//!
//! ## The 20260721 crash and the readiness race
//!
//! SSQ loading is asynchronous: a worker thread (`FUN_1801fdbf0` on 20260721)
//! drives each entry's step-data record through a status state machine at
//! `record+0x20`: `1`=open, `2`=read-header (allocates the buffer at `+0x8`,
//! sets len `+0x14`), `3`=**reading data into the buffer**, `4`=alt-load,
//! `5`=failed, `6`=**load complete**, `7`=cleanup, `8`=finalized. The main
//! thread's `onUpdate` only processes statuses `{0,5,6,8}` (idle/failed/
//! loaded/finalized) — correctly excluding the in-flight states `{1,2,3,4}`.
//!
//! That status gate is necessary but **not sufficient** the instant status
//! flips to `6`: the worker sets `6` right after issuing the read, and the
//! buffer bytes may not yet be fully written/visible to the main thread —
//! especially under CrossOver/Wine on Apple Silicon, where x86 is translated
//! to a weaker (ARM) memory model. Stock survives because processing one entry
//! per frame always leaves a full frame between "status→6" and the main
//! thread's read, so the buffer is settled by the time the cursor arrives.
//! Our loop eliminates that gap and reaches freshly-`6` entries with an
//! unsettled buffer. The game's chart-summary reader (`FUN_1801cbdc0`) then
//! walks the SSQ chunk list (`ptr += *ptr` over chunk lengths) off the end of
//! the buffer → `EXCEPTION_ACCESS_VIOLATION`. (20260616 happened not to expose
//! the window; 20260721's loading timing does.)
//!
//! ## The fix — bounded chunk-walk readiness gate
//!
//! Before we allow the original to process an entry, we validate that its SSQ
//! buffer is a **complete, walkable chunk list fully contained in
//! `[buf, buf+len)`** — a faithful, strictly bounds-checked mirror of the
//! game's own `FUN_1801cbdc0` walk. If the walk can't reach a valid terminator
//! without stepping outside the buffer, the entry isn't settled yet, so we
//! defer it (stop the loop) and let a later frame pick it up once the worker
//! has finished. Key property: **if the gate passes, the game's walk provably
//! stays in-bounds → it cannot run off the buffer.** Entries with no buffer
//! (`buf==0`/`len==0`, e.g. idle/failed) are allowed through unchanged — the
//! game skips the walk for them (its own `buf!=0 && len!=0` guard). This keeps
//! the batch fast (all genuinely-ready entries still process in one frame)
//! while being immune to the load race and to future status-enum drift.
//!
//! ## The 20260724 crash and the end-of-list overrun
//!
//! A second, independent crash class: when `onUpdate` processes the **final**
//! work-list item, it sets the actor's done flag (`actor+0x20 |= 4`). The
//! game's actor dispatcher (`FUN_18021dc70`, message 0x102) checks
//! `(actor+0x20 & 0x24) == 0` before every `onUpdate` call, so stock never
//! invokes it again after completion. Our batch loop calls the original
//! **directly**, bypassing that gate — and the pre-fix `should_process_more`
//! had no bounds check on the cursor, so after the final item it read
//! `work_array[total]`, 12 bytes **past the end of the heap allocation**.
//! When that heap garbage happened to pass the status/buffer gates, the
//! re-invoked `onUpdate` read the same garbage triple and looked up a garbage
//! mcode in the music DB — an unguarded `lower_bound` whose miss produces a
//! NULL vtable deref (`MOV RAX,[RSI]` at gamemdx+0x325EF on 20260721).
//!
//! Symptom fingerprint: intermittent boot crash at ~97% loading, only with
//! fast-bootup enabled, appearing/disappearing as `musicdb.xml` grows or is
//! rearranged (the file's size/order perturbs the heap layout and re-rolls
//! the garbage past the work array). The OOB read actually happened on nearly
//! every fast-bootup boot; it only crashed when the dice landed wrong.
//!
//! ## The fix — cursor bounds + done-flag gates
//!
//! `should_process_more` now mirrors the game's own two stop conditions
//! before touching the work array: it returns false once the actor's done
//! flag is set (`actor+0x20 & 0x24`, the dispatcher's exact gate) and once
//! the cursor reaches the work-list length (`counter < (end-begin)/12`, the
//! completion condition `onUpdate` itself uses). It also reads the cursor the
//! way the game does — `[actor+0x58 + phase*8]` with the phase index from
//! `actor+0x82` — instead of assuming phase 0, and reads the record status as
//! a full u32 (the game compares `dword`, so a u8 read could disagree on
//! garbage). The post-completion call is now structurally impossible.

use crate::log_info;
use crate::mods::mod_trait::{Mod, ModContext};
use retour::GenericDetour;

pub mod cache;
pub mod capture;
pub mod identity;
pub mod plan;
pub mod replay;

type UpdateFn = unsafe extern "C" fn(*mut u8);

/// Step-data record statuses the game's `onUpdate` will process (idle, failed,
/// load-complete, finalized). The in-flight states `{1,2,3,4}` and cleanup `7`
/// are excluded — matches `FUN_180032360`'s guard on 20260721. Stored as i32:
/// the game compares the full dword (`MOV ECX,dword ptr [..+0x20]`), so a
/// narrower read could disagree with it on garbage data.
const READY_STATUSES: &[i32] = &[0, 5, 6, 8];
const MAX_PER_FRAME: u32 = 512;

/// `CheckStepDataActor` field offsets (validated on gamemdx 20260721,
/// `FUN_180032360` / dispatcher `FUN_18021dc70`).
const ACTOR_FLAGS: usize = 0x20; // u32 lifecycle flags; bit 2 = done
const ACTOR_COUNTERS: usize = 0x58; // per-phase {u32 counter, u32 aux} pairs
const ACTOR_PHASE_COUNT: usize = 0x80; // u16 phase count
const ACTOR_PHASE: usize = 0x82; // u16 current phase index
const ACTOR_WORK_BEGIN: usize = 0x88; // work array begin (12-byte items)
const ACTOR_WORK_END: usize = 0x90; // work array end
/// ptr → u32 loading-percent display target (`counter*100/total_items`).
const ACTOR_PERCENT_PTR: usize = 0xD8;
/// Actor radar accumulators are PER-SIDE: side 0 → +0xA8..+0xB8, side 1 →
/// +0xBC..+0xCC (onUpdate's `local_228 += 5` each side iteration). Both
/// 5-int windows are max-accumulated across all files and copied to the
/// global config at completion.
const ACTOR_RADAR_ACC_SIDE0: usize = 0xA8;
const ACTOR_RADAR_ACC_SIDE1: usize = 0xBC;
/// Dispatcher's onUpdate gate mask on `ACTOR_FLAGS` — once `flags & 0x24 != 0`
/// the game never calls `onUpdate` again, and neither may we.
const ACTOR_DONE_MASK: u32 = 0x24;
/// Work-list item stride: `{+0: i32 entry_index, +4: i32 difficulty, +8: i32 mcode}`.
const WORK_ITEM_STRIDE: usize = 12;

/// Music-DB entry write offsets (stride 0x258; slot `idx = difficulty +
/// mode*5`), per research §3.8 / the onUpdate decode.
const ENTRY_SONG_MAX_BPM: usize = 0x94; // u16, max-accumulate
const ENTRY_SONG_MIN_BPM: usize = 0x96; // u16, min-accumulate
const ENTRY_MAX_BPM: usize = 0x98; // i32[10]
const ENTRY_CORE_BPM: usize = 0xC0; // i32[10]
const ENTRY_MIN_BPM: usize = 0xE8; // i32[10]
const ENTRY_SHOCK: usize = 0x11A; // u8[10]
const ENTRY_VARIABLE_BPM: usize = 0x124; // u8[10]
const ENTRY_FLAG_12E: usize = 0x12E; // u8[10]
const ENTRY_CORRUPT_FLAG: usize = 0x1B0; // u8 (flag ONLY, never the reporter)
const ENTRY_EX_SCORE: usize = 0x1B4; // i32[10]
                                     // Music-DB entry vtable byte offset of `hasChart(mode, difficulty) -> bool`:
                                     // `+0x70` on 20260324+ but `+0x58` on 20250805 / 20260224 (three vtable
                                     // entries were inserted; on the old builds `+0x70` is `isShock` — same
                                     // argument shape, so a hardcoded slot returns a silently WRONG answer, not a
                                     // crash). Derived by `SignatureStore::entry_has_chart_vslot` from the vcall
                                     // CheckStepDataActor::onUpdate itself makes; 0 = underived ⇒ replay stays
                                     // off (`has_chart` returns false and `enable` refuses to arm replay).
static mut ENTRY_HAS_CHART_VFUNC: usize = 0;

/// Step-data record field offsets (stride 0x40, base = `[table+0x08]`).
const REC_STRIDE: usize = 0x40;
const REC_BUF: usize = 0x08; // SSQ buffer pointer
const REC_LEN: usize = 0x14; // SSQ buffer length (u32)
const REC_STATUS: usize = 0x20; // load state machine (u32)

/// Upper bound on chunks we'll traverse while validating — guards against a
/// cyclic/garbage chunk list in a partially-written buffer.
const MAX_CHUNKS: u32 = 65536;

/// Step-data manager field offset: the per-pump open cap (u32, stock 4).
/// Raising it during the boot pass removes the ~4-opens/pump throttle so
/// cache-miss loads run at device speed (design §6.2 / research §3.1).
const MGR_OPEN_CAP: usize = 0x70;
/// Default raised cap. Overridable for A/B pacing measurement only via the
/// dev env var `DDR_FAST_BOOT_OPEN_CAP` (NOT operator config).
const DEFAULT_OPEN_CAP: u32 = 64;

/// Step-data manager field offset: the name-records base pointer (name
/// records are 0xA0-stride, path stored inline). Used to resolve a work
/// item's registered game path for the cache key.
const MGR_NAME_RECORDS: usize = 0x28;
/// Name-record stride and the inline path-string offset (research §3.3;
/// confirmed by onUpdate's own `sota.ssq`/`thr8.ssq` compare at
/// `name_base + entry_index*0xA0 + 0x11`).
const NAME_REC_STRIDE: usize = 0xA0;
const NAME_REC_PATH: usize = 0x11;
/// Hard cap on the inline path C-string read (the buffer spans ~+0x11..+0x8E).
const MAX_PATH_LEN: usize = 260;

static mut FAST_BOOTUP_HOOK: Option<GenericDetour<UpdateFn>> = None;
static mut GLOBAL_TABLE_PTR: *const u8 = std::ptr::null();
static mut IN_HOOK: bool = false;
/// One-shot latch for the end-of-list log line (per boot).
static mut COMPLETION_LOGGED: bool = false;

/// Pacing-raise lifecycle (all game-main-thread only, inside the hook).
static mut CAP_RAISED: bool = false;
static mut CAP_RESTORED: bool = false;
static mut STOCK_CAP: u32 = 0;
static mut BOOT_START: Option<std::time::Instant> = None;
static mut PROCESSED_TOTAL: u32 = 0;

/// Step-data cache lifecycle (design §Components → capture/identity/writer).
/// Cache-capture is armed at enable when the shared Analyze dispatcher is
/// available; the gamemdx PE stamp/size are the cache-header invalidators.
static mut GAMEMDX_STAMP: u32 = 0;
static mut GAMEMDX_SIZE: u32 = 0;
static mut CACHE_ARMED: bool = false;
/// First-onUpdate latch for the would-hit log + capture window open.
static mut CACHE_STARTED: bool = false;
/// Completion latch for the writer spawn.
static mut WRITER_SPAWNED: bool = false;

/// Replay path (design §Components → replay; §Data Models). Derived from the
/// resolved onUpdate body in Step 2. `REPLAY_ARMED` requires all three
/// replay-used addresses; missing any ⇒ no replay (stock + capture only,
/// NFR-1c).
type FindMcodeFn = unsafe extern "C" fn(i32) -> *mut u8;
type ReleaseFn = unsafe extern "C" fn(*mut u8, i32);
type HasChartFn = unsafe extern "C" fn(*mut u8, i32, i32) -> u8;

static mut FIND_MCODE: Option<FindMcodeFn> = None;
static mut RELEASE_FN: Option<ReleaseFn> = None;
static mut VARIABLE_BPM_THRESHOLD: f64 = 0.0;
static mut THRESHOLD_RESOLVED: bool = false;
static mut REPLAY_ARMED: bool = false;
/// First-onUpdate latch for the boot-plan build + record flips.
static mut PLAN_BUILT: bool = false;
/// The per-item Replay/Stock plan for this boot, indexed by work-list
/// position. Built once at the first hooked call; read-only afterward.
static BOOT_PLAN: std::sync::OnceLock<Vec<plan::ItemPlan>> = std::sync::OnceLock::new();
/// One-shot latch for the null-mcode-during-replay WARN.
static NULL_MCODE_WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Read whether we're currently inside our own `update_hook` (the boot
/// actor's `onUpdate`). The capture subscriber uses this to ignore any
/// Analyze call that isn't ours.
pub(crate) fn in_hook() -> bool {
    unsafe { std::ptr::read(std::ptr::addr_of!(IN_HOOK)) }
}

/// Read the loaded gamemdx module's PE `TimeDateStamp` (FileHeader) and
/// `SizeOfImage` — the cache-header build invalidators. `SizeOfImage` comes
/// from the module info; the stamp is decoded from the PE header at
/// `base + e_lfanew + 8`. A malformed/unreadable header yields stamp 0 (still
/// a consistent header for the loaded module — only a build change moves the
/// size).
unsafe fn read_pe_stamp_size(base: *const u8, size: usize) -> (u32, u32) {
    let size = size as u32;
    if base.is_null() {
        return (0, size);
    }
    let e_lfanew = (base.add(0x3C) as *const u32).read_unaligned() as usize;
    if e_lfanew == 0 || e_lfanew + 8 + 4 > size as usize {
        return (0, size);
    }
    let stamp = (base.add(e_lfanew + 8) as *const u32).read_unaligned();
    (stamp, size)
}

/// Resolve the desired open cap once. Dev-only env override for the cap-4 vs
/// cap-64 pacing A/B; anything invalid or unset yields the default.
fn desired_open_cap() -> u32 {
    match std::env::var("DDR_FAST_BOOT_OPEN_CAP") {
        Ok(v) => v
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|n| *n > 0)
            .unwrap_or(DEFAULT_OPEN_CAP),
        Err(_) => DEFAULT_OPEN_CAP,
    }
}

/// Read the live manager pointer (`*step_data_global_table`), or null.
unsafe fn manager() -> *const u8 {
    if GLOBAL_TABLE_PTR.is_null() {
        return std::ptr::null();
    }
    *(GLOBAL_TABLE_PTR as *const *const u8)
}

/// First-call latch: stash the stock cap and widen it. No-op (retries next
/// frame) if the manager isn't available yet.
unsafe fn raise_open_cap() {
    let mgr = manager();
    if mgr.is_null() {
        return;
    }
    let cap_ptr = mgr.add(MGR_OPEN_CAP) as *mut u32;
    let stock = cap_ptr.read_unaligned();
    STOCK_CAP = stock;
    let target = desired_open_cap();
    cap_ptr.write_unaligned(target);
    CAP_RAISED = true;
    BOOT_START = Some(std::time::Instant::now());
    log_info!(
        "FastBootup: boot pass begin -- open cap {}->{}",
        stock,
        target
    );
}

/// Completion latch: restore the stock cap and report the pass. Idempotent.
unsafe fn restore_open_cap() {
    if CAP_RESTORED || !CAP_RAISED || STOCK_CAP == 0 {
        return;
    }
    let mgr = manager();
    if !mgr.is_null() {
        (mgr.add(MGR_OPEN_CAP) as *mut u32).write_unaligned(STOCK_CAP);
    }
    CAP_RESTORED = true;
    let stock = STOCK_CAP;
    let processed = PROCESSED_TOTAL;
    let ms = BOOT_START.map(|t| t.elapsed().as_millis()).unwrap_or(0);
    log_info!(
        "FastBootup: boot pass complete -- processed {} items in {} ms (open cap restored to {})",
        processed,
        ms,
        stock
    );
}

unsafe extern "C" fn update_hook(actor: *mut u8) {
    // Belt-and-suspenders: the original (FUN_180032360) never re-enters its own
    // onUpdate, so this branch is effectively unreachable — but if the game ever
    // did re-enter, run the original once and bail rather than loop recursively.
    if IN_HOOK {
        if let Some(ref hook) = FAST_BOOTUP_HOOK {
            hook.call(actor);
        }
        return;
    }
    IN_HOOK = true;

    // First hooked call of the boot: widen the loader's open cap so the pump
    // (driven by the app tick, not us) issues loads as fast as the disk
    // allows. Restored the instant the actor signals completion.
    if !CAP_RAISED {
        raise_open_cap();
    }

    // First hooked call: open the capture window and, if the verifier is
    // ready, log the would-replay rate.
    if CACHE_ARMED && !CACHE_STARTED {
        cache_first_call(actor);
    }

    // First hooked call: build the boot plan (Replay/Stock per item) and flip
    // eligible records 1→6. Only replays when the verifier is ready and the
    // derived addresses resolved; otherwise every item stays Stock.
    if REPLAY_ARMED && !PLAN_BUILT {
        build_boot_plan(actor);
    }

    // Process the work list from the cursor. Replay items apply cached writes
    // and advance the cursor with no per-frame cap (pure memory); Stock items
    // go through the existing settled-buffer gate and the original, bounded by
    // MAX_PER_FRAME. The final item is always Stock (plan invariant), so the
    // game's own completion block runs natively on top of our accumulators.
    let mut processed = 0u32;
    // Defensive iteration cap: bounded by all items replayable in one frame
    // plus the stock cap plus slack. Guards against any cursor-advance bug.
    let mut guard = boot_total_items(actor).saturating_mul(2).saturating_add(64);
    while guard > 0 {
        guard -= 1;
        let pos = match boot_cursor_pos(actor) {
            Some(p) => p,
            None => break,
        };
        match plan_for(pos) {
            plan::ItemPlan::Replay => {
                replay_item(actor);
            }
            plan::ItemPlan::Stock => {
                if processed >= MAX_PER_FRAME {
                    break;
                }
                if !should_process_more(actor) {
                    break;
                }
                let capturing = CACHE_ARMED && capture::is_active();
                if capturing {
                    stash_capture_item(actor);
                }
                if let Some(ref hook) = FAST_BOOTUP_HOOK {
                    hook.call(actor);
                }
                if capturing {
                    capture::harvest();
                }
                processed += 1;
            }
        }
    }
    PROCESSED_TOTAL = PROCESSED_TOTAL.saturating_add(processed);

    // Completion: once the actor has set its done flag (the dispatcher's own
    // gate), the boot pass is over — restore the stock cap, close the capture
    // window, and hand the fresh captures to the writer thread.
    if !CAP_RESTORED
        && (actor.add(ACTOR_FLAGS) as *const u32).read_unaligned() & ACTOR_DONE_MASK != 0
    {
        restore_open_cap();
        finish_cache_pass();
    }

    IN_HOOK = false;
}

/// First-onUpdate cache setup: open the capture window and log the
/// would-replay rate over the work list (verifier-permitting). Latched.
unsafe fn cache_first_call(actor: *mut u8) {
    CACHE_STARTED = true;
    capture::begin();
    if !identity::is_ready() {
        log_info!(
            "FastBootup: cache verifier not ready at boot pass start -- capturing all, no would-hit stats"
        );
        return;
    }
    let (files, files_hit) = would_hit_stats(actor);
    log_info!(
        "FastBootup: step-data cache -- {}/{} file(s) verified for replay",
        files_hit,
        files
    );
}

/// Close the capture window and spawn the writer (latched). Called once the
/// boot pass completes.
unsafe fn finish_cache_pass() {
    if !CACHE_ARMED || WRITER_SPAWNED {
        return;
    }
    WRITER_SPAWNED = true;
    let captured = capture::store_len();
    capture::end();
    log_info!(
        "FastBootup: boot pass captured {} file(s) -- persisting step-data cache",
        captured
    );
    capture::spawn_writer(GAMEMDX_STAMP, GAMEMDX_SIZE);
}

/// Read the work item at the actor's current cursor and stash it for capture.
/// Items with `entry_index <= 0` (unregistered charts) or an unresolvable
/// name record clear the stash so a stray subscriber write can't attach to
/// the wrong file.
unsafe fn stash_capture_item(actor: *mut u8) {
    match current_item(actor) {
        Some((entry_index, difficulty, _mcode)) if entry_index > 0 => {
            match item_game_path(entry_index) {
                Some(gp) => capture::set_item(gp, difficulty),
                None => capture::clear_item(),
            }
        }
        _ => capture::clear_item(),
    }
}

/// Read the `{entry_index, difficulty, mcode}` of the item at the actor's
/// cursor (mirrors `should_process_more`'s cursor read). `None` if the
/// cursor/array is out of range.
unsafe fn current_item(actor: *mut u8) -> Option<(i32, i32, i32)> {
    let phase = (actor.add(ACTOR_PHASE) as *const u16).read_unaligned() as usize;
    let index = (actor.add(ACTOR_COUNTERS + phase * 8) as *const u32).read_unaligned() as usize;
    let array = *(actor.add(ACTOR_WORK_BEGIN) as *const *const u8);
    let end = *(actor.add(ACTOR_WORK_END) as *const *const u8);
    if array.is_null() || end.is_null() {
        return None;
    }
    let total = (end as usize).saturating_sub(array as usize) / WORK_ITEM_STRIDE;
    if index >= total {
        return None;
    }
    let item = array.add(index * WORK_ITEM_STRIDE);
    let entry_index = (item as *const i32).read_unaligned();
    let difficulty = (item.add(4) as *const i32).read_unaligned();
    let mcode = (item.add(8) as *const i32).read_unaligned();
    Some((entry_index, difficulty, mcode))
}

/// Resolve a record's registered game path from the manager's name records
/// (inline C-string at `name_base + entry_index*0xA0 + 0x11`).
unsafe fn item_game_path(entry_index: i32) -> Option<String> {
    if entry_index <= 0 {
        return None;
    }
    let mgr = manager();
    if mgr.is_null() {
        return None;
    }
    let name_base = *(mgr.add(MGR_NAME_RECORDS) as *const *const u8);
    if name_base.is_null() {
        return None;
    }
    let rec = name_base.add((entry_index as usize) * NAME_REC_STRIDE);
    read_cstr(rec.add(NAME_REC_PATH), MAX_PATH_LEN)
}

/// Read a NUL-terminated ASCII/UTF-8 path from game memory, capped.
unsafe fn read_cstr(p: *const u8, max: usize) -> Option<String> {
    let mut bytes = Vec::with_capacity(48);
    for i in 0..max {
        let b = p.add(i).read();
        if b == 0 {
            break;
        }
        bytes.push(b);
    }
    if bytes.is_empty() {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// Count distinct cached files in the work list and how many are verified
/// (would replay). For the first-onUpdate would-hit log only.
unsafe fn would_hit_stats(actor: *mut u8) -> (usize, usize) {
    use std::collections::HashSet;
    let array = *(actor.add(ACTOR_WORK_BEGIN) as *const *const u8);
    let end = *(actor.add(ACTOR_WORK_END) as *const *const u8);
    if array.is_null() || end.is_null() {
        return (0, 0);
    }
    let total = (end as usize).saturating_sub(array as usize) / WORK_ITEM_STRIDE;
    let mut seen: HashSet<String> = HashSet::new();
    let mut hits = 0usize;
    for i in 0..total {
        let entry_index = (array.add(i * WORK_ITEM_STRIDE) as *const i32).read_unaligned();
        if entry_index <= 0 {
            continue;
        }
        if let Some(gp) = item_game_path(entry_index) {
            let verified = identity::verdict(&gp);
            if seen.insert(gp) && verified {
                hits += 1;
            }
        }
    }
    (seen.len(), hits)
}

// ── Step 7: replay path ─────────────────────────────────────────────────

/// Total work-list items (for the loop's defensive iteration cap).
unsafe fn boot_total_items(actor: *mut u8) -> u32 {
    let array = *(actor.add(ACTOR_WORK_BEGIN) as *const *const u8);
    let end = *(actor.add(ACTOR_WORK_END) as *const *const u8);
    if array.is_null() || end.is_null() {
        return 0;
    }
    ((end as usize).saturating_sub(array as usize) / WORK_ITEM_STRIDE) as u32
}

/// The plan for a work-list position: `Replay` only when a boot plan was
/// built and marked it so; otherwise `Stock` (preserves capture-only
/// behavior when replay is unarmed / the verifier wasn't ready).
fn plan_for(pos: usize) -> plan::ItemPlan {
    BOOT_PLAN
        .get()
        .and_then(|v| v.get(pos).copied())
        .unwrap_or(plan::ItemPlan::Stock)
}

/// Loop-control gate for both branches: the done-flag + cursor-bounds check
/// (mirrors the front of `should_process_more`), returning the current
/// work-list position or `None` to stop. Does NOT check record status /
/// buffer readiness — that is the Stock branch's concern.
unsafe fn boot_cursor_pos(actor: *mut u8) -> Option<usize> {
    let flags = (actor.add(ACTOR_FLAGS) as *const u32).read_unaligned();
    if flags & ACTOR_DONE_MASK != 0 {
        return None;
    }
    let phase = (actor.add(ACTOR_PHASE) as *const u16).read_unaligned() as usize;
    let index = (actor.add(ACTOR_COUNTERS + phase * 8) as *const u32).read_unaligned() as usize;
    let array = *(actor.add(ACTOR_WORK_BEGIN) as *const *const u8);
    let end = *(actor.add(ACTOR_WORK_END) as *const *const u8);
    if array.is_null() || end.is_null() {
        return None;
    }
    let total = (end as usize).saturating_sub(array as usize) / WORK_ITEM_STRIDE;
    if index >= total {
        return None;
    }
    Some(index)
}

/// Build the per-item Replay/Stock plan and flip eligible records 1→6.
/// Latched. Only replays when the verifier is ready; a record already
/// in-flight (not status 1) at flip time is left alone and its items are
/// downgraded to Stock (design §Error Handling — self-healing).
unsafe fn build_boot_plan(actor: *mut u8) {
    PLAN_BUILT = true;
    if !identity::is_ready() {
        // Verifier lost the ~1 s race — process everything stock this boot.
        log_info!("FastBootup: verifier not ready at plan build -- all items stock this boot");
        return;
    }

    let array = *(actor.add(ACTOR_WORK_BEGIN) as *const *const u8);
    let end = *(actor.add(ACTOR_WORK_END) as *const *const u8);
    if array.is_null() || end.is_null() {
        return;
    }
    let total = (end as usize).saturating_sub(array as usize) / WORK_ITEM_STRIDE;

    let mut inputs: Vec<plan::PlannedInput> = Vec::with_capacity(total);
    for pos in 0..total {
        let item = array.add(pos * WORK_ITEM_STRIDE);
        let entry_index = (item as *const i32).read_unaligned();
        let difficulty = (item.add(4) as *const i32).read_unaligned();
        let hit = entry_index > 0
            && difficulty >= 0
            && item_game_path(entry_index)
                .map(|gp| identity::item_replayable(&gp, difficulty as u8))
                .unwrap_or(false);
        inputs.push(plan::PlannedInput { entry_index, hit });
    }

    let mut boot = plan::compute(&inputs);

    // Apply flips: only records that are still status 1 (queued, buf/len 0).
    // A record already opened by the pre-batch pump (status 2/3) is left
    // alone and its items downgraded to Stock so the loader owns it (the
    // stock path reads its buffer; replaying it would race the loader).
    let mgr = manager();
    let entries = if mgr.is_null() {
        std::ptr::null()
    } else {
        *(mgr.add(0x08) as *const *const u8)
    };
    let mut downgraded: std::collections::HashSet<i32> = std::collections::HashSet::new();
    let mut flipped = 0usize;
    if entries.is_null() {
        // No record array — can't flip; downgrade every flip candidate.
        downgraded.extend(boot.flips.iter().copied());
    } else {
        for &ei in &boot.flips {
            let record = entries.offset((ei as isize) * (REC_STRIDE as isize));
            let status = (record.add(REC_STATUS) as *const i32).read_unaligned();
            if status == 1 {
                (record.add(REC_STATUS) as *mut i32).write_unaligned(6);
                flipped += 1;
            } else {
                downgraded.insert(ei);
            }
        }
    }
    if !downgraded.is_empty() {
        for (i, input) in inputs.iter().enumerate() {
            if downgraded.contains(&input.entry_index) {
                boot.items[i] = plan::ItemPlan::Stock;
            }
        }
    }

    let replay_n = boot.replay_count();
    let _ = BOOT_PLAN.set(boot.items);
    log_info!(
        "FastBootup: replay plan -- {}/{} items from cache, {} record(s) flipped, {} in-flight downgraded",
        replay_n,
        total,
        flipped,
        downgraded.len()
    );
}

/// Replay the item at the actor's current cursor: apply the cached music-DB
/// writes + actor accumulators, queue the game's release, advance the cursor,
/// and update the percent — the pure-memory equivalent of one stock
/// `onUpdate` iteration. Always advances the cursor (even on a data/mcode
/// miss) so the loop can never stall.
unsafe fn replay_item(actor: *mut u8) {
    if let Some((entry_index, difficulty, mcode)) = current_item(actor) {
        // Fetch the two cached mode payloads + the file's radar special case.
        let fetched = if entry_index > 0 && difficulty >= 0 {
            item_game_path(entry_index).and_then(|gp| {
                let special = replay::special_file(&gp);
                identity::replay_payloads(&gp, difficulty as u8).map(|p| (p, special))
            })
        } else {
            None
        };

        if let Some((slots, special)) = fetched {
            let entry = FIND_MCODE.map(|f| f(mcode)).unwrap_or(std::ptr::null_mut());
            if entry.is_null() {
                if !NULL_MCODE_WARNED.swap(true, std::sync::atomic::Ordering::AcqRel) {
                    crate::log_warn!(
                        "FastBootup: replay found no music-DB entry for mcode {} -- skipping its DB writes (release + advance still applied)",
                        mcode
                    );
                }
            } else {
                apply_db_writes(actor, entry, difficulty, &slots, special);
            }
        }

        // Release exactly once per item, as stock onUpdate does.
        if let (Some(rel), mgr) = (RELEASE_FN, manager() as *mut u8) {
            if !mgr.is_null() {
                rel(mgr, entry_index);
            }
        }
    }

    // Advance the cursor + percent regardless (mirrors onUpdate's tail).
    advance_cursor_and_percent(actor);
}

/// Apply one item's cached music-DB writes (both modes) and fold its radar
/// into the actor's per-side accumulators. Transcribes onUpdate's post-Analyze
/// arithmetic via the pure `replay::compute_slot` / `fold_radar`.
unsafe fn apply_db_writes(
    actor: *mut u8,
    entry: *mut u8,
    difficulty: i32,
    slots: &[cache::SlotPayload; 2],
    special: replay::SpecialFile,
) {
    let threshold = VARIABLE_BPM_THRESHOLD;
    for mode in 0..2usize {
        let payload = &slots[mode];
        let has_chart = has_chart(entry, mode as i32, difficulty);
        let w = replay::compute_slot(payload, has_chart, threshold);
        let idx = (difficulty as usize) + mode * 5; // 0..9

        (entry.add(ENTRY_MAX_BPM + idx * 4) as *mut i32).write_unaligned(w.max_bpm);
        (entry.add(ENTRY_CORE_BPM + idx * 4) as *mut i32).write_unaligned(w.core_bpm);
        (entry.add(ENTRY_MIN_BPM + idx * 4) as *mut i32).write_unaligned(w.min_bpm);
        (entry.add(ENTRY_SHOCK + idx) as *mut u8).write_unaligned(w.shock as u8);
        (entry.add(ENTRY_VARIABLE_BPM + idx) as *mut u8).write_unaligned(w.variable_bpm as u8);
        (entry.add(ENTRY_FLAG_12E + idx) as *mut u8).write_unaligned(w.flag_12e as u8);
        (entry.add(ENTRY_EX_SCORE + idx * 4) as *mut i32).write_unaligned(w.ex_score);

        // Song-wide BPM accumulators (skip-zero; max for +0x94, min for +0x96).
        if let Some(v) = w.song_max_bpm {
            let p = entry.add(ENTRY_SONG_MAX_BPM) as *mut u16;
            let cur = p.read_unaligned();
            p.write_unaligned(if cur == 0 { v } else { cur.max(v) });
        }
        if let Some(v) = w.song_min_bpm {
            let p = entry.add(ENTRY_SONG_MIN_BPM) as *mut u16;
            let cur = p.read_unaligned();
            p.write_unaligned(if cur == 0 { v } else { cur.min(v) });
        }

        // Corruption flag: set only (never the reporter — FR-3/D9).
        if w.corrupt {
            (entry.add(ENTRY_CORRUPT_FLAG) as *mut u8).write_unaligned(1);
        }
    }

    // Radar accumulators are per-side: side 0 → +0xA8, side 1 → +0xBC. Each
    // side folds its own mode's radar block (sota/thr8 gate on indices 0/1).
    for side in 0..2usize {
        let base = if side == 0 {
            ACTOR_RADAR_ACC_SIDE0
        } else {
            ACTOR_RADAR_ACC_SIDE1
        };
        let mut acc = [0i32; 5];
        let ap = actor.add(base) as *const i32;
        for (i, v) in acc.iter_mut().enumerate() {
            *v = ap.add(i).read_unaligned();
        }
        replay::fold_radar(&mut acc, &slots[side].radar, special);
        let mp = actor.add(base) as *mut i32;
        for (i, v) in acc.iter().enumerate() {
            mp.add(i).write_unaligned(*v);
        }
    }
}

/// Call the music-DB entry's `hasChart(mode, difficulty)` vfunc (the
/// build's derived slot). False on any null pointer or an underived slot
/// (never a chart ⇒ never a corruption flag).
unsafe fn has_chart(entry: *mut u8, mode: i32, difficulty: i32) -> bool {
    if entry.is_null() {
        return false;
    }
    let vtable = *(entry as *const *const u8);
    if vtable.is_null() {
        return false;
    }
    let vslot = std::ptr::read(std::ptr::addr_of!(ENTRY_HAS_CHART_VFUNC));
    if vslot == 0 {
        return false;
    }
    let fptr = *(vtable.add(vslot) as *const *const u8);
    if fptr.is_null() {
        return false;
    }
    let f: HasChartFn = std::mem::transmute(fptr);
    f(entry, mode, difficulty) != 0
}

/// Advance the per-phase cursor and rewrite the loading-percent display,
/// reproducing the tail of stock onUpdate (counter++, zero aux + later
/// phases, `percent = counter*100/total`).
unsafe fn advance_cursor_and_percent(actor: *mut u8) {
    let counters = actor.add(ACTOR_COUNTERS);
    let phase = (actor.add(ACTOR_PHASE) as *const u16).read_unaligned() as usize;

    let cp = counters.add(phase * 8) as *mut u32;
    cp.write_unaligned(cp.read_unaligned().wrapping_add(1));
    (counters.add(phase * 8 + 4) as *mut u32).write_unaligned(0);

    let phase_count = (actor.add(ACTOR_PHASE_COUNT) as *const u16).read_unaligned() as usize;
    let mut ph = phase + 1;
    while ph < phase_count {
        (counters.add(ph * 8) as *mut u32).write_unaligned(0);
        (counters.add(ph * 8 + 4) as *mut u32).write_unaligned(0);
        ph += 1;
    }

    let array = *(actor.add(ACTOR_WORK_BEGIN) as *const *const u8);
    let end = *(actor.add(ACTOR_WORK_END) as *const *const u8);
    if array.is_null() || end.is_null() {
        return;
    }
    let total = (end as usize).saturating_sub(array as usize) / WORK_ITEM_STRIDE;
    if total == 0 {
        return;
    }
    let counter = cp.read_unaligned() as u64;
    let percent = (counter * 100 / total as u64) as u32;
    let disp = *(actor.add(ACTOR_PERCENT_PTR) as *const *mut u32);
    if !disp.is_null() {
        disp.write_unaligned(percent);
    }
}

/// True if the entry at the actor's current cursor is safe for the original to
/// process this instant: the actor isn't done, the cursor is within the work
/// list, the entry is in a processable status AND (if it carries an SSQ
/// buffer) that buffer is a complete chunk list fully contained within its
/// bounds. Returns false to defer (stop the batch) when the buffer isn't
/// settled yet or the cursor has run out of work.
unsafe fn should_process_more(actor: *mut u8) -> bool {
    // Defensive visibility barrier: pairs with the worker thread's buffer/status
    // writes. The real safety, though, is the bounded validation below — if the
    // buffer bytes aren't visible/complete yet, the walk fails and we defer.
    std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);

    // Done-flag gate — the actor dispatcher's exact check (`FUN_18021dc70`
    // message 0x102 requires `[actor+0x20] & 0x24 == 0` before calling
    // onUpdate). onUpdate sets bit 2 when the final work item is processed;
    // one more call past that point reads off the end of the work array.
    let flags = (actor.add(ACTOR_FLAGS) as *const u32).read_unaligned();
    if flags & ACTOR_DONE_MASK != 0 {
        return false;
    }

    // Cursor bounds gate — mirror onUpdate's own completion condition
    // (`counter[phase] < (end - begin) / 12`). The cursor is per-phase:
    // `[actor+0x58 + phase*8]` with the phase index at `actor+0x82`.
    let phase = (actor.add(ACTOR_PHASE) as *const u16).read_unaligned() as usize;
    let index = (actor.add(ACTOR_COUNTERS + phase * 8) as *const u32).read_unaligned();
    let array_ptr = *(actor.add(ACTOR_WORK_BEGIN) as *const *const u8);
    let array_end = *(actor.add(ACTOR_WORK_END) as *const *const u8);
    if array_ptr.is_null() || array_end.is_null() {
        return false;
    }
    let total = (array_end as usize).saturating_sub(array_ptr as usize) / WORK_ITEM_STRIDE;
    if index as usize >= total {
        // Work list fully processed (or cursor otherwise out of range). The
        // pre-fix code kept going here and read work_array[total] — heap
        // garbage that could send onUpdate into a NULL-deref mcode lookup
        // (the intermittent "97% loading" boot crash). Log once so field
        // reports can confirm the gate is doing its job.
        if !COMPLETION_LOGGED {
            COMPLETION_LOGGED = true;
            log_info!(
                "FastBootup: work list complete ({} items, phase {}) — stopping batch at list end",
                total,
                phase
            );
        }
        return false;
    }

    let entry_index =
        (array_ptr.add((index as usize) * WORK_ITEM_STRIDE) as *const i32).read_unaligned();
    if entry_index == 0 {
        return false;
    }

    let table = *(GLOBAL_TABLE_PTR as *const *const u8);
    if table.is_null() {
        return false;
    }

    let entries = *(table.add(0x08) as *const *const u8);
    if entries.is_null() {
        return false;
    }

    // Signed offset on purpose: the game stores -1 for songs whose SSQ file
    // couldn't be registered and reads the (garbage) record just before the
    // array — we must gate on the same bytes it will read.
    let record = entries.offset((entry_index as isize) * (REC_STRIDE as isize));
    let status = (record.add(REC_STATUS) as *const i32).read_unaligned();
    if !READY_STATUSES.contains(&status) {
        return false;
    }

    // The game only walks the SSQ chunk list when both the buffer pointer and
    // length are set (its own `local_f8 != 0 && local_f0 != 0` guard). If either
    // is absent (idle/failed entries) there is nothing to walk — let it through
    // so the cursor advances, exactly as the game would.
    let buf = *(record.add(REC_BUF) as *const *const u8);
    let len = (record.add(REC_LEN) as *const u32).read_unaligned() as usize;
    if buf.is_null() || len == 0 {
        return true;
    }

    // Buffer present: only proceed if it is fully loaded/settled, proven by a
    // strictly bounds-checked mirror of the game's chunk walk.
    ssq_chunk_list_walkable(buf, len)
}

/// Bounds-checked replica of `FUN_1801cbdc0`'s SSQ chunk-list traversal.
///
/// Chunk header: `{+0: u32 length, +4: u16 type, +6: u16 mark}`. The game stops
/// on `length==0`, `type==2`, or `mark==0xffff`, advancing `ptr += length` each
/// step. We follow the identical logic but verify every access stays inside
/// `[buf, buf+len)`. Returns true only if traversal reaches a valid terminator
/// without ever stepping outside the buffer — which guarantees the game's own
/// (identical) walk also stays in-bounds. A partially-written buffer either
/// exceeds bounds or fails to terminate within `MAX_CHUNKS`, and we return
/// false (defer).
unsafe fn ssq_chunk_list_walkable(buf: *const u8, len: usize) -> bool {
    let base = buf as usize;
    let end = base + len;
    let mut p = base;

    for _ in 0..MAX_CHUNKS {
        // Need 4 bytes for the length field (the exact access that faulted:
        // `CMP dword ptr [R8],EDI`).
        if p + 4 > end {
            return false;
        }
        let chunk_len = (p as *const u32).read_unaligned();
        // `*ptr == 0` terminator.
        if chunk_len == 0 {
            return true;
        }
        // Need the full 8-byte header for the type/mark checks.
        if p + 8 > end {
            return false;
        }
        let ctype = ((p + 4) as *const u16).read_unaligned();
        if ctype == 2 {
            return true;
        }
        let mark = ((p + 6) as *const u16).read_unaligned();
        if mark == 0xffff {
            return true;
        }
        // Advance by the chunk length. `checked_add` guards against a garbage
        // length that would wrap the address space.
        match p.checked_add(chunk_len as usize) {
            Some(next) if next <= end => p = next,
            _ => return false,
        }
    }

    // Ran past the chunk cap without terminating — treat as not-ready.
    false
}

pub struct FastBootupMod {
    update_addr: *const u8,
}

unsafe impl Send for FastBootupMod {}

impl FastBootupMod {
    pub fn new() -> Self {
        Self {
            update_addr: std::ptr::null(),
        }
    }
}

impl Mod for FastBootupMod {
    fn id(&self) -> &str {
        "fast-bootup"
    }
    fn name(&self) -> &str {
        "Fast Bootup"
    }
    fn description(&self) -> &str {
        "Significantly speeds up the initial game boot time"
    }
    fn required_signatures(&self) -> &[&str] {
        &["check_step_data_update", "step_data_global_table"]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        self.update_addr = ctx.signatures.require_address("check_step_data_update");
        unsafe {
            GLOBAL_TABLE_PTR = ctx.signatures.require_address("step_data_global_table");
            let (stamp, size) = read_pe_stamp_size(ctx.game_module.base, ctx.game_module.size);
            GAMEMDX_STAMP = stamp;
            GAMEMDX_SIZE = size;

            // Replay path: resolve the derived addresses (soft — a miss just
            // disables replay; capture/pacing/plain fast-bootup are unaffected,
            // NFR-1c). `variable_bpm_threshold` is a pointer to the f64 global;
            // read its value once here.
            if let Some(a) = ctx.signatures.get_address("find_music_by_mcode") {
                FIND_MCODE = Some(std::mem::transmute::<*const u8, FindMcodeFn>(a));
            }
            if let Some(a) = ctx.signatures.get_address("step_data_release") {
                RELEASE_FN = Some(std::mem::transmute::<*const u8, ReleaseFn>(a));
            }
            if let Some(a) = ctx.signatures.get_address("variable_bpm_threshold") {
                VARIABLE_BPM_THRESHOLD = *(a as *const f64);
                THRESHOLD_RESOLVED = true;
            }
            if let Some(slot) = ctx.signatures.entry_has_chart_vslot() {
                ENTRY_HAS_CHART_VFUNC = slot;
            }
        }
        // Register the boot-capture subscriber on the shared Analyze
        // dispatcher (harmless if the dispatcher never armed — the subscriber
        // self-gates on the boot window).
        capture::register();
        true
    }

    fn enable(&mut self) {
        unsafe {
            let target: UpdateFn = std::mem::transmute(self.update_addr);
            match crate::core::hooks::install_enabled(
                std::ptr::addr_of_mut!(FAST_BOOTUP_HOOK),
                target,
                update_hook,
            ) {
                Ok(()) => {
                    log_info!("FastBootup: enabled -- loading screen acceleration active");
                }
                Err(e) => {
                    crate::log_error!("FastBootup: failed to hook: {}", e);
                }
            }

            // Arm the boot-time step-data cache. Capture requires the shared
            // Analyze dispatcher (it reads the per-chart analyzer outputs at
            // that boundary). Replay additionally requires the derived
            // addresses (music-DB lookup, release, threshold). The verifier
            // reads + stats the bin off-thread so its index is ready before
            // the boot actor's first onUpdate.
            let find_ok = std::ptr::addr_of!(FIND_MCODE).read().is_some();
            let release_ok = std::ptr::addr_of!(RELEASE_FN).read().is_some();
            let threshold_ok = std::ptr::read(std::ptr::addr_of!(THRESHOLD_RESOLVED));
            let has_chart_ok = std::ptr::read(std::ptr::addr_of!(ENTRY_HAS_CHART_VFUNC)) != 0;
            let cache_armed = crate::services::analyze_hook::is_available();
            let replay_armed = find_ok && release_ok && threshold_ok && has_chart_ok;
            if !has_chart_ok {
                crate::log_warn!(
                    "FastBootup: hasChart vtable slot underived -- step-data cache replay disabled (capture + loader pacing only)"
                );
            }
            CACHE_ARMED = cache_armed;
            REPLAY_ARMED = replay_armed;
            if cache_armed || replay_armed {
                let stamp = std::ptr::read(std::ptr::addr_of!(GAMEMDX_STAMP));
                let size = std::ptr::read(std::ptr::addr_of!(GAMEMDX_SIZE));
                identity::spawn_verifier(stamp, size);
            }
            log_info!(
                "FastBootup: step-data cache armed (capture={}, replay={})",
                cache_armed,
                replay_armed
            );
            if !cache_armed {
                crate::log_warn!(
                    "FastBootup: Analyze dispatcher unavailable -- step-data cache capture inert"
                );
            }
        }
    }

    fn disable(&mut self) {
        unsafe {
            // Restore the open cap if we raised it mid-boot (idempotent).
            restore_open_cap();
            FAST_BOOTUP_HOOK = None;
        }
        // Close the capture window if disabled mid-boot (idempotent).
        capture::end();
        log_info!("FastBootup: disabled");
    }
}
