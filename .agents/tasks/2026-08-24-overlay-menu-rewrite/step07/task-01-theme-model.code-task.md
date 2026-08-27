# Task: Theme model — theme table, palettes, TabId::Theme, theme-tab builder

## Description
Create the mod-menu's pure theme layer: a new dependency-light
`src/mods/mod_menu/theme.rs` holding the `Theme`/`Palette`/`Background`
types and the four built-in themes (palettes authored here, all
backgrounds `Static` until Step 8), plus the model-side additions —
`TabId::Theme` and the pure `build_theme_tab` row builder. All
host-tested via `scripts/validate_mod_menu.sh` (theme.rs joins the
harness MODULES).

## Background
Step 7 of the overlay-menu rewrite (design §4.6, §4.4). Approved
decisions (2026-08-25): 2-task split (this pure layer + integration);
agent authors concrete palette RGBA values (maintainer tunes at the
demo); ANIMATED BACKGROUND defaults ON (inert until Step 8); the
display-string sweep stays in Step 9.

Current facts (verified 2026-08-25):
- `TabId` at model.rs:19-23 (`ALL` :26, `label()` :28-34,
  `index/next/prev` auto-extend via ALL). Tests hard-asserting 3 tabs:
  `tab_labels_stable` (model.rs:940, `ALL.len() == 3`) and
  `tab_nav_memory_and_wrap` (model.rs:850-880, walks the wrap cycle) —
  both must be updated for the 4-tab cycle.
- `RowSource::Theme` already exists as a unit variant (model.rs:83-92);
  input.rs has the matching stub (`RowSource::Theme => {}`, input.rs:309).
- `RowKind::Scalar` carries `formatted: Option<String>` (rendered
  verbatim when `Some`) — use it for the opacity row's `NN%` text.
- `chrome::PanelGradient { top: [u8;4], bottom: [u8;4] }` (chrome.rs:73-76)
  is public; stop alphas are ignored (panel alpha comes from the opacity
  param). `chrome::DEFAULT_GRADIENT` (chrome.rs:80-83) is the pre-theme
  placeholder this task's table supersedes.
- Opacity bounds/steps already exist: `chrome::OPACITY_MIN/MAX/STEP`
  (25/100/5) and `chrome::clamp_opacity`.
- Harness: `scripts/validate_mod_menu.sh` mounts `MODULES=(model.rs
  chrome.rs)` (line 27) as SIBLING modules under a temp crate root —
  `super::chrome::...` from theme.rs resolves correctly in BOTH the real
  crate (siblings under `mod_menu`) and the harness (siblings under the
  generated lib root). theme.rs may therefore reference `super::chrome`
  but nothing else outside itself (no `crate::` paths, no logging).
- tabs.rs builds `tab_rows` via an exhaustive match over `TabId::ALL`
  (tabs.rs:70-77) — adding the variant forces an arm; land a temporary
  `TabId::Theme => Vec::new()` arm there in THIS task to keep the crate
  green (task-02 replaces it).
- render.rs `TEXT_WIDGET_COUNT` (:179) auto-grows with `ALL.len()`; the
  tab bar geometry fits a 4th tab (TAB_X0 + 3·260 = 880 < panel right
  edge) — no render change needed in this task.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-24-overlay-menu-rewrite/design/detailed-design.md (§4.6 theme system, §4.4 configuration, §6 error ladder)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. **`src/mods/mod_menu/theme.rs`** (new, pure — only `super::chrome`
   allowed as an import):
   - `pub struct Palette` with one field per render.rs color/tint use.
     Text colors as `[f32; 3]`: `title`, `tab_active`, `tab_inactive`,
     `header`, `label`, `value`, `greyed`, `on_value`, `off_value`,
     `footer`, `hints`. Tint RGBs as `[u8; 3]` (render keeps its fixed
     alphas): `accent` (selection bar + tab underline + cursor),
     `header_bar`, `banner_back`. Panel gradient stops as
     `panel_top: [u8; 4]`, `panel_bottom: [u8; 4]`.
   - `impl Palette { pub fn gradient(&self) -> super::chrome::PanelGradient }`.
   - `pub struct Theme { pub id: &'static str, pub display: &'static str,
     pub palette: Palette, pub background: Background }` and
     `pub enum Background { Static }` (the `Shader { program }` variant
     arrives with Step 8 — a doc comment notes it).
   - `pub const THEMES: &[Theme]` with exactly these four entries, in
     this order (authored values below — maintainer tunes at the demo):
     - `arrows` / `"RHYTHM"` (deep blue/purple, cyan accent; ~the
       current look's structure): panel_top `[26,22,52,255]`,
       panel_bottom `[10,8,24,255]`, accent `[80,220,255]`, tab_active
       `[0.5,0.9,1.0]`, header `[0.45,0.85,1.0]`, header_bar
       `[80,180,255]`, banner_back `[14,12,30]`.
     - `bubbles` / `"BUBBLES"` (dark teal, warm accent): panel_top
       `[10,40,44,255]`, panel_bottom `[4,16,20,255]`, accent
       `[255,170,80]`, tab_active `[1.0,0.7,0.35]`, header
       `[0.4,0.85,0.8]`, header_bar `[70,200,190]`, banner_back
       `[8,22,24]`.
     - `wavefield` / `"WAVEFIELD"` (charcoal, green accent): panel_top
       `[30,32,34,255]`, panel_bottom `[12,13,14,255]`, accent
       `[90,235,120]`, tab_active `[0.4,0.95,0.5]`, header
       `[0.55,0.9,0.6]`, header_bar `[90,220,120]`, banner_back
       `[16,18,18]`.
     - `minimal` / `"MINIMAL"` (neutral dark grey, white accent):
       panel_top `[34,34,38,255]`, panel_bottom `[14,14,16,255]`,
       accent `[255,255,255]`, tab_active `[1.0,1.0,1.0]`, header
       `[0.85,0.85,0.85]`, header_bar `[200,200,200]`, banner_back
       `[18,18,20]`.
     - Shared across all four (kept as per-theme fields regardless):
       title/label/value `[1.0,1.0,1.0]`, tab_inactive/hints
       `[0.55,0.55,0.55]`, greyed `[0.45,0.45,0.45]`, on_value
       `[0.2,1.0,0.2]`, off_value `[1.0,0.3,0.3]`, footer
       `[0.75,0.75,0.75]`.
   - `pub const DEFAULT_THEME_INDEX: usize = 0` (arrows).
   - `pub fn resolve_theme_index(id: Option<&str>) -> (usize, bool)` —
     `(index, known)`: `None` ⇒ `(DEFAULT_THEME_INDEX, true)`; a
     matching id ⇒ `(index, true)`; unknown ⇒
     `(DEFAULT_THEME_INDEX, false)` so the (impure) caller can WARN once.
   - `pub fn theme(index: usize) -> &'static Theme` — index clamped to
     the table (never panics).
2. **model.rs — `TabId::Theme`**: new variant appended after
   `PlayerSettings`; `ALL` gains it (order: Mods, GlobalSettings,
   PlayerSettings, Theme); `label()` arm `"THEME"`. Update
   `tab_labels_stable` (len 4 + new label assert) and
   `tab_nav_memory_and_wrap` (4-tab wrap cycle).
3. **model.rs — `build_theme_tab`**: pure builder over plain inputs
   (model stays dependency-free — labels arrive as data, not via
   theme.rs):
   `pub fn build_theme_tab(theme_index: usize, theme_labels: &[String],
   animate: bool, animate_greyed: bool, opacity: i32) -> Vec<Row>`
   producing exactly three rows in order:
   - key `"theme"`, label `"THEME"`, `RowKind::Enum { index:
     theme_index, values: (0..n as i32).collect(), labels:
     theme_labels.to_vec() }`, description
     `"Menu color scheme and background style"`.
   - key `"animate_bg"`, label `"ANIMATED BACKGROUND"`,
     `RowKind::Boolean { value: animate }`, `greyed: animate_greyed`
     (always `false` in Step 7; Step 8's availability gate feeds it),
     description `"Animated shader background behind the menu (requires
     the Shader Fixes mod)"`.
   - key `"opacity"`, label `"MENU OPACITY"`, `RowKind::Scalar { value:
     opacity, min: 25, max: 100, step_fine: 5, step_coarse: 10,
     formatted: Some(format!("{opacity}%")) }`, description
     `"Menu panel opacity"`.
   All three: `source: RowSource::Theme`.
4. **tabs.rs temporary arm**: `TabId::Theme => Vec::new()` with a
   `// task-02 replaces this` comment (keeps the crate compiling).
5. **Harness**: add `theme.rs` to the MODULES array in
   `scripts/validate_mod_menu.sh`.
6. **Host tests** (theme.rs + model.rs, red-first where practical):
   - Table integrity: exactly 4 themes; ids and displays unique; every
     id is stem-charset-safe (lowercase ASCII alphanumeric/underscore —
     the engine texture-name rule chrome.rs tests enforce for stems);
     every gradient non-degenerate (top ≠ bottom); `gradient()` maps
     stops through to `PanelGradient` fields.
   - Resolution: `None` ⇒ default/known; each id round-trips to its
     index; `"bogus"` ⇒ `(0, false)`; `theme(usize::MAX)` clamps.
   - Builder: three rows in order with the exact keys/kinds; enum
     labels/values sized to the table; formatted opacity text; animate
     greyed passthrough; all sources `Theme`.
   - Tab model: updated `tab_labels_stable` + `tab_nav_memory_and_wrap`
     pass with 4 tabs; the remaining 18 model tests pass unchanged.

## Dependencies
- None beyond the existing mod_menu module split (Steps 1–6, all landed).

## Implementation Approach
1. theme.rs types + table + resolution fns with tests (red first).
2. model.rs TabId variant + test updates; temporary tabs.rs arm.
3. build_theme_tab + tests.
4. Harness MODULES update; run `./scripts/validate_mod_menu.sh`.
5. Gates: `cargo check --target x86_64-pc-windows-msvc` → `cargo fmt`
   (bare) → `./build.sh`.

## Acceptance Criteria

1. **Theme table integrity**
   - Given `theme::THEMES`
   - When inspected by the integrity tests
   - Then 4 entries (arrows/bubbles/wavefield/minimal) with unique
     stem-safe ids, unique displays, fully populated palettes, and
     distinct gradient stops.

2. **Unknown theme falls back**
   - Given `resolve_theme_index(Some("bogus"))`
   - When resolved
   - Then `(DEFAULT_THEME_INDEX, false)` — the caller can WARN and
     proceed with RHYTHM.

3. **Theme tab rows**
   - Given `build_theme_tab(2, &labels, true, false, 80)`
   - When built
   - Then exactly [THEME enum @ index 2, ANIMATED BACKGROUND bool ON,
     MENU OPACITY scalar 80 formatted "80%"], all `RowSource::Theme`,
     none greyed.

4. **Four-tab model**
   - Given `TabId::ALL`
   - When iterated / wrapped
   - Then MODS → GLOBAL SETTINGS → PLAYER SETTINGS → THEME → wrap; nav
     memory auto-sizes; the full harness passes
     (`./scripts/validate_mod_menu.sh`).

## Metadata
- **Complexity**: Medium
- **Labels**: mod-menu, pure-layer, theme, model
- **Required Skills**: Rust, repo host-test harness conventions
- **Generated By**: code-task-generator 2026-08-25
- **Source Plan**: .agents/planning/2026-08-24-overlay-menu-rewrite/implementation/plan.md
- **Plan Step**: Step 7: THEME tab — theme system with static backgrounds
