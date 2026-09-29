# Progress — DSU dancer port + DSU-exact cel/outline

Updated: 2026-09-28
Status: Step 5 of 5 — done (cabinet-validated by the maintainer 2026-09-28). Follow-up DSU2 port: content done, not yet cabinet-tested.
NEXT ACTION: cabinet test of the six `UMX2 *` dancers (pin e.g. `DDR_DANCERS_PIN=umx2emi00,umx2robo00,umx2maid00`); the maintainer commits.
Resume: read `design.md` (decisions D1–D4) and `docs/dancing_stage_unleashed_dancers_port_feasibility.md` (§11 = DSU2).

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
