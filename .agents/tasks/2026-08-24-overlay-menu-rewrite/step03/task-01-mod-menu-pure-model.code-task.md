# Task: Mod-menu pure model — rows, tabs, navigation

## Description
Create the pure, host-testable model layer for the rewritten mod menu:
the unified `Row` model, the MODS and GLOBAL SETTINGS tab list builders, and the
navigation state machine (cursor with header/greyed skip, wrap, scroll clamping,
per-tab cursor memory). Zero game dependencies; tested via a new
`scripts/validate_mod_menu.sh` temp-crate harness.

## Background
The overlay menu is being rebuilt as a four-tab modal (design §4.1–§4.2). This task
lays the model the shell (task-02) renders. Inputs are plain-data snapshots so the
builders and navigation are pure: registry mod entries (id/name/description/enabled)
and contributed rows (the existing `ScalarRowSpec`/`EnumRowSpec` registrations, whose
`parent_row_key` is reinterpreted as OWNING MOD ID per design §4.2). PLAYER SETTINGS
and THEME tabs arrive in later steps — the model must make adding a tab a matter of
adding a builder (the `TabId` enum + per-tab `TabState` are general from day one).

Current-code touchpoints to read first: `src/mods/mod_menu/rows.rs` (MenuRow/RowKind,
rebuild_rows grouping, visible_when), `src/mods/mod_menu/render.rs` (adjust_scroll,
selected_row_index — their clamp/underflow guards become test cases),
`src/mods/mod_menu/input.rs` (navigation entry points).

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-24-overlay-menu-rewrite/design/detailed-design.md (§4.2 row model, §4.8 input, FR-1/2/3/12/13)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. New `src/mods/mod_menu/model.rs`, dependency-free (no `crate::` imports), holding:
   - `Row { key, label, description, kind: RowKind, source: RowSource, greyed }` and
     `RowKind { Boolean{value}, Scalar{value,min,max,step_fine,step_coarse},
     Enum{index,values,labels}, Header }` per design §5 (RowSource callbacks stay in
     the impure layer — the model's `RowSource` is a plain discriminant:
     `RegistryToggle | Contributed | Mirrored | Theme`).
   - `TabId { Mods, GlobalSettings }` (extensible enum — later steps append variants)
     with display labels ("MODS", "GLOBAL SETTINGS").
   - Snapshot input types: `ModEntrySnap { id, name, description, enabled }` and
     `ContributedSnap { key, label, hint, kind, owning_mod_id: Option<String> }`.
   - `build_mods_tab(&[ModEntrySnap]) -> Vec<Row>` — one Boolean row per mod
     (excluding `mod-menu`), registration order (FR-2).
   - `build_global_tab(&[ModEntrySnap], &[ContributedSnap]) -> Vec<Row>` — for each
     enabled mod owning ≥1 contributed row: a `Header` row (mod name) then its rows in
     contributed order; disabled mods' groups hidden; unowned rows (no
     `owning_mod_id`) appended at the end without a header (FR-3).
   - `NavState` per tab: `{ cursor: usize, scroll: usize }` + a `Navigator` over
     `(rows, visible_rows_per_page)` implementing: up/down with wrap that SKIPS
     `Header` and `greyed` rows (an all-unselectable list parks cursor at 0, selection
     reported None); scroll follow (the old adjust_scroll semantics incl. the
     stale-high scroll_offset underflow guard); clamping after list shrink; and
     per-tab cursor memory (switching tabs preserves each tab's NavState within one
     open; reset on close) (FR-12/13).
   - `scroll_indicator(rows_len, cursor, page) -> (pos_1based, total)` for the "N/M"
     display, and a `page_window(scroll, page, len)` helper the renderer maps slots
     from.
2. Host tests in-module covering: MODS list construction (exclusion, order); GLOBAL
   grouping matrix (enabled/disabled owners, unowned tail, empty groups); navigation
   skip/wrap (header at ends, greyed runs, all-greyed); scroll clamp cases (encode the
   underflow-guard comments from the old code as tests); tab cursor memory; indicator
   math at boundaries.
3. New `scripts/validate_mod_menu.sh` mounting `model.rs` (validate_overlay_draw.sh
   pattern).
4. Wire `pub(super) mod model;` into `src/mods/mod_menu/mod.rs` — no consumer changes
   yet (task-02 integrates); crate still builds for the msvc target.

## Dependencies
- Step 1's module layout (present). No dependency on Step 2.

## Implementation Approach
1. Read the three touchpoint files; transcribe the guard semantics into test cases
   first.
2. Implement model + tests; iterate under `./scripts/validate_mod_menu.sh`.
3. Gates: harness green → `cargo check` → `cargo fmt` → `./build.sh`.

## Acceptance Criteria

1. **MODS builder**
   - Given mod entries incl. `mod-menu` and a disabled mod
   - When `build_mods_tab` runs
   - Then `mod-menu` is absent, all others present in input order as Boolean rows
     carrying name/description.

2. **GLOBAL grouping**
   - Given contributed rows owned by an enabled mod, a disabled mod, and one unowned
   - When `build_global_tab` runs
   - Then the enabled mod's header+rows appear in order, the disabled mod's group is
     absent entirely, and the unowned row trails without a header.

3. **Navigation skip/wrap/memory**
   - Given a row list with headers at both ends and a greyed run in the middle
   - When navigating up/down across the ends and switching tabs and back
   - Then the cursor never rests on a header/greyed row, wraps correctly, and each
     tab's cursor/scroll position is restored.

4. **Host-testable purity**
   - Given the module
   - When `./scripts/validate_mod_menu.sh` runs on the host
   - Then all tests compile and pass with no game/hook dependency.

## Metadata
- **Complexity**: Medium
- **Labels**: mod-menu, pure-layer, model
- **Required Skills**: Rust, repo host-test harness conventions
- **Generated By**: code-task-generator 2026-08-24
- **Source Plan**: .agents/planning/2026-08-24-overlay-menu-rewrite/implementation/plan.md
- **Plan Step**: Step 3: Tabbed shell — row model, MODS + GLOBAL SETTINGS tabs, dense layout
