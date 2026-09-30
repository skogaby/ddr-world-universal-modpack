# System 573 DDR Dancers, Movies and Flash Filesystem — RE Notes (2026-09-29)

**Question.** What 3D and video assets do the Konami System 573 mixes of DDR contain, how are
they stored, and can the polygon background dancers be extracted faithfully (mesh, textures,
faces/hands, dance animation) as a basis for a port?

**Answer.**
- **3rdMIX PLUS, 4thMIX PLUS and 5thMIX draw real-time polygon dancers.** Each is a rigid
  16-joint model of ~450–600 triangles plus one PSX texture page. All three mixes share the
  same 18 dance routines. They are fully decoded, and every model exports with every routine
  as an animation. Hand shapes and faces swap per frame, as in the game. 4thMIX PLUS has two
  characters (`onna_d`, `yuni`) that no other dumper names.
- **MAX, MAX 2 and EXTREME use pre-rendered video.** Their dancers and backgrounds are `.SBS`
  clips: headerless PSX MDEC frames, 304×176, 80 frames each, decoded here. There is no
  character geometry in those mixes.
- **The PS2's DDR STRIKE carries this engine.** It has 45 dancers in the `.cmd` / `chara.lst` /
  `chara.pos` layouts and the 5thMIX routine set, in a PS2 motion key-block layout that the
  decoder also reads. See docs/ps2_ddr_filedata_research.md §4.

**Tools** (all host-only Python and all self-contained; nothing is vendored):
- `tools/blender_ddr_addon/examples/port_character_sys573.py`: the conversion to a DDR World
  custom dancer (§7).
- `scripts/extract_sys573_data.py`: the flash filesystem, name recovery, and the SuperDisc
  table (§1).
- `scripts/sys573_dancer_dump.py`: models, textures, motion, OBJ/glTF export, previews and
  videos (§2–§4).
- `scripts/sys573_video.py`: `.SBS` → MP4 / PNG (§5).
- `scripts/test_sys573_formats.py` holds the host tests. `scripts/validate_sys573_tools.sh
  [mix-dir]...` runs them and can also survey extracted mixes.

**Sources.**
- A SuperDisc rip that ships `IN/{3P,4P,5,6,8}{G,C}` and `IN/8GP`.
- Ghidra (PSX loader) on `s573/aout.exe` of 3rdMIX PLUS and `soft/s573/aout.exe` of 5thMIX.
- **Address convention:** PS-X EXE virtual addresses as Ghidra shows them. Both executables
  load at `0x80010000`; the file offset is `VA - 0x80010000 + 0x800`. All addresses are
  3rdMIX PLUS unless marked (5th).

## 0. What to run

```bash
S=scripts
$S/extract_sys573_data.py superdisc <disc>                       # mixes -> IN/ images
$S/extract_sys573_data.py extract --game <disc>/IN/3PG --card <disc>/IN/3PC --out ~/573/3rd
$S/extract_sys573_data.py extract --game <disc>/IN/5G --card <disc>/IN/5C --out ~/573/5th \
    --names ~/573/3rd/_manifest.json --names ~/573/4th/_manifest.json   # older names help
$S/sys573_dancer_dump.py info ~/573/3rd/data/chara afro
$S/sys573_dancer_dump.py glb  ~/573/3rd/data/chara afro ~/573/3rd/data/motion afro.glb --bpm 130
$S/sys573_dancer_dump.py preview ~/573/5th/data/chara qp qp.png --motion ~/573/5th/data/motion/jazz1/jazz1.cmm --routine jazz1
$S/sys573_video.py mp4 out/ <disc>/6TH/*.SBS
```

## 1. Flash filesystem

A mix is installed as `GAME.DAT` (16 MiB on-board flash) plus `CARD.DAT` (32 MiB PCMCIA). On a
SuperDisc these are `IN/<id>G` / `IN/<id>C`. The installer's `PSX.EXE` holds the mapping as
`{label*, game image*, card image*}` rows at `0x80301204`:

| Mix | Images |
|---|---|
| 3rdMIX Ver.KOREA2 | 3KG / 3KC |
| 3rdMIX PLUS | 3PG / 3PC |
| 4thMIX PLUS | 4PG / 4PC |
| 5thMIX | 5G / 5C |
| MAX | 6G / 6C |
| MAX 2 | 7G / 7C |
| EXTREME | 8G / 8C |
| EXTREME PRO | 8GP / 8C |
| EuroMIX | E1 |
| EuroMIX 2 | E2G / E2C |
| Disney's Rave | DR |
| DCT | DCT |
| SOLO 2000 | S2K |

The mixes above with no card image are listed without one. `superdisc` prints the table
and flags the images a disc lacks. The root `D00xx.DAT` / `DATAxxJC.BIN` files of a
SuperDisc are not referenced by the installer or by any game executable examined.

- **File table:** at `GAME.DAT + 0xFE4000`, 16-byte entries
  `{u32 name_hash, u16 offset/0x800, u16 location (0 game, 1 card), u8 lz, u8 encrypted,
  u16 ?, u32 stored size}`. The table ends at `0xFFFFFFFF/0xFFFF`.
- **Name hash:** a 32-bit LFSR (poly `0x04C11DB7`, zero start). It consumes the low 6 bits of
  each character, LSB first.
  - Two consequences: `'0'..'9'` alias `'p'..'y'`, and case is lost (`qp` ≡ `q0`).
  - The hash is linear over GF(2). That gives the name solver below.
- **LZ:** a control byte gives 8 flags; a clear flag means a literal.
  - Set-flag tokens:
    - `0x00–0x7F`: 2-byte copy, distance `(b&3)<<8|next`, length `(b>>2)+3`.
    - `0x80–0xBF`: near copy, distance `(b&15)+1`, length `(b>>4)-6`.
    - `0xC0–0xFE`: a literal run of `b-0xB8` bytes.
    - `0xFF`: end.
  - `decode_lz` raises on a malformed stream, which the key search uses as its test.
- **Encryption:** only `mdb.bin`, `mp3_tab.bin` and `course/onimode.bin` are encrypted. The
  stream is `data[i] ^= (key*0x41C64E6D + i*0x3039) >> 5`.
  - `key` is one byte: `0x3A` for 5thMIX and MAX, `0x99` for EXTREME.
  - The tool brute-forces all 256 keys per file and keeps the key whose output LZ-decodes
    cleanly to the end, so no key table is needed.
- `boot/config.dat` is XORed with `(crc32_msb("/s573/config.dat") >> 8) & 0xFF`. Its
  `conversion` lines list paths.

**Name recovery** runs in rounds until nothing new is found:
1. Fixed boot names.
2. Literal paths in decoded files.
3. Numbered and region siblings of found names (`cos01_ta` → `cos02_ta`; `lang/japa` →
   `lang/engl`; `_bk` ↔ `_nm`…).
4. Song ids (every lowercase 3–6 character token of `*mdb.bin`, and code tokens) × the
   `data/mdb/<id>/…` layouts.
5. Directory prefixes and path stems in code × code identifiers.
6. **The layout solver.** Each layout shared by ≥ 3 named directories
   (`data/anime/{X}/{X}.anm`) is solved as a linear system for every unnamed hash and
   X length ≤ 5.
   - A solution is kept only if one of these holds:
     - X is a known token;
     - X fits layouts with different tail lengths (equal-length tails are XOR-dependent,
       so they give no evidence);
     - every named sibling has the same length and digit/letter style.
   - Solved rows are flagged in `_manifest.json`, because their spelling can alias.
7. `--names` adds candidates from a text list or another mix's `_manifest.json`. Later mixes
   still carry files of songs they no longer list.

Coverage, by image (entries named / total):

| Image | Named / total | Notes |
|---|---|---|
| 3PG | 972 / 972 | |
| 4PG | 892 / 970 | |
| 5G | 967 / 967 | with `--names` from 3rd/4th/EXTREME; 834 alone |
| 6G (MAX) | 420 / 725 | the unnamed rest are small card-side files, 3–21 KB (song banners and charts under names none of the sources give) |
| 8G | 1654 / 1656 | |

Names were compared with the SaxxonPike/windyfairy `sys573tools` dumper. Every file both tools
name has the same name, except one aliased pair in 4th: `data/mcard/*/page3.bin` comes out
here as `pages.bin`, the same hash. This tool names more entries than that dumper in
3rd/4th/5th: the dumper cannot name any `data/chara` model.

## 2. Characters (3rdMIX PLUS / 4thMIX PLUS / 5thMIX)

**Loader** `FUN_8003e3f0(player, chara)`; 5th `FUN_800447b0`.
- It reads `data/chara/<n>/<n>.cmd` and `data/chara/chara.lst` (or `inst/inst.tmd` + `inst.lst`
  for the instructor, `chara < 0`).
- It links one `GsDOBJ` per object to the joint coordinate `chara.lst[i].joint`.
- It records `first[joint]` = the lowest object index per joint, in the player struct at
  `+0x5E4 + 0x258`, and stores the object count in `first[16]`.
- The instructor models are standard Sony TMD (`0x41`) with 22 objects on their own
  `inst.lst` / `inst.pos` rig. They are out of scope here.

**`.cmd`:**
- Header: `u32 0, 0, nobj (=28), ?, 0, 0, ?, ?`.
- Object table at `+0x20`, `nobj` entries of `{u32 offset-from-0x20, u32 nsub, u32 0x1000}`.
- Each object starts with `nsub` sub-mesh headers of 12 u32 (0x30 bytes each). Offsets are
  relative to the object:

| Word | Meaning |
|---|---|
| 0 | kind: `0x34` textured Gouraud triangles, `0x30` (+`0x400`) flat colour |
| 1 | vertex offset (int16 xyz + pad) |
| 2 | vertex count |
| 3/4, 6–8 | normal / vertex counts (repeated) |
| 5 | normal offset (int16 xyz + pad, 4096 = 1.0) |
| 9 | index offset: per triangle 3 × `(u16 vertex, u16 normal)` |
| 10 | textured: per triangle 3 × `(u8 u, u8 v, u16)`, the u16s being CBA, TSB, pad; flat: one RGB, `0x80` = 1.0 |
| 11 | triangle count |

- The loader re-patches every primitive's tpage/clut (`FUN_8003f228`), so the in-file CBA/TSB
  values are placeholders.
- Vertices are joint-local.
- **Winding:** triangles are clockwise seen from outside. The stored normals point along
  `−(v1−v0)×(v2−v0)` for > 99% of triangles in all three mixes (the rest are degenerate or
  flat-normalled slivers); reverse the order for a counter-clockwise target.
- Vertex normal indices can exceed the header counts (words 3/7): read normals up to
  max(index) + 1.

**Rig:**
- `chara.lst` is `u8 nobj`, then per object `(u8 joint, u8 parent joint)`.
- `chara.pos` is 17 × int16 xyz; entry `j+1` is joint `j`'s rest offset from its parent. The
  coordinate setup `FUN_8003bcfc` skips entry 0.
- The parent table is at `0x80013198` (18 ints): `[anchor, root, hips, …]`:

| Joint | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Name | hips | thigh L | shin L | foot L | thigh R | shin R | foot R | chest | upper arm L | forearm L | upper arm R | forearm R | neck | hand L | hand R | head |
| Parent | root | 0 | 1 | 2 | 0 | 4 | 5 | 0 | 7 | 8 | 7 | 10 | 7 | 9 | 11 | 12 |

- PSX space is Y-down. The model faces +Z.
- Joints 13, 14 and 15 own 5 objects each: five hand shapes per hand; for the head, the head
  itself (the first object) plus four faces.

**Draw** `FUN_800441ac`:
- For each joint `j`, it draws object `first[j] + sel[j]`. `sel` is clamped to that joint's
  objects; the per-joint `sel` bytes are at player `+0x5A0`.
- For `j == 15` it also draws `first[15]`, the head base.
- The evaluator writes `sel` (§3). The head's value gets `+1`, so a face is always drawn
  over the head.
- Selector 0 on the head draws the base object twice with no face. That is why a naive static
  export shows a blank face. The exporter's rest pose uses face 1.

**Textures:**
- **3rd/4th:** `<n>.ctx` holds 16 TIMs of 64×64 at 4 bpp; the loader `FUN_800414ac` uploads 12.
  - TIM `i` goes to VRAM `x = 0x200 + player*0x40 + (i/4)*16`, `y = (i%4)*64`. In the model's
    256×256 texture page that is the tile at `u = (i/4)*64`, `v = (i%4)*64` (column-major).
  - TIM `i`'s CLUT goes to row `0xF0 + i`. Each tile keeps its own palette.
  - CLUT entry `0x7C1F` becomes transparent and `0x0000` becomes `0x4000` (opaque black).
- **5th:** `<n>.cmt` is one 8 bpp 256×256 TIM (`GetTPage(1, …)`) with the plain PSX rule that
  texel 0 is transparent.

## 3. Motion (`data/motion/<routine>/<routine>.cmm`)

**Container** (`FUN_8003d720` relocates it in place):
- `u16 'S'`, `u16 n`, `u32 8`, then `n` × `{u32 name offset, u32 clip offset}`.
- The clip names are `<routine><letter>` (`hiphop1a` … `hiphop1n`), plus stand-alone clips
  (`normal_00`, `normal_f2`…). `inst.cmm` holds the instructor's clips.

**Clip** (`FUN_8003d678`):
- `u32 size`, `u16 ntracks (=17)`, `u16 last (=1920)`, `u32 → track table`.
- Track 0 is the root; track `1+j` is joint `j`.

**Track** (`FUN_8003d880`):
- `u8 index`, `u8 ?`, `u8 has_translation`, `u8 ?`, `u32 size`, `u32 nchan`, `u32 → channel
  table`.
- Channel: `u8 type`, then at `+8` the key-block offset.
- Types:
  - `0/1/2` = rx/ry/rz.
  - `6/7/8` = tx/ty/tz. Present on the root and hips; applied only when `has_translation` is
    set, otherwise `chara.pos` is used.
  - `10` = stepped selector. Present on hand L, hand R and head.

**Key block** (`FUN_8003c304`):
- Segment index = `t >> (u16[+2] + u8[+0xD])`.
- The segment offset is `u16` at `+0x16 + 4*index`.
- A segment is a run of `(u16 time, s16 value)` keys. The code scans forward to the first key
  with time ≥ `t` and interpolates linearly from the previous key, using C truncating
  division.
- Values are shifted right by `u16[+6]`.
- The selector reads the previous key's value without interpolating.

**Evaluation** `FUN_8003c420(clip, coord, joint, t)`:
- If `t > last`, then `t %= last+1`.
- Rotation:
  - `RotMatrixZYX(0, ry, rz)` builds `Rz·Ry`. `RotMatrixZYX` at `0x8003DBF0` is `Rz·Ry·Rx`;
    vx is zeroed first.
  - `RotMatrixX(rx)` then **left**-multiplies (`0x8003DE80` rewrites rows 1–2).
  - So `R = Rx·Rz·Ry` in column-vector convention. Angles are 4096 per turn.
- `coord.t` = the animated translation, or the rest offset.
- The routine returns the selector, with `+1` on the head. `FUN_8003c018` evaluates all 17
  tracks (and blends two clips when a cross-fade is active, `FUN_8003d3a0`).
- A `type 1` first channel selects an axis/twist mode (`VectorNormalSS` path). It does not
  occur in shipped data.

**Timing and sequencing:**
- The song's beat position (`0x800D68B4`) is in 4096 per measure. `FUN_800824f0` maps
  `phase & 0xFFF` onto clip time as `phase × last >> 12`, so **one clip = one measure** and
  plays faster at higher BPM.
- **Root travel.** At every measure change, `FUN_800824f0` copies the root's world matrix
  (player `+0x74`) into the anchor coordinate (`+0x04`). The next measure's root motion is
  therefore relative to where the last one ended.
  - Clips start at root ≈ identity. The exporter chains routines the same way
    (`routine_frames`).
  - `FUN_80051340` separately pulls two dancers apart when they get too close.
- **Phrase table.** One row per routine, of 5 pointers: `routine, phrase1..4`.

| Mix | Table address | Phrases |
|---|---|---|
| 3rdMIX PLUS | `0x80015010` | overlapping, e.g. hiphop1 `abcde, efghi, ijkln` |
| 4thMIX PLUS | `0x800155A0` | one phrase per routine: `abcdefghijklm` (hiphop1: `…ln`) |
| 5thMIX | `0x80015A28` | same as 4th |

- `FUN_80082178` expands each phrase into `(letter, next letter)` sequence entries at
  `0x80136798` (0x24 bytes each). At the end of a phrase the next phrase is chosen at random.
  When the previous entry's second letter differs from the new clip, the new clip cross-fades
  in over the first quarter measure.
- In a straight a→z run those letters match, so the routine plays unblended. That is what the
  exporter writes.
- The 18 routines, shared byte-identically by 3rd/4th/5th:

| Group | Routines |
|---|---|
| hip hop | hiphop1, hiphop2 |
| jazz | jazz1, jazz2 |
| house | mhouse1, thouse2, thouse3 |
| soul | soul1, soul2 |
| other | capoera1, hopping1, lock1, n31, sino_, y11, y31 |
| idle clips | normal (9 of them) |

  Each dance is 13 measures (thouse2 has 14 clips). The exe's motion list at `0x800BC2E8` also
  names `wave1` / `wave2`, which no image ships.

## 4. Export (`sys573_dancer_dump.py glb`)

- The glTF has a node rig: `root` → 16 joint nodes with rest translations, and one child mesh
  node per object.
- Rigid parts need no skinning. Alternates (hand shapes, faces) are separate nodes whose
  visibility is animated with STEP scale keys (0 = hidden).
- There is one glTF animation per routine and per stand-alone clip: 25 per character (16
  routines + 9 `normal_*` idles). Keys are
  rotation plus translation for every joint; the root travels.
- Timing is measure = 240/BPM seconds (`--bpm`, default 130), sampled at `--fps`.
- Space: `(x, y, z)_psx → (−x, −y, z)`, a proper rotation (Y-up, still facing +Z), at 1 unit =
  1 mm.
- Materials:
  - one texture material with nearest filtering and an alpha mask;
  - one unlit-style PBR material per PSX flat colour, sRGB→linear.
- Verified:
  - host tests;
  - the `survey` of all three mixes (49 models: 16 + 18 + 15; 223 clips per mix, every clip
    sampled);
  - software previews of rest and dance poses in 3rd (afro) and 5th (qp). Faces and hands
    change on the beat.
  - a headless Blender 5.2 glTF import of `afro.glb`: one action per animation and 28 mesh
    objects. A
    Workbench render of `hiphop1` frame 45 matches the software preview's pose.
- 5thMIX (and, in other dumps, 4thMIX PLUS) lists 3rdMIX characters (`janet`, `lady`, `onna`,
  `robo`, `yaro`) whose `.cmd` entries are not models: their bytes are code/other data. Use
  the 3rdMIX files. `survey` reports and skips them.

**Porting to World:** §7.

## 5. Movies (`.SBS`, MAX / MAX 2 / EXTREME)

- An `.SBS` is headerless: one PSX MDEC "BS" v2 frame per 8 KiB slot, 80 frames per clip.
  The disc's `MOV/` or `6TH/` folders hold the song/background clips, and the images'
  `data/movie/common|howto/*.sbs` hold the system clips.
- Frame header: `{u16 mdec words, u16 0x3800, u16 qscale, u16 2}`.
- Bitstream: 16-bit LE words read MSB-first. Each macroblock is Cr, Cb, Y0–Y3 at 8×8; blocks
  are stored column-major.
  - DC is a 10-bit signed value × 2.
  - AC codes are MPEG-1 table B.14 (EOB `10`, escape `000001` + 6-bit run + 10-bit level),
    dequantised as `(|level| × q × qscale + 4) >> 3`.
- Every clip is 209 macroblocks = 304×176.
- Playback rate is not stored. `sys573_video.py` defaults to 30 fps, which is unverified.
- The dancers in these clips (e.g. MAX/EXTREME `JFAAAB`, `JFAFRA`, `JETHDA`) are pre-rendered
  CG with soft lighting and volumetric beams; there is no model data.

## 6. Open items

- MAX: 305 small card-side files stay unnamed. They are probably banner/chart files under
  names not in any available source; supplying a names list (`--names`) would close this.
- The playback fps of `.SBS` clips; the MAX/EXTREME clip-sequencing files `data/anime/*.anm` /
  `.can`; and the exact `FUN_8003d3a0` cross-fade weights. None of these is needed for export.

## 7. Conversion to DDR World (Background Dancers custom dancer)

Recipe: `tools/blender_ddr_addon/examples/port_character_sys573.py`; pure math in
`sys573_dancer_dump.py` (`world_*`, `routine_*`). It follows the Ultramix ports: the dancer keeps
its own rig and choreography, and the DLL plays the body's own `motion/*.anm` pool.

**Status (2026-09-29):**
- All 49 characters ship as `data_mods/custom_models/dancers/<N>MIX <Name>`, with 16 routines
  each: 784 clips in all, worst joint error 0.11 mm and 0 helper mismatches.
- `3rdMIX Afro` (key `ddr3afro00`) was cabinet-tested: it plays correctly. The other 48 were
  checked offline, including a Blender round-trip render of every exported model.

**Space.** World is row-vector, Y-up, metres, facing +Z with the dancer's left at +X.
`(x, y, z)_psx → (−x, −y, z) × 0.001` is a proper rotation, so there is no mirror, and
rotations convert as `R_world = C·R·Cᵀ`. The rest soles land at y ≈ 0.008; the 573 floor is
y = 0. The afro is 1.84 m to the top of the hair.

**Rig and mesh.**
- 31 bones in a fixed order (`world_bones`): `root`, then the 16 joints, then 14 helpers
  (`hand_L_alt0..4`, `hand_R_alt0..4`, `head_alt1..4`).
- Binds carry an identity rotation at each rest joint.
- The head's base object stays on `head`. Every other alternate rides its helper, which the
  clip scales between 1 and 1e-3. This reproduces §2's draw rule, face blinks included.
- The flat-colour sub-meshes become swatches in a 512×256 atlas (texture × white COLOR0 =
  `mdl_ch_constant_vc`).
- `.b2it` role aliases:

| World role | 573 joint |
|---|---|
| `Hips` | hips |
| `Spine2` | chest |
| `Head` | head |
| `LeftToeBase` | foot_L |
| `RightToeBase` | foot_R |

**Motion.**
- One `.anm` per routine, chaining its measures as §3's root re-basing does, at 120 frames per
  measure (World's 120-BPM dance clock) with keys every 2nd frame.
- A rigid re-framing of the binds by the exporter is absorbed as `W_target = B_target·B_src⁻¹·W`.
- Locals are `W_b·W_parent⁻¹` as kind 0x1C / 0x1D tracks, plus kind 10 scale tracks for the
  helpers.
- Root handling (`ROOT_MODE`):
  - `recentre` (default): the 573 path, centred on the mark;
  - `travel`: exactly as the 573 plays it;
  - `inplace`: root x/z removed.
- **Verified (afro, 16 routines; the same checks passed for all 49):**
  - The `.anm` evaluated by `anm_dump.evaluate_pose` matches the 573 pose within 0.11 mm per
    joint, with 0 helper-visibility mismatches.
  - The model passes the codec round trip, has unique bone identities, and uses 31 bones in one
    palette.
  - Re-importing the World files through the add-on renders correctly.

**Open:**
- Cabinet passes of the other 48.
- Which `ROOT_MODE` reads best on World's stages, and whether routine-end jumps need blending.
- Idle clips are not ported.
- The sidecar sex is a hand-made table.
- `ringf` / `ringm` are abstract ring-stack figures; they may want the `inplace` mode or
  exclusion.
