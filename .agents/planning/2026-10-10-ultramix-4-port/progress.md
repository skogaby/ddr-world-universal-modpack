# Progress — DDR ULTRAMIX 4 (Xbox, NTSC-U) dancer port

Updated: 2026-10-10
Status: all ten dancers ported; cabinet deploy #1 looked right but danced 2x fast -> re-ported at
15 Hz, re-deploy pending. Uncommitted (maintainer commits manually).
NEXT ACTION: maintainer — re-deploy the ten new `data_mods/custom_models/dancers/ULTRAMIX 1-4/*/`
folders (only their `motion/*.anm` changed) and check the dance speed against UMX1-3; then commit.
Resume protocol: this file → `docs/dancing_stage_unleashed_dancers_port_feasibility.md` §13 → the
docstring of `tools/blender_ddr_addon/examples/port_character_ultramix4.py`.

## Decisions (2026-10-10)
- No stages: UMX4 has no 3D gameplay stages. The only `mrdd` models are the Quest-mode city map
  (+ lights / ferris wheel) and `x_panel.ddm` (the dance mat). Not ported (RE note §13).
- One source folder for the whole series: `dancers/ULTRAMIX 1-3` → `ULTRAMIX 1-4` (git mv). UMX4's
  returning characters are NEW models, so they ship as numbered entries: `Lady 4`, `Honey 2`, `B 2`,
  `Maid-Zukin 3`, `Konsento 2` (the old `Honey`, `B`, `Konsento` became `Honey 1`, `B 1`,
  `Konsento 1`). New characters: `Yuni`, `Akira`, `Boldo`, `Charmy`, `Astro`. Keys `umx4<name>00`.
- The source slug changes (`ultramix_1_3` → `ultramix_1_4`), so a saved pick inside that source
  falls back to its default once.

## Done
- `scripts/extract_ultramix_data.py ultramix4_us` (multi-`.sng` support, root voice bank); rip unpacked
  to `~/Desktop/DDR ISOs/ultramix_4/extracted_full`.
- `scripts/ultramix_k3d_dump.py`: `.ddm` revision 4 (`_parse_ddm4`, `vertex_influences`, 4-weight
  `skin_pose` / `game_mesh`, `bind_to_clip`), `HIERARCHY_UMX4` / `umx4_hierarchy`,
  `hierarchy_order(names, hierarchy)`, `hierarchy` command picks the table. DSU1-3 paths unchanged
  in effect (same weights / math).
- `port_character_ultramix.build_armature(..., hierarchy=None)`.
- `port_character_ultramix4.py`: 10 dancers, 149 clips, max joint error 0.12 mm, ~80 s with previews.
  Previews (front + mid-clip, head close-ups) checked for all ten.
- `validate_background_dancers.sh` green.
- Frame-rate fix: UMX4 clips are 15 Hz (4:1 decimations of World takes; `ultramix_k3d_dump.py match
  ... 4`), keys now every 4th World frame (`FRAME_STEP_UMX4`, `ani_to_anm_spec(frame_step=)`,
  `port_character_ultramix3.check_clip(frame_step=)`). Exported clips vs World takes: r = 0.99.

## Deploy & test log
- 2026-10-10 #1 (30 Hz keying): all ten look right; dances ~2x faster than UMX1-3 / other dancers
  (BPM Sync off). Cause: the clips are 15 Hz. Fixed, re-ported.

## Cabinet watch-list
- The ten new rows appear under ULTRAMIX 1-4 with their labels; the renamed `Honey 1` / `B 1` /
  `Konsento 1` still load.
- 57-bone female rigs (Yuni, Lady 4, Honey 2, Charmy, Maid-Zukin 3) animate skirts / hair / wings.
- Chrome matcap materials (Konsento 2 pipes, Boldo / B 2 glasses, Charmy brim, Astro visor) read OK.
- Floor contact: Konsento 2 (median −9 cm) and Charmy (−8 cm) sit lowest; the data has them there.

## Key facts for a cold resume
- One UMX4 `.ddm` holds all 4 costumes; `<COSTUME>.csv` hides materials per costume. Costume 1 only.
- Clip pool = gender group + `unisex` from `animations.csv`, whole clips from frame 0, 15 Hz keys
  (every 4th World frame).
- Re-run: `PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup --python
  tools/blender_ddr_addon/examples/port_character_ultramix4.py` (`DANCERS=lady,robo` for a subset).
