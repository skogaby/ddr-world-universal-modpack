# Progress — HOTTEST PARTY 4 / 5 (Wii, EU) dancer + stage port

Updated: 2026-10-04
Status: all ported + host-validated; NOT cabinet-tested. Uncommitted (maintainer commits manually).
NEXT ACTION: maintainer — deploy `data_mods/custom_models/{dancers/HOTTSTPARTY 4,dancers/HOTTSTPARTY 5,
stages/HOTTEST PARTY 4,stages/HOTTEST PARTY 5}` and run the cabinet watch-list below; then commit.
Resume protocol: this file → `docs/wii_ddr_hottest_party_4_5_research.md` → the docstrings of
`port_character_hottest2.py` / `port_stage_hottest2.py` (`GAME=hp4|hp5`) and `zan_dump.plan_stage_ports`.

## Maintainer decisions (2026-10-04)
- Dancers split per game: `dancers/HOTTSTPARTY 4` (32) and `dancers/HOTTSTPARTY 5` (18). Neither cast
  contains the other or is part of HOTTSTPARTY 1-3.
- HP5's NAOKI / jun / U1 (HP4 outfits with retouched heads) are shipped in HP5 too.
- Stage labels stay `Stage NN`. The text bank's 34 stage names are not yet mapped to files.
- The fan wiki gallery (videogames-fanon "Hottest Party Dance/Gallery") is a naming cross-check. It
  confirms HP4 Rena / U.G. / Root / Chordia / Harmony and has nothing for HP5.

## Done
- Discs dumped (`extract_wii_ddr_data.py disc`). `extract` now skips the discs' `.svn/` copies. Full
  `extract --png` of both discs is clean.
- zan_dump:
  - second camera signature (`CAM_SIGNATURES`);
  - stages in named `STG<nnn>_MDL.bin` archives;
  - packed material +0x14 word (eye / mouth layer counts, which fixed blank faces);
  - `material_group` / `is_screen_material` (group 92);
  - `stage_signature`, `plan_stage_ports`;
  - `STAGE_FILE`.
- Dancer port (`GAME=hp4|hp5`): CHR<id><k:02> naming, `CAST_HP4` / `CAST_HP5`, the dance/ library
  (HP4: ss4 tempo, `L` alternates; HP5: tempo from piece length), no skin tone when the body has no
  group-2 material, HP5 using HP4's accessory files.
- Stage port (`GAME=hp4|hp5`):
  - `planned_stages`, with a `RECORDED_PLAN` fallback checked equal to the live plan;
  - group-92 screens → `offscreen1` (blend and alpha kept);
  - unplaced props dropped;
  - fallback main cameras (GAME_DEF_CAM #78..#85) for stages with fewer than 3 moving shots.
- Tests: `test_zan_formats` 28/28 (new `TestHottestParty45`). `validate_wii_ddr_tools.sh` handles HP4 /
  HP5 discs (0 problems on both). A format / layout check of all 4 new sources found 0 problems:
  184 models round-trip, ≤ 64 bones, palettes ≤ 52, role bones present, 600 clips + 58 loops + 72 sanm
  + 3923 camanm parse, sidecar parts = dirs, keys valid and unique across all sources.
- Regression: a `GAME=hp3` re-port of Rena 5 / Stage 101 is byte-identical to the shipped output.

## In flight
- Nothing. Sizes: dancers HP4 106 MB, HP5 63 MB; stages HP4 98 MB, HP5 61 MB.

## Deploy & test log
- 2026-10-04 first cabinet look: dancers fine. Stages: large white circles / squares (HP4); 201 and 301
  identical; sparse 008 / 200 / HP5 013 / 018; HP5 426 empty. Causes and fixes (research §3.2b / §3.2c):
  - the additive vertex alpha was forced to 1, in all HP1–HP5 ports;
  - group-91 stage-video surfaces were left as white cards (now 16-frame video flip-books);
  - flip-book lists of 64 or more entries were dropped;
  - 201 = 301 at half resolution, so 201 is dropped;
  - 008 / 200 are minimal on the disc.
  All HP1–HP5 stages re-ported; HP3 also gets its group-92 TV screens (maintainer decision).

## Cabinet watch-list
1. STAGE SCREENS on HP5 STG016 (100-box wall, each box a window of the movie), STG028–030 (PV monitors),
   STG015 (planets), HP4 STG044 (dome, ~1/3 strength), STG001 / 405 / 431.
2. Fallback cameras on HP5 stages and HP4 STG4xx: the front shots frame the dancers.
3. Faces on all dancers (the eye / mouth bake depends on the packed-count fix).
4. Naming: HP4 Dyna / Bridget / Ceja / Bossa / Nova / Hip / Hop; HP5 DISCO / EMI / RUBY / RAGE / YUNI.
5. HP4 choreography tempo (ss4 BPM) and the HP5 tempo guessed from piece length.

## Key facts for a cold resume
- Discs: `~/Desktop/DDR Wii ISOs/Hottest Party 4 (Europe)` (SDYPA4), `.../Hottest Party 5 (Europe)` (SURPA4).
  The stage planner also reads `.../Dance Dance Revolution - Furu Furu Party (Japan)` / `- Music Fit (Japan)`.
- HP4 CHR72/73 = MUSIC FIT CHR49/50 byte for byte (not re-ported).
- Group 92 is used by MUSIC FIT too. Its shipped port has no group-92 screens (open: re-port `GAME=hp3`).
