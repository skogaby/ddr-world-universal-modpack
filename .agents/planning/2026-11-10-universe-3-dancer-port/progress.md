# Progress — DanceDanceRevolution Universe 3 (Xbox 360, NTSC-U) dancer port

Updated: 2026-11-10
Status: all 9 dancers ported and round-trip previewed; not yet cabinet-tested. Uncommitted
(maintainer commits manually).
NEXT ACTION: maintainer — deploy `data_mods/custom_models/dancers/UNIVERSE 1-3/` (the whole folder;
it holds Universe 1, 2 and 3; DLL unchanged) and run the watch-list below together with the
Universe 2 one (`.agents/planning/2026-11-10-universe-2-dancer-port/progress.md`).
Resume protocol: this file → `docs/dancing_stage_unleashed_dancers_port_feasibility.md` §16 → the
docstring of `tools/blender_ddr_addon/examples/port_character_universe3.py`.

## Decisions (2026-11-10, maintainer)
- The cast is all new (no repeats of Universe 1 / 2), but it is only nine dancers, so it joins
  the shared source folder: `UNIVERSE 1-2` (never committed) → `UNIVERSE 1-3`.
- The avatar parts are not ported (as for Universe 2).

## Done
- Extracted with `extract_ultramix_data.py universe_us` (Universe 3 keeps Universe 1's US names;
  7318 files) into `~/Desktop/universe/u3/extracted`.
- `port_character_universe3.py` (new): drives `port_character_universe.py` with Universe 3's
  tables, labels from the NAME column in title case, `PLAN_ONLY=1`.
- Repeat check: no texture is byte-identical to a Universe 2 texture; the skeleton is
  identical (`.b2it`); the clips have Universe 2's frame counts.
- Port run (~3 min): 9 dancers, 129 clips, max joint error 0.14 mm, 6–13 meshes, palettes ≤ 52,
  65 MB. 72 new texture stems, with no registry collisions. Previews checked for all nine.
- Docs: RE note §16, add-on README section; every `UNIVERSE 1-2` reference renamed to `UNIVERSE 1-3`.

## Deploy & test log
- (none yet)

## Cabinet watch-list
- The 63 rows (U1 25 + U2 29 + U3 9) appear under DANCER SOURCE = UNIVERSE 1-3.
- Simba: his hair passes ~0.35 m below the floor during `z3_M_BRK02` (an inverted break move);
  kept as the data has it.

## Deviations & open questions
- `data/add01dat.bin` (0.5 MB, new on the Universe 3 disc) was not examined.
- `z3_avt_bd_m39_f.ddm` fails the parser's end-of-file check (avatar part; not ported).

## Key facts for a cold resume
- Rip: `~/Desktop/universe/ddr_universe_3.iso` → `~/Desktop/universe/u3/disc` →
  `~/Desktop/universe/u3/extracted`.
- Re-run: `PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup
  --python tools/blender_ddr_addon/examples/port_character_universe3.py` (`DANCERS=simba,miki`).
