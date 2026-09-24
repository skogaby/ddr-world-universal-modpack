# Progress — Background Movies on the stage screens

Updated: 2026-09-23
Status: DONE — cabinet pass GREEN (incl. training 7/9 scrubs keeping the movie in sync); diagnostics removed
NEXT ACTION: none. Maintainer commits.

- **2026-09-23 follow-up (design R2):** STAGE SCREENS made the DEFAULT at the maintainer's request —
  `MovieMode::DEFAULT = StageScreens` (absent / unknown config value and unknown row values), `style.rs`
  `LIVE_MOVIE_MODE` seeds 3; row 1 now maps explicitly to THUMBNAIL. A missing dependency still degrades to
  THUMBNAIL (`degrade`), as does every stage without screens. Shipped `mod-config.json` carries no `movie_mode`
  key, so installs without one switch automatically; an explicitly saved `"thumbnail"` stays. Docs: config.rs doc
  comment, mod.rs, README (paragraph, feature table, config row), AGENTS.md (row + config entry), research §8.8.

Resume protocol: read `implementation/plan.md` (the step list + per-step guidance), then
`design/detailed-design.md` §4 for the component being worked on; RE background in
`docs/background_dancers_research.md` §7 / §8 (§8.8 = what shipped).

## Done

- **Step 1** — `movie_layer_select` + `movie_actor_fit_case` in `src/core/signatures.rs` (after
  `movie_build_graph`), `derive_movie_screen_route` (after `derive_layer_table` in `resolve_derived`) publishing
  `movie_layer_select_imm` (address), `movie_fit_origin_off` / `movie_fit_size_off` (`publish_value`) + two pub
  accessors. Sweep ALL GREEN on the five builds: imm `+0x792AA / +0x783EA / +0x7CBDA / +0x7CFBA / +0x7D12A`
  (20250805 / 20260224 / 20260721 / 20260825 / 20260915), origin 0x108, size 0x120 everywhere; `shape_diff.py`
  (window 0x80) — both byte-shape-identical.
- **Step 2** — `movie_mode.rs`: `StageScreens` (row 3), `Capabilities`/`degrade`, `window_mode`,
  `routes_to_screens`, `SCREEN_TEXTURE_STEM`, `arc_members_have_screen`, `SCREEN_RT_EXTENT`, `fit_writable`,
  `RouteImm`/`ImmAction`/`imm_action`, `keys_list` + host tests. `style.rs` hint + WARN spellings.
  `movie_backdrop::live_movie_actor()`. NEW `screen_route.rs` (init/arm/disarm/on_frame + per-actor INFO + one-shot
  entry-10 INFO). `custom_scan::read_arc_members` → `pub(super)`. `lifecycle.rs`: `Tables.screen_stages` +
  `scan_screen_stages` enable INFO, `stage_has_screens`, `apply_movie_mode(has_screens)` (degrade + one WARN per
  mode per boot, arm BEFORE size writes, `stage screens: yes/no, routed: yes/no` in the per-song INFO),
  `restore_movie_mode` disarms first. `movie_size::any_shows_movie`. `mod.rs`: `screen_route::init` + `on_frame`
  (frame callback, before `lifecycle::on_frame`). Offline check: exactly the 10 stock `mapset_monitor00..03` /
  `replicant00..05` arcs in the World install list an `offscreen1.dds` member.
- **Step 3** — pure `render_item_layout::materials_sampling` (+ `TEXDATA_W/H`) + tests;
  `render_item.rs`: shared readers `tex_table` / `tex_table_hashes` / `material_slot_indices` (also used by
  `resolve_material_textures` + `texture_readiness`), `RenderItem::materials_sampling`,
  `RenderItem::sampled_texture_info`, `restyle_materials(allows, keep_stock_texture, variant_for)` with
  `RestyleStats.kept_screen` (screen checked BEFORE the blend rule). `session.rs::build_one`: `screen=` in both
  restyle INFOs; per non-hull item with screen materials one INFO (every style, incl. STOCK) with the bound
  `TextureData` size + hash (expect 1280x1280, hash 0x3420C1B9 = FNV-1("offscreen1")).
- **Step 4** — `MovieMode::MovieOnly` (row 4, `movie_only`, `MOVIE ONLY (NO DANCERS)`), `size_override` → None,
  `degrade` needs the probe only, `probes_backdrop`, `SceneMask.dancers` + `NOTHING`, `scene_mask` MovieOnly rule,
  tests. `director::produce` hides bodies + parts on `!mask.dancers`. `lifecycle::drive_live` probes on
  `probes_backdrop`, log names the mode. `style.rs` 5-value hint (147 chars — the footer is one line).
- **Step 5** — `export_model.py`: `SCREEN_TEXTURE_KEY`, `is_screen_texture` (fold = lower + `_` stripped),
  `write_textures_for` writes an 8×8 opaque-black `offscreen1.dds` instead of the image. `synthetic_test.py`: two
  cases (`offscreen1`, `OffScreen_1.001`) — texname `offscreen1`, DDS 8×8, no other DDS. README "Stage screens"
  section. `scripts/validate_blender_addon.sh <temp unpack of A3 pl_emi00/pl_rinon00*/mc_female/mapset_boom00/
  camera + startup>` → PASS (smoke, character, synthetic). `scripts/build_blender_addon.sh` packages fine.
- **Step 6** — script `~/blender-projects/ddr-peter-griffin/livingroom/work/tv_offscreen1.py` (outside the repo;
  `-- --dry` re-exports v2 unchanged: byte-identical to the shipped model → no exporter drift). Real run: image
  `offscreen1` 1024² (old screenshot letterboxed into the 16:9 band, Blender-only preview), UVs u 0..1 / v band,
  saved `out/living_room_game_v3.blend` (v2 kept), export to `out/export_v3/`. Verified: mesh 11 texture
  `offscreen1`, shader `mdl_bg_constant_vc`, TEXCOORD0 u 0–1, v 0.21875–0.78125 (D3D), the other 14 meshes'
  vertex/index counts identical to the shipped model, `write_model(model_to_spec(m)) == data`, only the `.model`
  changed + `offscreen1.dds` (8×8, 464 B) new. Copied into `data_mods/custom_models/stages/Griffin House/
  mapset_griffin00/gm_griffin00_room/`, `lr_screen.dds` deleted (2.7 MB), `camera/` untouched. Check render
  `out/check_tv_offscreen1.png` inspected: upright, unmirrored, band fills the TV.
- **Step 7** — docs: research §8 (header note, §8.5 heading + fit AOB, §8.6 heading, §8.7 check 5, NEW §8.8 "What
  shipped" incl. deviations), README (Background Movies paragraph five values + config row), AGENTS.md (Background
  Dancers row paragraph + `background_dancers.movie_mode` config entry). Full offline validation green (below).

Final gates (2026-09-23): `cargo check` clean, `cargo fmt`, `./build.sh` clean,
`scripts/validate_background_dancers.sh` 185 passed, `scripts/validate_signatures.sh ~/Desktop/ddr_modules` ALL
GREEN, `scripts/validate_blender_addon.sh` PASS, no new absolute-path hits.

## In flight

- Nothing — waiting for the cabinet pass. Nothing committed (maintainer commits).

## Deploy & test log

- **Cabinet pass 1 (2026-09-23, maintainer):** "Everything worked correctly" — the only oddity: training-mode
  7 (rewind) / 9 (fast forward) scrubs did not seem to keep the background movie in sync the way they normally do.
  The session's `log.txt` was overwritten by a later 16-second launch, so no evidence. Static RE found NO
  route-specific mechanism: entries 9 and 10 are the same `agcs::ScreenRoot` class (`FUN_180217df0`), both
  prepared (`vtbl+0x20`, gate `+0x10==0 && +0x11!=0`, `FUN_180003000`) and walked every frame, so the Movie's
  update (the get-frame / command dispatch / state refresh movie_sync depends on) runs identically; the fit
  fields are consumed only at step 2; the texture allocator (`FUN_180216170`) sets the draw size only when it is
  zero (so seeks cannot reset the fit). Diagnostic build shipped (below).
- **Diagnostic build (2026-09-23)** — per-reset movie snapshots + two movie_sync INFOs on its silent paths.
  Not needed: the maintainer retested on a song whose movie makes sync easy to judge and found scrubs DO keep the
  movie in sync on the stage screens (the first observation was a misread). All diagnostic code removed
  (`movie_sync.rs` restored to HEAD, the mod's song_reset subscription + `screen_route` snapshot code deleted).
  Kept: `movie_px` reads the allocator's pixel size (`+0x2C/+0x30`; `+0x24/+0x28` is the draw size the fit
  rewrites — the earlier per-actor INFO could print the fitted size). Build + 185 host tests green.

The design §7.3 checklist:

1. STAGE SCREENS + Lighting Style STOCK, `DDR_DANCERS_PIN=replicant05` (developer_mode) on a movie song: movie on
   every screen, contained in the square. Log: `stages with screens: monitor00, … replicant05 (10 of N)` at enable
   (11 with Griffin House), `stage screens: yes, routed: yes`, `movie layer select patched 09 -> 0A`, one `MovieActor
   … framed` INFO with the movie size, `layer entry 10: … active nodes >= 1`, `… sample 'offscreen1' … bound texture
   1280x1280 hash 0x3420C1B9`. Song select after: `restored 0A -> 09`.
2. ENDYMION on `replicant05` fills the square screens; a 4:3 song on `monitor00` fills its screen.
3. STAGE SCREENS on a stage without screens ⇒ thumbnail, `stage screens: no, routed: no`, byte never written.
4. Griffin House (`DDR_DANCERS_PIN=griffin00`) with a 16:9 and a 4:3 movie: the TV shows it, unmirrored, upright.
5. MOVIE ONLY: movie song ⇒ stock World look (no 3D, 2D background); non-movie song ⇒ dancers; VIDEO SIZE OFF ⇒
   dancers. Log: `background movies movie_only -- movie is drawn: the whole 3D scene hidden …`.
6. Lighting Style CEL + outlines on a screen stage: screens unlit, no outline around the screen quads; restyle INFO
   shows `screen=1+` on the screen parts.
7. Quick restart and a course on a screen stage (a new MovieActor each time is framed); training-mode seek.
8. Mod disable mid-song, then song select: byte back to `09`, next song stock.
9. Custom Resolution 1080p: same framing.

## Deviations & open questions

- Fit-case d32 offsets are **+14 / +22 / +44 / +58** (the design §4.8 table said +43 / +57 — off by one; the
  pattern is 62 bytes, the d32s follow the `0F 10 81` / `F2 0F 10 89` opcodes). Verified with a capstone/pefile
  check on all five builds.
- The screen texture-size INFO is logged in EVERY style (design put it next to the restyle INFO, which only
  exists for non-stock styles) so cabinet deploy #1 (Lighting Style STOCK) already confirms the binding.
- `kept_screen` counts every screen material (checked before the blend rule), so additive / alpha screen
  copies show under `screen=` rather than `blend=`.
- `screen_route::on_frame` also resets its per-actor memory when the step goes BACKWARDS (a new MovieActor at a
  reused address), and logs "no movie drawn" for an actor that ends at step 4.
- ADDED gate: STAGE SCREENS is not routed (song = THUMBNAIL, note `no entered side shows a movie`) when no entered
  side's VIDEO SIZE shows a movie (`movie_size::any_shows_movie`) — nothing to route, and on 20250805 the
  SceneManageActor builds a MovieActor even for VIDEO SIZE OFF, which the patch would then put on the screens.
  Residual edge (not handled): 2P versus on 20250805 where the GOVERNING side has VIDEO SIZE OFF and the other
  side ON — that build's MovieActor would be routed onto the screens anyway.
- Texture-hash assumption re-checked in Ghidra (20260825 `FUN_180273e20` / `FUN_180275ad0`): the KTMDL texture
  name is a 6-bit packed lowercase/digit string (no `_`), hashed by the same gs hasher (`DAT_1806f2040`) the
  shader path uses (FNV-1) — the table key of `offscreen1` is FNV-1("offscreen1") = 0x3420C1B9. The cabinet INFO
  confirms.
- Griffin export note: `write_textures=True` re-writes every DDS; all were byte-identical to the shipped ones, so
  only the `.model` changed. The DLL's custom-content cache repacks the stage arc on the next boot (fingerprint =
  member paths + mtimes — the copy touched every mtime, so the repack WILL happen once).
- A3 test data for the add-on validation was unpacked into the opencode temp dir (`a3data/`): chara/pl_emi00,
  pl_rinon00 + 7 parts, mc_female, map (boom00), camera (music_lesa, music_butt2, stage_camera), and `startup/`.

## Key facts for a cold resume

- Layer select (20260825 `0x18007cfb2`): `MOV RDX,[rip+layer_table]` at match−0x1B, draw-prio store at match−0x11,
  `CMP [RCX+0x148],R8B; MOV EAX,9; CMOVNZ RAX,R8` — write only byte match+8 (`09 ↔ 0A`).
- Fit: origin f64[3] @ +0x108, size f64[3] @ +0x120; consumed only while MovieActor step == 2 (msg 0x1045).
- Thumbnail flag `MovieActor+0x148` beats the patch — the routed song must have VIDEO SIZE FULLSCREEN (1).
- If screens stay restyled on the cabinet, the hash assumption is wrong: compare the logged `TextureData` hash
  with 0x3420C1B9 (the INFO flags a mismatch as `(NOT the offscreen1 key)`).
