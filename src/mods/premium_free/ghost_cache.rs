//! Same-credit PB ghost cache — the World port of the DDR A3 `pfree_ghost`
//! hook.
//!
//! ## The bug (RE 2026-09-01, gamemdx 20260721)
//!
//! `sequence::dance::GhostActor` init (`ghost_actor_init`) resolves the
//! pacemaker ghost through the player's score DB: a same-credit personal
//! best is recorded there as a NEGATIVE ghost id encoding a LOCAL stage slot
//! (`(side, stage) = ((-1-id)/5, (-1-id)%5)`), and init copies that slot's
//! grade stream (`PlayerWork + 0x590 + stage*0x2B8 + 0xB8`) into the actor's
//! ghost vector (`actor+0x98`), sets its state to 2 and raises the pacemaker
//! visibility byte (`*(actor+0x88)+0xC0`). In vanilla the slot is a
//! previous stage's record, so the copy is real.
//!
//! Under Premium Free the counter is frozen, so the "local slot" is ALWAYS
//! the current stage's record — which the stale-record fix virginises and
//! the game re-prepares (streams wiped) at every song select. The copy
//! therefore yields an EMPTY vector: on any replay of a chart you PB'd this
//! credit, the pacemaker target reads 0 and the ghost comparison is blank.
//! Identical to A3's "ghost is 0" bug, with the same actor offsets.
//!
//! ## The fix (A3 mechanism, zero note-packing)
//!
//! 1. **Cache** (post-original detour on `result_commit`): the commit has
//!    just written the record at the frozen index — snapshot its grade
//!    stream (`rec+0xB8..0xC0`, one byte per note, the exact bytes the ghost
//!    consumer wants) keyed by `(side, mcode, style, difficulty)` from the
//!    record header (`rec+0x00/+0x08/+0x04`), keep-if-better on `rec+0x10`.
//! 2. **Inject** (post-original detour on `ghost_actor_init`): when the
//!    freeze is active and the game left `actor+0x98` empty — and no network
//!    load is in flight (`state==0 && id>0`, A3's rule) — look the chart up
//!    via the freshly prepared record header at the frozen index (same key
//!    domain and clamping as the store side), copy the cached bytes in via
//!    the game's OWN `vector<u8>` copy-assign (`ghost_vec_copy` — allocator
//!    correct), set state 2 + timer 0 exactly like the local-slot branch,
//!    and raise the visibility byte.
//!
//! Injection is gated on the freeze patch so vanilla stays literally stock;
//! caching is unconditional-while-enabled (cheap, and the cache must exist
//! before the operator flips the option). The cache is cleared on every
//! EAM_EXIT / attract entry (session-scoped). Fail-open: any unresolved
//! signature ⇒ no detours, one WARN.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use once_cell::sync::OnceCell;
use retour::GenericDetour;

use crate::core::memory;
use crate::core::msvc::MsvcVec;
use crate::core::signatures::SignatureStore;
use crate::services::stage_records;
use crate::{log_info, log_warn};

use super::diag;

// ── Record layout (docs/premium_free_stale_record_bug.md + s_marvelous RE) ──
pub(super) const REC_MCODE: usize = 0x00;
pub(super) const REC_DIFF: usize = 0x04;
pub(super) const REC_STYLE: usize = 0x08;
pub(super) const REC_SCORE: usize = 0x10;
/// `vector<u8>` grade class per judged note — the ghost stream.
pub(super) const REC_GRADES_VEC: usize = 0xB8;

// ── GhostActor layout (ghost_actor_init disassembly, 20260721) ─────────
/// State array base — `i32 state, f32 timer` pairs, indexed by `+0x82`.
const GA_STATE_BASE: usize = 0x58;
const GA_STATE_IDX: usize = 0x82;
/// Owning side (i32).
const GA_SIDE: usize = 0x84;
/// Companion NoteResultActor pointer; its `+0xC0` byte is the pacemaker
/// visibility flag (`pacemaker_swap` forces the same byte).
const GA_PLAY: usize = 0x88;
const GA_READY_BYTE: usize = 0xC0;
/// Ghost id from the score-DB lookup (i64; <0 local slot, 0 none, >0 net).
const GA_GHOST_ID: usize = 0x90;
/// `vector<u8>` ghost stream.
const GA_VEC: usize = 0x98;

// ── Result commit (GamePlayActor) ──────────────────────────────────────
/// Owning side (i32).
const GPA_SIDE: usize = 0x84;

/// Cap on the cached stream size (a chart never has this many notes).
const MAX_GHOST_BYTES: usize = 100_000;
/// Cap on distinct cached charts per session.
const MAX_ENTRIES: usize = 256;

type ActorFn = unsafe extern "C" fn(*mut u8);
/// `vector<u8>::operator=(const vector&)` — `(dst, src) -> dst`.
type VecCopyFn = unsafe extern "C" fn(*mut u8, *const u8) -> u64;

static COMMIT_DETOUR: OnceCell<GenericDetour<ActorFn>> = OnceCell::new();
static GHOST_DETOUR: OnceCell<GenericDetour<ActorFn>> = OnceCell::new();
static VEC_COPY: OnceCell<VecCopyFn> = OnceCell::new();

static INSTALLED: AtomicBool = AtomicBool::new(false);
/// Enabled by the mod's `enable`, cleared by `disable`.
static ENABLED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    side: i32,
    mcode: i32,
    style: i32,
    diff: i32,
}

struct Entry {
    key: Key,
    score: i32,
    bytes: Vec<u8>,
}

static CACHE: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

/// Install both detours + resolve the copy fn. All-or-nothing; fail-open.
pub fn install(signatures: &SignatureStore) -> bool {
    if INSTALLED.load(Ordering::Acquire) {
        return true;
    }
    let (Some(commit), Some(ghost), Some(copy)) = (
        signatures.get_address("result_commit"),
        signatures.get_address("ghost_actor_init"),
        signatures.get_address("ghost_vec_copy"),
    ) else {
        log_warn!(
            "PremiumFree: ghost-cache signatures unresolved -- same-credit PB ghost stays broken under the freeze"
        );
        return false;
    };
    diag::decode_commit_skip_offsets(commit);
    unsafe {
        let commit_fn: ActorFn = std::mem::transmute(commit);
        let ghost_fn: ActorFn = std::mem::transmute(ghost);
        let copy_fn: VecCopyFn = std::mem::transmute(copy);
        let (Ok(d1), Ok(d2)) = (
            GenericDetour::new(commit_fn, commit_hook),
            GenericDetour::new(ghost_fn, ghost_hook),
        ) else {
            log_warn!("PremiumFree: ghost-cache detour creation failed");
            return false;
        };
        if d1.enable().is_err() || d2.enable().is_err() {
            log_warn!("PremiumFree: ghost-cache detour enable failed");
            return false;
        }
        let _ = VEC_COPY.set(copy_fn);
        let _ = COMMIT_DETOUR.set(d1);
        let _ = GHOST_DETOUR.set(d2);
    }
    INSTALLED.store(true, Ordering::Release);
    log_info!("PremiumFree: ghost cache installed (result_commit + GhostActor init detours)");
    true
}

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Release);
    if !on {
        clear();
    }
}

/// Drop every cached ghost (session end).
pub fn clear() {
    if let Ok(mut c) = CACHE.lock() {
        if !c.is_empty() {
            log_info!("PremiumFree: ghost cache cleared ({} entries)", c.len());
            c.clear();
        }
    }
}

// ── Result commit: snapshot the committed grade stream ────────────────

unsafe extern "C" fn commit_hook(actor: *mut u8) {
    // Diagnostic pre-tap: the commit's early-outs (panic-free reads).
    if ENABLED.load(Ordering::Acquire) && !actor.is_null() {
        diag::on_result_commit_pre(actor);
    }
    if let Some(d) = COMMIT_DETOUR.get() {
        d.call(actor);
    }
    if ENABLED.load(Ordering::Acquire) && !actor.is_null() {
        if let Err(e) = std::panic::catch_unwind(|| store_from_commit(actor)) {
            let _ = e;
        }
    }
}

fn store_from_commit(actor: *mut u8) {
    unsafe {
        let side = memory::read_i32(actor.add(GPA_SIDE));
        if !(0..=1).contains(&side) {
            return;
        }
        // Course mode commits elsewhere (+0x2D8) and GhostActor's course
        // branch reads the course record — leave it stock.
        if let Some(gw) = stage_records::game_work() {
            if memory::read_u64(gw.add(stage_records::course_field_offset())) != 0 {
                return;
            }
        }
        let Some(stage) = stage_records::stage_counter() else {
            return;
        };
        if !(0..stage_records::MAX_STAGE_RECORDS as i32).contains(&stage) {
            return;
        }
        let Some(rec) = stage_records::stage_record(side as usize, stage as usize) else {
            return;
        };
        let mcode = memory::read_i32(rec.add(REC_MCODE));
        if mcode < 0 {
            return;
        }
        let key = Key {
            side,
            mcode,
            style: memory::read_i32(rec.add(REC_STYLE)),
            diff: memory::read_i32(rec.add(REC_DIFF)),
        };
        let score = memory::read_i32(rec.add(REC_SCORE));
        let Some(bytes) = read_grade_stream(rec) else {
            diag::on_result_commit_post(side, stage, rec, None);
            return;
        };
        diag::on_result_commit_post(side, stage, rec, Some(bytes.len()));
        if bytes.is_empty() {
            return;
        }
        let Ok(mut cache) = CACHE.lock() else {
            return;
        };
        if let Some(e) = cache.iter_mut().find(|e| e.key == key) {
            if score < e.score {
                log_info!(
                    "PremiumFree: ghost cache keep P{} mcode={} style={} diff={} (score {} < cached {})",
                    side + 1,
                    mcode,
                    key.style,
                    key.diff,
                    score,
                    e.score
                );
                return;
            }
            e.score = score;
            e.bytes = bytes;
        } else {
            if cache.len() >= MAX_ENTRIES {
                cache.remove(0);
            }
            cache.push(Entry { key, score, bytes });
        }
        log_info!(
            "PremiumFree: ghost cache store P{} mcode={} style={} diff={} score={} notes={}",
            side + 1,
            mcode,
            key.style,
            key.diff,
            score,
            cache
                .iter()
                .find(|e| e.key == key)
                .map_or(0, |e| e.bytes.len())
        );
    }
}

/// Copy `rec+0xB8..0xC0` out (bounded).
unsafe fn read_grade_stream(rec: *const u8) -> Option<Vec<u8>> {
    let begin = memory::read_ptr(rec.add(REC_GRADES_VEC)) as *const u8;
    let end = memory::read_ptr(rec.add(REC_GRADES_VEC + 8)) as *const u8;
    if begin.is_null() || (end as usize) < (begin as usize) {
        return None;
    }
    let len = end as usize - begin as usize;
    if len > MAX_GHOST_BYTES {
        return None;
    }
    Some(std::slice::from_raw_parts(begin, len).to_vec())
}

// ── GhostActor init: inject when the game resolved nothing ────────────

unsafe extern "C" fn ghost_hook(actor: *mut u8) {
    if let Some(d) = GHOST_DETOUR.get() {
        d.call(actor);
    }
    if ENABLED.load(Ordering::Acquire) && !actor.is_null() {
        if let Err(e) = std::panic::catch_unwind(|| apply_to_ghost(actor)) {
            let _ = e;
        }
    }
}

fn apply_to_ghost(actor: *mut u8) {
    unsafe {
        let side = memory::read_i32(actor.add(GA_SIDE));
        if !(0..=1).contains(&side) {
            return;
        }
        let ghost_id = memory::read_u64(actor.add(GA_GHOST_ID)) as i64;
        let idx = (memory::read_u32(actor.add(GA_STATE_IDX)) & 0xFFFF) as usize;
        if idx > 8 {
            return; // state index out of any plausible range
        }
        let state_ptr = actor.add(GA_STATE_BASE + idx * 8) as *mut i32;
        let state = *state_ptr;
        let have = {
            let b = memory::read_ptr(actor.add(GA_VEC)) as usize;
            let e = memory::read_ptr(actor.add(GA_VEC + 8)) as usize;
            e.saturating_sub(b)
        };

        // Chart identity = the record the song-select commit just prepared at
        // the frozen index (same key domain + clamping as the store side).
        let Some(stage) = stage_records::stage_counter() else {
            return;
        };
        if !(0..stage_records::MAX_STAGE_RECORDS as i32).contains(&stage) {
            return;
        }
        let Some(rec) = stage_records::stage_record(side as usize, stage as usize) else {
            return;
        };
        let key = Key {
            side,
            mcode: memory::read_i32(rec.add(REC_MCODE)),
            style: memory::read_i32(rec.add(REC_STYLE)),
            diff: memory::read_i32(rec.add(REC_DIFF)),
        };

        // A3 rule: never touch an in-flight network load; otherwise inject
        // whenever the id is a local slot OR the vector came back empty.
        let network_in_flight = state == 0 && ghost_id > 0;
        let wants = !network_in_flight && (ghost_id < 0 || have == 0);

        if !super::freeze_active() {
            log_info!(
                "PremiumFree: ghost P{} mcode={} diff={} id={} state={} vec={} (freeze off -- stock)",
                side + 1,
                key.mcode,
                key.diff,
                ghost_id,
                state,
                have
            );
            return;
        }
        if !wants || key.mcode < 0 {
            log_info!(
                "PremiumFree: ghost P{} mcode={} diff={} id={} state={} vec={} (no injection needed)",
                side + 1,
                key.mcode,
                key.diff,
                ghost_id,
                state,
                have
            );
            return;
        }

        // Snapshot under the lock; never call game code while holding it.
        let bytes: Vec<u8> = {
            let Ok(cache) = CACHE.lock() else {
                return;
            };
            match cache.iter().find(|e| e.key == key) {
                Some(e) => e.bytes.clone(),
                None => {
                    drop(cache);
                    log_info!(
                        "PremiumFree: ghost P{} mcode={} style={} diff={} id={} state={} vec={} -- no cached same-chart PB",
                        side + 1,
                        key.mcode,
                        key.style,
                        key.diff,
                        ghost_id,
                        state,
                        have
                    );
                    return;
                }
            }
        };
        let Some(copy) = VEC_COPY.get() else {
            return;
        };
        let src = MsvcVec::<u8> {
            begin: bytes.as_ptr(),
            end: bytes.as_ptr().add(bytes.len()),
            cap_end: bytes.as_ptr().add(bytes.len()),
        };
        copy(actor.add(GA_VEC), &src as *const MsvcVec<u8> as *const u8);

        // Mirror the local-slot branch: state 2, timer 0, pacemaker visible.
        *state_ptr = 2;
        *(actor.add(GA_STATE_BASE + idx * 8 + 4) as *mut f32) = 0.0;
        let play = memory::read_ptr(actor.add(GA_PLAY)) as *mut u8;
        if !play.is_null() {
            memory::write_u8(play.add(GA_READY_BYTE), 1);
        }
        log_info!(
            "PremiumFree: ghost injected P{} mcode={} style={} diff={} id={} state={} had={} bytes={}",
            side + 1,
            key.mcode,
            key.style,
            key.diff,
            ghost_id,
            state,
            have,
            bytes.len()
        );
    }
}
