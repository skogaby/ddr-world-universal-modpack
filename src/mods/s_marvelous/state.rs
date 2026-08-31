//! Per-side live state for the S-Marvelous judgement feature: S-Marv
//! counters and the "current combo contains no Marvelous looser than the
//! window" tracking bit.
//!
//! Design: `.agents/planning/2026-08-29-s-marvelous-judgement/design/
//! detailed-design.md` §4.3 / §5.1. Fed from the `judge_submit` detour tap
//! in `power_user_statistics::data_feed` (game thread, every judgement), so
//! the armed path is a handful of relaxed atomic ops and the disarmed cost
//! is one load; single writer (the hook), lock-free readers.
//!
//! This file is deliberately std-only (no `crate::` imports) so the pure
//! transition core is mountable by the host-test harness
//! (`scripts/validate_s_marvelous.sh`) — plain `cargo test` cannot compile
//! the `retour` dependency on non-x86 hosts.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};

/// Stock Marvelous window is ±17 ms; S-Marvelous must be a STRICT subset,
/// so the window caps at 16 (a 17 ms window would classify every Marvelous
/// as S-Marvelous).
pub const MIN_WINDOW_MS: i32 = 1;
pub const MAX_WINDOW_MS: i32 = 16;
pub const DEFAULT_WINDOW_MS: i32 = 12;

/// Clamp an operator-configured window into the valid 1..=16 range.
pub fn clamp_window(ms: i32) -> i32 {
    ms.clamp(MIN_WINDOW_MS, MAX_WINDOW_MS)
}

// ── Pure transition core ────────────────────────────────────────────
//
// The single implementation of the classification semantics. The atomics
// wrapper below loads into this struct, applies, and stores back — so the
// host-tested logic is exactly what runs on the cabinet.

/// One side's per-song classification state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SideState {
    /// Number of S-Marvelous judgements this song.
    pub smarv_count: u32,
    /// Number of Marvelous-grade (grade 0) judgements this song — the
    /// denominator for the per-song log line (`smarv ⊆ marv_total`).
    pub marv_total: u32,
    /// True when the CURRENT combo contains a Marvelous looser than the
    /// window ("not all S-Marvelous anymore"). Reset when a combo
    /// (re)starts.
    pub combo_has_loose_marv: bool,
}

/// Apply one judgement event. Returns `true` iff the event classified as
/// S-Marvelous (the caller triggers the flash re-drive on `true`).
///
/// * `grade` — grade index (`judge_code - 0x1028`): 0=Marvelous, 1=Perfect,
///   2=Great, 3=Good, 4=Boo, 5=Miss, 6=O.K. (freeze).
/// * `ms` — signed ms delta; `None` for O.K. (carries no timing data).
/// * `combo` — the actor's live combo counter AFTER stock bookkeeping for
///   this event. `<= 1` means a combo just (re)started; the bit resets
///   BEFORE this event is classified so the combo's first step counts
///   against a clean slate.
/// * `window_ms` — the S-Marvelous window (validated > 0 by the caller).
pub fn apply_event(
    state: &mut SideState,
    grade: u32,
    ms: Option<i32>,
    combo: i32,
    window_ms: i32,
) -> bool {
    if combo <= 1 {
        state.combo_has_loose_marv = false;
    }
    match grade {
        0 => {
            state.marv_total = state.marv_total.saturating_add(1);
            if let Some(ms) = ms {
                if ms.unsigned_abs() <= window_ms as u32 {
                    state.smarv_count = state.smarv_count.saturating_add(1);
                    return true;
                }
                state.combo_has_loose_marv = true;
            }
        }
        // Worst-tier parity with the stock combo tracker: Perfect/Great/Good
        // degrade the all-S status. Boo/Miss (4/5) break the combo — the
        // next combo start resets the bit. O.K. (6) maps to Marvelous tier
        // in stock and carries no delta: neutral.
        1..=3 => state.combo_has_loose_marv = true,
        _ => {}
    }
    false
}

// ── Atomics wrapper (hot-path state) ────────────────────────────────

/// Armed window per side; 0 = disarmed (doubles as the armed flag).
static WINDOW_MS: [AtomicI32; 2] = [AtomicI32::new(0), AtomicI32::new(0)];
static SMARV_COUNT: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];
static MARV_TOTAL: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];
static COMBO_HAS_LOOSE_MARV: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
/// Sticky copy of the last armed window (see [`last_armed_window`]).
static LAST_WINDOW_MS: [AtomicI32; 2] = [AtomicI32::new(0), AtomicI32::new(0)];

/// Whether a side is armed. The ONLY cost the judge hook pays when the
/// feature is off/disarmed: one relaxed load.
#[inline]
pub fn is_armed(side: usize) -> bool {
    WINDOW_MS[side & 1].load(Ordering::Relaxed) > 0
}

/// Arm a side with the given (already clamped) window at GAMEPLAY entry.
pub fn arm(side: usize, window_ms: i32) {
    let clamped = clamp_window(window_ms);
    WINDOW_MS[side & 1].store(clamped, Ordering::Relaxed);
    LAST_WINDOW_MS[side & 1].store(clamped, Ordering::Relaxed);
}

/// Disarm both sides (GAMEPLAY exit).
pub fn disarm_all() {
    WINDOW_MS[0].store(0, Ordering::Relaxed);
    WINDOW_MS[1].store(0, Ordering::Relaxed);
}

/// The window the side was LAST armed with (sticky across the GAMEPLAY-exit
/// disarm — the results surfaces recompute S-Marv counts from the stage
/// record with the window that was live during the song). 0 = never armed
/// this session; results consumers fail closed to stock display on 0.
pub fn last_armed_window(side: usize) -> i32 {
    LAST_WINDOW_MS[side & 1].load(Ordering::Relaxed)
}

/// Clear per-song counters/bits for both sides (GAMEPLAY entry and every
/// in-place song reset). Leaves the armed windows untouched.
pub fn reset_song_state() {
    for side in 0..2 {
        SMARV_COUNT[side].store(0, Ordering::Relaxed);
        MARV_TOTAL[side].store(0, Ordering::Relaxed);
        COMBO_HAS_LOOSE_MARV[side].store(false, Ordering::Relaxed);
    }
}

pub fn smarv_count(side: usize) -> u32 {
    SMARV_COUNT[side & 1].load(Ordering::Relaxed)
}

/// Marvelous-grade judgement count this song (S-Marv ⊆ this).
pub fn marv_total(side: usize) -> u32 {
    MARV_TOTAL[side & 1].load(Ordering::Relaxed)
}

/// True while the current combo contains no Marvelous looser than the
/// window.
pub fn combo_is_all_smarv(side: usize) -> bool {
    !COMBO_HAS_LOOSE_MARV[side & 1].load(Ordering::Relaxed)
}

/// Hot-path entry, called from the judge_submit tap for every grade opcode
/// (0..=6) of an armed side. Single writer (game thread): load → pure
/// transition → store keeps the semantics in one place. Returns `true` iff
/// the event classified as S-Marvelous (the caller fires the flash
/// re-drive on `true`; disarmed always returns `false`).
#[inline]
pub fn on_judge_event(side: usize, grade_index: u32, ms: Option<i32>, combo: i32) -> bool {
    let side = side & 1;
    let window = WINDOW_MS[side].load(Ordering::Relaxed);
    if window <= 0 {
        return false;
    }
    let mut state = SideState {
        smarv_count: SMARV_COUNT[side].load(Ordering::Relaxed),
        marv_total: MARV_TOTAL[side].load(Ordering::Relaxed),
        combo_has_loose_marv: COMBO_HAS_LOOSE_MARV[side].load(Ordering::Relaxed),
    };
    let is_smarv = apply_event(&mut state, grade_index, ms, combo, window);
    SMARV_COUNT[side].store(state.smarv_count, Ordering::Relaxed);
    MARV_TOTAL[side].store(state.marv_total, Ordering::Relaxed);
    COMBO_HAS_LOOSE_MARV[side].store(state.combo_has_loose_marv, Ordering::Relaxed);
    is_smarv
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean() -> SideState {
        SideState::default()
    }

    #[test]
    fn window_edge_is_inclusive() {
        let mut s = clean();
        assert!(apply_event(&mut s, 0, Some(12), 2, 12));
        assert!(apply_event(&mut s, 0, Some(-12), 3, 12));
        assert_eq!(s.smarv_count, 2);
        assert_eq!(s.marv_total, 2);
        assert!(!s.combo_has_loose_marv);
    }

    #[test]
    fn loose_marvelous_degrades_without_counting() {
        for ms in [13, -13] {
            let mut s = clean();
            assert!(!apply_event(&mut s, 0, Some(ms), 5, 12));
            assert_eq!(s.smarv_count, 0);
            assert_eq!(s.marv_total, 1);
            assert!(s.combo_has_loose_marv);
        }
    }

    #[test]
    fn combo_restart_resets_bit_before_classifying() {
        let mut s = SideState {
            smarv_count: 4,
            marv_total: 5,
            combo_has_loose_marv: true,
        };
        apply_event(&mut s, 0, Some(3), 1, 12);
        assert_eq!(s.smarv_count, 5);
        assert_eq!(s.marv_total, 6);
        assert!(!s.combo_has_loose_marv);
    }

    #[test]
    fn ok_is_neutral() {
        let mut s = clean();
        assert!(!apply_event(&mut s, 6, None, 7, 12));
        assert_eq!(s, clean());

        let mut degraded = SideState {
            smarv_count: 1,
            marv_total: 2,
            combo_has_loose_marv: true,
        };
        let before = degraded;
        apply_event(&mut degraded, 6, None, 7, 12);
        assert_eq!(degraded, before);
    }

    #[test]
    fn perfect_great_good_degrade() {
        for grade in 1..=3 {
            let mut s = clean();
            apply_event(&mut s, grade, Some(20), 4, 12);
            assert!(s.combo_has_loose_marv, "grade {grade} must degrade");
            assert_eq!(s.smarv_count, 0);
        }
    }

    #[test]
    fn boo_breaks_combo_and_next_start_is_clean() {
        let mut s = clean();
        apply_event(&mut s, 2, Some(50), 4, 12); // Great: degraded
        assert!(s.combo_has_loose_marv);
        apply_event(&mut s, 4, Some(150), 0, 12); // Boo: combo broken
        apply_event(&mut s, 0, Some(2), 1, 12); // fresh combo, tight Marvelous
        assert!(!s.combo_has_loose_marv);
        assert_eq!(s.smarv_count, 1);
    }

    #[test]
    fn grade_zero_without_ms_is_defensive_noop() {
        let mut s = clean();
        apply_event(&mut s, 0, None, 4, 12);
        assert_eq!(s.smarv_count, 0);
        assert_eq!(s.marv_total, 1); // still a Marvelous-grade event
        assert!(!s.combo_has_loose_marv);
    }

    #[test]
    fn extreme_ms_does_not_panic() {
        let mut s = clean();
        apply_event(&mut s, 0, Some(i32::MIN), 4, 12);
        assert_eq!(s.smarv_count, 0);
        assert!(s.combo_has_loose_marv);
    }

    #[test]
    fn clamp_window_bounds() {
        assert_eq!(clamp_window(0), 1);
        assert_eq!(clamp_window(1), 1);
        assert_eq!(clamp_window(12), 12);
        assert_eq!(clamp_window(16), 16);
        assert_eq!(clamp_window(17), 16); // strictly below stock Marvelous
        assert_eq!(clamp_window(18), 16);
        assert_eq!(clamp_window(-5), 1);
        assert_eq!(clamp_window(i32::MIN), 1);
        assert_eq!(clamp_window(i32::MAX), 16);
    }

    /// The wrapper shares process-wide statics, so exercise it as one
    /// sequential scenario rather than parallel tests.
    #[test]
    fn wrapper_sequence() {
        // Disarmed: inert.
        disarm_all();
        reset_song_state();
        assert!(!on_judge_event(0, 0, Some(1), 1));
        assert_eq!(smarv_count(0), 0);
        assert!(!is_armed(0));

        // Arm side 0 only.
        arm(0, 12);
        assert!(is_armed(0));
        assert!(!is_armed(1));

        assert!(on_judge_event(0, 0, Some(-4), 1));
        assert!(on_judge_event(0, 0, Some(12), 2));
        assert!(!on_judge_event(1, 0, Some(0), 1)); // side 1 disarmed: ignored
        assert_eq!(smarv_count(0), 2);
        assert_eq!(marv_total(0), 2);
        assert_eq!(smarv_count(1), 0);
        assert_eq!(marv_total(1), 0);
        assert!(combo_is_all_smarv(0));

        assert!(!on_judge_event(0, 0, Some(15), 3)); // loose
        assert_eq!(smarv_count(0), 2);
        assert_eq!(marv_total(0), 3);
        assert!(!combo_is_all_smarv(0));

        // Song reset clears counters but keeps the armed window.
        reset_song_state();
        assert!(is_armed(0));
        assert_eq!(smarv_count(0), 0);
        assert_eq!(marv_total(0), 0);
        assert!(combo_is_all_smarv(0));

        // Arm clamps.
        arm(0, 40);
        assert!(on_judge_event(0, 0, Some(16), 1)); // clamped window 16
        assert!(!on_judge_event(0, 0, Some(17), 2)); // stock Marvelous edge stays loose
        assert_eq!(smarv_count(0), 1);

        disarm_all();
        assert!(!on_judge_event(0, 0, Some(1), 2)); // disarmed again
        assert_eq!(smarv_count(0), 1);
        reset_song_state();
    }
}

#[cfg(test)]
mod combo_override_predicate_tests {
    //! Step-5 plan test: the combo override predicate's state half
    //! (`combo_is_all_smarv`) across the §4.5 sequences. The other half
    //! (`stock worst == 0`) is the game's own field — cabinet-verified.
    use super::*;

    fn seq(events: &[(u32, Option<i32>, i32)]) -> SideState {
        let mut s = SideState::default();
        for &(grade, ms, combo) in events {
            apply_event(&mut s, grade, ms, combo, 12);
        }
        s
    }

    #[test]
    fn all_smarv_combo_keeps_the_bit() {
        let s = seq(&[(0, Some(3), 1), (0, Some(-8), 2), (0, Some(12), 3)]);
        assert!(!s.combo_has_loose_marv);
    }

    #[test]
    fn loose_marvelous_drops_it_mid_combo() {
        let s = seq(&[(0, Some(3), 1), (0, Some(14), 2)]);
        assert!(s.combo_has_loose_marv);
    }

    #[test]
    fn ok_step_is_neutral() {
        // Freeze O.K. (grade 6, no ms) maps to Marvelous tier in stock and
        // carries no timing delta — must NOT degrade the all-S status.
        let s = seq(&[(0, Some(3), 1), (6, None, 2), (0, Some(-2), 3)]);
        assert!(!s.combo_has_loose_marv);
    }

    #[test]
    fn combo_break_resets_on_next_combo_start() {
        let s = seq(&[
            (0, Some(15), 1), // loose marv — bit set
            (5, None, 0),     // miss: combo broken
            (0, Some(2), 1),  // new combo start (combo <= 1) resets
        ]);
        assert!(!s.combo_has_loose_marv);
    }

    #[test]
    fn lower_grades_degrade() {
        for g in 1..=3u32 {
            let s = seq(&[(0, Some(1), 1), (g, Some(1), 2)]);
            assert!(s.combo_has_loose_marv, "grade {} must degrade", g);
        }
    }
}
