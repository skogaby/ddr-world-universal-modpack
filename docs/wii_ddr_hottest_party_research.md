# Dancing Stage / DDR HOTTEST PARTY (Wii) Assets — RE Notes (2026-10-03)

**Question.** How are the assets of Dancing Stage HOTTEST PARTY (Wii, EU 2008, `RDDP`) stored?
The goal is to extract all of them exactly, then port the 3D dancers and stages into DDR World
as Background Dancers custom content. That is the same path as the PS2 / Xbox / System 573
ports (`docs/ps2_ddr_filedata_research.md`, `docs/dancing_stage_unleashed_dancers_port_feasibility.md`).

**Answer.**
- **The game runs on Hudson Soft's Mario Party engine**, and every container and model is a
  Hudson format. The Mario Party 4 decompilation (`mariopartyrd/marioparty4`) documents them:
  `hsfformat.h`, `hsfload.c`, `hsfmotion.c`, `EnvelopeExec.c`, `hsfdraw.c`, `decode.c`,
  `animdata.h` / `sprman.c`. Every layout below was checked against this disc's files.
- **`data/*.bin` are HuData packs** with exact member tables. File ids are `dir << 16 | index`,
  and `dir` indexes a 197-entry path table in `main.dol` (§2). All 7359 members of the 197 packs
  decode; zlib (7267 members) and Hudson LZ (92) are the only codecs used.
- **3D = HSFV037**: models, skinned envelopes, Hermite / linear / step curves (§4). All 2932 HSF
  members parse, skin at rest and sample.
- **40 dancers ported.** There are 8 characters × 4 costumes plus the female and male back-up
  dancer × 4, on ONE shared 26-joint rig with ONE choreography library. The library is 256
  clips joined into takes; the 34 dance takes of ≥ 3 bars (617.5 bars) are kept, retimed to
  World's 120 BPM dance clock (§5). They ship as `data_mods/custom_models/dancers/DDR HOTTST PRTY/`.
- **42 stages ported.** These are the 46 `data/stgNN` packs minus a test stub and its three copies
  (§6). Each brings its object loops, UV scrolls, material colour loops, its own camera shots (6;
  stg00 has 5) and the 59 shared dance close-ups. They ship as `data_mods/custom_models/stages/DDR HOTTST PRTY/`.
- **Everything else is extracted too** (§7): 4371 sprite animations (all UI art), 55 SSQ charts,
  the 215 MB sound archive (119 RWSD files, 490 DSP-ADPCM waves, every song), 67 BRSTM
  streams, the THP movies, the message banks as text, the HOME-menu U8 archives and the banner.
- Not yet cabinet-tested.

Tools: `scripts/extract_wii_ddr_data.py` (extractor), `scripts/hsf_dump.py` (HSF decoder +
World conversion math), `tools/blender_ddr_addon/examples/port_character_hottest.py`,
`port_stage_hottest.py`, `scripts/test_wii_ddr_formats.py` + `scripts/validate_wii_ddr_tools.sh`.
Ghidra: `ddr_hottest_party_main.dol` (Ghidra-GameCube-Loader; addresses below are virtual).

## 1. Disc layout

The data partition holds:

| Path | Contents |
|---|---|
| `sys/main.dol` | the game (2.26 MB); the data directory table, song list, eye-texture table |
| `data/*.bin` | 197 HuData packs: models, motions, sprites, charts (§2–§6) |
| `dll/*.rel` + `*.str` | 14 REL modules (game modes) + their build path strings (`d:\project\wii\ddr-jp\prog\DLLS\...`) |
| `sound/ddr.brsar` | the sound archive, 215 MB: every song (one RWSD group per song) and the SE banks |
| `sound/stream/*.brstm` | 67 streams: the `mu_sam_*` song previews and the `mu_bgm_*` menu music |
| `movie/*.thp` | 7 THP movies (the 121 MB ending) |
| `mess/messdata*.bin` | message banks, default + ENG / FRA / GER / ITA / SPA |
| `home/HomeButton2,3/` | the Wii HOME menu (U8 `.arc`, `.tpl`, csv) |
| `opening.bnr` | the IMET channel banner |

`extract_wii_ddr_data.py disc <game.wbfs|game.iso> <out>` dumps this tree from an image. It
decrypts the data partition with the Wii common key, and needs the `cryptography` package.
`extract` then works on the tree; it accepts a Dolphin / wit `DATA/files` nesting too.

## 2. HuData packs (`data/*.bin`)

```
u32 count, u32 offset[count]                 an entry ends where the next begins (the last at EOF)
entry: u32 raw_size, u32 codec, codec data   (decode.c HuDecodeData; DATA_DECODE_* in data.h)
  0 none   1 LZ   2 slide   3/4 fslide   5 RLE   7 zlib {u32 raw_size, u32 zlib_size, stream}
```
- **Codec 1 is Hudson LZ, not "stored".** The first survey misread it as stored. It is an
  8-flag LZSS: 1 = literal, a pair is `{lo, hi:2 | len:6}` + 3 into a 1 KiB ring starting at
  958. The 92 codec-1 entries are sprites and the 12-byte `c_000.bin` entry 0.
- **The directory table** is at `0x801C3D78`: pairs `{char *path, u32 handle}`, starting with
  `data/arrow.bin`, NULL-terminated, 197 entries. `FUN_80009e68` opens each one at boot. A file
  id `0x00920000 | k` is entry *k* of `data/stg00.bin` (dir 0x92). `dol/data_dirs.csv` lists
  them all.

Member kinds are recognized by structure:
- `.hsf` (`HSFV037\0`);
- `.spr` (ANIMDATA: `s16 banks, patterns, bitmaps, use; u32 bank, pattern, bitmap offsets`);
- `.ssq` (little-endian DDR chart chunks, a tempo chunk first);
- `.bin` (one member: `boot.bin`).

| Packs | Contents |
|---|---|
| `c_001..008`, `c_011..018`, `c_021..028`, `c_031..038` | dancer *n* (1..8) in costume *k* (`c_0<k-1><n>`): entry 0 the HSF model, entries 1..3 three 64² RGBA8 eye-expression sprites (AFRO: none) |
| `c_101/102 .. c_131/132` | the female / male back-up dancer, costumes 1..4 |
| `c_000.bin` | the choreography library: entry 0 a 12-byte header, 1..256 the dance clips, 257..294 hold poses (1–2 frames) |
| `c_000_01..55.bin` | one bundle per song (`c_000_<song>`): entry 0 the **SSQ chart**, 1 / 2 the song's two camera takes (full song length, `cameraShape1` / `cameraShape2`), 3.. the dance clips it uses — **byte-identical copies** of `c_000.bin` entries (1119 of 1119) |
| `c_000_0000.bin` | 40 empty motions |
| `stg00..50.bin` (45) | the stages (§6) |
| `ddrcam.bin` | 59 generic dance cameras (180 frames) + one empty entry |
| `chrselmdl.bin`, `danceview.bin`, `camtest.bin` | the character-select clips, the dance-viewer floor, a developer camera test |
| everything else | 2D UI: sprite animations, per language (`*_fr/_ge/_it/_sp`) |

## 3. Sprites, textures, charts

- **Sprites (`sprman.c` HuSprAnimRead).**
  - The header is followed by banks `{s16 frames, s16, u32 frame_ofs}`, frames
    `{s16 pattern, time, shift_x, shift_y, flip, pad}`, patterns
    `{s16 layers, cx, cy, w, h, pad, u32 layer_ofs}`, layers (0x20:
    `u8 alpha, flip, s16 bitmap, x, y, w, h, shift_x, shift_y, s16 vtx[8]`) and bitmaps (0x14:
    `u8 pix_size, u8 format, s16 palette_count, w, h, u32 size, palette_ofs, data_ofs`).
  - Format (`& 0xF`): 0 RGBA8, 1/2 RGB5A3, 3 C8, 4 C4, 5 IA8, 6 IA4, 7 I8, 8 I4, 9 A8, 10 CMPR.
  - The C8 / C4 palettes are RGB5A3 (`sprput.c`).
  - Extracted as `.spr` + `.spr.json` (banks / patterns / layers) + one PNG per bitmap.
- **GX textures.** These are the standard Dolphin decoders, vectorized in numpy: I4, I8, IA4,
  IA8, RGB565, RGB5A3, RGBA8 (AR / GB half-blocks), C4 / C8 / C14X2 with an IA8 / RGB565 /
  RGB5A3 TLUT, and CMPR (2 × 2 big-endian DXT1 sub-blocks). The test suite checks each one.
- **SSQ.** Each song's `c_000_NN` entry 0 is a DDR SSQ chart in the arcade format
  (`docs/ssq_format.md`):
  - a tempo chunk with a 150 tick base, an event chunk, step chunks;
  - type-9 chunks with param `0x114 .. 0x414`. These are per-difficulty marker charts: times in
    4096-per-bar units and 1 / 2 / 4 / 8 / 9 lane bytes, most likely the Wii Remote **hand
    markers**. They are not the choreography.
- **The song list** is at `0x80201F78`, 16-byte records
  `{u16 category, u16 song, u16, u16, u16 bpm_lo, u16 bpm_hi, char *title}`: 55 songs, song n =
  `data/c_000_n.bin`. It is in `dol/songs.csv`.

## 4. HSFV037

The full field layout is in `scripts/hsf_dump.py`'s docstring. In short:
- A header of 21 `{offset, count}` sections. The string section's count is its byte size; names
  are byte offsets into it. List fields point into a u32 symbol array.
- Objects are 0x144 bytes, with
  `type 0 null / 2 mesh / 3 root / 4 joint / 5 null / 7 camera`, a parent index and children
  (symbol list), and base T / R / S. R is Euler degrees, `M = T · Rz · Ry · Rx · S` on column
  vectors.
- Meshes index separate position / normal / colour / st buffers per face corner. Faces are 0x30
  bytes; strips keep their extra corners after the last face buffer.
  - GX order: a triangle is corners (0, 2, 1), a quad (0, 2, 3, 1).
  - Normals are f32 in a skinned file and s8/64 otherwise.
  - The GX order is mirrored from the normals: 2140 of EMI's 2142 triangles flip under
    `consistent_winding`.
- Materials (0x3C) carry `vtxMode` (5 = vertex colours), `litColor`, `flags`
  (bit 1 NOCULL, bit 4 ADDCOL, bit 5 INVCOL) and `pass` (low nibble ≠ 0 → the translucent pass),
  plus attribute ids. Attributes (0x84) carry the bitmap and the UV transform. Bitmaps (0x20)
  are GX textures with a palette index.
- **Envelopes (cenv).** Single = posNum vertices on one target. Dual = `w` · target1 +
  (1 − w) · target2 per weight run. Multi = n `{target, w}`. A vertex lands at
  `Σ w_j C_j W_j⁻¹ W_mesh v` (C current, W rest world; `EnvelopeExec.c`).
- **Motion tracks** (0x10): `u8 type, u8 start, u16 target (string offset, -1 for attribute
  tracks), u16 index, u16 channel, u16 curve, u16 nkeys, u32 data`.
  - Object channels are 8/9/10 T, 28/29/30 R, 31/32/33 S.
  - Camera channels are 8/9/10 position, 11/12/13 aim point, 14 roll (deg), 15 FOV (vertical,
    deg), 17 / 18 near / far.
  - Attribute channels 8 / 9 = UV T (the texture matrix translates by −T).
  - Material channels 0/1/2 = litColor.
- **Key search gotcha.** MayaConverter writes **pre-roll keys at negative times after key 0**,
  so the key arrays are not sorted. An example from a stg23 UV track: times
  `0, -84, 12, 112, …`. The game scans for the first key with `t < time` (step / linear) or the
  first segment with `k[i-1] ≤ t < k[i]` (Hermite); it never bisects. `hsf_dump.sample_curve`
  does the same. A bisecting evaluator gets these tracks wrong.
- Hermite keys are `{t, v, out, in}`, with
  `h(s) = v0(2s³−3s²+1) + v1(3s²−2s³) + out0(s³−2s²+s) + in1(s³−s²)`. The slopes are **not**
  scaled by the segment length (`GetBezier`).

## 5. The dancers

| Files | Model | Port label (key `hp<stem><costume>`) | Sex |
|---|---|---|---|
| `c_0k1` | `EMI` (+ skirt mesh) | Emi 1..4 (`hpemi01..04`) | F |
| `c_0k2` | `JENNY01` | Jenny 1..4 | F |
| `c_0k3` | `afro01` (+ accessory, muffler) | Afro 1..4 | M |
| `c_0k4` | `rage01` | Rage 1..4 | M |
| `c_0k5` | `hispanic` | Dancer A 1..4 | F |
| `c_0k6` | `black_f` | Dancer B 1..4 | F |
| `c_0k7` | `korea_m` | Dancer C 1..4 | M |
| `c_0k8` | `jamaika` | Dancer D 1..4 | M |
| `c_1k1` | `dancer_f01` | Backup F 1..4 | F |
| `c_1k2` | `dancer_m` | Backup M 1..4 | M |

**Names.** The four new characters get neutral placeholder labels. The EU message bank's
outfit-unlock messages (`[6.11] .. [6.74]`, one per venue) name the characters Harmony, Root,
Gaku, Rena, Domi, Danca, Chordia and U.G. (the last one with "you can choose any back-up
dancer"). Nothing on the disc found so far links those names to a model:
- no string table;
- no name-plate sprite (character select shows portraits only);
- the eye-texture table at `0x80200F8C` lists the slots in the model order above.

The venue → outfit unlock tables (a stride-36 run `11, 20, …, 74` in `danceviewDll.rel` 0x210A)
are the lead if the names are wanted.

**Rig.**
- All 40 models share one skeleton, with bit-identical joint positions: 26 type-4 joints from
  `Hips` (133.4 units up) through `Head*end`, `*Wrist*end` and `*Toe*end`.
- MayaConverter wraps each joint in `<J>*root` / `<J>*leaf` helper objects, which are identity
  and unanimated; `hsf_dump.rig_joints` drops them. The file lists children before parents.
- The file frame is already World's (Y up, facing +Z, left at +X). One uniform scale,
  `GAME_SCALE = 0.970 / 133.4`, puts the Hips at 0.97 m; EMI is then 1.73 m tall.
- The meshes have up to 5 influences (the exporter keeps the 4 largest), a 512² body sheet and a
  128 × 64 eye sheet.
- The game swaps the eye material's texture for the three 64² blink sprites
  (`tex*<name>*eye01.tga` → `S3c00Xm*_eye`). The embedded sheet is the open-eyes one, so the
  port uses it as is. DANCER A's eye sheet is 129 × 64 and is resampled to 128.

**Choreography.**
- Every character dances the same library. A song's bundle lists the clips it uses (for example,
  song 01 uses library entries 5, 27–34, 61–68, 193), and the selection code (`FUN_800665f8` /
  `FUN_8004cce8`) requests motions by library id (`0x0006xxxx`), reusing the loaded bundle's
  copy.
- **Clip lengths in bars.** `danceviewDll.rel` (the dance viewer mode) holds the library ids
  `0x00060001..0x00060100`, then one u32 per clip in SSQ bar units (0x1000 = one bar).
  `extract_wii_ddr_data.danceview_clip_bars` reads it, and `dol/dance_clip_bars.csv` lists it.
- Frames ÷ bars gives the authored tempo. The library mixes 120 frames/bar (120 BPM), ~99.3
  (145 BPM), ~81.3 (177 BPM), ~100 and ~205 (70 BPM) takes. 6 of the 256 table values are off
  their take's tempo by more than 5 % (e.g. #1, 120 frames listed as 3 bars); the port re-derives
  those from the take's median.
- **Takes.** MayaConverter cut continuous takes into pieces whose last pose is the next piece's
  first pose (< 1 unit on every joint). `hsf_dump.chain_clips` joins them back:
  - 55 takes in all;
  - the 36 of ≥ 3 bars (754.5 bars) are candidates. World cuts every clip 1.5 s before its end,
    so a 1- or 2-bar piece would barely show;
  - the two longest of them (entries 225–256, 137 bars) are used ONLY by song 27, "Lesson by
    DJ" (category 14 in the song table), the step tutorial. They are demonstrations, a dancer
    stepping on the arrows, and are left out (`lesson_only_clips`), which leaves 34 takes and
    617.5 bars.
- **Retime.** Source time = output frame × (frames per bar ÷ 120), so one bar lasts 120 frames
  of World's dance clock. That clock runs at chart BPM / 120 under `bpm_sync`.
  - Keys land every 2nd frame (World slerps between them).
  - The Hips x/z path is re-centred on the mark (`ROOT_MODE=recentre`), because the takes
    travel up to ±1.5 m.
  - Every clip is checked against the HSF pose: < 0.1 mm per joint.
- **Dealing.** The full library is ~6.9 MB of `.anm` per dancer (~275 MB for 40), so it is dealt
  across each character's four costumes, longest take first onto the costume with the fewest
  bars. Each costume carries ~154 bars in 8–9 takes, ~1.7 MB, and the four together dance all
  of it.
- `Spine2` is aliased to `Spine1` in the body `.b2it` (World's BIG HEAD / attach role bones);
  Hips, Head and the toes are named alike.

## 6. The stages

**Pack layout.** A stage pack is a list of `(model, motion)` pairs, then the dancers' formation
markers, then six camera motions (`cameraShape1` + `camera1*aim`, 180 frames):
- the backdrop `BG<NN>` (sky / walls, often UV-scrolled);
- the floor `stgNN_01`, `_02K`, …;
- the props `objNN_A_01..`, each a null tree over static meshes, each with its own loop of
  180..6000 frames;
- the markers: `chr*<formation>*<dancer>` / `look*` meshes with an 8 × 8 `none` texture.

**The test stub.** `stg05` and its byte-identical copies `stg10/15/20` are a test stub
(`BG00`, one floor piece, a `dammy` camera) and are not ported. Several stages are remixes of
others' pieces, e.g. `stg33` (01 + 02 + 11), `stg38`, `stg43`, `stg44`, `stg48` and `stg49`.
They are ported as they are, each a different composition in the game. The EU message bank names
17 unlockable stages (Ampliture, Blue Modus, Conceptia, …, Sunshine), but there is no
file → name map, so the labels are `Stage NN` (keys `hpstageNN`).

**Colour.**
- The stage textures are mostly white alpha masks (`A*…`, CI8 or RGBA8). The colour comes from
  the **vertex colours** (`vtxMode` 5 on 95 % of the materials), which is exactly World's
  `_vc` shader (texture × COLOR0). A Workbench preview shows them white, so
  `port_stage_hottest.preview` rewires the materials to emit texture × vertex colour.
- The COLOR0 bytes must be the GX bytes as is. The port first wrote them through Blender's
  linear `color` accessor, which sRGB-encodes (a 0.5 shipped as 188). The port now writes
  `color_srgb`, and all 42 stages were re-ported with it on 2026-10-04 (only the `.model` files changed:
  142 of 174; the other 32 carry white / black only). See `docs/wii_ddr_hottest_party_2_3_research.md` §7.2.
- `litColor` is the GX channel's ambient register (`GXSetChanAmbColor`). It is left out of the
  static colour; the 30 material tracks that animate it go onto `vConstantColor`.

**Blend groups** (`hsf_dump.material_kind`, refined in the port):

| Condition | Group | World flags |
|---|---|---|
| ADDCOL | `add` | 0x06C1 + 4 |
| INVCOL | `sub` | 0x06C1 + 8 |
| translucent pass (`pass & 0xF` or `invAlpha`) with real partial alpha (texture or vertex) | `ble` (prio −1) | 0x02C1 |
| everything else | `dec`, alpha-tested; the first model's opaque meshes become `bg` (prio −2, World's `_bg` backdrop rules) | 0x0001 |

The table's flags are the two-sided ones; a culled mesh ships them without 0x0001.

**Culling** (fixed 2026-10-05; until then every mesh was exported two-sided). The HSF draw
(`FUN_8006a3a8`) takes `object flags | material flags`: bit 1 (NOCULL) → `GXSetCullMode(0)`
(`FUN_8009709c`), else back-face culling (the model attribute 0x800000 and a global mirror flag
swap front / back). NOCULL is the exception, not the default: 23,478 of 251,341 stage
triangles, all from material flags (no object sets it; stg04, 07, 13, 21, 24 … have none). The
visible side is the reverse of the GX corner order, which agrees with the normals on 99.5 % of
the culled triangles. stg04's fans (`obj04*A*01*Sensu*`) are 361 pairs of single-sided
triangles on the same three positions facing opposite ways; drawn two-sided, World z-fought
them (the fan "flicker"). The port now exports a NOCULL mesh two-sided and every other one
single-sided in the reversed GX order (`hsf_dump.cull_winding`); a mesh under a mirroring world
stays two-sided.

**Parts.** World allows 32 frame-board instances (16 for a P2 preview) and 64 bones per
instance, so parts must stay few and small:
- models group by loop length (a model joins a group whose length its own divides; it is
  sampled at `t mod L`);
- then by blend group;
- then split at 63 animated anchors.

Most stages come out as 3–5 parts; stg03 (seven loop lengths) and stg07 come out as 8–9.

**Rig per part.** `root` plus one flat bone per animated anchor, which is a mesh's deepest
animated ancestor:
- the static chain below the anchor is baked;
- a constant track re-poses its object for good, so the geometry is baked at the frame-0 pose;
- `_play_loop.anm` carries q / t / scale-relative-to-rest every 2nd frame plus a wrap key.

**Material loops.** `_play_loop.sanm` carries params 2 / 3 = −(attribute T x / y), unwrapped
across the repeats of a shorter loop, and 4..6 = litColor (`mdl_ch_constant_c_vc`). One
attribute rotZ track (stg17) is not representable and is skipped with a log line.
Each part stays within World's 48 animated material params.

**Cameras.**
- The stage's shots become `<key>_st01..06`, and the 59 camera motions of `ddrcam.bin` (60
  entries, one without a camera) become `_non01..59` (main rotation + close-ups).
- Position + aim + roll are converted with `tzm_dump.look_at_rows` in cm.
- The FOV is vertical (MTXPerspective). The port keeps its vertical extent on World's 16:9:
  `half_tan_h = tan(fov/2) · 16/9 → tzm_dump.world_camanm_fov`.
- Every camera is checked: < 1e-6 m error.

The per-song camera takes (the `c_000_NN` entries 1 / 2, whole-song choreographed cameras) are
extracted but not ported. They belong to a song, not a stage.

## 7. Everything else

- **`sound/ddr.brsar`** (RSAR 1.3). It has 552 sounds (492 WAVE, 60 STRM = the external
  `.brstm`), 179 files and 120 groups. Each song is a group `GR_DDR_<song>` of one RWSD whose
  one stereo 32 kHz DSP-ADPCM wave sits in the group's wave block.
  - RWSD WAVE entries are `{u8 format, loop, channels, rate_hi, u16 rate, u16, u32 loop_start,
    loop_end (nibbles), channel table, data location}`, channel info `{data offset, adpcm offset}`
    and the standard DSP coefficient block.
  - The extractor writes each group's files, `sounds.csv` (sound → file, type, player) and, with
    `--wav`, every wave.
  - **DSP-ADPCM in pure Python is fast enough** (≈ 1.6 s per 2-minute stereo song), so no ffmpeg
    is needed. The output matches ffmpeg's `adpcm_thp` within ±14 LSB; ffmpeg skips the SDK's
    +1024 rounding.
- **`.brstm`** RSTM 1.0: HEAD (stream info, channel ADPCM info), DATA interleaved blocks
  (the last block padded); decoded the same way.
- **Messages.**
  - The layout is `u32 groups, u32 group_ofs[]` (relative to offset 4), and per group
    `u32 count, u32 msg_ofs[]` (relative to the group + 4). A message is `{u16, u16}` + text.
  - In the text, 0x10 = space and 0x0A = newline. The other control bytes (`0x85` …) are written
    as `{xx}`; groups 1 / 3 hold the song credits and the SSQ names.
- **THP** movies are copied; `--mp4` converts them with ffmpeg.
- **U8** (`.arc`, the banner's IMD5 / LZ77-wrapped members) is unpacked recursively; TPLs inside
  become PNGs.
- **RELs** are copied. `danceviewDll.rel` supplies the clip bar table (§5).

## 8. Not done / open

- **The character display names** (§5).
- **The stage display names** (§6).
- The **hand-marker** SSQ chunks are decoded only far enough to know they are not the dance
  timeline.
- The per-song **camera takes**, the dancers' **blink** (the expression sprites) and the stage
  **lighting** (`litColor` as ambient, GX lights) are not ported. Neither are the
  `chrselmdl.bin` character-select clips, which use the same rig and could join the library.
- Attribute rotation tracks are skipped (§6).

## 9. Reproduce

```bash
G=~/Desktop/"DDR Wii ISOs/Dancing Stage - Hottest Party (Europe)"   # the unpacked disc
# (or: scripts/extract_wii_ddr_data.py disc game.wbfs "$G")
scripts/extract_wii_ddr_data.py extract "$G" ~/Desktop/"DDR Wii ISOs"/hottest_party_extracted --png --wav   # ~2 min
scripts/validate_wii_ddr_tools.sh ~/Desktop/"DDR Wii ISOs"/hottest_party_extracted
DANCERS=all /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
  --python tools/blender_ddr_addon/examples/port_character_hottest.py   # ~20 s per dancer, ~14 min
STAGES=all /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
  --python tools/blender_ddr_addon/examples/port_stage_hottest.py       # ~1 min per stage
```
Both port scripts write straight into `data_mods/custom_models/{dancers,stages}/DDR HOTTST PRTY/`.
