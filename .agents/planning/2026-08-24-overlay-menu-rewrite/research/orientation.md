# Orientation — Overlay Menu Rewrite

Date: 2026-08-24. Findings from reading `src/mods/mod_menu.rs` in full plus two
research passes (custom_options API surface; rendering/scene/input capabilities).

## Current implementation (the rewrite target)

`src/mods/mod_menu.rs` (~1170 lines, single file):

- **State:** one global `Mutex<ModMenuState>` — `rows` (rebuilt on open), `contributed_rows`
  (persistent, from `register_scalar_row`/`register_enum_row`), `visible_rows` (indices
  passing `visible_when`), cursor/scroll, widget handles.
- **Row model:** `MenuRow { key, label, hint, kind: Boolean|Scalar|Enum, indent, visible_when,
  on_change }`. Registry mod toggles are `Boolean` with `on_change=None` (routed through the
  registry toggle + `save_mod_states`); contributed rows fire `on_change(i32)`.
- **Rendering:** 25 TextWidgets, zero images — header, instructions line, ASCII-dash
  separator, `>` cursor, 7 slots × (name @ full scale, desc @ 0.5, status column @ x=1100).
  `SLOT_HEIGHT=55`, list starts y=120. White text straight over gameplay. This is the whole
  "UI".
- **Input:** triple-0 open/close gesture (1250 ms window), exclusive consumer while open
  (8/2/4/6 or menu buttons; Start-held = coarse), `set_input_suppressed(true)` blocks the
  game side, hold-to-repeat via a generation-tokened poll thread.
- **Registration API for overlay rows:** `ScalarRowSpec`/`EnumRowSpec` (label/hint strings,
  parent_row_key gating, on_change). Consumers today: `timing_offsets` (4 scalars),
  `fps_unlock` (enum), `quick_restart_or_fail` (RESTART DELAY enum),
  `music_wheel_song_length` (X/Y offset scalars), `shader_fixes` (AA enum). These are all
  **cabinet-wide** values — they map naturally onto the proposed "global config" tab.
- Useful separability already exists: registry rows (mod on/off) vs contributed rows
  (config) are distinct in the model; the rewrite's MODS/GLOBAL tab split has a clean seam.

## custom_options framework (per-player, in-game menu) — mirroring constraints

Full report highlights (see `src/services/custom_options/`):

1. **No display strings at runtime.** `RegisterSpec` carries only an id; the label is the
   texture `seop_item_<id>` (registry.rs:58-60). Human-readable strings live offline in
   `scripts/option_strings.py` (en/ja/ko) and exist only as rendered PNGs. Enum values
   likewise carry `label_texture_name` only (api.rs:62-73). **Mirroring rows into a
   text-rendering overlay requires adding display-string fields** (or rendering the same
   textures via ImageWidgets — but those are IFS-residency-dependent).
2. **Scalar value text is reusable:** `rows.rs::format_scalar_value` composes the value
   string in Rust (`ScalarFormat` variants) — the overlay can call the same formatter.
3. **Per-side values:** `values: [i32; 2]` per option in a single `Mutex<FrameworkState>`.
   APIs: `get_value(side, id)`, `set_value(id, side, v)` (fires the single-owner
   `on_change`), `set_value_silent` (seed without callback), `set_scalar_bounds` (live
   re-bounds, used by training_mode). **No multicast value-changed subscription exists** —
   the overlay needs a new observer hook or polling to reflect in-game edits live.
4. **No unregister; re-enable = expected `Duplicate`.** Disable flips
   `set_option_available(id, false)`; the in-game builder filters unavailable rows per open.
   Placement flags must be idempotent like availability.
5. **Visibility:** `ShowWhen::{Always, Equals, NotEquals}` against a parent's live per-side
   value, evaluated per side. Headers (`UiKind::Header`) are stateless decorative rows; in
   the in-game menu unlisted headers are excluded when `row_order` is configured.
6. **Persistence:** `PersistMode::{Full, SaveOnly, None, Session}`; network loads land at
   scene-25 entry; JSON cache under `custom_options.p1/p2`. Overlay edits that go through
   `set_value` inherit all of this for free.
7. **Config keys today:** `persist_network`, `persist_json`, `lane_gamma_correction`,
   `preview_window`, `animate_backgrounds`, `row_order`, `p1`/`p2`.
8. **Callbacks run on the render thread** for user edits; registration-time priming on the
   enabling thread. Don't hold the STATE mutex across `run_on_render_thread`.

## Rendering capabilities

What exists today (all cited in the research pass):

- **TextWidget:** position, RGBA+alpha, scale, alignment, outline, multi-line. One font
  (captured game bitmap font).
- **ImageWidget:** arbitrary x/y/w/h (free stretch), UV rect, ABGR tint incl. alpha, blend
  mode, rotation. A semi-transparent 80 % panel is a one-liner. Rounded corners need PNG
  art (or 9-slice via UV).
- **Loose-PNG loading at runtime:** `asset_loader` loads PNGs straight from disk —
  including **runtime-synthesized PNGs** (training strip HUD writes generated PNGs to
  `data_mods/_cache/` and binds them to sprites). Menu chrome can be generated, not shipped.
- **AFP clips at will:** `bm2d_package::request_load` + `bm2d_api::create_layer_from_package`
  → position/scale/mask/alpha/play an animated clip anywhere (bg_preview_overlay is the
  full precedent, incl. group/priority above the options modal). An AFP clip could be an
  animated menu background today — but authoring NEW AFP content is untooled.
- **Z-order = render-list append order (creation order); the widget node pool is finite and
  nodes are permanently consumed** (`destroy()` only hides). Allocate once, reuse; pool
  headroom for a richer menu is an open research item.
- **Input:** suppression detours cover the 10-key and all five menu buttons; 12 pinpad keys
  + Start + 4 directions per side available to the menu.

### Shader-drawn backgrounds (the ambitious path)

- The game's render command list is fully mapped (docs/custom_arrow_renderer_research.md):
  records `{u16 tag, u16 size}` — 0x13 SetShader, 0x14 SetVSConstantF, 0x04 quad batches,
  0x05/0x06 DrawVertices, 0x11 SetTexture, 0x0C scissor.
- **The DLL already emits brand-new draw records** (mine_render appends
  SetTexture/SetShader/quads; pass_rewrite emits constant uploads) — but only inside the
  gameplay lane pass via `render_notes_hook`.
- Gap = two well-scoped RE items: (1) an emission site valid in **every scene** with
  late/high z (the 11 layer slots + walker are documented); (2) a bindable custom shader
  program outside gameplay — easiest via extending `gs_screencommand_default.gsp` with
  extra programs (multi-program containers are first-class; the default-shader global is
  already derived). Judgment: moderate, days not weeks; risks are z-order and scene
  lifetime of the objects.
- SM3 (vs_3_0/ps_3_0) via fxc 9.29 is the compile path; procedural arrows/bubbles/waves fit
  SM3 budgets easily.
- **Zero-RE fallbacks:** AFP clip background; UV-animated / position-animated ImageWidget
  sprites (a scrolling-arrow background is buildable today); static synthesized gradient.

## Scene/session awareness

- `scene_manager::current_scene()` + `on_scene_change`; scene constants in
  `src/types/scenes.rs`: attract band 2..=16 (ATTRACT_DEMO 16), MODE_SELECT 20,
  SONG_SELECT 25, GAMEPLAY 28, EAM_EXIT 34.
- "Player entered" per side: `stage_records::side_entered(side)` (PlayerWork+0x4),
  fail-closed `None` when unavailable.

## Implications for the rough idea

1. The MODS vs GLOBAL split falls out of the existing row model cleanly.
2. Mirroring in-game options requires: display-string fields on `RegisterSpec`/`EnumValue`
   (or texture-based labels), a placement parameter, and a value-changed observer (or
   polling) — all additive to custom_options.
3. Per-player tab needs a side-selection UX (the in-game menu is inherently per-side; the
   overlay has one cursor) — a decision the rough idea doesn't cover.
4. WebUI cosmetic rows (preview boxes, on-demand art) don't translate 1:1 into the overlay;
   placement default may want per-registration override.
5. Modal chrome (panel, tabs, rounded corners, scrollbar, 80 % opacity) is fully buildable
   today. Animated theme backgrounds: sprite/AFP paths today, shader path after a
   moderate, well-scoped RE spike.
6. Widget pool capacity and z-order (creation order) need verification for a widget-heavy
   menu.
