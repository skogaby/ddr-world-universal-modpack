# Progress — DSU dancer port + DSU-exact cel/outline

Updated: 2026-09-29
Status: Step 5 of 5 — done (cabinet-validated by the maintainer 2026-09-28). Follow-up DSU2 and DSU3 ports: content done, not yet cabinet-tested.
NEXT ACTION: cabinet test of the `UMX2 *` and `UMX3 *` dancers (pin e.g. `DDR_DANCERS_PIN=umx3afro00,umx3robo00,umx3honey00`; UMX3 is the first multi-mesh UMX port — copy the whole folders, incl. 4 `.dds` each); the maintainer commits.
Resume: read `design.md` (decisions D1–D4) and `docs/dancing_stage_unleashed_dancers_port_feasibility.md` (§11 = DSU2, §12 = DSU3).

## Done
1. **D1 own motion pool.** Files: `selection.rs` (`DancerCandidate.motion`, `playlist_for`,
   `own_motion_from_members`, `clip_member` / `motion_arc_name`), `custom_content.rs` (the planner
   fills `motion`), `pick.rs` (motion arc listing, summary), `session.rs` (clips read from the body arc,
   plus a warning when a clip targets bones the rig lacks). The harness has 4 new tests and is green.
2. **D3 converter.** `scripts/ultramix_k3d_dump.py`: `game_space`, `game_bind_matrices`, `game_mesh`,
   `hierarchy_order`, `clip_game_worlds`, `ani_to_anm_spec`. The Blender port script is
   `tools/blender_ddr_addon/examples/port_character_ultramix.py`.
   - Output: `data_mods/custom_models/dancers/Ultramix {Afro,Lady}`. Afro has 35 bones and 14 clips,
     lady 32 bones and 22 clips.
   - Every clip's joint error is < 0.15 mm.
   - The Rust evaluator matches Python on a shipped clip.
   - A Blender re-import round trip renders correctly.
3. **D2 b2it role aliases.** Hips→root, Spine2→Sternum, Left/RightToeBase→Toe_L/R, written by the
   port script.
4. **D4 shader and DLL.**
   - `mdl_cel.hlsl` is rewritten to DSU's toon shading and outline. Blobs are rebuilt with fxc; only the 9
     cel/outline blobs changed.
   - `outline.rs` now plans one black hull. `style.rs` drops the OUTLINE STYLE row and both width rows.
     `config.rs` keeps the retired keys only so it can log that they are ignored.
   - `render_item::set_outline_push_scale` replaces `set_outline_width`.
   - README and the add-on README are updated.
    - Checks pass: `cargo check`, `cargo fmt`, `./build.sh`, `validate_background_dancers.sh`
      (196 tests).
5. **DSU2 follow-up (content only, no DLL change).** `scripts/extract_ultramix_data.py dsu2_eu`
   unpacks the DSU2 rip (x_data `.hbn` TOC, `.sng` TOC at 0x800, plus `resource_EU.krc` → DDS and
   `voice/*.BKT` → raw PCM). `tools/blender_ddr_addon/examples/port_character_ultramix2.py` builds
   `data_mods/custom_models/dancers/UMX2 {Afro,Lady,Emi,Rage,Konsento03,Maid-Zukin}`:
   - keys `umx2{afro,lady,emi,rage,robo,maid}00`; rigs of 30–40 bones;
   - 15 / 20 / 12 / 15 / 15 / 12 clips; 89 clips in all, max joint error 0.14 mm;
   - P1 costume only (the maintainer's choice).
   - Missing ancestor and role joints are unweighted helper bones.
   - Clips play whole (DSU2 has no [15, n − 15] trim). Each pool is the union of the dancer's 3
     anim groups.
   - `port_character_ultramix.py` gained a `__main__` guard and a `tex_src` parameter, both
     behaviour-neutral. It was not re-run, because the DSU1 rip is no longer on disk.
   - Blender previews of the re-imported exports look correct for all six.
     `validate_background_dancers.sh` is green.
6. **DSU3 follow-up (content only, no DLL change).**
   - `extract_ultramix_data.py dsu3_eu` unpacks the DSU3 rip into `~/Desktop/dsu3/extracted_full`.
     It uses the DSU2 formats and skips one leaked `// Begin US ` comment entry in the `.sng` TOC.
   - `ultramix_k3d_dump.py` changes:
     - `parse_ddm` reads the multi-material `.ddm` revision. It returns `materials` for both
       revisions; the DSU1/2 path is unchanged.
     - `HIERARCHY` gains the DSU3 joint names. The `hierarchy` check is ≤ 3e-5 over 34 clips;
       the skirt tips are simulated.
   - `port_character_ultramix.py`: `add_role_aliases` and `exported_rig` take an optional
     `aliases` argument. This is behaviour-neutral.
   - `port_character_ultramix3.py` builds `UMX3 {Afro,Lady,Emi,Rage,Konsento,Maid-Zukin,B,Honey}`
     (keys `umx3{afro,lady,emi,rage,robo,maid,b,honey}00`):
     - 25–30 bones and 4 meshes / materials (COSTUME1 textures) per dancer;
     - 14 / 17 / 11 / 14 / 14 / 11 / 14 / 17 clips, 112 in all, max joint error 0.13 mm;
     - only robo needs helper bones (6 of them, binds taken from afro).
   - Blender previews of the re-imported exports look correct for all eight.
     `validate_background_dancers.sh` is green (196).
   - The script's output is deterministic: a re-run is byte-identical.

## Deploy & test log
- 2026-09-28, local CrossOver install.
  - Installed: the new DLL, the cel/outline blobs, and both dancer folders. Changed `mod-config.json`:
    `style = cel`, `outlines = on`, `big_head = off`, `layeredfs.developer_mode = true`.
  - Launched with `DDR_DANCERS_PIN=boom00,umxafro00,umxlady00`.
  - Boot log: both dancers discovered ("14 / 22 own clip(s) in motion/"), the pin was honoured,
    synthesis ran, and the retired-keys note was logged.
  - Backups of the originals: `$TMPDIR/opencode/install_backup/{ddr_world_hook.dll,mod-config.json,blobs/}`.
  - Maintainer's manual test in gameplay: both DSU dancers load and dance their own clips cleanly,
    and the DSU cel shading + outlines look right. PASS.

## Open
- Confirmed in the cabinet test: `.anm` members inside a body arc are harmless to the engine.
- Stage-prop outlines are now DSU-thin (about 0.5 px at 20 m). That is expected from DSU's formula.
- Not ported: DSU's self-shadow map, the per-bar light colour, and the blink texture.
- DSU2 open items:
  - First on-cabinet run of a rig with unweighted helper bones and a helper `root`. This path is
    supported: 6 shipped, cabinet-validated dancers already carry unweighted bones, including an
    unweighted root in all 6. Every UMX2 rig still has a single root.
  - DSU2 costumes P2–P4 would each need a separate key.
  - DSU2's 2:1 PREFER-group weighting is not reproduced.
- DSU3 open items:
  - First UMX port with 4 meshes / 4 textures per body. Multi-mesh custom bodies already ship
    (Miku, Teto, CJ).
  - Robo's plug and base reach 0.22 m below the floor (median). That is DSU3's own clip data.
  - Afro's sunglasses slot draws the `glasses` env-map texture as a plain UV texture.
  - The B' folder / label is `UMX3 B`: the apostrophe is kept out of the path.
  - Not ported: the P2–P4 costumes, the blink, and the 50/50 group-vs-unisex weighting.
