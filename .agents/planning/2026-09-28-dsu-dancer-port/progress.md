# Progress — DSU dancer port + DSU-exact cel/outline

Updated: 2026-09-28
Status: Step 5 of 5 — done (cabinet-validated by the maintainer 2026-09-28).
NEXT ACTION: none — the maintainer commits. Optional follow-ups under Open.
Resume: read `design.md` (decisions D1–D4) and `docs/dancing_stage_unleashed_dancers_port_feasibility.md`.

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
