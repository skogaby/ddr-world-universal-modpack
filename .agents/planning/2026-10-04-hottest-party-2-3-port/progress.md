# Progress — HOTTEST PARTY 2 / 3 (FuruFuru Party / MUSIC FIT, Wii JP) dancer + stage port

Updated: 2026-10-04
Status: all ported + host-validated; NOT cabinet-tested. Uncommitted (maintainer commits manually).
NEXT ACTION: maintainer — deploy `data_mods/custom_models/{dancers/HOTTSTPARTY 1-3,stages/HOTTEST PARTY 2,stages/HOTTEST PARTY 3}`
and run the cabinet watch-list below; then commit (the deleted `dancers/HOTTEST PARTY 1|2|3` folders go with it).
Resume protocol: this file → `docs/wii_ddr_hottest_party_2_3_research.md` (formats, Ghidra, naming, subset analysis) →
the docstrings of `scripts/zan_dump.py`, `tools/blender_ddr_addon/examples/port_character_hottest2.py`, `port_stage_hottest2.py`.

## Maintainer decisions (2026-10-04)
- Dancers: ONE de-duplicated source `dancers/HOTTSTPARTY 1-3/` from MUSIC FIT's cast table (HP2's cast is a subset).
  The HP1 port's 40 dancers are RETIRED (deleted); `dancers/HOTTEST PARTY 2|3` deleted.
- Stages: keep per-game folders only where unique. Result: `stages/HOTTEST PARTY 1` (42, untouched), `HOTTEST PARTY 2`
  (59), `HOTTEST PARTY 3` (17 — MUSIC FIT's STG000/041–055 are FuruFuru Party's re-exports, shipped once from HP2,
  which still has their movie screens). HP1 shares almost no art with HP2/HP3 (DDS-payload hashes: a handful).
- Movie screens → `offscreen1`; every texture animation must play (flip-books as atlases).

## Done
- Naming (research §6): MUSIC FIT's name plates + portraits + per-variant renders (+ the fan-wiki gallery). Back-up costume
  files hold two people each (v1/2, v3/4): Pia/Gliss (CHR09/29/49), Forte/Sharp (10/30/50), Bossa/Nova (14/31/54),
  Hip/Hop (15/32/55). CHR42/47/48 = Dyna/Bridget/Ceja (the old "Hip/Nova/Hop" and "Backup A–D" labels were wrong).
  HP1 port mapping: Emi=Rena, Jenny=Domi, Afro=U.G., Rage=Root, Dancer A–D=Chordia/Harmony/Gaku/Danca,
  Backup F 1/3=Pia 2/4=Gliss, Backup M 1/3=Forte 2/4=Sharp.
- Dancers: `port_character_hottest2.py` rewritten for the combined cast (`CAST`, keys `hp<person><nn>`, chronological
  HP1→HP2→HP3 outfit order); library = MUSIC FIT songs + FuruFuru Party pieces MUSIC FIT lacks (1022 clips, 73 HP2).
  137 dancers, 521 MB, labels ≤ 15 bytes, keys unique. Full run clean (~10 min).
- Ghidra (FuruFuru Party main.dol): `_MOV` prop flag (FUN_800386bc), stage movie / PV playback (FUN_8002fbec,
  FUN_800302e8), the texture-matrix update (FUN_800e921c setup / FUN_800e9490): per-axis key counts (0xFF ends an axis),
  flag byte 1 = hold, **GX offset = (-u, +v)** — the first port's u scrolls ran backwards. +0x28 high u16 is a
  material serial, not a flip interval; +0x30/+0x34 are flip END frames (confirmed with STG042's signboard).
- zan_dump: `sample_uv` (game semantics), `texmtx_offset`, `scroll_offset`, `uv_axis_counts`, `flip_book`,
  `flip_index`; docstring fixed (node +0x98 u16 flags + u16 nsub; material fields).
- Stage port: screens (`is_screen`: the `root` quad of `*_MOV*` props → `offscreen1`, 16:9 band, opaque white);
  flip-book atlases (`apply_atlases`: strip along the non-scrolling axis, tile-line clipping, wrap gutters, sampling
  check vs the source = the UV-sign check); one `.sanm` per part incl. static parts, own clip (lcm ≤ 21600 frames),
  exact steps (two keys per frame); parts split at 48 material floats; binds = nearest rotation of the rest world,
  keys = bind·rest⁻¹·world(t) against the exporter's bind (fixes STG030); shear (STG049's 0.08) = a rotating node
  under a non-uniformly scaled parent — not representable in TRS bones; bind turned onto the principal axes, logged
  `SHEAR` (HP2 ≤ 0.55 m, HP3 STG103 ≤ 0.84 m at light-cone tips). HP3 `is_hp2_reexport` skip.
- Tests: `test_zan_formats.py` 21/21 (archive builder None names fixed; UV semantics, sign, phase, flip-books);
  `validate_wii_ddr_tools.sh` runs it + a zan disc-survey leg (both discs 0 problems; MUSIC FIT STG046 has 5
  OBJSET nodes whose screen prop it dropped — counted, not a problem); `validate_background_dancers.sh` 222 OK.
- Docs: `docs/wii_ddr_hottest_party_2_3_research.md`; add-on README sections; root README source mention; HP1
  scripts' stale `DDR HOTTST PRTY` defaults fixed (HP1 dancer port marked retired).

- 2026-10-04 (after the first cabinet look): dark-skinned dancers had pale bodies. Cause: the body's skin material
  (colour group 2 = material +0x28 u16) is a neutral sheet the game tints per costume / variant from a main.dol
  table (Ghidra: FUN_8003ac98 / FUN_800ed73c, MUSIC FIT FUN_8004a7a8). `zan_dump.skin_tone_table` reads it from
  `sys/*.dol`; the port multiplies the tone into the skin material's vertex colours (and now writes the exact
  bytes via `color_srgb`). All 137 re-ported; every model carries its tone. Tests 23/23, survey checks tones.
- 2026-10-04 (later): **stage COLOR0 fix.** `port_stage_hottest2.py` wrote vertex colours through Blender's linear
  `color` accessor, so the shipped bytes were sRGB-encoded (too bright, washed out; research §7.2 has the evidence).
  It now writes `color_srgb`. All 59 HP2 + 17 HP3 stages re-ported (only the `.model` files changed: 226 + 73). Measured:
  shipped triples = disc bytes. The same fix went into `port_stage_hottest.py`, `port_stage_supernova.py` and
  `port_character_supernova.py`. HP1 / SN / X stages and the SN / X GUS glasses were re-ported too, once the
  maintainer re-downloaded the discs (see `.agents/planning/2026-09-30-custom-model-sources/progress.md`). Every
  shipped COLOR0 byte is now the disc's. `scripts/fix_vertex_colour_srgb.py` (the in-place undo) is kept only for
  older-checkout output and refuses to write without `--legacy-port`.

## In flight
- Nothing. Last runs: all 59 HP2 / 17 HP3 stages (then the HP2 flip-book stages + all HP3 again with `ATLAS_MAX = 4096`
  and grid-folded atlases; STG105/111 last). Sizes: stages HP2 115 MB, HP3 198 MB (uncompressed atlases), dancers 521 MB.
  Previews (temp, not in the repo): `$TMPDIR/opencode/hp/prev2|prev3` (screens show a red/blue/yellow test card).

## Deploy & test log
- (none yet)

## Cabinet watch-list
1. STAGE SCREENS mode on HP2 STG046–055 (screens show the song movie; STG054 = an 18-screen wall; STG047's floor is a TV).
2. Flip-books animate and step cleanly (STG044 rings, STG042 signboard, HP3 STG101/103/106/109 — 30–69 flip materials).
3. UV scroll directions (u sign flipped vs the first port) — e.g. STG042 signboard chevrons slide the way they point.
4. Parts / instance budget: HP3 STG108 has 9 parts, STG110 8.
5. SHEAR stages (HP2 STG030/049, HP3 STG103): light cones slightly mis-shaped at their tips.
6. Dancer naming / sex / choreography (HP2-only songs h2s* clips) and the HP1-outfit remakes.
7. Skin tones: body matches face on U.G., Harmony, Danca, Chordia, Root, Sharp, Gliss, Ceja.
8. Stage vertex colours: HP1 / HP2 / HP3 stages look deeper and more saturated than before (correct: the disc bytes),
   not murky. Edited models re-pack on their own (the cache fingerprint folds the file mtimes).

## Key facts for a cold resume
- Scale: Hips 8.593 units → 0.97 m (`GAME_SCALE`); file frame = World's (Y up, +Z forward, left +X).
- Costumes: CHR<nn>0 = {body ZMB, head ZMB (rigid on mii_head)}; CHR<nn><k> = {body TPL, head TPL}. Eras: CHR2x/3x HP1
  outfit, 0x/1x HP2, 4x/5x HP3; Mii bodies CHR81–88 not ported.
- Discs: `~/Desktop/DDR Wii ISOs/Furu Furu Party (Japan)` (RD4JA4), `.../Music Fit (Japan)` (RJRJA4); Ghidra project
  DDRWorld_Ghidra has both main.dols (SDA2 r2 = 0x802DF580 in FuruFuru Party's).
- `.sanm` is evaluated by the DLL (`src/core/anm/sanm.rs`, director.rs) on its own clip length; World's own `.tanm`
  (texture tracks) is dead — hence atlases.
