//! Host tests for the row texture ALIASES (`RegisterSpec::label_texture_like` /
//! `preview_texture_like`, 2026-09-30): a registered row may render another
//! row's label texture (`seop_item_<alias>`) and preview chrome
//! (`seop_image_<alias>`) instead of the names derived from its own id, so N
//! runtime-named rows (the Background Dancers per-source model rows) share
//! one shipped label PNG and one chrome. Without an alias every name stays
//! id-derived.

use super::api::{RegisterSpec, ScalarFormat};
use super::registry::{FrameworkState, RegisteredOption};

fn registered(spec: RegisterSpec) -> (FrameworkState, usize) {
    let mut state = FrameworkState::default();
    let handle = state.try_register(spec).expect("test registration");
    (state, handle.0 as usize)
}

fn opt(state: &FrameworkState, idx: usize) -> &RegisteredOption {
    &state.options[idx]
}

#[test]
fn both_aliases_resolve() {
    let (state, i) = registered(
        RegisterSpec::scalar(
            "background_dancer_ddr_strike",
            0,
            3,
            1,
            ScalarFormat::Integer,
        )
        .label_texture_like("background_dancer")
        .preview_texture_like("background_dancer"),
    );
    let o = opt(&state, i);
    assert_eq!(o.label_texture_name(), "seop_item_background_dancer");
    assert_eq!(o.preview_image_base_name(), "seop_image_background_dancer");
    assert_eq!(
        o.preview_image_names(),
        vec!["seop_image_background_dancer".to_string()]
    );
    assert_eq!(
        o.preview_image_name_for_value(2),
        "seop_image_background_dancer"
    );
}

#[test]
fn no_alias_is_id_derived() {
    let (state, i) = registered(RegisterSpec::scalar(
        "plain_row",
        0,
        3,
        1,
        ScalarFormat::Integer,
    ));
    let o = opt(&state, i);
    assert_eq!(o.label_texture_name(), "seop_item_plain_row");
    assert_eq!(o.preview_image_base_name(), "seop_image_plain_row");
    assert_eq!(
        o.preview_image_names(),
        vec!["seop_image_plain_row".to_string()]
    );
}

#[test]
fn preview_alias_alone_leaves_label() {
    let (state, i) = registered(
        RegisterSpec::scalar("x", 0, 3, 1, ScalarFormat::Integer)
            .preview_texture_like("background_stage"),
    );
    let o = opt(&state, i);
    assert_eq!(o.label_texture_name(), "seop_item_x");
    assert_eq!(o.preview_image_base_name(), "seop_image_background_stage");
}

#[test]
fn label_alias_alone_leaves_preview() {
    let (state, i) = registered(
        RegisterSpec::scalar("y", 0, 3, 1, ScalarFormat::Integer).label_texture_like("shared"),
    );
    let o = opt(&state, i);
    assert_eq!(o.label_texture_name(), "seop_item_shared");
    assert_eq!(o.preview_image_base_name(), "seop_image_y");
}

#[test]
fn enum_with_preview_keys_uses_alias_base() {
    // Every bool_toggle value carries a preview key, so the per-value names
    // build on the alias base and no bare base is needed.
    let (state, i) = registered(RegisterSpec::bool_toggle("t").preview_texture_like("base"));
    let o = opt(&state, i);
    assert_eq!(
        o.preview_image_names(),
        vec![
            "seop_image_base_off".to_string(),
            "seop_image_base_on".to_string()
        ]
    );
    assert_eq!(o.preview_image_name_for_value(1), "seop_image_base_on");
    // The label is still the toggle's own.
    assert_eq!(o.label_texture_name(), "seop_item_t");
}
