# Task: RANDOM badge — `preview/badge.rs` + the generated `preview_random.png`

## Description
When BACKGROUND DANCER / BACKGROUND STAGE is focused with the `RANDOM` value, the preview box shows a
static badge instead of a 3D scene (FR-9, design §4.6 "preview/badge.rs"): one native `ImageWidget` per
side over a repo-shipped 170×150 PNG (`data_mods/background_dancers/tex/preview_random.png`, generated
by a new `scripts/gen_preview_random_badge.py` — dice / "?" art on a dark panel matching the preview
backdrop), positioned over the box's canvas rect, shown iff `focused ∧ value == RANDOM ∧ modal open ∧
!mod_menu::is_open()`. Fail-open: a missing PNG / unavailable `asset_loader` / refused widget ⇒ no badge
(one WARN), the box keeps its chrome.

## Background
Widget precedent: `src/mods/training_mode/scrub_indicator.rs` — `asset_loader::load("./data_mods/<mod>/
tex/<stem>.png", stem)` once (never released; process-lifetime chrome), `asset_loader::resolve(stem)`
polled until it yields a texture handle, then `widget_renderer::create_image_widget(&ImageWidgetConfig {
x, y, width, height, texture_name: None, .. })` + `set_texture_id(handle)`, `show()` / `hide()`, all on
the render thread (`widget_renderer::run_on_render_thread`, which in this codebase is the game thread's
frame pump — the same thread the preview driver's `on_frame` runs on). Art precedent:
`scripts/gen_training_scrub_icons.py` (PIL, 4× supersampled). The badge's canvas rect =
`preview::layout::box_rect(side, marker)` with the marker from `preview_gen::marker_rect_for(id, Green)`
or `FALLBACK_MARKER` — the same rect the 3D box uses, so badge and preview never disagree.

Preview state: `preview::state::SlotState::is_focused()` is true while one of OUR rows is focused (RANDOM
included) and `wanted().is_none()` means RANDOM (or another row when `!is_focused()`); `PreviewSlot.modal_open`
is per side. Z-order: the widget layer renders in RENDER_2D at 0x65–0x67, BELOW the pass clones at
0x68+ — but a RANDOM badge and a live 3D preview are mutually exclusive on a side (the driver disables the
passes when nothing is wanted), so the ordering never matters. The `mod_menu` overlay raises its own
widgets above every mod-owned widget on open; the badge additionally hides itself while it is open.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-21-background-dancers-selection-options/design/detailed-design.md`
  (§4.6 "preview/badge.rs", §4.7 "preview_random.png", §6 fail-open rows, FR-9, FR-11)

**Additional References (if relevant to this task):**
- `.agents/planning/2026-09-21-background-dancers-selection-options/implementation/plan.md` (Step 6 item 1)
- `src/mods/training_mode/scrub_indicator.rs`, `scripts/gen_training_scrub_icons.py`
- `src/mods/background_dancers/preview/{mod.rs, layout.rs, state.rs}` (the driver this plugs into)
- `src/widgets/image_widget.rs`, `src/services/asset_loader.rs`, `src/services/widget_renderer.rs`

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `scripts/gen_preview_random_badge.py`: writes `data_mods/background_dancers/tex/preview_random.png`
   (170×150 RGBA; a rounded dark panel `#0C0C14` matching `layout::BACKDROP_ARGB`, a large centred
   die face or "?" glyph in the option-menu white with a subtle outline, the caption `RANDOM` beneath;
   4× supersampled; no fonts beyond PIL's default / a bundled-free approach — draw the glyph geometrically
   if no TTF is available). Commit the PNG (the DLL loads it at runtime).
2. `src/mods/background_dancers/preview/badge.rs` (engine-facing):
   - `const TEX_PATH: &str = "./data_mods/background_dancers/tex/preview_random.png"; const TEX_STEM: &str =
     "preview_random";` (stem unique in the ResourceManager namespace).
   - `pub struct Badge { widget: Option<ImageWidget>, texture: Option<i32>, load_requested: bool,
     load_failed: bool, shown: bool, rect: Option<CanvasRect> }` with `pub const fn new()`.
   - `pub fn set_visible(&mut self, side: usize, on: bool, rect: CanvasRect)` — game thread (from the
     driver's `on_frame`): when `on`: request the load once (`asset_loader::is_available()` else latch
     failed + one WARN), poll `resolve` until a handle, lazily create the widget at `rect` (`set_texture_id`),
     `set_position/set_size` when the rect changed, `show()`; when `!on`: `hide()` if shown. Idempotent per
     frame (no work when nothing changed).
   - `pub fn destroy(&mut self)` — hide + `widget.destroy()` (mod disable).
3. `preview/mod.rs`: `PreviewSlot.badge: Badge`; in `drive_slot` after the live-window step compute
   `badge_on = slot.modal_open && slot.state.is_focused() && slot.state.wanted().is_none() && !menu_open`
   and call `slot.badge.set_visible(side, badge_on, rect)` where `rect` = `box_rect(side, marker)` (reuse
   `box_for`'s marker lookup — factor a `marker_for(kind)` helper; the RANDOM value carries no kind, so
   read the FOCUSED kind: extend `SlotState` with `focused_kind: Option<I::Kind>`? Simpler: store
   `slot.focused_kind: Option<Kind>` in `on_preview_request` (`Some(kind)` when ours) and clear it in
   `on_clear` paths); `shutdown` destroys both badges; the badge never touches the passes.
   Keep `ACTIVE` true while a badge is shown (it must hide on menu open/close edges) — fold into
   `PreviewSlot::is_idle`.
4. WARN once per boot on: asset loader unavailable, load refused, widget creation refused (`Warn` bits).
5. `cargo check` clean, `cargo fmt`, `./build.sh`.

## Dependencies
- Step 5 (`preview/mod.rs`, `layout::box_rect`, `state::SlotState`).

## Implementation Approach
1. The PNG generator + PNG first (visual check: open the file).
2. `badge.rs` modelled on `scrub_indicator.rs`; then the two driver hooks; then gates.

## Acceptance Criteria

1. **Badge shows for RANDOM**
   - Given the options modal open, BACKGROUND DANCER focused, value `RANDOM`
   - When the row is focused
   - Then `preview_random` appears exactly over the preview box (cabinet), no 3D passes are enabled, and
     the log shows one `WidgetRenderer`/asset resolve line for `preview_random` per boot.

2. **Badge yields to the preview**
   - Given the badge shown
   - When the value changes to `EMI #2`
   - Then the badge hides immediately and the 3D preview starts after the settle; changing back to
     `RANDOM` tears the preview down and the badge returns.

3. **Hidden with the overlay menu / on close**
   - Given the badge shown
   - When the 0-0-0 menu opens, or the modal closes, or song select is left
   - Then the badge hides (and returns after the 0-0-0 menu closes while the row is still focused).

4. **Fail-open**
   - Given the PNG missing from the install
   - When RANDOM is focused
   - Then one WARN, no badge, no crash, the rows keep working.

5. **Build**
   - Given the crate
   - When `cargo check --target x86_64-pc-windows-msvc` and `./build.sh` run
   - Then both are clean.

## Metadata
- **Complexity**: Medium
- **Labels**: background-dancers, preview, widgets, engine-facing
- **Required Skills**: Rust, the widget_renderer / asset_loader pattern, PIL
- **Generated By**: code-task-generator 2026-09-21
- **Source Plan**: `.agents/planning/2026-09-21-background-dancers-selection-options/implementation/plan.md`
- **Plan Step**: Step 6: RANDOM badge, polish, documentation, validation matrix
