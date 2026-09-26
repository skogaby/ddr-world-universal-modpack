//! DDR SELECTION themes — the stage panel's per-player score sets, engine
//! side: reads World's records into [`logic::SideInputs`] for the theme fill
//! (`panel.rs::fill_theme`, game thread, post-original of the ShutterActor
//! update that adopted A3's root — the update in which World's own kind-3
//! fill read the same data). The rules are `score_set_logic.rs`; the RE is
//! `docs/ddr_selection_theme_score_sets.md`.
//!
//! * **Chart**: the stage record header (`stage_records::stage_record`: mcode
//!   `+0`, difficulty `+4`, style `+8`), written by the song-select commit.
//! * **Best record**: World's own-best lookup `best_record(PlayerWork +
//!   score_db, mcode, style, difficulty)` — the case-0 call of the swept
//!   `ghost_id_lookup` (`derive_ddr_sel_score_set`; score db `0x178`, `0x188`
//!   on the old builds). Entry `+0` score, `+4` rank, `+8` clear kind.
//! * **Name**: World's rule on `PlayerWork+0xC` (`PLAYER1` / `PLAYER2` for an
//!   empty name). **Area**: `PlayerWork+0x20` when the profile byte `+5` is
//!   set (else 0, A3's `unknown`), through A3's region rule
//!   (`cabinet::licence_key_version`). The `PlayerWork` header offsets are
//!   identical on every supported build.
//! * **Target**: the TARGET option (`target_name_sites()`: `+0x1328`,
//!   `+0x1308` old) — own best, a rival (the kind-3 set with the slot's code)
//!   or a ranking kind. The set is found by the same first-match search
//!   World's resolver runs, over the fully probed container (World's walks
//!   it unchecked); `rival_set_score_entry` gives the record,
//!   `rival_set_dancer_name` the name. World has no area getter: a rival's
//!   area is `set+0x54` (between its code `+0x50` and name `+0x58`), a
//!   ranking holder's the dword before its name (`{code, area, name[12]}`).
//!   A set that cannot be found hides the target set.
//!
//! Every pointer is `memory::is_readable`-probed; a failed read hides only
//! its field ([`RecordRead::Unavailable`], `None`). A missing capability is
//! one WARN at init.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;

use crate::core::memory;
use crate::core::signatures::{DdrSelScoreSetSites, SignatureStore, TargetNameSites};
use crate::services::{cabinet, stage_records};
use crate::{log_info, log_warn};

use super::score_set_logic::TargetChoice;
use super::score_set_logic::{self as logic, Record, RecordRead, SetInputs, SideInputs};

/// `best_record` / `score_entry` / `dancer_name`: `(db-or-set, mcode, style,
/// difficulty)`.
type LookupFn = unsafe extern "C" fn(*const u8, u32, i32, i32) -> *const u8;

// ── PlayerWork header (identical on every supported build) ──────────────
const PW_SIDE: usize = 0x0;
const PW_ENTERED: usize = 0x4;
const PW_PROFILE: usize = 0x5;
const PW_NAME: usize = 0xC;
/// `char[9]` (8 characters + NUL).
const PW_NAME_LEN: usize = 9;
const PW_AREA: usize = 0x20;
const PW_HEADER_LEN: usize = 0x24;

// ── Stage record header ─────────────────────────────────────────────────
const REC_MCODE: usize = 0x0;
const REC_DIFF: usize = 0x4;
const REC_STYLE: usize = 0x8;

// ── Score entry (0x30 bytes; three fields read) ─────────────────────────
const ENTRY_SCORE: usize = 0x0;
const ENTRY_RANK: usize = 0x4;
const ENTRY_CLEAR: usize = 0x8;

// ── Trees the lookups walk: header node `_Isnil` flags ─────────────────
/// Score db / score map: header at `+8` of the db, `_Isnil` `+0x201`.
const DB_HEADER: usize = 0x8;
const SET_SCORES_HEADER: usize = 0x18;
const SCORE_NODE_LEN: usize = 0x202;
/// Ranking holder map: header at `set+0x38`, `_Isnil` `+0xE5`.
const SET_HOLDERS_HEADER: usize = 0x38;
const HOLDER_NODE_LEN: usize = 0xE6;

// ── Rival-set container / set (pinned by the `ghost_id_lookup` AOB) ────
const CONTAINER_BEGIN: usize = 0x0;
const CONTAINER_END: usize = 0x8;
const CONTAINER_PROBE_LEN: usize = 0x28;
const SET_KIND: usize = 0x0;
const SET_RIVAL_CODE: usize = 0x50;
const SET_RIVAL_AREA: usize = 0x54;
const SET_PROBE_LEN: usize = 0x60;
const SET_KIND_RIVAL: i32 = 3;
/// Sanity cap on the container's set count (stock: 3 rankings + 3 rivals).
const MAX_SETS: usize = 64;
/// A holder / rival name slot (8 characters + NUL used).
const NAME_SLOT: usize = 12;

struct Sites {
    best: Option<DdrSelScoreSetSites>,
    target: Option<TargetNameSites>,
}
// Raw pointers into the game module — valid for the process lifetime.
unsafe impl Send for Sites {}
unsafe impl Sync for Sites {}

static SITES: OnceLock<Sites> = OnceLock::new();
static WARNED: AtomicU32 = AtomicU32::new(0);
const W_PLAYER: u32 = 1;
const W_CHART: u32 = 2;
const W_RECORD: u32 = 4;
const W_SETS: u32 = 8;

fn warn_once(bit: u32) -> bool {
    WARNED.fetch_or(bit, Ordering::Relaxed) & bit == 0
}

/// Capture the sites (mod init). Each miss hides its fields, one WARN here.
pub fn init(signatures: &SignatureStore) {
    let best = signatures.ddr_sel_score_set_sites();
    let target = signatures.target_name_sites();
    match (&best, &target) {
        (Some(b), Some(t)) => log_info!(
            "DDR SELECTION: theme score sets ready (score db +0x{:X}, TARGET +0x{:X})",
            b.pw_score_db_off,
            t.pw_target_off
        ),
        _ => log_warn!(
            "DDR SELECTION: theme score sets partial ({}{}) -- those fields stay hidden",
            if best.is_none() {
                "best-record lookup unresolved; "
            } else {
                ""
            },
            if target.is_none() {
                "target sets unresolved"
            } else {
                ""
            }
        ),
    }
    let _ = SITES.set(Sites { best, target });
}

/// The theme's area package in the operator's language
/// (`common_area_lang_eng_v2`, …); the caller checks the arc exists.
pub fn area_package(skin: u8) -> Option<String> {
    let theme = super::policy::theme(skin)?;
    let lang = cabinet::game_language().unwrap_or(0);
    Some(logic::area_package(logic::lang_suffix(lang), theme.suffix))
}

/// Package readiness and session state for one fill.
#[derive(Clone, Copy, Debug)]
pub struct FillCtx {
    pub stage: i32,
    pub event_mode: i32,
    /// `common_texture_v0` is registered.
    pub glyphs_ready: bool,
    /// The area package is registered.
    pub area_ready: bool,
    /// `cabinet::licence_key_version()` (0 when unreadable).
    pub region: i32,
}

impl FillCtx {
    pub fn new(stage: i32, glyphs_ready: bool, area_ready: bool) -> Self {
        FillCtx {
            stage,
            event_mode: stage_records::event_mode().unwrap_or(0),
            glyphs_ready,
            area_ready,
            region: cabinet::licence_key_version().unwrap_or(0),
        }
    }
}

/// One side's inputs (game thread).
pub fn side_inputs(side: usize, ctx: &FillCtx) -> SideInputs {
    let hidden = SideInputs {
        side,
        visible: false,
        glyphs_ready: ctx.glyphs_ready,
        high: SetInputs {
            difficulty: None,
            name: None,
            area: None,
            record: RecordRead::Unavailable,
        },
        target: None,
    };
    let Some(pw) = stage_records::player_work(side) else {
        return hidden;
    };
    if !memory::is_readable(pw, PW_HEADER_LEN) {
        if warn_once(W_PLAYER) {
            log_warn!("DDR SELECTION: PlayerWork[{side}] unreadable -- its score set stays hidden");
        }
        return hidden;
    }
    let entered = unsafe { memory::read_u8(pw.add(PW_ENTERED)) } != 0;
    if !logic::set_visible(entered, false, ctx.stage) {
        return hidden;
    }
    let chart = chart(side, ctx.stage);
    let (name, area) = unsafe { own_name_area(pw, ctx) };
    let record = match chart {
        Some(c) => own_record(pw, c),
        None => RecordRead::Unavailable,
    };
    let high = SetInputs {
        difficulty: chart.map(|c| c.diff),
        name: Some(name.clone()),
        area: area.clone(),
        record,
    };
    let target = chart.and_then(|c| target_set(pw, c, ctx, &name, &area, record));
    SideInputs {
        side,
        visible: true,
        glyphs_ready: ctx.glyphs_ready,
        high,
        target,
    }
}

/// One line for the fill INFO.
pub fn describe(inp: &SideInputs) -> String {
    if !inp.visible {
        return format!("P{} hidden", inp.side + 1);
    }
    let rec = |r: &RecordRead| match r {
        RecordRead::Unavailable => "record n/a".to_string(),
        RecordRead::Absent => "no record".to_string(),
        RecordRead::Found(r) => format!("{} rank {} clear {}", r.score, r.rank, r.clear_kind),
    };
    format!(
        "P{} diff {:?} {}{}, target {}",
        inp.side + 1,
        inp.high.difficulty,
        rec(&inp.high.record),
        if inp.high.area.is_some() {
            ""
        } else {
            ", no area"
        },
        match &inp.target {
            None => "hidden".to_string(),
            Some(t) => format!(
                "{} ({})",
                rec(&t.record),
                t.name
                    .as_deref()
                    .map(|n| n.len().to_string() + " chars")
                    .unwrap_or_else(|| "no name".into())
            ),
        }
    )
}

#[derive(Clone, Copy, Debug)]
struct Chart {
    mcode: u32,
    diff: i32,
    style: i32,
}

fn chart(side: usize, stage: i32) -> Option<Chart> {
    let rec = usize::try_from(stage)
        .ok()
        .and_then(|st| stage_records::stage_record(side, st));
    let Some(rec) = rec.filter(|&r| memory::is_readable(r, REC_STYLE + 4)) else {
        if warn_once(W_CHART) {
            log_warn!("DDR SELECTION: stage record (stage {stage}) unreadable -- score-set records / difficulty hidden");
        }
        return None;
    };
    unsafe {
        Some(Chart {
            mcode: memory::read_u32(rec.add(REC_MCODE)),
            diff: memory::read_i32(rec.add(REC_DIFF)),
            style: memory::read_i32(rec.add(REC_STYLE)),
        })
    }
}

/// World's name rule and A3's area rule for the side itself.
unsafe fn own_name_area(pw: *const u8, ctx: &FillCtx) -> (Vec<u8>, Option<String>) {
    let mut raw = [0u8; PW_NAME_LEN];
    std::ptr::copy_nonoverlapping(pw.add(PW_NAME), raw.as_mut_ptr(), PW_NAME_LEN);
    let name = logic::player_name(&raw, memory::read_u32(pw.add(PW_SIDE)));
    let area = ctx.area_ready.then(|| {
        let code = if memory::read_u8(pw.add(PW_PROFILE)) != 0 {
            memory::read_i32(pw.add(PW_AREA))
        } else {
            0
        };
        logic::area_texture(ctx.region, code)
    });
    (name, area)
}

fn own_record(pw: *const u8, c: Chart) -> RecordRead {
    let Some(s) = SITES.get().and_then(|s| s.best) else {
        return RecordRead::Unavailable;
    };
    let db = unsafe { pw.add(s.pw_score_db_off) };
    if !tree_ok(db, DB_HEADER, SCORE_NODE_LEN) {
        if warn_once(W_RECORD) {
            log_warn!("DDR SELECTION: the score db is unreadable -- the best record stays hidden");
        }
        return RecordRead::Unavailable;
    }
    unsafe {
        let f: LookupFn = std::mem::transmute(s.best_record);
        entry(f(db, c.mcode, c.style, c.diff))
    }
}

/// The map a lookup walks: `owner + header_off` holds a readable header node.
fn tree_ok(owner: *const u8, header_off: usize, node_len: usize) -> bool {
    unsafe {
        if !memory::is_readable(owner, header_off + 8) {
            return false;
        }
        let header = memory::read_ptr(owner.add(header_off));
        !header.is_null() && memory::is_readable(header, node_len)
    }
}

/// A lookup's result: null = no record.
unsafe fn entry(p: *const u8) -> RecordRead {
    if p.is_null() {
        return RecordRead::Absent;
    }
    if !memory::is_readable(p, ENTRY_CLEAR + 4) {
        return RecordRead::Unavailable;
    }
    RecordRead::Found(Record {
        score: memory::read_u32(p.add(ENTRY_SCORE)),
        rank: memory::read_u32(p.add(ENTRY_RANK)),
        clear_kind: memory::read_u32(p.add(ENTRY_CLEAR)),
    })
}

/// The target set, or `None` when hidden / unresolved.
fn target_set(
    pw: *const u8,
    c: Chart,
    ctx: &FillCtx,
    own_name: &[u8],
    own_area: &Option<String>,
    own_record: RecordRead,
) -> Option<SetInputs> {
    let t = SITES.get()?.target?;
    if !memory::is_readable(unsafe { pw.add(t.pw_target_off) }, 16) {
        return None;
    }
    let option = unsafe { memory::read_i32(pw.add(t.pw_target_off)) };
    let set = match logic::target_choice(option, ctx.event_mode) {
        TargetChoice::Hidden => return None,
        TargetChoice::Own => {
            return Some(SetInputs {
                difficulty: None,
                name: Some(own_name.to_vec()),
                area: own_area.clone(),
                record: own_record,
            })
        }
        TargetChoice::Rival(slot) => {
            let code = unsafe { memory::read_i32(pw.add(t.pw_target_off + 4 + slot * 4)) };
            if code == 0 {
                return None;
            }
            find_set(&t, |p| unsafe {
                memory::read_i32(p.add(SET_KIND)) == SET_KIND_RIVAL
                    && memory::read_i32(p.add(SET_RIVAL_CODE)) == code
            })?
        }
        TargetChoice::Ranking(kind) => {
            find_set(&t, |p| unsafe { memory::read_i32(p.add(SET_KIND)) == kind })?
        }
    };
    let rival = unsafe { memory::read_i32(set.add(SET_KIND)) } == SET_KIND_RIVAL;
    let record = if tree_ok(set, SET_SCORES_HEADER, SCORE_NODE_LEN) {
        unsafe {
            let f: LookupFn = std::mem::transmute(t.score_entry);
            entry(f(set, c.mcode, c.style, c.diff))
        }
    } else {
        RecordRead::Unavailable
    };
    let (name, area_code) = unsafe { set_name_area(&t, set, rival, c) };
    Some(SetInputs {
        difficulty: None,
        name,
        area: area_code
            .filter(|_| ctx.area_ready)
            .map(|a| logic::area_texture(ctx.region, a)),
        record,
    })
}

/// A set's dancer name and area (see the module doc).
unsafe fn set_name_area(
    t: &TargetNameSites,
    set: *const u8,
    rival: bool,
    c: Chart,
) -> (Option<Vec<u8>>, Option<i32>) {
    if !rival && !tree_ok(set, SET_HOLDERS_HEADER, HOLDER_NODE_LEN) {
        return (None, None);
    }
    let f: LookupFn = std::mem::transmute(t.dancer_name);
    let p = f(set, c.mcode, c.style, c.diff);
    if p.is_null() || !memory::is_readable(p, 1) {
        return (None, None);
    }
    let len = if memory::is_readable(p, NAME_SLOT) {
        NAME_SLOT
    } else {
        1
    };
    let mut raw = vec![0u8; len];
    std::ptr::copy_nonoverlapping(p, raw.as_mut_ptr(), len);
    let n = raw.iter().position(|&b| b == 0).unwrap_or(len);
    raw.truncate(n);
    let area = if rival {
        Some(memory::read_i32(set.add(SET_RIVAL_AREA)))
    } else if n > 0 && memory::is_readable(p.wrapping_sub(4), 4) {
        Some(memory::read_i32(p.wrapping_sub(4)))
    } else {
        Some(0)
    };
    (Some(raw), area)
}

/// World's first-match search over the probed set container (its own walk
/// is unchecked and falls back to a default set / reads past the end).
fn find_set(t: &TargetNameSites, pred: impl Fn(*const u8) -> bool) -> Option<*const u8> {
    let sets = rival_sets(t);
    if sets.is_none() && warn_once(W_SETS) {
        log_warn!("DDR SELECTION: the rival-set container is unreadable -- target sets hidden");
    }
    sets?.into_iter().find(|&p| pred(p))
}

/// The container's set pointers, every one probed (`None` when any hop is
/// unreadable or the vector is implausible).
fn rival_sets(t: &TargetNameSites) -> Option<Vec<*const u8>> {
    unsafe {
        if !memory::is_readable(t.rival_sets_global, 8) {
            return None;
        }
        let owner = memory::read_ptr(t.rival_sets_global);
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
