use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::movie_policy::{CallOutcome, MoviePolicy, MovieSuppressor};

const VFW_E_CANNOT_RENDER: u32 = 0x8004_0218;

/// Drives one `call` against a 32-byte fake player and reports
/// (returned hr, outcome, original-call count, state dword, opened byte).
fn drive(policy: &MoviePolicy, original_hr: u32) -> (u32, CallOutcome, usize, u32, u8) {
    let calls = AtomicUsize::new(0);
    let mut player = [0xa5u8; 32];
    let (hr, outcome) = unsafe {
        policy.call(
            player.as_mut_ptr().cast::<c_void>(),
            std::ptr::null_mut(),
            |_, _| {
                calls.fetch_add(1, Ordering::AcqRel);
                original_hr
            },
        )
    };
    let state = u32::from_le_bytes(player[8..12].try_into().unwrap());
    (
        hr,
        outcome,
        calls.load(Ordering::Acquire),
        state,
        player[0x14],
    )
}

#[test]
fn contributor_combinations_preserve_original_or_exact_stub_behavior() {
    for non_native in [false, true] {
        for song_rate in [false, true] {
            let policy = MoviePolicy::new();
            policy.set(MovieSuppressor::NonNativeOs, non_native);
            policy.set(MovieSuppressor::SongRate, song_rate);
            let (hr, outcome, calls, state, opened) = drive(&policy, 0x1234);
            if non_native || song_rate {
                assert_eq!(hr, 0);
                assert_eq!(outcome, CallOutcome::Suppressed);
                assert_eq!(calls, 0);
                assert_eq!(state, 3);
                assert_eq!(opened, 0xa5);
            } else {
                assert_eq!(hr, 0x1234);
                assert_eq!(outcome, CallOutcome::Passthrough);
                assert_eq!(calls, 1);
                assert_eq!(state, u32::from_le_bytes([0xa5; 4]));
                assert_eq!(opened, 0xa5);
            }
        }
    }
}

#[test]
fn fallback_mode_passes_through_successful_builds() {
    let policy = MoviePolicy::new();
    policy.set(MovieSuppressor::NonNativeOs, true);
    policy.set_fallback(true);
    let (hr, outcome, calls, state, opened) = drive(&policy, 0);
    assert_eq!(hr, 0);
    assert_eq!(outcome, CallOutcome::Passthrough);
    assert_eq!(calls, 1);
    // player untouched — the real success epilogue already wrote state
    assert_eq!(state, u32::from_le_bytes([0xa5; 4]));
    assert_eq!(opened, 0xa5);
}

#[test]
fn fallback_mode_fakes_success_on_failed_builds() {
    let policy = MoviePolicy::new();
    policy.set(MovieSuppressor::NonNativeOs, true);
    policy.set_fallback(true);
    let (hr, outcome, calls, state, opened) = drive(&policy, VFW_E_CANNOT_RENDER);
    assert_eq!(hr, 0);
    assert_eq!(outcome, CallOutcome::FallbackFaked(VFW_E_CANNOT_RENDER));
    assert_eq!(calls, 1);
    assert_eq!(state, 3);
    // opened byte must stay untouched — get-frame gates COM access on it
    assert_eq!(opened, 0xa5);
}

#[test]
fn fallback_mode_does_not_fake_positive_success_codes() {
    // BuildGraph returns 0 on success today, but only FAILED hrs (high bit)
    // may trigger the fake — an S_ partial-success code passes through.
    let policy = MoviePolicy::new();
    policy.set(MovieSuppressor::NonNativeOs, true);
    policy.set_fallback(true);
    let (hr, outcome, calls, state, _) = drive(&policy, 0x0004_0242);
    assert_eq!(hr, 0x0004_0242);
    assert_eq!(outcome, CallOutcome::Passthrough);
    assert_eq!(calls, 1);
    assert_eq!(state, u32::from_le_bytes([0xa5; 4]));
}

#[test]
fn song_rate_suppression_wins_over_fallback_mode() {
    let policy = MoviePolicy::new();
    policy.set(MovieSuppressor::NonNativeOs, true);
    policy.set_fallback(true);
    policy.set(MovieSuppressor::SongRate, true);
    let (hr, outcome, calls, state, _) = drive(&policy, 0);
    assert_eq!(hr, 0);
    assert_eq!(outcome, CallOutcome::Suppressed);
    assert_eq!(calls, 0);
    assert_eq!(state, 3);
}

#[test]
fn fallback_flag_is_inert_without_the_contributor() {
    let policy = MoviePolicy::new();
    policy.set_fallback(true);
    let (hr, outcome, calls, state, _) = drive(&policy, VFW_E_CANNOT_RENDER);
    assert_eq!(hr, VFW_E_CANNOT_RENDER);
    assert_eq!(outcome, CallOutcome::Passthrough);
    assert_eq!(calls, 1);
    assert_eq!(state, u32::from_le_bytes([0xa5; 4]));
}

#[test]
fn contributors_are_independent_and_song_rate_starts_false() {
    let policy = MoviePolicy::new();
    assert!(!policy.is_suppressed(MovieSuppressor::NonNativeOs));
    assert!(!policy.is_suppressed(MovieSuppressor::SongRate));
    policy.set(MovieSuppressor::NonNativeOs, true);
    assert!(policy.should_suppress());
    assert!(!policy.is_suppressed(MovieSuppressor::SongRate));
    // fallback mode converts full suppression into try-then-fallback
    policy.set_fallback(true);
    assert!(!policy.should_suppress());
    policy.set_fallback(false);
    assert!(policy.should_suppress());
    policy.set(MovieSuppressor::NonNativeOs, false);
    assert!(!policy.should_suppress());
}
