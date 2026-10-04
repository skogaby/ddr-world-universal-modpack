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
  | +0x28 | low u16 = number of layer passes; the high u16 is a per-model serial (nonzero on 147 non-animated HP2 stage materials, zero on 18 flip-books) — **not** an animation interval |
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

## 8. Not done / open

- Cabinet test (screens in STAGE SCREENS mode, flip-books, the `.sanm` budget).
- DXT output for the atlases (World's DDS reader takes DXT1/3/5; the add-on's writer does not).
- A part's `.sanm` clip is the lcm of its periods capped at 21600 frames: where the lcm is larger
  (about a dozen parts) the shorter loops restart once per 6 minutes.
- Shear (§7.2) is approximated; the worst cases are additive light cones.
- Dyna / Bridget / Ceja's mutual order (§6).
- Not ported: the Mii bodies, `_S` split-screen stages, COL hulls, EFF effect nodes, the eye /
  mouth flip-book animation on the dancers (first frame baked), additive dancer passes, the stage
  lights (`LIGPOS_` / `LIGTAR_`).

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
