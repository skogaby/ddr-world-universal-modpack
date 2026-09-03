//! Stage Records — shared, fail-closed decode of the per-stage play-record
//! layout (hoisted from `premium_free`).
//!
//! Decodes everything once from the matched `stage_record_accessor` signature
//! bytes (`getStageRecord(side, stage)` — a tiny leaf accessor whose matched
//! bytes contain every constant as RIP disp32s / disp8s / imm32s; see the
//! signature comment in `core/signatures.rs` for the byte map). Nothing is
//! hardcoded:
//!
//! | Constant                              | Source in matched bytes | 20260324+   |
//! |---------------------------------------|-------------------------|-------------|
//! | GameWork ptr-ptr global               | +3 RIP disp32           | —           |
//! | player_work_table                     | +16 RIP disp32          | —           |
//! | course-mode field offset (GameWork)   | +23 disp8               | 0x70        |
//! | course record offset (PlayerWork)     | +36 imm32               | 0x2D8       |
//! | record stride                         | +47 imm32               | 0x2B8       |
//! | record base offset (PlayerWork)       | +55 imm32               | 0x590       |
//!
//! Older builds (20250805, 20260224) compile the same accessor differently
//! (`stage_record_accessor_v1`): the course record is `ADD imm32` at +36
//! (0x2B8) and the stage record is `(stage + skew) * stride` — skew imm8 at
//! +51 (2), stride imm32 at +55 (0x2B8) — so base = skew*stride = 0x570.
//! PlayerWork grew 0x20 between those builds and 20260324; every consumer
//! reads the decoded values, never the literals.
//!
//! Consumers: `premium_free` (stale-record virginise — save-integrity
//! load-bearing), the logout-save sanitiser in `custom_options_persistence`
//! (record wipes for tainted sides), and `quick_logout` (side-entered session
//! gate). Validation fails closed: any out-of-range constant, out-of-module
//! global, or disagreement with the independently derived `player_work_table`
//! leaves `is_available() == false` and every accessor returning `None`.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

use crate::core::memory;
use crate::core::module_resolver::GameModule;
use crate::core::scanner::decode_rip_relative;
use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};

/// The per-stage record array has exactly 5 slots (game ctor:
/// `_eh_vector_constructor_iterator_(work+base, stride, 5, ...)`).
pub const MAX_STAGE_RECORDS: usize = 5;

static AVAILABLE: AtomicBool = AtomicBool::new(false);
/// Global holding the pointer to the pointer to GameWork (double indirection,
/// mirrors the accessor's `MOV RAX,[global]; MOV R8,[RAX]`).
static GAME_WORK_PTR_GLOBAL: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
/// Per-side player-work table (`table[side] -> wrapper -> PlayerWork`).
static PLAYER_WORK_TABLE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
/// Offset of the course-mode field inside GameWork.
static COURSE_FIELD_OFFSET: AtomicUsize = AtomicUsize::new(0);
/// Offset of the 0-based stage counter inside GameWork (disp8 of the
/// `INC dword [RCX+0xC]` at `premium_free_stage_inc + 3` — the same decode
/// `premium_free` performs for its patch site, hoisted here read-only so the
/// song-rate eligibility/save paths can share it; 0 = undecoded). Decoded at
/// init, before `premium_free` can NOP the site.
static STAGE_COUNTER_OFFSET: AtomicUsize = AtomicUsize::new(0);
/// Offset of the course-mode play record inside PlayerWork (style == 10
/// sessions marshal this single record instead of the array).
static COURSE_RECORD_OFFSET: AtomicUsize = AtomicUsize::new(0);
/// Per-stage record array base offset inside PlayerWork.
static REC_BASE: AtomicUsize = AtomicUsize::new(0);
/// Per-stage record stride.
static REC_STRIDE: AtomicUsize = AtomicUsize::new(0);

// ── Optional session-state decode (`final_stage_probe` + `max_stage_global`)
// Non-fatal secondary decode, like the stage-counter one: a miss only leaves
// `session_state_available() == false` (the quick-fail fast path then always
// falls back to the natural flow). See
// `docs/quick_restart_fail_speedup_research.md` §6/§7.
static SESSION_STATE_AVAILABLE: AtomicBool = AtomicBool::new(false);
/// Offset of the event-mode field inside GameWork (0xD0 on all verified
/// builds; values 1/2 = the event/special scene chain).
static EVENT_MODE_OFFSET: AtomicUsize = AtomicUsize::new(0);
/// Offset of the final-stage override inside GameWork (0x10; stock code only
/// ever resets it to -1 — see docs/quick_logout_research.md §5.2).
static FINAL_STAGE_OVERRIDE_OFFSET: AtomicUsize = AtomicUsize::new(0);
/// The operator's `/gameOptions/max_stage/current` cache global. Normal
/// stage count = value + 1; the last normal 0-based stage index = value.
static MAX_STAGE_GLOBAL: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
/// Offset of the inlined `ddr::player::Option` inside `PlayerWork` (0xE0 on
/// 20260324+, 0xF0 on 20250805 / 20260224) — derived by
/// `SignatureStore::derive_player_option_table` from the game's own accessor.
/// 0 = unknown.
static PLAYER_OPTION_OFFSET: AtomicUsize = AtomicUsize::new(0);

/// Offset of `ddr::player::Option` within `PlayerWork`, or `None` when the
/// accessor derivation failed. Consumers must go inert on `None` — the
/// offset moved between builds, so no default is safe.
pub fn player_option_offset() -> Option<usize> {
    match PLAYER_OPTION_OFFSET.load(Ordering::Acquire) {
        0 => None,
        v => Some(v),
    }
}

/// Decode + validate the layout. Fail closed: any validation failure leaves
/// the service unavailable (and its consumers fail closed in turn).
pub fn init(signatures: &SignatureStore, module: &GameModule) -> bool {
    // Published independently of the record-layout decode below: the Option
    // offset comes from the `player_option_ctx_load` accessor derivation and
    // several consumers (assist_tick, per-song offsets, song_reset, real
    // speed, mine_render) need it even when the record accessor is missing.
    if let Some(off) = signatures.player_option_offset() {
        PLAYER_OPTION_OFFSET.store(off, Ordering::Release);
    }
    // Two codegen shapes of the same leaf accessor: `stage_record_accessor`
    // (20260324+: JZ over the course ADD, then IMUL + LEA base) and
    // `stage_record_accessor_v1` (20250805 / 20260224: course branch first,
    // then `(stage + skew) * stride` with base = skew*stride). Byte maps in the
    // signature comments.
    let (accessor, v1) = match signatures.get_address("stage_record_accessor") {
        Some(a) => (a, false),
        None => match signatures.get_address("stage_record_accessor_v1") {
            Some(a) => {
                log_info!("stage_records: using pre-20260324 accessor shape (v1)");
                (a, true)
            }
            None => {
                log_warn!("stage_records: stage_record_accessor signature not resolved");
                return false;
            }
        },
    };
    let pwt = match signatures.get_address("player_work_table") {
        Some(a) => a,
        None => {
            log_warn!("stage_records: player_work_table not derived");
            return false;
        }
    };

    // Decode from the matched bytes (see the byte map in the module docs /
    // the signature comment in signatures.rs).
    let (game_work_global, table, course_off, course_rec_off, rec_stride, rec_base) = unsafe {
        if v1 {
            let stride = memory::read_u32(accessor.add(55)) as usize;
            let skew = memory::read_u8(accessor.add(51)) as usize;
            (
                decode_rip_relative(accessor.add(3)),
                decode_rip_relative(accessor.add(16)),
                memory::read_u8(accessor.add(23)) as usize,
                memory::read_u32(accessor.add(36)) as usize,
                stride,
                skew.wrapping_mul(stride),
            )
        } else {
            (
                decode_rip_relative(accessor.add(3)),
                decode_rip_relative(accessor.add(16)),
                memory::read_u8(accessor.add(23)) as usize,
                memory::read_u32(accessor.add(36)) as usize,
                memory::read_u32(accessor.add(47)) as usize,
                memory::read_u32(accessor.add(55)) as usize,
            )
        }
    };

    // Sanity: the wildcarded layout constants must look like the known shape
    // (course field 0x70, course record 0x2D8, stride 0x2B8, base 0x590 on
    // 2026 builds). A wild value means the accessor changed — fail closed
    // rather than risk poisoning score submissions.
    if !(0x8..=0x7F).contains(&course_off)
        || !(0x100..=0xFFF).contains(&rec_stride)
        || !(0x100..=0x1FFF).contains(&rec_base)
        || !(0x100..=0x1FFF).contains(&course_rec_off)
    {
        log_warn!(
            "stage_records: stage_record_accessor layout out of range (course field +0x{:X}, course rec +0x{:X}, stride 0x{:X}, base 0x{:X}) -- unavailable",
            course_off,
            course_rec_off,
            rec_stride,
            rec_base
        );
        return false;
    }

    // The accessor's table LEA must agree with the independently derived
    // player_work_table (both dereference table[side] -> wrapper -> work).
    if pwt != table {
        log_warn!(
            "stage_records: accessor player-work table {:p} disagrees with derived player_work_table {:p} -- unavailable",
            table,
            pwt
        );
        return false;
    }

    // Both decoded globals must live inside the game module.
    let module_start = module.base as usize;
    let module_end = module_start + module.size;
    for (name, p) in [
        ("game-work global", game_work_global),
        ("player-work table", table),
    ] {
        let a = p as usize;
        if a < module_start || a >= module_end {
            log_warn!(
                "stage_records: derived {} {:p} outside module -- unavailable",
                name,
                p
            );
            return false;
        }
    }

    GAME_WORK_PTR_GLOBAL.store(game_work_global as *mut u8, Ordering::Release);
    PLAYER_WORK_TABLE.store(table as *mut u8, Ordering::Release);
    COURSE_FIELD_OFFSET.store(course_off, Ordering::Release);
    COURSE_RECORD_OFFSET.store(course_rec_off, Ordering::Release);
    REC_STRIDE.store(rec_stride, Ordering::Release);
    REC_BASE.store(rec_base, Ordering::Release);
    AVAILABLE.store(true, Ordering::Release);

    // Optional stage-counter decode (non-fatal): validate the exact
    // `INC dword [RCX+disp8]` bytes at `premium_free_stage_inc + 3` and take
    // the disp8, exactly as premium_free does before patching. This runs at
    // boot, before any mod can NOP the site, so the literal-byte check is
    // reliable. A miss only leaves `stage_counter()` unavailable (its
    // consumers fail closed); the record layout above is unaffected.
    if let Some(inc_anchor) = signatures.get_address("premium_free_stage_inc") {
        let inc = unsafe { inc_anchor.add(3) };
        let bytes = unsafe { [memory::read_u8(inc), memory::read_u8(inc.add(1))] };
        if bytes == [0xFF, 0x41] {
            let offset = unsafe { memory::read_u8(inc.add(2)) } as usize;
            // Sanity: a small dword-aligned GameWork header slot (0xC on all
            // 2026 builds). Anything else means the instruction changed.
            if (0x4..=0x7C).contains(&offset) && offset % 4 == 0 {
                STAGE_COUNTER_OFFSET.store(offset, Ordering::Release);
                log_info!(
                    "stage_records: stage counter decoded at GameWork+0x{:X}",
                    offset
                );
            } else {
                log_warn!(
                    "stage_records: stage counter disp8 0x{:X} out of range -- stage counter unavailable",
                    offset
                );
            }
        } else {
            log_warn!(
                "stage_records: premium_free_stage_inc bytes unexpected -- stage counter unavailable"
            );
        }
    } else {
        log_warn!(
            "stage_records: premium_free_stage_inc signature unresolved -- stage counter unavailable"
        );
    }

    log_info!(
        "stage_records: layout decoded (records work+0x{:X}, stride 0x{:X}, course rec +0x{:X}, course field +0x{:X})",
        rec_base,
        rec_stride,
        course_rec_off,
        course_off
    );

    // Optional session-state decode (non-fatal): the `final_stage_probe`
    // match yields the event-mode and final-stage-override offsets, plus
    // cross-checkable copies of constants decoded above. Every check must
    // pass or the session-state accessors stay unavailable (their consumer —
    // the quick-fail fast path — then falls back to the natural flow).
    decode_session_state(signatures, module, game_work_global, course_off);

    true
}

/// Decode + validate the GameWork session-state constants from the
/// `final_stage_probe` match (byte map in the signature comment /
/// `docs/quick_restart_fail_speedup_research.md` §6.1) and the derived
/// `max_stage_global`. Fail closed on any cross-check miss.
fn decode_session_state(
    signatures: &SignatureStore,
    module: &GameModule,
    game_work_global: *const u8,
    accessor_course_off: usize,
) {
    let probe = match signatures.get_address("final_stage_probe") {
        Some(a) => a,
        None => {
            log_warn!("stage_records: final_stage_probe unresolved -- session state unavailable");
            return;
        }
    };
    let max_stage_global = match signatures.get_address("max_stage_global") {
        Some(a) => a,
        None => {
            log_warn!("stage_records: max_stage_global not derived -- session state unavailable");
            return;
        }
    };

    let (gw_opcode_ok, probe_game_work, course_off, stage_off, event_off, override_off) = unsafe {
        (
            // The GameWork load sits 7 bytes before the match: MOV RAX,[rip+d32].
            memory::read_u8(probe.sub(7)) == 0x48
                && memory::read_u8(probe.sub(6)) == 0x8B
                && memory::read_u8(probe.sub(5)) == 0x05,
            decode_rip_relative(probe.sub(4)),
            memory::read_u8(probe.add(6)) as usize,
            memory::read_u8(probe.add(10)) as usize,
            memory::read_u32(probe.add(15)) as usize,
            memory::read_u8(probe.add(31)) as usize,
        )
    };

    // Cross-check 1: the probe must read the same GameWork global as the
    // stage_record_accessor (strongest proof the AOB matched the right leaf).
    if !gw_opcode_ok || probe_game_work != game_work_global {
        log_warn!(
            "stage_records: final_stage_probe GameWork global disagrees with accessor ({:p} vs {:p}) -- session state unavailable",
            probe_game_work,
            game_work_global
        );
        return;
    }
    // Cross-check 2: course offset must agree with the accessor's decode.
    if course_off != accessor_course_off {
        log_warn!(
            "stage_records: final_stage_probe course offset +0x{:X} disagrees with accessor +0x{:X} -- session state unavailable",
            course_off,
            accessor_course_off
        );
        return;
    }
    // Cross-check 3: stage offset must agree with the premium_free_stage_inc
    // decode when that decode succeeded (it runs just above).
    let inc_stage_off = STAGE_COUNTER_OFFSET.load(Ordering::Acquire);
    if inc_stage_off != 0 && stage_off != inc_stage_off {
        log_warn!(
            "stage_records: final_stage_probe stage offset +0x{:X} disagrees with stage-inc +0x{:X} -- session state unavailable",
            stage_off,
            inc_stage_off
        );
        return;
    }
    // Range sanity for the two new offsets (0xD0 / 0x10 on all verified
    // builds — small GameWork header slots).
    if !(0x8..=0xFFF).contains(&event_off) || !(0x4..=0x7C).contains(&override_off) {
        log_warn!(
            "stage_records: final_stage_probe offsets out of range (event +0x{:X}, override +0x{:X}) -- session state unavailable",
            event_off,
            override_off
        );
        return;
    }
    // The derived global must live inside the game module.
    let a = max_stage_global as usize;
    if a < module.base as usize || a >= module.base as usize + module.size {
        log_warn!(
            "stage_records: max_stage_global {:p} outside module -- session state unavailable",
            max_stage_global
        );
        return;
    }

    EVENT_MODE_OFFSET.store(event_off, Ordering::Release);
    FINAL_STAGE_OVERRIDE_OFFSET.store(override_off, Ordering::Release);
    MAX_STAGE_GLOBAL.store(max_stage_global as *mut u8, Ordering::Release);
    SESSION_STATE_AVAILABLE.store(true, Ordering::Release);
    log_info!(
        "stage_records: session state decoded (event +0x{:X}, override +0x{:X}, max-stage global {:p})",
        event_off,
        override_off,
        max_stage_global
    );
}

pub fn is_available() -> bool {
    AVAILABLE.load(Ordering::Acquire)
}

/// Resolve the live GameWork pointer (double indirection through the decoded
/// global), null-checked at both hops.
pub fn game_work() -> Option<*mut u8> {
    if !is_available() {
        return None;
    }
    let global = GAME_WORK_PTR_GLOBAL.load(Ordering::Acquire);
    if global.is_null() {
        return None;
    }
    unsafe {
        let ptr1 = memory::read_ptr(global);
        if ptr1.is_null() {
            return None;
        }
        let game_work = memory::read_ptr(ptr1);
        if game_work.is_null() {
            None
        } else {
            Some(game_work as *mut u8)
        }
    }
}

/// Resolve a side's live PlayerWork pointer
/// (`table[side] -> wrapper -> PlayerWork`), null-checked at both hops.
pub fn player_work(side: usize) -> Option<*mut u8> {
    if !is_available() || side >= 2 {
        return None;
    }
    let table = PLAYER_WORK_TABLE.load(Ordering::Acquire);
    if table.is_null() {
        return None;
    }
    unsafe {
        let wrapper = memory::read_ptr(table.add(side * 8));
        if wrapper.is_null() {
            return None;
        }
        let work = memory::read_ptr(wrapper);
        if work.is_null() {
            None
        } else {
            Some(work as *mut u8)
        }
    }
}

/// Whether `side` has entered the session — the `PlayerWork+0x4` entered
/// byte (nonzero once the side joined; the quick_logout session gate's
/// field). `None` when the player-work chain is unavailable — callers
/// choose their own conservative default.
pub fn side_entered(side: usize) -> Option<bool> {
    const PLAYER_WORK_ENTERED_OFFSET: usize = 0x4;
    let work = player_work(side)?;
    Some(unsafe { memory::read_u8(work.add(PLAYER_WORK_ENTERED_OFFSET)) } != 0)
}

/// Pointer to a side's per-stage play record (`stage` 0..5). The record's
/// first i32 is `mcode` (-1 = virgin — the save marshal's skip key).
pub fn stage_record(side: usize, stage: usize) -> Option<*mut u8> {
    if stage >= MAX_STAGE_RECORDS {
        return None;
    }
    let work = player_work(side)?;
    let rec_base = REC_BASE.load(Ordering::Acquire);
    let rec_stride = REC_STRIDE.load(Ordering::Acquire);
    if rec_base == 0 || rec_stride == 0 {
        return None;
    }
    Some(unsafe { work.add(rec_base + stage * rec_stride) })
}

/// Pointer to a side's course-mode play record (marshalled instead of the
/// array when the session style is course).
pub fn course_record(side: usize) -> Option<*mut u8> {
    let work = player_work(side)?;
    let off = COURSE_RECORD_OFFSET.load(Ordering::Acquire);
    if off == 0 {
        return None;
    }
    Some(unsafe { work.add(off) })
}

/// Offset of the course-mode field inside GameWork (for premium_free's
/// course-mode skip). Meaningful only while `is_available()`.
pub fn course_field_offset() -> usize {
    COURSE_FIELD_OFFSET.load(Ordering::Acquire)
}

/// Decoded per-stage record array base offset (for logging/diagnostics).
pub fn record_base() -> usize {
    REC_BASE.load(Ordering::Acquire)
}

/// Decoded per-stage record stride (for logging/diagnostics).
pub fn record_stride() -> usize {
    REC_STRIDE.load(Ordering::Acquire)
}

/// Live 0-based stage counter (`GameWork + decoded offset`, 0xC on 2026
/// builds). `None` when the layout, the GameWork pointer, or the counter
/// decode is unavailable — consumers (song-rate eligibility and the rate
/// save-claim decode) fail closed on `None`. Note: `premium_free` freezes
/// this counter while active; that is consistent for rate purposes (the
/// scene-26 arm and the save-time claim read the same frozen value).
pub fn stage_counter() -> Option<i32> {
    let offset = STAGE_COUNTER_OFFSET.load(Ordering::Acquire);
    if offset == 0 {
        return None;
    }
    let game_work = game_work()?;
    Some(unsafe { memory::read_i32(game_work.add(offset)) })
}

/// True when the optional session-state decode (`final_stage_probe` +
/// `max_stage_global`, with all cross-checks) succeeded. The quick-fail fast
/// path requires this; everything else is indifferent.
pub fn session_state_available() -> bool {
    SESSION_STATE_AVAILABLE.load(Ordering::Acquire)
}

/// Live event-mode field (`GameWork+0xD0` on verified builds). 1/2 = the
/// event/special scene chain (which never runs the plain GAMEPLAY scene, but
/// the quick-fail fast path checks it belt-and-braces). `None` when the
/// session-state decode or the GameWork pointer is unavailable.
pub fn event_mode() -> Option<i32> {
    if !session_state_available() {
        return None;
    }
    let offset = EVENT_MODE_OFFSET.load(Ordering::Acquire);
    let game_work = game_work()?;
    Some(unsafe { memory::read_i32(game_work.add(offset)) })
}

/// Live final-stage override (`GameWork+0x10`). Stock code only ever writes
/// -1 (`GameWork::reset`); a non--1 value means some mod made the current
/// stage the last one — the quick-fail fast path then falls back so the
/// natural flow can end the session.
pub fn final_stage_override() -> Option<i32> {
    if !session_state_available() {
        return None;
    }
    let offset = FINAL_STAGE_OVERRIDE_OFFSET.load(Ordering::Acquire);
    let game_work = game_work()?;
    Some(unsafe { memory::read_i32(game_work.add(offset)) })
}

/// The operator's `/gameOptions/max_stage/current` setting (re-read from AVS
/// at every session start by the game). Normal stage count = value + 1; the
/// last normal 0-based stage index = value. `None` when the session-state
/// decode is unavailable.
pub fn max_stage_setting() -> Option<i32> {
    if !session_state_available() {
        return None;
    }
    let global = MAX_STAGE_GLOBAL.load(Ordering::Acquire);
    if global.is_null() {
        return None;
    }
    Some(unsafe { memory::read_i32(global) })
}
