# DDR FuruFuru Party / DDR MUSIC FIT (Wii JP = HOTTEST PARTY 2 / 3) Assets — RE Notes (2026-10-04)

**Question.** How are the dancers, dance clips, stages and cameras of DanceDanceRevolution
FuruFuru Party (Wii, JP 2008, `RD4JA4` = HOTTEST PARTY 2) and DanceDanceRevolution MUSIC FIT
(Wii, JP 2009, `RJRJA4` = HOTTEST PARTY 3) stored, and how do they port to DDR World as
Background Dancers custom content? HOTTEST PARTY 1 (Hudson engine) is covered by
`docs/wii_ddr_hottest_party_research.md`.

**Answer.**
- **Both games run on Konami's own Wii library, `zan`** (`CzanFileManager`, `CzanModel`,
  `CzanMovieManager`, `CtsStageObj`, … in `main.dol`), not HP1's Hudson engine. Every container
  is a `WII\0` archive of ZMB models, ZAB motions, GX TPL textures and camera records (§1–§4).
  `scripts/zan_dump.py` decodes all of it; its survey parses every archive of both discs with
  0 problems (`scripts/validate_wii_ddr_tools.sh <disc dir>`).
- **One de-duplicated dancer source: `data_mods/custom_models/dancers/HOTTSTPARTY 1-3/`,
  137 dancers, keys `hp<person><nn>`** (§5). MUSIC FIT carries FuruFuru Party's whole cast plus
  remakes of HOTTEST PARTY 1's leads and back-ups in their HP1 outfits on the new rig, so it is
  the only costume source; the choreography library is MUSIC FIT's songs plus FuruFuru Party's
  pieces MUSIC FIT lacks (1022 clips, 73 of them HP2-only). The HP1 port's own 40 dancers and the
  per-game `HOTTEST PARTY 2|3` dancer folders were retired (maintainer, 2026-10-04).
- **Body skin colour comes from a main.dol table, not the textures** (§5.0): the game tints each
  costume's skin material per colour variant. The port applies it (fixed 2026-10-04 after the
  first cabinet test showed pale bodies under dark faces).
- **Names are the games' own** (§6): MUSIC FIT's select-screen name plates matched to costumes
  through the portraits. A back-up costume file holds two people (variants 1/2 and 3/4).
- **Stages: `stages/HOTTEST PARTY 2/` (FuruFuru Party, 59) and `stages/HOTTEST PARTY 3/`
  (MUSIC FIT, 17)** — MUSIC FIT's STG000 and STG041–055 are FuruFuru Party's again (§7.1) and
  ship once, from FuruFuru Party (which still has their movie screens). Movie screens map to
  World's `offscreen1` (§7.3); UV scrolls and texture flip-books both play, the flip-books as
  atlases on stepped UV offsets (§7.4).
- Not yet cabinet-tested.

Tools: `scripts/zan_dump.py` (decoder + conversion math), `scripts/extract_wii_ddr_data.py`
(`disc`, `extract` / `archive` handle zan trees), `tools/blender_ddr_addon/examples/
port_character_hottest2.py`, `port_stage_hottest2.py`, `scripts/test_zan_formats.py` +
`scripts/validate_wii_ddr_tools.sh`. Ghidra: `ddr_furu_furu_party_main.dol` /
`ddr_music_fit_main.dol` (the maintainer's renamed `sys/main.dol`s; addresses below are
FuruFuru Party's, virtual). SDA2 base `r2 = 0x802DF580`.

## 1. Containers

`WII\0` archive: `char magic[4], f32 1.0, u32 count, u32 name_words`, then `count × {u32
offset, u32 size}` (archive-relative), then — when `name_words > 0` — the member names. The
name stride is NOT always `name_words × 4`: `name_words 4` archives pack 17–23-character names at
a 24-byte stride, so the decoder takes the stride that makes every name a clean C string
(`zan_dump._name_stride`). Archives nest; unnamed members are addressed `#index`.

Members are sniffed by magic (`zan_dump.kind_of`): `ZMB GC\0\0`, `ZAB GC\0\0`, GX TPL
(`00 20 AF 30`), cameras (§4: no magic, the 12 constant bytes at +0x34), nested archives, and a
few `zms` / `teb` / plain binaries (fonts, UI layouts) that are extracted but not decoded.

## 2. ZMB models

Header `ZMB GC\0\0`, `u32[3]`, `f32 1.0`, then the offsets of three blocks (textures,
materials, nodes; 0 = absent), each `{u32 count, f32 version, u32 offset, u32 0}`.

- **Textures** are 0x20-byte names only. The pictures live in the TPL beside the model (a stage
  model's `X.tpl`, a costume's colour-variant file); the material texture index is the TPL image
  index.
- **Materials**, version 1.0 = 0x38 bytes, 3.0 = 0x50:

  | off | field |
  |---|---|
  | +0x00 | `u32 color0, color1, color2` (RGBA8), +0x0C `f32` |
  | +0x10 | `u8 flags[4]`: [0] lit, [1] two-sided, [2] & 0x7F blend (0 opaque, 1 additive, 2 darken = ZERO/INVSRCALPHA, 3 alpha) with bit 7 = soft blend vs alpha-test at 160, [3] ≠ 0 an effect pass (`FUN_8010def4` / `FUN_8010dec8`) |
  | +0x14 / +0x18 | `u32 ntex` (low u16) / `u32` list of TPL indices — more than one = a **flip-book** |
  | +0x1F | (runtime) set when the material has a texture matrix |
  | +0x20 / +0x24 | v3: `f32` constant UV scroll per 60 Hz frame (used when there are no keys) |
  | +0x28 | u16 **colour group** (the game re-colours the group's material at run time: 2 = a dancer's skin, 3 = a Mii's favourite colour, 11–43 the Mii body parts; also on 147 HP2 stage materials) — **not** an animation interval; +0x2A u16 = number of layer passes |
  | +0x2C | `u32` layers: more material-shaped records (environment, eye, mouth passes) |
  | +0x30 / +0x34 | `u32 nframes` / `u32` list: the flip-book's **end frame** per texture-list entry (60 Hz); the last is the cycle |
  | +0x38 / +0x3C | v3: `u32 nkeys` / UV-offset keys `{f32 time (s), u, v, u8 flags[4]}` (§2.1) |

- **Nodes**, 0xA0 each: `char name[0x30]` (Shift-JIS), `f32 local[16]` (ROW-vector, translation
  in row 3; `world = local · parent world`), bbox, bone length, `s32 parent`, `u16 flags`
  (1 = sprite), `u16 nsub`, `u32 mesh`.
- **Submeshes** (0x40): material, flags (bit 0 skinned, bit 16 a second UV set), packet /
  position / skin / normal / uv / colour counts and arrays. Every packet is a triangle strip of
  per-corner index arrays. Skin influences name their joint (`char[0x3C]`); positions sit at the
  rest pose in model space.

### 2.1 UV keys and the texture matrix (`FUN_800e921c` setup, `FUN_800e9490` per frame)

The texture matrix is a GX 2×4 matrix in a per-model 0x50-byte array (`model+0x4C`, count
`+0x50`; entry `+0x30` = the material), loaded with `GXLoadTexMtxImm(mtx, GX_TEXMTX0, 2x4)`
(`FUN_8015bbc0`) before `GXSetTexCoordGen2(.., 0x1E, ..)` when material byte `+0x1F` is set.

- A key set has one clock T: period `P = key[n-1].t - key[0].t`, starting at `T = P`, wrapped by
  `P`.
- Each axis has its own key count: the keys before the first one whose flag byte
  `flags[k][axis]` (byte 0 = u, byte 1 = v) is `0xFF`. Its period is `key[n_axis-1].t - key[0].t`.
  Bytes 2 / 3 are not read.
- A segment whose start key has `flags[k][axis] == 1` holds that key's value; any other value is
  linear.
- **Sign:** `m03 = -1 · u`, `m13 = +1 · v` (the per-axis factors are the SDA2 constants
  `r2-0x5D48 = -1.0` / `r2-0x5D4C = +1.0`), and GX samples `s' = s + m03, t' = t + m13`. A
  constant scroll does `m03 -= speedU · 60 · dt`, `m13 += speedV · 60 · dt`, wrapped into (−1, 1).
  DDR World's `m_vTexAnime` adds its offset the same way (`uv' = uv / scale + off`, both
  v-down), so the `.sanm` offU / offV are `(-u, +v)` (`zan_dump.texmtx_offset`,
  `scroll_offset`). The first port wrote `+u` — every horizontal scroll ran backwards.

**MUSIC FIT twins and the units (verified 2026-10-06, decompiled).** The addresses above are
FuruFuru Party's; MUSIC FIT's are `FUN_8010b98c` (per-frame update), `FUN_8010bf58`
(absolute-time variant), both called from the per-model update `FUN_8010c518` ←
`FUN_8010c3b8` (node pose) ← the motion player `FUN_8010b128` (or the motion-less
`FUN_801037e4` / `FUN_80103834`). What `FUN_8010b98c(dt_s, model)` does, in order:
- `frames = 60 · dt_s`; for every material with a non-zero constant speed (`+0x20` u,
  `+0x24` v): `m03 -= speedU · frames`, `m13 += speedV · frames`, each wrapped into (−1, 1)
  (`> 1 → −1`, `< −1 → +1`). Speeds are therefore **texture repeats per 60 Hz frame**.
- then, for every material with UV keys (material version ≥ 3): `T += dt_s` (key times are
  **seconds**), `T` wrapped by the key period, per-axis hold / linear as above, and the result
  is **written** to `m03` / `m13` (`−u`, `+v`) — so when a material has both, the keys WIN
  (the constant-speed increment is overwritten every frame). MUSIC FIT STG201 grid material
  3 has both: keys v 0 → 1 over 0.13333 s (2 keys, `FF 00 FF 00`) and speed (0, 0.125); they
  agree at 0.125 repeat / frame = **7.5 repeats per second**.
- `dt_s` for a stage object = `rate(+0x24c, default 1.0) · speed(+0x198) / 60` per update
  call (`FUN_8010b128`: `(*(obj+0x24c) * *(model+0x198)) / 60.0`) — a FIXED 1/60 s per
  tick, not wall time; `+0x198` is 1.2 when the display runs 50 Hz (`FUN_80100ef0`:
  `**(game+0x258) == 1`), so the scroll stays 7.5 repeats / real second on PAL. No scale, no
  rate multiplier is applied to objects 0 / 1 at the switch (`FUN_80037354` only calls
  `FUN_8010b108(0.0, hole, 0, 0, 0)` at f 300 = seek the hole's motion to frame 0).
- the matrix is the only per-frame texture effect: the draw (`FUN_80107744`) loads it with
  `GXLoadTexMtxImm(mtx, GX_TEXMTX0, 2x4)` (`FUN_80184ce4(.., 0x1e, 1)`) and sets
  `GXSetTexCoordGen2(TEXCOORD0, MTX2x4, TEX0, TEXMTX0 = 0x1E)` (`FUN_801807cc(0,1,4,0x1e,0,0x7d)`)
  when material byte `+0x1F` is set, else matrix 0x3C (identity). No second matrix, no
  projection, no TEV-side coordinate generation.
- the Wii **wraps**: every TPL image header of STG201 (`DRAW_STG201_01.tpl` 15 images,
  `BG_STG201.tpl` 8, the OBJA sets) carries `wrapS = wrapT = GX_REPEAT`, `min = mag =
  GX_LINEAR`, no mips (`extract_wii_ddr_data.parse_tpl` + the 0x20-byte image header:
  `>HHIIIIIIfBBBB`). A 60 Hz emulation of the grid tube with exactly these values
  (`.agents/scratchpad/2026-10-06-flight-re/emu_grid.py`) streams the ring + its three
  trails forward by 16 px of 128 every frame, seamlessly across the wrap — no aliasing, as
  the footage shows.

The older reading in `zan_dump` (step when `flags[2·axis] == 0xFF`) was wrong; 2881 of the 5574
stage UV keys on both discs (`_S` copies included) are `(FF, 01, FF, 01)` = "no u keys, v held" —
atlas-row light animations (STG000 `pl01`: v steps 0 → .25 → .5 → .75).

### 2.2 Flip-books

The texture list (+0x14 / +0x18) cycles through TPL images; entry *i* shows until frame
`frames[i]`, the last entry's end is the cycle (`zan_dump.flip_book`, `flip_index`). Eyes and
mouths are flip-book LAYERS (MUSIC FIT CHR01, Rena: a 7-image blink, 2 frames each). Stage flip-books:
52 on FuruFuru Party's stages, 130 on MUSIC FIT's (`validate_wii_ddr_tools.sh` counts).

The END-time reading is data-derived, and the UV sign of §2.1 confirms it on the one material
that has both: STG042's signboard (`OBJA_N_STG42_signboard01_NC` materials 2/3) flips
`scroll01 → scroll02 → scroll03` at frames 300 / 600 / 900 while its UV keys run u 0 → 1, 1 → 0,
then v 0 → −1 over 5 s each. With end times and `(-u, +v)`, the chevron sheet (`<<`, shown
5–10 s) slides left and the triangle sheet (`▼`, 10–15 s) slides down — each the way it
points (the board's u runs left → right as seen from the stage, its v downwards). Start times
or `(+u, +v)` break one of the two.

## 3. ZAB motions

`ZAB GC\0\0`, `f32 1.0`, bone count, length (frames, 60 Hz), a 0x30-stride bone table → per bone
channels `{kind (0 T, 1 R, 2 S), key size, nkeys, keys}`; a key is `{u32 frame, f32 value[3|4]}`,
rotation a quaternion `(x, y, z, w)` whose row-vector matrix is the local rotation. Values
replace the node's local transform; linear (slerp). At load `FUN_800e2b8c` binds the bones to
nodes by name, rebases the key offsets and turns the frame ints into seconds (÷ 60) in place.

## 4. Cameras

`f32 length (s)` then 6 tracks `{u32 nkeys, u32 offset}`: position `{t, xyz}`, rotation
`{t, quaternion}` (row 1 = −view direction, no roll), FOV `{t, deg}` (MTXPerspective's fovY),
aim `{t, xyz}`, near, far; 12 constant bytes at +0x34. A stage's shots are its own; the generic
dance cameras are `game/GAME_DEF_CAM.bin /#0` (31 in FuruFuru Party, 60 in MUSIC FIT).

## 5. The dancers

A costume is `sound/stream/character/CHR<nn>0.bin` = {body ZMB, head ZMB} plus one texture file
per colour variant `CHR<nn><k>.bin` = {body TPL, head TPL}. One 37-bone Maya rig (+ `Acc*` /
`mii_head` attach joints) for everybody; the head rides `mii_head` (the game swaps it for a Mii).
Eyes / mouth are UV-set-1 overlay layers; the port bakes their first frame into the face texture.
Accessories: `accessory/<Joint>_<costume code>*.bin`. The Mii bodies (FuruFuru Party CHR51–54,
MUSIC FIT CHR81–88) have no head and are not ported.

**Choreography:** each song's `motion/MOT010_SSQ<nnn>.bin` is its dance as one-bar ZAB pieces;
bars come from the song's SSQ tempo. The port chains them into takes, cuts ~8-bar clips, drops
duplicates and deals 12 per dancer (`port_character_hottest2.py` docstring).

### 5.1 HOTTEST PARTY 2 ⊂ HOTTEST PARTY 3

Comparing every FuruFuru Party costume with MUSIC FIT's (triangles, variant counts, texture
names): CHR01–10, 21, 24, 26, 28–30 match MUSIC FIT's same-numbered costume triangle for
triangle; FuruFuru Party's specials CHR41–43 (NAOKI, U1, jun) are MUSIC FIT's CHR11–13; CHR02,
22, 23, 25, 27 are re-exports a few triangles apart (e.g. CHR27 1451 → 1430 body triangles, the
head identical) with two extra (`Acc`) nodes; the texture files are re-encoded (`tex_n_01.tga` →
`HP2A_01_body.tga`), not byte-identical. Only the Mii bodies are HP2-only. MUSIC FIT adds two back-up bodies
(CHR14/15: Bossa / Nova, Hip / Hop) with their HP1-outfit remakes (CHR31/32), its own outfits
(CHR41–55) and the new CHR42/47/48.

### 5.0 Skin tones (`zan_dump.skin_tone_table`)

The body's skin is NOT in its texture. Every costume's body has one material of colour group 2
(§2) over a neutral pale skin sheet; the head's skin is baked into the head texture. At creation
(`FUN_8004a7a8` in MUSIC FIT, `FUN_800403d0` in FuruFuru Party) the dancer copies its tones from
a main.dol table into the object (MUSIC FIT: 6 RGB slots, one per colour variant; FuruFuru Party:
4); on every draw `FUN_8003ac98` finds the group-2 material (`FUN_800edca0(model, 1)`) and sets
its colour to the material colour × the variant's tone (`FUN_800ed73c`). For a Mii the tone is
the Mii's skin colour instead, and group 3 gets its favourite colour.

The table is a block of RGB8 arrays, one pointer per (dancer group, variant slot): groups CHR01–15,
CHR21–32, CHR41–55 (and the Mii bodies, no table), index = costume − group base (`FUN_80021e94`).
Variants 1/2 share one array and 3/4 another, which is what tells a back-up costume's two people
apart: Pia `#ffe1c3` / Gliss `#f4b161`, Forte `#ffd8c3` / Sharp `#dd9058`. The rest: Rena `#ffd29b`,
Domi `#ffe6d6`, U.G. `#8c3602`, Root `#ffc18c`, Chordia `#ecc58f`, Harmony `#a2593f`, Gaku `#f4c7a6`,
Danca `#966437`, NAOKI `#ffe0dc`, U1 `#ffbb90`, jun `#ffe0ce`, Bossa / Nova `#ffdcb3`, Hip / Hop
`#fecd9e`, Dyna `#ffd3b6`, Bridget `#ffeacd`, Ceja `#eaa25d` — the same tone in every outfit of a
person. The first port ignored the table, so every dark-skinned dancer had a pale body under a
dark face; the port now multiplies the tone into the skin material's vertex colours (World's
`_vc` shaders multiply COLOR0, as the Wii's colour register does).

Choreography is NOT a subset: 229 of FuruFuru Party's 1853 distinct dance pieces are not on
MUSIC FIT (songs 032, 036, 041, 046, 035, …), so the library takes them from the HP2 disc.

### 5.2 Cast table (`port_character_hottest2.CAST`)

Variants numbered across a person's costumes in chronological outfit order: the HP1 outfit
(CHR2x/3x), FuruFuru Party's (CHR0x/1x), MUSIC FIT's (CHR4x/5x).

| Person | Costumes (variants) | Dancers |
|---|---|---|
| Rena / Domi / U.G. / Root / Chordia / Harmony / Gaku / Danca | CHR21–28 (4), CHR01–08 (2), CHR41, 43–46 (2; not Domi/Gaku/Danca) | 8 / 6 / 8 / 8 / 8 / 8 / 6 / 6 |
| NAOKI / U1 / jun | CHR11–13 (4), CHR51–53 (2 / 2 / 3) | 6 / 6 / 7 |
| Dyna / Bridget / Ceja | CHR42 / 47 / 48 (4) | 4 each |
| Pia / Gliss | CHR29, 09, 49 variants 1–2 / 3–4 | 6 each |
| Forte / Sharp | CHR30, 10, 50 variants 1–2 / 3–4 | 6 each |
| Bossa / Nova | CHR31, 14, 54 variants 1–2 / 3–4 | 6 each |
| Hip / Hop | CHR32, 15, 55 variants 1–2 / 3–4 | 6 each |

137 dancers, 521 MB, labels ≤ 15 bytes (`Harmony 8`), keys unique (`hprena01` …).

## 6. Naming evidence

- **Name plates** (`select/select_bin_us.bin`, a 26-plate sheet): Rena, Domi, U.G., Root,
  Chordia, Harmony, Gaku, Danca, Mii, jun, NAOKI, U1, **Dyna, Bridget, Ceja, Pia, Forte, Gliss,
  Sharp, Bossa, Hip, Nova, Hop**, RANDOM ♂ / ♀ / ALL. Not in `text/text_*.bin` (ASCII or
  UTF-16).
- **Portraits** (same archive) run: the 8 leads, Mii, jun, NAOKI, U1, then black-haired F,
  platinum M, blonde F, dark-skinned M, green-haired F, red-haired M, brown-haired F, blonde M,
  then the pig-tailed girl (CHR42), the punk girl (CHR47), the capped girl (CHR48).
- **Back-ups.** Rendering all 4 variants of each back-up costume (previews of the exported
  models) shows each file holds two heads: variants 1/2 are one person in black / white, 3/4
  another. The eight back-up plates in plate order (Pia, Forte, Gliss, Sharp, Bossa, Hip, Nova,
  Hop) fall on the eight back-up portraits in portrait order and on these heads: Pia = black
  short hair (CHR09/29/49 v1–2), Gliss = blonde (v3–4), Forte = platinum (CHR10/30/50 v1–2),
  Sharp = dark-skinned (v3–4), Bossa = green long hair (CHR14/31/54 v1–2), Nova = brown (v3–4),
  Hip = red (CHR15/32/55 v1–2), Hop = blonde (v3–4). The fan wiki gallery
  (videogames-fanon.fandom.com, "Hottest Party Dance/Gallery" — a fan-fiction site whose
  back-up pictures are screenshots of these models) names the same eight heads the same way.
  The earlier labels (Backup A–D, Hip/Nova/Hop for CHR42/47/48) came from matching plates to
  portraits by index and were wrong.
- **Dyna / Bridget / Ceja** take the remaining three plates in plate order = CHR42 / 47 / 48 in
  portrait order. Weakest link: plates list them before Pia, portraits after Hop, so their
  mutual order is assumed, not proven.
- **Leads and specials:** plates and portraits agree in both games. HOTTEST PARTY 1's port
  (`port_character_hottest.py`, retired) used placeholder names; its cast maps to these: Emi =
  Rena, Jenny = Domi, Afro = U.G., Rage = Root, Dancer A–D = Chordia, Harmony, Gaku, Danca,
  Backup F 1/3 = Pia and 2/4 = Gliss, Backup M 1/3 = Forte and 2/4 = Sharp (CHR21–30 are those
  costumes on the zan rig).

## 7. The stages

`stage/STG<nnn>.bin` = models `DRAW_*` (stage), `BG_*` (backdrop), `OBJ[AB]_[NZS]_<name>*` (props),
`COL_*` (layout + cull hulls), each with its own one-loop ZAB, plus the camera shots.
`STG<nnn>_S.bin` are split-screen copies (not ported). COL's `OBJSET_<key>_<nn>` nodes place the
prop whose name after `OBJ?_?_` is `<key>` (`FUN_80037634`); `EFF_` / `LIGPOS_` / `LIGTAR_`
nodes are effect and light spots. A prop's name suffix sets object flags (`FUN_800386bc`):
`_NC` 0x80000000, `_BC` 0x40000000, `_MOV` 0x20000000.

### 7.1 FuruFuru Party vs MUSIC FIT vs HOTTEST PARTY 1

- MUSIC FIT `STG000` = FuruFuru Party's byte for byte.
- MUSIC FIT `STG041–055` = FuruFuru Party's re-exported: the same member names, most textures
  byte-identical, geometry identical except where MUSIC FIT replaced the movie-screen props
  (STG046, 048, 050, 054, 055 lose their `_MOV` props; 053 gains two star props) and fewer stage
  cameras. The port ships FuruFuru Party's (with screens) and skips these 16
  (`port_stage_hottest2.is_hp2_reexport`: the same-named HP2 file shares a byte-identical member).
- MUSIC FIT `STG101–103` reuse FuruFuru Party's numbers for NEW stages (no member shared);
  `STG104–111`, `201–206` are new. MUSIC FIT ships 17 unique stages.
- HOTTEST PARTY 1's 42 ported stages are a different engine and art: hashing every ported DDS
  payload finds only a handful of shared textures (one picture common to HP1 stg39/46 and most HP2
  stages; 1–3 between HP1 26/27/28/32/43 and HP2 13/14/25/27/28). So three stage
  sources: `HOTTEST PARTY 1` (42), `HOTTEST PARTY 2` (59), `HOTTEST PARTY 3` (17), no overlap.

### 7.2 Port (`port_stage_hottest2.py`)

Per (model, OBJSET instance) entry, vertices baked through their frame-0 world; parts per (loop
group, blend group) with ≤ 63 animated anchors and ≤ 48 animated material floats
(`frame_board::MAX_MAT_PARAMS`); a flat rig of one bone per animated anchor;
`_play_loop.anm` + `_play_loop.sanm`; cameras `_st` (own) + `_non` (GAME_DEF_CAM).

- **Culling** (fixed 2026-10-05). zan sets `GX_CULL_BACK` unless the material's flags[1] asks for
  no culling (`FUN_8010dec8`: flags[1] → NONE, its second argument → FRONT, else BACK; the draw
  is `GX_TRIANGLESTRIP` 0x98, `FUN_800e50b0`). GX's visible side is the reverse of the strip
  order: on 99 % of the single-sided stage triangles of all four zan discs that reverse agrees
  with the vertex normals. The port used to ship every mesh two-sided (flags 0x0001), so faces
  modelled back to back drew both: FuruFuru Party STG021's fan blades are a front quad
  (`sensu_02`, normal +y) and a back quad (`sensu_01a`, −y) on the same four positions, both
  single-sided, and World z-fought them (the "flicker" on the opening / closing fans; the back
  face won most of the time, so the stage showed the wrong side of every fan). Now
  `hsf_dump.cull_winding`: a no-cull material stays two-sided (`consistent_winding`), a culled
  one is exported single-sided (flags without 0x0001) in the reversed strip order whatever its
  normals say. A mesh under a mirroring world (det < 0: HP2 / HP3 STG043 / 045 backdrops, 0.6 %
  of the single-sided submeshes) stays two-sided, since whether the engine flips its cull mode
  there is not settled.
- **Binds.** A bone's bind is the nearest proper rotation of its anchor's rest world, and its keys
  are `bind · rest⁻¹ · world(t)` — so whatever the rest world holds (non-uniform scale, a
  mirror) the vertices baked through it land where the zan engine puts them. The first port
  normalised the rest world's rows without orthogonalising them; at STG030 that bind was sheared
  by 0.17 and the exporter's (Blender's) bind disagreed — the "re-framed bind" assert. The keys
  are now written against the bind the exporter actually wrote.
- **Shear.** A node rotating under a non-uniformly scaled parent shears (`world = R(t) ·
  diag(s) · R_parent`); World's bones are TRS and its evaluator cancels parent scale, so that
  cannot be carried exactly — STG049's "loop rotation error 0.08" was this, not a slerp. Such a
  bone's bind is turned onto the motion's mean principal axes (the TRS fit is then closest) and
  the port logs a `SHEAR` line with the worst vertex offset (STG049's swinging light cones:
  ≤ 0.55 m at their tips, down from 2.0 m with the plain bind; STG030 ≤ 0.31 m; MUSIC FIT STG103
  ≤ 0.84 m; no other stage).
- **Vertex colours are the disc's bytes** (fixed 2026-10-04). The GX vertex colour is RGBA8 and
  World's `mdl_bg_*_vc` multiplies COLOR0 bytes as is, so the faithful byte is `round(255 v)`.
  The first port wrote them through Blender's linear `color` accessor, which sRGB-encodes, so
  the exporter (which writes the stored `color_srgb` bytes) shipped `round(255 · srgb(v))`. Measured
  on the shipped STG047 (32 distinct RGB triples in the source submeshes' `col`): 32 of 32 shipped
  triples matched `srgb_encode(source)` exactly and only 2 matched the source (the white and black
  ones). STG001: 18 of 19 (19 within ±1, Blender's curve is off by one on 4 of 256 bytes) against 2 of 19.
  E.g. `(63,63,127)` shipped as `(136,136,187)`, `(7,7,7)` as `(46,46,46)`. Re-ported with
  `color_srgb`: 32 / 19 / 58 / 16 of 32 / 19 / 58 / 16 triples exact on STG047 / 001 / 030 / 012 and
  203 of 203 on MUSIC FIT STG101. The buggy HP2 + HP3 output also served as the test corpus for
  `scripts/fix_vertex_colour_srgb.py` (an in-place undo; every other affected source was re-ported in the end). Its
  `round(255 · srgb_decode(b/255))` matched the re-port on 278,464 of 456,665 vertices exactly
  and was ±1 on the rest, with no other byte different.

### 7.3 Movie screens

`_MOV` props (FuruFuru Party STG046–055: `movie01`, `tv01/02`, `mov01/02`, `Atv01`) are a frame
and dressing around a quad on the node named `root`: white texture, vertex alpha 0, aspect ≈1.73.
The game plays a stage movie (`movie/stage/{ut,mt,st,rm}00<n>.thp`, picked by object flags in
`FUN_8002fbec`) or the song's own PV (`movie/pv/ddr%03d.thp`, `FUN_800302e8`) there. The port
textures that quad `offscreen1` (README "Stage screens": World's STAGE SCREENS mode plays the
song's movie on it), remaps its v range onto the 16:9 band 0.21875–0.78125, paints it opaque
white and gives it no material animation; the bezel, letterbox bars, scan-line sheet
(`A_mov01`, v −4..1) and glass overlay (`A_D1_floor2`) stay as they are. The previews put a test
card on `offscreen1` (red left, blue right, yellow top): upright and unmirrored.

### 7.4 Material animation in World

World's `.sanm` animates shader parameters only — its texture-track `.tanm` has no evaluator
(`docs/3d_model_format_research.md` §1, §8), so nothing swaps a material's texture. A flip-book
therefore becomes an **atlas**: its distinct frames side by side along one axis in slots of
`[4 px wrap gutter | frame (twice when that axis also scrolls) | 4 px gutter]`, ≤ 4096 px; the
strip runs along the axis that does not scroll (so the other axis keeps the texture's repeat),
folding into rows (smallest power-of-two area) when it would pass 4096 px and the material never
wraps across (MUSIC FIT STG105 / 111: 20–39 frames of 128–256 px), else its cells shrink along the
strip (only STG109's one both-ways-scrolling 30-frame sheet);
the triangles are clipped at that axis' tile lines and mapped into one cell; offU / offV jump
from cell to cell. Exact steps use two keys on one frame (`core/anm/sample.rs` and
`anm_dump.sample_track` take the later one), so flips and held keys switch on their frame with
no blend. Each part's `.sanm` runs its own clip (the lcm of its materials' periods, ≤ 6 min),
static parts included. The port checks every atlas by sampling it where World will (mesh uv +
offset) against the source frame where the Wii would (tile uv + texture-matrix offset) at
interior points over a cycle, and every `.sanm` against the analytic offsets frame by frame.

Sizes: `HOTTEST PARTY 2` 115 MB, `HOTTEST PARTY 3` 198 MB (the add-on writes uncompressed
A8R8G8B8 DDS; STG111's three 1024 × 2048 atlases are 10.7 MB each), dancers 521 MB.

### 7.5 The flight stages (investigated and ported 2026-10-05; effects, intro cameras, sky burst and its sound too)

Cabinet report: on the "flying tunnel" stages the dancer dances on the launch platform while the
tunnel animation loops. In the games the dancer takes off and flies through the tunnel for the
whole song. What the data shows:

- **Which stages.** MUSIC FIT `STG201 / 205 / 206` (= `hp3stage201/205/206`; three colourways of
  one set: `DRAW_STG201_0n` spiral tunnel, `OBJA_Z_hole0n` tube, `OBJA_Z_BG201` space backdrop,
  plus STG109's stage and sea as `OBJA_Z_DRAW109` / `OBJA_Z_BG109` = the launch platform), HP4
  `STG301` (the same layout, new models) and FuruFuru Party `STG102 / 103` (a ring tunnel, no
  platform). MUSIC FIT STG202 / 204, FuruFuru Party STG101 and HP4 STG200 are bare floor + sky
  stubs (64 + 38–50 vertices), not tunnels. MUSIC FIT's main.dol loads `stage/STG201_EFF.bin`
  when a play-setup flag 0x200 is set (`FUN_800b44d8`, next to the stage file).
- **The flight is the song's choreography, not the stage's.** Flight pieces are authored with
  the Hips at the model ORIGIN (height ≈ 0, a dancer normally stands at 8.6), the torso level
  (Hips → Head ≈ +z) for the whole piece. A scan of every `MOT010_SSQ*.bin` / `DANCE_*_MOT_010.bin`
  finds them only in a few whole songs: MUSIC FIT 046, 047, 048, 049, 051 (13–24 one-bar flight
  pieces each), HOTTEST PARTY 4 053, 054, 055 (17–27; HP5 re-ships them as `DANCE_HP4U_05x_*`),
  FuruFuru Party 049 (two long flight loops, 400 / 300 frames). Each has one TAKE-OFF piece, the
  same 600-frame (10 s) one in all eight MUSIC FIT / HP4 songs (#12 or #13): about 7 s idling at
  the back of the platform (z −21.6), a run to its front edge and a leap (Hips to y 60, z +38);
  FuruFuru Party 049's #1 (260 frames) stands, jumps and turns into the flight pose.
- **The stage.** The spiral tunnel (`DRAW_STG201_0n`: two spirals at the far ends, z ±2055..3205,
  and the grid tube `DRAW_B03_grid01` the flyer is inside, r 101..259, z ±2991) is centred on the
  origin, where the flight pieces put the dancer; the platform is at the origin too.
- **The switch is an intro script in main.dol, armed by the SONG.** A song whose play setup carries
  flag 0x200 (MUSIC FIT `FUN_800b44d8` / `FUN_800b4b70` then also load `stage/STG201_EFF.bin` and
  `game/GAME_CHR_EFF.bin`) arms it at the song start (`FUN_80035634` → `FUN_8003729c`) and
  `FUN_80037354` runs it per 60 fps frame f (constants at SDA2 r2 = 0x8032d840 − 0x7cd0..0x7c68).
  It drives the stage objects, which are the stage file's ZMBs in file order (`FUN_80043508`: 0
  `DRAW_STG201_0n` tunnel, 1 `BG_STG201` space with planets, 2 COL, 3 `OBJA_Z_BG109` sky + sea,
  4 `OBJA_Z_BG201` plain space, 5 `OBJA_Z_DRAW109` platform, 6 `OBJA_Z_hole0n` tunnel mouth; MUSIC
  FIT STG205 / 206 and HP4 STG301 keep the order):
  - at f 0: 0, 1, 6 hidden; 3, 4, 5 shown;
  - f 60..210: object 5's colour `0.85·(210 − f)/150 + 0.15` (the platform dims to 0.15);
  - f 180: stage effect (category 5 = STG201_EFF, `FUN_800431a4`, at a COL node) + sound 0xf6 / 0xf7;
  - f 300..360: object 6 fades in (alpha `(f − 300)/60`) and starts its motion at 300 — the
    one-shot opening (`Dummy_scale` 0.3 → 1 and the `Dummy_tube_*` nodes extending over its frames
    0–134, then held to 2000). Its grid tube `DRAW_B03_grid02` is SKINNED to
    `Dummy_tube_front / center / back`: over the opening it stretches from the mouth (z ≈ +2500,
    y ≈ 650) back to z ≈ −2500 through the origin — around the launch platform, so from about 7 s
    the take-off runs inside the scrolling grid tube (the tunnel "arriving" at the burst);
  - f 360..420: object 3 fades out (`(420 − f)/60`; hidden at alpha ≤ 1e-5), revealing object 4;
  - f 544: the character effects, mode 2 (`FUN_8004b5a8(.., 2)`) — the leap of the take-off;
  - a stage-controller value (`+0xea0`, by its shape a light level: 1 → 0.5 over f 60..210, → 0.15
    over 360..420, a flash back to 0.5 at 422..423) — not ported;
  - the switch: when the intro camera ends (`FUN_80048290`; the cameras `STG201_CAM00_01..03`,
    3 + 4 + 3 s = 600 frames, the take-off's length) objects 0, 1 show, 3..6 hide and the character
    effects switch to mode 0 (1 when the play mode `+0xe4` is 4).
  - from the switch the WHOLE scene (camera, stage, dancers) is translated by a world offset that
    starts at (0, 0, −50000) and moves +3 units per frame (play object +0x3ac, `FUN_800377e0`):
    invisible except to world-space effect state, which streams behind the flyer — the rainbow
    trail (`docs/wii_ddr_zan_effects_research.md` §3.4).
  The first port looped every model's ZAB and showed them all, so the tube re-opened every 33 s —
  the "broken looping tunnel".
- **The effects** (ported 2026-10-05; full RE in `docs/wii_ddr_zan_effects_research.md`). zan `CzanEff` banks by category: 1 `GAME_STG_EFF`,
  3 `GAME_APL_EFF`, 5 `STG201_EFF` (`stage_effects01.teb`, 1 effect), 6 `GAME_CHR_EFF`
  (`boss_ddr3.TEB`, 24 effects + 17 textures: star sprites per player colour — gold, blue, pink,
  green — a rainbow gradient strip, rings, flares). `FUN_8004b5a8` plays, for player p in mode m,
  effects `8m + 2p`, `8m + 2p + 1`, `8m + 2p + 1` (table `0x80218ad8`) at the dancer's attach
  joints (`0x80218b68`: modes 0 / 1 joints [0, 1, 2], mode 2 [5, 3, 4]; joint 5 position-only) and
  `FUN_8004b360` re-sets their matrices each frame. A TEB holds effects → node trees (per node
  `{u8 child, u8 next, u8 type, u8, u32 data}`: type 0 root, 1 `CzanEffPart` emitter, 2
  `CzanEffMdl` model); a part's data is a flag word selecting sub-blocks (emitter: life, spawn
  interval, max / per-spawn counts, position / rotation spread; draw: texture, size, key lists;
  child-chain trails; ribbons; velocity / gravity; key tables). The runtime is a full particle
  system (`FUN_8012b5b8` part update + spawn, `FUN_8012ce18` particle step: velocity, gravity,
  camera-facing billboards from the camera matrix, size / alpha / colour keys over the normalised
  life, flip-book UVs; `FUN_8012ddf4` draw; `FUN_80131d08` / `FUN_80131764` ribbon strips; play /
  matrix / stop API `FUN_8012aca8` / `FUN_8012af28` / `FUN_8012ae74`). Nothing in it is geometry
  that could be ported as a model: the port re-implements the particle runtime
  (`src/mods/background_dancers/flight_fx.rs`) and draws it through bone-driven pool models.
- **The port** (2026-10-05). Characters: flight / take-off pieces (`zan_dump.piece_class`) leave
  the dance library; every zan dancer gets `motion/flight/takeoff.anm` + 4 flight loops
  (`port_character_hottest2` FLIGHT). Stages: `mapset_<key>/flight.txt` marks the six stages; parts
  are named by role — `pre_plat_*` (object 5), `pre_sky_*` (3, a `_bg` part), `pre_space_*` (4,
  `_bg`), `pre_hole_*` (6, a one-shot clip), `fly_*` (0 and 1: `fly_bg` is the planets' space) —
  and Background Dancers replays the intro script on them on the take-off's clock
  (`director_math::intro_look`, through the instance tint) and switches at the take-off's end. The
  FuruFuru Party ring tunnels (STG102 / 103, no platform) carry the marker alone; how FuruFuru
  Party switches them (its flight song 049 has its own 260-frame take-off) was not looked at.
  HP4 STG301 runs MUSIC FIT's script (HP4's `FUN_80106298` is the same at f 180 / 544; its sky
  fades 390..420, not ported).
- **After the first cabinet run (2026-10-05).** (1) The tunnel lines shuffled back and forth: two
  causes, both fixed. The tunnel's UV scrolls (`DRAW_STG201_0n`: a 40-frame-and-a-hair v period,
  a 482.x-frame u period) wrap BETWEEN two integer frames, where the `.sanm` key writer saw no jump,
  so the segment after the wrap slid the texture back a whole repeat within one frame — the writer
  now continues the slope to the wrap frame and pairs the keys there (`offset_keys`; every stage
  re-ported, the fix is generic). And the `fly_*` parts ran on DANCE time: at chart tempo × the
  grid's 0.125-texture-per-frame scroll the lines strobed; they run on the real clock now, as on the
  Wii. (2) The take-off ran on dance time and was filmed by the stage's main shots: it now runs on the
  real clock and through the intro shots (exported as `<key>_intro01..03`, see
  `docs/wii_ddr_zan_effects_research.md` §5), so the camera faces the mouth during the burst
  (STG201_CAM00_02, intro 3–7 s, ~10° off the burst). (3) The sky burst (f 180 stage effect at
  `EFF_04_01`) and its sound `SE_DDR_BOSS` are ported (effects doc §1.2 / §5).
- **Intro cameras (the camera controller's mode 2).** Both games load a stage's camera archive
  `/#1/` the same way (FuruFuru Party `FUN_8003d234` from `FUN_8002cbb8`; MUSIC FIT `FUN_800472d0`
  from `FUN_80033c20`): `/#1/#0` = the main shots, `/#1/#1`, `/#1/#2` = close-up groups (MUSIC
  FIT: a `CAM02` entry), and any camera DIRECTLY in `/#1/` is appended as the intro list (`+0x1f4`
  / `+0x240`, mode `+0xc4` / `+0x100` = 2). `FUN_8003dfd0` / `FUN_80048290` report the intro over;
  FuruFuru Party's play sequence (`FUN_800302e8` state 4) waits for it AND 1 s (`r2-0x7d4c` = 1.0
  × 60 frames) before state 5 starts the song (and the PV) — every FuruFuru Party stage has one
  3 s intro shot `/#1/#3`, played before the song. MUSIC FIT STG201's intro list is
  `STG201_CAM00_01..03` (10 s) = the take-off, which is why READY comes after the take-off there.
  The port exports a flight stage's intro shots as `<key>_intro01..` (`is_intro_camera`):
  FuruFuru Party STG102 / 103 get their `/#1/#3` (3 s; the stage cycle takes over after it).
- **READY delay (implemented 2026-10-05, cabinet check pending).** In the games the take-off is a
  cut-in BEFORE gameplay (READY after it). World's intro: DPS step 5 waits for the 5.0 s READY?
  dwell (`DPS+0x130` vs 5.0, `docs/quick_restart_fail_speedup_research.md` §12.2) with the stage
  panel (ShutterActor kind 3) up; the song starts at step 6/7; the panel reveals and shows READY?
  after that. The hold (`background_dancers::flight_hold_logic`): the stock panel for 4 s of the
  dwell, then the panel's layer hidden (`afp_layer_set_attribute(id, 1, 0)` — not dismissed, so
  its own reveal and READY? still follow the song's start) and the dwell timer held at 4.0 while
  the take-off plays on its own wall clock (the scene graph is already enabled in step 5); 0.30 s
  before the take-off's end the panel shows again and the timer is seeded past the threshold, so
  music 0 lands on the switch. Cabinet run #4 (both skins: no hold) and the disassembly of 20260915
  `DancePlaySequence::onUpdate` step 5 (+0x591E4..+0x592B5) refined it: the gate is `timer >= 5.0`
  ∧ ShutterActor state ∈ {0, 4} ∧ the song bank prepared, and only once it passes does the DPS
  broadcast `0x1043`, call the shutter's reveal (state 4), set the SceneGraph ENABLE bit and step
  on — nothing 3D draws during the dwell, so the hold sets the bit itself
  (`scene3d::scene_graph::set_enabled`) and starts only once the panel settled (state 4). DDR
  SELECTION's legacy panel seeds the timer to 1000 every pre-song frame from the ShutterActor
  update; it defers to a hold announced through `services::ready_hold`. Stand-down (the take-off then plays over the song as before):
  another driver seeding the timer (quick restart's fresh DPS, DDR SELECTION's legacy intro), the
  scene not ready within 4 s, the timer unresolved. Open for the cabinet: what else World draws
  in step 5 (lanes / HUD under the panel), the release-to-anchor latency (logged), whether the
  panel's own animation re-asserts its visibility.
- **After the third cabinet run (2026-10-05).** (1) The tunnel grid still stuttered at 120 Hz, the
  `.sanm` offline monotonic. A smoothed real clock (`clock::SmoothClock`, the music count tracked
  in wall time) was tried, deployed in runs #4 / #5 with no visible effect, and removed: the real
  clock is the raw music count again (nothing in the DLL alters or substitutes the game's clock),
  and a bounded read-only diagnostic (`lifecycle::FlightDiag`, removed after run #6) logged the
  clock, the grid offset and the frame-board reads after the switch. The stutter's cause was
  World's CLAMP sampling of mod textures — see "Session 6 RE" below.
  (2) No scroll during the
  take-off: the port baked every stage mesh RIGID at rest, but both tunnels are skinned (the only
  skinned stage meshes on the four discs, with HP4 STG002 / 043's filter props): the mouth's tube
  never stretched around the platform, and the flight tunnel `DRAW_B03_grid01` (skinned to
  `Dummy01` / `Dummy02` / `Dummy03`) stayed straight where the Wii bends it — its ends sway up to
  ~1400 units over the 2000-frame loop. `port_stage_hottest2` now ports skinned submeshes (one
  bone per skin joint referenced to the joint's rest world, ≤ 4 weights; checked by re-skinning
  the exported `.model` with its `.anm` against the zan deformation); HP3 STG201 / 205 / 206,
  HP4 STG301 / 002 / 043 re-ported. (3) The READY hold above.

- **Session 6 RE (2026-10-06) — the tunnel jitter and the skybox smears have ONE root
  cause, in World, not in the clock.** (1) The world offset re-verified (`FUN_800377e0`):
  `z += 3.0` per unpaused tick while `z < 50000`; the same vec3 is copied into the camera
  (`camera+0x60`, added to eye AND target in `FUN_800478b8`), into every stage object's base
  matrix (`FUN_80042fa0` → `FUN_800459fc(obj+0x14)` = `MTXTransApply(identity, offset)`) and
  into every dancer's matrix (`FUN_800fb500`) — same sign, same value: nothing moves relative
  to the camera, the tunnel's motion IS the texture matrix alone (§2.1). The port is right not
  to move the tunnel. (2) World samples every mod-shipped texture with **CLAMP** (not in
  `data/data/texture.db` → default attr `0x55`; `docs/3d_model_format_research.md` §3.8),
  where the Wii's TPL says REPEAT. The ported grid tube maps v −2 → −0.5 → 1 across each pair
  of rings and repeats that (plus −5 / 4 at the ends), so under CLAMP each segment shows ONE
  ring cluster where `v + offV ∈ [0, 1]`; as `offV` runs 0 → 1 the cluster slides one repeat
  along the tube and then **snaps back** a whole repeat when the clip wraps — a 7.5 Hz
  sawtooth at any frame rate (offline: ring at mesh v 0.24 → −0.61 over 8 frames, then 0.24
  again; under WRAP a third ring enters as one leaves and the stream is seamless). That is
  the "back and forth" (the "slow creep" is the tube's skinned bend); it is why neither the
  `.sanm` wrap fix nor the smoothed clock changed anything, and why the cabinet shows sparse
  single rings (one per segment) instead of the Wii's dense ring + trail clusters. The same
  CLAMP smears `fly_bg` (u −4.19..5.13) into grey bands and `fly_add`'s streaks; the planets
  (UVs inside [0, 1]) are untouched. (3) Second, independent issue kept from the run-#5
  analysis: `fly_ble` is `0x2C0` (alpha blend, z-write ON) and World's blended alpha test is
  `GREATEREQUAL 0`, so fully transparent grid texels write depth and reject the additive smoke
  behind the tube (hard-edged patches). The Wii's z-write for its "soft" (`0x83`) materials
  was NOT traced (the GX state wrappers in `FUN_80107744` are TEV / vertex-descriptor calls;
  the blend / z-mode set-up sits in the stage pass, not RE'd) — assumption, not fact.
  Fix applied and cabinet-confirmed (run #6, 2026-10-06): `background_dancers::texture_wrap`
  patches World's texture.db default attr to WRAP (`docs/3d_model_format_research.md` §3.8) —
  the grid streams steadily and the sky is clean; the z-write issue (3) produced no visible
  artefact once the textures wrapped, so mesh flag `0x400` was not applied.

## 8. Not done / open

- Cabinet test (screens in STAGE SCREENS mode, flip-books, the `.sanm` budget).
- DXT output for the atlases (World's DDS reader takes DXT1/3/5; the add-on's writer does not).
- A part's `.sanm` clip is the lcm of its periods capped at 21600 frames: where the lcm is larger
  (about a dozen parts) the shorter loops restart once per 6 minutes.
- Shear (§7.2) is approximated; the worst cases are additive light cones.
- Dyna / Bridget / Ceja's mutual order (§6).
- Stage materials also carry colour groups (§2); what the stage code puts in them
  (`FUN_80036d20`: per-stage colours on up to 10 materials × 8 groups) is not ported — the
  stages keep their file colours.
- Not ported: the Mii bodies, `_S` split-screen stages, COL hulls, EFF effect nodes, the eye /
  mouth flip-book animation on the dancers (first frame baked), additive dancer passes, the stage
  lights (`LIGPOS_` / `LIGTAR_`), the flight stages' `CzanEff` particle effects (§7.5: the flyer's
  orb, rainbow trail and stars; the intro's stage effect) and their light-level ramp.

## 9. Reproduce

```bash
# dump a disc: scripts/extract_wii_ddr_data.py disc <game>.wbfs ~/"Desktop/DDR Wii ISOs/Music Fit (Japan)"
./scripts/validate_wii_ddr_tools.sh ~/"Desktop/DDR Wii ISOs/Furu Furu Party (Japan)" ~/"Desktop/DDR Wii ISOs/Music Fit (Japan)"
DANCERS=all /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
  --python tools/blender_ddr_addon/examples/port_character_hottest2.py          # ~10 min, 137 dancers
GAME=hp2 STAGES=all /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
  --python tools/blender_ddr_addon/examples/port_stage_hottest2.py              # 59 stages
GAME=hp3 STAGES=all ... port_stage_hottest2.py                                  # 17 stages
```
