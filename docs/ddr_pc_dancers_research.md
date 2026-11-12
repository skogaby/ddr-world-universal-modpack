# Dance Dance Revolution for Windows (KCEA 2002) — Dancers RE Notes (2026-10-10)

The Windows release of DDR ("DDR PC", Konami Computer Entertainment America, July 2002; the cracked
exe reads `gimpsRus 03 Aug 2002`) is the **System 573 dancer engine on Direct3D 8**. Its 24 built-in
characters are the 3rdMIX–5thMIX cast re-dressed (the texture names still say `yaro3`, `afro4`,
`iizm`, `lad8b`, `meido`, `evilb`, `robo4`, `spac*` …), and Konami later published 24 more as
"downloadable characters" (`ddr_char01.exe` … `ddr_char12-rick.exe`, `ddr_char_jason.exe`). The
content is not in 573 file formats any more: KCEA compiled every mesh, texture table and dance
routine **into the executables as C data**, baked the 573's rotation tracks down to per-frame
matrices and pre-posed the meshes in each joint's frame.

Tools: `scripts/ddrpc_dancer_dump.py` (decoders, survey, OBJ/texture dump, World conversion math),
`tools/blender_ddr_addon/examples/port_character_ddrpc.py` (the port). Programs in the Ghidra
project: `F74707_DanceDanceRevolution.exe` (the retail exe from `Data.Cab`), `alex.dll` (one
downloadable character). Addresses below are VAs of the retail exe (image base `0x400000`).

## 0. What to run

```bash
# 1. unpack the installer CD once (cabextract) and the downloadable characters (they are zips)
W=~/Desktop/ddrpc_work; mkdir -p $W/game $W/character_dll
cabextract -d $W/cab "<cd>/Data.Cab"
ln -s ../cab/F74707_DanceDanceRevolution.exe $W/game/DanceDanceRevolution.exe
ln -s ../cab/F91401_data.bin $W/game/data.bin
for f in "<dlc dir>"/*.exe; do unzip -o -q "$f" -d /tmp/dlc_$(basename "$f" .exe); done
cp /tmp/dlc_*/character_installer/*.dll /tmp/dlc_*/character_installer/*.bin $W/character_dll/
#   ddr_char_jason.exe ships jason.bin with a DLL also called guy.dll (a different model from
#   ddr_char02-guy's guy.dll): rename that one jason.dll before copying.
ln -s ../character_dll $W/game/character_dll

# 2. inspect
python3 scripts/ddrpc_dancer_dump.py info   $W/game $W/character_dll
python3 scripts/ddrpc_dancer_dump.py survey $W/game $W/character_dll
python3 scripts/ddrpc_dancer_dump.py textures $W/game Rage /tmp/rage_tex
python3 scripts/ddrpc_dancer_dump.py obj $W/game Afro /tmp/afro.obj --routine MF_hiphop1 --frame 300

# 3. port (the whole cast, ~10 min without previews; §6)
DANCERS=all /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
    --python-exit-code 1 --python tools/blender_ddr_addon/examples/port_character_ddrpc.py
```

An installed game works as `<game dir>` too (`DanceDanceRevolution.exe` + `data.bin` beside each
other; `character_dll/` is where the manual tells players to drop the `.dll`/`.bin` pairs).

## 1. Distribution

| File | What |
|---|---|
| `Data.Cab` (56 MB, MSZIP) | 610 members: `F74707_DanceDanceRevolution.exe` (12.7 MB), `F91401_data.bin` (90 MB), the manual, and ~600 `.htm`/`.jpg` of the bundled web manual |
| `original_data.bin` (191 MB, CD root) | `original_data\0` header + the song audio (ADPCM WAVE, `RIFF…WAVEfmt` with `wFormatTag 0x11`), read straight from the CD; nothing visual |
| `s573.exe` | 24 KB launcher stub |
| `ddr_char*.exe` | plain **zip** archives (`file` says "Zip archive"); each holds `character_installer/{<name>.dll, <name>.bin, install.exe, manual.txt}` for one or two characters |
| `<name>.dll` | 57–61 KB PE DLL exporting one symbol, `dll_header` (§2). The `.text` is only the MSVC CRT; the model is `.data` |
| `<name>.bin` | the character's BMPs back to back: 38×38 icon, 100×160 portrait, 640×480 splash, then its 64×64 (or larger) textures |

The exe's `.data` section (`0x507000`, 14 MB) holds everything the 573 kept on flash: the 24
built-in `dll_header` records, their models, all routines, and the offset table of `data.bin`.

## 2. `dll_header` and the model (`FUN_00401060`, `FUN_00401bd0`, `FUN_004c1d60`)

`FUN_00401060` fills a 48-slot table (`DAT_0101dac4`) with the 24 built-in records at
`0xFEE410` (44 bytes apart) and then `LoadLibraryA`s every `character_dll\*.dll`, takes
`GetProcAddress(…, "dll_header")`, and stores it at slot `header[0]` if that slot is free and
the named `.bin` exists. Downloadable ids are 24–47; the character select shows 48.

```
dll_header (11 x u32)
 0  id            0..47
 1  sex           0 male, 1 female (which motion pool and playlist table)
 2  float scale   model scale (built-ins 0.90..1.0; every DLL 1.0)
 3  char*[ntex]   texture file names (t0_…bmp = face frame 0, t1..t3 = blink / mouth frames)
 4  u32[ntex]     texture source: built-in = data.bin file id; DLL = index into the .bin table
 5  model*
 6  u32[]         DLL: .bin member offsets (0-terminated after entry 0); built-in: 0
 7  char*         DLL: the .bin file name; built-in: 0
 8  u32           portrait (built-in: data.bin id; DLL: .bin member index)
 9  0
10  u32           splash
```

```
model
 +0   u32 ntex, nmat, nvert, ntri, njoint (=16)
 +20  mat*      nmat x 76 bytes: D3DMATERIAL8 (17 floats, UNUSED: SetMaterial is never called)
                + u32 nframes + u32* frames (texture indices; the state machine picks one)
 +24  vert*     nvert x 36 bytes, D3DFVF 0x152 = XYZ | NORMAL | DIFFUSE | TEX1
 +28  u32[17]   first vertex per joint (vertices are grouped by joint; [16] = nvert)
 +32  u16[3*ntri] indices
 +36  u32[nmat+1] first index per material
 +40  char[16][ntex] texture name stems (often empty)
```

**Draw** (`FUN_004c1d60`): `ProcessVertices` per joint with that joint's current matrix
(`FUN_004c1d60`'s loop over `joint_vert_start`), then per material `SetTexture(0, frame)` and
`DrawIndexedPrimitive(TRIANGLELIST)` over `[mat_tri_start[i], mat_tri_start[i+1])`. Render state
set in `FUN_004bf990`: `DIFFUSEMATERIALSOURCE = D3DMCS_COLOR1`, `AMBIENTMATERIALSOURCE =
D3DMCS_MATERIAL`, lighting on, texture stage `MODULATE(TEXTURE, DIFFUSE)`; the dancer draw sets
`D3DRS_CULLMODE = D3DCULL_NONE` (byte `+0x54` of the object, `FUN_00402240`). So the shaded colour
is **the vertex diffuse**, times the texture where there is one. A material with no frame list
(`SetTexture(NULL)`) draws the vertex colour alone — these are the 573's flat-colour primitives,
and every shipped one is a single colour per material (survey: 0 multi-colour untextured
materials; textured materials are all white).

Some frame-list pointers land in `.bss` (`0x101D668` …): they read 0 at run time, i.e. texture 0.
`Image.bytes` reproduces that; the raw file bytes there are garbage.

Textures are loaded with `D3DCOLORKEY 0xFFF800F8` (`FUN_00401bd0` → `FUN_004c2a10`): texels
`(248, 0, 248)` are transparent. Built-in textures are `data.bin` files; `data.bin` is a plain
concatenation indexed by a 2596-entry `u32` offset table at `0xFF50E0` (`FUN_00402f30`: size =
`off[id+1] − off[id]`; the table ends exactly at the file size). `data.bin` also holds the
portraits (100×160 8 bpp), splashes (640×480), banners, `.ssq` charts and `RIFF` sounds.

Vertex counts: 507–984 per dancer; 565–811 triangles; 7–22 materials; 7–15 textures (64×64 up
to 256×256, 4/8/24 bpp BMP). Vertices are **joint-local in the joint's rest frame**, i.e. already
rotated the way the 573 object was mounted on its `GsCOORDINATE2` — not merely translated.

## 3. Rig and motion (`FUN_00402240`)

Joint order in the file (named from the rest matrices and the meshes; D3D left-handed, model
faces **−Z**, so the dancer's left is **+X**):

| 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| chest | head | hips | upperarm R | foot R | forearm R | shin R | thigh R | hand R | neck | upperarm L | foot L | forearm L | shin L | thigh L | hand L |

Rest hips at y ≈ 10.0 units (= 1.0 m at the port's 0.1 scale; the 573's were 1060 mm). The hands
and faces are NOT alternates any more: hand shapes were collapsed to one, and the face's blink /
mouth frames are the material's texture frames (`t0..t3`) switched per beat by `FUN_00402240`'s
end-of-routine case table (eye-blink when `(beat & 0x3f) < 0x20`, mouth modes by `% 5`). The
port keeps frame 0.

```
routine  = u32 clip*[] (0-terminated)        18 of them, named in the exe's motion list
clip     = { u32 nframes (=60), u32 njoint (=16), float*[nframes] }
frame    = 16 x 12 floats: D3D row matrix (3 basis rows + translation), joint -> world
```

- `0xFEE850`: 18 list pointers; `0xFEE898`: 19 name pointers (`inst_motions` first, then
  `M_normal, F_normal, M_y31, M_capoera1, M_y11, M_thouse2, MF_hopping1, MF_thouse3, MF_hiphop2,
  MF_hiphop1, F_soul2, F_n31, F_mhouse1, F_jazz1, F_jazz2, F_sino_, F_lock1, F_soul1`). The
  prefix is the sex that plays the routine; the names are the 573's (§`sys573_dancers_research.md`
  3), 16 dances of 13–14 measures plus two 5-measure idles.
- One clip = one measure at **60 frames**; `FUN_00402240` advances `+1` frame per tick
  (`DAT_00FEE830`) and moves to the next clip at 60, so the 573's 1920-unit measure became 60
  frames. Nothing re-bases the root between measures: the baked matrices are **in place** (hips
  x/z stay ≈ 0 across every routine), the 573's travel was removed in the bake.
- Rotation blocks are orthonormal to 3.5e-6; the port re-orthonormalises (SVD) before taking a
  quaternion.
- **Playlists**: two tables of pointer rows indexed by `id & 7` — male `0xFEE14C` (8 rows × 8
  routines + 0), female `0xFEE26C` (8 × 12 + 0). A male plays the 8 `M_`/`MF_` routines, a
  female the 12 `F_`/`MF_` ones, in the row's order, looping; `routine_index = id*3 % (7|11) + 1`
  at song start. Ni-Na (id 10) is flagged **male** in the header and dances the male pool (her
  573 ancestor `iizf` was a girl); the port keeps the header's sex.

## 4. Space conversion (`ddrpc_dancer_dump.d3d_to_world`)

World is row-vector, Y-up, metres, facing +Z, dancer's left at +X. D3D here is row-vector,
Y-up, left-handed, facing −Z, dancer's left at +X. `S = diag(1, 1, −1, 1)` maps one to the other
(`v_w = v_d S`, `M_w = S M_d S`, translation × 0.1). The mirror flips handedness, so D3D's
clockwise front faces come out counter-clockwise in World's right-handed frame: **index order is
kept** (unlike the 573 port). The survey's `ccw` fraction (stored normal vs. geometric normal
after the mirror) is 0.78–0.94 per dancer, the rest being the sloppy normals a `CULL_NONE` game
never noticed; the port exports two-sided like the game draws.

## 5. Conversion to DDR World

Recipe: `tools/blender_ddr_addon/examples/port_character_ddrpc.py`; pure math in
`ddrpc_dancer_dump.py` (`rest_matrices`, `world_binds`, `world_atlas`, `world_mesh`,
`routine_worlds`, `retarget_worlds`, `worlds_to_anm_spec`). It follows the Ultramix / 573 ports:
the dancer keeps its own rig and choreography and the DLL plays the body's own `motion/*.anm` pool.

**Rig.** 17 bones: `root` + the 16 joints in parent-first order (`BONE_NAMES`); hierarchy is the
573's (`hips → chest → neck → head`, arms from `chest`, legs from `hips`). **Binds are the
sex's `normal` idle, clip 0, frame 0 joint matrices** (converted) — rotation included. The first
attempt used translation-only binds and every limb came out twisted: the joint-local meshes are
authored in the rotated joint frames. `.b2it` role aliases as the 573 port: `Hips/Spine2/Head/
LeftToeBase/RightToeBase → hips/chest/head/foot_L/foot_R`.

**Mesh.** One rigid-skinned mesh, every vertex 1.0 on its joint, the file's normals via
`ddr_normal`, `mdl_ch_constant_vc`, two-sided. Texture = one atlas (shelf-packed, power-of-two,
≤ 512×512 so far) of every BMP at 2× nearest with the colour key as alpha 0, plus a 16×16 swatch
per flat material colour; those materials get white COLOR0 and point at their swatch (the 573
port's convention, so Workbench previews are truthful). Textured materials keep the file's
(white) diffuse as COLOR0.

**Motion.** One `.anm` per routine of the dancer's own playlist (8 or 12 per dancer), named by
the 573 routine (`hopping1.anm`, the `M_/F_/MF_` prefix dropped): the clips back to back, one key
per PC frame at every 2nd World frame (120 per measure — World's 120-BPM dance clock, `bpm_sync`
maps it), kind 0x1C/0x1D local tracks, constant tracks collapsed. `ROOT_MODE=inplace` (default;
the data never travels) or `recentre`. Each clip is evaluated back with `anm_dump.evaluate_pose`
against the PC matrices: worst joint error **0.13 mm** over all 484 clips.

**Sidecar.** `<key>, pl, <sex>, A, <header scale>, 0.75|0.8, 0.0`; keys `ddrpc<name>00`
(`Robo2000 → ddrpcrobo2k00`).

**Status (2026-10-10):** 49 dancers under `data_mods/custom_models/dancers/DDR WINDOWS 2K2/`
(24 built-in + 25 downloadable — `guy.dll` and `jason.dll` are different models even though
Jason's pack ships its DLL under the name `guy.dll`). All passed the codec round trip, unique
bone identities, one palette, and a Blender re-import render of every exported model (contact
sheet matches the game's portraits). **Not yet cabinet-tested.**

## 6. Timings and gotchas

- Full port without previews ≈ 10 min (Blender 5.x, M-series). `PREVIEW=1` adds ~15 s per dancer
  — run it as a separate pass if you want all 49.
- Blender's Python has no Pillow: `ddrpc_dancer_dump` decodes BMP and writes PNG itself.
- A background Blender started from a tool shell that is later killed dies with it; run long
  batches with `nohup … &` and poll the log.
- The `Error: model data and motion data does not match..` string in the exe fires when a clip
  has fewer joints than the model's `joint_vert_start` table — the models and clips are all 16.

## 7. Open items

- Cabinet pass of the ported dancers (shadow / Big Head bones via the role aliases, two-sided
  flag, atlas alpha test).
- Blink / mouth texture frames are dropped (World has no per-material texture switch in a
  dancer clip; the 573 port's helper-bone trick does not apply since the PC has one face mesh).
- The `normal` idles are not ported (World has its own idle).
- Six of the 24 downloadable slots in the manual's "24" were never found online beyond these 13
  packs (ids 24–47 are all taken by the 25 DLLs we have, so the set is complete).
