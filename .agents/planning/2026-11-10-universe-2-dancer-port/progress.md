# Progress — DanceDanceRevolution Universe 2 (Xbox 360, PAL) dancer port

Updated: 2026-11-10
Status: all 29 dancers ported and round-trip previewed; not yet cabinet-tested. Uncommitted
(maintainer commits manually).
NEXT ACTION: maintainer — deploy `data_mods/custom_models/dancers/UNIVERSE 1-3/` (the whole folder:
U1's 25 moved with the rename; DLL unchanged) and run the watch-list below.
Resume protocol: this file → `docs/dancing_stage_unleashed_dancers_port_feasibility.md` §15 → the
docstring of `tools/blender_ddr_addon/examples/port_character_universe2.py` (which drives
`port_character_universe.py`, the Universe 1 port; `.agents/planning/2026-11-01-universe-dancer-port/`).

## Decisions (2026-11-10, maintainer)
- The Universe 1, 2 and 3 dancers share one source folder: `dancers/UNIVERSE` → `UNIVERSE 1-3`
  (git mv). U1's `Queen` → `Queen 1`; keys are unchanged, so only the row labels move. Universe 3
  is tracked in `.agents/planning/2026-11-10-universe-3-dancer-port/`.
- Emi / Rage / Afro / Honey costumes A–D are U1's meshes in new colourways. They ship as
  `<Name> <n> Alt`. Costume E is new: `<Name> 5`.
- B and Queen have four new costumes each: `B 5..8`, `Queen 2..5` (numbered after U1, UMX4 style).
- The create-your-own avatar parts (22 + 22 bodies, 5 + 5 heads × 4) are not ported, like U1's
  back-up dancers.

## Done
- `scripts/extract_ultramix_data.py universe2_eu` (`x_data_XENON_EU.*`, 2439 files).
- `port_character_universe.py`: disc settings (`MODELS_CSV`, `ANIMATIONS_CSV`, `COSTUME_COLUMNS`,
  `TEXTURE_PREFIXES`, `EXTRA_TEXTURE_PREFIXES`, `TITLE`, `SCRIPT`) and an explicit texture
  prefix are now module settings. Defaults reproduce the U1 port (SOURCE now `UNIVERSE 1-3`; Queen
  labelled `Queen 1`).
- `port_character_universe2.py` (new): label/key plan, column-5 repeat check, `PLAN_ONLY=1`.
- U1 vs U2 comparison: U2 A–D triangle counts match U1 within 0.4% (Emi 1: 3.4%). Head and face
  textures match (mean |Δ| < 0.4/255); outfit textures differ (mean |Δ| 17–140). U2 clips are
  U1's takes: body tracks are identical, skirt / breast chains re-done.
- Port run (~11 min): 29 dancers, 421 clips, max joint error 0.14 mm, 6–11 meshes, palettes ≤ 52,
  221 MB. B / Queen column 5 skipped by the repeat check. Previews checked for all 29 (contact
  sheet). 209 new texture stems, no registry-key collision with any shipped custom model.
- Docs: RE note §15 (+ §14 / TL;DR path updates), add-on README section.

## Deploy & test log
- (none yet)

## Cabinet watch-list
- The 29 new rows appear under DANCER SOURCE = UNIVERSE 1-3 next to U1's 25, which keep their
  labels apart from `Queen 1`. A saved per-source selection of the old `universe` slug falls back
  to the default (the slug is now `universe_1_3`).
- 87 / 113-bone rigs, the same as U1 (U1's watch-list still applies; neither has been on a
  cabinet yet).
- Translucent lenses: Honey 2 Alt, B 8, Queen 2 (`GLASS_ALPHA` 0.5).

## Deviations & open questions
- RENDER `specular` (new in U2) is drawn like `toon`; the `*_spe` maps are unused. Its look is in
  `default.xex` (the maintainer offered to load it in Ghidra).
- `z2_Britney_*`: an unused near-copy of Angel, named only in leftover `___z2_default_models___*`
  tables. It is not ported.

## Key facts for a cold resume
- Rip: `~/Desktop/universe/ddr_universe_2.iso` (PAL, XGD2) → `disc/` (xdvdfs_extract) →
  `extracted/` (extract_ultramix_data `universe2_eu`).
- Re-run: `PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup
  --python tools/blender_ddr_addon/examples/port_character_universe2.py` (`DANCERS=emi:5,b:2`
  for a subset).
