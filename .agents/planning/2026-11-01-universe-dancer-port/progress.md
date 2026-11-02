# Progress — DanceDanceRevolution Universe (Xbox 360, NTSC-U) dancer port

Updated: 2026-11-01
Status: all 25 dancers ported and round-trip previewed; not yet cabinet-tested. Uncommitted
(maintainer commits manually).
NEXT ACTION: maintainer — deploy `data_mods/custom_models/dancers/UNIVERSE/` (DLL unchanged) and
run the cabinet watch-list below, first of all the 87 / 113-bone rigs.
Resume protocol: this file → `docs/dancing_stage_unleashed_dancers_port_feasibility.md` §14 → the
docstring of `tools/blender_ddr_addon/examples/port_character_universe.py`.

## Decisions (2026-11-01, maintainer)
- All four costumes of Emi, Rage, Lady, Afro, Honey and B are ported, each as its own dancer
  (`<Name> 1..4`, keys `univ<type>0<n>`). Every costume is a separate model on the disc. Queen's
  four costume columns are one model with one texture set, so she ships once (`Queen`,
  `univqueen01`).
- The 20 generic back-up dancers (`z_b[fm]_body` + 10 heads each) are NOT ported. They were
  reviewed on a contact sheet (one blue suit, only the head changes).
- New source folder `UNIVERSE` (its own DANCER SOURCE row). If Universe 2 / 3 are ported later, it
  can become `UNIVERSE 1-3`.

## Done
- `scripts/xdvdfs_extract.py` (new): `list` / `extract` with globs for Xbox / 360 XDVDFS images
  (plain XISO + XGD1/2/3 partition offsets). Universe is XGD2 (`0x0FD90000`).
- `scripts/extract_ultramix_data.py universe_us`: x_data only (`sng` may now be an empty list).
- `scripts/ultramix_k3d_dump.py`: `.ddm` 0x112 palette groups (UMX4 = 1 group, unchanged in
  effect), odd index padding, zero-normal repair in `game_mesh`, `HIERARCHY_UNIVERSE` +
  `k3d_hierarchy`, `survey` command; fixed `hierarchy`'s report using the last clip's table.
- `scripts/test_k3d_formats.py` + `scripts/validate_k3d_tools.sh`: host tests (10) + rip survey,
  green (47 dancer models, 26 quest props, 257 clips).
- `tools/blender_ddr_addon/examples/port_character_universe.py`: 25 dancers, 365 clips, max joint
  error 0.14 mm, ~19 min with previews. Round-trip previews (re-imported export, mid-clip) checked
  for all 25. Output 203 MB (textures 125, clips 75, models 11).
- Docs: RE note §14, add-on README section.

## Deploy & test log
- (none yet)

## Cabinet watch-list
- **Bone count**: 87 (male) / 113 (female) posed bones per dancer, more than any earlier port
  (≤ 59). The DLL buffers follow the plan and the frame board holds 256, but this size has not
  been seen on a cabinet for a dancer. Watch for missing limbs, a collapsed mesh or WARNs from
  `background_dancers` / `scene3d` on load.
- The 25 rows appear under DANCER SOURCE = UNIVERSE with their labels.
- Dance speed matches the UMX4 / other dancers with BPM Sync off (15 Hz keys every 4th frame).
- Translucent lenses (Honey 2, Queen) at `GLASS_ALPHA` 0.5; re-port with another value if wrong.
- Sphere-mapped chrome / gold / enamel (Lady, Afro, Honey, B, Queen) reads as metal.
- Floor contact: median lowest vertex −0.025 to −0.055 m (soles); Rage 4 / Afro 1–2 dip to
  −0.13..−0.15 m at the bottom of a move. Kept as the data has it.

## Deviations & open questions
- `semitrans` alpha, the play window, and what the game does with unlisted / missing-texture
  materials are untraced. All are in `default.xex`; the maintainer can load it in Ghidra if
  needed (XEX2 loader).
- Texture stems are `uv<type><n>_<texture>` (≤ 20 alphanumerics): `univ<type>0<n>_…` would
  overflow on `lady_clothbottom_b`.

## Key facts for a cold resume
- Rip: `~/Desktop/ddr_universe/ddr_universe.iso` → `disc/` (xdvdfs_extract) → `extracted/`
  (extract_ultramix_data `universe_us`).
- The data is little-endian; the `.ddm` / `.ani` formats are UMX4's plus palette groups.
- Re-run: `PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup
  --python tools/blender_ddr_addon/examples/port_character_universe.py` (`DANCERS=lady:3,queen`
  for a subset; ~45 s per dancer with previews).
