# Research — Existing Code Leverage

What the repo already provides for a per-player **arrow/receptor scale + opacity**
mod, and where the boundaries are. (Companion: `arrow-render-re.md` for the new
Ghidra findings.)

## 1. The template: `overlay_element_styling`

`src/mods/overlay_element_styling/{mod.rs, capture.rs, color_hook.rs}`.

**Reusable verbatim:**
- Option-row pattern (mod.rs:158–186): two `RegisterSpec::scalar` rows —
  `overlay_scale` (25–150, step 5/25, default 100) and `overlay_opacity`
  (0–100, step 5/25, default 100), `PersistMode::Full` (builder default),
  `on_change` mirroring into per-side `[AtomicI32; 2]` (mod.rs:91–99).
  Duplicate-on-re-enable treated as success; enable-time reseed from
  `custom_options::get_value` (mod.rs:353–362).
- Two-tier value reads: registry-first cold path, atomic-only hot path
  (mod.rs:106–131).
- Side attribution: single-active-side via the `player_array_anchor` presence
  read (triple-deref `*(*(*slot)+4) != 0`, capture.rs:159–175 — originally
  `center_arrows_single::read_presence`); versus fallback = X threshold at 640.
- Scene lifecycle: clear state on GAMEPLAY enter/exit via `scene_manager`
  (mod.rs:367–376).
- Detour hygiene: `install_enabled` (store-before-enable), `catch_unwind`,
  original-first, graceful degradation with `required_signatures() = &[]` and
  load-bearing checks in `init`/`enable` + `is_active()` self-report.
- Label PNG workflow: add `(id, "TEXT")` to `scripts/gen_option_labels.py`
  `LABELS`, emits `seop_item_<id>.png` into
  `data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/`.

**NOT transferable:** everything CMovieClip/AFP-layer specific (Create/SetColor
detours, `layer_set_scale_translate_raw`, CXFORM compose). **Arrows and
receptors are not AFP clips** — they are sprite quads written into the
`gs::Screen` CommandList by dedicated renderer classes (see `arrow-render-re.md`).

## 2. The arrow render path the repo already hooks

- Signatures in `src/core/signatures.rs`: `render_notes` (the ArrowRenderer
  per-frame draw — **already detoured by `note_types_expansion::mine_render`**,
  so this feature must share via a dispatcher if it needs that hook point),
  `render_sprite_final` (the per-quad fill: `(this, &sprite, x, y, w, h,
  &uv[4], twist, &color)` → 0x34-byte ROTATESPRITE), `set_direction`,
  `get_offset_y` (pure scroll-offset fn), plus derived `set_render_state`,
  `screen_renderer_state`, `default_shader`.
- ArrowRenderer field map (mine_render.rs:41–77): +0x20 atlas TextureData*,
  +0x2C blend, **+0x30/+0x34 posX/posY (lane origin)**, +0x50 twist, +0x54 UV[4],
  **+0x64 color RGBA (whole-lane tint; alpha = byte 3)**, +0x68 appearance,
  +0xA0 speed×100, +0xA4 boost, +0xA8 beat_count, +0xAC music_count, +0xB0 mode
  (0 single / 1 double), +0xB8 Results-vector ref, +0xC0 arrow shader,
  +0xEC judged-option, +0xF4 offset_y.
- Note geometry: lane-relative `x = 96 * dir` (`ARROW_SIZE = 96`), `y =
  round(get_offset_y(dBeat, speed, boost, music_count))`; the fill adds
  posX/posY and handles reverse + appearance alpha internally.
- mine_render's own cull margin (mine_render.rs:436–443): skip if `fy > 720`
  or `fy + 96 + offset_y + 96 < 0` — mirrors the game's collector window.
- **Mines bypass the fill**: mine_render computes positions and emits quads
  directly into the CommandList. If arrows scale, mines must apply the same
  transform inside mine_render (integration point).
- CommandList discipline (docs/mine_render_architecture.md): restore blend
  (+0x2C + `set_render_state`), re-bind arrow shader/atlas at pass end —
  downstream receptor draw inherits CommandList state.

## 3. `center_arrows_single` + the HUD layout system

- `hud_layout_builder` / `hud_layout_setter` signatures; the named-coord map
  stores 6 ints per key (`x, y, w, h, scaleX, scaleY`) for keys incl.
  `arrow_raw`, `arrow`, `freeze_judge`. The lane renderers consume only
  `x`/`y` from the `arrow` entry (positions of ArrowRenderer/SpotRenderer/
  JudgeEffectRenderer); **the scale fields are not honored by the lane path**
  (verified — see `arrow-render-re.md` §6), so layout-map scale writes are a
  dead end for this feature.
- `hud_layout_setter` already carries `center_arrows_single`'s detour
  (one-detour-per-target applies if we ever need it).
- Presence read + `PER_SIDE_PARENT_BASE 0xE0` / stride 0x48 side derivation
  (center_arrows_single.rs:63–64, 202–264).

## 4. Per-side / GamePlayActor context

- `judge_hook` pre-callbacks deliver a per-frame `GamePlayActor*`;
  `GamePlayActor+0x84` = playSide; `player_work_table` global via
  `player_work_table_anchor`. This is the established route from actor → side.
- Renderer instances have **no side field**; versus runs two full renderer
  sets. Side binding must come from posX (X<640 split) + single-active-side
  presence read — the exact approach `overlay_element_styling` ships.

## 5. Constraints checklist for this feature

- One detour per target: `render_notes` is taken (mine_render);
  `render_sprite_final` is currently **not detoured** — it is the natural new
  hook point (all lane quads flow through it; see RE doc).
- Hot path: the fill runs per quad per frame (~10–60 calls/frame typical;
  more with the extended cull window) — callback must be tight, atomics only.
- No panics across FFI; graceful degradation if signatures/services missing.
- Scalar rows persist via `PersistMode::Full` → values live in
  `custom_options.{p1,p2}` JSON + network round-trip automatically.
