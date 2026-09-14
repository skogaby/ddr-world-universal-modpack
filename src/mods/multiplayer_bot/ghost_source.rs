//! The Target Score replay's data source: the HUMAN's loaded pacemaker ghost,
//! read from their `sequence::dance::GhostActor` — engine-facing.
//!
//! Zero detours. The GhostActor is a child the `GamePlayActor` holds at a
//! BUILD-DEPENDENT field (`+0x1F8` on 20260324+, `+0x1F0` on 20250805 /
//! 20260224 — the GamePlayActor layout fork sits at this field), published by
//! `derive_ghost_actor_probe` as `gpa_ghost_actor_off` from the actor's
//! state-2 wait site with the `isReady` callee prologue as identity gate.
//! Every pointer read here is `memory::is_readable`-probed, the actor is
//! RTTI-vtable gated (`ghost_actor_vtable`), and its state must read 2
//! (ready) — which, per the RE, it always does by the time the bot's first
//! judge frame runs: `GamePlayActor::onUpdate` holds state 2 until the ghost
//! is ready (download success, failure, or request failure), so the vector is
//! FINAL when the judging state begins.
//!
//! Called once per Results rebuild from the judge pre-callback (game thread);
//! nothing here runs per frame.

use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::services::song_reset;
use crate::{log_info, log_warn};

// ── GamePlayActor ───────────────────────────────────────────────────────
const GPA_SIDE: usize = 0x84;

// ── GhostActor (A3-identical 64-bit layout; state fields re-attested per
// build by the probe derivation's identity gate) ────────────────────────
const GA_STATE_BASE: usize = 0x58;
const GA_STATE_IDX: usize = 0x82;
const GA_GHOST_ID: usize = 0x90;
const GA_VEC: usize = 0x98;
/// Bytes probed on the GhostActor (`+0x00` vtable .. `+0xA8` vector cap).
const GA_PROBE_LEN: usize = 0xA8;
/// Ready state of the GhostActor's state machine.
const GA_STATE_READY: i32 = 2;
/// Cap on the ghost stream (a chart never has this many notes).
const MAX_GHOST_BYTES: usize = 100_000;

/// Published `gpa_ghost_actor_off` (0 = underived).
static GPA_GHOST_OFF: AtomicUsize = AtomicUsize::new(0);
/// Resolved `ghost_actor_vtable` (null = unresolved).
static GHOST_VTABLE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());

/// The human's ghost as read from their GhostActor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ghost {
    /// `GhostActor+0x90`: < 0 local stage slot, 0 none, > 0 network id.
    pub id: i64,
    /// One grade class per Results entry, chart order.
    pub bytes: Vec<u8>,
}

/// Why the ghost could not be read. Every variant is a per-song fallback
/// (Level 10) in the filler; `reason()` is the WARN's word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// `gpa_ghost_actor_off` / `ghost_actor_vtable` underived this boot.
    Unavailable,
    /// No live DPS or no GamePlayActor for the human side.
    NoActor,
    /// A pointer failed its readability probe.
    Unreadable,
    /// The field did not point at a GhostActor (vtable mismatch).
    Identity,
    /// The GhostActor is not in its ready state (carries the state read).
    NotReady(i32),
    /// The ghost vector is empty (id 0, empty or failed download).
    Empty,
    /// The vector length is implausible.
    TooLong,
}

impl Refusal {
    pub fn reason(self) -> &'static str {
        match self {
            Refusal::Unavailable => "derivation missing",
            Refusal::NoActor => "no human GamePlayActor",
            Refusal::Unreadable => "unreadable",
            Refusal::Identity => "GhostActor identity",
            Refusal::NotReady(_) => "GhostActor not ready",
            Refusal::Empty => "empty",
            Refusal::TooLong => "implausible length",
        }
    }
}

/// Capture the derived offset + RTTI vtable. A miss is NOT a mod refusal:
/// the Target tier simply falls back per song (one WARN here, one per song).
pub fn init(signatures: &SignatureStore) {
    let off = signatures.gpa_ghost_actor_off();
    let vt = signatures.get_address("ghost_actor_vtable");
    match (off, vt) {
        (Some(off), Some(vt)) => {
            GPA_GHOST_OFF.store(off, Ordering::Release);
            GHOST_VTABLE.store(vt as *mut u8, Ordering::Release);
            log_info!(
                "MultiplayerBot: ghost source ready (GamePlayActor+0x{:X} -> GhostActor, vtable {:p})",
                off,
                vt
            );
        }
        _ => log_warn!(
            "MultiplayerBot: ghost source unavailable (gpa_ghost_actor_off {} / ghost_actor_vtable {}) -- Target Score falls back to LV10 every song",
            if off.is_some() { "ok" } else { "MISSING" },
            if vt.is_some() { "ok" } else { "MISSING" }
        ),
    }
}

pub fn is_available() -> bool {
    GPA_GHOST_OFF.load(Ordering::Acquire) != 0 && !GHOST_VTABLE.load(Ordering::Acquire).is_null()
}

/// Read the HUMAN's ghost for a bot on `bot_side` (the other side's
/// GamePlayActor). Game thread only.
pub fn read_human_ghost(bot_side: usize) -> Result<Ghost, Refusal> {
    if !is_available() {
        return Err(Refusal::Unavailable);
    }
    let off = GPA_GHOST_OFF.load(Ordering::Acquire);
    let vt = GHOST_VTABLE.load(Ordering::Acquire) as *const u8;
    let dps = song_reset::live_dps().ok_or(Refusal::NoActor)?;
    let human_side = (1 - (bot_side & 1)) as i32;
    let gpa = song_reset::gameplay_actors(dps)
        .into_iter()
        .find(|&a| {
            memory::is_readable(a, GPA_SIDE + 4)
                && unsafe { memory::read_i32(a.add(GPA_SIDE)) } == human_side
        })
        .ok_or(Refusal::NoActor)?;
    unsafe {
        let field = gpa.add(off);
        if !memory::is_readable(field, 8) {
            return Err(Refusal::Unreadable);
        }
        let ghost = memory::read_ptr(field) as *const u8;
        if ghost.is_null() || !memory::is_readable(ghost, GA_PROBE_LEN) {
            return Err(Refusal::Unreadable);
        }
        if memory::read_ptr(ghost) != vt {
            return Err(Refusal::Identity);
        }
        let idx = (memory::read_u32(ghost.add(GA_STATE_IDX)) & 0xFFFF) as usize;
        if idx > 8 {
            return Err(Refusal::Identity);
        }
        let state = memory::read_i32(ghost.add(GA_STATE_BASE + idx * 8));
        if state != GA_STATE_READY {
            return Err(Refusal::NotReady(state));
        }
        let id = memory::read_u64(ghost.add(GA_GHOST_ID)) as i64;
        let begin = memory::read_ptr(ghost.add(GA_VEC)) as *const u8;
        let end = memory::read_ptr(ghost.add(GA_VEC + 8)) as *const u8;
        if begin.is_null() || end.is_null() || (end as usize) < (begin as usize) {
            return Err(Refusal::Empty);
        }
        let len = end as usize - begin as usize;
        if len == 0 {
            return Err(Refusal::Empty);
        }
        if len > MAX_GHOST_BYTES {
            return Err(Refusal::TooLong);
        }
        if !memory::is_readable(begin, len) {
            return Err(Refusal::Unreadable);
        }
        Ok(Ghost {
            id,
            bytes: std::slice::from_raw_parts(begin, len).to_vec(),
        })
    }
}
