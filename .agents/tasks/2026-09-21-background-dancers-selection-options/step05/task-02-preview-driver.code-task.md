# Task: Preview driver — live dancer / stage previews in the options box (per side)

## Description
The engine-facing preview driver (design §4.6, §5.3, §6): `preview/mod.rs` subscribes to
`custom_options::on_preview_request / on_menu_open / on_menu_close`, keeps one `PreviewSlot` per side,
and from the mod's per-frame callback starts / drives / tears down a `PreviewWindow` (a `SceneWindow`
over a stage-only or dancer-only `Pick`, a `Session` with the side's slot base + private pass-mask bit,
no hulls, the synthetic schedule for stage scenes), lazily creates the side's `PassSet` (clear + two pass
clones at the side's priorities / filter bit / box rect), writes the camera each frame
(`preview/camera.rs`: the stage's `.camanm` director sample cropped to the box aspect, the fixed dancer
camera, or the cropped fallback), toggles the passes with `!mod_menu::is_open()`, and tears everything
down on focus loss / modal close / leaving scene 25. Everything fails open (no compositor ⇒ the driver
manages nothing; per-preview failures ⇒ that preview shows nothing, one WARN per class).

## Background
Building blocks (all shipped): `viewport_pass::{is_available, create, PassSet, render_target_dims,
RtRect, ClearSpec}` + `viewport_pass_layout::{FILTER_BIT, PRIO_BASE}` (Step 3; `reap()` is already called
ONCE per frame in `background_dancers/mod.rs` — do not add a call); `scene_window::{load_arcs,
SceneWindow}` (Step 4: `start(tag, scope, pick, loaded, opts)`, `drive_assets(make_session) ->
has_built`, `park_engine_destroyed`, `publish(t, visible)`, `retry_textures`, `attached_diagnostics`,
`begin_teardown(exit_label) -> bool`, `drive_teardown(label) -> bool`, `finish(label)`, `neutralise()`);
`Session::new(pick, parsed, requested_at, tempo_opts, style, hulls, slot_base, item_pass_mask)` +
`with_schedule(schedule::synthetic_schedule(STAGE_CUT_PERIOD_S))`; `director::{produce (via publish),
camera_frame}`; `Pick::{stage_only, dancer_only}`, `ParseOptions::PREVIEW`; `preview::layout` /
`preview::state` (task-01); `camera_math::{view_proj, Frustum}` + `CamSample::frustum()`;
`lifecycle::tables_snapshot()`; `options::{OPT_DANCER, OPT_STAGE, value(kind, side)}`;
`catalog::Catalog::key(kind, value)` (through `options`) ; `style::{effective, tempo_options}`;
`preview_gen::marker_rect_for(id, MarkerColor::Green)`; `mod_menu::is_open()`; `scene::SONG_SELECT`.

Threading: `on_preview_request` / `on_menu_open` / `on_menu_close` fire on the game thread from inside
the options UI — they must only record state (a short `Mutex` hold, no engine calls). ALL engine work —
`PassSet` create / set_rect / set_camera / set_enabled / detach, `load_arcs`, `SceneWindow` phases — runs
from the mod's `input_manager::on_frame` callback (mid-frame; the compositor's contract). `SceneWindow`
teardown needs the scene graph ENABLED (`item_listed` only updates then): it is at scene 25 and stays so
through scenes 26/27 until `DancePlaySequence::onInitialize`, so a teardown begun at scene-25 exit
completes while the gameplay window is still loading — never defer it. Preview nodes are stamped
`FILTER_BIT[side]` so the stock passes never draw them and each clone draws only its side.

Frame-board slots: `Session::new(.., SLOT_BASE[side], Some(FILTER_BIT[side]))` — P1 0.., P2 16..; the
session's budget is what remains of the board (32 − base), which for P1 exceeds the design's 16 — cap the
PICK instead: a stage with more than `MAX_PREVIEW_INSTANCES` parts is truncated to the first 16 rows'
parts with one WARN (the parse then produces ≤ 16 stage parts). Dancer scenes are ≤ 1 + 5 parts.

Camera (design §4.7 amendment): stage ⇒ `director::camera_frame(sess, t)` → `CamSample` (16:9-baked,
symmetric) → replace `l/r` by `±t × box_aspect` (keep `b/t`, eye/target/up/near/far) →
`camera_math::view_proj(&frustum)`; no camera set ⇒ `fallback_extents(box_aspect)` at
`FALLBACK_EYE/TARGET`, near 0.1, far `FALLBACK_FAR`; dancer ⇒ `dancer_extents(box_aspect)` at
`DANCER_EYE/TARGET`, near/far from `layout`. The dancer stands at the origin facing +z (`dancer_x(0, 1)
= 0`). Time base: `t = built_at.elapsed()` seconds (wall clock, `TempoOptions`-free); stage-only sessions
take `tempo_options()` for the `dance_schedule` call (it yields `None` without dancers) and the synthetic
schedule fallback; the camera event loop advances from `t` exactly as in gameplay.

Visibility: the side's passes are ENABLED only while its window `has_built()` ∧ `!mod_menu::is_open()`;
disabled otherwise (a disabled set costs nothing and stays attached across scenes; detached only at
`shutdown`). The clear is `ClearSpec { depth: true, color: Some(BACKDROP_ARGB) }` for both kinds (a dark
backdrop behind the figure; it also hides the chrome where a cropped stage has no geometry).

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-21-background-dancers-selection-options/design/detailed-design.md`
  (§3.1, §4.5 contract, §4.6, §4.7 amendment, §5.3, §6 error table, §7 cabinet items 3–4)

**Additional References (if relevant to this task):**
- `.agents/planning/2026-09-21-background-dancers-selection-options/implementation/plan.md` (Step 5 demo)
- `.agents/planning/2026-09-21-background-dancers-selection-options/research/preview-compositing.md` §1 (thread rules)
- `src/mods/background_dancers/viewport_smoke.rs` (the shipped `PassSet` consumer: rect from the marker,
  `render_target_dims` retry when unreadable, `Frustum::perspective` → `set_camera`)
- `src/mods/background_dancers/lifecycle.rs` (`drive_live`'s use of `SceneWindow`; `window_entry`'s seed
  + `arc_exists` closure), `src/mods/background_dancers/scene_window.rs`
- `src/mods/webui_options/bg_preview_overlay.rs` (`on_menu_open/close/on_preview_request` precedent:
  ENABLED gate, side ≤ 1, state under one lock, close not gated on ENABLED)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `src/mods/background_dancers/preview/mod.rs`:
   - `pub fn init()` (from the mod's `enable`, once — `CALLBACKS_REGISTERED` latch; sets `ENABLED`),
     `pub fn shutdown()` (mod disable: `ENABLED = false`; from the render thread via
     `widget_renderer::run_on_render_thread`: tear down both sides' windows the `neutralise` way — the frame
     callback is gone — and `detach` both `PassSet`s), `pub fn on_scene_change(prev, next)` (leaving
     `scene::SONG_SELECT` ⇒ `on_clear` both sides + request teardown), `pub fn on_frame()` (O(1) idle:
     an `ACTIVE` atomic set by any request / live window).
   - Per side `PreviewSlot { state: SlotState<(Kind, String)>, modal_open: bool, live: Option<PreviewWindow>,
     retiring: Vec<PreviewWindow>, passes: Option<PassSet>, rect: Option<(RtRect, f32 /*aspect*/)>,
     warned: u8 /*bitmask of one-shot WARN classes*/ }` in a `Mutex<[PreviewSlot; 2]>`.
   - Callbacks: `on_menu_open(side)` ⇒ `modal_open = true`; `on_menu_close(side)` (NOT gated on ENABLED)
     ⇒ `modal_open = false`, `state.on_clear()`; `on_preview_request(side, id)`: `ENABLED ∧ side ≤ 1 ∧
     modal_open` else return; `id ∈ {OPT_DANCER, OPT_STAGE}` ⇒ `kind`, `value = options::value(kind,
     side)`, `key = catalog key` (through a new `options::choice_key(kind, side) -> Option<String>` or the
     existing `stage_choice/dancer_choice`), `state.on_request(true, key.map(|k| (kind, k)), now_ms)`; other
     ids ⇒ `state.on_request(false, None, now_ms)`. Set `ACTIVE`.
   - `on_frame` per side: drive `retiring` windows (`drive_teardown("preview teardown")` → `finish("preview ")`);
     `match state.poll(now)`: `Teardown` ⇒ `live.take()` → `begin_teardown("preview exit")` (true ⇒ push to
     `retiring`, else `finish_silent`) → `state.mark_torn_down()`, passes disabled; `Start(id)` (only when
     `retiring` is empty) ⇒ `start_preview(side, id)` → `live = Some(w)`, `state.mark_started(id)`; then drive
     `live`: `drive_assets(make_session)`; when built: `park_engine_destroyed`, `publish(t, true)`, camera →
     `passes.set_camera`, `retry_textures`, `attached_diagnostics`; `passes.set_enabled(has_built &&
     !mod_menu::is_open())`. Residency timeout / abandoned ⇒ `SceneWindow` already logged; the passes stay
     disabled. `ACTIVE = false` when both sides have no live/retiring window and no pending state.
   - `start_preview`: guard `viewport_pass::is_available()` (WARN once per boot on refusal — class bit);
     `tables_snapshot()`; the pick via `Pick::stage_only` / `Pick::dancer_only` (stage parts capped at
     `MAX_PREVIEW_INSTANCES` rows with one WARN); seed = `seed_from(nanos ^ side, 25)`; `pick.seed = seed`;
     INFO `BackgroundDancers: preview P{} -- {}` with `pick.summary()`; ensure the side's `PassSet` (create
     lazily with `FILTER_BIT[side]`, `PRIO_BASE[side]`, the box rect from `marker_rect_for(id, Green)` or
     `FALLBACK_MARKER` × `render_target_dims()`; unreadable dims ⇒ retry next frame, keep `Start` pending
     by not calling `mark_started`), `set_rect` per preview (dims re-read), `set_enabled(false)` until built;
     `load_arcs(&pick, &ParseOptions::PREVIEW)`; `SceneWindow::start(format!("BackgroundDancers: preview
     P{}", side + 1), "this preview", pick, loaded, ParseOptions::PREVIEW)`.
   - `make_session` closure: `Session::new(pick.clone(), parsed, requested_at, style::tempo_options(),
     style::effective().style, HullPlan::none(), SLOT_BASE[side], Some(FILTER_BIT[side]))
     .with_schedule(synthetic_schedule(STAGE_CUT_PERIOD_S)?)` (only when `synthetic_schedule` is `Some`).
   - One-shot WARN classes: compositor unavailable; `graph_stats().enabled == false` at preview start;
     tables unavailable; marker template unreadable (INFO, falls back); catalog key missing.
2. `src/mods/background_dancers/preview/scene.rs`: `pub struct PreviewWindow { pub identity: (Kind, String),
   pub scene: SceneWindow, pub built_at: Option<Instant>, pub seed: u64 }` + `pub fn build_pick(kind, key,
   tables, rng) -> Option<Pick>` (the cap + the two builders) + `pub fn make_session(side, pick, parsed,
   requested_at) -> Session`.
3. `src/mods/background_dancers/preview/camera.rs`: `pub fn frustum_for(window: &mut PreviewWindow, t: f32,
   aspect: f32) -> Frustum` — stage with camera ⇒ `camera_frame` sample cropped (`Extents` from
   `layout::crop_to_aspect(sample.t, aspect)` — note `sample.t` is the vertical half-tangent since `w = 1`);
   stage without ⇒ fallback; dancer ⇒ dancer camera. `pub fn apply(frustum, passes: &mut PassSet)` =
   `view_proj` + `set_camera`.
4. `background_dancers/mod.rs`: `preview::init()` after `options::register(..)` in `enable`;
   `preview::on_scene_change(prev, next)` in the scene callback; `preview::on_frame()` in the frame callback
   BEFORE the single `viewport_pass::reap()`; `preview::shutdown()` in `disable` before `viewport_smoke::shutdown()`.
5. `options.rs`: `pub fn choice_key(kind: Kind, side: u8) -> Option<String>` (the catalog key of the live
   value; `None` for RANDOM / rows down) — the existing `stage_choice`/`dancer_choice` may delegate to it.
6. No new signatures; no engine offsets; every pointer the driver touches is inside `viewport_pass` /
   `scene_window` / `session` (already probed). `cargo check` clean, `cargo fmt`, harness green, `./build.sh`.

## Dependencies
- task-01 of this step (`Pick::{stage_only, dancer_only}`, `preview::{layout, state}`); Steps 3–4.

## Implementation Approach
1. `preview/scene.rs` + `preview/camera.rs` (small, mostly glue); then `preview/mod.rs` around
   `SlotState`; then the mod wiring.
2. `cargo check --target x86_64-pc-windows-msvc`; `./scripts/validate_background_dancers.sh`; `cargo fmt`;
   `./build.sh`. Hand the DLL to the maintainer for the cabinet demo below.

## Acceptance Criteria

1. **Dancer preview**
   - Given the options modal open at song select with BACKGROUND DANCER focused and `EMI #2` selected
   - When 150 ms pass after the last value change
   - Then the log shows `BackgroundDancers: preview P1 -- stage=none{option} … dancers=[emi01(F) …]`,
     `viewport_pass: attached clear@0x68 …`, `… preview P1: FileManager::Load accepted …`, `… parsed in …`,
     `… built …`, and Emi dances in the box over a dark backdrop with the modal chrome around her.

2. **Stage preview**
   - Given BACKGROUND STAGE focused with `BOOM #3` selected
   - When the settle elapses
   - Then the stage animates in the box under its own camera cuts (cropped, correct vertical framing), and
     the log carries the stage pick line with `dancers=[]`.

3. **Re-target + teardown**
   - Given a live preview
   - When the value is scrubbed quickly, then focus moves to another row, then the modal closes / song
     select is left
   - Then the preview follows the LAST value after the settle (one teardown + one start per settled
     change), and each teardown logs `preview exit -- N node(s) disabled`, `destroy(s) queued`, `destroyed
     by the engine flush`, `arc handle(s) freed`; the next song plays normally with its own dancers.

4. **Two players**
   - Given both modals open
   - When P1 previews a dancer and P2 a stage
   - Then both render simultaneously in their own boxes (`clear@0x68` and `clear@0x6b` sets), and the
     0-0-0 menu hides both while open.

5. **Fail-open**
   - Given `viewport_pass::is_available() == false`
   - When a preview is requested
   - Then one WARN, no passes, no window; the rows keep working.

## Metadata
- **Complexity**: High
- **Labels**: background-dancers, preview, engine-facing
- **Required Skills**: Rust, the mod's SceneWindow lifecycle, the compositor contract
- **Generated By**: code-task-generator 2026-09-21
- **Source Plan**: `.agents/planning/2026-09-21-background-dancers-selection-options/implementation/plan.md`
- **Plan Step**: Step 5: Preview driver — live dancer and stage previews
