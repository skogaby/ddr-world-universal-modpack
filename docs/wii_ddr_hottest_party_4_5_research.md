# DDR HOTTEST PARTY 4 / 5 (Wii, EU) Assets — RE Notes (2026-10-04)

**Question.** How do the dancers and stages of DanceDanceRevolution HOTTEST PARTY 4 (Wii, EU 2010,
`SDYPA4`) and HOTTEST PARTY 5 (Wii, EU 2011, `SURPA4`) differ from FuruFuru Party / MUSIC FIT
(HP2 / HP3, `docs/wii_ddr_hottest_party_2_3_research.md`), how much of them is already shipped, and
how do they port to DDR World?

**Answer.**
- **Same `zan` engine, same formats, same 37-bone rig.** Four differences matter: costume files are
  `CHR<id><variant:02>.bin`; stages nest their members in named archives
  (`STG<nnn>_MDL.bin` / `_CAM.bin` / `_EFF.bin` / `_Prm.bin`); cameras carry other bytes at +0x34;
  and a material's texture-count word packs a purpose field (§1). `zan_dump.survey` parses every
  archive on both discs with 0 problems.
- **Neither cast contains the other, and neither is part of HOTTSTPARTY 1-3** (§2), so each game is
  its own source: `dancers/HOTTSTPARTY 4/` (32 dancers) and `dancers/HOTTSTPARTY 5/` (18).
- **Stages: `stages/HOTTEST PARTY 4/` (26) and `stages/HOTTEST PARTY 5/` (15)**, chosen by
  `zan_dump.plan_stage_ports`. That skips what HP2 / HP3 / HP4 already ship, the in-disc
  duplicates and the COL-only `STG4xx` (§3).
- **Movie screens are colour-group-92 materials** (§3.2), and the port maps them to `offscreen1`.
- **HP5 stages have almost no cameras of their own.** HP5 choreographs its cameras per song, so
  stages with fewer than three moving shots get GAME_DEF_CAM's new 6 s front shots as mains (§3.3).
- Not yet cabinet-tested.

Tools: `scripts/zan_dump.py`, `scripts/extract_wii_ddr_data.py` (`disc`, `extract` — skips the
discs' `.svn/` copies), `tools/blender_ddr_addon/examples/port_character_hottest2.py` and
`port_stage_hottest2.py` with `GAME=hp4|hp5`, `scripts/test_zan_formats.py` (`TestHottestParty45`),
`scripts/validate_wii_ddr_tools.sh <disc dir>`.

## 1. Disc and format differences

| | HP2 / HP3 | HP4 / HP5 |
|---|---|---|
| costume | `sound/stream/character/CHR<nn:02><k>.bin` | `CHR<id><k:02>.bin` (HP4 61–75, 101; HP5 201–209; Mii bodies 81–90) |
| choreography | `motion/MOT010_SSQ<song>.bin` | `dance/DANCE_<song>[L]_MOT_010.bin` (HP4) / `DANCE_HP<n><U\|J>_<song>_<E\|L\|S><nn>_MOT_010.bin` (HP5) |
| stage | `/#0` members + `/#1` cameras | `/STG<nnn>_MDL.bin`, `/STG<nnn>_CAM.bin` (some still `/#0`, `/#1`); member names need not match the file number (HP5 STG011 holds `DRAW_STG101_01`) |
| camera +0x34 | `28f81200 7cf71200 5cf71200` | `5a005b00 5c005d00 5e005f00` (`zan_dump.CAM_SIGNATURES`) |
| skin tone | colour group 2 + main.dol table | no group-2 material: the skin is baked into the texture (no table in either main.dol) |
| material +0x14 | `u32 ntex` | `{u16 purpose (1 eye, 2 mouth, 0x400 some stage layers), u8 flag, u8 count}`: `0x00010107` is a 7-frame eye layer. Read as a u16 count, it made the eye / mouth flip-book layers look empty, and faces were blank. `_material` takes the low byte when the low u16 is ≥ 0x100. On HP2 / HP3 this only affects 3 MUSIC FIT `gcm_clear` layers, which are not ported. |

Both discs ship their Subversion working copies: every directory has a `.svn/`. `extract` skips
dot-directories.

## 2. The dancers

**HP4** (`port_character_hottest2.CAST_HP4`). The leads have one new outfit in two colours.
The texture codes keep MUSIC FIT's per-person numbering (`HP4A_01` Rena, `_03` U.G., `_04` Root,
`_05` Chordia, `_06` Harmony, `_09` Dyna, `_10` Bridget, `_11` Ceja; `HP4C_01/02/03` NAOKI / U1 /
jun; `HP4B_03` Bossa / Nova, `_04` Hip / Hop, two people per file as in HP3). The select-screen
plates and portraits (`select/select_bin_us.bin` /#2/#1, #3) agree. So does the fan wiki gallery
(videogames-fanon.fandom.com "Hottest Party Dance/Gallery"), whose "Hottest Party 4 outfit"
pictures of Rena, U.G., Root, Chordia and Harmony match CHR61 / 64 / 65 / 66 / 67. CHR101 is U1's
outfit with other legs (`HP4C_04_leg`). CHR72 / 73 (`HP3B_01/02`) are MUSIC FIT's CHR49 / 50 byte
for byte (Pia / Forte), already shipped, and not re-ported. Domi, Gaku, Danca, Gliss and Sharp
have no HP4 costume.

**HP5** (`CAST_HP5`): five new characters, matched to plates through the portraits. DISCO is the
afro (CHR202), EMI the hooded blue hair (201), RUBY the tanned one (203, texture `HP5_03_ruby_*`),
YUNI the glasses and pigtails (204), RAGE the fedora (205). A new Rena (206) joins them, along
with HP4's NAOKI / jun / U1 outfits re-exported with retouched heads (`HP5C_01/03/02` = CHR207 /
208 / 209, bodies identical to HP4's CHR69 / 71 / 70; shipped too, maintainer decision). HP5 ships
only HP4's accessory files, so jun's fan is `AccRHand_HP4C_03`.

Keys `hp4<person><nn>` / `hp5<person><nn>`. 32 + 18 dancers, labels ≤ 15 bytes, keys unique
across all sources.

**Choreography.**
- **HP4: 77 song files.** Every `NNNL` file is an alternate dance for song NNN, with pieces that
  differ from NNN's. The tempo comes from `ssq/ss4/MU_DDR_<song>.ss4`. Only 275 of HP4's 3372
  pieces are in HP2 / HP3. The library is 433 clips, 3420 bars.
- **HP5: 253 files** of HP2–HP5 songs (tagged by origin game, numbered by that game). Nearly every
  piece is an HP2 / HP3 / HP4 one again. There is no per-file SSQ, so the tempo comes from the
  one-bar pieces' length. The library is 393 clips, 2922 bars. Each source deals its own disc's
  library (12 clips per dancer).

## 3. The stages

### 3.1 What ships (`zan_dump.plan_stage_ports`, `port_stage_hottest2.planned_stages`)

A stage is skipped when:
- it draws nothing;
- its `stage_signature` (node transforms, geometry, UVs, pictures and motion of every model, the
  COL layout included, names ignored) equals a shipped or already-covered stage;
- it is a same-named HP2 re-export (the MUSIC FIT rule);
- or it duplicates a lower-numbered stage of its own disc.

| HP4 skips | reason |
|---|---|
| STG041–055 | FuruFuru Party's (via MUSIC FIT's re-exports): `HOTTEST PARTY 2` |
| STG107–111, 202–206 | MUSIC FIT's: `HOTTEST PARTY 3` |
| STG000, 401, 407–430 | no models (bare COL layouts) |
| `STG042_P`, `STG046_P` | HP2-layout copies (not `STG<nnn>.bin`) |

HP4's STG042 / 043 / 044 are new stages that reuse old numbers. STG042, for example, keeps three
of HP2 STG011's sixteen props on a new set, plus new prop models.

| HP5 skips | reason |
|---|---|
| STG201, 301, 402–406, 431, 432 | HP4's (identical) |
| STG202–206 | MUSIC FIT's |
| STG011 = 002, 017 / 024–027 = 013, 021 = 018, 022 = 019, 023 = 020 | in-disc duplicates |
| STG000, 401, 407–434 (but 426, 431, 432) | no models |

HP4 STG201 is STG301 at half the texture resolution (128² vs 256²), with its camera rig nudged. It
is the same set on screen, so it is dropped by hand (`NEAR_DUPLICATES`) and 301 ships.

Result: HP4 ships STG001–008, 042–044, 101–106, 200, 301, 402–406, 431, 432 (26); HP5 ships
STG001–003, 012–016, 018–020, 028–030, 426 (15). The planner reads all four discs. When the older
dumps are missing, the port falls back to `RECORDED_PLAN`, which was checked equal to the live plan
on 2026-10-04.

**Unplaced props.** HP4 / HP5 leave unused props in the archives: they have no OBJSET node and
were modelled at the origin. Examples: 14 of STG042's, STG102's spare `filter02`, STG103's
`monitor01`. They are not drawn. FuruFuru Party / MUSIC FIT's unplaced props are still drawn
where they were modelled.

### 3.2 Movie screens = colour group 92

A material's +0x28 high u16 (the colour group, research 2/3 §2) is **92** on every surface the
game plays a movie on. Surveyed on all four discs, it is on nothing else. The surfaces carry a
white or tiny placeholder texture (`monitor01.tga`, `MOV_cap.tga`, `movie_test.tga`). Examples:
- HP5's `OBJB_Z_pv01_43 / pv02_CE / pv03_WI` PV monitors (the suffixes match the `_43` / `_CE` /
  `_WI` strings in main.dol);
- STG016's `movieBox` wall of 100 boxes, each with its own window of the picture;
- STG015's planets;
- HP4's STG044 dome, STG001 / 405 / 431 monitors;
- MUSIC FIT's TV sets.

FuruFuru Party's screens are the `_MOV` props' `root` quads instead, in group 0. Groups 91 and
93–103 are the frames, bezels and caps around screens.

The port textures group-92 meshes `offscreen1` and maps v onto the 16:9 band as authored (0..1 =
the whole picture). It keeps their blend: STG044's dome is alpha-blended at a third of its
strength. Additive screens (STG431) get the vertex alpha folded into the colour. This applies to
`GAME=hp4|hp5` only. The shipped HP3 port predates the rule, so re-porting `GAME=hp3` would give
MUSIC FIT's TVs screens too (open).

### 3.2b Stage videos = colour group 91, and the stage parameters

Group **91** is where the game plays the stage's *own* video (`movie/stage/*.thp`: abstract VJ
loops, not the song's PV). The surfaces are white `monitor*.tga` / `S3white.tga` placeholder cards
on HP4 STG003 / 044 / 103 / 402–406, HP5 STG002 / 003 / 012–018 / 426 and MUSIC FIT STG104. HP4 /
HP5's `STG<nnn>_Prm.bin` /#0 (a `ZAR` record; `zan_dump.stage_params`) selects the video:

| +0xC0 type | meaning | +0xE0 name |
|---|---|---|
| 0 | plain 3D stage | — |
| 1 | full-screen background video only (the COL-only STG4xx) | `bgv<nn>` |
| 2 | stage video on the group-91 surfaces | `quarter` — `quarter01.thp` etc. are 2×2 mosaics, each surface's UVs pick a quadrant |
| 3 | likewise, one named video | `single02` … `single07` |
| 4 | the song's PV on the group-92 monitors | — (HP5 STG028–030) |

MUSIC FIT and the plain HP4 stages leave the pick to the song (`ani / fvo / mvo / pop / upt`
01–04). The port decodes 16 frames (ffmpeg, 240 px, one every 0.75 s) of the named video, or of
`upt01` when the stage names none. It turns them into a flip-book on the group-91 material, which
the atlas machinery animates. Without this, these stages showed blank white cards: HP5 STG013 /
018 are a single movie room, and STG426 is an empty box.

### 3.2c Bugs fixed on 2026-10-04 (all zan ports, and HP1)

- **Additive vertex alpha.** Every HP1–HP5 stage port forced additive meshes' vertex alpha to 1.
  Both engines blend additive as SRCALPHA + ONE (zan `FUN_8010def4` and Hudson's HSF draw
  `FUN_8006a3a8`, `GXSetBlendMode(1, 4, 1, 5)`), and so does World's flags2 = 4. So a 0.25-alpha
  floor glow became a solid white disc or square. Additive submeshes with partial alpha: HP1 190 in
  16 stages, HP2 172, HP3 141, HP4 229, HP5 158. Re-ported; on HP1 STG01 the re-port changed only
  those alpha bytes.
- **Long flip-books.** `_material` dropped texture lists of 64 or more entries. MUSIC FIT STG102
  (253), HP4 STG002 / 043 / 102 (78–97) and HP5 STG426 (77) lost their flip-books, and four STG426
  materials drew untextured. The cap is now `MAX_FLIP` = 1024.
- MUSIC FIT's group-92 TV sets are screens now too (§3.2).

### 3.3 Cameras

HP4 stages mostly carry 6 shots. HP5 stages carry one static 3 s view, or none. HP4's STG4xx have
no camera archive at all. World keeps a fixed camera when a stage has no `_st` clip, so
`main_cameras` keeps the own shots that move and, below three, adds GAME_DEF_CAM `/#0/#78..#85`.
Those are HP4's new 6-second front shots (dolly, pan and crane moves on the dancers, 2.4–4.1 m
out). GAME_DEF_CAM (86 shots in both games) also supplies the `_non` close-ups.

## 4. Not done / open

- Cabinet test: screens (HP5 STG016's box wall, STG028–030's PV monitors, HP4 STG044's dome),
  flip-books, the fallback cameras.
- Sparse by design: HP4 STG008 (a sky dome and a floor) and STG200 (a floor and a signal ring,
  no `_Prm`) are minimal on the disc too. Nothing is missing.
- Stage videos are a 16-frame, 1.3 fps flip-book of the real THP. A song-chosen video (MUSIC FIT,
  HP4) is fixed to `upt01`.
- The per-stage colour-group tints (groups 11–43, FuruFuru Party `FUN_80036d20`) are not ported.
- Stage display names. Both text banks (`text/text_eng.bin` /#2) list 34 names (Street Show, The
  Vision, Silent Dome, …), but no file → name table has been found yet, so the labels are
  `Stage NN`.
- The per-song choreographed cameras (`dance/DANCE_*_FRE.bin` / `_CHO.bin`) are not ported.
- Mii bodies (CHR81–90) are not ported. Neither are HP5 Root / Harmony / the back-ups, which have
  portraits but no costume file.
- MUSIC FIT's group-92 screens (§3.2).

## 5. Reproduce

```bash
W=~/Desktop/"DDR Wii ISOs"
scripts/extract_wii_ddr_data.py disc "$W/Hottest Party 4.wbfs" "$W/Hottest Party 4 (Europe)"   # likewise HP5
./scripts/validate_wii_ddr_tools.sh "$W/Hottest Party 4 (Europe)" "$W/Hottest Party 5 (Europe)"
B=/Applications/Blender.app/Contents/MacOS/Blender; E=tools/blender_ddr_addon/examples
GAME=hp4 DANCERS=all $B -b --factory-startup --python $E/port_character_hottest2.py   # ~6 min, 32 dancers
GAME=hp5 DANCERS=all $B -b --factory-startup --python $E/port_character_hottest2.py   # 18 dancers
GAME=hp4 STAGES=all  $B -b --factory-startup --python $E/port_stage_hottest2.py       # 27 stages (reads HP2 / HP3 dumps too)
GAME=hp5 STAGES=all  $B -b --factory-startup --python $E/port_stage_hottest2.py       # 15 stages
```
