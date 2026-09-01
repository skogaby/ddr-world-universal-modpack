//! Premium Free diagnostics — WARN-ONLY signature taps for the two open bug
//! reports (2026-09-01). Silent in healthy play (two clean 1P + 2P cabinet
//! runs); a `BUG-1` / `BUG-2 SIGNATURE` line in a field log pinpoints the
//! stale record. Active only while the freeze patch is on.
//!
//! **Bug 1** (results graph tab: previous song's timing stats + empty
//! visualisation). The GraphTab ctor stamps `tab+0x14C = GameWork+0xC` and
//! its ingest reads the ms-error stream (`rec+0xD8`, → numeric stats) and the
//! note-entry vector (`rec+0x98`, → `has_data` + series) of
//! `record[tab_stage]`. The result commit writes the same index with
//! replace semantics — so statically the tab should see the current play.
//! The taps below dump the record header + all three stream lengths for BOTH
//! sides at GAMEPLAY entry and RESULTS_DETAIL entry, plus the result
//! commit's two early-outs (`actor+0x280`/`+0x288` nonzero ⇒ commit SKIPPED ⇒
//! the record keeps the previous play).
//!
//! **Bug 2** (STAGE_INDICATOR shows the previous difficulty on a same-song
//! re-pick). The song-select commit writes the display difficulty
//! `PlayerWork+0x5C` (and `+0x54` mcode) ONLY inside the same
//! `new_mcode != rec->mcode` guard the stale-record fix forces open. The taps
//! log `rec+0x00/+0x04` vs `PW+0x54/+0x5C` at every scene of the decide
//! chain and WARN on disagreement at GAMEPLAY entry.
//!
//! 1P run 2026-09-01 (20260825): neither signature fired; the tester's
//! session was 2P, so every dump covers both sides (entered or not — the
//! song-select commit writes both sides' records regardless).

use crate::core::memory;
use crate::log_warn;
use crate::services::{scene_manager, stage_records};
use crate::types::scenes::scene;

use super::ghost_cache::{REC_DIFF, REC_MCODE};

/// `PlayerWork+0x54` — the side's currently selected music code; written by
/// the song-select commit's guarded path (RE: `FUN_1800fdfa0` →
/// `FUN_1800fcf40`, 20260721). DIAGNOSTIC READ ONLY.
const DIAG_PW_MCODE: usize = 0x54;
/// `PlayerWork+0x5C` — the side's currently selected (clamped) difficulty;
/// same writer. DIAGNOSTIC READ ONLY.
const DIAG_PW_DIFF: usize = 0x5C;

const REC_GRADES_VEC: usize = 0xB8;
const REC_ERRORS_VEC: usize = 0xD8;

/// Result-commit early-out fields (GamePlayActor).
const GPA_SKIP_BYTE: usize = 0x280;
const GPA_SKIP_QWORD: usize = 0x288;
const GPA_SIDE: usize = 0x84;

/// Scene-entry tap: called from the mod's scene callback (freeze active).
pub fn on_scene_enter(_prev: i32, next: i32) {
    // Both checks are GAMEPLAY-entry invariants (record freshly prepared,
    // display fields refreshed by the same guarded commit path).
    if next != scene::GAMEPLAY {
        return;
    }
    let Some(stage) = stage_records::stage_counter() else {
        return;
    };
    if !(0..stage_records::MAX_STAGE_RECORDS as i32).contains(&stage) {
        return;
    }
    for side in 0..2usize {
        let entered = stage_records::side_entered(side) == Some(true);
        let (Some(rec), Some(pw)) = (
            stage_records::stage_record(side, stage as usize),
            stage_records::player_work(side),
        ) else {
            continue;
        };
        unsafe {
            let rec_mcode = memory::read_i32(rec.add(REC_MCODE));
            let rec_diff = memory::read_i32(rec.add(REC_DIFF));
            let pw_mcode = memory::read_i32(pw.add(DIAG_PW_MCODE));
            let pw_diff = memory::read_i32(pw.add(DIAG_PW_DIFF));
            let grades = vec_len(rec, REC_GRADES_VEC);
            let errors = vec_len(rec, REC_ERRORS_VEC) / 2;
            if !entered {
                continue;
            }
            if rec_mcode >= 0 && (rec_mcode != pw_mcode || rec_diff != pw_diff) {
                log_warn!(
                    "PremiumFree[diag] BUG-2 SIGNATURE: P{} record (mcode {}, diff {}) != PlayerWork display fields (mcode {}, diff {}) at GAMEPLAY entry -- song-select commit guard did not refresh both",
                    side + 1,
                    rec_mcode,
                    rec_diff,
                    pw_mcode,
                    pw_diff
                );
            }
            if grades != 0 || errors != 0 {
                log_warn!(
                    "PremiumFree[diag] BUG-1 SIGNATURE: P{} record streams NOT empty at GAMEPLAY entry (grades={}, errors={}) -- record was not re-prepared, results will show stale streams",
                    side + 1,
                    grades,
                    errors
                );
            }
        }
    }
}

/// Pre-original tap on the result commit: report the early-outs. Attract-
/// demo GamePlayActors carry `actor+0x280 = 1` by design (the demo never
/// commits) — only a real GAMEPLAY-scene skip is a bug-1 signature.
pub fn on_result_commit_pre(actor: *const u8) {
    // The commit runs after the scene id has already advanced past GAMEPLAY
    // (2P run 2026-09-01), so gate on "inside a play session" instead.
    if scene_manager::current_scene() < scene::SONG_SELECT {
        return;
    }
    unsafe {
        let side = memory::read_i32(actor.add(GPA_SIDE));
        let skip1 = memory::read_u8(actor.add(GPA_SKIP_BYTE));
        let skip2 = memory::read_u64(actor.add(GPA_SKIP_QWORD));
        if skip1 != 0 || skip2 != 0 {
            log_warn!(
                "PremiumFree[diag] BUG-1 SIGNATURE: result commit for P{} will be SKIPPED (actor+0x280={}, actor+0x288=0x{:X}) -- record keeps the previous play",
                side + 1,
                skip1,
                skip2
            );
        }
    }
}

/// Post-original tap on the result commit: WARN if the commit left a record
/// that does not look like a fresh play (the only way results can be stale).
pub fn on_result_commit_post(side: i32, stage: i32, rec: *const u8, grades: Option<usize>) {
    unsafe {
        let errors = vec_len(rec, REC_ERRORS_VEC) / 2;
        if grades.is_none() || grades == Some(0) || errors == 0 {
            log_warn!(
                "PremiumFree[diag] BUG-1 SIGNATURE: result commit P{} left rec[{}] (mcode {}) with empty streams (grades={:?}, errors={}) -- results graph will be empty",
                side + 1,
                stage,
                memory::read_i32(rec.add(REC_MCODE)),
                grades,
                errors
            );
        }
    }
}

/// Byte length of a `vector<T>` at `rec+off` (0 on any implausible shape).
unsafe fn vec_len(rec: *const u8, off: usize) -> usize {
    let b = memory::read_ptr(rec.add(off)) as usize;
    let e = memory::read_ptr(rec.add(off + 8)) as usize;
    if b == 0 || e < b || e - b > 0x100_0000 {
        return 0;
    }
    e - b
}
