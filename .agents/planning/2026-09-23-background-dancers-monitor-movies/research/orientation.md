# Orientation — movies on the stage monitors

Static findings gathered before the decision register. The A3 mechanism itself is recorded in
`docs/background_dancers_research.md` §8 (written the same day); this file adds what the planning pass
found in the modpack and in the remaining World RE, plus the Griffin House source.

## 1. RE facts added during orientation (World 20260825 unless noted)

- **Entry 10's layer is a 1280 × 1280 canvas and is walked by default.** The layer-table builder
  `FUN_18002aab0` calls the entry-10 ScreenRoot's set-size vslot (`vtbl+8`) with `xmm1 = xmm2 = 1280.0f`
  (`MOVSS xmm6,[0x18038fb74]` = 1280.0; the other override entries get `(1280, 720)` = `xmm6/xmm7`).
  The ScreenRoot ctor `FUN_180217df0` writes `u16 +0x10 = 0x0100` (byte `+0x10` = 0, `+0x11` = 1) and
  `+0x12 = 1`, which is the layer dispatcher's walk gate (`+0x10 == 0 && +0x12 != 0`,
  `docs/overlay_draw_research.md` "layer dispatcher") — entry 10 is walked every frame even though
  nothing is registered into it. Its node pool holds 0x100 nodes; the BM2D-group boot install (same
  function) puts groups only into entries 0, 2, 7, 8 — entry 10's list is exclusive to a routed movie.
- **The MovieActor fit fields** (case `0x1045` of `FUN_18007d250`): origin = f64 `(x, y, z)` at
  `+0x108/+0x110/+0x118`, size = f64 `(w, h, d)` at `+0x120/+0x128/+0x130`; the fit `FUN_18007d030(this,
  &size, &origin)` reads only x/y and w/h. The fit runs ONLY while the actor's step == 2 (the message case
  is gated on it); at the step-2→3 transition the Movie object keeps the last size/position it was given.
  The fields are written by the ctor `FUN_18007c960` (from the SceneManageActor's marker rect) and read by
  nothing else — so a write while the step is 0/1/2 fully determines the framing.
- **AOB for the fit fields** (swept with a one-off capstone script over the five builds, unique on each,
  identical displacements `0x108 / 0x118 / 0x120 / 0x130` everywhere):
  `83 7C C1 58 02 0F 85 ?? ?? ?? ?? 0F 10 81 [d32] F2 0F 10 89 [d32] 4C 8D 44 24 20 48 8D 54 24 40 0F 29 44 24 20 0F 10 81 [d32] F2 0F 11 4C 24 30 F2 0F 10 89 [d32]`
  — 20250805 `0x180079570`, 20260224 `0x1800786b0`, 20260721 `0x18007cea0`, 20260825 `0x18007d280`,
  20260915 `0x18007d3f0`.
- **AOB for the layer select**: `44 38 81 48 01 00 00 B8 09 00 00 00 49 0F 45 C0 48 8D 04 40 48 8B 54 C2 08`,
  imm at match+8 — table in `docs/background_dancers_research.md` §8.5.
- **Screen UV convention** (stock `gm_monitor01_monitor1`, a 4-vertex quad facing +Z): the top edge
  (y = 6.68) has v = 0.2, the bottom (y = 1.30) v = 0.8, u grows with +x — the render target is sampled
  u → right, v → down as seen by the camera, i.e. an ordinary unmirrored D3D mapping.
- **The texture hash** the model converter stores for a table entry and the one the boot registration
  gives the render target are the same function of the folded name (`scripts/ktmdl_dump.py::
  texture_registry_key` = FNV-1 of the lower-cased, `_`-stripped stem; `src/services/scene3d/pure.rs::
  fnv1_name_hash`). `offscreen1` folds to itself.

## 2. Modpack code the feature lands in

| Concern | Where | What it does today |
|---|---|---|
| Mode enum, size table, backdrop classify, scene mask | `src/mods/background_dancers/movie_mode.rs` (pure, host-tested) | `MovieMode {Off, Thumbnail, Fullscreen}`, row values 0/1/2, config keys `off/thumbnail/fullscreen`; `SceneMask {stage, shadows}` (dancers always visible) |
| VIDEO SIZE override | `src/mods/background_dancers/movie_size.rs` | writes `Customize+0x30` per entered side at window entry, restores at exit |
| Movie probe | `src/mods/background_dancers/movie_backdrop.rs` | live DPS → SceneManageActor → MovieActor (RTTI vtables) → StackStep; `probe()` → `Backdrop` |
| Window lifecycle | `src/mods/background_dancers/lifecycle.rs` | `window_entry` picks the stage THEN calls `apply_movie_mode()` (so the stage key is known when the route is decided); `drive_live` probes the backdrop only in FULLSCREEN mode and only after `drive_assets` reports something built (an early return — a per-song writer that must beat the movie's first 0x1045 cannot live behind it) |
| Candidate tables | `lifecycle.rs::init_tables` | builds stock + custom stage candidates at mod enable (arc existence via `scene3d::arc_set::resolve_path`) |
| Arc header read | `src/mods/background_dancers/custom_scan.rs::read_arc_members` (private) | 64 KiB prefix read → member paths; the only header-only reader in the crate |
| Row + persistence | `src/mods/background_dancers/style.rs` | GLOBAL SETTINGS enum row `background-dancers-movie-mode`, writes the section WHOLE; config string `background_dancers.movie_mode` |
| Restyle eligibility | `src/services/scene3d/render_item_layout.rs::restyle_eligible_materials` (pure) + `render_item.rs::restyle_materials` | blend-group-0 ∧ instance allows; no texture criterion. Hull twins hide every record whose material was not restyled (`mark_hull_records`) — so a restyle exemption automatically removes the outline too |
| Texture table access | `render_item.rs::resolve_material_textures` | already walks `res+0x80` entries (hash `+0`, ptr `+8`) and the material slot indices `u16 mat+slot*2` under mask `mat+0x14` |
| Patch primitive | `src/core/memory.rs::make_writable/restore_protection` (pattern: `src/mods/anytime_speedmod.rs::write_gate`) | a checked imm rewrite on the game thread |

Existing gating pattern to copy: `lifecycle::effective_movie_mode` degrades FULLSCREEN to THUMBNAIL with one
WARN per boot when a dependency is missing.

## 3. Griffin House source

- Deliverable today: `data_mods/custom_models/stages/Griffin House/mapset_griffin00/` (`gm_griffin00_room/`
  = one `.model` + 15 DDS; `camera/griffin_st01..04.camanm`). The room is ONE model, 15 meshes, every mesh
  `mdl_bg_constant_vc`, alpha-tested, opaque; mesh 11 is the TV screen (6 vertices, texture `lrscreen`).
- Source: the maintainer's Blender project `ddr-peter-griffin/livingroom` (outside the repo):
  `out/living_room_game_v2.blend` (object `gm_griffin00_room`, 15 materials; the screen is material
  `TVScreenShot` = Principled BSDF + one Image Texture `lr_screen.001`, 1024 × 512, packed, a DDR World
  screenshot letterboxed to the panel). Produced by `work/decorate_room.py`, which also contains the
  export call (`export_model.export_model(..., [room], write_textures=True)` into `out/export/`).
- The screen: two quads, normal +Y (Blender), x −0.557…0.577, z 0.754…1.412 (1.13 × 0.66 m, aspect 1.72),
  UVs currently u = (x1 − x)/(x1 − x0) (the "not mirrored as seen from +Y" rule), v = (z − z0)/(z1 − z0).
- Add-on facts: the KTMDL texture name comes from the image stem (`export_model.sanitize_texture_stem`,
  alnum + `_`, folded by the game); `write_textures=True` writes `<stem>.dds` from the Blender image.
  Colour attribute present (`Col`, opaque white) ⇒ the exporter picks `mdl_bg_constant_vc`.

## 4. Unknowns

1. Whether the movie really lands in list 5 and shows on the screens (static chain is complete; nothing
   in World has exercised entry 10 — cabinet test).
2. Whether an arc-shipped `offscreen1.dds` stays harmless under the World revival's load/unload cycle
   (first registration wins; stock monitor arcs already loaded without incident in deploy #2).
3. Custom resolution: entry 10's canvas is a constant 1280², the OFFSCREEN1 viewport becomes `render_w²`
   — expected to scale like every other canvas→viewport pair, unverified.
