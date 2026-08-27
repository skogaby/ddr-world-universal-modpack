//! The public row-registration API (cabinet-wide rows mods contribute to the
//! overlay), the contributed-row store, and the edit paths (registry toggles
//! + contributed value updates).
//!
//! `MenuRow`/`RowKind` here are the REGISTRATION record types — the public,
//! frozen API surface (`ScalarRowSpec`/`EnumRowSpec` consumers). The display
//! model (`super::model`) has its own `RowKind`; `tabs.rs` converts.

use std::collections::HashMap;
use std::sync::Arc;

use crate::mods::mod_trait::ModInfo;

use super::{tabs, MOD_MENU_STATE};

pub type ToggleCallback = Arc<dyn Fn(&str, bool) + Send + Sync>;
pub type EntriesCallback = Arc<dyn Fn() -> Vec<ModInfo> + Send + Sync>;

/// Fired when a row's value changes (boolean toggle or scalar adjust). The
/// payload is the new value, with a boolean encoded as `0`/`1`.
pub type RowChangeCallback = Arc<dyn Fn(i32) + Send + Sync>;

/// Value type of a registered row.
pub enum RowKind {
    /// On/off row.
    Boolean { value: bool },
    /// Numeric row adjusted by left/right (fine) or Start-held left/right
    /// (coarse). Clamped to `[min, max]`.
    Scalar {
        value: i32,
        min: i32,
        max: i32,
        step_fine: i32,
        step_coarse: i32,
    },
    /// Labeled pick-list adjusted by left/right (cycle the selected entry,
    /// clamped at the ends — no wrap, matching `Scalar`). `index` selects into
    /// the parallel `values`/`labels` vectors: `values[index]` is the raw value
    /// passed to `on_change` (e.g. an FPS number), `labels[index]` is what the
    /// value column renders (e.g. `"144fps"`).
    Enum {
        index: usize,
        values: Vec<i32>,
        labels: Vec<String>,
    },
}

/// One registered (contributed) row. `visible_when` holds the OWNING MOD id
/// (historically "parent row key" — every registrant passes its mod id): on
/// the GLOBAL SETTINGS tab the row renders under that mod's section header
/// while the mod is enabled, and is hidden while it's disabled.
pub struct MenuRow {
    /// Stable row id (distinct from any registry mod id).
    pub key: String,
    pub label: String,
    /// Footer description while the row is selected.
    pub hint: String,
    pub kind: RowKind,
    /// Visual indent level (legacy field; the tabbed layout derives grouping
    /// from `visible_when` instead).
    pub indent: u8,
    /// `Some((owning_mod_id, 1))` — see the struct docs.
    pub visible_when: Option<(String, i32)>,
    /// Change callback driving the owning mod.
    pub on_change: Option<RowChangeCallback>,
}

// ── Public row-registration API (for mods to contribute rows) ───────

/// Declarative spec for a scalar row a mod contributes to the overlay.
/// `parent_row_key` is the registering mod's id (the row renders under that
/// mod's group on the GLOBAL SETTINGS tab while the mod is enabled).
/// `on_change(new_value)` fires on each adjustment.
pub struct ScalarRowSpec {
    /// Stable row id (distinct from any registry mod id).
    pub key: String,
    pub label: String,
    pub hint: String,
    /// Owning mod id (`None` = ungrouped: renders at the tail of GLOBAL
    /// SETTINGS, always visible).
    pub parent_row_key: Option<String>,
    pub min: i32,
    pub max: i32,
    pub step_fine: i32,
    pub step_coarse: i32,
    pub initial: i32,
    pub on_change: RowChangeCallback,
}

/// Declarative spec for an enum (labeled pick-list) row a mod contributes.
/// Like `ScalarRowSpec` but the value space is the discrete `values` list with
/// parallel `labels` (e.g. values `[60,120,144]`, labels `["60fps",...]`).
/// Left/Right cycles entries (clamped at the ends). `on_change(new_value)`
/// fires with the selected `values[index]`.
pub struct EnumRowSpec {
    /// Stable row id (distinct from any registry mod id).
    pub key: String,
    pub label: String,
    pub hint: String,
    /// Owning mod id (`None` = ungrouped; see `ScalarRowSpec`).
    pub parent_row_key: Option<String>,
    /// Discrete selectable values, caller-normalized (sorted, deduped).
    /// Parallel to `labels`.
    pub values: Vec<i32>,
    /// Display strings parallel to `values` (e.g. `"144fps"`).
    pub labels: Vec<String>,
    /// Initial selection as a raw value; resolved to the matching index (falls
    /// back to index 0 if not present in `values`).
    pub initial_value: i32,
    pub on_change: RowChangeCallback,
}

/// Register a scalar row contributed by a mod. Stored in `contributed_rows`;
/// tab lists rebuild from the store on each open/edit. Replaces any existing
/// contributed row with the same key (idempotent re-registration). Safe to
/// call before the menu service is otherwise used.
pub fn register_scalar_row(spec: ScalarRowSpec) {
    let row = MenuRow {
        key: spec.key,
        label: spec.label,
        hint: spec.hint,
        kind: RowKind::Scalar {
            value: spec.initial.clamp(spec.min, spec.max),
            min: spec.min,
            max: spec.max,
            step_fine: spec.step_fine,
            step_coarse: spec.step_coarse,
        },
        indent: 1,
        visible_when: spec.parent_row_key.map(|p| (p, 1)),
        on_change: Some(spec.on_change),
    };
    insert_contributed(row);
}

/// Register an enum (labeled pick-list) row contributed by a mod. Replaces
/// any existing contributed row with the same key (idempotent
/// re-registration). `values`/`labels` should be parallel and pre-normalized
/// by the caller; the initial value is resolved to its index (or 0 if absent).
pub fn register_enum_row(spec: EnumRowSpec) {
    let index = enum_index_of(&spec.values, spec.initial_value).unwrap_or(0);
    let row = MenuRow {
        key: spec.key,
        label: spec.label,
        hint: spec.hint,
        kind: RowKind::Enum {
            index,
            values: spec.values,
            labels: spec.labels,
        },
        indent: 1,
        visible_when: spec.parent_row_key.map(|p| (p, 1)),
        on_change: Some(spec.on_change),
    };
    insert_contributed(row);
}

fn insert_contributed(row: MenuRow) {
    let Ok(mut state) = MOD_MENU_STATE.lock() else {
        return;
    };
    if let Some(existing) = state.contributed_rows.iter_mut().find(|r| r.key == row.key) {
        *existing = row;
    } else {
        state.contributed_rows.push(row);
    }
}

/// Remove all contributed rows owned by a mod (matched by exact key
/// membership). Called from a mod's `disable()`. `keys` is the set of row
/// keys to drop.
pub fn remove_rows_for(keys: &[&str]) {
    let Ok(mut state) = MOD_MENU_STATE.lock() else {
        return;
    };
    state
        .contributed_rows
        .retain(|r| !keys.contains(&r.key.as_str()));
}

/// Index of `value` within `values`, or `None` if absent. Used to mirror an
/// externally-set enum value back into the row's `index`.
fn enum_index_of(values: &[i32], value: i32) -> Option<usize> {
    values.iter().position(|&v| v == value)
}

/// The change callback registered for a contributed row, by key.
pub(super) fn on_change_for(key: &str) -> Option<RowChangeCallback> {
    let state = MOD_MENU_STATE.lock().ok()?;
    state
        .contributed_rows
        .iter()
        .find(|r| r.key == key)
        .and_then(|r| r.on_change.clone())
}

/// Toggle a registry mod by id, persist the new enable map, and rebuild the
/// tab lists (the toggled mod's enable()/disable() may have registered or
/// removed contributed rows, and MODS/GLOBAL both render enable state).
pub(super) fn toggle_registry_mod(id: &str, enable: bool) {
    let (toggle_cb, entries_cb) = {
        let Ok(state) = MOD_MENU_STATE.lock() else {
            return;
        };
        (
            state.toggle_callback.clone(),
            state.entries_callback.clone(),
        )
    };
    let Some(toggle_cb) = toggle_cb else {
        return;
    };
    toggle_cb(id, enable);

    // Persist the full enable map from a fresh registry read.
    if let Some(entries_cb) = entries_cb {
        let config: HashMap<String, bool> = entries_cb()
            .into_iter()
            .filter(|e| e.id != "mod-menu")
            .map(|e| (e.id, e.enabled))
            .collect();
        crate::mods::config::save_mod_states(&config);
    }

    tabs::rebuild_and_refresh();
}

/// Write `value` into a registration record's kind-specific storage
/// (bool → !=0, scalar → raw, enum → mirrored to the matching index; an
/// out-of-list enum value leaves the index unchanged).
fn apply_row_value(kind: &mut RowKind, value: i32) {
    match kind {
        RowKind::Boolean { value: ref mut v } => *v = value != 0,
        RowKind::Scalar {
            value: ref mut v, ..
        } => *v = value,
        RowKind::Enum {
            ref mut index,
            ref values,
            ..
        } => {
            if let Some(i) = enum_index_of(values, value) {
                *index = i;
            }
        }
    }
}

/// Update a contributed row's stored value (by key) and rebuild/refresh the
/// display. The owning mod is authoritative — the caller fires `on_change`
/// first; this mirrors the value into the registration store so the next
/// rebuild (and the next open) renders it.
pub(super) fn set_row_value_and_refresh(key: &str, value: i32) {
    {
        let Ok(mut state) = MOD_MENU_STATE.lock() else {
            return;
        };
        if let Some(row) = state.contributed_rows.iter_mut().find(|r| r.key == key) {
            apply_row_value(&mut row.kind, value);
        }
    }
    tabs::rebuild_and_refresh();
}
