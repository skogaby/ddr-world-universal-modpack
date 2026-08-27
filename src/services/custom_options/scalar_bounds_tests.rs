//! Host tests for [`FrameworkState::set_scalar_bounds`] — the live
//! per-song re-bounding of a scalar row's `min`/`max` (Training Mode R2
//! second amendment: the SONG START/END TIME rows' RANGE is clamped to
//! the highlighted song, so the stepper cannot even express a value past
//! the song's length). Every consumer of the bounds (the press-time
//! stepping clamp, the row's position-marker fraction) reads the registry
//! live under the lock, so a kernel-side mutation is immediately
//! effective — these tests pin the kernel half: field mutation, stored
//! value re-clamping with deferred callback tuples, and the
//! unknown-id/enum refusals.

use super::api::{PersistMode, RegisterSpec, ScalarFormat};
use super::registry::FrameworkState;

fn scalar_spec(id: &'static str, min: i32, max: i32, default: i32) -> RegisterSpec {
    RegisterSpec::scalar(id, min, max, 5, ScalarFormat::Integer)
        .step_coarse(30)
        .default_value(default)
        .persist_mode(PersistMode::Session)
}

fn bounds_of(state: &FrameworkState, id: &str) -> (i32, i32) {
    let idx = state.index_of(id).unwrap();
    match state.options[idx].ui_kind {
        super::api::UiKind::Scalar { min, max, .. } => (min, max),
        _ => panic!("scalar expected"),
    }
}

#[test]
fn bounds_update_and_stick() {
    let mut state = FrameworkState::default();
    state
        .try_register(scalar_spec("sb_end", 5, 200, 200))
        .unwrap();
    // Highlight a 123.4 s song: the END row's range becomes 5..=125.
    let callbacks = state
        .set_scalar_bounds("sb_end", 5, 125)
        .expect("scalar row");
    assert_eq!(bounds_of(&state, "sb_end"), (5, 125));
    // The stored default 200 was above the new max: clamped per side,
    // one deferred callback tuple each.
    assert_eq!(callbacks.len(), 2);
    for (side, tuple) in callbacks.iter().enumerate() {
        assert_eq!(tuple.0, "sb_end");
        assert_eq!(tuple.2, side as u8);
        assert_eq!(tuple.3, 125, "clamped to the new max");
    }
    assert_eq!(state.get_value("sb_end", 0), Some(125));
    assert_eq!(state.get_value("sb_end", 1), Some(125));

    // Re-bounding to a longer song widens the range; values inside the
    // new range are untouched and dispatch nothing.
    let callbacks = state
        .set_scalar_bounds("sb_end", 5, 180)
        .expect("scalar row");
    assert_eq!(bounds_of(&state, "sb_end"), (5, 180));
    assert!(callbacks.is_empty(), "in-range values stay put");
    assert_eq!(state.get_value("sb_end", 0), Some(125));
}

#[test]
fn bounds_clamp_respects_the_new_min_too() {
    let mut state = FrameworkState::default();
    state
        .try_register(scalar_spec("sb_min", 0, 200, 0))
        .unwrap();
    let callbacks = state
        .set_scalar_bounds("sb_min", 5, 90)
        .expect("scalar row");
    assert_eq!(bounds_of(&state, "sb_min"), (5, 90));
    assert_eq!(callbacks.len(), 2, "both sides sat below the new min");
    assert_eq!(state.get_value("sb_min", 0), Some(5));
}

#[test]
fn non_scalar_and_unknown_rows_are_refused() {
    let mut state = FrameworkState::default();
    state
        .try_register(RegisterSpec::bool_toggle("sb_bool"))
        .unwrap();
    assert!(state.set_scalar_bounds("sb_bool", 0, 1).is_none());
    assert!(state.set_scalar_bounds("sb_missing", 0, 10).is_none());
}

#[test]
fn inverted_bounds_are_refused() {
    let mut state = FrameworkState::default();
    state
        .try_register(scalar_spec("sb_inv", 0, 200, 100))
        .unwrap();
    assert!(state.set_scalar_bounds("sb_inv", 90, 10).is_none());
    // Untouched on refusal.
    assert_eq!(bounds_of(&state, "sb_inv"), (0, 200));
    assert_eq!(state.get_value("sb_inv", 0), Some(100));
}
