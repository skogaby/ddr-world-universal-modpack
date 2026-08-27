//! Host tests for `score_guard`'s song-rate additions: the pending rate-save
//! ledger, full-sanitization readiness, and the logout league-strip
//! semantics. Pure state-machine coverage — no hooks, no game memory.
//!
//! Two groups touch process-global statics (tests run in parallel):
//! [`global_stage_suppression_includes_rate_state`] (side-1 rate ledger
//! only) and the training/assist-tick taint tests, which serialize on
//! [`TAINT_TEST_LOCK`] and restore a clean baseline. Every other test uses a
//! private `RateSaveLedger`/`SanitizationReadiness` instance.

use super::score_guard::{
    logout_league_forward_allowed, LeagueStripOutcome, RateAppendOutcome, RateSaveLedger,
    RateSavePolicy, RateSuppressReason, SanitizationReadiness,
};

const P1: usize = 0;
const P2: usize = 1;

fn suppress_no_consume(reason: RateSuppressReason) -> RateSavePolicy {
    RateSavePolicy::SuppressNoConsume(reason)
}

// ── Append identity and Quick Restart deduplication ──────────────────

#[test]
fn append_is_idempotent_per_generation() {
    let ledger = RateSaveLedger::new();
    assert_eq!(ledger.append(P1, 7, 0), RateAppendOutcome::Appended);
    // Quick Restart / idempotent recommit of the same generation.
    assert_eq!(ledger.append(P1, 7, 0), RateAppendOutcome::AlreadyRecorded);
    assert_eq!(ledger.pending_count(P1), 1);
}

#[test]
fn append_rejects_invalid_inputs() {
    let ledger = RateSaveLedger::new();
    assert_eq!(ledger.append(2, 1, 0), RateAppendOutcome::Invalid);
    assert_eq!(ledger.append(P1, 0, 0), RateAppendOutcome::Invalid);
    assert_eq!(ledger.pending_count(P1), 0);
    assert!(!ledger.any_rate_state());
}

#[test]
fn pending_count_ignores_consumed_tombstones() {
    let ledger = RateSaveLedger::new();
    ledger.append(P1, 1, 0);
    ledger.append(P1, 2, 1);
    assert_eq!(ledger.pending_count(P1), 2);
    let claim = ledger.claim(P1, 0).expect("stage 0 pending");
    ledger.consume(claim);
    assert_eq!(ledger.pending_count(P1), 1);
}

// ── Exact claim/consume behavior ─────────────────────────────────────

#[test]
fn claim_takes_oldest_matching_stage_exactly_once() {
    let ledger = RateSaveLedger::new();
    // Two rate plays on the same (frozen) stage index: oldest first.
    ledger.append(P1, 10, 3);
    ledger.append(P1, 11, 3);
    let first = ledger.claim(P1, 3).expect("oldest pending");
    assert_eq!(first.generation, 10);
    ledger.consume(first);
    let second = ledger.claim(P1, 3).expect("next pending");
    assert_eq!(second.generation, 11);
    ledger.consume(second);
    assert!(ledger.claim(P1, 3).is_none());
}

#[test]
fn claim_never_matches_a_different_stage() {
    let ledger = RateSaveLedger::new();
    ledger.append(P1, 5, 2);
    assert!(ledger.claim(P1, 3).is_none());
    assert_eq!(ledger.pending_count(P1), 1);
}

// ── Election: the save-trampoline policy ─────────────────────────────

#[test]
fn election_consumes_exactly_the_matching_save() {
    let ledger = RateSaveLedger::new();
    ledger.append(P1, 42, 1);
    assert_eq!(
        ledger.elect(Some(P1), Some(1)),
        RateSavePolicy::SuppressConsume {
            generation: 42,
            stage_index: 1
        }
    );
    assert_eq!(ledger.pending_count(P1), 0);
}

#[test]
fn duplicate_sender_retry_suppresses_without_second_consumption() {
    let ledger = RateSaveLedger::new();
    ledger.append(P1, 42, 1);
    assert!(matches!(
        ledger.elect(Some(P1), Some(1)),
        RateSavePolicy::SuppressConsume { .. }
    ));
    // The retried sender call for the same stage hits the Consumed tombstone.
    assert_eq!(
        ledger.elect(Some(P1), Some(1)),
        suppress_no_consume(RateSuppressReason::Duplicate)
    );
    assert_eq!(ledger.pending_count(P1), 0);
}

#[test]
fn reordered_save_fails_closed_then_exact_save_consumes() {
    let ledger = RateSaveLedger::new();
    ledger.append(P1, 7, 2);
    // A (clean) stage-3 save arrives while stage 2's rate entry is pending:
    // suppressed WITHOUT consuming — the entry must survive for its own save.
    assert_eq!(
        ledger.elect(Some(P1), Some(3)),
        suppress_no_consume(RateSuppressReason::NoExactMatch)
    );
    assert_eq!(ledger.pending_count(P1), 1);
    assert_eq!(
        ledger.elect(Some(P1), Some(2)),
        RateSavePolicy::SuppressConsume {
            generation: 7,
            stage_index: 2
        }
    );
}

#[test]
fn pending_entries_survive_scene_changes_by_construction() {
    // The ledger has no scene coupling at all: a delayed save after any
    // number of later identity arms still finds its entry.
    let ledger = RateSaveLedger::new();
    ledger.append(P2, 9, 0);
    // ... arbitrary later scene traffic happens elsewhere ...
    assert_eq!(
        ledger.elect(Some(P2), Some(0)),
        RateSavePolicy::SuppressConsume {
            generation: 9,
            stage_index: 0
        }
    );
}

#[test]
fn unknown_side_fails_closed_while_any_rate_state_exists() {
    let ledger = RateSaveLedger::new();
    ledger.append(P2, 3, 0);
    // Unknown side may NOT default to P1, and may not consume P2's entry.
    assert_eq!(
        ledger.elect(None, Some(0)),
        suppress_no_consume(RateSuppressReason::UnknownSide)
    );
    assert_eq!(ledger.pending_count(P2), 1);
    // Out-of-range side behaves exactly like unknown.
    assert_eq!(
        ledger.elect(Some(4), Some(0)),
        suppress_no_consume(RateSuppressReason::UnknownSide)
    );
}

#[test]
fn unknown_side_without_rate_state_defers_to_legacy_policy() {
    let ledger = RateSaveLedger::new();
    assert_eq!(ledger.elect(None, Some(0)), RateSavePolicy::NoRateOpinion);
    assert_eq!(ledger.elect(None, None), RateSavePolicy::NoRateOpinion);
}

#[test]
fn unknown_stage_fails_closed_only_while_side_has_pending() {
    let ledger = RateSaveLedger::new();
    ledger.append(P1, 4, 1);
    assert_eq!(
        ledger.elect(Some(P1), None),
        suppress_no_consume(RateSuppressReason::UnknownStage)
    );
    assert_eq!(ledger.pending_count(P1), 1);
    // The other side is unaffected by P1's pending entry.
    assert_eq!(ledger.elect(Some(P2), None), RateSavePolicy::NoRateOpinion);
    assert_eq!(
        ledger.elect(Some(P2), Some(1)),
        RateSavePolicy::NoRateOpinion
    );
}

// ── Card-in reset ownership ──────────────────────────────────────────

#[test]
fn reset_clears_only_the_matched_side() {
    let ledger = RateSaveLedger::new();
    ledger.append(P1, 1, 0);
    ledger.append(P2, 2, 0);
    // P2's card-in reset cannot erase P1 state.
    ledger.reset_side(P2);
    assert_eq!(ledger.pending_count(P2), 0);
    assert_eq!(ledger.pending_count(P1), 1);
    assert_eq!(
        ledger.elect(Some(P1), Some(0)),
        RateSavePolicy::SuppressConsume {
            generation: 1,
            stage_index: 0
        }
    );
}

#[test]
fn reset_clears_consumed_tombstones_and_reopens_the_stage() {
    let ledger = RateSaveLedger::new();
    ledger.append(P1, 1, 0);
    assert!(matches!(
        ledger.elect(Some(P1), Some(0)),
        RateSavePolicy::SuppressConsume { .. }
    ));
    ledger.reset_side(P1);
    // A fresh session's clean save at the same stage is no longer a duplicate.
    assert_eq!(
        ledger.elect(Some(P1), Some(0)),
        RateSavePolicy::NoRateOpinion
    );
}

#[test]
fn out_of_range_reset_is_a_no_op() {
    let ledger = RateSaveLedger::new();
    ledger.append(P1, 1, 0);
    ledger.reset_side(9);
    assert_eq!(ledger.pending_count(P1), 1);
}

// ── Overflow fail-closed ─────────────────────────────────────────────

#[test]
fn ring_overflow_is_sticky_fail_closed_until_reset() {
    let ledger = RateSaveLedger::new();
    for generation in 1..=8u64 {
        assert_eq!(
            ledger.append(P1, generation, generation as i32),
            RateAppendOutcome::Appended
        );
    }
    assert_eq!(ledger.append(P1, 9, 0), RateAppendOutcome::Overflow);
    assert!(ledger.overflowed(P1));
    // Everything for the side suppresses, even an exact match, without
    // consuming (unrecordable state means nothing can be trusted).
    assert_eq!(
        ledger.elect(Some(P1), Some(1)),
        suppress_no_consume(RateSuppressReason::Overflow)
    );
    assert_eq!(ledger.pending_count(P1), 8);
    // The other side is unaffected.
    assert_eq!(
        ledger.elect(Some(P2), Some(0)),
        RateSavePolicy::NoRateOpinion
    );
    ledger.reset_side(P1);
    assert!(!ledger.overflowed(P1));
    assert_eq!(ledger.pending_count(P1), 0);
    assert_eq!(
        ledger.elect(Some(P1), Some(1)),
        RateSavePolicy::NoRateOpinion
    );
}

// ── Full-sanitization readiness (AC3) ────────────────────────────────

#[test]
fn full_sanitization_requires_every_prerequisite() {
    // Table-driven: each prerequisite individually absent keeps the
    // conjunction false; the complete set flips it true.
    for missing in 0..5usize {
        let readiness = SanitizationReadiness::new();
        let save_detour = missing != 0;
        if missing != 1 {
            readiness.mark_stage_records_ready();
        }
        if missing != 2 {
            readiness.mark_scene_manager_ready();
        }
        if missing != 3 {
            readiness.mark_sanitiser_registered();
        }
        if missing != 4 {
            readiness.mark_league_strip_available();
        }
        assert!(
            !readiness.is_complete(save_detour),
            "prerequisite {missing} absent must fail readiness"
        );
    }
    let readiness = SanitizationReadiness::new();
    readiness.mark_stage_records_ready();
    readiness.mark_scene_manager_ready();
    readiness.mark_sanitiser_registered();
    readiness.mark_league_strip_available();
    assert!(readiness.is_complete(true));
    assert!(readiness.league_strip_available());
}

// ── Logout league-strip semantics ────────────────────────────────────

#[test]
fn league_strip_tri_state_forwards_only_safe_outcomes() {
    assert!(logout_league_forward_allowed(
        LeagueStripOutcome::NodeAbsent
    ));
    assert!(logout_league_forward_allowed(LeagueStripOutcome::Removed));
    assert!(!logout_league_forward_allowed(
        LeagueStripOutcome::RemovalFailed
    ));
}

// ── Training + assist-tick taints (Step 5, R5) ───────────────────────
//
// These tests mutate the process-global taint statics on BOTH sides, so
// they serialize on a shared lock and each restores a clean-taint state
// before releasing it (order independence). They never touch the rate
// ledger, so the rate-ledger global test below stays disjoint.

use std::sync::{Mutex, PoisonError};

static TAINT_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Take the taint-test lock (poison-tolerant: one failing test must not
/// cascade into the others) and force a clean taint baseline.
fn locked_clean_taints() -> std::sync::MutexGuard<'static, ()> {
    let guard = TAINT_TEST_LOCK
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    reset_taints_clean();
    guard
}

/// Clear every per-song/per-session taint the tests below can set.
fn reset_taints_clean() {
    use super::score_guard;
    score_guard::reset_song_taint();
    score_guard::reset_session();
    for side in [P1, P2] {
        score_guard::set_autoplay_taint(side, false);
        score_guard::set_assist_tick_taint(side, false);
    }
}

#[test]
fn training_taint_suppresses_only_its_side() {
    use super::score_guard;
    let _guard = locked_clean_taints();
    assert!(!score_guard::is_stage_suppressed(P1));
    assert!(!score_guard::is_stage_suppressed(P2));
    score_guard::set_training_taint(P1);
    assert!(score_guard::is_stage_suppressed(P1));
    assert!(!score_guard::is_stage_suppressed(P2));
    // Idempotent one-way set.
    score_guard::set_training_taint(P1);
    assert!(score_guard::is_stage_suppressed(P1));
    reset_taints_clean();
    assert!(!score_guard::is_stage_suppressed(P1));
}

#[test]
fn assist_tick_taint_is_level_driven() {
    use super::score_guard;
    let _guard = locked_clean_taints();
    score_guard::set_assist_tick_taint(P1, true);
    assert!(score_guard::is_stage_suppressed(P1));
    assert!(!score_guard::is_stage_suppressed(P2));
    // Level semantics: writing false clears it (the autoplay model).
    score_guard::set_assist_tick_taint(P1, false);
    assert!(!score_guard::is_stage_suppressed(P1));
    reset_taints_clean();
}

#[test]
fn reset_song_taint_clears_training_but_not_assist_tick() {
    use super::score_guard;
    let _guard = locked_clean_taints();
    score_guard::set_training_taint(P1);
    score_guard::set_training_taint(P2);
    score_guard::set_assist_tick_taint(P1, true);
    score_guard::reset_song_taint();
    // P2 carried only the training taint: cleared on both sides.
    assert!(!score_guard::is_stage_suppressed(P2));
    // P1 must STILL be suppressed — its assist-tick taint survives the
    // song reset (cross-mod scene-callback ordering; see score_guard doc).
    assert!(score_guard::is_stage_suppressed(P1));
    // Level-writing assist-tick false proves training really was cleared.
    score_guard::set_assist_tick_taint(P1, false);
    assert!(!score_guard::is_stage_suppressed(P1));
    reset_taints_clean();
}

#[test]
fn reset_session_clears_both_new_sources() {
    use super::score_guard;
    let _guard = locked_clean_taints();
    for side in [P1, P2] {
        score_guard::set_training_taint(side);
        score_guard::set_assist_tick_taint(side, true);
    }
    score_guard::reset_session();
    assert!(!score_guard::is_stage_suppressed(P1));
    assert!(!score_guard::is_stage_suppressed(P2));
    reset_taints_clean();
}

#[test]
fn out_of_range_taint_sides_are_no_ops() {
    use super::score_guard;
    let _guard = locked_clean_taints();
    score_guard::set_training_taint(2);
    score_guard::set_training_taint(usize::MAX);
    score_guard::set_assist_tick_taint(9, true);
    assert!(!score_guard::is_stage_suppressed(P1));
    assert!(!score_guard::is_stage_suppressed(P2));
    // Out-of-range reader stays fail-open.
    assert!(!score_guard::is_stage_suppressed(7));
    reset_taints_clean();
}

#[test]
fn new_sources_compose_with_existing_ones() {
    use super::score_guard;
    let _guard = locked_clean_taints();
    // Training on P1 + the global quick-fail: both sides suppressed; the
    // song reset clears both (both are per-song).
    score_guard::set_training_taint(P1);
    score_guard::set_quick_fail();
    assert!(score_guard::is_stage_suppressed(P1));
    assert!(score_guard::is_stage_suppressed(P2));
    score_guard::reset_song_taint();
    assert!(!score_guard::is_stage_suppressed(P1));
    assert!(!score_guard::is_stage_suppressed(P2));
    // Assist-tick + autoplay on P2: suppressed until BOTH clear.
    score_guard::set_assist_tick_taint(P2, true);
    score_guard::set_autoplay_taint(P2, true);
    assert!(score_guard::is_stage_suppressed(P2));
    score_guard::set_autoplay_taint(P2, false);
    assert!(score_guard::is_stage_suppressed(P2));
    score_guard::set_assist_tick_taint(P2, false);
    assert!(!score_guard::is_stage_suppressed(P2));
    reset_taints_clean();
}

// ── Global integration (rate ledger; touches only side-1 rate state) ──

#[test]
fn global_stage_suppression_includes_rate_state() {
    use super::score_guard;
    // Clean baseline for side 1 (side 0 is left alone in case other suites
    // ever share the process).
    score_guard::reset_rate_state_for_side(1);
    assert!(!score_guard::is_stage_suppressed(1));
    score_guard::append_pending_rate_save(1, 99, 0);
    assert!(score_guard::is_stage_suppressed(1));
    assert!(score_guard::any_pending_rate_state());
    assert_eq!(score_guard::pending_rate_count(1), 1);
    assert!(!score_guard::rate_overflowed(1));
    assert!(matches!(
        score_guard::elect_rate_save_policy(Some(1), Some(0)),
        score_guard::RateSavePolicy::SuppressConsume { .. }
    ));
    assert!(!score_guard::is_stage_suppressed(1));
    score_guard::reset_rate_state_for_side(1);
    // Out-of-range side stays fail-open for the legacy reader.
    assert!(!score_guard::is_stage_suppressed(7));
}
