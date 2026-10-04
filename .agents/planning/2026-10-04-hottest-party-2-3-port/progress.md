# Progress — HOTTEST PARTY 2 / 3 (FuruFuru Party / MUSIC FIT, Wii JP) dancer + stage port

Updated: 2026-10-04
Status: dancers ported (full runs in flight); stages ~half ported (two full runs aborted on asserts); tests/docs pending.
NEXT ACTION: fix the two stage asserts (below), re-run `GAME=hp2|hp3 STAGES=all` port_stage_hottest2.py, then tests → docs.
Resume protocol: read this file, then `scripts/zan_dump.py` docstring (formats), then the two port scripts' docstrings.
No commits — maintainer commits manually.

## Inputs
- Dumped discs (via `scripts/extract_wii_ddr_data.py disc`): `~/Desktop/DDR Wii ISOs/Furu Furu Party (Japan)` (RD4JA4) and
  `~/Desktop/DDR Wii ISOs/Music Fit (Japan)` (RJRJA4). The .wbfs files are on `~/Desktop` itself. The maintainer renamed
  `sys/main.dol` to `sys/ddr_furu_furu_party_main.dol` / `ddr_music_fit_main.dol` (also loaded in Ghidra project DDRWorld_Ghidra).
- Output: `data_mods/custom_models/{dancers,stages}/HOTTEST PARTY 2|3/`.

## Done (new files, all uncommitted)
- `scripts/zan_dump.py`: Konami zan engine decoder — `WII\0` archives (name-stride heuristic), ZMB (textures/materials v1 0x38 & v3
  0x50/nodes 0xA0/submeshes/strips/skin by joint name), ZAB (T/R(quat x,y,z,w, row-vector)/S keys), cameras (f32 length + 6 tracks,
  signature at 0x34; NOT an `@@` magic), material_mode (from main.dol FUN_8010def4/FUN_8010dec8: flags[2]&0x7F blend 0 opaque/1 add/
  2 darken/3 alpha, bit7 soft vs alpha-test 160; flags[1] two-sided; flags[0] lit; flags[3]≠0 = effect pass), rig/pieces/weld,
  bake_overlay (eyes/mouth UV-1 overlay baked into one picture), chain_motions, clip_bars, ssq_tempo, chunk_take (~8 bars),
  deal_rotating, stage_sources/stage_instances (COL `OBJSET_<key>_<nn>` → prop `OBJ[AB]_[NZS]_<key>*`), v3 UV keys, `survey` CLI.
  Survey of both discs: 0 problems. Docstring has a stale bit: node +0x98 is u16 flags (1 = sprite) + u16 nsub (code is right).
- `scripts/extract_wii_ddr_data.py`: zan support — `is_zan_game`, `extract_zan_archive` (members + archive.json, --png), `archive`
  subcommand, `extract` handles zan trees (skips HP1 DOL tables). Not yet run over a whole disc.
- `tools/blender_ddr_addon/examples/port_character_hottest2.py` (GAME=hp2|hp3): CASTS table (HP2 76 dancers, HP3 135), accessories
  (`accessory/<Joint>_<code>*.bin`, wii_con excluded, additive glows skipped), library = song MOT files chained+chunked, PER_DANCER=12,
  previews verified (bodies, faces/eyes, dances, accessories all good). ~4.6 MB per dancer.
- `tools/blender_ddr_addon/examples/port_stage_hottest2.py`: entries per (model, OBJSET instance), parts/loops/UV sanm, `_st` = stage
  cams, `_non` = GAME_DEF_CAM.bin /#0 (31 HP2 / 60 HP3); FOV treated as vertical fovY. HP3 skips stages byte-identical to HP2 (STG000).
  Previews of ~17 stages look correct (HP2 0,1,9,11,21,27,42,47,103; HP3 41,101,104,105,109,111,201,204,206).
- `scripts/test_zan_formats.py`: 19 synthetic tests, 18 pass.

## In flight / failing (as of handoff)
- Background full runs, logs in `$TMPDIR/opencode/hp/full_{d2,d3,s2,s3}.log` (macOS: /var/folders/.../T/opencode/hp):
  dancers hp2 + hp3 still running (no errors); stages hp2 died at STG030, hp3 at STG049 (stages before those are written).
- STG030 (hp2): `exporter re-framed the bind` — Blender bone-roll numerics differ by >1e-3 for some binds (seen 2e-4 at 180° rotations).
  Fix idea: drop the bind assert to a WARN and instead compute loop worlds relative to `file_binds` (the exporter's) — i.e. use
  fb instead of mb in loop_spec/expected, so the mesh and anim agree with what was written.
- STG049 (hp3) add part: loop rotation error 0.08 — likely a slerp across >180° key spacing or a mirrored/zero-scale anchor where
  `rigid_row`'s per-frame sign flip changes between frames (quaternion of the flipped matrix). Investigate that anchor; maybe
  decompose with a fixed sign per anchor (decide the flip at frame 0, keep it).
- test_extractor_unpacks fails: `archive()` test builder calls `.encode()` on a None name when name_words>0 — give unnamed members ''.
- Stage label 'Stage %02d' → HP2 'Stage 00..55, 101..103'; HP3 'Stage 41..55, 101..111, 201..206'.

## Remaining
1. Fix the two asserts, re-run stages (both games), check every stage's preview (`PREVIEW=1 PREVIEW_DIR=...`).
2. Confirm dancer runs finished cleanly (counts 76 / 135), spot-check a few HP3 previews.
3. Extend `scripts/validate_wii_ddr_tools.sh`: run test_zan_formats; add a zan disc survey leg (zan_dump.survey + every CHR*0 rig ≤ 64 joints,
   every OBJSET matched).
4. Run `scripts/validate_background_dancers.sh` (catalog/labels/keys) — labels ≤15 bytes, keys unique.
5. Docs: new `docs/wii_ddr_hottest_party_2_3_research.md` (formats, Ghidra findings above, naming evidence, open questions), add a
   section to this file's sibling summary / `.agents/planning/2026-09-30-custom-model-sources/progress.md` "In flight", README/addon
   playbook mention. Note sizes (HP3 dancers ~620 MB) and the duplication (HP3 re-ships HP1/HP2-era outfits).
6. `git grep -nE "/(Users|home)/[^/ ]+/" -- . ':!target'` must add no new hits.

## Key facts for a cold resume
- Scale: Hips 8.593 units → 0.97 m (`GAME_SCALE`); file frame = World's (Y up, +Z forward, left +X); stage units same scale.
- Characters: CHR<nn>0 = {body ZMB, head ZMB (rigid on mii_head)}; CHR<nn><k> = {body TPL, head TPL}; material texture index = TPL index.
- Names: leads 01..08 = Rena, Domi, U.G., Root, Chordia, Harmony, Gaku, Danca (name plates + portraits agree in both games);
  specials s_01/02/03 = NAOKI, U1, jun; HP3A_09/10/11 = Hip, Nova, Hop (portrait order); back-ups unresolved → Backup A–D.
  Corollary for HP1's port: its Emi/Jenny/Afro/Rage/Dancer A–D are Rena/Domi/U.G./Root/Chordia/Harmony/Gaku/Danca (not renamed).
- Mii bodies (HP2 CHR51–54, HP3 CHR81–88) have no head: not ported. Stage `_S` files and COL meshes not ported; movie screens
  (`*_MOV*` props) render as plain white (offscreen1 mapping = follow-up); texture flip-books keep frame 0.
