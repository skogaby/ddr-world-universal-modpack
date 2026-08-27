//! Host tests for `UiKind::Header` registration validation: headers are
//! stateless, display-only rows — the registry refuses a header spec carrying
//! persistence, callbacks, parent/child links, transforms, or a value, and a
//! registered header is inert on every persistence surface (no wire field, no
//! JSON cache, no session reset) and injects no preview art.

use super::api::{PersistMode, RegisterError, RegisterSpec, ShowWhen, UiKind};
use super::registry::FrameworkState;

fn assert_header_refused(spec: RegisterSpec, what: &str) {
    let mut state = FrameworkState::default();
    match state.try_register(spec) {
        Err(RegisterError::HeaderCarriesState { .. }) => {}
        other => panic!("header spec carrying {what} must be refused, got {other:?}"),
    }
    // A refused registration leaves nothing behind.
    assert!(state.options.is_empty());
}

#[test]
fn plain_header_registers_stateless() {
    let mut state = FrameworkState::default();
    let handle = state
        .try_register(RegisterSpec::header("hdr_group"))
        .unwrap();
    assert_eq!(state.index_of("hdr_group"), Some(handle.0 as usize));

    let opt = &state.options[handle.0 as usize];
    assert!(matches!(opt.ui_kind, UiKind::Header));
    assert!(matches!(opt.show_when, ShowWhen::Always));
    assert_eq!(opt.persist, PersistMode::None);
    assert_eq!(opt.values, [0, 0]);
    assert!(opt.save_transform.is_none());
    assert!(opt.load_transform.is_none());
    assert!(opt.available, "headers default available like any option");
}

#[test]
fn header_with_persistence_is_refused() {
    for mode in [
        PersistMode::Full,
        PersistMode::SaveOnly,
        PersistMode::Session,
    ] {
        assert_header_refused(
            RegisterSpec::header("hdr_persist").persist_mode(mode),
            "persistence",
        );
    }
}

#[test]
fn header_with_change_callback_is_refused() {
    fn some_callback(_side: u8, _value: i32) {}
    assert_header_refused(
        RegisterSpec::header("hdr_cb").on_change(some_callback),
        "a change callback",
    );
}

#[test]
fn header_with_show_when_link_is_refused() {
    // Register a real parent first so the refusal exercised is the header
    // rule, not UnknownParent.
    let mut state = FrameworkState::default();
    state
        .try_register(RegisterSpec::bool_toggle("hdr_parent"))
        .unwrap();
    let spec = RegisterSpec::header("hdr_child").show_when(ShowWhen::Equals {
        parent_id: "hdr_parent".to_string(),
        value: 1,
    });
    match state.try_register(spec) {
        Err(RegisterError::HeaderCarriesState { .. }) => {}
        other => panic!("header with a parent link must be refused, got {other:?}"),
    }
    assert_eq!(state.options.len(), 1);
}

#[test]
fn header_with_transforms_is_refused() {
    fn identity(_id: &str, value: i32) -> i32 {
        value
    }
    assert_header_refused(
        RegisterSpec::header("hdr_save_t").save_transform(identity),
        "a save transform",
    );
    assert_header_refused(
        RegisterSpec::header("hdr_both_t").persist_transform(identity, identity),
        "persist transforms",
    );
}

#[test]
fn header_with_nonzero_default_value_is_refused() {
    assert_header_refused(
        RegisterSpec::header("hdr_value").default_value(1),
        "a value",
    );
}

#[test]
fn duplicate_header_id_is_refused_as_duplicate() {
    let mut state = FrameworkState::default();
    state.try_register(RegisterSpec::header("hdr_dup")).unwrap();
    match state.try_register(RegisterSpec::header("hdr_dup")) {
        Err(RegisterError::Duplicate { .. }) => {}
        other => panic!("duplicate header id must hit the Duplicate path, got {other:?}"),
    }
}

#[test]
fn headers_are_inert_on_every_persistence_surface() {
    let mut state = FrameworkState::default();
    state
        .try_register(RegisterSpec::header("hdr_inert"))
        .unwrap();
    let opt = &state.options[0];
    // PersistMode::None ⇒ excluded from network save, network load, the JSON
    // cache, and card-in session resets — nothing serializes, no wire field.
    assert!(!opt.persist.saved_to_network());
    assert!(!opt.persist.loaded_from_network());
    assert!(!opt.persist.json_cached());
    assert!(!opt.persist.session_scoped());
    assert!(state.reset_session_values(0).is_empty());
}

#[test]
fn headers_inject_no_preview_art() {
    let mut state = FrameworkState::default();
    state
        .try_register(RegisterSpec::header("hdr_preview"))
        .unwrap();
    assert!(state.preview_image_names_for("hdr_preview").is_empty());
    assert!(state.ribbon_texture_names_for("hdr_preview").is_empty());
    // The row label still follows the seop_item_<id> convention.
    assert_eq!(
        state.options[0].label_texture_name(),
        "seop_item_hdr_preview"
    );
}
