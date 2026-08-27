//! Public API types for the custom options framework.
//!
//! Mods call [`register_option`](super::register_option) from within their
//! `enable()` with a [`RegisterSpec`] describing the option they want to add.
//! On success they receive an [`OptionHandle`] they can use to read the
//! option's current per-player value later.
//!
//! Error cases ([`RegisterError`]) are all graceful: the mod continues, the
//! option simply doesn't appear in the UI.

/// Opaque handle returned by a successful registration. Holds a registry
/// index internally; the representation is not part of the public contract.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct OptionHandle(pub(super) u32);

/// Which menus a row appears in: the game's native in-game options menu
/// (MODS tab) and/or the DLL's overlay mod menu (PLAYER SETTINGS tab).
/// Registration default is BOTH; the operator's
/// `custom_options.option_menu_settings` entries override per menu at read
/// time (config wins — see `ordering::placement_override_for`). Both `false`
/// = the row exists (values, persistence, handles all live) but renders
/// nowhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuPlacement {
    /// Injected into the game's native options menu.
    pub in_game: bool,
    /// Mirrored into the overlay menu's PLAYER SETTINGS tab.
    pub overlay: bool,
}

impl Default for MenuPlacement {
    fn default() -> Self {
        Self {
            in_game: true,
            overlay: true,
        }
    }
}

/// Which UI template a row uses.
#[derive(Debug, Clone)]
pub enum UiKind {
    /// Enum row: left/right cycles through a fixed list of labeled values.
    /// Uses the `option_item` AFP template.
    Enum { allowed_values: Vec<EnumValue> },

    /// Scalar row: numeric input within `[min, max]`. Uses the
    /// `OptionElement<int>` donor and renders the current value as text
    /// through the game's native digit-sprite compositor (`seop_num_*`).
    ///
    /// Two step sizes mirror the native scalar-option behavior: `step_fine`
    /// on a plain left/right press, `step_coarse` when Start is held
    /// simultaneously. Set them equal to disable the coarse/fine
    /// distinction.
    Scalar {
        min: i32,
        max: i32,
        step_fine: i32,
        step_coarse: i32,
        format: ScalarFormat,
    },

    /// Header row: a non-selectable, display-only group heading (design
    /// §4.8). Rendered + laid out like any row but skipped by every cursor
    /// path (the engine's own gray-row mechanism — the row's `+0x28`
    /// selectability interface is swapped for a mod-owned `{return 0, no-op}`
    /// table), and drawn as a full-width label only (no value box, marker,
    /// tri-arrows, or preview).
    ///
    /// Headers hold NO state: no values, no persistence, no callbacks, no
    /// parent/child links — the registrar refuses a header spec carrying any
    /// of those ([`RegisterError::HeaderCarriesState`]). Construct via
    /// [`RegisterSpec::header`]. Per R10 a header row is injected only when
    /// its id appears in the operator's `custom_options.option_menu_settings`;
    /// an unlisted header is absent entirely (see `ordering.rs`).
    Header,
}

/// One entry in a [`UiKind::Enum`]'s allowed-values list. `label_texture_name`
/// is the bare asset name (no extension); the game composes it into its
/// texture-atlas lookup at row-render time. Konami's stock value-ribbon
/// sprites under the `seop_op_*` prefix (e.g. `"seop_op_on"`, `"seop_op_off"`,
/// `"seop_op_normal"`, `"seop_op_dark"`) are always available; for bespoke
/// labels, mods ship `seop_op_<name>.png` via LayeredFS.
#[derive(Debug, Clone)]
pub struct EnumValue {
    pub value: i32,
    pub label_texture_name: String,
    /// Optional suffix keying this value's preview-box image. When `Some(k)`,
    /// selecting this value shows `seop_image_<option_id>_<k>` in the options
    /// preview box (mirroring Konami's per-value naming, e.g.
    /// `seop_image_lanecover_hidden`); the mod ships
    /// `seop_image_<option_id>_<k>.png` via LayeredFS. When `None`, this value
    /// falls back to the option's single `seop_image_<option_id>` preview.
    /// See `docs/option_preview_image_box.md`.
    pub preview_key: Option<String>,
    /// Optional human-readable label for text-rendering menus (the overlay's
    /// PLAYER SETTINGS tab). `None` falls back to a prettified
    /// `label_texture_name` suffix (see [`prettify_texture_suffix`]). The
    /// in-game menu ignores this — it renders the texture.
    pub display_label: Option<String>,
}

impl EnumValue {
    /// Construct an enum value with no per-value preview image (falls back to
    /// the option's single `seop_image_<id>`). Convenience for the common
    /// case so callers don't have to write `preview_key: None`.
    pub fn new(value: i32, label_texture_name: impl Into<String>) -> Self {
        Self {
            value,
            label_texture_name: label_texture_name.into(),
            preview_key: None,
            display_label: None,
        }
    }

    /// Construct an enum value whose selection shows a distinct preview image
    /// `seop_image_<id>_<preview_key>`.
    pub fn with_preview(
        value: i32,
        label_texture_name: impl Into<String>,
        preview_key: impl Into<String>,
    ) -> Self {
        Self {
            value,
            label_texture_name: label_texture_name.into(),
            preview_key: Some(preview_key.into()),
            display_label: None,
        }
    }

    /// Construct an enum value carrying an explicit text label for the
    /// overlay menu (no per-value preview image).
    pub fn with_display(
        value: i32,
        label_texture_name: impl Into<String>,
        display_label: impl Into<String>,
    ) -> Self {
        Self {
            value,
            label_texture_name: label_texture_name.into(),
            preview_key: None,
            display_label: Some(display_label.into()),
        }
    }

    /// Builder-style: attach/replace the overlay display label.
    pub fn display_label(mut self, label: impl Into<String>) -> Self {
        self.display_label = Some(label.into());
        self
    }
}

/// Display formatter for scalar rows.
#[derive(Debug, Clone, Copy)]
pub enum ScalarFormat {
    /// Plain integer, e.g. `"490"`.
    Integer,
    /// Fixed-point with `decimals` digits after the point, e.g. `decimals=2`
    /// renders `150` as `"1.50"`.
    FixedPoint { decimals: u8 },
    /// Integer with a **display-only** offset: renders
    /// `value + display_offset`. The stored value, persistence, callbacks,
    /// clamping, and step logic all operate on the raw value — only the text
    /// pushed to the row's value TextLayer shifts. Used by the WebUI cosmetic
    /// pickers, whose internal value is a 0-based asset index but whose
    /// selector displays 1-based ("1".."N", parity with the retired
    /// "ITEM #001" ribbons).
    OffsetInteger { display_offset: i32 },
    /// Integer with an embedded unit suffix, replicating the STOCK timing
    /// rows' display (DISPLAY/JUDGMENT TIMING): nonzero renders with an
    /// explicit sign (`"-41ms"`, `"+10ms"` — the game's `%+dms`), zero
    /// renders `"±0ms"` where `±` is the **Shift-JIS** plus-minus glyph
    /// (bytes `0x81 0x7D` — gamemdx `FUN_18016e4e0` @20260721 selects
    /// exactly these two format strings). The row's value TextLayer feeds
    /// the game's own `string::assign` + BmpString compositor, which is
    /// SJIS-native, so the bytes pass through unmodified. `unit` must be
    /// plain ASCII (it is embedded verbatim); keep it short — the whole
    /// string must fit the 15-byte SSO buffer.
    SignedUnit { unit: &'static str },
    /// Unsigned integer with an embedded unit suffix and NO sign, e.g.
    /// `"12ms"`, `"100%"`, `"70kg"`. For positive-only rows where the stock
    /// `SignedUnit` `+`/`±` prefix would be noise. `unit` must be plain
    /// ASCII (embedded verbatim); keep the whole composed string within the
    /// 15-byte SSO buffer (see `SignedUnit`).
    Unit { unit: &'static str },
    /// Value is a duration in SECONDS, rendered as `"M:SS"` (e.g. `90` →
    /// `"1:30"`), matching the music-wheel LENGTH readout convention.
    /// Negative values clamp to `"0:00"` (no row should produce one).
    MinutesSeconds,
    /// [`OffsetInteger`](Self::OffsetInteger) with a leading label, e.g.
    /// prefix `"Char #"` + offset 1 renders raw index `2` as `"Char #3"`.
    /// Used by the WebUI cosmetic pickers. `prefix` must be plain ASCII;
    /// keep it short — prefix + digits must fit the 15-byte SSO buffer
    /// even at the category's maximum count (e.g. `"Cover #"` + 5 digits
    /// = 12 bytes).
    PrefixedIndex {
        prefix: &'static str,
        display_offset: i32,
    },
}

/// How (and whether) an option's value participates in the persistence
/// layers. One declarative knob at the registration site; the persistence
/// service consults it at its choke points — always through the exhaustive
/// matrix methods below ([`saved_to_network`](Self::saved_to_network),
/// [`loaded_from_network`](Self::loaded_from_network),
/// [`json_cached`](Self::json_cached),
/// [`session_scoped`](Self::session_scoped)), never through ad-hoc
/// comparisons, so a new variant can never be silently misfiled.
///
/// | Mode       | network save | network load | JSON cache (write + prime) | card-in reset |
/// |------------|:------------:|:------------:|:--------------------------:|:-------------:|
/// | `Full`     | yes          | yes          | yes                        | no            |
/// | `SaveOnly` | yes          | no           | no                         | no            |
/// | `None`     | no           | no           | no                         | no            |
/// | `Session`  | no           | no           | no                         | **yes**       |
///
/// `SaveOnly` exists for options whose loaded state arrives through a
/// channel the game itself owns (e.g. the WebUI customize values, applied by
/// the game's native `<customize>` profile load): the DLL still *sends* the
/// value on save — the direction the game lacks — but never reads it back,
/// so the game's own load path stays the single source of truth.
///
/// `Session` exists for practice-session tools (the Training Mode bound
/// rows): the value must follow the CARD, not the profile — a player carding
/// in next week must not inherit last week's setting. It serializes nothing
/// in any direction and is reset to the row's `default_value`, per side, at
/// the card-in profile-load lifecycle point (the same SONG_SELECT-entry
/// drain where `Full` values land; see
/// `custom_options_persistence::apply_pending_card_in_resets`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistMode {
    /// Network save + network load + JSON cache. The default; equivalent to
    /// the historical `persist: true`.
    Full,
    /// Emitted on network save only; skipped by the network load and by the
    /// JSON cache write + prime.
    SaveOnly,
    /// No persistence at all. Equivalent to the historical `persist: false`.
    /// NOTE: the in-memory value is NOT reset on card swap — it lives for
    /// the process (the cabinet stays up across sessions). For values that
    /// must not leak between players' sessions use [`PersistMode::Session`].
    None,
    /// Session-scoped: no persistence in any direction, AND the value resets
    /// to `default_value` (per side) when a player cards in. For
    /// practice-session tools that must never follow the profile.
    Session,
}

impl PersistMode {
    /// Whether the option contributes a `<mod_{id}>` wire field to the
    /// network save. Consulted by `snapshot_for_save`.
    pub fn saved_to_network(self) -> bool {
        match self {
            PersistMode::Full | PersistMode::SaveOnly => true,
            PersistMode::None | PersistMode::Session => false,
        }
    }

    /// Whether a loaded value (network response OR the offline JSON prime —
    /// both funnel through `resolve_from_load`) may be written into the
    /// per-player cache.
    pub fn loaded_from_network(self) -> bool {
        match self {
            PersistMode::Full => true,
            PersistMode::SaveOnly | PersistMode::None | PersistMode::Session => false,
        }
    }

    /// Whether the option participates in the offline JSON cache write
    /// (`mod-config.json`). Consulted by `json_persisted`.
    pub fn json_cached(self) -> bool {
        match self {
            PersistMode::Full => true,
            PersistMode::SaveOnly | PersistMode::None | PersistMode::Session => false,
        }
    }

    /// Whether the option's per-side value resets to `default_value` at the
    /// card-in profile-load lifecycle point. Consulted by
    /// `reset_session_values`.
    pub fn session_scoped(self) -> bool {
        match self {
            PersistMode::Session => true,
            PersistMode::Full | PersistMode::SaveOnly | PersistMode::None => false,
        }
    }
}

/// Page tag for the metadata-map key. All custom options live on Page6
/// (the Mods tab). Kept as a type for internal use by the row-allocation
/// and filter systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PageTag {
    Page6,
}

impl PageTag {
    pub(crate) fn metadata_key(self) -> &'static str {
        match self {
            PageTag::Page6 => "Page6",
        }
    }
}

/// Declarative visibility predicate: under what condition the row should be
/// visible to the player.
///
/// [`ShowWhen::Equals`]/[`ShowWhen::NotEquals`] reference another registered
/// option by id.
/// **The referenced option must already be registered** when this option is
/// registered — the framework validates the reference synchronously and
/// rejects the registration with [`RegisterError::UnknownParent`] otherwise.
/// Mods that use parent/child options must register the parent first.
#[derive(Debug, Clone)]
pub enum ShowWhen {
    /// Always visible.
    Always,
    /// Visible only when the registered option with `parent_id` currently
    /// equals `value` for the relevant player side.
    Equals { parent_id: String, value: i32 },
    /// Visible only when the registered option with `parent_id` currently
    /// does NOT equal `value` for the relevant player side (e.g. a
    /// sub-option that is meaningless at the parent's default).
    NotEquals { parent_id: String, value: i32 },
}

/// Change-callback signature. Fires on three events:
///   - initial load (value primed from persisted or default)
///   - user advances the value in the options menu
///   - another component explicitly sets the value
///
/// `player_side` is `0` for P1 or `1` for P2. The callback runs on the game's
/// render thread and must not block or panic. Calling back into
/// [`register_option`](super::register_option),
/// [`get_value`](super::get_value), or any other entry point on the
/// `custom_options` service during a callback is not supported — the callback
/// fires while the framework's lock is released, but re-entering a write
/// path from within a callback is still a recipe for surprises and is
/// explicitly discouraged.
pub type OnChangeFn = fn(player_side: u8, new_value: i32);

/// Declarative description of one option. Passed to
/// [`register_option`](super::register_option).
///
/// The row's left-column label texture is derived from `id` as
/// `seop_item_<id>`; mods ship the matching PNG at
/// `data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/seop_item_<id>.png`
/// and the framework automatically clones the stock option-row atlas to
/// include the new texture.
#[derive(Debug, Clone)]
pub struct RegisterSpec {
    /// Stable identifier for this option. Used as the kbin element-name
    /// suffix (`<mod_{id}>`) and as the label-texture basename
    /// (`seop_item_<id>`). Keep it snake_case and kbin-valid (letters,
    /// digits, underscore; must start with a letter).
    pub id: &'static str,

    /// Row type (enum or scalar) plus its associated data.
    pub ui_kind: UiKind,

    /// Initial value primed into both players' caches at registration time.
    pub default_value: i32,

    /// Callback invoked when the value changes for either side.
    pub on_change: OnChangeFn,

    /// Visibility predicate. [`ShowWhen::Always`] for unconditional rows.
    pub show_when: ShowWhen,

    /// How this option's value participates in the persistence layers
    /// (network save / network load / offline JSON cache). Defaults to
    /// [`PersistMode::Full`]. See [`PersistMode`] for the matrix.
    pub persist: PersistMode,

    /// Optional transform applied to the in-memory value when serializing
    /// to the server. Defaults to identity. Use this when the wire format
    /// differs from the internal representation — e.g., the UI stores a
    /// sequential index 0..=N but the server should persist a stable asset
    /// ID so asset list changes don't shift saved values.
    ///
    /// The `id` argument lets a single shared transform function dispatch on
    /// the option id without needing a closure. Implementations should return
    /// the input unchanged if they don't recognize the id.
    pub save_transform: Option<fn(id: &str, value: i32) -> i32>,

    /// Optional transform applied to a value loaded from the server before
    /// it's written into the per-player cache. Defaults to identity. Should
    /// be the inverse of `save_transform`.
    pub load_transform: Option<fn(id: &str, value: i32) -> i32>,

    /// Which menus the row appears in. Defaults to both; the operator's
    /// `option_menu_settings` config overrides per menu at read time.
    pub menus: MenuPlacement,

    /// Human-readable row label for text-rendering menus (the overlay).
    /// `None` falls back to [`prettify_id`]`(id)`. The in-game menu ignores
    /// this — it renders the `seop_item_<id>` texture.
    pub display_name: Option<&'static str>,

    /// One-line description shown in the overlay's footer while the row is
    /// selected. `None` falls back to empty.
    pub description: Option<&'static str>,
}

impl RegisterSpec {
    /// Convenience builder for a boolean on/off toggle. Produces a
    /// [`UiKind::Enum`] with two values — `0` labeled `"seop_op_off"` and
    /// `1` labeled `"seop_op_on"`, both of which are stock sprites already
    /// present in Konami's shared value-ribbon atlas (no new assets needed
    /// for the value side; the row label follows the derived
    /// `seop_item_<id>` convention).
    ///
    /// The returned spec defaults to `default_value = 0` (off),
    /// `on_change = |_,_| {}` (caller must set), and
    /// `show_when = ShowWhen::Always`. Chain builder-style setters to
    /// customize. All options appear on the Mods tab (Page6).
    pub fn bool_toggle(id: &'static str) -> Self {
        Self {
            id,
            ui_kind: UiKind::Enum {
                // Per-value preview keys so each state shows its own preview
                // image (`seop_image_<id>_off` / `seop_image_<id>_on`) rather
                // than a single shared base. The value-ribbon chip still uses
                // the stock flat `seop_op_off` / `seop_op_on`. Options that
                // haven't shipped on/off preview PNGs just get a hidden preview
                // box (the availability gate), so this is safe for all toggles.
                allowed_values: vec![
                    EnumValue::with_preview(0, "seop_op_off", "off").display_label("OFF"),
                    EnumValue::with_preview(1, "seop_op_on", "on").display_label("ON"),
                ],
            },
            default_value: 0,
            on_change: default_on_change_noop,
            show_when: ShowWhen::Always,
            persist: PersistMode::Full,
            save_transform: None,
            load_transform: None,
            menus: MenuPlacement::default(),
            display_name: None,
            description: None,
        }
    }

    /// Builder for an enum (fixed-list) option. The row cycles left/right
    /// through `allowed_values`; each value's `label_texture_name` is its
    /// value-ribbon chip and its optional `preview_key` selects the preview-
    /// box image. Defaults match the other builders (`default_value` = the
    /// first value's `value`, no-op `on_change`, `ShowWhen::Always`,
    /// persist on). Chain the builder setters to customize.
    ///
    /// `allowed_values` must be non-empty; an empty list yields a row that
    /// can't cycle (the registrar accepts it but it's almost certainly a
    /// caller bug).
    pub fn enum_values(id: &'static str, allowed_values: Vec<EnumValue>) -> Self {
        let default_value = allowed_values.first().map(|v| v.value).unwrap_or(0);
        Self {
            id,
            ui_kind: UiKind::Enum { allowed_values },
            default_value,
            on_change: default_on_change_noop,
            show_when: ShowWhen::Always,
            persist: PersistMode::Full,
            save_transform: None,
            load_transform: None,
            menus: MenuPlacement::default(),
            display_name: None,
            description: None,
        }
    }

    /// Builder for a scalar (numeric-range) option. Use `with_on_change`
    /// and `with_default_value` to customize; `step_coarse` defaults to
    /// `step_fine` (i.e. no coarse-step acceleration). Supply
    /// `ScalarFormat::Integer` for integer displays or
    /// `ScalarFormat::FixedPoint { decimals }` for values that should
    /// render as e.g. `"1.50"` for an internal `150` with `decimals=2`.
    pub fn scalar(
        id: &'static str,
        min: i32,
        max: i32,
        step_fine: i32,
        format: ScalarFormat,
    ) -> Self {
        Self {
            id,
            ui_kind: UiKind::Scalar {
                min,
                max,
                step_fine,
                step_coarse: step_fine,
                format,
            },
            default_value: min,
            on_change: default_on_change_noop,
            show_when: ShowWhen::Always,
            persist: PersistMode::Full,
            save_transform: None,
            load_transform: None,
            menus: MenuPlacement::default(),
            display_name: None,
            description: None,
        }
    }

    /// Builder for a header row ([`UiKind::Header`]) — a stateless,
    /// non-selectable group heading. The label texture follows the standard
    /// `seop_item_<id>` convention (ship the PNG via LayeredFS like any row
    /// label). The texture IS the header's entire visible art — an opaque
    /// full-width bar (the header renderer hides the row's value box and
    /// draws nothing else) — so author it full-row-width and opaque.
    ///
    /// The returned spec is the ONLY valid header shape: `PersistMode::None`,
    /// no callback, `ShowWhen::Always`, no transforms, `default_value = 0`.
    /// Chaining any state-bearing builder setter onto it produces a spec the
    /// registrar refuses with [`RegisterError::HeaderCarriesState`].
    pub fn header(id: &'static str) -> Self {
        Self {
            id,
            ui_kind: UiKind::Header,
            default_value: 0,
            on_change: default_on_change_noop,
            show_when: ShowWhen::Always,
            persist: PersistMode::None,
            save_transform: None,
            load_transform: None,
            menus: MenuPlacement::default(),
            display_name: None,
            description: None,
        }
    }

    /// Override `step_coarse` on a scalar spec. No-op (and logged WARN
    /// by the registrar) on enum specs.
    pub fn step_coarse(mut self, coarse: i32) -> Self {
        if let UiKind::Scalar {
            ref mut step_coarse,
            ..
        } = self.ui_kind
        {
            *step_coarse = coarse;
        }
        self
    }

    /// Set the initial value primed into both players' caches.
    pub fn default_value(mut self, value: i32) -> Self {
        self.default_value = value;
        self
    }

    /// Set the change callback.
    pub fn on_change(mut self, cb: OnChangeFn) -> Self {
        self.on_change = cb;
        self
    }

    /// Set the visibility predicate.
    pub fn show_when(mut self, predicate: ShowWhen) -> Self {
        self.show_when = predicate;
        self
    }

    /// Disable persistence entirely for this option
    /// ([`PersistMode::None`]). The value never round-trips through the
    /// backend or the JSON cache — but it is NOT reset on card swap (it
    /// lives in memory for the process). For session-scoped values that
    /// reset at card-in, use `.persist_mode(PersistMode::Session)`.
    pub fn no_persist(mut self) -> Self {
        self.persist = PersistMode::None;
        self
    }

    /// Set this option's [`PersistMode`] explicitly. Builders default to
    /// [`PersistMode::Full`]; use [`PersistMode::SaveOnly`] for options whose
    /// loaded state arrives through a game-native channel (the DLL emits the
    /// value on network save but never reads it back).
    pub fn persist_mode(mut self, mode: PersistMode) -> Self {
        self.persist = mode;
        self
    }

    /// Install save/load transforms. `save` is applied to the in-memory value
    /// before it's sent to the server; `load` is applied to a value received
    /// from the server before it's written into the cache. The two functions
    /// must be inverses: `load(id, save(id, v)) == v` for every `v` the
    /// option can hold. Useful when the wire format differs from the internal
    /// representation (e.g. server stores stable asset IDs while the UI
    /// operates on sequential indices).
    pub fn persist_transform(
        mut self,
        save: fn(id: &str, value: i32) -> i32,
        load: fn(id: &str, value: i32) -> i32,
    ) -> Self {
        self.save_transform = Some(save);
        self.load_transform = Some(load);
        self
    }

    /// Install only the save-side transform (in-memory value → wire value).
    /// For [`PersistMode::SaveOnly`] options no load path consults a
    /// `load_transform`, so registering one would be dead code — this setter
    /// leaves `load_transform` as `None`.
    pub fn save_transform(mut self, save: fn(id: &str, value: i32) -> i32) -> Self {
        self.save_transform = Some(save);
        self
    }

    /// Set which menus this row appears in (default: both).
    pub fn menus(mut self, menus: MenuPlacement) -> Self {
        self.menus = menus;
        self
    }

    /// Convenience: appear only in the game's native options menu.
    pub fn in_game_only(mut self) -> Self {
        self.menus = MenuPlacement {
            in_game: true,
            overlay: false,
        };
        self
    }

    /// Convenience: appear only in the overlay mod menu.
    pub fn overlay_only(mut self) -> Self {
        self.menus = MenuPlacement {
            in_game: false,
            overlay: true,
        };
        self
    }

    /// Set the human-readable row label for text-rendering menus.
    ///
    /// STYLE (maintainer convention, 2026-08-25): option-row labels are
    /// Title Case ("Song Playback Speed") — ALL CAPS is reserved for
    /// header rows, tab labels, and enum VALUE labels (OFF/ON etc.).
    pub fn display_name(mut self, name: &'static str) -> Self {
        self.display_name = Some(name);
        self
    }

    /// Set the one-line footer description for the overlay menu.
    pub fn description(mut self, description: &'static str) -> Self {
        self.description = Some(description);
        self
    }
}

// ── Display-string fallbacks ─────────────────────────────────────────

/// Prettify a snake_case option id into a Title Case label:
/// `"song_speed"` → `"Song Speed"`. The fallback for rows registered
/// without an explicit `display_name` (the Step 9 sweep replaces most
/// uses with curated strings).
pub(crate) fn prettify_id(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    for word in id.split('_').filter(|w| !w.is_empty()) {
        if !out.is_empty() {
            out.push(' ');
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

/// Prettify an enum value's `label_texture_name` into a display label by
/// stripping the known atlas prefixes (`seop_image_`, `seop_item_`,
/// `seop_op_`) and Title-Casing the remainder: `"seop_op_dark"` → `"Dark"`.
/// Unprefixed names prettify as-is.
pub(crate) fn prettify_texture_suffix(name: &str) -> String {
    const PREFIXES: [&str; 3] = ["seop_image_", "seop_item_", "seop_op_"];
    let stripped = PREFIXES
        .iter()
        .find_map(|p| name.strip_prefix(p))
        .unwrap_or(name);
    prettify_id(stripped)
}

// ── Scalar display formatting ────────────────────────────────────────

/// Format an integer value according to the row's [`ScalarFormat`]. Returns
/// raw display BYTES because the output feeds the game's SJIS-native
/// `string::assign`/BmpString pipeline directly — `SignedUnit`'s zero case
/// embeds the Shift-JIS `±` glyph (`0x81 0x7D`), which is not valid UTF-8.
/// Fixed-point renders the value as `v / 10^decimals` with the fractional
/// part zero-padded to `decimals` digits, matching the convention the game
/// uses for scroll speed (e.g. `decimals=2`: `150` → `"1.50"`).
/// Offset-integer renders `value + display_offset` — display-only; the
/// stored value is untouched. Signed-unit replicates the stock timing rows
/// (see the variant's doc above). Lives here (beside [`ScalarFormat`])
/// so both menus — the in-game rows and the overlay snapshot — render
/// values through the same function.
pub(crate) fn format_scalar_value(value: i32, format: ScalarFormat) -> Vec<u8> {
    match format {
        ScalarFormat::Integer => value.to_string().into_bytes(),
        ScalarFormat::OffsetInteger { display_offset } => value
            .saturating_add(display_offset)
            .to_string()
            .into_bytes(),
        ScalarFormat::FixedPoint { decimals } => {
            if decimals == 0 {
                return value.to_string().into_bytes();
            }
            let divisor = 10i32.pow(decimals as u32);
            let abs = value.unsigned_abs() as i64;
            let whole = abs / divisor as i64;
            let frac = abs % divisor as i64;
            let sign = if value < 0 { "-" } else { "" };
            format!("{sign}{whole}.{frac:0width$}", width = decimals as usize).into_bytes()
        }
        ScalarFormat::SignedUnit { unit } => {
            if value == 0 {
                // Stock zero case: SJIS ± (0x81 0x7D) + "0" + unit.
                let mut out = vec![0x81u8, 0x7D, b'0'];
                out.extend_from_slice(unit.as_bytes());
                out
            } else {
                // Stock nonzero case: %+d + unit (explicit sign).
                format!("{value:+}{unit}").into_bytes()
            }
        }
        ScalarFormat::Unit { unit } => format!("{value}{unit}").into_bytes(),
        ScalarFormat::MinutesSeconds => {
            let total = value.max(0);
            format!("{}:{:02}", total / 60, total % 60).into_bytes()
        }
        ScalarFormat::PrefixedIndex {
            prefix,
            display_offset,
        } => format!("{prefix}{}", value.saturating_add(display_offset)).into_bytes(),
    }
}

/// UTF-8 view of [`format_scalar_value`] for text-rendering menus (the
/// overlay snapshot): identical text, with `SignedUnit`'s Shift-JIS `±`
/// zero-case pair mapped to the UTF-8 `"±"` glyph. Both menus therefore
/// render every scalar value identically modulo that one encoding hop.
pub(crate) fn format_scalar_value_utf8(value: i32, format: ScalarFormat) -> String {
    // Only SignedUnit's zero case emits non-UTF-8 bytes (the SJIS ± pair) —
    // handle it explicitly; everything else is plain ASCII.
    if let ScalarFormat::SignedUnit { unit } = format {
        if value == 0 {
            return format!("±0{unit}");
        }
    }
    String::from_utf8(format_scalar_value(value, format)).unwrap_or_default()
}

fn default_on_change_noop(_side: u8, _value: i32) {}

/// Whether `cb` is the registration-time default no-op callback. Used by the
/// registry's header validation: a header spec must not carry a real change
/// callback (headers hold no state, so nothing can ever change). Reliable
/// here — both sides of the comparison name the same non-generic fn item in
/// this module.
pub(crate) fn is_default_on_change(cb: OnChangeFn) -> bool {
    std::ptr::fn_addr_eq(cb, default_on_change_noop as OnChangeFn)
}

/// Failure reasons for [`register_option`](super::register_option). All
/// variants are recoverable: the mod continues, the option simply isn't
/// registered.
#[derive(Debug, Clone)]
pub enum RegisterError {
    /// The service hasn't been initialized. Can happen if a mod's `enable()`
    /// runs before `custom_options::init()`.
    NotInitialized,

    /// An option with the same id was already registered.
    Duplicate { id: String },

    /// [`ShowWhen::Equals`] referenced a `parent_id` that isn't registered.
    /// Register the parent option first.
    UnknownParent { id: String, parent_id: String },

    /// A [`UiKind::Header`] spec carried state a header must not hold
    /// (persistence, a change callback, a parent/child link, transforms, or
    /// a non-zero value). Headers are display-only; build them with
    /// [`RegisterSpec::header`] and chain nothing state-bearing. `what`
    /// names the offending field.
    HeaderCarriesState { id: String, what: &'static str },
}

impl std::fmt::Display for RegisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegisterError::NotInitialized => {
                write!(f, "custom_options service is not initialized")
            }
            RegisterError::Duplicate { id } => {
                write!(f, "option id {id:?} is already registered")
            }
            RegisterError::UnknownParent { id, parent_id } => write!(
                f,
                "option {id:?} references unknown parent {parent_id:?}; \
                 register the parent first"
            ),
            RegisterError::HeaderCarriesState { id, what } => write!(
                f,
                "header {id:?} carries {what}; headers are stateless — \
                 build with RegisterSpec::header and chain nothing state-bearing"
            ),
        }
    }
}

impl std::error::Error for RegisterError {}

#[cfg(test)]
mod tests {
    use super::*;

    // ── MenuPlacement / builders ─────────────────────────────────────

    #[test]
    fn menu_placement_defaults_to_both() {
        let p = MenuPlacement::default();
        assert!(p.in_game && p.overlay);
        let spec = RegisterSpec::bool_toggle("x");
        assert_eq!(spec.menus, MenuPlacement::default());
        assert!(spec.display_name.is_none() && spec.description.is_none());
    }

    #[test]
    fn placement_builders() {
        let spec = RegisterSpec::bool_toggle("x").in_game_only();
        assert!(spec.menus.in_game && !spec.menus.overlay);
        let spec = RegisterSpec::bool_toggle("x").overlay_only();
        assert!(!spec.menus.in_game && spec.menus.overlay);
        let spec = RegisterSpec::bool_toggle("x").menus(MenuPlacement {
            in_game: false,
            overlay: false,
        });
        assert!(!spec.menus.in_game && !spec.menus.overlay);
    }

    #[test]
    fn display_string_builders() {
        let spec = RegisterSpec::bool_toggle("x")
            .display_name("Premium Free")
            .description("Unlimited songs per credit");
        assert_eq!(spec.display_name, Some("Premium Free"));
        assert_eq!(spec.description, Some("Unlimited songs per credit"));
    }

    // ── EnumValue display labels ─────────────────────────────────────

    #[test]
    fn bool_toggle_carries_off_on_display_labels() {
        let spec = RegisterSpec::bool_toggle("x");
        let UiKind::Enum { allowed_values } = &spec.ui_kind else {
            panic!("bool_toggle must be an enum");
        };
        let labels: Vec<_> = allowed_values
            .iter()
            .map(|v| v.display_label.as_deref())
            .collect();
        assert_eq!(labels, vec![Some("OFF"), Some("ON")]);
    }

    #[test]
    fn enum_value_constructors() {
        assert!(EnumValue::new(0, "seop_op_off").display_label.is_none());
        assert!(EnumValue::with_preview(0, "seop_op_off", "off")
            .display_label
            .is_none());
        let v = EnumValue::with_display(2, "seop_op_dark", "Dark");
        assert_eq!(v.display_label.as_deref(), Some("Dark"));
        assert!(v.preview_key.is_none());
    }

    // ── Prettify fallbacks ───────────────────────────────────────────

    #[test]
    fn prettify_id_cases() {
        assert_eq!(prettify_id("song_speed"), "Song Speed");
        assert_eq!(prettify_id("autoplay"), "Autoplay");
        assert_eq!(prettify_id("assist_tick_volume"), "Assist Tick Volume");
        assert_eq!(
            prettify_id("customize_character_p1"),
            "Customize Character P1"
        );
        assert_eq!(prettify_id("a__b"), "A B"); // repeated underscores collapse
        assert_eq!(prettify_id(""), "");
    }

    #[test]
    fn prettify_texture_suffix_cases() {
        assert_eq!(prettify_texture_suffix("seop_op_dark"), "Dark");
        assert_eq!(prettify_texture_suffix("seop_op_on"), "On");
        assert_eq!(
            prettify_texture_suffix("seop_image_lanecover_hidden"),
            "Lanecover Hidden"
        );
        assert_eq!(
            prettify_texture_suffix("seop_item_premium_free"),
            "Premium Free"
        );
        assert_eq!(prettify_texture_suffix("plain_name"), "Plain Name");
    }

    // ── Scalar formatting (moved fn + UTF-8 view) ────────────────────

    #[test]
    fn format_scalar_value_moved_parity_spot_checks() {
        // The exhaustive byte pins live in scalar_format_tests.rs (in-crate);
        // these spot-check the moved fn end-to-end in the host harness.
        assert_eq!(format_scalar_value(490, ScalarFormat::Integer), b"490");
        assert_eq!(
            format_scalar_value(150, ScalarFormat::FixedPoint { decimals: 2 }),
            b"1.50"
        );
        assert_eq!(
            format_scalar_value(0, ScalarFormat::SignedUnit { unit: "ms" }),
            vec![0x81u8, 0x7D, b'0', b'm', b's']
        );
    }

    #[test]
    fn format_scalar_value_utf8_all_variants() {
        let f = format_scalar_value_utf8;
        assert_eq!(f(490, ScalarFormat::Integer), "490");
        assert_eq!(f(150, ScalarFormat::FixedPoint { decimals: 2 }), "1.50");
        assert_eq!(f(2, ScalarFormat::OffsetInteger { display_offset: 1 }), "3");
        assert_eq!(f(-41, ScalarFormat::SignedUnit { unit: "ms" }), "-41ms");
        assert_eq!(f(10, ScalarFormat::SignedUnit { unit: "ms" }), "+10ms");
        // The SJIS ± zero case maps to the UTF-8 glyph.
        assert_eq!(f(0, ScalarFormat::SignedUnit { unit: "ms" }), "±0ms");
        assert_eq!(f(70, ScalarFormat::Unit { unit: "kg" }), "70kg");
        assert_eq!(f(90, ScalarFormat::MinutesSeconds), "1:30");
        assert_eq!(
            f(
                2,
                ScalarFormat::PrefixedIndex {
                    prefix: "Char #",
                    display_offset: 1
                }
            ),
            "Char #3"
        );
    }
}
