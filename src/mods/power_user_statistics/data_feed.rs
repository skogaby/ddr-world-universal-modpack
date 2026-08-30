//! Per-step ms-error data feed — installs a retour detour on `judge_submit`
//! to capture per-step timing data into a shared buffer for Timing Stats,
//! Pacemaker→MsError, and CSV Export sub-features. Also hosts the minimal
//! hot-path taps for features whose policy lives elsewhere: the
//! auto-calibration accumulator (`timing_offsets::calibration`) and the
//! S-Marvelous classification feed (`s_marvelous::state`) — the detour body
//! is the only place the per-step ms error exists (one detour per target).

use std::sync::atomic::{AtomicI32, AtomicI64, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use retour::GenericDetour;

use crate::core::signatures::SignatureStore;
use crate::services::song_rate::clock_patch::RateSnapshot;
use crate::{log_info, log_warn};

use super::{csv_export, timing_stats_widget};

/// Normal grade judgment opcodes: 0x1028 + grade (0..7).
/// Grades 0..5 (M/P/G/Gd/Boo/Miss) carry meaningful ms-error.
/// Grade 6 (OK = freeze hold) carries EX but no ms-error.
const OPCODE_GRADE_BASE: u32 = 0x1028;
const OPCODE_MARVELOUS: u32 = 0x1028;
const OPCODE_PERFECT: u32 = 0x1029;
const OPCODE_GREAT: u32 = 0x102A;
const OPCODE_OK: u32 = 0x102E;

/// Player side offset on GamePlayActor.
const ACTOR_PLAY_SIDE_OFFSET: usize = 0x84;
/// Live combo counter on GamePlayActor (read by the S-Marvelous tap, only
/// while a side is armed).
const ACTOR_COMBO_OFFSET: usize = 0x1DC;

// NOTE: this module used to also carry a "session/match struct" offset table
// (`ACTOR_SESSION_OFFSET = 0x88` plus songcode/chart-info offsets hanging off
// it). All of it was dead code, and the premise was wrong: `GamePlayActor+0x88`
// is the play STYLE int (`1 == DOUBLE`), not a pointer to anything. The live
// song-identity path is `csv_export::read_song_identity`, which goes through
// the actor's parent DancePlaySequence (`actor+0x08`) and reads the basename
// at `DPS+0xA0` and the difficulty at `DPS+0x50`. Removed 2026-07-25 so the
// wrong offsets can't be picked up by a future reader.

type JudgeSubmitFn =
    unsafe extern "C" fn(actor: *mut u8, result: *mut u8, judge_code: u32, scratch: *mut u8);

static DETOUR: OnceLock<GenericDetour<JudgeSubmitFn>> = OnceLock::new();

/// Per-player accumulated ms-error statistics for the current song.
pub struct MsErrorAccum {
    pub current: i32,
    pub max_abs: i32,
    pub sum_abs: i64,
    pub sum: i64,
    pub count: u32,
    /// Cumulative EX score lost (max_possible - actual).
    /// Each step: loss = 3 - ex_value_for_grade.
    pub ex_loss: i32,
    pub per_step: Option<Vec<StepRecord>>,
    /// Songcode + difficulty captured on the first judgment of the song.
    pub song_identity: Option<SongIdentity>,
}

pub struct SongIdentity {
    pub songcode: String,
    pub difficulty: i32,
    /// The song-rate snapshot latched WITH the identity (first judgment —
    /// strictly after any loader-thread rate commit). The CSV export's rate
    /// columns read this copy, never the live publication, which resets to
    /// identity at gameplay exit before the scene-28 flush (design req 34).
    pub rate: RateSnapshot,
}

pub struct StepRecord {
    pub expected_ms: i32,
    pub actual_ms: i32,
    pub delta_ms: i32,
}

impl MsErrorAccum {
    pub fn new() -> Self {
        Self {
            current: 0,
            max_abs: 0,
            sum_abs: 0,
            sum: 0,
            count: 0,
            ex_loss: 0,
            per_step: None,
            song_identity: None,
        }
    }

    pub fn reset(&mut self, collect_per_step: bool) {
        self.current = 0;
        self.max_abs = 0;
        self.sum_abs = 0;
        self.sum = 0;
        self.count = 0;
        self.ex_loss = 0;
        self.song_identity = None;
        if collect_per_step {
            self.per_step = Some(Vec::new());
        } else {
            self.per_step = None;
        }
    }
}

/// EX value per grade: M=3, P=2, G=1, Good/Boo/Miss=0, OK=3, NG=0.
fn ex_value_for_opcode(judge_code: u32) -> i32 {
    match judge_code {
        OPCODE_MARVELOUS => 3,
        OPCODE_PERFECT => 2,
        OPCODE_GREAT => 1,
        OPCODE_OK => 3,
        _ => 0,
    }
}

static BUFFERS: OnceLock<[Mutex<MsErrorAccum>; 2]> = OnceLock::new();

/// Atomic snapshot of the most recent ms-error per player — readable
/// without locking from the pacemaker swap detour (hot path).
static LATEST_MS_ERROR: [AtomicI32; 2] = [AtomicI32::new(0), AtomicI32::new(0)];

pub fn buffers() -> &'static [Mutex<MsErrorAccum>; 2] {
    BUFFERS.get_or_init(|| {
        [
            Mutex::new(MsErrorAccum::new()),
            Mutex::new(MsErrorAccum::new()),
        ]
    })
}

pub fn latest_ms_error(player: usize) -> i32 {
    LATEST_MS_ERROR[player & 1].load(Ordering::Acquire)
}

/// Reset both buffers (called on scene 28 entry).
pub fn reset_buffers(collect_per_step_p1: bool, collect_per_step_p2: bool) {
    let bufs = buffers();
    if let Ok(mut b) = bufs[0].lock() {
        b.reset(collect_per_step_p1);
    }
    if let Ok(mut b) = bufs[1].lock() {
        b.reset(collect_per_step_p2);
    }
    LATEST_MS_ERROR[0].store(0, Ordering::Release);
    LATEST_MS_ERROR[1].store(0, Ordering::Release);
}

// ── Auto-calibration tap ────────────────────────────────────────────
//
// A minimal per-song accumulator for the timing-offsets auto-calibration
// feature (`src/mods/timing_offsets/calibration.rs`). Lives here because the
// detour body is the only place the per-step ms error exists (one detour per
// target); the calibration POLICY stays in timing_offsets. Pure relaxed
// atomics: disarmed cost is one load + compare per judgment; the take
// happens on the scene-change thread strictly after the song's last
// judgment, so ordering between the three counters is immaterial.

/// Side being calibrated, or -1 when disarmed.
static CALIB_SIDE: AtomicI32 = AtomicI32::new(-1);
/// Sum of signed ms errors (grades M/P/G/Gd/Boo of the calibrated side).
static CALIB_SUM: AtomicI64 = AtomicI64::new(0);
/// Sum of squared ms errors (stddev derivation).
static CALIB_SUM_SQ: AtomicI64 = AtomicI64::new(0);
/// Sample count.
static CALIB_COUNT: AtomicU32 = AtomicU32::new(0);

/// Arm the calibration tap for `side` (0/1), clearing the counters.
pub fn calibration_arm(side: usize) {
    calibration_reset();
    CALIB_SIDE.store((side & 1) as i32, Ordering::Release);
}

/// Clear the counters (in-place song restart / training scrub) without
/// changing the armed side.
pub fn calibration_reset() {
    CALIB_SUM.store(0, Ordering::Relaxed);
    CALIB_SUM_SQ.store(0, Ordering::Relaxed);
    CALIB_COUNT.store(0, Ordering::Relaxed);
}

/// Snapshot `(sum, sum_sq, count)` and disarm the tap.
pub fn calibration_take() -> (i64, i64, u32) {
    CALIB_SIDE.store(-1, Ordering::Release);
    (
        CALIB_SUM.load(Ordering::Relaxed),
        CALIB_SUM_SQ.load(Ordering::Relaxed),
        CALIB_COUNT.load(Ordering::Relaxed),
    )
}

/// Install the detour on `judge_submit`. Idempotent: returns `true` when the
/// detour is already installed (both the power-user-statistics mod and the
/// timing-offsets calibration sub-feature call this — whichever inits first
/// installs). Returns false on failure.
pub fn install(signatures: &SignatureStore) -> bool {
    if DETOUR.get().is_some() {
        return true;
    }
    let Some(addr) = signatures.get_address("judge_submit") else {
        log_warn!("data_feed: judge_submit signature not resolved");
        return false;
    };

    unsafe {
        let target: JudgeSubmitFn = std::mem::transmute(addr);
        let detour = match GenericDetour::new(target, judge_submit_hook) {
            Ok(d) => d,
            Err(e) => {
                log_warn!("data_feed: failed to create detour: {}", e);
                return false;
            }
        };
        // Publish to the OnceLock BEFORE enabling: once the target prologue
        // is patched, any thread can enter the hook, which needs `.get()`
        // to reach the original (store-before-enable rule).
        if DETOUR.set(detour).is_err() {
            log_warn!("data_feed: detour slot already populated");
            return false;
        }
        let enable_result = DETOUR.get().map(|d| d.enable());
        if let Some(Err(e)) = enable_result {
            log_warn!("data_feed: failed to enable detour: {}", e);
            return false;
        }
    }

    log_info!("data_feed: installed judge_submit detour @ {:p}", addr);
    true
}

/// Whether the judge_submit detour is live (the calibration sub-feature
/// gates its row registration on this).
pub fn is_installed() -> bool {
    DETOUR.get().is_some()
}

unsafe extern "C" fn judge_submit_hook(
    actor: *mut u8,
    result: *mut u8,
    judge_code: u32,
    scratch: *mut u8,
) {
    // Only process grade opcodes (0x1028..0x102E for M/P/G/Gd/Boo/Miss/OK).
    // Skip shock codes (0x1030, 0x1031) and cancel (0x1046).
    let grade_index = judge_code.wrapping_sub(OPCODE_GRADE_BASE);
    let is_grade_opcode = grade_index <= 6; // 0..=6 covers M through OK

    // S-Marvelous flash re-drive, deferred to AFTER the original call at
    // the bottom: the original's 0x1028 handler plays `in_marvelous` on
    // the judgement clip — a re-drive issued before it gets clobbered the
    // same event (deploy #5: green log, stock word on screen). Set by the
    // classification tap below.
    let mut smarv_side: Option<usize> = None;

    if is_grade_opcode {
        let player_side = *(actor.add(ACTOR_PLAY_SIDE_OFFSET) as *const i32) as usize;

        if player_side <= 1 {
            // ── S-Marvelous classification tap ──────────────────────
            // Feeds `s_marvelous::state` (policy lives in that mod). Must
            // see EVERY grade opcode 0..=6 — the combo-bit machine needs
            // grades 1..6 too, and O.K. passes `ms: None` — so it sits
            // before the has_ms_error split. Disarmed cost: one relaxed
            // load. Armed: two extra aligned reads + a few relaxed
            // atomics; independent of the try_lock buffer path below so a
            // contended lock can never drop an S-Marv event.
            if crate::mods::s_marvelous::state::is_armed(player_side) {
                let combo = *(actor.add(ACTOR_COMBO_OFFSET) as *const i32);
                let ms = if judge_code != OPCODE_OK && !scratch.is_null() {
                    Some(*(scratch.add(4) as *const i32))
                } else {
                    None
                };
                if crate::mods::s_marvelous::state::on_judge_event(
                    player_side,
                    grade_index,
                    ms,
                    combo,
                ) {
                    // S-Marvelous: re-drive the judgement flash AFTER the
                    // original below (which plays the stock in_marvelous).
                    smarv_side = Some(player_side);
                }
            }

            let ex_earned = ex_value_for_opcode(judge_code);
            let ex_loss_this_step = 3 - ex_earned;

            // OK (freeze hold) contributes EX but has no ms-error timing data.
            let has_ms_error = judge_code != OPCODE_OK && !scratch.is_null();

            if has_ms_error {
                let ms_error = *(scratch.add(4) as *const i32);
                LATEST_MS_ERROR[player_side].store(ms_error, Ordering::Release);

                // Auto-calibration tap: grades M/P/G/Gd/Boo (index 0..=4 —
                // Miss sits at the window edge and is excluded) of the armed
                // side only. Disarmed cost: one relaxed load + compare.
                if grade_index <= 4 && player_side as i32 == CALIB_SIDE.load(Ordering::Relaxed) {
                    CALIB_SUM.fetch_add(ms_error as i64, Ordering::Relaxed);
                    CALIB_SUM_SQ
                        .fetch_add((ms_error as i64) * (ms_error as i64), Ordering::Relaxed);
                    CALIB_COUNT.fetch_add(1, Ordering::Relaxed);
                }

                let bufs = buffers();
                if let Ok(mut b) = bufs[player_side].try_lock() {
                    b.current = ms_error;
                    let abs_err = ms_error.unsigned_abs() as i32;
                    if abs_err > b.max_abs {
                        b.max_abs = abs_err;
                    }
                    b.sum_abs += abs_err as i64;
                    b.sum += ms_error as i64;
                    b.count += 1;
                    b.ex_loss += ex_loss_this_step;

                    if let Some(ref mut steps) = b.per_step {
                        let note_ptr = *(result as *const *const u8);
                        let expected_ms = if !note_ptr.is_null() {
                            *(note_ptr.add(0x08) as *const i32)
                        } else {
                            0
                        };
                        let actual_ms = expected_ms + ms_error;
                        steps.push(StepRecord {
                            expected_ms,
                            actual_ms,
                            delta_ms: ms_error,
                        });
                    }
                }
            } else if judge_code == OPCODE_OK {
                // Freeze hold: EX contribution only, no ms-error.
                let bufs = buffers();
                if let Ok(mut b) = bufs[player_side].try_lock() {
                    b.ex_loss += ex_loss_this_step;
                }
            }

            csv_export::snapshot_song_identity(actor, player_side);
            timing_stats_widget::update_text(player_side);
        }
    }

    // Always call the original so score/combo/gauge updates proceed.
    if let Some(detour) = DETOUR.get() {
        detour.call(actor, result, judge_code, scratch);
    }

    // S-Marvelous flash re-drive — must run after the original's stock
    // `in_marvelous` play so the label jump is the LAST write this event.
    // Passes the dispatch actor: the NoteResultActor (whose stored wrapper
    // the stock handler drives) lives in its subtree.
    if let Some(side) = smarv_side {
        crate::mods::s_marvelous::flash::on_smarvelous(side, actor);
    }
}
