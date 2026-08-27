# Task: Player-tab model — TabId, pinned selector navigation, builder, eligibility

## Description
Extend the mod-menu's pure model layer for the PLAYER SETTINGS tab:
`TabId::PlayerSettings`; a pinned-slot navigation extension (the
`CONFIGURING: PLAYER 1/2` selector sits ABOVE the scroll region and is
reachable by cursor without scrolling away); `RowKind::Scalar.formatted`
(display parity for mirrored scalars); the pure `build_player_tab` list
builder over snapshot-shaped inputs; and the pure session-gating/eligibility
matrix (editable sides, default-side resolution, selector lock states —
fail-closed). All host-tested via `scripts/validate_mod_menu.sh`.

## Background
Step 6 of the overlay-menu rewrite (design §4.2/§4.8/§4.9, FR-4/FR-5).
Approved decisions (2026-08-25): PLAYER-tab-conditional layout (selector
takes the first row band; 11 content rows on that tab, 12 elsewhere);
ShowWhen-invisible rows OMITTED (matches in-game); `formatted:
Option<String>` added to model `RowKind::Scalar`; pinned-slot focus model
(UP from the top row and wrap-from-bottom focus the selector; LEFT/RIGHT
there switch among editable sides; DOWN returns to the list); observer
repaint coalesced (task-02).

Current model facts (verified):
- `TabId` at model.rs:18-22 (+`ALL` :25, `label()` :27-32, `index/next/prev`
  :34-44 auto-extend); `TabNav::new` sizes states by `TabId::ALL` (:220);
  test `tab_labels_stable` asserts `ALL.len() == 2` (:694) — update to 3.
- `Row { key, label, description, kind, source, greyed }` (:89-99);
  `Row::selectable()` (:103-105); `RowSource::Mirrored` unit variant (:82,
  Copy — the row's `key` carries the option id per :90-92).
- `RowKind::Scalar { value, min, max, step_fine, step_coarse }` (:56-62) —
  no formatted field yet.
- `Navigator` (:252-376): `new(rows, page)`, `selected`, `up/down` (wrap +
  skip unselectable), `clamp_after_rebuild`, `follow_scroll`, `page_window`,
  `scroll_indicator`, `overflows`. `NavState { cursor, scroll }` (:197-200).
- tabs.rs:42-48 builds `tab_rows` via an exhaustive `match` over `ALL` —
  the compiler forces the new arm (integration wires it; THIS task only
  provides the builder fn).

The snapshot input type is `custom_options::OverlayRowInfo { id,
display_name, description, kind: OverlayRowKind{Bool/Enum/Scalar{..,
formatted}/Header}, visible }` — but the model must stay DEPENDENCY-FREE
(harness rule): define model-local input mirrors (like `ModEntrySnap`/
`ContributedSnap`) and let tabs.rs convert.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-24-overlay-menu-rewrite/design/detailed-design.md (§4.2 row model, §4.8 input, §4.9 gating, FR-4/FR-5)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. **TabId**: add `PlayerSettings` variant + `ALL` entry (after
   GlobalSettings, before the future Theme) + `label()` arm
   (`"PLAYER SETTINGS"`); update the `ALL.len()` test.
2. **RowKind::Scalar.formatted**: add `formatted: Option<String>` (render
   shows it verbatim when `Some`, falls back to the existing `+N` text —
   render change is task-02; existing builders/tests set `None`).
3. **Snapshot input mirror** (model-local, plain data): `MirroredRowSnap
   { id, display_name, description, kind: MirroredKindSnap, visible }` with
   `MirroredKindSnap { Bool{value}, Enum{index, values, labels},
   Scalar{value, min, max, step_fine, step_coarse, formatted}, Header }`.
4. **`build_player_tab(rows: &[MirroredRowSnap], editable: bool) ->
   Vec<Row>`**: `visible == false` rows OMITTED; kinds map 1:1 (Bool→Boolean,
   Enum→Enum, Scalar→Scalar carrying `Some(formatted)`, Header→Header);
   `source: Mirrored`; `key` = option id; `label` = display_name;
   `description` = description; `greyed = !editable` on every non-header row
   (headers render as section separators and are never selectable anyway).
5. **Eligibility matrix** (pure): a `SideGate` input pair
   (`entered: [Option<bool>; 2]`, `in_attract_band: bool`) →
   `pub fn editable_sides(...) -> [bool; 2]` where
   `editable[s] = entered[s] == Some(true) && !in_attract_band`
   (fail-closed: `None` ⇒ false); `pub fn resolve_selected_side(desired:
   u8, editable: [bool; 2]) -> u8` (desired if editable; else the single
   editable side; else desired unchanged — display-only when nothing is
   editable); `pub fn selector_state(editable: [bool; 2]) -> SelectorState
   { Free, Locked, AllGated }` (two editable ⇒ Free; one ⇒ Locked; zero ⇒
   AllGated — selector greyed + banner + rows greyed).
6. **Pinned-slot navigation**: extend the navigation model so the PLAYER
   tab's cursor can rest on the pinned selector ABOVE the list without the
   selector participating in scrolling:
   - Representation: implementer's choice, but pure and per-tab (suggest a
     `pinned: bool` on `Navigator::new_with_pinned(rows, page, pinned)` plus
     a sentinel cursor value or a `focus: Focus { Pinned, List }` leg in
     NavState — whichever keeps the existing 13 model tests passing
     UNCHANGED for non-pinned tabs).
   - Semantics: UP from the first selectable row focuses the selector; UP
     from the selector wraps to the LAST selectable row; DOWN from the
     selector goes to the first selectable row; DOWN from the last
     selectable row wraps to the selector (i.e. the selector joins the
     wrap cycle at the top). An all-greyed/empty list parks focus on the
     selector. `selected()` returns None while pinned-focused; scrolling
     ignores the pinned slot; `scroll_indicator`/`page_window`/`overflows`
     unchanged.
   - When the tab has no pinned slot the behavior is byte-identical to
     today (existing tests prove it).
7. **Host tests** (in model.rs, run via the harness): builder matrix
   (visible omission, kind mapping incl. formatted passthrough, greyed-all,
   header passthrough); eligibility matrix (None fail-closed, attract band,
   single/dual/zero editable, resolve_selected_side legs, selector states);
   pinned navigation (focus transitions incl. wrap both directions,
   all-greyed parks on selector, selected() None while pinned, non-pinned
   regression = existing suite untouched).

## Dependencies
- Step 5's `overlay_snapshot` shape (mirrored by the model-local snap types;
  no code dependency — the model stays dependency-free).

## Implementation Approach
1. TabId + test updates (mechanical; compiler drives via exhaustive match —
   note tabs.rs will NOT compile until its arm exists, so land a temporary
   `TabId::PlayerSettings => Vec::new()` arm there in THIS task to keep the
   crate green (task-02 replaces it with the real builder call).
2. Kind/snap/builder + eligibility with tests (red first).
3. Pinned navigation extension, keeping the existing tests green.
4. Gates: `./scripts/validate_mod_menu.sh` → `cargo check` → `cargo fmt` →
   `./build.sh`.

## Acceptance Criteria

1. **Tab appears in the model**
   - Given `TabId::ALL`
   - When iterated
   - Then MODS, GLOBAL SETTINGS, PLAYER SETTINGS in that order; nav memory
     auto-sizes; labels stable.

2. **Builder correctness**
   - Given snap rows (visible+invisible, all four kinds, formatted scalar)
     with `editable = false`
   - When `build_player_tab` runs
   - Then invisible rows are absent, kinds map 1:1 with formatted carried,
     and every non-header row is greyed.

3. **Eligibility fail-closed**
   - Given `entered = [None, Some(true)]` outside the attract band
   - When evaluated
   - Then only side 1 is editable; selector state Locked; desired side 0
     resolves to 1.

4. **Pinned navigation**
   - Given a pinned Navigator over a list with a leading header
   - When pressing UP from the first selectable row, UP again, and DOWN
     twice
   - Then focus goes selector → last selectable row (wrap) → selector →
     first selectable row; `selected()` is None while on the selector; the
     13 pre-existing navigation tests pass unchanged.

## Metadata
- **Complexity**: Medium-High
- **Labels**: mod-menu, pure-layer, model, player-tab
- **Required Skills**: Rust, repo host-test harness conventions
- **Generated By**: code-task-generator 2026-08-25
- **Source Plan**: .agents/planning/2026-08-24-overlay-menu-rewrite/implementation/plan.md
- **Plan Step**: Step 6: PLAYER SETTINGS tab — mirroring, side selector, session gating
