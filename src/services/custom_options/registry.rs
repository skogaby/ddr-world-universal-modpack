//! Mutable framework state: the list of registered options and the per-player
//! value cache. Guarded by a single mutex ([`STATE`]).
//!
//! Write paths (registration, load-resolve, user-change) acquire the lock,
//! mutate, and release before invoking change callbacks — this keeps
//! callbacks free to call back into read APIs without deadlocking.
//!
//! A registered option's entry is stable once created: [`OptionHandle`]
//! indices into `options` never shift, so handles remain valid for the
//! lifetime of the process.

use once_cell::sync::Lazy;
use std::sync::Mutex;

use super::api::{OnChangeFn, OptionHandle, PersistMode, RegisterSpec, ShowWhen, UiKind};

/// All information the framework keeps about one registered option. Derived
/// from [`RegisterSpec`] at registration time; the only fields mutated
/// afterward (always under the [`STATE`] lock) are the per-side [`values`],
/// the injection [`available`] flag, and the panic-replacement of
/// `on_change`.
///
/// The [`values`] array holds the current value for each player side (index
/// 0 = P1, 1 = P2). It's primed from `default_value` at registration and
/// updated by user input, persistence load, and explicit writes.
#[derive(Debug)]
pub(crate) struct RegisteredOption {
    pub(crate) id: String,
    pub(crate) ui_kind: UiKind,
    pub(crate) on_change: OnChangeFn,
    pub(crate) show_when: ShowWhen,
    /// Per-player current value. `values[0]` = P1, `values[1]` = P2.
    pub(crate) values: [i32; 2],
    /// The registration-time default, kept so session-scoped options can be
    /// restored to it at card-in (see [`FrameworkState::reset_session_values`]).
    pub(crate) default_value: i32,
    /// How this option participates in the persistence layers (network
    /// save / network load / offline JSON cache). See [`PersistMode`].
    pub(crate) persist: PersistMode,
    /// Optional transform applied to the in-memory value before it's sent
    /// to the server. `None` means identity.
    pub(crate) save_transform: Option<fn(id: &str, value: i32) -> i32>,
    /// Optional transform applied to a value received from the server
    /// before it's written into the cache. `None` means identity.
    pub(crate) load_transform: Option<fn(id: &str, value: i32) -> i32>,
    /// Whether the builder hook injects this row at the next form open.
    /// Defaults true; flipped by `set_option_available` (under the STATE
    /// lock). Availability affects INJECTION ONLY — registration, handles,
    /// values, and persistence are untouched, and an already-open form is
    /// never mutated (rows are created only from the per-open snapshot).
    pub(crate) available: bool,
    /// Registered menu placement (in-game / overlay). Consumers resolve the
    /// effective placement as config-override-wins:
    /// `ordering::placement_override_for(id)` legs replace these defaults.
    pub(crate) menus: super::api::MenuPlacement,
    /// Optional human-readable label for text-rendering menus (the overlay);
    /// `None` falls back to a prettified id at snapshot time.
    pub(crate) display_name: Option<&'static str>,
    /// Optional overlay footer description; `None` falls back to empty.
    pub(crate) description: Option<&'static str>,
}

impl RegisteredOption {
    /// Row-label texture name derived from the option id. Matches the
    /// convention enforced at `register_label_for` time; the PNG must
    /// exist at `data_mods/custom_options/.../tex/seop_item_<id>.png`.
    pub(crate) fn label_texture_name(&self) -> String {
        format!("seop_item_{}", self.id)
    }

    /// Base preview-image texture name for this option (`seop_image_<id>`).
    /// Used as the fallback for enum values without a `preview_key`, and as
    /// the sole preview for scalar/boolean rows. The mod ships the matching
    /// PNG via LayeredFS into the options IFS, same atlas-injection path as
    /// the row labels. See `docs/option_preview_image_box.md`.
    pub(crate) fn preview_image_base_name(&self) -> String {
        format!("seop_image_{}", self.id)
    }

    /// Preview-image texture name to show when value `value` is selected.
    /// For an enum value carrying a `preview_key`, this is
    /// `seop_image_<id>_<key>`; otherwise it falls back to the base name.
    /// Scalar options always return the base name (their preview is fixed,
    /// matching native scalar rows).
    pub(crate) fn preview_image_name_for_value(&self, value: i32) -> String {
        if let UiKind::Enum { allowed_values } = &self.ui_kind {
            if let Some(key) = allowed_values
                .iter()
                .find(|v| v.value == value)
                .and_then(|v| v.preview_key.as_deref())
            {
                return format!("seop_image_{}_{}", self.id, key);
            }
        }
        self.preview_image_base_name()
    }

    /// Every distinct preview-image texture name this option can display —
    /// the per-value `seop_image_<id>_<key>` names for enum values that
    /// carry a `preview_key`, plus the base `seop_image_<id>` whenever any
    /// value falls back to it (or the option is scalar/boolean). Drives
    /// atlas injection so each shipped PNG gets a cloned slot. Deduplicated,
    /// order-stable (base first).
    pub(crate) fn preview_image_names(&self) -> Vec<String> {
        // Headers are display-only and can never be the focused row — no
        // preview panel exists for them, so nothing enters the atlas.
        if matches!(self.ui_kind, UiKind::Header) {
            return Vec::new();
        }
        let mut names = Vec::new();
        let base = self.preview_image_base_name();
        let mut needs_base = true;

        if let UiKind::Enum { allowed_values } = &self.ui_kind {
            // The base is needed only if at least one value lacks a key.
            needs_base = allowed_values.iter().any(|v| v.preview_key.is_none());
            for v in allowed_values {
                if let Some(key) = v.preview_key.as_deref() {
                    let name = format!("seop_image_{}_{}", self.id, key);
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
            }
        }

        if needs_base {
            names.insert(0, base);
        }
        names
    }
}

/// Framework state singleton.
#[derive(Default)]
pub(crate) struct FrameworkState {
    /// Registered options. An [`OptionHandle`]'s internal index refers
    /// directly into this vector; entries are append-only, never reordered.
    pub(crate) options: Vec<RegisteredOption>,
}

impl FrameworkState {
    /// Look up an option's index by its id.
    pub(crate) fn index_of(&self, id: &str) -> Option<usize> {
        self.options.iter().position(|o| o.id == id)
    }

    /// Core registration logic. Performs all validation and, on success,
    /// appends the new `RegisteredOption` and returns its handle. Does NOT
    /// fire the initial change callback — the caller is responsible for
    /// that (after dropping the lock) so callbacks can't re-enter the state.
    ///
    /// Validation order (first failure wins):
    ///   1. Duplicate id                -> Duplicate
    ///   2. Header carrying state       -> HeaderCarriesState
    ///   3. ShowWhen::Equals parent     -> UnknownParent if not registered
    pub(crate) fn try_register(
        &mut self,
        spec: RegisterSpec,
    ) -> Result<OptionHandle, super::api::RegisterError> {
        use super::api::RegisterError;

        if self.index_of(spec.id).is_some() {
            return Err(RegisterError::Duplicate {
                id: spec.id.to_string(),
            });
        }
        // Headers are stateless display-only rows (design §4.8): refuse a
        // header spec carrying anything that implies state. Persistence
        // surfaces (network save/load, JSON cache, session reset) all key
        // off PersistMode, so `None` — checked here — keeps a registered
        // header inert on every one of them without further special-casing.
        // Display strings and menu placement are deliberately ALLOWED on
        // headers (overlay-rewrite Step 5 decision): they carry no state —
        // headers render as section separators in the overlay, and the
        // operator may place/hide a header per menu like any row.
        if matches!(spec.ui_kind, UiKind::Header) {
            let offending = if spec.persist != PersistMode::None {
                Some("persistence")
            } else if !super::api::is_default_on_change(spec.on_change) {
                Some("a change callback")
            } else if !matches!(spec.show_when, ShowWhen::Always) {
                Some("a parent/child link")
            } else if spec.save_transform.is_some() || spec.load_transform.is_some() {
                Some("persist transforms")
            } else if spec.default_value != 0 {
                Some("a value")
            } else {
                None
            };
            if let Some(what) = offending {
                return Err(RegisterError::HeaderCarriesState {
                    id: spec.id.to_string(),
                    what,
                });
            }
        }
        if let ShowWhen::Equals { parent_id, .. } | ShowWhen::NotEquals { parent_id, .. } =
            &spec.show_when
        {
            if self.index_of(parent_id).is_none() {
                return Err(RegisterError::UnknownParent {
                    id: spec.id.to_string(),
                    parent_id: parent_id.clone(),
                });
            }
        }

        let handle = OptionHandle(self.options.len() as u32);
        self.options.push(RegisteredOption {
            id: spec.id.to_string(),
            ui_kind: spec.ui_kind,
            on_change: spec.on_change,
            show_when: spec.show_when,
            values: [spec.default_value; 2],
            default_value: spec.default_value,
            persist: spec.persist,
            save_transform: spec.save_transform,
            load_transform: spec.load_transform,
            available: true,
            menus: spec.menus,
            display_name: spec.display_name,
            description: spec.description,
        });
        Ok(handle)
    }

    /// Flip an option's injection availability. Returns `false` (no-op)
    /// when the id isn't registered. Visibility changes apply at the next
    /// form rebuild — the builder hook filters its per-open snapshot.
    pub(crate) fn set_available(&mut self, id: &str, available: bool) -> bool {
        match self.index_of(id) {
            Some(index) => {
                self.options[index].available = available;
                true
            }
            None => false,
        }
    }

    /// Write a new value into an option's per-player cache. Returns the
    /// `OnChangeFn` + (side, value) the caller should invoke *after*
    /// releasing the lock.
    ///
    /// Returns `None` if the id isn't registered or `side >= 2`.
    pub(crate) fn set_value(
        &mut self,
        id: &str,
        side: u8,
        value: i32,
    ) -> Option<(OnChangeFn, u8, i32)> {
        if side >= 2 {
            return None;
        }
        let idx = self.index_of(id)?;
        let opt = &mut self.options[idx];
        opt.values[side as usize] = value;
        Some((opt.on_change, side, value))
    }

    /// Card-in reset: restore every session-scoped option
    /// ([`PersistMode::session_scoped`]) to its `default_value` for the
    /// carded-in `side`. Options already at their default are untouched (a
    /// no-op reset dispatches no callback). Returns `(id, OnChangeFn, side,
    /// default)` tuples the caller must invoke *after* releasing the lock —
    /// the same deferred-dispatch contract as [`set_value`](Self::set_value);
    /// the id rides along so the dispatcher's panic-suppression can find the
    /// option.
    ///
    /// `side >= 2` is a safe no-op (the facade passes raw sides through).
    pub(crate) fn reset_session_values(&mut self, side: u8) -> Vec<(String, OnChangeFn, u8, i32)> {
        if side >= 2 {
            return Vec::new();
        }
        let mut callbacks = Vec::new();
        for opt in &mut self.options {
            if !opt.persist.session_scoped() {
                continue;
            }
            if opt.values[side as usize] == opt.default_value {
                continue;
            }
            opt.values[side as usize] = opt.default_value;
            callbacks.push((opt.id.clone(), opt.on_change, side, opt.default_value));
        }
        callbacks
    }

    /// Re-bound a scalar option's `min`/`max` at runtime (Training Mode's
    /// per-song row ranges: the stepper must not even be able to express a
    /// value past the highlighted song's length). Every bounds consumer —
    /// the press-time stepping clamp, the row's position-marker fraction —
    /// reads the registry live, so the new range is effective the same
    /// frame. Stored values outside the new range are clamped into it;
    /// the returned `(id, OnChangeFn, side, clamped)` tuples follow the
    /// deferred-dispatch contract ([`set_value`](Self::set_value)).
    ///
    /// `None` (nothing touched) for unknown ids, non-scalar rows, or
    /// inverted bounds (`min > max`).
    pub(crate) fn set_scalar_bounds(
        &mut self,
        id: &str,
        new_min: i32,
        new_max: i32,
    ) -> Option<Vec<(String, OnChangeFn, u8, i32)>> {
        if new_min > new_max {
            return None;
        }
        let idx = self.index_of(id)?;
        let opt = &mut self.options[idx];
        let UiKind::Scalar {
            ref mut min,
            ref mut max,
            ..
        } = opt.ui_kind
        else {
            return None;
        };
        *min = new_min;
        *max = new_max;
        let mut callbacks = Vec::new();
        for side in 0..2u8 {
            let current = opt.values[side as usize];
            let clamped = current.clamp(new_min, new_max);
            if clamped != current {
                opt.values[side as usize] = clamped;
                callbacks.push((opt.id.clone(), opt.on_change, side, clamped));
            }
        }
        Some(callbacks)
    }

    /// Every preview-image texture name option `id` can display
    /// (`seop_image_<id>` plus any per-value `seop_image_<id>_<key>`).
    /// Returns an empty vec if the id isn't registered. Used at registration
    /// time to drive atlas injection.
    pub(crate) fn preview_image_names_for(&self, id: &str) -> Vec<String> {
        match self.index_of(id) {
            Some(i) => self.options[i].preview_image_names(),
            None => Vec::new(),
        }
    }

    /// Every value-ribbon texture name option `id` uses
    /// (`EnumValue::label_texture_name` for each enum value). Empty for scalar
    /// options (their value renders as digits, not a ribbon sprite) and for
    /// unregistered ids. Deduplicated, order-stable. Drives ribbon atlas
    /// injection; stock names are filtered downstream in `asset_gen`.
    pub(crate) fn ribbon_texture_names_for(&self, id: &str) -> Vec<String> {
        let mut names = Vec::new();
        if let Some(i) = self.index_of(id) {
            if let UiKind::Enum { allowed_values } = &self.options[i].ui_kind {
                for v in allowed_values {
                    if !names.contains(&v.label_texture_name) {
                        names.push(v.label_texture_name.clone());
                    }
                }
            }
        }
        names
    }

    /// Read an option's current value for `side`. Returns `None` if the id
    /// isn't registered or `side >= 2`.
    pub(crate) fn get_value(&self, id: &str, side: u8) -> Option<i32> {
        if side >= 2 {
            return None;
        }
        self.index_of(id)
            .map(|i| self.options[i].values[side as usize])
    }

    /// Whether an option's [`ShowWhen`] predicate is currently satisfied for
    /// `side`. Out-of-range handles and unknown parents are fail-open
    /// (visible). Shared by the in-game rows (scroll-mask filtering) and the
    /// overlay snapshot's `visible` flag so both menus agree.
    pub(crate) fn show_when_satisfied(&self, handle: OptionHandle, side: u8) -> bool {
        let idx = handle.0 as usize;
        if idx >= self.options.len() {
            return true;
        }
        match &self.options[idx].show_when {
            ShowWhen::Always => true,
            ShowWhen::Equals { parent_id, value } => match self.index_of(parent_id) {
                Some(parent_idx) => self.options[parent_idx].values[side as usize] == *value,
                None => true,
            },
            ShowWhen::NotEquals { parent_id, value } => match self.index_of(parent_id) {
                Some(parent_idx) => self.options[parent_idx].values[side as usize] != *value,
                None => true,
            },
        }
    }
}

pub(crate) static STATE: Lazy<Mutex<FrameworkState>> =
    Lazy::new(|| Mutex::new(FrameworkState::default()));

// ── Overlay snapshot (overlay-menu rewrite design §4.3.4) ────────────

/// Plain-data row kind for the overlay menu's mirror. `Bool` is detected
/// from the exact `bool_toggle` shape (two values 0/1 with the stock
/// `seop_op_off`/`seop_op_on` textures); every other enum stays `Enum`.
#[derive(Debug, Clone, PartialEq)]
pub enum OverlayRowKind {
    Bool {
        value: bool,
    },
    Enum {
        /// Index of the current value within `values`/`labels` (0 when the
        /// stored value is somehow not in the list — fail-open).
        index: usize,
        values: Vec<i32>,
        labels: Vec<String>,
    },
    Scalar {
        value: i32,
        min: i32,
        max: i32,
        step_fine: i32,
        step_coarse: i32,
        /// Display text identical to the in-game row (modulo the SJIS `±` →
        /// UTF-8 hop) — from [`super::api::format_scalar_value_utf8`].
        formatted: String,
    },
    Header,
}

/// One overlay-menu row: plain data, no handles, no callbacks. Built under
/// the STATE lock by [`overlay_snapshot_rows`]; consumed lock-free by the
/// overlay's tab builder.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayRowInfo {
    pub id: String,
    /// Explicit `display_name` else the prettified id.
    pub display_name: String,
    /// Explicit `description` else empty.
    pub description: String,
    pub kind: OverlayRowKind,
    /// Per-side ShowWhen evaluation — REPORTED, not filtered (the overlay
    /// hides/greys per its own rules).
    pub visible: bool,
}

/// Whether an enum shape is the `bool_toggle` signature.
fn is_bool_toggle_shape(allowed_values: &[super::api::EnumValue]) -> bool {
    allowed_values.len() == 2
        && allowed_values[0].value == 0
        && allowed_values[1].value == 1
        && allowed_values[0].label_texture_name == "seop_op_off"
        && allowed_values[1].label_texture_name == "seop_op_on"
}

/// Build one option's overlay row for `side`. Pure vs `&FrameworkState`.
fn overlay_row(state: &FrameworkState, idx: usize, side: u8) -> OverlayRowInfo {
    use super::api::{format_scalar_value_utf8, prettify_id, prettify_texture_suffix};

    let opt = &state.options[idx];
    let value = opt.values[(side as usize).min(1)];
    let kind = match &opt.ui_kind {
        UiKind::Header => OverlayRowKind::Header,
        UiKind::Enum { allowed_values } if is_bool_toggle_shape(allowed_values) => {
            OverlayRowKind::Bool { value: value == 1 }
        }
        UiKind::Enum { allowed_values } => {
            let values: Vec<i32> = allowed_values.iter().map(|v| v.value).collect();
            let labels: Vec<String> = allowed_values
                .iter()
                .map(|v| match &v.display_label {
                    Some(label) => label.clone(),
                    None => prettify_texture_suffix(&v.label_texture_name),
                })
                .collect();
            let index = values.iter().position(|v| *v == value).unwrap_or(0);
            OverlayRowKind::Enum {
                index,
                values,
                labels,
            }
        }
        UiKind::Scalar {
            min,
            max,
            step_fine,
            step_coarse,
            format,
        } => OverlayRowKind::Scalar {
            value,
            min: *min,
            max: *max,
            step_fine: *step_fine,
            step_coarse: *step_coarse,
            formatted: format_scalar_value_utf8(value, *format),
        },
    };

    OverlayRowInfo {
        id: opt.id.clone(),
        display_name: match opt.display_name {
            Some(name) => name.to_string(),
            None => prettify_id(&opt.id),
        },
        description: opt.description.unwrap_or("").to_string(),
        kind,
        visible: state.show_when_satisfied(OptionHandle(idx as u32), side),
    }
}

/// Compose the overlay snapshot: candidates = options passing availability
/// AND resolved overlay placement (`overlay_override(id)` — the operator's
/// config — wins over the registered `menus.overlay`), ordered by
/// `order_for` over the candidates' (ids, header-mask) — the SAME
/// composition shape as the builder hook's in-game path, so both menus
/// honor `option_menu_settings` identically. Pure: the ordering/placement
/// closures are injected (the runtime facade passes `ordering`'s real
/// queries; tests pass synthetics).
pub(crate) fn overlay_snapshot_rows(
    state: &FrameworkState,
    side: u8,
    overlay_override: &dyn Fn(&str) -> Option<bool>,
    order_for: &dyn Fn(&[&str], &[bool]) -> Vec<usize>,
) -> Vec<OverlayRowInfo> {
    let candidates: Vec<usize> = state
        .options
        .iter()
        .enumerate()
        .filter(|(_, opt)| {
            let overlay = overlay_override(&opt.id).unwrap_or(opt.menus.overlay);
            opt.available && overlay
        })
        .map(|(i, _)| i)
        .collect();

    let ids: Vec<&str> = candidates
        .iter()
        .map(|&i| state.options[i].id.as_str())
        .collect();
    let is_header: Vec<bool> = candidates
        .iter()
        .map(|&i| matches!(state.options[i].ui_kind, UiKind::Header))
        .collect();

    order_for(&ids, &is_header)
        .into_iter()
        .filter_map(|display_idx| candidates.get(display_idx).copied())
        .map(|option_idx| overlay_row(state, option_idx, side))
        .collect()
}

#[cfg(test)]
mod overlay_snapshot_tests {
    use super::super::api::{
        format_scalar_value_utf8, EnumValue, RegisterSpec, ScalarFormat, ShowWhen,
    };
    use super::*;

    /// Identity order over non-header candidates + listed headers — the
    /// unconfigured `display_order_for` semantics, reimplemented for test
    /// injection (the real ordering module is exercised by its own tests).
    fn identity_order(ids: &[&str], is_header: &[bool]) -> Vec<usize> {
        (0..ids.len())
            .filter(|&i| !is_header.get(i).copied().unwrap_or(false))
            .collect()
    }

    fn no_override(_: &str) -> Option<bool> {
        None
    }

    fn register(state: &mut FrameworkState, spec: RegisterSpec) {
        state.try_register(spec).expect("test registration");
    }

    fn snapshot(state: &FrameworkState, side: u8) -> Vec<OverlayRowInfo> {
        overlay_snapshot_rows(state, side, &no_override, &identity_order)
    }

    fn ids(rows: &[OverlayRowInfo]) -> Vec<&str> {
        rows.iter().map(|r| r.id.as_str()).collect()
    }

    // ── Filtering / ordering ─────────────────────────────────────────

    #[test]
    fn availability_and_overlay_placement_drop_rows() {
        let mut state = FrameworkState::default();
        register(&mut state, RegisterSpec::bool_toggle("kept"));
        register(&mut state, RegisterSpec::bool_toggle("unavailable"));
        register(
            &mut state,
            RegisterSpec::bool_toggle("in_game_only").in_game_only(),
        );
        state.set_available("unavailable", false);

        assert_eq!(ids(&snapshot(&state, 0)), vec!["kept"]);
    }

    #[test]
    fn config_override_beats_registration_both_directions() {
        let mut state = FrameworkState::default();
        register(&mut state, RegisterSpec::bool_toggle("reg_on_cfg_off"));
        register(
            &mut state,
            RegisterSpec::bool_toggle("reg_off_cfg_on").in_game_only(),
        );

        let over = |id: &str| -> Option<bool> {
            match id {
                "reg_on_cfg_off" => Some(false),
                "reg_off_cfg_on" => Some(true),
                _ => None,
            }
        };
        let rows = overlay_snapshot_rows(&state, 0, &over, &identity_order);
        assert_eq!(ids(&rows), vec!["reg_off_cfg_on"]);
    }

    #[test]
    fn order_permutation_is_honored() {
        let mut state = FrameworkState::default();
        register(&mut state, RegisterSpec::bool_toggle("a"));
        register(&mut state, RegisterSpec::bool_toggle("b"));
        register(&mut state, RegisterSpec::bool_toggle("c"));

        let reversed = |ids: &[&str], _h: &[bool]| -> Vec<usize> { (0..ids.len()).rev().collect() };
        let rows = overlay_snapshot_rows(&state, 0, &no_override, &reversed);
        assert_eq!(ids(&rows), vec!["c", "b", "a"]);
    }

    // ── Kinds ────────────────────────────────────────────────────────

    #[test]
    fn bool_toggle_detected_custom_two_value_enum_stays_enum() {
        let mut state = FrameworkState::default();
        register(&mut state, RegisterSpec::bool_toggle("toggle"));
        register(
            &mut state,
            RegisterSpec::enum_values(
                "custom",
                vec![
                    EnumValue::new(0, "seop_op_off"),
                    EnumValue::new(1, "seop_op_left"),
                ],
            ),
        );
        let rows = snapshot(&state, 0);
        assert_eq!(rows[0].kind, OverlayRowKind::Bool { value: false });
        assert!(matches!(rows[1].kind, OverlayRowKind::Enum { .. }));
    }

    #[test]
    fn enum_labels_fall_back_to_prettified_texture_suffix() {
        let mut state = FrameworkState::default();
        register(
            &mut state,
            RegisterSpec::enum_values(
                "mode",
                vec![
                    EnumValue::new(0, "seop_op_dark"),
                    EnumValue::with_display(1, "seop_op_x", "Custom Label"),
                ],
            ),
        );
        let rows = snapshot(&state, 0);
        let OverlayRowKind::Enum {
            index,
            values,
            labels,
        } = &rows[0].kind
        else {
            panic!("expected enum");
        };
        assert_eq!(*index, 0);
        assert_eq!(values, &vec![0, 1]);
        assert_eq!(
            labels,
            &vec!["Dark".to_string(), "Custom Label".to_string()]
        );
    }

    #[test]
    fn scalar_carries_live_bounds_and_formatted_parity() {
        let mut state = FrameworkState::default();
        register(
            &mut state,
            RegisterSpec::scalar("speed", 25, 175, 5, ScalarFormat::Unit { unit: "%" })
                .step_coarse(10)
                .default_value(100),
        );
        // Live re-bounding (the Training Mode pattern) must be reflected.
        let clamped = state.set_scalar_bounds("speed", 50, 90).expect("bounds");
        assert_eq!(clamped.len(), 2, "both sides clamped 100 -> 90");

        let rows = snapshot(&state, 0);
        let OverlayRowKind::Scalar {
            value,
            min,
            max,
            step_fine,
            step_coarse,
            formatted,
        } = &rows[0].kind
        else {
            panic!("expected scalar");
        };
        assert_eq!(
            (*value, *min, *max, *step_fine, *step_coarse),
            (90, 50, 90, 5, 10)
        );
        assert_eq!(
            formatted,
            &format_scalar_value_utf8(90, ScalarFormat::Unit { unit: "%" })
        );
        assert_eq!(formatted, "90%");
    }

    #[test]
    fn formatted_parity_across_all_variants() {
        let cases: Vec<(ScalarFormat, i32)> = vec![
            (ScalarFormat::Integer, 490),
            (ScalarFormat::FixedPoint { decimals: 2 }, 150),
            (ScalarFormat::OffsetInteger { display_offset: 1 }, 2),
            (ScalarFormat::SignedUnit { unit: "ms" }, 0),
            (ScalarFormat::Unit { unit: "kg" }, 70),
            (ScalarFormat::MinutesSeconds, 90),
            (
                ScalarFormat::PrefixedIndex {
                    prefix: "Char #",
                    display_offset: 1,
                },
                2,
            ),
        ];
        for (format, value) in cases {
            let mut state = FrameworkState::default();
            register(
                &mut state,
                RegisterSpec::scalar("s", -100, 500, 1, format).default_value(value),
            );
            let rows = snapshot(&state, 0);
            let OverlayRowKind::Scalar { formatted, .. } = &rows[0].kind else {
                panic!("expected scalar");
            };
            assert_eq!(
                formatted,
                &format_scalar_value_utf8(value, format),
                "{format:?}"
            );
        }
    }

    // ── Display strings / visibility ─────────────────────────────────

    #[test]
    fn display_string_fallbacks_and_passthrough() {
        let mut state = FrameworkState::default();
        register(&mut state, RegisterSpec::bool_toggle("song_speed"));
        register(
            &mut state,
            RegisterSpec::bool_toggle("explicit")
                .display_name("Premium Free")
                .description("Unlimited songs"),
        );
        let rows = snapshot(&state, 0);
        assert_eq!(rows[0].display_name, "Song Speed");
        assert_eq!(rows[0].description, "");
        assert_eq!(rows[1].display_name, "Premium Free");
        assert_eq!(rows[1].description, "Unlimited songs");
    }

    #[test]
    fn header_row_included_by_injected_order_and_kind_mapped() {
        let mut state = FrameworkState::default();
        register(&mut state, RegisterSpec::header("header_training"));
        register(&mut state, RegisterSpec::bool_toggle("a"));
        // Injected order that LISTS the header (identity_order would drop it).
        let listed = |ids: &[&str], _h: &[bool]| -> Vec<usize> { (0..ids.len()).collect() };
        let rows = overlay_snapshot_rows(&state, 0, &no_override, &listed);
        assert_eq!(rows[0].kind, OverlayRowKind::Header);
        assert_eq!(rows[0].display_name, "Header Training");
    }

    #[test]
    fn show_when_reported_per_side_not_filtered() {
        let mut state = FrameworkState::default();
        register(&mut state, RegisterSpec::bool_toggle("parent"));
        register(
            &mut state,
            RegisterSpec::bool_toggle("child").show_when(ShowWhen::Equals {
                parent_id: "parent".to_string(),
                value: 1,
            }),
        );
        // Parent ON for P1 only.
        let _ = state.set_value("parent", 0, 1);

        let p1 = snapshot(&state, 0);
        let p2 = snapshot(&state, 1);
        assert_eq!(ids(&p1), vec!["parent", "child"], "reported, not filtered");
        assert!(p1[1].visible, "P1 child visible (parent ON)");
        assert!(!p2[1].visible, "P2 child hidden (parent OFF)");
    }

    #[test]
    fn unknown_parent_fails_open_visible() {
        // Registered THEN parent made unavailable is still resolvable; the
        // fail-open leg needs a dangling reference — construct via a parent
        // that never got registered (bypass try_register's validation by
        // registering the parent, snapshotting the child's show_when, then
        // testing against a fresh state through show_when_satisfied).
        let mut state = FrameworkState::default();
        register(
            &mut state,
            RegisterSpec::bool_toggle("orphan"), // stand-in row at idx 0
        );
        // Manually point its show_when at a parent that doesn't exist.
        state.options[0].show_when = ShowWhen::Equals {
            parent_id: "ghost".to_string(),
            value: 1,
        };
        let rows = snapshot(&state, 0);
        assert!(rows[0].visible, "unknown parent must fail open");
    }

    // ── Mutation primitives return the observer feed ─────────────────

    #[test]
    fn mutation_primitives_produce_changed_only_tuples() {
        let mut state = FrameworkState::default();
        register(&mut state, RegisterSpec::bool_toggle("t"));
        register(
            &mut state,
            RegisterSpec::scalar("s", 0, 100, 1, ScalarFormat::Integer).default_value(50),
        );
        register(
            &mut state,
            RegisterSpec::bool_toggle("sess").persist_mode(super::super::api::PersistMode::Session),
        );

        // set_value primitive is UNCONDITIONAL (returns Some for any
        // registered id — the facades dedupe before observer dispatch);
        // reset/bounds primitives are changed-only by construction.
        assert!(state.set_value("t", 0, 1).is_some());
        assert!(
            state.set_value("t", 0, 1).is_some(),
            "primitive is unconditional — facades own the no-change gate"
        );

        // reset_session_values: only Session rows off-default.
        let _ = state.set_value("sess", 0, 1);
        let resets = state.reset_session_values(0);
        assert_eq!(resets.len(), 1);
        assert_eq!(resets[0].0, "sess");
        assert_eq!(resets[0].3, 0, "restored to default");
        assert!(
            state.reset_session_values(0).is_empty(),
            "already at default"
        );

        // set_scalar_bounds: tuples only for clamped sides.
        let _ = state.set_value("s", 1, 90);
        let clamped = state.set_scalar_bounds("s", 0, 60).expect("bounds ok");
        assert_eq!(
            clamped.len(),
            1,
            "side 0's 50 is in range; only side 1's 90 clamps"
        );
        assert_eq!(
            (clamped[0].0.as_str(), clamped[0].2, clamped[0].3),
            ("s", 1, 60)
        );
    }
}
