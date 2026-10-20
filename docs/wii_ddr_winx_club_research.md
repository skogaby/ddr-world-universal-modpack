# DanceDanceRevolution Winx Club (Wii, EU) Assets — RE Notes (2026-10-20)

**Question.** How are the dancers, stages and the mid-song Enchantix transformation of
DanceDanceRevolution Winx Club (Wii, EU 2009, `RW6PA4`) stored and run, and how do they port to
DDR World's Background Dancers?

**Answer.**
- **The zan engine of FuruFuru Party / MUSIC FIT** (`docs/wii_ddr_hottest_party_2_3_research.md`):
  same archives, ZMB / ZAB / camera formats and 37-bone rig, in HP2 / HP3's file layout, with
  the costumes under `character/` instead of `sound/stream/character/`. `zan_dump.survey` parses
  every archive with 0 problems once the third camera signature (§4) is known.
- **24 dancers** (`dancers/WINX CLUB/`): six fairies × {default outfit, Denim, Dance, Enchantix}.
  The Enchantix ("Fairy") form is only ever reached by transforming in game (§3).
- **6 stages** (`stages/WINX CLUB/`): STG001–006. STG007–055 / 101–103 are FuruFuru Party's
  stages again (already shipped as `HOTTEST PARTY 2`) and are not re-ported.
- **The transformation** fires when a player's Dance Gauge is full before 80 % of the song: a
  0.5 s fade, then a 2 s wipe from the girl's outfit to her Enchantix twin (both play the same
  transformation clip), with its own effect and camera, until the next choreography piece (§3).
- Not yet cabinet-tested. The transformation is ported as data only (clips + twin markers); the
  DLL side is open (§6).

Tools: `scripts/extract_wii_ddr_data.py disc`, `scripts/zan_dump.py`,
`tools/blender_ddr_addon/examples/port_character_hottest2.py` / `port_stage_hottest2.py` with
`GAME=winx`, `scripts/test_zan_formats.py` (`TestWinxClub`), `scripts/validate_wii_ddr_tools.sh
<disc dir>`. Ghidra: `ddr_winx_club_main.dol` (SDA bases r2 = 0x80297e20, r13 = 0x80296740).
Addresses below are main.dol virtual addresses.

## 1. Disc

| path | content |
|---|---|
| `character/CHR<nn:02><k>.bin` | `k = 0`: {body ZMB, head ZMB}; `k = 1`: {body TPL, head TPL} (one colour variant) |
| `accessory/AccSpine1_wing0<n>.bin` | the Enchantix wings {ZMB, TPL}; also `AccL/RHand_wii_con`, `mii_head_acc*` |
| `motion/MOT010_SSQ<nnn>.bin` | per-song choreography: one-bar ZAB pieces (001–055) |
| `ssq/MU_WIN_<nnn>.ss9` | tempo for 001–033 (the rest take the pieces' own length) |
| `game/APLMOT/APL_CHARA0<n>.bin` | per girl: `/#0/#0` transformation (120 f), `/#0/#1` pose (40 f), `/#1` DBBOX / `/#2` hair tracks |
| `game/HAIRMOT/HAIR_MOT<nn>.bin`, `game/COLMOT/COL_MOT<nn>.bin` | per-song dynamic-bone drivers (rot_DB*) and DBBOX collider tracks |
| `game/EVENT_EFFECT.bin` | `chenge_effect.TEB` + TPL: the transformation effect (ribbon + sparkles + glow, 2 s loop) |
| `game/CHARA_EFFECT.bin`, `GAME_APL_EFF.bin`, `GAME_STG_EFF.bin` | `chara_effects`, `concent`, `ddr2_effect` TEBs |
| `game/GAME_DEF_CAM.bin` | `/#0` 39 generic shots, `/#1` 51, `/#2` 17, `/#3/#0` the transformation shot (92 f) |
| `stage/STG<nnn>.bin` | `/#0` models (DRAW / BG / OBJ / COL) + `/#1` cameras, as HP2 |
| `text/_ENG/SYS_TEXT.bin` | bank #1: Bloom, Stella, Musa, Flora, Tecna, Layla |
| `select/_ENG/stage_select.bin` | the stage-select thumbnails + name plates |

The disc ships CVS/ working copies (the extractor's dot-dir skip does not apply; they are tiny).

## 2. The dancers

Character ids (`FUN_8001d760`, base table `DAT_801c845c` = {0, 10, 20, 30, 50}):

| ids | file | outfit | label |
|---|---|---|---|
| 10–15 | CHR1n (`wa_0n`) | default | `Bloom` |
| 20–25 | CHR2n (`wc_0n`) | denim (all six) | `Bloom Denim` |
| 30–35 | CHR3n (`wd_0n`) | dance | `Bloom Dance` |
| 0–5 | CHR0n (`wb_0n`) + `AccSpine1_wing0n` | Enchantix | `Bloom Fairy` |
| 50–53 | CHR5n | Mii bodies | not ported |

Person n = 1..6 in SYS_TEXT order. Hair colours on the head textures and `wd_02_stella_body`
agree. The random pick (`FUN_80033a88`) never draws ids 0–5. The Enchantix ids are drawn 0.8 zan
units higher (`FUN_8003149c`, ≈ 9 cm: they float), and the port lifts them by the same amount.

Format points:
- No colour-group-2 skin material: the skin is in the texture (as HP4 / HP5).
- **Heads are skinned** to their own dynamic-bone chains (`bone_head`, `DB_hair01..04`,
  `DB_side`, `DB_L/Rside*`), and so are the wings (`wing_root`, `DB_L/Rwing_01/02`) and some
  ribbons (Layla Denim's `DB_himo*`, kept as static rig joints). The game simulates them with
  per-song driver tracks (HAIR_MOT) and DBBOX colliders (COL_MOT). The port bakes them at rest,
  rigid on the attach joint (`zan_dump._fold_weights`): hair and wings do not sway.
- `DBBOX_*` nodes are collision boxes on `dummy.tga` (they showed as a white box): not drawn.
- The Enchantix `ZDRAW_kira_*` / `*_kirakira` sparkle overlays and the wing glow are additive:
  not drawn (World's dancer shaders have no additive mode).
- CHR36 (Layla Denim) reuses `wb_06_ref03.tga`, so the wings are matched on `wb_0n_body`.

Choreography: 55 MOT files → 363 clips / 2653 bars, 12 per dancer. MOT050–052 (360–440-frame
showcase pieces) are skipped, and MOT049 has one flight piece (3 bars; no take-off, so no flight).

## 3. The Enchantix transformation (main.dol)

Each player owns **two dancer slots**: i (her outfit) and i + 2 (the Enchantix twin of the same
girl), both fed the same choreography (`FUN_800a76e4(seq, i, i, i + 2, ..)` in `FUN_8002eb34`).
The sequencer is the object at game + 0x2dc, ticked per frame by `FUN_800a6d3c`. Its timer is in
seconds (`1 / (fps / (skip + 1))` per frame) and its constants are in SDA2.

**Trigger** (`FUN_8003149c` → `FUN_800a7650`, per player, at most 2): the player is active,
player + 0xa0970 == 1000 (the **Dance Gauge**: clamped to 0..1000 in `FUN_800284a8`; gains in
`FUN_80028130` grow with the combo at + 0xa09e4), the song position `FUN_8001caec()` ≤ 0.8 × the
song length (`+0x400770 >> 12`), and the player has not transformed yet. In practice: a full
gauge, reached by holding a combo.

| state | duration | what happens |
|---|---|---|
| 0 | — | wait for a triggered player whose state is 0 |
| 1 | 0.5 s | scene colour 1 → 0 (seq + 0x2c, copied to the stage + 0x2b48 and the 0x2cc object + 0x6c) |
| 2 | until APL 0 ends (2 s) | APL clip 0 on both twins (+ APL hair / col tracks); effect `chenge_effect` #0 spawned (category 4 = EVENT_EFFECT bank, `FUN_800fafac(.., 0, 4, 0x23)`); camera cut (blend 0) to the event list = GAME_DEF_CAM `/#3/#0` (`FUN_8003aedc` member 3 → cam + 0x274, `FUN_8003d1d8`); scene colour 0 → 1 over 0.5 s. **Wipe**: a world point lerps (0, −10, 0) → (0, 30, 0) zan units over seq + 8 = 2.0 s (set in `FUN_8002eb34`), projected to screen y; `FUN_800a73e0` draws the outfit twin in a GX viewport ABOVE the line and the Enchantix twin BELOW it, so the fairy is revealed from the feet up; the effect rides the line (+0.2). The normal dancers are not drawn. A per-slot mode switches 5 → 6 at 0.6 × 2 s |
| 3 | until the next choreography piece | APL clip 1 (the 40-frame pose) on both twins; the effect fades out over 0.5 s (`FUN_800fb13c`) |
| end | | `FUN_800a7678` (called from `FUN_800307f0` when the dance sequencer starts the next MOT piece): player state 4 (transformed, never re-triggers), event camera released (`FUN_8003d224`), slot visibility handed to the twin (`FUN_800406d8` pair link), sequencer state 4 (0.5 s), then 0 (the other player may go) |

The dance then continues on the Enchantix twin for the rest of the song.

## 4. Cameras

Stage cameras carry HP2's +0x34 signature. GAME_DEF_CAM `/#1` `/#2` `/#3` carry a third one,
`28f81300 7cf71300 5cf71300` (`zan_dump.CAM_SIGNATURES[2]`). `FUN_8003aedc` loads GAME_DEF_CAM
members 0..2 as the generic shot lists (the port's `_non` close-ups come from `/#0`) and member 3
as the event (transformation) list.

## 5. The stages

STG001–006 are Winx Club's own; labels come from the stage-select plates, matched through the
thumbnails:

| file | label | notes |
|---|---|---|
| STG001 | Alfea | the school courtyard, static |
| STG002 | Entrance Hall | the stained-glass lobby, static |
| STG003 | Redfountain | the show stage: 27 props, 93 UV-animated materials, 800-frame loop |
| STG004 | School Library | 10 props, 2400-frame loop |
| STG005 | Forest Village | no plate (a story-mode set); 5 flip-books |
| STG006 | City of Magix | the plaza, a fountain flip-book |

None has a `_Prm`, a group-91 / 92 screen or a `_MOV` prop. Stage previews checked in Workbench:
all six render as on the select-screen thumbnails.

STG007–055 / 101–103 are FuruFuru Party's: every single-texture material is byte-identical to the
shipped `HOTTEST PARTY 2/Stage NN`, except STG009's backdrop tile (a grey hexagon) and STG035's
EU "HOTTEST PARTY" graffiti. STG000 is empty. Not ported (maintainer, 2026-10-20).

## 6. Porting the transformation to World (open)

Shipped as data:
- every dancer: `pl_<key>/motion/transform/transform.anm` (APL 0, 120 f) + `pose.anm` (APL 1,
  40 f), 1:1 at 60 Hz, not re-centred;
- every outfit: `pl_<key>/transform.txt`, whose first non-comment line is its Enchantix key.
  The Background Dancers playlist reads only `motion/*.anm`, so the clips never play as dances.

Still needed (DLL): trigger from the side's life gauge or combo
(`song_reset::judge_diag(side).combo`; a gauge read would need a new accessor next to
`classify_gauge`), the twin loaded beside the outfit, the switch (World has no per-dancer
scissor: the wipe needs another shape, e.g. a swap under the effect at the wipe's midpoint), the
chenge_effect TEB through `flight_fx`'s CzanEff runtime, and the `/#3/#0` camera.

## 7. Reproduce

```bash
D=~/Desktop/ddr_wii
scripts/extract_wii_ddr_data.py disc "$D/Dance Dance Revolution - Winx Club (Europe) (En,Fr,De,Es,It).wbfs" "$D/Winx Club (Europe)"
./scripts/validate_wii_ddr_tools.sh "$D/Winx Club (Europe)"
B=/Applications/Blender.app/Contents/MacOS/Blender; E=tools/blender_ddr_addon/examples
GAME=winx DANCERS=all $B -b --factory-startup --python $E/port_character_hottest2.py   # ~5 min, 24 dancers
GAME=winx STAGES=all  $B -b --factory-startup --python $E/port_stage_hottest2.py       # ~1.5 min, 6 stages
```
