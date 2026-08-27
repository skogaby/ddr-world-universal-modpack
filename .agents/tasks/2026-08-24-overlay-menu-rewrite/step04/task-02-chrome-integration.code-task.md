# Task: Chrome integration — cache/load pipeline, widget z-order, selection bar, scrollbar

## Description
Wire the pure chrome synthesis (task-01) into the live menu: the `overlay_menu.opacity`
config section, a background synthesis → hash-sidecar cache → `asset_loader` load
pipeline kicked at boot, ImageWidget chrome allocated in z-order beneath the Step 3
text widgets (panel, header backing bars, tab active-indicator, selection bar,
scrollbar track/thumb), per-refresh positioning, and the design §6 fallback ladder
(solid strip → text-only). After this task the menu is a rounded 80 %-opaque modal
floating over visible gameplay edges.

## Background
Step 3 shipped a text-only tabbed shell; Step 4 adds the modal look (design §4.5).
Task-01 provides `src/mods/mod_menu/chrome.rs` (pure): `synthesize_panel`,
`synthesize_strip`, `encode_png`, `clamp_opacity`, `cache_key_material`,
`panel_file_stem`/`strip_file_stem`, `DEFAULT_GRADIENT`, `LAYOUT_VERSION`. This task
is everything impure.

Key repo facts (verified):
- **Z among mod widgets = render-list creation order**; widget-pool nodes are
  permanently consumed (`destroy()` only hides). Chrome must therefore be created
  BEFORE the text widgets inside `render::allocate_widgets` (lazy, first open) —
  panel first (bottom), then chrome sprites, then the existing text creation order
  unchanged on top. This is the maintainer-approved resolution of the design's
  allocation-order wording.
- Design §6's "text before chrome" exhaustion protection is honored via a headroom
  pre-check instead: the boot free-pool count is 254 vs a ~49-widget worst case, so
  expose the Step 1 free-list walk as a callable count and skip ALL chrome creation
  (text-only rung, one WARN) if headroom is insufficient for chrome + text combined.
- `widget_renderer::create_image_widget(&ImageWidgetConfig)` with
  `texture_name: None` + later `set_texture_id(handle as i32)` is the pattern that
  avoids the per-widget auto-resolver thread (see
  `src/mods/training_mode/strip_hud.rs` `ensure_strip_widget`/`poll_resolve`).
- `asset_loader::load(path, stem)` accepts loose `./data_mods/...` PNG paths,
  registers the texture under the bare filename stem, resolves in ~0.7 s via
  polling `resolve`/`resolve_hash` (None while loading). ALL asset_loader calls are
  game-thread-only — route through `widget_renderer::run_on_render_thread`; each
  `load` pairs with exactly one `release`, never while bound to a visible sprite.
- Hash-sidecar cache precedent: `src/services/avs_layeredfs/cache_hasher.rs`
  (`CacheHasher::new(<out>.hash)` → `add_str(chrome::cache_key_material(..))` →
  `matches() && file exists` = hit → write file → `commit()`), canonical usage in
  `src/mods/webui_options/preview_overlay.rs::ensure_brightened_arc`.
- Config pattern: section struct with `Option` fields + `#[serde(default)]`, added
  to `ConfigFile` AND both explicit default-construction blocks in
  `src/mods/config.rs::init()`; consumers read via `config::get()`.
- Never hold the `MOD_MENU_STATE` mutex across a `run_on_render_thread` schedule
  (deadlock).
- Menu open/refresh paths: `mod.rs::open()/close()`, `render.rs::allocate_widgets/
  refresh_all/hide_all_widgets/destroy_widgets`; layout constants at the top of
  render.rs (`MODAL_X/Y/W`, `VISIBLE_ROWS=12`, `ROW_H=34`, `LIST_START_Y`,
  `CONTENT_X`, `RIGHT_X`). There is no `MODAL_H` constant yet — add it (600).

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-24-overlay-menu-rewrite/design/detailed-design.md (§4.5 chrome & layout, §6 error ladder, §5 config)

**Additional References (if relevant to this task):**
- src/mods/training_mode/strip_hud.rs — asset_loader load/poll/bind + background
  synthesis thread + PENDING mailbox + render-pump pattern
- src/mods/webui_options/preview_overlay.rs (ensure_brightened_arc) — CacheHasher
  sidecar usage

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. **Config**: add `OverlayMenuConfig { opacity: Option<i32> }` (serde defaults) to
   `src/mods/config.rs` (`ConfigFile.overlay_menu: Option<OverlayMenuConfig>` +
   both default blocks). Theme/animate keys arrive in Step 7 — only `opacity` now.
   Read once at mod-menu `enable()` via `config::get()`, mapped through
   `chrome::clamp_opacity` (default 80 when absent). No write-back in this step.
2. **New impure module `src/mods/mod_menu/chrome_loader.rs`** (pure/impure split per
   repo convention — the harness requires chrome.rs dependency-free), owning:
   - Boot kick from `enable()`: spawn one background thread (`catch_unwind`,
     WARN on panic) that ensures the cache dir `./data_mods/_cache/mod_menu/`,
     checks each piece's sidecar (`CacheHasher` + `chrome::cache_key_material`),
     synthesizes + `encode_png` + writes on miss, `commit()`s the hash, then
     deposits `(path, stem)` pairs into a mailbox.
   - A render-thread pump (strip_hud pattern) that drains the mailbox, issues
     `asset_loader::load` (guarded on `asset_loader::is_available()`), then polls
     `resolve` until both textures bind, stashing `TextureHandle`s in a
     `ChromeTextures` state (`Mutex`, phases: Pending/Loading/Resolved/Failed).
     The pump self-requeues only while work remains; it must be a no-op cost when
     done.
   - Fallback ladder (design §6), each rung one latched WARN: synthesis/encode/write
     failure, asset_loader unavailable, or load refusal ⇒ that piece Failed. Panel
     Failed but strip Resolved ⇒ render the panel as the strip texture stretched to
     the modal rect, tinted panel-base color with alpha = opacity (solid rung).
     Both Failed ⇒ text-only (all chrome hidden; menu fully functional).
   - No `release` during normal operation (textures live for the process; a single
     load each). Opacity is read once at enable — no runtime re-synthesis this step
     (Step 7 adds it).
3. **Widget allocation restructure** in `render.rs::allocate_widgets` (still lazy on
   first open), creation order = z:
   1. Panel ImageWidget — modal rect `(MODAL_X, MODAL_Y, MODAL_W, MODAL_H)`,
      `texture_name: None`, color 0xFFFFFFFF (opacity baked in texture),
      blend alpha.
   2. Header backing bars — `VISIBLE_ROWS` (12) strip ImageWidgets, one per slot,
      shown only when that slot renders a Header row (full content width, row
      height, accent-tinted low alpha).
   3. Tab active-indicator — 1 strip widget under the active tab label.
   4. Selection bar — 1 strip widget, full content width at the selected row,
      accent tint, low alpha (text stays legible on top).
   5. Scrollbar track + thumb — 2 strip widgets at the right edge of the content
      area (track spans the 12-row band; thumb height/position proportional to
      page/len and scroll/len).
   6. Existing text-widget creation order unchanged, after all chrome.
   Precede chrome creation with the headroom check (expose a
   `widget_renderer::free_node_count()` from the Step 1 diagnostic walk); if
   `free < chrome + text` estimate, skip chrome entirely (one WARN) and allocate
   text as today. Texture ids bind at creation when already Resolved, else the pump
   binds them when they resolve (widgets stay hidden until bound AND the menu is
   open).
4. **Refresh wiring** in `refresh_all`: position/show the selection bar with the
   cursor (hide when selection is None), the tab indicator under the active tab,
   header backings per visible slot kind, scrollbar shown only on overflow
   (`Navigator::overflows`) with proportional thumb geometry; panel always shown
   while open. `hide_all_widgets` and `destroy_widgets` cover every new widget.
   Keep the `">"` text cursor for now (redundant with the bar; the maintainer
   decides its fate at visual review). Colors: extend the pre-theme `COL_*` palette
   block with chrome tints (selection/indicator/scrollbar/header-backing) — Step 7
   swaps these for theme lookups.
5. **Constraints**: all hook-reachable additions panic-free (no unwrap/indexing in
   pump/render closures; `catch_unwind` on the synthesis thread); logging via
   `log_info!`/`log_warn!` only; every failure fail-open per the ladder; no changes
   to model.rs/tabs.rs/input.rs row semantics (chrome is render-only).
6. **Gates**: `./scripts/validate_mod_menu.sh` (existing tests untouched) →
   `cargo check --target x86_64-pc-windows-msvc` → `cargo fmt` (bare) →
   `./build.sh`.

## Dependencies
- task-01 (`chrome.rs` pure layer + harness extension) — must be complete first.
- `asset_loader`, `widget_renderer`, `cache_hasher` services (present).

## Implementation Approach
1. Read strip_hud.rs's load/poll/bind + mailbox pump end-to-end; read
   ensure_brightened_arc's sidecar flow.
2. Land the config section + enable-time read (smallest piece).
3. Build chrome_loader.rs (boot kick → cache → load → resolve state machine) with
   the ladder; verify boot log shows synthesis/cache-hit + resolve lines.
4. Restructure allocate_widgets (z-order + headroom check), then wire refresh_all
   geometry; keep each chrome element individually optional (missing texture ⇒
   hidden element, never a crash).
5. Gates, then deploy for maintainer visual sign-off.

## Acceptance Criteria

1. **Boot pipeline**
   - Given a clean cache dir and the DLL booting to attract
   - When the boot kick runs
   - Then the log shows panel+strip synthesis and asset resolution without errors,
     and `data_mods/_cache/mod_menu/` holds the PNGs + `.hash` sidecars; a second
     boot logs cache hits (no re-synthesis).

2. **Modal chrome renders**
   - Given the chrome textures resolved and the menu opened via triple-0
   - When the menu renders
   - Then a rounded 80 %-opaque gradient panel backs the whole modal with gameplay
     visible around the edges, the active tab carries an indicator, the selected
     row carries a selection bar that tracks navigation, and header rows carry
     backing bars — all beneath the text.

3. **Scrollbar on overflow**
   - Given a tab whose row list exceeds 12 rows
   - When scrolling through it
   - Then a proportional scrollbar thumb tracks the scroll position alongside the
     N/M readout, and tabs that fit show no scrollbar.

4. **Fallback ladder**
   - Given a simulated panel failure (e.g. synthesis error or asset_loader
     unavailable)
   - When the menu opens
   - Then the solid-strip rung (or text-only rung) renders with exactly one WARN
     per failure class, and every menu function (nav/toggle/adjust/persist) still
     works.

5. **Opacity config**
   - Given `overlay_menu.opacity: 50` in mod-config.json
   - When the DLL boots and the menu opens
   - Then the panel renders at 50 % opacity (and out-of-range values clamp+snap
     with one WARN-free default path — clamping is silent normalization).

## Metadata
- **Complexity**: High
- **Labels**: mod-menu, chrome, asset-loader, widgets, integration
- **Required Skills**: Rust, repo widget/asset_loader/render-thread conventions
- **Generated By**: code-task-generator 2026-08-24
- **Source Plan**: .agents/planning/2026-08-24-overlay-menu-rewrite/implementation/plan.md
- **Plan Step**: Step 4: Modal chrome — synthesized panel, scrollbar, opacity
