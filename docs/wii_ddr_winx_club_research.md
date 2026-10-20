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
- **Ported to Background Dancers (2026-10-20, not yet cabinet-tested; §6):** the transformation
  (twin loaded beside the outfit, a full combo of any kind held to 80 % of the song, the fade / transform / pose
  sequence, the event camera, `chenge_effect`, the Wii's screen-space wipe through `_wipe` shader
  variants), the dynamic bones (the Wii's own solver, §7.1), the additive `kira` overlays with
  their UV scroll (§7.2) and a wing-tip sparkle trail (§7.4 — an assumption: the Wii never plays
  that bank).

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
  ribbons (Layla Denim's `DB_himo*`). The game simulates them (§7.1) with per-song root drivers
  (HAIR_MOT) and DBBOX colliders (COL_MOT). The port keeps those skeletons in the rig (the head's
  under `mii_head`, the wings' under `AccSpine1`, `port_character_hottest2.winx_extras`; ≤ 59 bones)
  and the DLL solves them (§7.1 "Port"); `zan_dump._fold_weights` still folds any OTHER foreign
  joint.
- `DBBOX_*` nodes are collision boxes on `dummy.tga` (they showed as a white box): not drawn; their
  boxes go into `dynbones.txt`.
- The `ZDRAW_kira_*` / `*_kirakira` sparkle overlays and the wings' third submesh are additive:
  kept as additive slots (mesh flags 0x06C1 / flags2 4) with their UV scroll (§7.2).
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

## 6. Porting the transformation to World (implemented 2026-10-20)

Content (`port_character_hottest2.py` / `port_winx_fx.py`, `GAME=winx`):
- every dancer: `pl_<key>/motion/transform/transform.anm` (APL 0, 120 f) + `pose.anm` (APL 1,
  40 f), 1:1 at 60 Hz, not re-centred (a subdirectory of `motion/`: never a dance clip);
- every outfit: the marker `pl_<key>/transform/<twin key>.txt` (the key in the member NAME — the
  DLL's content planner sees member names only; the text repeats it and adds `lift <m>` =
  ENCHANTIX_LIFT, the twin's float) and the event camera `pl_<key>/camera/transform.camanm`
  (GAME_DEF_CAM `/#3/#0`, 92 f, the stage ports' camera math);
- every Fairy: `pl_<key>/winx_fx/chenge.{teb,txt}` + pools `fx_<key>_c*` (the transformation
  effect, sized at its simulated peak: 124 quads + 32 ribbon strips in 4 pool models) and
  `winx_fx/wing.{teb,txt}` + `fx_<key>_waa` (70 quads; §7.4).

DLL (`src/mods/background_dancers/`, `transform_logic.rs` = the pure sequence):

| Wii | World port |
|---|---|
| two dancer slots per player | `Pick::twins` (`pick::transform_pick`: gameplay, human sides, an outfit whose twin is installed; never on a flight stage) → the Fairy's arc loaded beside the outfit, `InstanceKind::Twin`, hidden until the transformation |
| full Dance Gauge before 0.8 × the song | (maintainer 2026-10-20, difficulty-independent) the side still holds a FULL COMBO of any kind — Marvelous / Perfect / Great / Good; no Boo, Miss or N.G. in `song_reset::judge_diag(side).grades` — when the music count reaches 0.8 × the chart's song-over (`chart_end_raw`); polled per frame from 80 % on; once per side per play; one at a time (a second side waits for the first switch, and must still hold its full combo then); never the Multiplayer Bot's phantom side; a rewind before the trigger re-arms |
| state 1, colour 1 → 0 (0.5 s) | the stage parts' and dancers' tint rgb, real clock |
| state 2: APL 0 on both twins, colour 0 → 1, event camera, `chenge_effect` #0, the wipe | the same on the real clock; camera slot 0 from `transform.camanm` moved by the dancer's x / z placement (an assumption — the Wii's shot is world-fixed); the effect = `chara_fx::JointFx` at the wipe line + 0.2 units; the wipe (below) |
| state 3: APL 1 until the next choreography piece | `pose.anm` until the next BAR line of the clip playing underneath (a Wii MOT piece is one bar = 2 s of the port's dance time); the effect fades out over 0.5 s |
| end: pair link, camera released | the twin plays the OUTFIT's playlist at the schedule position (its clips retargeted onto the twin's rig by bone name, `transform_logic::retarget_map`, lifted by `lift`); the outfit hides for the rest of the song; the stage cameras resume |
| normal dancers hidden in states 2–3 | every other dancer (and its shadow) hidden in those phases |

Not ported: the per-slot mode 5 → 6 at 0.6 × 2 s (`FUN_8003fb58`, a dancer draw mode), the
transformation sound, the APL hair / col tracks (the solver runs on the body's motion instead),
per-song cameras (out of scope everywhere).

## 7. Dynamic bones, sparkle overlays and the wipe (RE 2026-10-20, round 2)

Decompiles of every function below: `.agents/scratchpad/2026-10-20-winx-club-port/dynbones_decomp.c`.

### 7.1 Dynamic bones (hair, side locks, ribbons, wings)

**Which nodes.** zan classifies nodes by name prefix (table `801e3da8`, 22 entries; the type
lands in node + 0x2a = index + 1): `DB_` = 0x15 (a dynamic bone), `DBBOX_` = 0x16 (a collider;
`FUN_800d5020` lists them per model at + 0x1d4 / + 0x1d8, and node + 0x2b = the name's number,
`DBBOX_12` → 12). A dynamic node has ZMB node flags 0x0100 (the byte at + 0x98, which
`FUN_800d5114` tests), no submesh, and its "mesh" offset (+ 0x9c) points to a **0x70-byte config**.
The chain roots (`rot_DBbone`, `rot_DB_stella_L/R`, `rot_DB_Layla_L/R`, `rot_DB_tecna`,
`rot_DBbone_wa03_L/R`, `rot_DBbone_wb03_L/R`) are plain nodes that `HAIR_MOT<song>` animates per
choreography cue (`FUN_8003f85c`, motion index from the song's cue table); `COL_MOT` / APL `/#1`
animate the DBBOX colliders.

| chain | nodes (parent) | gravity | angle limits (rad) |
|---|---|---|---|
| hair | DB_hair01..04 (rot_DBbone) | yes | z ±0.70..0.87, y −0.26..1.48 |
| side | DB_side (bone_head), DB_L/Rside01..04 | yes | z −1.40..0.87, y −0.79..0.96 |
| ribbon | DB_himo01..03 (Hips, Layla Denim) | yes | ±0.52..0.87 |
| wings | DB_L/Rwing_01..02 (wing_root) | **no** | z −0.35..1.22 / ±0.52, y off |

**Config (0x70, big-endian).**

| off | type | meaning |
|---|---|---|
| 0x00 | u32 | 1 = gravity (−3 × 0.1 per step on the lag vector's y) |
| 0x04 | f32 | 3.0 / 8.0 (not read by the step; open) |
| 0x08 | u8 n, u8 ids[n] | DBBOX ids this bone does NOT collide with (side hair skips its own head box 01) |
| 0x10 | axis block | x (twist): enable, limit-enable, …; min 0x14, max 0x18, stiffness 0x20, damping 0x24. Not read by the step |
| 0x30 | axis block | **z**: +0 enable, +1 limit-enable, +2 damping-enable; min 0x34, max 0x38 (applied negated), stiffness 0x40, damping 0x44 |
| 0x50 | axis block | **y**: same layout; min 0x54, max 0x58, stiffness 0x60, damping 0x64 |

**Runtime.** `FUN_800d4ea8` builds per node a 0x24-byte param record {node world, parent world,
model mtx (+0x1e0), the node's bone length (+0x90), dt (written each step), collider model, mtx
+0x210, DBBOX count, DBBOX list} and a 0xb4-byte state (`FUN_8010ec9c`: direction vec3 +0, a second
vec3 +0xc, counters +0x18 / +0x1c, previous world +0x20, first-frame flag +0x50, rest local +0x54,
a pending teleport offset +0x84 that `FUN_8010ed1c` sets, so a jump does not fling the hair). Each
frame `FUN_800d52c0` updates the model worlds in node order and calls `FUN_800d5114` for every
dynamic node, i.e. root first, so each bone sees its parent's solved world. The step is
`FUN_8010d430(out, state, params, config)`, one bone at a time:
1. world = parent world × local; the **lag** = previous position − current position (plus the
   teleport offset; zero on the first frame); gravity is added to its y when the flag is set;
   `speed` = |lag| / 0.1; the lag is normalised.
2. Target angles (yaw, pitch; twist stays 0) are those that point the bone along `tip + lag`, in
   the rest frame (`FUN_8010e30c`); clamped to the axis limits (`FUN_8010e224`); turned back into a
   direction (`FUN_8010e494`) and a bone matrix that aims along it (`FUN_8010d864`).
3. Collision (`FUN_8010da98` → `FUN_8010dcd4` / `FUN_8010e8fc` / `FUN_8010e67c`, boxes via
   `FUN_800d82c0`): the segment is tested against the model's DBBOX hulls except the excluded
   ids, and pushed out along the hit face; a hit scales `speed` by 1.5 × depth.
4. Spring: the current angles move toward the target by `k = 0.06 · (dt·60) · speed / 20`
   (≤ 0.99), or `(dt·60) · (1 − damping)` when the axis's damping flag is set (`FUN_8010e5c8`);
   then decay toward rest by `1.1 · stiffness · |angle|` per axis (`FUN_8010e138`); clamped again.
   After a reset, five frames blend the state toward + 0xc at 0.3 · dt·60 instead.
5. The state direction gives the output matrix; the world is stored as "previous".

So it is an angle-space lag spring per bone, not a Verlet chain. It is cheap (two angles per bone),
deterministic and dt-scaled, and ports to Rust as a pure, harness-testable module.

**Port (implemented 2026-10-20).** The exported rig keeps the head's own skeleton (`bone_head` /
`rot_DB*` / `DB_*` under `mii_head`), the wings' (`wing_root`, `DB_*wing*` under `AccSpine1`) and
the body's `DB_*`, skinned; `pl_<key>/dynbones.txt` (`zan_dump.format_dynbones`) carries per DB_
node its parent, length (m), gravity flag, z / y blocks, excluded DBBOX ids (+ the rest local,
informational) and the body's DBBOX boxes as joint-local AABBs (m). The reference solver is
`zan_dump.dynbone_step` / `dynbone_world` (tests `test_zan_formats.TestWinxClub`), the DLL's
`dynbones_logic.rs` (cross-checked against it on a shared scenario, ≤ 2e-3 m), solved in
`director.rs` after `evaluate_into` and before Big Head / the publish, at fixed 60 Hz steps (the
Wii's tick; ≤ 4 catch-up steps, dt clamped at 0.1 s) on the real clock, model space (the lag in
zan units). Reset on the song start and on a rewind; a clip cut (a new schedule segment) or a
change between dance / transform / pose clips is a teleport (no lag that frame). The HAIR_MOT
root drivers are per-song cues of Winx songs: the roots stay at rest and the motion comes from the
body's own movement.

**Exact vs approximated.** Exact (from the decompile): the config fields; the constants (0.1,
3.0, 1/60, 0.06, 20, 0.99, 1.1, 1.5); the lag + gravity, `speed = |lag| / 0.1`; the angle
convention (bone axis local +x, yaw = atan2(−z, x) about y, pitch = atan2(y, |xz|) about z, in
the rest frame; the direction back through RotY · RotZ); the clamps (yaw by the y block, pitch by
the NEGATED z block, a disabled axis = 0); the gains incl. the decompile's CROSSING (the y block's
damping flag / value drives the pitch gain, the z block's the yaw gain) and the decay (skipped on a
collision hit); the state kept as a world aim direction. Approximated: the collision (the bone's
TIP against each non-excluded box, pushed to the box's nearest face; `FUN_8010dcd4` /
`FUN_8010e67c`'s segment math and the 10.0 / 1000 constants are not reproduced), the teleport
rule (a root jump > 0.5 m per step, or the director's clip-cut flag, instead of the `+0x84`
offset `FUN_8010ed1c` sets), the omitted 5-frame settle blend after a reset (`state + 0x1c`),
and model space instead of world space (the dancers do not travel).

### 7.2 Sparkle overlays (`kira`)

`ZDRAW_kira_LFarm / LUarm / RFarm / RUarm / chest` (rigid on the arm / chest joints),
`wb_0n_body_kirakira` (skinned, over the whole Enchantix body) and the wings' third submesh share
one material: texture `wb_0n_ref03` (a 64² sparkle sheet), **additive**, alpha-tested, with UV
keys that scroll u by one tile per second (key 0 s → u 0, 1 s → u 1, wrapping). The Denim outfit
of Layla (CHR36) carries the same arm overlays.

Other outfits carry the same overlays too: CHR24 (`ZDRAW_kirakira_hip`, `wc_04_body_kirakira`),
CHR33 / CHR34 / CHR35 (leg / body kira).

**Port (implemented).** Every additive submesh of a Winx body stays in the body model as its own
material slot (`ddr_flags` 0x06C1 / `ddr_flags2` 4 on the material — `export_model._mesh_flags`
honours a material-level override), and the body ships a material loop
`pl_<key>/pl_<key>_play_loop.sanm` (member `data/chara/pl_<key>/pl_<key>_play_loop.sanm`): per
kira material its offU track from `zan_dump.texmtx_offset` of the UV keys (0 → −1 over 60 frames,
loop). The DLL samples it on the real clock into the body's material copies
(`director::body_material_params`, published with the pose). Blended records stay stock-shaded (no
restyle, no outline hull).

### 7.3 The wipe

The Wii's wipe is in **screen space**: a world point is projected to a screen y and two GX
viewports split the screen there (§3). World has no per-model scissor or clip plane (the 2D
viewport RE does not reach the `MODEL:*` passes), but every model pixel shader already reads
`VPOS` (the stipple dissolve, `texkill(c2.y − stipple(vPos / 32))`). **Port plan:** a `_wipe`
variant of each pixel shader the dancers use (the stock `mdl_ch_constant_vc` family and the synth
`_lit` / `_cel` ones) that adds `clip(side · (vPos.y − threshold))`, the threshold in a per-item
constant the board writes every frame (c2.w is free on non-hull items); the outfit twin draws with
side +1 and the Enchantix twin with −1, the line projected from the Wii's (0, −10 → 30, 0) zan
units over 2 s. Variants go through `shaders/src/` → `scripts/build_shaders.sh` →
`shader_synthesis.rs` (fingerprint bump). Without `shader-fixes` the fallback is a crossfade
(tint alpha) or a cut under the effect.

**Port (implemented).** c2.w was verified free on non-hull items: `item+0x4C` is written only by
`render_item::set_outline_push_scale` (hull twins) and read only by the synthesized outline VS
(`mdl_cel.hlsl`); stock shaders never read `ModelParameters.w`. VPOS alone would need the render
target's pixel height (variable under custom resolutions), so the `_wipe` VS hand the clip
position's (y, w) to the PS (TEXCOORD6, perspective-correct ⇒ the pixel's exact NDC y) and the PS
discards with `wipe_clip(wp, c2.w)` (`mdl_common.hlsli`): `w = ±(line_ndc + 4)`, + keeps above
(the outfit), − below (the twin), 0 = no wipe. Containers (`shader_layout::WIPE_VARIANTS`, the
dancers' only shader `mdl_ch_constant_vc`): `_wipe` = our UNLIT replica of the stock VS +
`mdl_uv3_wipe.ps` (the stock `tex · COLOR0` + the discard), `_lit_wipe` = the lit VS + the same
PS, `_cel_wipe` = the cel VS / PS with `WIPE`; synthesis fingerprint "v8". Both bodies of a
transforming dancer are built with EVERY material on them (opaque → the scene style's wipe,
blended kira → `_wipe`). The word travels as the frame board's new AUX word: `visit(2)` copies it
into a body item's `ModelParameters.w`; a hull twin (whose .w is its push) hides while it is set
(the outline PS has no wipe — no outline during the 2 s). The line: the dancer-relative point
(0, −10 → 30, 0) zan units × GAME_SCALE through this frame's camera (`camera_math`). Not served
(shader-fixes off / blobs missing): one WARN, the twin's tint alpha crossfades in over the 2 s.

### 7.4 Wing sparkles

`CtsCharaAtc` (`FUN_80037ccc`) looks up `DB_Lwing_02` / `DB_Rwing_02`. `game/CHARA_EFFECT.bin`
(`chara_effects.TEB`: one 2 s looping effect, 300 particles, 1.2 s life, one every 0.2 s) is
the likely wing-tip trail. It runs on flight_fx's CzanEff runtime like the flyer effects.

**Binding RE (2026-10-20): main.dol never plays it.** The loader `FUN_80089a24` stores the four
effect files in the resource manager (game + 0x288): +0x30 GAME_STG_EFF, +0x34 GAME_APL_EFF,
+0x38 EVENT_EFFECT, +0x3c CHARA_EFFECT (store at 0x80089b80). The getters `FUN_8008a2f4` (+0x30)
and `FUN_8008a30c` (+0x38) have no +0x3c sibling, and no other read of +0x3c off the manager
exists (a full `.text1` scan; only the reset `FUN_8008a88c` zeroes it). Category registration
`FUN_800fa968` has four callers (categories 0 menu, 1 GAME_STG_EFF via `FUN_8002ddd0`, 3, 4
EVENT_EFFECT via `FUN_800a75b8`); category 2 is never registered. Play `FUN_800fafac` / stop
`FUN_800fb13c` are called only for categories 1, 3, 4. The wing joints `CtsCharaAtc` resolves
(attachment sub-model `FUN_80037b9c`, slots +0x8c / +0x90) are read only by `FUN_80038910` (a
floor-projected position), reached only through a branch in `FUN_8003149c` (`rlwinm.` on r31 at
0x800319fc) that can never be taken. Not checked: a generic loop over the manager in its release
function. **The port's binding is therefore an ASSUMPTION (maintainer's fallback):** effect 0 of
`chara_effects` at both `DB_*wing_02` tips (the joint moved along +x by the bone's length) of a
Fairy dancer — standalone, and the twin once it shows — continuously (`chara_fx::JointFx`, two
instances, pools sized at the simulated peak: ~33 alive per emitter).

## 8. Reproduce

```bash
D=~/Desktop/ddr_wii
scripts/extract_wii_ddr_data.py disc "$D/Dance Dance Revolution - Winx Club (Europe) (En,Fr,De,Es,It).wbfs" "$D/Winx Club (Europe)"
./scripts/validate_wii_ddr_tools.sh "$D/Winx Club (Europe)"
B=/Applications/Blender.app/Contents/MacOS/Blender; E=tools/blender_ddr_addon/examples
GAME=winx DANCERS=all $B -b --factory-startup --python $E/port_character_hottest2.py   # ~5 min, 24 dancers
GAME=winx STAGES=all  $B -b --factory-startup --python $E/port_stage_hottest2.py       # ~1.5 min, 6 stages
python3 $E/port_winx_fx.py      # after the dancers: the Fairy dancers' effect banks + pools
./scripts/build_shaders.sh      # the `_wipe` blobs (pinned fxc under CrossOver)
```
