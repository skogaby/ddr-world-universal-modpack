//! Snapshot assembly + tab row-list rebuilds — the impure glue between the
//! registry / contributed-row registrations / custom_options overlay
//! snapshot and the pure `model` builders. Also owns the session-gating
//! adapter (`editable_sides_now`) the PLAYER SETTINGS tab and the marshaled
//! edit path share.

use crate::services::{custom_options, scene_manager, stage_records, widget_renderer};
use crate::types::scenes::{ATTRACT_SCENE_MAX, ATTRACT_SCENE_MIN};

use super::model::{self, ContributedSnap, MirroredKindSnap, MirroredRowSnap, ModEntrySnap, TabId};
use super::render;
use super::rows::RowKind as RegKind;
use super::{chrome_loader, theme, ModMenuState, MOD_MENU_STATE};

/// Which sides are currently editable (design §4.9): entered per
/// `stage_records::side_entered` (None ⇒ fail-closed) AND the scene outside
/// the attract/boot band. Scene unknown (manager unavailable / pre-first-
/// transition `-1`) counts as in-band — fail-closed.
pub(super) fn editable_sides_now() -> [bool; 2] {
    let entered = [
        stage_records::side_entered(0),
        stage_records::side_entered(1),
    ];
    let in_band = if scene_manager::is_available() {
        let scene = scene_manager::current_scene();
        scene < 0 || (ATTRACT_SCENE_MIN..=ATTRACT_SCENE_MAX).contains(&scene)
    } else {
        true
    };
    model::editable_sides(entered, in_band)
}

/// Rebuild every tab's display list from current registry + contributed +
/// mirrored-option state, then re-validate the active tab's cursor/scroll
/// against its new list.
///
/// Calls the registry `entries_callback` while holding the menu state lock —
/// the same ordering the old `rebuild_rows` used (menu lock → registry lock);
/// nothing takes the locks in the opposite order. The PLAYER tab's
/// `overlay_snapshot` takes the same registry lock internally (same pair).
pub(super) fn rebuild_tabs(state: &mut ModMenuState) {
    let entries: Vec<ModEntrySnap> = match state.entries_callback {
        Some(ref cb) => cb()
            .into_iter()
            .map(|e| ModEntrySnap {
                id: e.id,
                name: e.name,
                description: e.description,
                enabled: e.enabled,
            })
            .collect(),
        None => Vec::new(),
    };
    let contributed: Vec<ContributedSnap> = state
        .contributed_rows
        .iter()
        .map(|r| ContributedSnap {
            key: r.key.clone(),
            label: r.label.clone(),
            hint: r.hint.clone(),
            kind: convert_kind(&r.kind),
            owning_mod_id: r.visible_when.as_ref().map(|(parent, _)| parent.clone()),
        })
        .collect();

    // Player tab first (needs &mut state for the side resolve + framework
    // flag; the map below only borrows the locals).
    let mut player_rows = build_player_rows(state);

    state.tab_rows = TabId::ALL
        .iter()
        .map(|t| match t {
            TabId::Mods => model::build_mods_tab(&entries),
            TabId::GlobalSettings => model::build_global_tab(&entries, &contributed),
            TabId::PlayerSettings => std::mem::take(&mut player_rows),
            TabId::Theme => {
                let labels: Vec<String> = theme::THEMES
                    .iter()
                    .map(|t| t.display.to_string())
                    .collect();
                model::build_theme_tab(
                    chrome_loader::active_theme_index(),
                    &labels,
                    chrome_loader::animate_background(),
                    // Greyed when the ACTIVE theme has no live shader path
                    // (MINIMAL is Static by design; otherwise synthesis must
                    // have published the theme program indices — design §6).
                    !super::background_available(),
                    chrome_loader::effective_opacity(),
                )
            }
        })
        .collect();

    clamp_active(state);
}

/// Build the PLAYER SETTINGS row list: resolve the configured side against
/// the editable set, snapshot the framework's overlay-placed options for it,
/// and grey everything when the side has no active session (FR-4/FR-5).
fn build_player_rows(state: &mut ModMenuState) -> Vec<model::Row> {
    if !custom_options::is_available() {
        state.framework_unavailable = true;
        return Vec::new();
    }
    state.framework_unavailable = false;

    let editable = editable_sides_now();
    let side = model::resolve_selected_side(state.player_side, editable);
    state.player_side = side;
    state.player_editable = editable;

    let snaps: Vec<MirroredRowSnap> = custom_options::overlay_snapshot(side)
        .into_iter()
        .map(convert_overlay_row)
        .collect();
    model::build_player_tab(&snaps, editable[side as usize])
}

/// Framework snapshot row → model-local mirror (the model stays
/// dependency-free; this is the one conversion point).
fn convert_overlay_row(info: custom_options::OverlayRowInfo) -> MirroredRowSnap {
    use custom_options::OverlayRowKind as K;
    MirroredRowSnap {
        id: info.id,
        display_name: info.display_name,
        description: info.description,
        kind: match info.kind {
            K::Bool { value } => MirroredKindSnap::Bool { value },
            K::Enum {
                index,
                values,
                labels,
            } => MirroredKindSnap::Enum {
                index,
                values,
                labels,
            },
            K::Scalar {
                value,
                min,
                max,
                step_fine,
                step_coarse,
                formatted,
            } => MirroredKindSnap::Scalar {
                value,
                min,
                max,
                step_fine,
                step_coarse,
                formatted,
            },
            K::Header => MirroredKindSnap::Header,
        },
        visible: info.visible,
    }
}

/// Re-validate the ACTIVE tab's navigation against its (possibly rebuilt)
/// row list.
pub(super) fn clamp_active(state: &mut ModMenuState) {
    let tab = state.tab_nav.active();
    let tab_idx = tab.index();
    let mut nav = state.tab_nav.state();
    if let Some(rows) = state.tab_rows.get(tab_idx) {
        render::navigator_for(tab, rows).clamp_after_rebuild(&mut nav);
    }
    *state.tab_nav.state_mut() = nav;
}

/// Rebuild all tabs and schedule a repaint — the post-edit path (a toggle may
/// have registered/removed contributed rows; values changed).
pub(super) fn rebuild_and_refresh() {
    {
        let Ok(mut state) = MOD_MENU_STATE.lock() else {
            return;
        };
        if !state.is_open {
            return;
        }
        rebuild_tabs(&mut state);
    }
    widget_renderer::run_on_render_thread(|| {
        if let Ok(state) = MOD_MENU_STATE.lock() {
            render::refresh_all(&state);
        }
    });
}

/// Registration-record kind → display-model kind. Two types by design: the
/// registration API (`rows::RowKind`) is public/frozen; the model's kind is
/// the internal display contract.
fn convert_kind(k: &RegKind) -> model::RowKind {
    match k {
        RegKind::Boolean { value } => model::RowKind::Boolean { value: *value },
        RegKind::Scalar {
            value,
            min,
            max,
            step_fine,
            step_coarse,
        } => model::RowKind::Scalar {
            value: *value,
            min: *min,
            max: *max,
            step_fine: *step_fine,
            step_coarse: *step_coarse,
            // Contributed rows render the plain signed-integer text (the
            // formatted channel is the mirrored options' parity carrier).
            formatted: None,
        },
        RegKind::Enum {
            index,
            values,
            labels,
        } => model::RowKind::Enum {
            index: *index,
            values: values.clone(),
            labels: labels.clone(),
        },
    }
}
