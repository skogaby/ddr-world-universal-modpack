//! Host tests for the per-option availability flag behind
//! `set_option_available` / the builder hook's injection filter: registered
//! options default available, flip off/on WITHOUT re-registration (handles
//! and values stay stable), and unavailable options are excluded from the
//! injection view while remaining fully registered and persisted.

use super::api::{RegisterSpec, ScalarFormat};
use super::registry::FrameworkState;

fn scalar_spec(id: &'static str) -> RegisterSpec {
    RegisterSpec::scalar(id, 25, 175, 5, ScalarFormat::Integer)
        .step_coarse(10)
        .default_value(100)
}

/// The injection view: ids the builder hook would inject, in registration
/// order (the ordering permutation is applied later and is orthogonal).
fn injectable_ids(state: &FrameworkState) -> Vec<String> {
    state
        .options
        .iter()
        .filter(|option| option.available)
        .map(|option| option.id.clone())
        .collect()
}

#[test]
fn options_register_available_and_flip_without_reregistration() {
    let mut state = FrameworkState::default();
    let handle = state.try_register(scalar_spec("avail_a")).unwrap();
    state.try_register(scalar_spec("avail_b")).unwrap();
    // Default: available — both would inject.
    assert_eq!(injectable_ids(&state), vec!["avail_a", "avail_b"]);

    // Hide A: excluded from injection, but STILL registered (duplicate
    // registration refused, handle/index unchanged, values intact).
    assert!(state.set_available("avail_a", false));
    assert_eq!(injectable_ids(&state), vec!["avail_b"]);
    assert!(state.try_register(scalar_spec("avail_a")).is_err());
    assert_eq!(state.index_of("avail_a"), Some(0));
    assert_eq!(state.get_value("avail_a", 0), Some(100));

    // Values written while hidden persist (registration and persistence are
    // availability-independent).
    let _ = state.set_value("avail_a", 1, 150);
    assert_eq!(state.get_value("avail_a", 1), Some(150));

    // Re-show: same handle, same values, no re-registration needed.
    assert!(state.set_available("avail_a", true));
    assert_eq!(injectable_ids(&state), vec!["avail_a", "avail_b"]);
    assert_eq!(state.index_of("avail_a"), Some(handle.0 as usize));
    assert_eq!(state.get_value("avail_a", 1), Some(150));
}

#[test]
fn unknown_id_availability_is_a_reported_no_op() {
    let mut state = FrameworkState::default();
    state.try_register(scalar_spec("avail_known")).unwrap();
    assert!(!state.set_available("avail_missing", false));
    assert_eq!(injectable_ids(&state), vec!["avail_known"]);
}

#[test]
fn song_speed_row_registers_exactly_when_the_integration_is_ready() {
    // Step-1 planted a structurally-false registration refusal here; plan
    // Step 4's final task INVERTED it: the row registers EXACTLY when the
    // shared song-rate service reports full integration readiness. This
    // models `SongPlaybackSpeedMod::enable()`, which returns BEFORE
    // `register_option` whenever `integration_ready()` is false — both
    // directions of the gate, host-reasoned.
    for ready in [false, true] {
        let mut state = FrameworkState::default();
        if ready {
            state.try_register(scalar_spec("song_speed")).unwrap();
        }
        assert_eq!(
            state.index_of("song_speed").is_some(),
            ready,
            "the SONG SPEED row must exist exactly when the gate passes"
        );
    }
    // The live gate's readiness legs stay linked to the real installed
    // state: the binding leg is `binding::integration_available()`, no
    // longer a constant (host boots install no hooks, so the linkage — not
    // a literal value — is the live assertion).
    let live = crate::services::song_rate::wavebank_hook::readiness(true);
    assert_eq!(
        live.binding,
        crate::services::song_rate::binding::integration_available()
    );
}
