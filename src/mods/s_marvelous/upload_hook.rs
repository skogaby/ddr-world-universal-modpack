//! The engine-facing half of the S-Marvelous score upload (server-upload
//! design §4.3 "producer"): resolves the stage record the save marshal just
//! serialised, reads its streams + stock counters, runs the pure
//! [`super::upload::build_payload`] and hands the persistence service a
//! [`NodeSpec`] for `/data/s_marv`. Registered as a `/data` subtree producer
//! at enable, unregistered at disable.
//!
//! Every gate that yields `None` is either silent (a state the design says
//! emits nothing) or a latched WARN (a state that should not happen). The
//! stock packet is never touched here — a `None` simply means no node.
//!
//! PANIC SAFETY: runs inside the persistence trampoline's post-original
//! section on the game thread. No unwrap/indexing; every game-memory read is
//! range-checked (`memory::is_readable`) before it is dereferenced.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::core::memory;
use crate::services::custom_options_persistence::{self as persistence, NodeLeaf, NodeSpec};
use crate::services::stage_records;
use crate::{log_info, log_warn};

use super::records::{self, RawStreams};
use super::upload::{self, Leaf, RecordInputs};
use super::{lamp, state};

/// `savekind` of the per-stage score save (`ReflectSavePlayerData(side, 2, stage)`).
const SAVEKIND_STAGE: i32 = 2;

/// Stage-record field offsets the marshal reads for the identity and the
/// stock counters this node duplicates (RE 20260825, `ReflectSavePlayerData`).
const REC_MCODE: usize = 0x00;
const REC_DIFFICULTY: usize = 0x04;
const REC_STYLE: usize = 0x08;
const REC_MARV: usize = 0x28;
const REC_FAST: usize = 0x6C;
const REC_SLOW: usize = 0x70;
/// The clear kind the marshal puts on the wire — `rec+0x54`, the SAME field the
/// results emblem predicate reads (staging `+0x1B70 ← piVar10[0x15]`; wire
/// order `… rank(+0x50), clearkind(+0x54) …`). NOT `rec+0x270`: that is
/// `folder` (staging `+0x1B5C`), which the first cabinet run shipped as
/// `clearkind = 7` (2026-09-12).
const REC_WIRE_CLEARKIND: usize = 0x54;
/// Bytes of the record we read scalars from — probed once up front.
const REC_PROBE_LEN: usize = REC_SLOW + 4;

/// Mod-enabled gate mirrored from `mod.rs::ACTIVE` (the producer must not
/// depend on `mod.rs` internals; the mod flips this beside `ACTIVE`).
static ENABLED: AtomicBool = AtomicBool::new(false);

/// One-shot latches for the refusals that should never happen in healthy play.
static WARNED_RECORD: AtomicBool = AtomicBool::new(false);
static WARNED_STREAMS: AtomicBool = AtomicBool::new(false);
static WARNED_BUILD: AtomicBool = AtomicBool::new(false);

fn warn_once(latch: &AtomicBool, msg: &str) {
    if !latch.swap(true, Ordering::AcqRel) {
        log_warn!("SMarvelous: upload — {} (node omitted; latched)", msg);
    }
}

/// Register the producer (mod enable).
pub fn activate() {
    ENABLED.store(true, Ordering::Release);
    persistence::register_data_node_producer(upload::NODE_NAME, produce);
}

/// Unregister the producer (mod disable).
pub fn deactivate() {
    ENABLED.store(false, Ordering::Release);
    persistence::unregister_data_node_producer(upload::NODE_NAME);
}

/// The registered producer. Gate ladder (design §4.3): enabled → side armed
/// for the whole song → per-stage save → not course mode → record resolvable
/// with `mcode != -1` → streams readable → payload consistent.
fn produce(side: u8, savekind: i32) -> Option<NodeSpec> {
    if !ENABLED.load(Ordering::Acquire) || side > 1 {
        return None;
    }
    let side = side as usize;
    if !state::armed_this_song(side) || savekind != SAVEKIND_STAGE {
        return None;
    }
    let window = state::last_armed_window(side);
    if window <= 0 {
        return None;
    }
    if course_mode() {
        return None; // v1: courses omitted (design D7); the course record is not this layout
    }
    let Some(stage) = stage_records::stage_counter() else {
        warn_once(&WARNED_RECORD, "stage counter unavailable");
        return None;
    };
    if !(0..stage_records::MAX_STAGE_RECORDS as i32).contains(&stage) {
        warn_once(&WARNED_RECORD, "stage counter out of range");
        return None;
    }
    let Some(record) = stage_records::stage_record(side, stage as usize) else {
        warn_once(&WARNED_RECORD, "stage record unresolvable");
        return None;
    };
    let record = record as *const u8;
    if !memory::is_readable(record, REC_PROBE_LEN) {
        warn_once(&WARNED_RECORD, "stage record unreadable");
        return None;
    }
    // SAFETY: probed readable for REC_PROBE_LEN bytes; game thread.
    let (mcode, difficulty, style, stock_marv, stock_fast, stock_slow, wire_clearkind) = unsafe {
        (
            memory::read_i32(record.add(REC_MCODE)),
            memory::read_i32(record.add(REC_DIFFICULTY)),
            memory::read_i32(record.add(REC_STYLE)),
            memory::read_i32(record.add(REC_MARV)),
            memory::read_i32(record.add(REC_FAST)),
            memory::read_i32(record.add(REC_SLOW)),
            memory::read_i32(record.add(REC_WIRE_CLEARKIND)),
        )
    };
    if mcode < 0 {
        return None; // virgin record (the marshal's own skip key) — nothing was played
    }
    // SAFETY: same live record; the stream readers bound every vector.
    let streams: RawStreams = match unsafe { records::read_raw_streams(record) } {
        Some(s) => s,
        None => {
            warn_once(&WARNED_STREAMS, "record streams unreadable/inconsistent");
            return None;
        }
    };
    let inputs = RecordInputs {
        mcode,
        style,
        difficulty,
        stock_marv,
        stock_fast,
        stock_slow,
        wire_clearkind,
        streams: &streams,
    };
    let Some(payload) = upload::build_payload(&inputs, window) else {
        warn_once(
            &WARNED_BUILD,
            "payload refused (Marvelous counter disagrees with judged stream)",
        );
        return None;
    };
    log_info!(
        "SMarvelous: upload side={} stage={} mcode={} chart={} window={} smarv={} marv={} fast={} slow={} clearkind={} ghostlen={}",
        side,
        stage,
        payload.mcode,
        payload.chart(),
        payload.window_ms,
        payload.judge_smarv,
        payload.judge_marv,
        payload.fastcount,
        payload.slowcount,
        payload.clearkind,
        payload.ghost.len()
    );
    if payload.is_smfc() {
        // Local feed (design D15): the violet lamp shows at the very next
        // song select, before the server ever echoes it back.
        lamp::insert_local(side, payload.mcode, payload.chart());
    }
    Some(NodeSpec {
        name: upload::NODE_NAME,
        leaves: upload::to_leaves(&payload)
            .into_iter()
            .map(|l| match l {
                Leaf::S32(n, v) => NodeLeaf::S32(n, v),
                Leaf::Str(n, v) => NodeLeaf::Str(n, v),
            })
            .collect(),
    })
}

/// Course/Dan session gate: `GameWork + course_field != 0` (the same field
/// `premium_free` and the results tab's populate branch consult).
fn course_mode() -> bool {
    if !stage_records::is_available() {
        return false;
    }
    let Some(game_work) = stage_records::game_work() else {
        return false;
    };
    let off = stage_records::course_field_offset();
    let p = unsafe { game_work.add(off) } as *const u8;
    if !memory::is_readable(p, 8) {
        return false;
    }
    unsafe { memory::read_u64(p) != 0 }
}
