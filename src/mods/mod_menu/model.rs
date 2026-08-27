//! Pure model layer for the rewritten mod-menu overlay: the unified row
//! model, tab identity + per-tab navigation memory, tab row-list builders
//! (MODS, GLOBAL SETTINGS), and the cursor/scroll navigation state machine.
//!
//! Deliberately **dependency-free** (no `crate::` imports) so its tests run on
//! any host via the temp-crate harness (`scripts/validate_mod_menu.sh`). The
//! impure shell (`tabs.rs`/`render.rs`/`input.rs`) assembles snapshots, calls
//! the builders, and renders/edits through the results; value mutation and
//! callbacks stay in the impure layer.
//!
//! Design: overlay-menu rewrite detailed design §4.2 (row model), §4.8
//! (navigation), FR-1/2/3/12/13.

// ── Tabs ────────────────────────────────────────────────────────────

/// Menu tab identity. Everything here iterates `TabId::ALL` so adding a
/// variant is one edit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TabId {
    Mods,
    GlobalSettings,
    PlayerSettings,
    Theme,
}

impl TabId {
    pub const ALL: &'static [TabId] = &[
        TabId::Mods,
        TabId::GlobalSettings,
        TabId::PlayerSettings,
        TabId::Theme,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TabId::Mods => "TOGGLE MODS",
            TabId::GlobalSettings => "GLOBAL SETTINGS",
            TabId::PlayerSettings => "PLAYER SETTINGS",
            TabId::Theme => "APPEARANCE",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&t| t == self).unwrap_or(0)
    }

    pub fn next(self) -> TabId {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> TabId {
        Self::ALL[(self.index() + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

// ── Rows ────────────────────────────────────────────────────────────

/// Value/kind of a menu row (the display model; ownership of the authoritative
/// value stays with the impure layer / owning mod).
#[derive(Clone, Debug, PartialEq)]
pub enum RowKind {
    /// On/off row (registry mod toggles; later, mirrored bool options).
    Boolean { value: bool },
    /// Numeric row adjusted Left/Right (fine) or Start-held (coarse).
    /// `formatted` — when `Some`, the renderer shows it VERBATIM in the value
    /// column (mirrored options carry the framework's formatted text —
    /// `"±0ms"`, `"1.50"`, `"Char #3"` — for display parity with the in-game
    /// menu); `None` falls back to the plain signed-integer text.
    Scalar {
        value: i32,
        min: i32,
        max: i32,
        step_fine: i32,
        step_coarse: i32,
        formatted: Option<String>,
    },
    /// Labeled pick-list; `values[index]` is the raw value, `labels[index]`
    /// renders in the value column.
    Enum {
        index: usize,
        values: Vec<i32>,
        labels: Vec<String>,
    },
    /// Decorative section heading — label-only, never selectable.
    Header,
}

/// Where a row came from — decides which edit path the impure layer drives.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowSource {
    /// A registry mod's master toggle (registry toggle + save_mod_states).
    RegistryToggle,
    /// A cabinet-wide row registered via the overlay's own API (on_change).
    Contributed,
    /// A per-player custom_options mirror (arrives with the PLAYER tab step).
    Mirrored,
    /// A THEME tab appearance row (edited via the theme arm in `input.rs`;
    /// keys `theme` / `animate_bg` / `opacity` — see [`build_theme_tab`]).
    Theme,
}

/// One display row.
#[derive(Clone, Debug)]
pub struct Row {
    /// Stable id: mod id (RegistryToggle), contributed row key, option id
    /// (Mirrored), or a `__header_*` synthetic key.
    pub key: String,
    pub label: String,
    /// Footer text while selected.
    pub description: String,
    pub kind: RowKind,
    pub source: RowSource,
    /// Rendered dim; cursor skips; edits refused.
    pub greyed: bool,
}

impl Row {
    pub fn selectable(&self) -> bool {
        !self.greyed && !matches!(self.kind, RowKind::Header)
    }
}

// ── Snapshot inputs (assembled by the impure layer) ─────────────────

/// A registry mod entry as of menu open / rebuild.
#[derive(Clone, Debug)]
pub struct ModEntrySnap {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
}

/// A contributed (cabinet-wide) row registration.
#[derive(Clone, Debug)]
pub struct ContributedSnap {
    pub key: String,
    pub label: String,
    pub hint: String,
    pub kind: RowKind,
    /// The registering mod's id (the old `parent_row_key`); `None` = unowned
    /// (renders at the tail of GLOBAL SETTINGS without a group header).
    pub owning_mod_id: Option<String>,
}

// ── Tab builders ────────────────────────────────────────────────────

/// MODS tab: one Boolean toggle row per registry mod, registration order,
/// excluding the menu itself (FR-2).
pub fn build_mods_tab(entries: &[ModEntrySnap]) -> Vec<Row> {
    entries
        .iter()
        .filter(|e| e.id != "mod-menu")
        .map(|e| Row {
            key: e.id.clone(),
            label: e.name.clone(),
            description: e.description.clone(),
            kind: RowKind::Boolean { value: e.enabled },
            source: RowSource::RegistryToggle,
            greyed: false,
        })
        .collect()
}

/// GLOBAL SETTINGS tab: for each ENABLED mod owning ≥1 contributed row, a
/// Header row (the mod's display name) followed by its rows in contributed
/// order; disabled mods' groups are hidden entirely (their master toggle
/// lives on MODS). Unowned rows trail without a header (FR-3).
pub fn build_global_tab(entries: &[ModEntrySnap], contributed: &[ContributedSnap]) -> Vec<Row> {
    let mut rows = Vec::new();
    for e in entries.iter().filter(|e| e.enabled && e.id != "mod-menu") {
        let owned: Vec<&ContributedSnap> = contributed
            .iter()
            .filter(|c| c.owning_mod_id.as_deref() == Some(e.id.as_str()))
            .collect();
        if owned.is_empty() {
            continue;
        }
        rows.push(Row {
            key: format!("__header_{}", e.id),
            label: e.name.clone(),
            description: String::new(),
            kind: RowKind::Header,
            source: RowSource::Contributed,
            greyed: false,
        });
        for c in owned {
            rows.push(contributed_row(c));
        }
    }
    for c in contributed.iter().filter(|c| c.owning_mod_id.is_none()) {
        rows.push(contributed_row(c));
    }
    rows
}

fn contributed_row(c: &ContributedSnap) -> Row {
    Row {
        key: c.key.clone(),
        label: c.label.clone(),
        description: c.hint.clone(),
        kind: c.kind.clone(),
        source: RowSource::Contributed,
        greyed: false,
    }
}

// ── PLAYER SETTINGS tab (mirrored custom_options) ───────────────────

/// One mirrored option row from the custom_options overlay snapshot —
/// the model-local, dependency-free mirror of `OverlayRowInfo` (tabs.rs
/// converts; this module never imports the service).
#[derive(Clone, Debug, PartialEq)]
pub struct MirroredRowSnap {
    /// Option id (becomes the row `key` — the edit path's `set_value` id).
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub kind: MirroredKindSnap,
    /// Per-side ShowWhen evaluation; `false` rows are OMITTED from the tab
    /// (matches the in-game menu, which hides them).
    pub visible: bool,
}

/// Mirror of `OverlayRowKind` (see `MirroredRowSnap`).
#[derive(Clone, Debug, PartialEq)]
pub enum MirroredKindSnap {
    Bool {
        value: bool,
    },
    Enum {
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
        /// The framework's formatted display text (parity with the in-game
        /// row) — carried into `RowKind::Scalar.formatted`.
        formatted: String,
    },
    Header,
}

/// Build the PLAYER SETTINGS row list from snapshot rows for the selected
/// side. `visible == false` rows are omitted; kinds map 1:1 (formatted
/// carried); every non-header row is greyed when the side isn't editable
/// (FR-5's browsable-but-locked state — headers are never selectable
/// anyway).
pub fn build_player_tab(rows: &[MirroredRowSnap], editable: bool) -> Vec<Row> {
    rows.iter()
        .filter(|r| r.visible)
        .map(|r| {
            let kind = match &r.kind {
                MirroredKindSnap::Bool { value } => RowKind::Boolean { value: *value },
                MirroredKindSnap::Enum {
                    index,
                    values,
                    labels,
                } => RowKind::Enum {
                    index: *index,
                    values: values.clone(),
                    labels: labels.clone(),
                },
                MirroredKindSnap::Scalar {
                    value,
                    min,
                    max,
                    step_fine,
                    step_coarse,
                    formatted,
                } => RowKind::Scalar {
                    value: *value,
                    min: *min,
                    max: *max,
                    step_fine: *step_fine,
                    step_coarse: *step_coarse,
                    formatted: Some(formatted.clone()),
                },
                MirroredKindSnap::Header => RowKind::Header,
            };
            let is_header = matches!(kind, RowKind::Header);
            Row {
                key: r.id.clone(),
                label: r.display_name.clone(),
                description: r.description.clone(),
                kind,
                source: RowSource::Mirrored,
                greyed: !editable && !is_header,
            }
        })
        .collect()
}

// ── THEME tab (appearance rows) ─────────────────────────────────────

/// Row key of the THEME enum row.
pub const THEME_ROW_KEY: &str = "theme";
/// Row key of the ANIMATED BACKGROUND bool row.
pub const ANIMATE_ROW_KEY: &str = "animate_bg";
/// Row key of the MENU OPACITY scalar row.
pub const OPACITY_ROW_KEY: &str = "opacity";

/// Build the THEME tab's fixed three-row list (design §4.6). Pure over
/// plain inputs — the theme display labels arrive as data (the impure
/// shell collects them from `theme::THEMES`). `animate_greyed` is Step 8's
/// availability gate (always `false` until then); the opacity row carries
/// the chrome bounds (25..=100, fine 5 / coarse 10) with a formatted
/// `"NN%"` value.
pub fn build_theme_tab(
    theme_index: usize,
    theme_labels: &[String],
    animate: bool,
    animate_greyed: bool,
    opacity: i32,
) -> Vec<Row> {
    vec![
        Row {
            key: THEME_ROW_KEY.to_string(),
            label: "Theme".to_string(),
            description: "Menu color scheme and background style".to_string(),
            kind: RowKind::Enum {
                index: theme_index.min(theme_labels.len().saturating_sub(1)),
                values: (0..theme_labels.len() as i32).collect(),
                labels: theme_labels.to_vec(),
            },
            source: RowSource::Theme,
            greyed: false,
        },
        Row {
            key: ANIMATE_ROW_KEY.to_string(),
            label: "Animated Background".to_string(),
            description:
                "Animated shader background behind the menu (requires the Shader Fixes mod)"
                    .to_string(),
            kind: RowKind::Boolean { value: animate },
            source: RowSource::Theme,
            greyed: animate_greyed,
        },
        Row {
            key: OPACITY_ROW_KEY.to_string(),
            label: "Menu Opacity".to_string(),
            description: "Menu panel opacity".to_string(),
            kind: RowKind::Scalar {
                value: opacity,
                min: 25,
                max: 100,
                step_fine: 5,
                step_coarse: 10,
                formatted: Some(format!("{opacity}%")),
            },
            source: RowSource::Theme,
            greyed: false,
        },
    ]
}

// ── Session gating & side selection (design §4.9) ───────────────────

/// Side-selector presentation state, from how many sides are editable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectorState {
    /// Both sides editable — LEFT/RIGHT switches freely.
    Free,
    /// Exactly one side editable — selector locked to it (greyed).
    Locked,
    /// No side editable — selector greyed, banner shown, all rows greyed.
    AllGated,
}

/// Which sides are editable: entered (fail-closed on `None` — records
/// unavailable) AND outside the attract/boot scene band.
pub fn editable_sides(entered: [Option<bool>; 2], in_attract_band: bool) -> [bool; 2] {
    [
        entered[0] == Some(true) && !in_attract_band,
        entered[1] == Some(true) && !in_attract_band,
    ]
}

/// Resolve the displayed/configured side: the desired side when editable;
/// else the single editable side; else the desired side unchanged (nothing
/// is editable — display-only).
pub fn resolve_selected_side(desired: u8, editable: [bool; 2]) -> u8 {
    let desired_idx = (desired as usize).min(1);
    if editable[desired_idx] {
        return desired_idx as u8;
    }
    match editable {
        [true, false] => 0,
        [false, true] => 1,
        _ => desired_idx as u8,
    }
}

/// Selector presentation from the editable set.
pub fn selector_state(editable: [bool; 2]) -> SelectorState {
    match editable {
        [true, true] => SelectorState::Free,
        [true, false] | [false, true] => SelectorState::Locked,
        [false, false] => SelectorState::AllGated,
    }
}

// ── Navigation ──────────────────────────────────────────────────────

/// Cursor + scroll for one tab. `cursor` indexes the tab's row list directly.
/// `pinned_focus` — cursor rests on the tab's PINNED slot (the PLAYER
/// SETTINGS side selector) above the scroll region; only meaningful on tabs
/// whose Navigator is built `with_pinned` (always false elsewhere).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NavState {
    pub cursor: usize,
    pub scroll: usize,
    pub pinned_focus: bool,
}

/// Per-tab navigation memory + the active tab. Owned by the shell for the
/// duration of one menu open; `reset()` on close (FR-1).
#[derive(Clone, Debug)]
pub struct TabNav {
    active: TabId,
    states: Vec<NavState>, // parallel to TabId::ALL
}

impl Default for TabNav {
    fn default() -> Self {
        Self::new()
    }
}

impl TabNav {
    pub fn new() -> Self {
        Self {
            active: TabId::ALL[0],
            states: vec![NavState::default(); TabId::ALL.len()],
        }
    }

    pub fn active(&self) -> TabId {
        self.active
    }

    pub fn switch_next(&mut self) {
        self.active = self.active.next();
    }

    pub fn switch_prev(&mut self) {
        self.active = self.active.prev();
    }

    pub fn state(&self) -> NavState {
        self.states[self.active.index()]
    }

    pub fn state_mut(&mut self) -> &mut NavState {
        let i = self.active.index();
        &mut self.states[i]
    }

    /// Back to the first tab with all positions forgotten (menu close).
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

/// Pure navigation over one tab's built row list. `page` = visible row
/// slots. `pinned` = the tab carries a pinned selector slot ABOVE the scroll
/// region (the PLAYER SETTINGS `CONFIGURING` row): the cursor can rest on it
/// (`NavState.pinned_focus`), it joins the wrap cycle at the top, and it
/// never scrolls.
pub struct Navigator<'a> {
    rows: &'a [Row],
    page: usize,
    pinned: bool,
}

impl<'a> Navigator<'a> {
    pub fn new(rows: &'a [Row], page: usize) -> Self {
        Self {
            rows,
            page: page.max(1),
            pinned: false,
        }
    }

    /// A navigator whose tab carries a pinned selector slot (PLAYER
    /// SETTINGS). With `pinned = false` this is exactly [`Navigator::new`].
    pub fn new_with_pinned(rows: &'a [Row], page: usize, pinned: bool) -> Self {
        Self {
            rows,
            page: page.max(1),
            pinned,
        }
    }

    /// The selected row index, or `None` when the cursor rests on nothing
    /// selectable (empty list / all headers+greyed / the pinned slot).
    pub fn selected(&self, nav: &NavState) -> Option<usize> {
        if self.pinned && nav.pinned_focus {
            return None;
        }
        self.rows
            .get(nav.cursor)
            .filter(|r| r.selectable())
            .map(|_| nav.cursor)
    }

    /// Whether the cursor currently rests on the pinned slot.
    pub fn pinned_focused(&self, nav: &NavState) -> bool {
        self.pinned && nav.pinned_focus
    }

    /// First selectable row index, if any.
    fn first_selectable(&self) -> Option<usize> {
        self.rows.iter().position(|r| r.selectable())
    }

    /// Last selectable row index, if any.
    fn last_selectable(&self) -> Option<usize> {
        self.rows.iter().rposition(|r| r.selectable())
    }

    /// Move up (wrapping), skipping unselectable rows. No-op when nothing is
    /// selectable (on a pinned tab, focus parks on the selector instead).
    pub fn up(&self, nav: &mut NavState) {
        if self.pinned {
            if nav.pinned_focus {
                // Selector → last selectable row (wrap); stay when none.
                if let Some(last) = self.last_selectable() {
                    nav.pinned_focus = false;
                    nav.cursor = last;
                    self.follow_scroll(nav);
                }
                return;
            }
            // First selectable row → selector.
            if self.first_selectable().is_none_or(|f| nav.cursor <= f) {
                nav.pinned_focus = true;
                return;
            }
        }
        self.step(nav, /* down = */ false);
    }

    /// Move down (wrapping), skipping unselectable rows. No-op when nothing
    /// is selectable (on a pinned tab, focus parks on the selector instead).
    pub fn down(&self, nav: &mut NavState) {
        if self.pinned {
            if nav.pinned_focus {
                // Selector → first selectable row; stay when none.
                if let Some(first) = self.first_selectable() {
                    nav.pinned_focus = false;
                    nav.cursor = first;
                    self.follow_scroll(nav);
                }
                return;
            }
            // Last selectable row → selector (the wrap passes the top).
            if self.last_selectable().is_none_or(|l| nav.cursor >= l) {
                nav.pinned_focus = true;
                return;
            }
        }
        self.step(nav, /* down = */ true);
    }

    fn step(&self, nav: &mut NavState, down: bool) {
        let len = self.rows.len();
        if len == 0 {
            return;
        }
        let mut i = nav.cursor.min(len - 1);
        for _ in 0..len {
            i = if down {
                (i + 1) % len
            } else {
                (i + len - 1) % len
            };
            if self.rows[i].selectable() {
                nav.cursor = i;
                self.follow_scroll(nav);
                return;
            }
        }
        // Nothing selectable anywhere: leave the cursor parked.
    }

    /// Re-validate after the row list changed (rebuild, visibility collapse):
    /// clamp the cursor into range, snap it to the nearest selectable row
    /// (searching down first, then up), and re-clamp the scroll window —
    /// including the stale-high `scroll > cursor` case that would otherwise
    /// underflow the renderer's `cursor - scroll` slot math, and a scroll
    /// stranded past the end of a shrunken list.
    pub fn clamp_after_rebuild(&self, nav: &mut NavState) {
        let len = self.rows.len();
        if len == 0 {
            *nav = NavState::default();
            // A pinned tab with no rows parks focus on the selector.
            nav.pinned_focus = self.pinned;
            return;
        }
        // Focus already on the pinned slot survives a rebuild (only the
        // scroll needs re-clamping below the selector).
        if self.pinned && nav.pinned_focus {
            nav.scroll = nav.scroll.min(len.saturating_sub(self.page));
            return;
        }
        let mut cursor = nav.cursor.min(len - 1);
        if !self.rows[cursor].selectable() {
            let down = (cursor + 1..len).find(|&i| self.rows[i].selectable());
            let up = (0..cursor).rev().find(|&i| self.rows[i].selectable());
            match down.or(up) {
                Some(sel) => cursor = sel,
                None => {
                    // Nothing selectable anywhere: a pinned tab parks focus
                    // on the selector (all-greyed session gating).
                    if self.pinned {
                        nav.pinned_focus = true;
                        nav.scroll = nav.scroll.min(len.saturating_sub(self.page));
                        return;
                    }
                }
            }
        }
        nav.cursor = cursor;
        // Scroll: never past the last full page, never below the cursor's page.
        nav.scroll = nav.scroll.min(len.saturating_sub(self.page));
        self.follow_scroll(nav);
    }

    /// Keep the cursor inside the visible window (the old `adjust_scroll`),
    /// and pull any run of UNSELECTABLE rows sitting directly above the
    /// cursor into view with it. Without that pull, a decorative header at
    /// the top of a list (or directly above any row) can never scroll back
    /// on screen once pushed out — the cursor can't rest on it, so plain
    /// cursor-following parks the window one row too low forever (the same
    /// bug the in-game options scroll driver had with decorative headers).
    /// Cursor visibility wins when both can't fit in one page.
    pub fn follow_scroll(&self, nav: &mut NavState) {
        // Top of the unselectable run immediately above the cursor.
        let mut top = nav.cursor.min(self.rows.len().saturating_sub(1));
        while top > 0 && !self.rows[top - 1].selectable() {
            top -= 1;
        }
        if top < nav.scroll {
            nav.scroll = top;
        }
        if nav.cursor < nav.scroll {
            nav.scroll = nav.cursor;
        } else if nav.cursor >= nav.scroll + self.page {
            nav.scroll = nav.cursor + 1 - self.page;
        }
    }

    /// Row indices visible in the current window (renderer maps these onto
    /// its fixed slots).
    pub fn page_window(&self, nav: &NavState) -> std::ops::Range<usize> {
        let start = nav.scroll.min(self.rows.len());
        let end = (start + self.page).min(self.rows.len());
        start..end
    }

    /// 1-based cursor position + total, for the "N/M" indicator.
    pub fn scroll_indicator(&self, nav: &NavState) -> (usize, usize) {
        let len = self.rows.len();
        if len == 0 {
            return (0, 0);
        }
        (nav.cursor.min(len - 1) + 1, len)
    }

    /// Whether the list overflows one page (scrollbar + N/M shown).
    pub fn overflows(&self) -> bool {
        self.rows.len() > self.page
    }
}

// ── Tests (run via scripts/validate_mod_menu.sh) ────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(v: &[(&str, bool)]) -> Vec<ModEntrySnap> {
        v.iter()
            .map(|(id, enabled)| ModEntrySnap {
                id: id.to_string(),
                name: id.to_uppercase(),
                description: format!("{id} desc"),
                enabled: *enabled,
            })
            .collect()
    }

    fn contrib(key: &str, owner: Option<&str>) -> ContributedSnap {
        ContributedSnap {
            key: key.to_string(),
            label: key.to_uppercase(),
            hint: format!("{key} hint"),
            kind: RowKind::Scalar {
                value: 0,
                min: -10,
                max: 10,
                step_fine: 1,
                step_coarse: 5,
                formatted: None,
            },
            owning_mod_id: owner.map(str::to_string),
        }
    }

    fn header(key: &str) -> Row {
        Row {
            key: key.into(),
            label: String::new(),
            description: String::new(),
            kind: RowKind::Header,
            source: RowSource::Contributed,
            greyed: false,
        }
    }

    fn bool_row(key: &str, greyed: bool) -> Row {
        Row {
            key: key.into(),
            label: key.to_uppercase(),
            description: String::new(),
            kind: RowKind::Boolean { value: false },
            source: RowSource::RegistryToggle,
            greyed,
        }
    }

    // ── builders ────────────────────────────────────────────────────

    #[test]
    fn mods_tab_excludes_menu_and_preserves_order() {
        let rows = build_mods_tab(&mods(&[
            ("alpha", true),
            ("mod-menu", true),
            ("beta", false),
        ]));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].key, "alpha");
        assert_eq!(rows[0].kind, RowKind::Boolean { value: true });
        assert_eq!(rows[0].source, RowSource::RegistryToggle);
        assert_eq!(rows[0].description, "alpha desc");
        assert_eq!(rows[1].key, "beta");
        assert_eq!(rows[1].kind, RowKind::Boolean { value: false });
    }

    #[test]
    fn global_tab_groups_by_enabled_owner() {
        let entries = mods(&[("fps", true), ("timing", false), ("quick", true)]);
        let contributed = vec![
            contrib("fps_target", Some("fps")),
            contrib("sound_offset", Some("timing")), // owner disabled → hidden
            contrib("restart_delay", Some("quick")),
            contrib("orphan", None),
            contrib("ghost", Some("not-registered")), // unknown owner → hidden
        ];
        let rows = build_global_tab(&entries, &contributed);
        let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "__header_fps",
                "fps_target",
                "__header_quick",
                "restart_delay",
                "orphan"
            ]
        );
        assert_eq!(rows[0].kind, RowKind::Header);
        assert_eq!(rows[0].label, "FPS");
        assert!(!rows[0].selectable());
        assert_eq!(rows[4].description, "orphan hint");
    }

    #[test]
    fn global_tab_no_header_for_mod_without_rows_and_empty_inputs() {
        let entries = mods(&[("bare", true)]);
        assert!(build_global_tab(&entries, &[]).is_empty());
        assert!(build_global_tab(&[], &[contrib("x", Some("bare"))]).is_empty());
    }

    // ── navigation ──────────────────────────────────────────────────

    #[test]
    fn navigation_skips_headers_and_greyed_with_wrap() {
        // header, A, greyed, greyed, B, header
        let rows = vec![
            header("__h1"),
            bool_row("a", false),
            bool_row("g1", true),
            bool_row("g2", true),
            bool_row("b", false),
            header("__h2"),
        ];
        let nav_over = Navigator::new(&rows, 12);
        let mut nav = NavState::default();
        nav_over.clamp_after_rebuild(&mut nav);
        assert_eq!(nav.cursor, 1); // snapped off the leading header
        nav_over.down(&mut nav);
        assert_eq!(nav.cursor, 4); // skipped the greyed run
        nav_over.down(&mut nav);
        assert_eq!(nav.cursor, 1); // wrapped past the trailing header
        nav_over.up(&mut nav);
        assert_eq!(nav.cursor, 4); // wrap upward
    }

    #[test]
    fn all_unselectable_parks_and_reports_none() {
        let rows = vec![header("__h"), bool_row("g", true)];
        let nav_over = Navigator::new(&rows, 12);
        let mut nav = NavState {
            cursor: 5,
            scroll: 3,
            ..NavState::default()
        };
        nav_over.clamp_after_rebuild(&mut nav);
        assert!(nav.cursor < rows.len());
        assert_eq!(nav_over.selected(&nav), None);
        let before = nav;
        nav_over.down(&mut nav);
        nav_over.up(&mut nav);
        assert_eq!(nav, before); // no-ops
    }

    #[test]
    fn clamp_snaps_to_nearest_selectable_down_then_up() {
        // A, header, header — cursor on trailing header snaps UP to A.
        let rows = vec![bool_row("a", false), header("__h1"), header("__h2")];
        let nav_over = Navigator::new(&rows, 12);
        let mut nav = NavState {
            cursor: 2,
            scroll: 0,
            ..NavState::default()
        };
        nav_over.clamp_after_rebuild(&mut nav);
        assert_eq!(nav.cursor, 0);

        // header, A — cursor on leading header snaps DOWN to A first.
        let rows = vec![header("__h"), bool_row("a", false)];
        let nav_over = Navigator::new(&rows, 12);
        let mut nav = NavState::default();
        nav_over.clamp_after_rebuild(&mut nav);
        assert_eq!(nav.cursor, 1);
    }

    #[test]
    fn clamp_repairs_stale_scroll_after_shrink() {
        let rows: Vec<Row> = (0..4).map(|i| bool_row(&format!("r{i}"), false)).collect();
        let nav_over = Navigator::new(&rows, 3);
        // Cursor and scroll both point far past the shrunken list.
        let mut nav = NavState {
            cursor: 20,
            scroll: 18,
            ..NavState::default()
        };
        nav_over.clamp_after_rebuild(&mut nav);
        assert_eq!(nav.cursor, 3);
        // scroll ≤ cursor (no renderer underflow) and ≤ len - page.
        assert!(nav.scroll <= nav.cursor);
        assert!(nav.scroll <= 1);
        // Empty list resets outright.
        let empty: Vec<Row> = Vec::new();
        let nav_over = Navigator::new(&empty, 3);
        nav_over.clamp_after_rebuild(&mut nav);
        assert_eq!(nav, NavState::default());
    }

    #[test]
    fn scroll_follows_cursor_across_pages() {
        let rows: Vec<Row> = (0..30).map(|i| bool_row(&format!("r{i}"), false)).collect();
        let nav_over = Navigator::new(&rows, 12);
        let mut nav = NavState::default();
        for _ in 0..12 {
            nav_over.down(&mut nav);
        }
        assert_eq!(nav.cursor, 12);
        assert_eq!(nav.scroll, 1); // window slid by one
        assert_eq!(nav_over.page_window(&nav), 1..13);
        // Wrap from the bottom returns the window to the top.
        nav.cursor = 29;
        nav_over.follow_scroll(&mut nav);
        assert_eq!(nav.scroll, 18);
        nav_over.down(&mut nav);
        assert_eq!(nav.cursor, 0);
        assert_eq!(nav.scroll, 0);
        assert_eq!(nav_over.page_window(&nav), 0..12);
        assert!(nav_over.overflows());
    }

    #[test]
    fn scroll_indicator_bounds() {
        let rows: Vec<Row> = (0..5).map(|i| bool_row(&format!("r{i}"), false)).collect();
        let nav_over = Navigator::new(&rows, 12);
        assert_eq!(nav_over.scroll_indicator(&NavState::default()), (1, 5));
        assert_eq!(
            nav_over.scroll_indicator(&NavState {
                cursor: 4,
                scroll: 0,
                ..NavState::default()
            }),
            (5, 5)
        );
        assert!(!nav_over.overflows());
        let empty: Vec<Row> = Vec::new();
        assert_eq!(
            Navigator::new(&empty, 12).scroll_indicator(&NavState::default()),
            (0, 0)
        );
    }

    #[test]
    fn tab_nav_memory_and_wrap() {
        let mut tn = TabNav::new();
        assert_eq!(tn.active(), TabId::Mods);
        tn.state_mut().cursor = 7;
        tn.state_mut().scroll = 3;
        tn.switch_next();
        assert_eq!(tn.active(), TabId::GlobalSettings);
        assert_eq!(tn.state(), NavState::default());
        tn.state_mut().cursor = 2;
        tn.switch_next();
        assert_eq!(tn.active(), TabId::PlayerSettings);
        assert_eq!(tn.state(), NavState::default());
        tn.switch_next();
        assert_eq!(tn.active(), TabId::Theme);
        assert_eq!(tn.state(), NavState::default());
        tn.switch_next(); // wraps back to Mods
        assert_eq!(tn.active(), TabId::Mods);
        assert_eq!(
            tn.state(),
            NavState {
                cursor: 7,
                scroll: 3,
                ..NavState::default()
            }
        );
        tn.switch_prev(); // wraps to the last tab
        assert_eq!(tn.active(), TabId::Theme);
        tn.switch_prev();
        assert_eq!(tn.active(), TabId::PlayerSettings);
        tn.switch_prev();
        assert_eq!(tn.active(), TabId::GlobalSettings);
        assert_eq!(tn.state().cursor, 2, "per-tab memory survives the loop");
        tn.reset();
        assert_eq!(tn.active(), TabId::Mods);
        assert_eq!(tn.state(), NavState::default());
    }

    #[test]
    fn leading_header_scrolls_back_into_view() {
        // Header at index 0, then 20 selectable rows, page of 4.
        let mut rows = vec![header("__h")];
        rows.extend((0..20).map(|i| bool_row(&format!("r{i}"), false)));
        let nav_over = Navigator::new(&rows, 4);
        let mut nav = NavState::default();
        nav_over.clamp_after_rebuild(&mut nav);
        assert_eq!((nav.cursor, nav.scroll), (1, 0)); // header visible at open
                                                      // Scroll deep enough to push the header out.
        for _ in 0..12 {
            nav_over.down(&mut nav);
        }
        assert!(nav.scroll > 0);
        // Come back to the first selectable row (via wrap or ups) — the
        // header above it must be pulled back into view.
        while nav.cursor != 1 {
            nav_over.up(&mut nav);
        }
        assert_eq!(nav.scroll, 0, "leading header must scroll back into view");
        // Same via wrap-around from the bottom.
        nav_over.up(&mut nav); // wraps to the last selectable
        assert_eq!(nav.cursor, 20);
        nav_over.down(&mut nav); // wraps to the first selectable
        assert_eq!((nav.cursor, nav.scroll), (1, 0));
    }

    #[test]
    fn mid_list_header_scrolls_back_into_view() {
        // 6 rows, header at index 3, page of 3.
        let rows = vec![
            bool_row("a", false),
            bool_row("b", false),
            bool_row("c", false),
            header("__h"),
            bool_row("d", false),
            bool_row("e", false),
        ];
        let nav_over = Navigator::new(&rows, 3);
        let mut nav = NavState {
            cursor: 5,
            scroll: 3,
            ..NavState::default()
        };
        // Scrolling up to "d" (index 4) must reveal the header at 3 with it.
        nav_over.up(&mut nav);
        assert_eq!(nav.cursor, 4);
        assert_eq!(nav.scroll, 3);
        nav_over.up(&mut nav); // "c" (2)
        assert_eq!(nav.cursor, 2);
        assert!(nav.scroll <= 2);
    }

    #[test]
    fn tab_labels_stable() {
        assert_eq!(TabId::Mods.label(), "TOGGLE MODS");
        assert_eq!(TabId::GlobalSettings.label(), "GLOBAL SETTINGS");
        assert_eq!(TabId::PlayerSettings.label(), "PLAYER SETTINGS");
        assert_eq!(TabId::Theme.label(), "APPEARANCE");
        assert_eq!(TabId::ALL.len(), 4);
    }

    // ── PLAYER SETTINGS builder ──────────────────────────────────────

    fn snap(id: &str, kind: MirroredKindSnap, visible: bool) -> MirroredRowSnap {
        MirroredRowSnap {
            id: id.to_string(),
            display_name: id.to_uppercase(),
            description: format!("{id} desc"),
            kind,
            visible,
        }
    }

    #[test]
    fn player_tab_builder_maps_kinds_and_omits_invisible() {
        let rows = vec![
            snap("hdr", MirroredKindSnap::Header, true),
            snap("toggle", MirroredKindSnap::Bool { value: true }, true),
            snap(
                "hidden_child",
                MirroredKindSnap::Bool { value: false },
                false,
            ),
            snap(
                "mode",
                MirroredKindSnap::Enum {
                    index: 1,
                    values: vec![0, 1],
                    labels: vec!["Off".into(), "Dark".into()],
                },
                true,
            ),
            snap(
                "speed",
                MirroredKindSnap::Scalar {
                    value: 90,
                    min: 25,
                    max: 175,
                    step_fine: 5,
                    step_coarse: 10,
                    formatted: "90%".into(),
                },
                true,
            ),
        ];
        let built = build_player_tab(&rows, true);
        let keys: Vec<&str> = built.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(
            keys,
            vec!["hdr", "toggle", "mode", "speed"],
            "invisible omitted"
        );
        assert!(built.iter().all(|r| r.source == RowSource::Mirrored));
        assert!(built.iter().all(|r| !r.greyed), "editable ⇒ nothing greyed");
        assert_eq!(built[0].kind, RowKind::Header);
        assert_eq!(built[1].kind, RowKind::Boolean { value: true });
        assert_eq!(built[1].label, "TOGGLE");
        assert_eq!(built[1].description, "toggle desc");
        let RowKind::Scalar { formatted, .. } = &built[3].kind else {
            panic!("expected scalar");
        };
        assert_eq!(formatted.as_deref(), Some("90%"), "formatted carried");
    }

    #[test]
    fn player_tab_builder_greys_non_headers_when_not_editable() {
        let rows = vec![
            snap("hdr", MirroredKindSnap::Header, true),
            snap("toggle", MirroredKindSnap::Bool { value: false }, true),
        ];
        let built = build_player_tab(&rows, false);
        assert!(!built[0].greyed, "headers stay ungreyed (never selectable)");
        assert!(built[1].greyed, "session-gated rows grey");
        assert!(built.iter().all(|r| !r.selectable()), "nothing selectable");
    }

    // ── Session gating / side selection ──────────────────────────────

    #[test]
    fn eligibility_fail_closed_and_attract_band() {
        assert_eq!(editable_sides([None, None], false), [false, false]);
        assert_eq!(editable_sides([Some(true), None], false), [true, false]);
        assert_eq!(
            editable_sides([Some(true), Some(true)], true),
            [false, false],
            "attract band gates everything"
        );
        assert_eq!(
            editable_sides([Some(false), Some(true)], false),
            [false, true]
        );
    }

    #[test]
    fn side_resolution_and_selector_states() {
        // Desired side editable ⇒ kept.
        assert_eq!(resolve_selected_side(1, [true, true]), 1);
        // Desired gated, other editable ⇒ snaps to the editable one.
        assert_eq!(resolve_selected_side(0, [false, true]), 1);
        assert_eq!(resolve_selected_side(1, [true, false]), 0);
        // Nothing editable ⇒ desired unchanged (display-only).
        assert_eq!(resolve_selected_side(1, [false, false]), 1);
        // Out-of-range desired clamps to side 1.
        assert_eq!(resolve_selected_side(7, [true, true]), 1);

        assert_eq!(selector_state([true, true]), SelectorState::Free);
        assert_eq!(selector_state([true, false]), SelectorState::Locked);
        assert_eq!(selector_state([false, true]), SelectorState::Locked);
        assert_eq!(selector_state([false, false]), SelectorState::AllGated);
    }

    // ── Pinned-slot navigation ───────────────────────────────────────

    #[test]
    fn pinned_focus_wrap_cycle() {
        // Leading header so "first selectable" ≠ row 0.
        let rows = vec![header("__h"), bool_row("a", false), bool_row("b", false)];
        let nav_over = Navigator::new_with_pinned(&rows, 12, true);
        let mut nav = NavState::default();
        nav_over.clamp_after_rebuild(&mut nav);
        assert_eq!(
            nav_over.selected(&nav),
            Some(1),
            "starts on first selectable"
        );

        nav_over.up(&mut nav); // first selectable → selector
        assert!(nav_over.pinned_focused(&nav));
        assert_eq!(nav_over.selected(&nav), None, "selector selects nothing");

        nav_over.up(&mut nav); // selector → last selectable (wrap)
        assert!(!nav_over.pinned_focused(&nav));
        assert_eq!(nav_over.selected(&nav), Some(2));

        nav_over.down(&mut nav); // last selectable → selector (wrap)
        assert!(nav_over.pinned_focused(&nav));

        nav_over.down(&mut nav); // selector → first selectable
        assert_eq!(nav_over.selected(&nav), Some(1));
    }

    #[test]
    fn pinned_all_greyed_parks_on_selector() {
        let rows = vec![header("__h"), bool_row("a", true), bool_row("b", true)];
        let nav_over = Navigator::new_with_pinned(&rows, 12, true);
        let mut nav = NavState::default();
        nav_over.clamp_after_rebuild(&mut nav);
        assert!(
            nav_over.pinned_focused(&nav),
            "all-greyed parks on selector"
        );
        nav_over.up(&mut nav);
        assert!(
            nav_over.pinned_focused(&nav),
            "no selectable rows to leave to"
        );
        nav_over.down(&mut nav);
        assert!(nav_over.pinned_focused(&nav));

        // Empty list likewise.
        let empty: Vec<Row> = Vec::new();
        let nav_over = Navigator::new_with_pinned(&empty, 12, true);
        let mut nav = NavState::default();
        nav_over.clamp_after_rebuild(&mut nav);
        assert!(nav_over.pinned_focused(&nav));
    }

    #[test]
    fn pinned_focus_survives_rebuild_and_non_pinned_is_unchanged() {
        let rows = vec![bool_row("a", false)];
        let nav_over = Navigator::new_with_pinned(&rows, 12, true);
        let mut nav = NavState::default();
        nav_over.up(&mut nav); // onto the selector
        assert!(nav_over.pinned_focused(&nav));
        nav_over.clamp_after_rebuild(&mut nav);
        assert!(
            nav_over.pinned_focused(&nav),
            "rebuild keeps selector focus"
        );

        // A non-pinned navigator never reports or enters pinned focus.
        let nav_over = Navigator::new(&rows, 12);
        let mut nav = NavState::default();
        nav_over.up(&mut nav);
        nav_over.up(&mut nav);
        assert!(!nav_over.pinned_focused(&nav));
        assert_eq!(nav_over.selected(&nav), Some(0));
    }

    // ── THEME builder ────────────────────────────────────────────────

    #[test]
    fn theme_tab_rows() {
        let labels: Vec<String> = ["RHYTHM", "BUBBLES", "WAVEFIELD", "MINIMAL"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let rows = build_theme_tab(2, &labels, true, false, 80);
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.source == RowSource::Theme));
        assert!(rows.iter().all(|r| !r.greyed));

        assert_eq!(rows[0].key, THEME_ROW_KEY);
        assert_eq!(rows[0].label, "Theme");
        match &rows[0].kind {
            RowKind::Enum {
                index,
                values,
                labels: l,
            } => {
                assert_eq!(*index, 2);
                assert_eq!(values, &[0, 1, 2, 3]);
                assert_eq!(l, &labels);
            }
            other => panic!("theme row kind {other:?}"),
        }

        assert_eq!(rows[1].key, ANIMATE_ROW_KEY);
        assert_eq!(rows[1].label, "Animated Background");
        assert_eq!(rows[1].kind, RowKind::Boolean { value: true });

        assert_eq!(rows[2].key, OPACITY_ROW_KEY);
        assert_eq!(rows[2].label, "Menu Opacity");
        match &rows[2].kind {
            RowKind::Scalar {
                value,
                min,
                max,
                step_fine,
                step_coarse,
                formatted,
            } => {
                assert_eq!(
                    (*value, *min, *max, *step_fine, *step_coarse),
                    (80, 25, 100, 5, 10)
                );
                assert_eq!(formatted.as_deref(), Some("80%"));
            }
            other => panic!("opacity row kind {other:?}"),
        }
    }

    #[test]
    fn theme_tab_animate_greyed() {
        let labels = vec!["RHYTHM".to_string()];
        let rows = build_theme_tab(0, &labels, false, true, 25);
        assert!(!rows[0].greyed);
        assert!(rows[1].greyed, "only the animate row greys");
        assert!(!rows[2].greyed);
        assert_eq!(rows[1].kind, RowKind::Boolean { value: false });
        // Out-of-range theme index clamps into the label table.
        let rows = build_theme_tab(9, &labels, false, false, 25);
        match &rows[0].kind {
            RowKind::Enum { index, .. } => assert_eq!(*index, 0),
            other => panic!("theme row kind {other:?}"),
        }
    }
}
