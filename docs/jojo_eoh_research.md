# JoJo's Bizarre Adventure: Eyes of Heaven / All Star Battle R (PS4) — data formats, the dancer and stage ports

Research for porting the EoH character models (and, later, the All Star Battle R stages, which
share every format below) into DDR World's Background Dancers. Tools: `scripts/cpk_dump.py`,
`scripts/xfbin_dump.py`, `scripts/unfself.py`; the port is
`tools/blender_ddr_addon/examples/port_character_eoh.py` (+ `eoh_source.py`). Status 2026-10-24:
Jotaro, Dio Brando, DIO, Star Platinum, The World and Gyro ship as
`data_mods/custom_models/dancers/JOJO EOH/<Name>` (keys `eoh<code>`), round-trip previewed; not yet
cabinet-tested.

## 1. Package

`jjba.pkg` (5.26 GB, `EP0700-CUSA04909_00-JOJOEOH000000000`, EU, pubtool 2018-03-25) is a fake
PKG: PkgTool.Core (LibOrbisPkg 0.2, macOS x64 build under Rosetta) `pkg_extract` unpacks it with
the default passcode. `uroot/eboot.bin` is a fake-signed SELF with every segment unencrypted;
`scripts/unfself.py` rewrites it as a plain x86-64 FreeBSD ELF for Ghidra (not needed so far —
every format below was readable from public documentation plus checks against the data).

The game data is seven CRI CPK archives in `uroot/data/sim/` (`CPKMC2.45.00`, plain `@UTF`
tables, TOC with directory names, members uncompressed, 2 KiB aligned):

| archive | files | content |
|---|---|---|
| `data0.cpk` | 4710 | `data/spc/` per-character parameter / effect / colour-set xfbins, `data/com/`, UI |
| `data1.cpk` | 536 | misc |
| `dataRegion0.cpk` | 2029 | `data/spc/PS4/<code>bod1.xfbin` **(the character bodies)**, `data/level/PS4/`, `data/stage/PS4/`, `data/gimmick/PS4/`, `data/mob/PS4/` |
| `dataRegion1.cpk` | 1654 | more PS4 region assets |
| `stream0.cpk` | 261 | movies, `data/sound/PS4/` xfbins |
| `sound0/1.cpk` | 146 | ACB / AWB audio |

9001 of the ~9300 members are `.xfbin`. `scripts/cpk_dump.py list|extract` reads them (and
decompresses CRILAYLA members should another CPK use it).

## 2. Content map

Character code = `<part digit><3 letters><costume 2 digits>`: `3jtr01` Jotaro, `1dio01` Dio
Brando, `3dio01` DIO, `3stp01` Star Platinum, `3wld01` The World, `7jir01` Gyro (`7jny` Johnny,
`1jnt` Jonathan, `4jsk` Josuke, `5grn` Giorno, `6jln` Jolyne, `8jsk` Josuke 8, ...; ~250 codes).
Costume `11/21/31..` are alternate outfits (only `col` + `bod1` files). Per character:

- `data/spc/PS4/<code>bod1.xfbin` — body, face, eye_l/r, upper/lower teeth, tongue models, the
  clump rig (130–190 coords), dynamics (cloth physics), `_m` (mask) / `_n` (normal) / `_iris`
  textures. Some have extras: Dio `kiba` (fangs), `arm` + `arm cool` (alternate arms), Gyro
  `l/r holster01/02`.
- `data/spc/<code>col<N>.xfbin`, N = 0..3 — colour set N: `t_<code>_body01_d_#N` (2048×1024 DXT5
  diffuse) and `_s_#N` (shadow). The body's materials reference the `#0` names; the game swaps the
  set.
- `data/spc/<code>mot1/2`, `act`, `apl`, `end`, `start`, `gha*` (Great Heat Attack cinematics:
  camera + anm + particles), `prm*.bin` — not used by the port.
- `data/level/PS4/lv_sa0Na_NN.xfbin` + `data/stage/PS4/sa0Na.xfbin` — the brawler arenas (one `sa`
  per Part: `sa01` Part 1 … `sa08`, `sa99` extras; `sd*` demo stages). Not ported: ASBR's
  fighting-game stages are the better DDR fit (`.agents/planning`, hybrid plan).

## 3. XFBIN (NUCC container)

Big-endian everywhere (PS4 included). Layout in `scripts/xfbin_dump.py`'s docstring; summary:

- header `NUCC` … (0x1C bytes), then the chunk table: counts/sizes of the type / path / name
  string tables, the strings, `(type, path, name)` chunk maps, chunk references, and the chunk-map
  index list;
- pages: chunks `size:u32 map_index:u32 nucc_id:u16 unk:u16 data`, ending with `nuccChunkPage`
  (`page_size`, `reference_size`). **Every chunk index inside a chunk's data is relative to the
  page's slice of the index list**, and a slot in that slice can name a chunk stored in ANOTHER
  xfbin (the body's materials point at the `col` file's diffuse) — the reader keeps those as
  `external` stubs with only type / path / name.

Chunk payloads decoded: `nuccChunkTexture` (u16 pad, w, h, pad, u32 size, NTP3), `nuccChunkCoord`
(f32 position[3], rotation[3] degrees, scale[3], f32, u16), `nuccChunkClump` (coord count + parent
s16 list + coord indices; model indices; model groups), `nuccChunkModel` (u16, rigging flag u16,
material flags u8[4], u32 0, clump index, hit index, bone coord index (absent when it equals the
NUD size), optional 6 floats when `material_flags[1] & 4`, NUD, u16 material count + indices),
`nuccChunkMaterial` (group count, format byte → float count, texture groups of texture indices).
The material floats are, per set bit of the format byte, `UV0..UV3 = (offset u, offset v, scale u,
scale v)` (bits 0..3), then blend rate / type (0x10), falloff (0x20), outline id (0x40); the shaders
sample `uv * scale + offset` — a flip-book sheet is a 1/8 × 1/4 (fire) or 1/16 × 1 (crowd) cell
scale with the offset stepping by cells (sd01afire03's static `(0.75, 0.25, 0.25, 0.25)` = cell 3,
row 1).

### Anm (`nuccChunkAnm`, `xfbin_dump.AnmChunk`)

Layout after SutandoTsukai181's `xfbin_lib` `anm_wip` branch (curve formats, the material curve
order from Al-Hydra's Blender-XFBIN-Importer); every one of the 350 Anm chunks of the 34 ASBR stage
files parses to its exact payload size:

```
length:u32 (ticks)  frame_size:u32 (100 ticks per frame)  entry_count:u16  loop:u16 (1 = looping)
clump_count:u16  other_count:u16  coord_parent_count:u32
clumps[]: clump_ref:u32 bone_count:u16 model_count:u16 bone_refs:u32[] model_refs:u32[]
others: u32[other_count]                       page chunk indices (camera / light chunks)
coord_parents[]: parent_clump:s16 parent_bone:u16 child_clump:s16 child_bone:u16
entries[]: clump:s16 bone:u16 kind:u16 curve_count:u16, curve headers (index:u16 format:u16
           keys:u16 flags:s16), then each curve's keys, 4-byte aligned per curve
```

- **Clump / bone / model refs index the page's chunk REFERENCE table** (the `(name, chunk map)`
  pairs between the chunk maps and the index list, `reference_size` of each `nuccChunkPage`), not
  the page's chunk indices: the reference NAME is the instance the clip drives, the chunk map its
  template. One template coord is stamped many times by effect clips (`sd04a_eff_lightred000..005`
  → coord `sd04a_eff_lightred00`); the clip's own coord tree comes from `coord_parents`.
- Entry kinds 1 coord, 2 camera, 4 material, 5 directional light, 6 point light (9 ambient).
  Coord curves: 0 location (cm), 1 rotation, 2 scale, 3 visibility. Material curves (18 floats):
  0..7 offsets u/v of UV0..UV3, 8..11 scales of UV0/UV1, 12/13 blend rates, 14 falloff, 15 glare,
  16 alpha, 17 outline id.
- Curve formats: `0x05` / `0x08` / `0x15` vec3 per frame (0x08 = Euler degrees, the coord order),
  `0x06` / `0x0A` / `0x0C` keyed (`s32 frame in ticks` + vec3 / quaternion / float, ending in a
  frame −1 sentinel), `0x0B` / `0x16` / `0x18` float per frame, `0x0F` s16 / 0x8000, `0x10` s16 vec3
  / 0x1000, `0x11` s16 quaternion / 0x4000, `0x14` u8 rgb. A per-frame curve carries frames
  0..length/100 (frames + 1 keys; 1 key = constant).
- **Quaternions are the inverse of the coord rotation**: conjugated, sd02a's horse clip reproduces
  the rest coords to 1e-3; raw it is off by up to 1.7 (the Blender importer also inverts them).
- **30 Hz**: the Blender importer plays them at 30 fps; Battlefield's chariot lap (≈ 198 m in 330
  frames) is 18 m/s at 30 Hz. The port keys every 2nd World frame.

| stage | what the looping clips animate (ported) | not ported |
|---|---|---|
| sd01a Dio Castle | sea UV scroll (two normal layers), sky cloud ring rotation + cloud UV, crowd billboards (`planemob`, 16-cell flip-books), fireplace / balcony flames (8 × 4 flip-books) | light glows, shine dust, NPC zombies |
| sd02a Battlefield | sky + cloud rings (rotation), torch flames (flip-books), the `08moball` banners / flags (flip-books), **Wamuu's chariot lapping the rim** (below) | gimmick stones, heat haze, mob soldiers (skinned NPCs) |
| sd03a El Cairo City | sky cloud UV, crowd billboards (flip-book) | the flag (dropped clump), newspaper, traffic signals (visibility), car gimmick |
| sd03b DIO Mansion | sky cloud UV | smoke, shine lines, nuke gimmick |
| sd04a Morioh Town | sky cloud UV, crowd billboards, the burning house's three smoke columns (`sd04aeff` instances of `sd04a_eff_bigsmoke00..02`, scrolling smoke sheets, whole song — `INSTANCES`) | red lights (visibility), puddles, hose spray (particles), NPC crowd (skinned) |
| sd04b Kira Estate | — | flying debris (`flyobj`, dropped), smoke, NPCs |
| sd05a Naples Station | sky UV, wet-road UV scroll (`dirt`), headlight beam drift | the driving cars (drive-through), signals |
| sd06b Green Dolphin | sky cloud rotations | frog rain gimmick |
| sd06c Kennedy Space | sea UV scroll, cloud rings | shore foam strips (`wave00`, a dropped clump), gimmicks |
| sd07a Philly Coast | sky UV, sea UV scrolls (static plane, near sea), the shore sea mesh's 34 wave bones, wave strips | waterfall / shine particles |
| sd08a Wall Eyes | sky cloud rotation, sea + waterfall UV scrolls, wall wet sheen | gimmicks, NPCs |
| sa03e Cairo Bridge | river UV scroll, the burning truck's fire sheets | sky clouds (the clip does not loop), knife gimmick |
| sa04b Cape Boingy (first shipped mislabelled "Kira Hse Fire"; the game's stage select calls it **Cape Boingy-Boing**, icon `ssel_icon_4byn`) | sky cloud rotations, sea UV (the clip names `…sea00_01`, the file `…sea00_m01`: aliased), 27 wave strips' drift | its gimmick is three Boingy-Boing manga objects (`dogu` / `dohyuuu` / `dojuu` sound-effect props + particles); **no fire sheet, mesh or particle exists in `sa04b` / `sa04b_sf00`** — the burning house with the fire engine is Morioh Town |
| sa05a Colosseum | sky cloud rotations | wheelchair / sleep gimmick |
| sa07a Rocky Mtn Vilg | sky / star / cloud UV scrolls | lamps (particles), smoke, Kayaro gimmick |

Folder labels follow the game's stage select (`data/ui/btlsel/ssel/eng/ssel_txt.xfbin`: the
`ssel_icon_<code>` cards and the `ssel_stage_name` sheets), shortened to 15 bytes. The first ship
mislabelled five (sd03a / sd03b swapped as "DIO Mansion" / "Cairo Rooftop", sa04b "Kira Hse Fire",
sa05a "Naples Rain", sa07a "Philadelphia"); renamed 2026-10-25. Keys (`jj<id>`) are unchanged.

Battlefield's gimmick is a horse chariot (`2kyu01t0 trall` horse rig instanced twice,
`2kyu03t0 trall` the chariot, `sd02am2vmp01bod3` its rider) with clips `nut00` (idle at the centre,
240 frames), `run00` / `run01` (in-place gallop, 60 / 40 frames) and the route dummies `route00`
(entry), `route01` (a closed lap of the rim, ≈ 58 × 67 m, 330 frames) and `route02` (a lap through
the centre). The port (`LAPS`) carries `run01`'s rig along `route01`.

### Rig

`world = parent_world · T(position) · R · S(scale)` with `R = Rx · Ry · Rz` (a column vector is
rotated about Z, then Y, then X — Max's XYZ Euler). Checked on every body: the other order folds
the figure flat. Root `<code>t0` sits at the pelvis (`(0, -6.8, 104.9)` cm, rotated -90° about Z),
`pelvis` is rotated `(-90, 0, -90)`, Biped bones point down their local +X. Mesh space is Z-up
centimetres, the character facing **-Y**, left hand at **+X** (Blender's and the DDR add-on's
axes; no mirror). Jotaro: head joint 181.6 cm, hands at ±80 cm in a T-pose, feet at 12 cm,
toes at 0.

Coord names (`<code>t0 ` prefix dropped): `pelvis spine spine1 spine2 neck head`, `l|r clavicle
upperarm forearm hand finger0..4 (+ 01/02 phalanges) wep d`, `l|r thigh calf foot toe0`, twist
helpers `l|r arm bone01..06`, `clavicle bone`, face rig `face01 face02 jaw c nose l|r eye 01..08
mayu 01..05 lip 01..07 cheek 01..03 eyebone01/02_l|r`, hair / cap, cloth chains `l|r cloth01..07`
(Jotaro's coat), `collar01..`, `chain01..04`, `pocket`, Dio `muffler00..04`, Gyro `mant00..33`
(cape), `b hair00..32`, `l|r heel`. Model anchors `body`, `face`, `eye_l/r`, `tongue` are
identity coords.

### NUD (`NDP3`)

Header (bone range, three buffer sizes, bounding sphere), 0x30-byte mesh groups, 0x30-byte
meshes. Vertex layout per `vertex_size`: EoH bodies use `0x13`/`0x33` — f32 position, pad, f32
normal, pad, bitangent[4], tangent[4] (type 3), bones type `0x10` (u32 ids[4] + f32 weights[4];
Dynamics-heavy models use 0x30 → half floats) — and `uv_size` `0x12` / `0x22` (1 or 2 half-float
UV sets + u8 RGBA colour, in the separate pre-vertex buffer because the mesh is skinned). Vertex
colours are 255 everywhere. Bone ids index the clump coord list. Faces: strips with -1 restarts,
**clockwise from outside** (checked against the vertex normals on all six bodies: 0 CCW
triangles); `xfbin_dump` emits them CCW. Materials: 32-byte header (flags, blend factors, alpha
test, `cull_mode` 0x405 = back / 0 = none, z offset), 24-byte texture slots, `NU_*Param`
properties (`NU_toneParam`, `NU_outline0Param`, `NU_hatchingParam`: the cel shader's).

Unskinned models (`rigging_flag & 1`: teeth, fangs) are authored in their coord's frame — the
reader keeps them raw, the Blender builder moves them to the coord's world and weights them to it.

### NTP3 textures

`NTP3`, version 0x100 here: per texture `total:u32 0:u32 data:u32 header:u16 0:u16 u8 u8 mips
u8 fmt:u8 w:u16 h:u16 u32 cube:u32`, then 16 bytes of offsets (version ≥ 0x200 only — in 0x100
the data starts at `+0x10 + header_size`), per-mip sizes when `mips > 1`, eXt/GIDX. Pixel formats
0 DXT1, 1 DXT3, 2 DXT5, plain linear block data — **no PS4 swizzle** (`xfbin_lib`'s NUT reader
rejects the 0x100 header layout; `xfbin_dump.parse_ntp3` handles both). `NutTexture.to_dds`
writes a DDS, optionally dropping levels above `max_size` (2048×1024 → 1024×512, which the game
accepts; `load_texture`'s 1024 cap).

The iris sheets (`t_<code>_iris`, 256², DXT1) keep the iris at UV `(0, 0)` and rely on the
sampler's wrap: the eye meshes' UV set 1 runs −0.56..0.59. `eoh_source.roll_half_dds` rolls the
DXT block grid by half (lossless; mips below 8 px dropped) and the port shifts the UVs by 0.5.

## 4. Port (`examples/port_character_eoh.py`)

The donor-rig flow (`port_lib`): `eoh_source.build` makes a Blender armature at the coord world
positions and one mesh per model (UV `v = 1 − v_file`, file normals, weights by coord name),
then conform → bake → class/position weight retarget onto `pl_rage00`. Trunk: linear z-map from
the thigh sockets (DDR UpLeg) to `neck` (DDR Neck), `S = kz` (≈ 0.0078–0.0092 m/cm: the
195 cm Jotaro becomes a 1.7 m DDR male with his proportions). Chains: spine → spine1 → spine2 →
neck → head; clavicle → upperarm → forearm → hand (terminal, follows the forearm); thigh (source
socket width) → calf → foot → toe0. Soles are stretched below the ankle to the stock 0.012 m.
Everything from the jaw up is forced rigid on Head (README "Keep the head rigid"). Classes:
twist helpers with their limb, `cloth*` → `skirt`, collar / chain / muffler / mant → `spine`,
`heel` → `foot`, face / hair / cap → `head`. Dio's muffler is authored streaming 90 cm straight
back in the T-pose; `HANG` poses the chain to drape down the back before the bake.

**Dynamic chains** (`DYN_CHAINS`): Jotaro's collar chain (`chain01..04`, root on `Neck`) and Dio's
muffler (`muffler01..04`, root on `Spine2`) become `DB_*` bones appended after the 33 (game +x along
the segment), posed to drape before the bake (that pose is the solver's rest), weighted from the
source chain groups, and described in `pl_<key>/dynbones.txt` for the DLL's Winx-hair solver
(`dynbones_logic.rs`; Winx's `units_per_metre`, stiffness 0.08–0.1, damping 0.25–0.3, ±1 rad limits,
a torso box on Spine2 / Spine the lower links are pushed out of). The exporter writes 37 bones.

Textures: the `_d` diffuse only (the game is unlit; `_n`, `_m`, `_s` dropped), stems
`eoh<code><sheet>` (`eoh3jtr01body01d`, `eoh3jtr01iris`, Dio's `eoh1dio01hair01d`, DIO's
`eoh3dio01eyelashes`). Source cull mode 0 exports two-sided. Checks: 33 bones, weight sums,
codec round trip, Workbench previews (rest, `mc_male_br01/02` frames, face / hand / feet
close-ups). ~2 s per character headless.

```bash
EOH_DIR=~/Desktop/EoH/cpk DDR_3D_DATA=$W/game/data DDR_3D_RLIST=$W/startup/data/chara/chara_resources.rlist \
OUT_BASE=$W/dancers CHARA=all PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
  --python tools/blender_ddr_addon/examples/port_character_eoh.py
# then copy <OUT_BASE>/<Label>/export -> data_mods/custom_models/dancers/JOJO EOH/<Label>
```

In-game (2026-10-24, maintainer): all six load and dance. Under BIG HEAD the eyes went black and
DIO's / Dio's faces read distorted — not the models: the outline hull pushed along the un-normalised
skinned normal, so the 3× Head bone tripled the ink on the head (fixed in `mdl_cel.hlsl`,
`.agents/learnings/learnings.md` 2026-10-25). Jotaro's chain stuck straight out → the dynamic chains above.

Open: colour sets `COL=1..3` (label `<Name> 2..4`, key `eoh<code>c<N>`) work but are not shipped;
Dio's `arm cool` alternate arms are skipped; the Stands' aura / outline shaders are not reproduced
(the modpack's own outlines apply).

## 5. All Star Battle R (PS4) — the stages

`absr-1.0.pkg` (3.93 GB, `EP0700-CUSA28972_00-JASBRMAINGAME000`) is a fake PKG whose embedded image
key does NOT decrypt its PFS (PkgTool: "inode 0 is corrupt"; the PFS image digest fails
`pkg_validate`). `pkg_extract --passcode 00000000000000000000000000000000` derives the key from the
passcode instead and extracts everything. Data: `uroot/data/launch/{data,stage,platform,sound,adx2,
movie}.cpk` + per-language `eng/…`; the same NUCC / NUD / NTP3 formats as EoH, read by
`scripts/xfbin_dump.py` unchanged.

Stages: `stage.cpk` `data/stage/<id>.xfbin` (14 arenas: `sd01a` Dio's Castle … `sd08a` Wall Eyes,
`sa03e/sa04b/sa05a/sa07a` the ASB-R additions) + `<id>_sf00.xfbin` (the "special finish" dressings,
skipped) and `platform.cpk` `data/stage/HIGH_SPEC/<id>.xfbin` (PS4 Pro versions of 7 stages, used
when present). A stage file is a flat list of clumps, each a small static coord tree (cm, Z-up)
with its models: `<id>_flo` floor + near props, `<id>_build00..03`, `<id>_tree00`, `<id>_plant00`,
`<id>_trans` (vehicles), `<id>amobplaneNN` (crowd billboards), `<id>bac` (the skydome, r ≈ 600 m,
inward-wound), `<id>hit` (collision + the spawn markers `dmystart_1p/2p` at (∓10, ±100) facing each
other along Y, so the fighting camera looks along X), `*_eff_*` / `*gimmick*` (effects, stage
hazards) and `*bod1` (NPC bodies). Materials: NUD slot 0 the diffuse (`_t01`, `_d`), slot 1 a
`*_shadowsmap` lightmap on UV set 1 (buildings, floor) or a leaf cut-out on UV set 1 over a flat
tint (vegetation); `_nm` normal maps; blend ONE/ZERO opaque, SRC_ALPHA translucent, dst ONE additive.
Many sheets are 2048² (a few 4096×1024) with NO mip chain.

**Port** (`tools/blender_ddr_addon/examples/port_stage_asbr.py`, 2026-10-25; ships as
`data_mods/custom_models/stages/JOJOS BZRRE ADV/<Label>`, keys `jj<id>`, 15 stages): every
model of every kept clump is baked through its coord chain, scaled 0.007 (0.7 × true size; 1:1 and
0.85 both read too big in-game) and rotated so the fighting
camera's side (`FACING`, default the −X side) faces the DDR camera, then joined per clump into the
`bg` (skydome, `:-2`), `dec` (opaque), `add`, `ble` (`:-1`) parts with the stock flag conventions
(`port_stage_supernova.py`'s). Textures: the diffuse only (the lightmap and normal map are dropped —
World is unlit); vegetation takes the cut-out sheet + UV set 1 when sheet 0 is a flat tint
(`Sheets.transparency`); the skydome keeps its gradient, not the cloud overlay. Sheets over 1024 or
without mips go through `xfbin_dump.dxt_chain` — an exact DXT1/5 block decode, 2×2 texel average,
range-fit re-encode, in block space, which also builds the missing mip chain (≈5 s per 2048² sheet
in pure Python; visually lossless at background distance). Vertex colours and NUD cull (0 →
two-sided) are kept; the picked sheet's material UV transform becomes World's m_vTexAnime (params
0..3 = 1 / scale, offset), baked into the uvs only on a clamped sky axis.

**Animation** (2026-10-25, `ANIMATE=1`; the Anm table in §3): the stage's looping clips minus the
gimmick / cut-in / hazard / route / intro ones. Material UV offsets → the part's `_play_loop.sanm`
(params 2 / 3; a flip-book axis — sub-tile scale or quarter-tile jumps — holds each source frame with
equal-time key pairs, a scroll is unwrapped and interpolated); animated coords → a flat rig (`root`
with the game-identity bind + one bone per animated anchor, skinned meshes weighted by coord) and
`_play_loop.anm` (q48 rotation + float translation + relative scale, keys every 2nd World frame,
decimated, checked with `anm_dump.evaluate_pose` against the source worlds: ≤ 1e-3 rotation, ≤ 1e-5
relative translation). A part's clip is the lcm of its periods (capped at 6 min). Env-mapped water
(cube + normal + height sheets) shows its normal map shaded under a fixed light into a tint sheet
(`water_sheet`), scrolled with the source's first normal layer. Dio's Castle's crowd billboards and
flame quads and Battlefield's torches come back (`KEEP`); Battlefield's chariot laps the rim in three
`lap` parts (63 animated anchors per part). Cameras: borrowed (stock set, or Griffin House's for
`sd04b` / `sd08a`, eyes scaled with the stage, plus generated `_non` cut-aways — below).

In-game round 2 (2026-10-26) and what changed:
- **Clouds raced**: sky / cloud clips (coord or material targets of a sky clump, not sea / water)
  play at `SKY_SPEED` = 0.1 and go into their own part `sky_bg` (`:-2`; the name keeps the `_bg`
  hull / restyle exemption), since a 10× clip would explode the lcm with the stage's other loops.
- **Battlefield's sky band**: its dome + cloud rings live in `sd02a01`, not a `bac`/`sky` clump, so
  it was `dec` and its gradient (v to 1.2, sampled WRAP) folded over; `BG_EXTRA` makes it `bg`
  (the clamp applies).
- **Philly Coast's beach gaps**: `sd07aeff_sea00` poses the near-shore sea bones 0.2–1.3 m below the
  skinned shore mesh's bind (rest coords), opening the beach / water seam; its coord curves are not
  ported (`COORD_SKIP_CLIPS`), its UV scroll is.
- Parts with only a material loop now export static (`mdl_bg_*`, as stock `gm_boom00_bg` does with
  its sky `.sanm`; the `.sanm` binds by material identity) — only parts with animated anchors are
  rigged. (Done for Rocky Mountain Village's black tree boxes, which it did NOT fix: see round 3.)
- **Wall Eyes' scrolling rocks**: on an env-mapped material (slot 0 a cube map) the UV transforms
  and their scrolls belong to the normal-map layers (the wet sheen), not the diffuse; the diffuse
  now keeps identity uvs (also Naples Station's wet road).
- **Camera cuts**: the director cuts to a `_non` shot at every dance-clip change (A3's rule); the
  borrowed Griffin House set had none, so the clip switches showed. `scripts/gen_cutaway_shots.py`
  derives 8 push-in cut-aways from the main shots (on the dancer → main-eye segment, so they stay in
  the room), also added to Griffin House itself.

Round 3 (2026-10-26):
- **Rocky Mountain Village's tree boxes, for real**: `xfbin_dump.dxt_halve` re-encoded every DXT1
  block in the opaque 4-colour mode, so the generated mip levels of a cut-out sheet had no
  transparent texels and drew black at distance (the full-size level was fine — previews at close
  range looked right). Blocks with any texel under alpha 128 now use the 3-colour + transparent
  mode. Every downsampled ASBR sheet was rebuilt; the shipped EoH dancers carry no affected sheet.
- **Wall Eyes' missing sky**: its borrowed Griffin House shots clip at 200 m and the sky dome sits at
  ~760 m (at 0.7 scale); `CAMERA_FAR` sets 1500 m on that set (`gen_cutaway_shots.py --far`). It was
  never visible on this stage.

Joseph Joestar's scarf (EoH `2jsp01`, 2026-10-26): its tail streams 0.75 m straight back from the
neck in the T-pose like Dio's muffler; with the rip deleted, the dynamic chain was added to the
SHIPPED model by `tools/blender_ddr_addon/examples/add_dynchain_dancer.py` (import the `.model`,
fit 4 joints along the strip's vertices, re-pose it down the back, append `DB_scarf_01..04` on
`Neck`, write `dynbones.txt`; the 33 DDR bones, materials and textures are checked unchanged).
Previews: `<key>_{front,front_far,back,side,top,persp}.png` in the work dir (EEVEE emission stand-ins
for the unlit shaders, so the UV transforms show; `persp` is the DDR camera's view).

Open: per-stage `FACING` where the ortho previews read backwards; `.camanm` sets from the stage's
`camera001` chunk (8 bytes — a reference; the real shots are in the `*_sf00` / `*cut_in` anm
chunks); visibility curves (traffic signals, blinking lights) and instanced effect coords are not
ported; the UV-offset sign follows the `uv * scale + offset` reading (flip-books and the fire sheets
agree with it, a scroll's direction is not verified in-game); the `_sf00` finish dressings as
alternate stages.
