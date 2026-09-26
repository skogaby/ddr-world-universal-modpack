//! Whose ghost a Target Score replay is replaying — the bot's name plate
//! (engine-facing; the pure sanitizer is `session::sanitize_target_name`).
//!
//! Resolved ONCE per song at the flip (`impersonation::apply`, game thread),
//! before any byte is written, by asking the game the same question its
//! `GhostActor` init will ask at GAMEPLAY setup — nothing is detoured:
//!
//! 1. `ghost_id_lookup(human)` — the game's TARGET-option verdict (the
//!    function `ghost_actor_init` calls). `0` ⇒ no pacemaker ghost: the
//!    replay will fall back to LV10, so the plate stays `TARGET`.
//! 2. `PlayerWork[human] + target`: −1 OFF, 0 own best, 1..3 rival slot,
//!    4..6 the world / area / machine ranking sets.
//!    * own best ⇒ the human's own name (`PlayerWork+0xC`);
//!    * rival n ⇒ the rival set (kind 3) whose code (`set+0x50`) is
//!      `PlayerWork[target + 4 + (n-1)*4]`;
//!    * ranking ⇒ the set of kind `n - 4`.
//! 3. For a set: `score_entry(set, chart)` must exist and carry the SAME
//!    ghost id (`entry+0x10`) the lookup returned — the proof that this set
//!    is the one the pacemaker loaded — then `dancer_name(set, chart)`: the
//!    rival's own name, or the chart's record holder for a ranking set.
//!
//! The chart key is the lookup's own: `PlayerWork` mcode, `GameWork+4`
//! style, `PlayerWork` difficulty (min 1 when the PlayerWork style is
//! double). Every `PlayerWork` offset is decoded from the lookup's body
//! (`derive_target_name_sites` — they differ on 20250805 / 20260224), the
//! set container / kind / rival-code fields are pinned by its AOB, and the
//! getters are the game's. Every pointer is `memory::is_readable`-probed
//! before a read, and for a set target the lookup (which walks the set
//! container unchecked) is only called once our identical search found the
//! set in the fully probed container. Any miss ⇒ `Err` (one INFO at the
//! caller; the plate stays `TARGET`).

use std::sync::OnceLock;

use super::session::{self, NAME_LEN};
use crate::core::memory;
use crate::core::signatures::{SignatureStore, TargetNameSites};
use crate::services::stage_records;
use crate::{log_info, log_warn};

/// `i64 ghost_id(int side)`.
type GhostIdFn = unsafe extern "C" fn(i32) -> i64;
/// `score_entry` / `dancer_name`: `(set, mcode, style, difficulty)`.
type SetQueryFn = unsafe extern "C" fn(*const u8, u32, i32, i32) -> *const u8;

// ── PlayerWork / GameWork (build-invariant header) ──────────────────────
const PW_NAME: usize = 0xC;
const GW_STYLE: usize = 0x4;
/// GameWork bytes the lookup reads (`+0x4` style … `+0xD0` battle mode).
const GW_PROBE_LEN: usize = 0xD4;

// ── Rival-set container / set (pinned by the `ghost_id_lookup` AOB) ────
const CONTAINER_BEGIN: usize = 0x0;
const CONTAINER_END: usize = 0x8;
const CONTAINER_PROBE_LEN: usize = 0x28;
const SET_KIND: usize = 0x0;
const SET_RIVAL_CODE: usize = 0x50;
const SET_PROBE_LEN: usize = 0x60;
const SET_KIND_RIVAL: i32 = 3;
/// Sanity cap on the container's set count (stock: 3 rankings + 3 rivals).
const MAX_SETS: usize = 64;
/// Score entry: ghost id at `+0x10`.
const ENTRY_GHOST_ID: usize = 0x10;
/// A holder / rival name slot is at most 12 bytes (8 chars + NUL used).
const NAME_SLOT: usize = 12;

struct Sites(TargetNameSites);
// Raw pointers into the game module — valid for the process lifetime.
unsafe impl Send for Sites {}
unsafe impl Sync for Sites {}

static SITES: OnceLock<Sites> = OnceLock::new();

/// Where the resolved name came from (for the flip INFO).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    OwnBest,
    Rival(u8),
    /// The ranking set's kind (0 world, 1 area, 2 machine).
    Ranking(u8),
}

#[derive(Debug, Clone, Copy)]
pub struct Resolved {
    pub name: [u8; NAME_LEN],
    pub source: Source,
    pub ghost_id: i64,
}

/// Capture the derived sites (mod init). A miss is fail-open: every Target
/// Score plate reads `TARGET`, one WARN here.
pub fn init(signatures: &SignatureStore) {
    match signatures.target_name_sites() {
        Some(s) => {
            log_info!(
                "MultiplayerBot: target-name lookup ready (PlayerWork target +0x{:X}, mcode +0x{:X}, diff +0x{:X})",
                s.pw_target_off,
                s.pw_mcode_off,
                s.pw_diff_off
            );
            let _ = SITES.set(Sites(s));
        }
        None => log_warn!(
            "MultiplayerBot: target-name lookup unavailable -- Target Score plates read TARGET"
        ),
    }
}

pub fn is_available() -> bool {
    SITES.get().is_some()
}

/// Resolve the name of the ghost the HUMAN's pacemaker will load this song.
/// Game thread only (the flip). `Err` carries the reason for the INFO.
pub fn resolve(human: usize) -> Result<Resolved, &'static str> {
    let s = &SITES.get().ok_or("lookup unavailable")?.0;
    let pw = stage_records::player_work(human).ok_or("human PlayerWork unavailable")?;
    let pw_len = (s.pw_target_off + 16).max(PW_NAME + NAME_LEN);
    if !memory::is_readable(pw, pw_len) {
        return Err("human PlayerWork unreadable");
    }
    let gw = stage_records::game_work().ok_or("GameWork unavailable")?;
    if !memory::is_readable(gw, GW_PROBE_LEN) {
        return Err("GameWork unreadable");
    }
    // The lookup walks the set container WITHOUT checks (and falls back to
    // a default set / reads past the end when the chosen set is absent), so
    // the chain is probed and our own search must find the set the game's
    // identical search will pick BEFORE the lookup is called at all.
    let sets = || rival_sets(s).ok_or("rival-set container unreadable");
    let target = unsafe { memory::read_i32(pw.add(s.pw_target_off)) };
    let (source, set) = match target {
        0 => (Source::OwnBest, None),
        1..=3 => {
            let code = unsafe {
                memory::read_i32(pw.add(s.pw_target_off + 4 + (target as usize - 1) * 4))
            };
            if code == 0 {
                return Err("rival slot empty");
            }
            let set = sets()?
                .into_iter()
                .find(|&p| unsafe {
                    memory::read_i32(p.add(SET_KIND)) == SET_KIND_RIVAL
                        && memory::read_i32(p.add(SET_RIVAL_CODE)) == code
                })
                .ok_or("rival set not loaded")?;
            (Source::Rival(target as u8), Some(set))
        }
        4..=6 => {
            let kind = target - 4;
            let set = sets()?
                .into_iter()
                .find(|&p| unsafe { memory::read_i32(p.add(SET_KIND)) == kind })
                .ok_or("ranking set not loaded")?;
            (Source::Ranking(kind as u8), Some(set))
        }
        _ => return Err("TARGET option off"),
    };

    let ghost_id = unsafe {
        let f: GhostIdFn = std::mem::transmute(s.ghost_id_lookup);
        f(human as i32)
    };
    if ghost_id == 0 {
        return Err("no target ghost (no score for this chart, or battle mode)");
    }

    let raw: [u8; NAME_SLOT] = match set {
        None => unsafe { read_slot(pw.add(PW_NAME), NAME_LEN) }.ok_or("own name unreadable")?,
        Some(set) => unsafe {
            let mcode = memory::read_u32(pw.add(s.pw_mcode_off));
            let style = memory::read_i32(gw.add(GW_STYLE));
            let diff = memory::read_i32(pw.add(s.pw_diff_off));
            let diff = if memory::read_i32(pw.add(s.pw_style_off)) == 1 {
                diff.max(1)
            } else {
                diff
            };
            let entry_fn: SetQueryFn = std::mem::transmute(s.score_entry);
            let entry = entry_fn(set, mcode, style, diff);
            if entry.is_null() || !memory::is_readable(entry, ENTRY_GHOST_ID + 8) {
                return Err("target set has no entry for this chart");
            }
            if memory::read_u64(entry.add(ENTRY_GHOST_ID)) as i64 != ghost_id {
                return Err("target set entry does not carry this ghost");
            }
            let name_fn: SetQueryFn = std::mem::transmute(s.dancer_name);
            let name = name_fn(set, mcode, style, diff);
            if name.is_null() {
                return Err("dancer name null");
            }
            read_slot(name, NAME_SLOT).ok_or("dancer name unreadable")?
        },
    };
    let name = session::sanitize_target_name(&raw).ok_or("target has no usable name")?;
    Ok(Resolved {
        name,
        source,
        ghost_id,
    })
}

/// The container's set pointers, every one probed; `None` when any hop of
/// `**rival_sets_global` or any element is unreadable, or the vector is
/// implausible.
fn rival_sets(s: &TargetNameSites) -> Option<Vec<*const u8>> {
    unsafe {
        if !memory::is_readable(s.rival_sets_global, 8) {
            return None;
        }
        let owner = memory::read_ptr(s.rival_sets_global);
        if owner.is_null() || !memory::is_readable(owner, 8) {
            return None;
        }
        let container = memory::read_ptr(owner);
        if container.is_null() || !memory::is_readable(container, CONTAINER_PROBE_LEN) {
            return None;
        }
        let begin = memory::read_ptr(container.add(CONTAINER_BEGIN)) as usize;
        let end = memory::read_ptr(container.add(CONTAINER_END)) as usize;
        if end < begin || (end - begin) % 8 != 0 || (end - begin) / 8 > MAX_SETS {
            return None;
        }
        let count = (end - begin) / 8;
        if count > 0 && !memory::is_readable(begin as *const u8, count * 8) {
            return None;
        }
        // Strict: the game's own search dereferences every element up to
        // its hit, so one bad element means the lookup must not run.
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let set = memory::read_ptr((begin + i * 8) as *const u8);
            if set.is_null() || !memory::is_readable(set, SET_PROBE_LEN) {
                return None;
            }
            out.push(set);
        }
        Some(out)
    }
}

/// Copy up to `len` bytes of a name slot (the rest zero), probed.
unsafe fn read_slot(p: *const u8, len: usize) -> Option<[u8; NAME_SLOT]> {
    let len = len.min(NAME_SLOT);
    if !memory::is_readable(p, len) {
        return None;
    }
    let mut out = [0u8; NAME_SLOT];
    std::ptr::copy_nonoverlapping(p, out.as_mut_ptr(), len);
    Some(out)
}
