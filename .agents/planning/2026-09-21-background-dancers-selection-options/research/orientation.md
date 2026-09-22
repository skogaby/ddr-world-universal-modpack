# Orientation — Background Dancer / Stage selection options with live 3D previews

Compiled 2026-09-21 from the codebase, `docs/background_dancers_research.md`,
`docs/background_dancers_feasibility.md`, `docs/custom_arrow_renderer_research.md`, the
`2026-09-16-enable-background-dancers` planning set, and a short Ghidra session on
`gamemdx_20260825.dll` (addresses below are file-relative to `0x180000000`).

## 1. The option rows — straightforward, everything exists

### 1.1 Candidate tables and labels
- Stages come from `data/map/map_resources.rlist` (34 rows, 7 `dummy00` rows excluded, keys repeat:
  `boom00` ×2, `monitor00` ×2) and dancers from `data/chara/chara_resources.rlist` (26 rows,
  unlock ids ignored), both read from `data/arc/startup.arc` by `lifecycle::init_tables()`
  (`src/mods/background_dancers/lifecycle.rs:91-177`). Camera sets are row-parallel to the stage
  rows (`data/camera/stage_camera_resources.rlist`).
- **Keys ARE the arc stems** — `pl_<key>.arc` / `mapset_<key>.arc` — so both dancers and stages
  are "scrapeable" names. Dancer keys are `<name><costume##>`: `yuni00 rage00 afro00 jenny00
  emi01 babylon00 gus00 ruby00 alice00 julio00 bonnie00 zero00 rinon00 emi02 alice01 rinon02
  yuni02 rinon01 concent00 zukin00 pix00 emi00 yuni01 rage01 afro01 jenny01`. Stage keys:
  `boom00..06 club00 crystaldium00 cyber00 dawnstreet00 disco00 floor00 lovesweets00
  monitor00..03 replicant00..05 speaker00` (25 distinct keys). Stage names are internal but
  still descriptive. Longest derived label (`REPLICANT #6`, `CRYSTALDIUM`) is ≤ 15 bytes — the
  scalar value-text budget (MSVC SSO, `rows.rs:2464-2499`; over 15 bytes heap-promotes + leaks).
- Existing fixed-choice plumbing: `selection::apply_pin` + `session::assemble_pick`
  (`selection.rs:434-456`, `session.rs:201-230`), today fed only by the dev env var
  `DDR_DANCERS_PIN` read once at enable (`lifecycle.rs:192-206`). `Tables` is private
  (`lifecycle.rs:78-86`) — needs a public accessor. `apply_pin` pins the FIRST row of a stage key;
  the option should pick uniformly among the key's rows like `pick_stage` does.

### 1.2 Framework fit (`src/services/custom_options/`)
- `RegisterSpec::scalar(id, min, max, step, format)` renders the value as TEXT
  (`OptionElement<int>` donor). **No existing `ScalarFormat` can render arbitrary per-value
  names** — `Labeled` is `prefix + value` with ONE terminal label (`api.rs:155-214`). A new variant
  (fn-pointer labeler, stays `Copy`) is the minimal change at `format_scalar_value`
  (`api.rs:747`). Alternative: `UiKind::Enum` with one `seop_op_<key>` ribbon PNG per value — 51
  new textures + regeneration; rejected as heavy.
- Placement under PLAYFIELD STYLING OPTIONS is purely `option_menu_settings` ORDER
  (`ordering.rs:104-155`); the header is owned by `decorative_option_headers.rs`. New ids go into
  the shipped `mod-config.json` after `arrow_opacity` (line ~164) with `overlay: false` (or
  `.in_game_only()`), exactly like the `customize_*` rows. The updater's header-scoped merge
  delivers them to existing installs.
- Per-side vs mirrored: `versus_mirror::register(&[id])` + `mirror_edit` at the `on_change` tail
  (`src/services/versus_mirror.rs:71-113`) — the premium_free / song_speed shape.
- Persistence: `PersistMode::Local` (JSON cache `custom_options.p1/p2.<id>`, never on the wire —
  the multiplayer_bot precedent) with a `load_transform` clamp against the discovered count.
- Conditional presence: register inside `BackgroundDancersMod::enable()` after
  `init_tables()`, `Duplicate` ⇒ `set_option_available(id, true)`, `set_option_available(id,
  false)` in `disable()` — the standard pattern. Caveat shared by every mod: the label/preview
  atlas is flushed ONCE after all enables (`lib.rs:629`), so a mod toggled ON live gets its row
  textures next launch.
- Textures: `scripts/option_strings.py` LABELS (en/ja/ko mandatory) + a `TemplateSpec` with a
  green marker rect for the live preview + a fallback `PreviewSpec`; regenerate with
  `scripts/gen_option_labels.py`.

### 1.3 Preview plumbing (from `webui_options`)
- Target rect = a solid green marker on `seop_image_<id>_TEMPLATE.png`, read at DLL init by
  `preview_gen::marker_rect_for` (`preview_gen.rs:143-167`). Screen placement =
  `CHROME_ORIGIN[side] + marker` where `CHROME_ORIGIN = [(185,463) P1, (742,463) P2]` in the
  1280×720 canvas (`bg_preview_overlay.rs:135-138`; template renders 1:1). Not detected live —
  baked at generation, read from the PNG.
- Focus/open/close: `custom_options::on_preview_request(side, option_id)` fires every focus
  tick on a mod row; `on_menu_open` / `on_menu_close` per side (`mod.rs:609-677`). Both sides can
  have the modal open simultaneously.
- `generate_chrome(option_id)` (`preview_gen.rs:188-236`, `pub(super)`) writes the base preview
  image with markers cleared; must run BEFORE `register_option` so the atlas sees it.

## 2. The live 3D preview — the hard part, and what Ghidra says

### 2.1 Why the stock render path cannot show it
The three MODEL passes are attached to the RENDER-3D target list (`display+0x28`) at priorities
0x66/0x67/0x68 (`FUN_1801f2c30`), i.e. the 3D is always the frame floor under every 2D layer.
At song select the UI is opaque; the only visibility tool the mod owns is the whole-layer alpha-0
`background_hide`. No rectangular hole is possible (D3D9 scissor is include-only).

### 2.2 Dead end confirmed: "tag-0x10 model draw" (Option C)
On 20260825 the 2D ScreenCommandList walker is `FUN_18026a040`; its `case 0x10` handler
`FUN_180269600` → `FUN_18026cc00` emits gd tag 8 (SetTexture) from a texture OBJECT
(`{TextureData*, f32[4] SamplerParameters}`), with a default-texture fallback. **It is a
SetTexture-by-object record, not a model draw.** The feasibility doc's Option C is disproved.

### 2.3 The viable route: a mod-owned clone of the MODEL pass, attached into the 2D stack
- **Target lists are plain vectors, not intrusive lists.** `FUN_1802666c0(list, viewport, prio)`
  = `push_back({viewport*, prio})` + sort (`FUN_1802668f0`); `FUN_1802667d0(list, viewport)` =
  detach. A mod-owned object can be attached and detached at will (game thread).
- **Each pass carries its own D3D viewport rect.** `FUN_1802666c0` fills `viewport+0x08..+0x18`
  `{x,y,w,h}` from the target's u16 dims ONLY when all four are zero — a pre-filled rect is kept.
  So a clone with `{x,y,w,h}` = the preview box in render-target pixels renders clipped to that
  box with the projection mapped onto it, no scissor and no off-centre frustum tricks needed.
- **Pass object layout** (0xF8 bytes, ctor `FUN_1801f6510`): `+0x08..+0x20` four callbacks
  (copied from a 0x20 block), `+0x28` sort mode (0x15 OPACITY / 0x1A TRANS), `+0x2C` node-mask
  FILTER (0x56 / 0x46), `+0x30` `gs::Renders::Model::Viewport<Render>` vftable (2 slots: render
  `FUN_1801f68a0`, dtor), `+0x38..+0x44` viewport rect, `+0x48/+0x4C` minZ 0 / maxZ 1.0,
  `+0x50` name hash, `+0x54` flags, `+0x58` proj (0x40), `+0x98` view (0x40), `+0xE0` self
  back-pointer (the render vfunc reads `viewport+0xB0` = outer), `+0xE8` item list (the graph's
  `+0x30` list, set by the manager ctor), `+0xF0` callback-block pointer.
- **Camera matrices are per pass.** The manager tick `FUN_180023fb0` memcpy's the ACTIVE
  camera's view (`cam+0x08`) → `pass+0x98` and proj (`cam+0x1C8`) → `pass+0x58` for the FOUR
  stock passes only. A clone gets whatever we write there — so a preview does NOT need camera
  slot 0 at all; we compute view/proj ourselves and write them into the clone(s) each frame.
- **Per-side isolation via unused node-mask bits.** Stock filters use bits
  `0x01|0x02|0x04|0x10|0x40`; `0x08`, `0x20`, `0x80` are free. Preview items tagged `0x08` (P1) /
  `0x20` (P2) with clone filters `0x08` / `0x20` are drawn ONLY by their clone and never by the
  stock passes (no wasted frame-floor draw). Two players can preview different things in their
  own boxes with their own cameras in the same frame.
- The render entry `FUN_1802606d0(outer, gdEmitter, itemList)` iterates the whole item list with
  the pass's filter/sort, does the once-per-frame bone upload (already shared between
  OPACITY/TRANS), and emits the draw stream. Nothing in it is pass-identity-specific.
- Attach point: the RENDER_2D target list (`display+0x38`) holds the three 2D-list viewports
  (`DAT_1806f1598/1580/1568`) at 0x65/0x66/0x67. A clone at priority ≥ 0x68 draws after every
  AFP layer — above the options modal (and above the 0-0-0 overlay, which shares those lists).

### 2.4 Open research items for Step 4 (all small, all Ghidra)
1. The target-list render loop: confirm it applies each viewport's rect (D3D SetViewport) before
   calling slot 0 and what it passes as the item list; confirm the clear flags word
   (`list+0x20`, ctor `FUN_180266660` sets 7) — RENDER_2D is believed to clear DEPTH ONLY.
2. Depth in the 2D stack: do AFP quads write depth? If yes, the model records (z-test from mesh
   flags) fail behind the UI → need a depth clear scoped to the box (D3D9 `Clear` with a rect,
   or a tiny mod-owned "clear" viewport at 0x68 before the pass at 0x69). The gd record shape
   for Clear is known from the 2D tag 0 forwarding (`FUN_1802678a0`).
3. `SceneGraph::update` pass 5 (frustum cull with camera slot 0): does it drop items from the
   list or only tag per-camera visibility? Determines whether the preview scene must sit inside
   slot 0's frustum (mitigation: point slot 0 at the preview scene — stock World draws nothing 3D).
4. View/proj construction (`FUN_180220b80(camera)`): reproduce in Rust (the projection formula is
   already in `docs/3d_model_format_research.md` §6 / `core/anm/camera.rs`; the look-at view is
   standard) — validate against a live dump of `pass+0x58/+0x98` during gameplay.
5. Render-target pixel mapping: the RENDER_2D target is the display (output-sized under
   custom_resolution) — rect = `(CHROME_ORIGIN + marker) × output/1280`.

### 2.5 Mod-side reuse and gaps
- Time base: `director::produce(sess, t, visible)` / `director::camera_frame(sess, t)` take a
  plain `f32`; `TempoOptions::REAL_TIME` exists — a wall-clock preview driver is trivial
  (`lifecycle.rs:825-841` is the only music-count coupling).
- `Pick.stage` is mandatory and `make_pick` forces ≥ 1 dancer; `parse_pick` opens the stage arc;
  `camera_frame` needs a dance schedule. A dancer-only or stage-only preview needs both made
  optional (small, mechanical).
- Session build/teardown (`build_pending`, `begin_teardown`, `drive_teardown`) are reusable
  verbatim; teardown's `item_listed` observation needs the graph enabled — true at scene 25
  (the enable bit is cleared only by `DancePlaySequence::onInitialize`). Previews MUST be torn
  down before leaving scene 25.
- `node::visit(4)` refuses when `!song_reset::live_dps_probed()` (= no live TransitionSequence
  child) — passes at scene 25, drops nodes during scene transitions (fine).
- `frame_board::MAX_INSTANCES = 32` process-wide; two preview sessions + hull twins can exceed
  it — previews should skip outlines or the cap needs raising.
- Load latency: FileManager residency 22–102 ms warm, hundreds of ms cold for textures; arcs
  `pl_*` ≈ 1 MB, `mapset_*` 1–2 MB, `mc_*` 2.8 MB/sex. A settle debounce on value changes is
  needed (the 150 ms `RefreshCell` precedent in `song_rate/preview.rs`).
- Mod menu: the preview draws above the widget layer too — suppress while `mod_menu::is_open()`.
