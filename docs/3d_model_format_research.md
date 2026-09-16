# DDR 3D Model & Animation Formats (KTMDL / ANM family) — RE Notes

**Target binary:** `gamemdx_20240402.dll` (DDR A3 final, x64). All addresses are
file-relative to the `0x180000000` image base. Sample data: DDR A3 install,
`data/arc/{pl_*,mapset_*,mc_*,camera/*}.arc`.

**Goal:** enough of the on-disk formats to write a Blender importer/exporter for
DDR dancer/stage models and their animations. Everything marked **VERIFIED** was
confirmed against the decompiled loader *and* a Python parser run over every
sample file (284 `.model`, 118 `.anm`, 104 `.camanm`, 22 `.sanm`, 22 `.tanm`).
Items marked **OPEN** still need work — see §10.

Reference tooling (committed): **`scripts/ktmdl_dump.py`** (parser, vertex
decoder, name packers, `--survey`) and **`scripts/anm_dump.py`** (container +
every track decoder, `encode_q48`, game-equivalent sampling, `--pose N --model
X` pose reconstruction). Both are import-safe modules meant to seed the Blender
add-on. Every constant below is reproduced there.

---

## 1. Container layout and file inventory

Models ship inside Konami ARC archives (`scripts/unpack_arc.py` handles them —
magic `0x19751120`, optional Konami LZ77). Per-asset directory layout inside the
arc:

```
data/chara/pl_<char>NN/            dancer body            .model .dds .grp2it .b2it
data/chara/pl_<char>NN_faceNN/     face swap part         .model .dds .grp2it
data/chara/pl_<char>NN_headNN/     hair/hat part          .model .dds .grp2it
data/chara/pl_<char>NN_{chest,forearm,hips}NN/  accessory parts
data/chara/pl_shadow00/            floor shadow quad      .model .dds .grp2it
data/chara/mc_{male,female}/       shared dance loops     mc_<sex>_<xx>NN_exec.anm, mc_<sex>_ne01_loop.anm
data/chara/mc_<sex>_<song>/        per-song choreography  mc_<sex>_<song>_<song>_exec.anm
data/map/gm_<stage>NN_<part>/      stage geometry         .model .dds .grp2it [.b2it] [*_play_loop.anm] [*.sanm] [*.tanm]
data/camera/music/<song>/          per-song camera        music_<song>.camanm
```

The game composes filenames with `"mc_%s_%s_%s_exec"` / `"%s.anm"` /
`"%s.b2it"` / `"%s.camanm"` (strings at `0x18026b5a0/0x18026b5b4/0x18026b310/0x18026b140`).

| Extension | Magic | Purpose | Status |
|---|---|---|---|
| `.model` | `"KTMDL\0\0\0"` | skinned/static mesh + skeleton + materials | VERIFIED (incl. mesh flags → render states, material params → shader constants) |
| `.anm` | `0xFF010001` | skeletal animation (bone tracks) | container + track layout VERIFIED, decoders VERIFIED |
| `.camanm` | `0xFF010001` | camera animation (chunk type 4) | layout + channel semantics + game projection VERIFIED (§6) |
| `.sanm` | `0xFF010001` | material *shader-parameter* animation (types 14+15) | VERIFIED — animates the material's `parameters` block (§7); dead in A3 (registry collision, §8) |
| `.tanm` | `0xFF010001` | material *texture/UV* animation (types 6+10) | layout VERIFIED; no evaluator exists (dead data) |
| `.b2it` | `"B2IT"` | bone name → bone index table | VERIFIED |
| `.grp2it` | `"B2IT"` | group name → index table (always one entry `model`) | VERIFIED |
| `.dds` | `DDS ` | textures (stock DXT) | standard |

Loader registration (`FUN_180002890` → per-extension callback tables at
`0x1801e6110` / `0x1801e61f8`): `model` → `FUN_18014cc40` → `FUN_180146cd0`
(registers a `gs::ModelData` GPU resource); `anm`/`vanm`/other → `FUN_180102fc0`
(checks magic, byte-swaps if big-endian, registers the raw buffer keyed by FNV-1
of the file's base name).

---

## 2. Name packing (the "6-bit name") — VERIFIED

Every identity in these formats (bone names, texture names, material names,
animation targets) is a **u64 of ten 6-bit codes**, MSB-first, not a string:

```
code = (v >> (54 - 6*i)) & 0x3F      i = 0..9  (bits 63..60 unused by the decoder)
 0x00        : empty
 0x01..0x1A  : 'a'..'z'   (chr(code + 0x60))          -- case is dropped
 0x1C..0x25  : '0'..'9'   (chr(code + 0x14))
 0x26..0x2B  : tokens "alp","nrm","rep","cube","dxt","merged" (table at 0x1802829d4)
```

Underscores and any other characters are **dropped**. Two variants exist:

* **Identity (bones, materials, textures' Maya node names):** one u64. Names
  longer than 10 chars are **XOR-folded**: char *i* lands on slot `i % 10`
  (e.g. `LeftUpLegRoll` → `cijtuplegr`). Bits 63..60 hold the number of dropped
  characters (`LeftToe_end` → bit 60 set). The game never decodes these — it only
  compares the u64s.
* **Texture *file* names (`+0x7C` table, 16-byte entries):** two u64s decoded
  sequentially → up to 20 chars, no folding (`jx_st002_03_add` → `jxst00203add`).
  The game decodes (`FUN_18018c3a0`) and hashes the decoded string with FNV-1.

```python
def pack_identity(s):            # bones / materials
    v, i, dropped = 0, 0, 0
    for ch in s.lower():
        if 'a' <= ch <= 'z': c = ord(ch) - 0x60
        elif '0' <= ch <= '9': c = ord(ch) - 0x14
        else: dropped += 1; continue
        v ^= c << (54 - 6*(i % 10)); i += 1
    return v | (dropped << 60)

def decode6(v):                  # texture-name half / debugging aid
    out = ''
    for sh in range(54, -1, -6):
        c = (v >> sh) & 0x3F
        if c == 0: continue
        out += chr(c+0x60) if c < 0x1C else chr(c+0x14) if c < 0x26 else \
               ['alp','nrm','rep','cube','dxt','merged'][c-0x26]
    return out
```

Checked: all 33 bones of `pl_emi00` round-trip against `pl_emi00.b2it` names;
texture-name table matched the `.dds` basenames of every model dir.

**Hash used everywhere else:** FNV-1 32-bit (`h = 0x811C9DC5; h = h*0x01000193 ^ b`)
— shader names, texture registry keys, animation targets. (Note: FNV-**1**, not
1a; the AVS file layer elsewhere in this repo uses 1a.)

---

## 3. KTMDL (`.model`) — VERIFIED

Binder `FUN_18018c610` (turns header offsets into pointers), converter
`FUN_180189900` (builds the GPU resource), helpers `FUN_18018a010` (bones),
`FUN_18018b000/FUN_18018b470` (material keys + vertex decls), `FUN_18018a0f0`
(VB upload), `FUN_18018a250` (IB upload), `FUN_18018a340` (blend-index remap),
`FUN_18018a590/a740/a990/ad50` (materials, textures, draw records).

All offsets in the header are **absolute file offsets**; offsets inside mesh /
buffer-descriptor records are **relative to the record**. Little-endian. Version
`2.2` in every sample; the loader requires `major == 2 && minor > 1`.

### 3.1 Header (0xC0 bytes)

```
0x00  char[8]  "KTMDL\0\0\0"
0x08  u32      version major (2)              required == 2
0x0C  u32      version minor (2)              required  > 1
0x10  u16      1 (checked == 1; endian/format flag)   u16 pad
0x14  u32      0
0x18  u32 bone_count        0x1C  u32 bone_off          -> §3.2  (0xB0 each)
0x20  u32 palette_count(1)  0x24  u32 palette_off       -> §3.4  (u16 slots)
0x28  u32 mesh_count        0x2C  u32 mesh_off          -> §3.3  (0x60 each)
0x30  u32 bufdesc_count     0x34  u32 bufdesc_off       -> §3.5  (0x20 each)
0x38  u32 0                 0x3C  u32 (== mesh_off)     unused section
0x40  u32 node_count        0x44  u32 node_off          -> §3.9  (0x30 each)
0x48  u32 material_count    0x4C  u32 material_off      -> §3.7  (0xA0 each)
0x50  u32 texture_count     0x54  u32 texture_off       -> §3.8  (0x50 each)
0x58  u32 1                 0x5C  u32 info_off          -> §3.10 (0x30)
0x60  u32 debug_off                                     -> §3.11 (shader/texture name strings)
0x64  u32 == bone_off  (legacy duplicate; only used if +0x94 != 0)
0x68  u32 0
0x6C  u32 == bufdesc_off (duplicate)
0x70  u32 == element_off (duplicate)
0x74  u32 buffer_blob_size   (file_size == vdata_off + blob_size + 16 in all samples)
0x78  u32 vdata_off          start of the vertex/index data blob
0x7C  u32 texname_count     0x80  u32 texname_off       -> 16-byte packed names (§2)
0x84  u32 0
0x88  u32 element_count     0x8C  u32 element_off       -> §3.6 (4 bytes each)
0x90  u32 file_size
0x94  u32 0
0x98  u32 0x7D931373        constant in all 284 files (exporter/format tag; not read)
0x9C..0xBF  zero
```

Section order in every sample: header, bones, palette, meshes(+their stream/
index descriptors interleaved in the bufdesc section), elements, nodes, info,
materials, texnames, textures, debug strings, buffer blob.

### 3.2 Bone (0xB0 bytes each)

```
0x00  u64   name identity (§2, XOR-folded)      0x08 u64 0
0x10  f32[16]  bind matrix   — MODEL-SPACE (absolute), row-major, row-vector
                convention: translation in [12],[13],[14]; p_world = p_local * M
0x50  f32[16]  inverse bind matrix (== inverse(+0x10); checked ≈I on all 2188 bones)
0x90  f32[3]   bone-space AABB max   0x9C f32 0
0xA0  f32[3]   bone-space AABB min
0xAC  i16      parent bone index (-1 = root)
0xAE  u16      0xFFFF on root bones, 0 otherwise
```

* Bones are topologically ordered (`parent < index` always).
* The AABB covers every vertex this bone influences (weight > 0) — **but as the two
  corners of the MODEL-space box pushed through the inverse bind matrix**, stored
  unsorted (`+0x90` = transformed max corner, `+0xA0` = transformed min corner), NOT a
  tight bone-space box. Verified exact (< 1e-4) on 313 stock bones; the tight-box
  hypothesis is off by up to 3.9 units. `tools/blender_ddr_addon/export_model.py`
  reproduces the rule. Stage models use bones as rigid animated nodes (e.g. `kanban1..29`).
* Character skeleton (33 bones, HumanIK-style): `Reference1, Hips, Spine,
  LeftUpLeg, RightUpLeg, Spine1, LeftUpLegRoll, RightUpLegRoll, Spine2, LeftLeg,
  RightLeg, Neck, LeftCollar, RightCollar, LeftLegRoll, RightLegRoll, Head,
  LeftArm, RightArm, LeftFoot, RightFoot, LeftArmRoll, RightArmRoll, LeftToeBase,
  RightToeBase, LeftForeArm, RightForeArm, LeftToe_end, RightToe_end,
  LeftForeArmRoll, RightForeArmRoll, LeftHand, RightHand` (index order). Units are
  metres (Hips at y≈0.97).
* GPU side: `FUN_18018a010` copies bind → array A, inverse bind → array B,
  parent index → array C.

### 3.3 Mesh (0x60 bytes each)

```
0x00  u16  flags       (render-state bits — see below)
0x02  u8   primitive   (always 1) 0→TRISTRIP 1→TRILIST 2→TRIFAN 3→LINELIST 4→LINESTRIP else POINTLIST
0x03  u8   flags2      (0 or 4; part of the flag word, see below)
0x04  u32  material index (→ §3.7)
0x08  u16  node index (→ §3.9; always 0 in stock data — used by the chunk-9 node-visibility animation, §7)
0x0A  u16 0   0x0C u32 0
0x10  u32  vertex_stream_count (always 1)   0x14  i32  rel. offset → vertex buffer descriptor(s) (§3.5)
0x18  u32  palette_entry_count (npal)       0x1C  i32  rel. offset → u16[npal] bone palette (global bone indices)
0x20  u32  index_buffer_count (always 1)    0x24  i32  rel. offset → index buffer descriptor (§3.5)
0x28  u32  0
0x2C  u8   texture_slot_count (0..2)        0x2D u8[3] 0
0x30  u32[8] texture entries: low u16 = texture index (§3.8); high u16 seen as 0 or 6 — not read by the game
0x50  f32[4] bounding sphere (cx, cy, cz, radius)   — used for frustum culling of static meshes;
                                                       skinned meshes are culled by their palette bones' AABBs (§3.2)
```

**Flag word → render states (VERIFIED, decompiled end to end):** the loader
(`FUN_18018ac50`) folds `flags`/`flags2` into a per-draw mask; the draw loop
(`gs_pass_draw_sorted` `FUN_180178aa0` → `gs_apply_mesh_flag_render_states`
`FUN_1801780b0` → the command-list executor `gs_cmdlist_execute_d3d9`
`FUN_18016a800`) turns that mask into D3D9 `SetRenderState` calls:

| file bit | mask | effect when SET (default when clear) |
|---|---|---|
| `flags & 0x0001` | 2 | `D3DRS_CULLMODE = NONE` — two-sided (default: back-face culling, winding from the pass) |
| `flags & 0x0020` | 4 | `D3DRS_ZENABLE = FALSE` — no depth test (default: test on) |
| `flags & 0x0040` | 1 | **transparent** — the mesh is collected by the `MODEL:TRANS` / `LOWPRIO_TRANS` pass (sorted back-to-front by sphere centre) and skipped by `MODEL:OPACITY`; also implies blend mode *alpha* when `flags2 & 0x3E == 0` |
| `flags & 0x0400` | 8 | `D3DRS_ZWRITEENABLE = FALSE` (default: write on) |
| `flags & 0x0800` | — | suppresses the whole blend-mode group (no blending even if 0x40 / `flags2` ask for it) |
| `flags2 & 0x01` | 0x10 | `D3DRS_ALPHATESTENABLE = FALSE` (default: alpha test ON, `GREATEREQUAL`, ref `0x7F` when unblended, ref `0` when blended) |
| `flags2 & 0x3E` | 0x20 / 0x40 / 0x60 | blend mode: `2` = alpha (`SRCALPHA, INVSRCALPHA, ADD`), `4` = additive (`SRCALPHA, ONE, ADD`), `8` = subtractive (`SRCALPHA, ONE, REVSUBTRACT`); alpha channel always `ONE, ZERO, ADD` |
| `flags & 0xF000` | bits 28..31 | copied up; bit 31 (`0x8000`) only bypasses the debug-material override — cosmetic |
| `flags & 0x03BE`, `flags2 & 0xC0` | — | **not read** by the game (Maya-exporter leftovers: 0x100 is present on 39 stock meshes and does nothing) |

Mask bit `0x100` ("skinned") is set by the loader from the vertex declaration
(an element with usage `0x20` = BLENDWEIGHT), never from the file; it selects the
bone-matrix texture bind at draw time. At runtime a mesh whose per-instance
colour alpha is < 1 is additionally forced to blend mode *alpha* and into the
TRANS pass even if its flags say opaque.

Observed stock combinations: `0x0000` (opaque, culled), `0x0001` (opaque,
two-sided), `0x0100` (≡ 0x0000), `0x02C0/0x03C0` (alpha-blended, z-write on),
`0x06C0/0x07C0` with `flags2 = 4` (additive, no z-write — light beams, glows),
`0x06C1/0x07C1` (same, two-sided), `0x0400` (opaque, no z-write). Exporter
defaults: `0x0000` for opaque characters, `0x02C0` for alpha-blended props,
`0x06C0` + `flags2 = 4` for additive effects. `scripts/ktmdl_dump.py::decode_mesh_flags`
reproduces the table.

**Which pass draws a mesh** = (node pass mask set by the game, never by the
file) ∧ (mesh flag 0x40). The four passes (`FUN_180137670`, `DAT_1802ed708..720`)
accept node masks `MODEL:DISTANTVIEW = 0x01`, `MODEL:OPACITY = 0x56`,
`MODEL:LOWPRIO_TRANS = 0x10`, `MODEL:TRANS = 0x46`. Dancer body/shadow nodes get
mask 2 (OPACITY + TRANS), stage parts get 4 (same passes), and stage parts
listed with a `:N` suffix in `map_resources.rlist` (§8) get `0x10` (OPACITY +
LOWPRIO_TRANS, drawn in priority order `N` instead of depth order). Within the
node's passes, opaque records go to OPACITY (sorted front-to-back) and
transparent records to TRANS/LOWPRIO_TRANS.

### 3.4 Bone palette

`palette_off` points at a u16 array of **global bone indices**. Each mesh's
`+0x1C` points into it (`npal` entries). Vertex `BLENDINDICES` are
**palette-local** (0..npal-1) in the file; at load time `FUN_18018a340`
rewrites them in the GPU copy to global bone indices via **the mesh's own
`+0x1C` table** (only for meshes whose declaration has BLENDWEIGHT). `npal` ≤ 32
and header `+0x20` (palette count) = 1 in all 284 samples, every mesh of a file
pointing at the same table.

**Per-mesh palettes are supported by the loader (VERIFIED 2026-09-15, `FUN_180189900`
+ `FUN_18018a340`):** the resource builder allocates and copies
`header.palette_count × 0x34` u16 → u8 slots starting at `header.palette_off`
(the u8 copy feeds the skinned-mesh frustum cull), i.e. it treats the palette
section as `palette_count` consecutive **52-slot blocks**; the blend-index remap
reads each mesh's own slice. So a model whose skinned meshes together reference
more than 52 bones is legal as long as (a) every single mesh stays ≤ 52, (b) the
distinct tables are laid out as 52-slot blocks (zero-padded) with
`header.palette_count` = block count, and (c) each mesh's `+0x1C` points at its
block. `ktmdl_dump.write_model` emits the stock single-table layout when every
mesh shares one table (byte-identical to all 284 stock files) and the block layout
otherwise; `tools/blender_ddr_addon/export_model.py` partitions a material's
triangles greedily by bone set (`partition_tris_by_bones`) so no single KTMDL mesh
exceeds 52 bones. Host-tested with a 64-bone rig (`tests/synthetic_test.py`:
2 meshes, palettes 52 + 13, every vertex resolves to its bone). Still ≤255 global
bones (indices are bytes after the remap). In-game confirmation of a multi-block
file is pending (§10).

### 3.5 Buffer descriptor (0x20 bytes each; both VB and IB)

```
0x00  i32  data offset, relative to this descriptor → into the blob
0x04  u32  count (vertices, or u16 indices)
0x08  u8   1 = VB, 0 = IB
0x09  u8   vertex stride (0 for IB)
0x0A  u8   vertex element count (0 for IB)
0x0B  u8   1 = VB, 0 = IB
0x0C  i32  rel. offset → element list (§3.6); 0 for IB
0x10  u32  0
0x14  u32  type: 0 = vertex buffer, 1 = index buffer (u16), 2 = other/ignored
0x18  u32 0   0x1C u32 0
```

Index buffers are `u16` triangle lists (count = 3 × triangles). The loader
computes vertex-index range (max−min) per VB for `DrawIndexedPrimitive`. Stock
data contains **1956 same-winding duplicate triangles** (mostly additive glow
props — drawn twice on purpose) and 1720 zero-area triangles; no repeated-index
triangles. Blender cannot hold two faces over one vertex set, so the importer
gives every repeat its own vertex copies (`_split_duplicate_faces`).

### 3.6 Vertex element (4 bytes) — a D3D9 `D3DVERTEXELEMENT9` in disguise

`u8 stream (0)`, `u8 byte_offset`, `u8 type`, `u8 usage`.

| file type | D3DDECLTYPE | | file usage | D3DDECLUSAGE / index |
|---|---|---|---|---|
| 0 | FLOAT1 | | 0x10, 0x11 | POSITION 0 |
| 1 | FLOAT2 | | 0x12 | NORMAL |
| 2 | FLOAT3 | | 0x13 | COLOR 0 |
| 3 | FLOAT4 | | 0x14 | COLOR 1 |
| 5 | UBYTE4N | | 0x16..0x1D | TEXCOORD 0..7 |
| 6 | SHORT2N | | 0x1E | BINORMAL |
| 7 | SHORT4N | | 0x1F | TANGENT |
| 8 | USHORT2N | | 0x20 | BLENDWEIGHT |
| 9 | USHORT4N | | 0x21 | BLENDINDICES |
| 0xB | UBYTE4 | | 0x22 | PSIZE |
| 0xC | SHORT2 | | 0x23 | TESSFACTOR |
| 0xD | SHORT4 | | <0x10 | raw D3D usage value |
| 0x10 | FLOAT16_2 | | | |
| 0x11 | FLOAT16_4 | | | |
| 0x12 | D3DCOLOR | | | |
| 0x14 | UDEC3 | | | |
| 0x15 | DEC3N | | | |

(`FUN_18018b1c0` / `FUN_18018b290`.) Only five layouts occur in stock data:

```
A  stride 48 (212 meshes, skinned + vertex colour):
   POSITION FLOAT3 @0 | BLENDINDICES UBYTE4 @12 | BLENDWEIGHT FLOAT3 @16 | NORMAL FLOAT3 @28 | TEXCOORD0 FLOAT16_2 @40 | COLOR0 D3DCOLOR @44
D  stride 44 ( 80, skinned):        POSITION @0 | BLENDINDICES @12 | BLENDWEIGHT @16 | NORMAL @28 | TEXCOORD0 @40
E  stride 44 (  2, skinned, no UV): POSITION @0 | BLENDINDICES @12 | BLENDWEIGHT @16 | NORMAL @28 | COLOR0 @40
B  stride 32 ( 99, static + colour):POSITION @0 | NORMAL @12 | TEXCOORD0 FLOAT16_2 @24 | COLOR0 @28
C  stride 28 ( 92, static):         POSITION @0 | NORMAL @12 | TEXCOORD0 FLOAT16_2 @24
```

**Skinning convention (VERIFIED against `gs_model_skinning_default` VS and the
bone AABBs):** `BLENDINDICES = (i0,i1,i2,i3)` palette-local; `BLENDWEIGHT =
(w1,w2,w3)` are the weights of `i1,i2,i3`; **`w0 = 1 − w1 − w2 − w3`** is the
weight of `i0`. Unused slots are index 0 / weight 0. The VS fetches bone
matrices from a texture (`s3`, 3 float4 rows per bone at v = 0.125/0.375/0.625;
row k · (pos,1) gives output component k) — i.e. 3×4 matrices, row = output
component. Colour is `D3DCOLOR` (BGRA bytes), UVs are IEEE half floats.
**UV orientation (VERIFIED numerically):** D3D convention — `v = 0` is the TOP
row of the DDS. `pl_emi00`'s hat mesh (the topmost geometry) maps to
`v ∈ [0.003, 0.286]` and the hat art sits in the top rows of `mdx_emi01.dds`;
vertex height vs `v` correlates at −0.82. Blender import must use `v' = 1 − v`
(and the reverse on export).

### 3.7 Material (0xA0 bytes each)

```
0x00  u64   Maya shading-node name identity (§2, folded) — hashed to FNV-1(decoded) at load (FUN_18018c310)
0x08  u32 0   0x0C u32 0
0x10  u16   param_count (2 or 3)     0x12 u16 0
0x14  u32   FNV-1 hash of the shader name (must match a string in §3.11)
0x18  u64   0 (pad; zero in all 471 stock materials — NOT read)
0x20  f32[4] × param_count   shader constants, uploaded VERBATIM (room for 8 → 0x20..0xA0)
```

(An earlier revision of this note placed the params at `+0x18`; the loader
`ktmdl_build_material_records` `FUN_18018a990` copies from `+0x20` — the values
quoted below are the corrected ones.)

Shader selection (`FUN_18018af30`): look up `+0x14` in the debug id table →
string → FNV-1 → shader registry (`shader.arc/<name>.gsp`, see
`docs/shader_replacement_research.md`). If absent, falls back to
`gs_model_skinning_default` when the mesh decl has BLENDWEIGHT, else
`gs_model_default`. Names seen in stock data and their counts:

```
mdl_ch_constant_vc 203   mdl_bg_constant_vc 96   mdl_bg_lambert 90   mdl_ch_lambert 70
mdl_ch_constant_c_vc 7   mdl_bg_constant 2       mdl_ch_constant_c 1 mdl_bg_constant_c_vc 1
mdl_ch_constant_vc_notex 1
```
`ch` = character (skinned VS), `bg` = background (static), `constant` = unlit,
`lambert` = lit, `vc` = uses COLOR0, `notex` = untextured. **Note:** A3's
`shader.arc` ships no `mdl_*_lambert.gsp`, so those 160 materials silently fall
back to the `gs_model_*_default` programs.

**What the fallback actually draws (VERIFIED 2026-09-15, `fxc /dumpbin` of
`gs_model_default.gsp` / `gs_model_skinning_default.gsp` — both PS blobs are
byte-identical):**
```
VS  o0 = pos · WorldViewProjection(c18..c21) ; o1.xy = TEXCOORD0 (NO m_vTexAnime) ; o2 = c23 (draw tint ONLY — COLOR0 not declared)
    (skinning variant: 4-weight palette skinning through the bone texture s3, w0 = 1 − Σw, ModelParameters c22 = texture width)
PS  oC0 = tex2D(s0, uv) * v1                       ; + screen-space dissolve:
    texkill( ModelParameters.y (c2.y) − tex2D(StippleMaskPattern s15, vPos/32).y )   ← a 32×32 stipple, no kill at y = 1.0
```
So every `lambert` material in A3 renders as **unlit texture × per-draw tint**;
its COLOR0 (never present on stock lambert meshes anyway), its `m_vTexAnime` and
its constant colour are all ignored, and NORMAL is read by nothing — there is no
lighting anywhere in the model pipeline. The `mdl_*` shaders differ only by:
`_vc` multiplies `COLOR0` into the tint in the VS, `_c` applies `p1.rgb` then adds
`p2.rgb` in the PS (`oC0.rgb = tex·color·c4.rgb + c5.rgb`, alpha = `tex.a·color.a`),
`_notex` skips the texture, and all `mdl_*` VS apply `m_vTexAnime`. The Blender
importer's node trees follow exactly this table (`import_model._shader_traits`);
a `lambert` material carries `mat["ddr_shader_note"]` explaining the fallback.

**Parameter semantics (VERIFIED — `fxc /dumpbin` of the stock `.gsp` blobs +
the draw loop):** every `mdl_*` VS/PS declares
```
struct parameters { float4 m_vTexAnime; float4 vConstatntColor; float4 vOffsetColor; }  // VS c24..c26, PS c3..c5
```
and `gs_model_bind_material_vs24_ps3` (`FUN_180183070`) emits
`SetVertexShaderConstantF(24, params, param_count)` +
`SetPixelShaderConstantF(3, params, param_count)` straight from the material
record — so the file's float4s ARE the shader constants:

* `p0 = m_vTexAnime = (scaleU, scaleV, offU, offV)`: VS computes
  `uv_out = uv * (1/x, 1/y) + zw`. Stock: `(1,1,0,0)` (identity) on 465
  materials; `(1,1,0.817,0)`, `(1,1,0,0.183)`, `(1,1,0,0.175)` on 5 (static UV
  offsets). Never zero (it is a divisor).
* `p1 = vConstatntColor = (r,g,b,a)`: stock `(1,1,1,0)` (435), `(1,1,1,1)` (23),
  grey `(k,k,k,0)` with k ∈ {0.8, 0.977, 0.951, 0.542, 0.061} (6), `(0,0,0,0)` (3).
  Only the `_c` shader variants read it (`oC0.rgb = tex * color * vConstatntColor.rgb
  + vOffsetColor.rgb`); the plain `constant_vc` PS is `oC0 = tex2D(s0, uv) * color`.
* `p2 = vOffsetColor`: stock `(0,0,0,0)` always; present only on `param_count = 3`
  materials (the `constant*` shaders), absent (`param_count = 2`) on `lambert`.
* `color` above = VS `c23` = the per-draw tint (`ModelUnitParameters.m_color` =
  item colour `node+0xD8` × per-record colour, default white) × COLOR0 — a
  runtime/instance value, not a file value. `ModelParameters` (VS `c22`) =
  `(bone_count, 1, bone_count-or-0, …)` for the bone-matrix texture width.

The animated `.sanm` record (§7) is the same 32-float block: at bind time the
animation player copies each targeted material's `+0x20..+0xA0` into a type-10
pose record (`anim_player_bind_set` `FUN_180158160`, slot 7, matched by
`FNV-1(decoded material name)`), the type-14 tracks overwrite individual floats,
and `anim_player_apply_frame` (`FUN_180158ad0`) writes the record back into the
render item's private material copy (`FUN_18015b0e0`) every frame. Exporter rule:
write `(1,1,0,0) / (1,1,1,0) / (0,0,0,0)` with `param_count = 3` for `constant`
shaders and `2` for the `lambert` names; a UV offset/scale can be baked into `p0`.

### 3.8 Texture (0x50 bytes each)

```
0x00  u64  Maya file-node name identity (e.g. "file1", "mdx_emi01_t") — informational
0x08  u16 0    0x0A u8 2   0x0B u8 2          (Maya sampler settings — NOT read by the loader, VERIFIED)
0x0C  u8 2     (idem)      0x0D u8 kind    0x0E u16 texname index (→ +0x7C table)
0x10  f32 1.0  0x14 f32 1.0                    (NOT read; the runtime stage record is hard-set to 1,1,0,0)
0x18..0x4F zero
```

`kind` (`FUN_18018a870`) selects the sampler stage: 0 = colour map (n-th kind-0
texture → stage n), 1 → stage 6+n, 2 → 4, 3 → 5, 4 → 2, 7 → 3. All stock
textures are kind 0. Texture binding (`FUN_18018a740`): decode the 20-char
texname (§2) → FNV-1 → texture registry (`FUN_180186100`), else a default
texture. **VERIFIED:** the DDS loader (`TextureFileTask` → `AsyncRegisterJob`,
`FUN_18014c6f0` → `FUN_18014c560`) registers each texture under
`FNV1(FUN_180145b80(basename))`, where `FUN_180145b80` lower-cases the
extension-less file name and **deletes every `_`**. So `mdx_emi01.dds` is keyed
`"mdxemi01"`, exactly what the packed texname decodes to. Exporter rule: texture
file names must be `[A-Za-z0-9_]` only (other characters survive the registry
hash but are dropped by the 6-bit packer, so they would never match), ≤ 20
alphanumerics.

**DDS pixel formats the loader accepts (`FUN_180164800` → D3DFMT, VERIFIED):**
fourcc `DXT1/DXT2/DXT3/DXT4/DXT5`, and uncompressed `A8R8G8B8` (0x15),
`X8R8G8B8` (0x16), `A8B8G8R8`/`X8B8G8R8` (0x20/0x21), `R8G8B8` (0x14), `R5G6B5`
(0x17), `A4R4G4B4` (0x1A), `A8L8` (0x33), `A8` (0x1C), `L8` (0x32), `G16R16`
(0x22), plus the 0x6E/0x6F..0x75 float/16-bit fourcc formats. Stock textures:
140 × A8R8G8B8, 101 × DXT1, 29 × DXT5, 3 × R5G6B5, 2 × X8R8G8B8 — all with a
3-level mip chain (`DDSD_LINEARSIZE`, caps `0x401008`), power-of-two except one.
So an exporter needs no DXT encoder: `scripts/ktmdl_dump.py::write_dds_a8r8g8b8`
writes the stock 32-bit shape (header byte-identical to the stock files; level 0
identical, mips differ only by the downsampling filter).

### 3.9 Node (0x30 bytes each)

```
0x00  u32  0 (0x84010000-style values on the 4 two-node stage models)
0x04  u32  0x034F1053 on every root node; 0x11D23D53 on the 4 child nodes  — exporter tag, NOT read by the loader (write the stock value)
0x08  u32  0
0x0C  u16  parent (0xFFFF root)     0x0E u16 0xFFFF root / 0 child
0x10  f32[4] bbox max    0x20 f32[4] bbox min
```
283 of 284 models have exactly one root node whose bbox equals the info bbox;
the loader copies only the parent index per node (to `GPU resource +0x90`,
consulted by the node-visibility animation, §7). Meshes reference their node via
mesh `+0x08` (always 0 in stock data). Treat as "one root node mirroring §3.10"
for export.

### 3.10 Info (0x30 bytes)

`u64 0x11CE18154C50230F` (identical constant in all files), `u64 0`, `f32[4]
bbox max`, `f32[4] bbox min` (model-space, w = 0). Copied to the GPU resource
header.

### 3.11 Debug-string block (`+0x60`) — load-bearing for shaders

```
0x00  u32 0
0x04  u32 rel. offset (always 0x14) → texture-file-name table
0x08  u32 rel. offset → shader-name table
0x0C  u32 shader_count
0x10  u32 rel. offset → u32[shader_count] FNV-1 ids (parallel to the shader-name table)
0x14  texture table: u32 offs[n] (relative to 0x14) then NUL strings ("pl_shadow00.dds")
      ids[shader_count]
      shader table: u32 offs[shader_count] (relative to table start) then NUL strings ("mdl_bg_constant")
```
The game resolves `material.+0x14` → id → string → FNV-1 → shader. Without this
block every material falls back to the default shader.

### 3.12 Buffer blob

`vdata_off .. vdata_off + blob_size`: all vertex buffers then all index buffers,
each 16-byte aligned (padding 0..40 bytes observed). Descriptors point into it
with relative offsets, so an exporter can lay it out freely.

---

## 4. B2IT (`.b2it`, `.grp2it`) — VERIFIED

```
0x00  "B2IT"
0x04  u32 file size
0x08  u64 0
0x10  u32 count
0x14  u32 offset → u32[count] absolute offsets to NUL-terminated names, sorted (ordinal, case-sensitive)
0x18  u32 offset → u32[count] target index (bone index in the .model), parallel to names
0x1C  u32 0
```
`.b2it` maps **original** bone names (with case and underscores) to bone
indices — the game uses it for attachment lookups; the `.model` itself only
carries folded identities. `.grp2it` always contains `{"model": 0}`. Emit both.

---

## 5. ANM family container — VERIFIED

Shared by `.anm .camanm .sanm .tanm` (and `vanm`, unseen). Little-endian on
PC; `FUN_180104220` byte-swaps big-endian sources using tables that also serve
as the struct definitions below.

```
0x00  u32  0xFF010001
0x04  u16  frame_count (last frame index; tracks hold frame_count+1 uniform keys) — duration = frame_count / fps
0x06  u16  exporter tag (anm: 0/1; camanm 0; sanm/tanm 1) — NOT read by the game (VERIFIED: the object ctor reads only +0x10.., the clock only +4)
0x08  u32  anm: 1; camanm/sanm/tanm: fps (60, or 24)  — read as fps only when a type-4 chunk exists
0x0C  u32  anm: 0; others: 1 — not read
0x10  u32  chunk_offsets[] (absolute), terminated by 0
```
Runtime default fps when not supplied: 60.0 (`DAT_1802dc26c`).

Chunk header: `u32 tag = 0xFF010002 + type`, `u16 h4`, `u16 h6`. Chunk map from
the animation object ctor `FUN_18013a800` (object field ← chunk):

| type | tag | body | seen in | object field |
|---|---|---|---|---|
| 0 | `0xFF010002` | track-offset list | `.anm` | +0x0C (bone tracks) |
| 1 | `0xFF010003` | skeleton hierarchy | `.anm` | (not stored) |
| 2 | `0xFF010004` | ? | — | +0x10 |
| 4 | `0xFF010006` | 6 fixed track slots (camera) | `.camanm` | +0x14 |
| 5 | `0xFF010007` | ? | — | +0x18 |
| 6 | `0xFF010008` | `h4` × u64 packed material names | `.tanm` | (byte-swap only) |
| 9 | `0xFF01000B` | track-offset list (kind 0x20 bit streams, target = node index) → node visibility (§7) | — | +0x1C |
| 10 | `0xFF01000C` | track-offset list | `.tanm` | +0x20 |
| 11 | `0xFF01000D` | light list: `h4` × u64 names ∈ {ambient, directional, point, spot} → category vector | — | +0x24 |
| 14 | `0xFF010010` | track-offset list | `.sanm` | +0x28 |
| 15 | `0xFF010011` | `h4` × 32-byte `{u64 name, u64 name2, u32 fnv, u8[12] 0}` → FNV-1(decode(name)) vector | `.sanm` | +0x50 vector |

**Track-offset list body:** at chunk+8, `u32 rel_offsets[]` (relative to the
chunk start), 0-terminated. **Type 1 body:** `u32 count @+4(h4)`, `u32 rel →
u8 pairs[count] = (bone_index, parent_or_0xFF)`, `u32 rel → u16 0/2` (trailing
word — the whole chunk is skipped by the object ctor, so both it and the pairs
are exporter-side validation data only). **Type 4 body:** six `u32` relative
track offsets at chunk+8..+0x1C (0 = absent).

### 5.1 Track (16 bytes) — VERIFIED

```
0x00  u16  kind (value encoding + channel, table below)
0x02  u16  exporter tag (anm: 3 with uniform keys, 0 with explicit times; camanm/sanm/tanm: 0) — NOT read by any evaluator (VERIFIED)
0x04  u16  key_count
0x06  u8   target index (bone index for type 0; camera slot / material slot / node index for others)
0x07  u8   secondary target (sanm: parameter component index) — see §7
0x08  u32  rel. offset → u16 times[key_count]   (0 = uniform: key k is at frame k)
0x0C  u32  rel. offset → values (kind-dependent stride)
```

Evaluator (`FUN_18013ab80`, one per chunk type; this one for bone tracks):
`f = t_seconds * fps`; with explicit times binary-search key `i` with
`times[i] ≤ floor(f) < times[i+1]`, `u = (f − times[i]) / (times[i+1] − times[i])`;
uniform: `i = floor(f)`, `u = frac(f)`; clamped at the last key. Then
`decoder[kind](out, values, i, i+1, u)` with `out` selected by kind:

| kind | channel | key stride | encoding | decoder |
|---|---|---|---|---|
| 1 | rotation | 16 | float4 quaternion (x,y,z,w), slerp | `FUN_180139300` |
| 2 | rotation | 32 | 2 × quat (value, tangent) — squad-style | `FUN_1801391a0` |
| 4 | translation | 16 | float3 (+pad), lerp | `FUN_180139570` |
| 7 | translation | 32 | float3 value @0 + float3 tangent @16, cubic Hermite | `FUN_180139420` |
| 8 | scalar | 4 | float, lerp (camera/material channels) — VERIFIED `(1−u)·a + u·b` | `0x180139600` (named `anm_decode_kind8_float_lerp` in the Ghidra DB) |
| 9 | scalar | 8 | float value + float tangent, Hermite | `FUN_18013a560` |
| 10 | scale | 16 | float3, lerp | `FUN_180139570` |
| 11 | scale | 32 | float3 + tangent Hermite | `FUN_180139420` |
| 0x16 | rotation | 8 | 64-bit packed axis-angle quat (§5.2), slerp | `FUN_180139740` |
| 0x17 | rotation | 16 | 2 × 64-bit packed (value, tangent) | `FUN_180139630` |
| 0x19 | translation | 6 | 3 × half float, lerp | `FUN_180139800` |
| 0x1A | rotation | 8 | 48-bit smallest-three quat in a u64 slot | `FUN_180139ad0` |
| 0x1B | scalar | 4 | float, **step** (key i while u < 1, else key i+1; no interpolation) | `0x180139aa0` (`anm_decode_kind1B_float_step`) |
| **0x1C** | rotation | **6** | **48-bit smallest-three quaternion (§5.2)**, slerp | `FUN_180139c60` |
| **0x1D** | translation | **12** | float3, lerp | `FUN_180139ed0` |
| **0x1E** | translation | **6** | 3 × half float, lerp | `FUN_18013a1d0` |
| **0x1F** | translation | 12 + 6/key | float3 base at values+0, then 3 × half float per key, lerp, result = base + delta | `FUN_180139f60` |
| 0x20 | visibility | 1 bit | bit `i` of a bit-stream (`byte[i>>3] >> (i&7)`), step; writes u8 `1` (bit set = visible) / `2` (clear = hidden) — the chunk-9 node-visibility channel (§7) | `0x18013a470` (`anm_decode_kind20_bit_step`) |
| 0x22 | scalar | 8 | float + tangent, alt. Hermite basis | `FUN_18013a4e0` |

Stock usage: `.anm` bone tracks use **0x1C (rot, 2570), 0x1D (pos, 2362),
10 (scale, 821), 0x1E (pos, 156), 0x1F (pos, 54)** only. `.camanm` uses 1, 4, 8.
`.sanm`/`.tanm` use 8 only. A minimal exporter therefore needs only 0x1C / 0x1D
/ 10 (or 4) for skeletal data and 1/4/8 for cameras.

Output pose record per bone (40 bytes): `quat[4] @0, pos[3] @0x10, scale[3] @0x1C`;
tracks whose target ≥ the model's bone count are skipped, so **track target
indices are the model's bone indices** (the type-1 chunk repeats the hierarchy
for validation).

**Keys are LOCAL (parent-relative) TRS — VERIFIED two ways.** (a) The pose
record is initialised by `FUN_18013ba50`, which decomposes
`bindWorld[i] · inverse(bindWorld[parent])` into the same `{quat, pos, scale}`
slots (roots decompose their world matrix directly). (b) Reconstructing frame 0
of `mc_female_ne01_loop.anm` with `world_i = local_i · world_parent`
(row-vector convention) against `pl_emi00`'s bind matrices reproduces the
spine/legs/feet within quantisation error (spine Σ|ΔR| = 0.06); the arms
differ only because the bind pose is a T-pose and the neutral loop stands with
arms down. Quaternion component order is **(x, y, z, w)**; the rotation matrix
built from it uses the same row-vector convention as the bind matrices.

**Pose → matrix (`FUN_18013bc20`, `FUN_18013c000`, `FUN_18013be60`):**
`local = S · R(q)`, translation in row 3, then for non-root bones the local
matrix's columns are divided by the **parent's scale** before
`world = local · parentWorld` — i.e. Maya-style *segment scale compensation*
(children inherit parent rotation/translation but not scale). Bones without a
track in a given channel keep the bind-derived default.

### 5.2 Compressed rotation encodings — VERIFIED (decompiled)

**48-bit smallest-three (kind 0x1C, 0x1A):** read 6 bytes as a little-endian
integer `v`:
```
a = (v >> 32) & 0x7FFF ;  b = (v >> 17) & 0x7FFF ;  c = (v >> 2) & 0x7FFF ;  m = v & 3
f(x) = (x - 16383.5) / 23169.767578125          # -> [-0.7071, 0.7071]
A,B,C = f(a), f(b), f(c);  D = sqrt(1 - (A²+B²+C²))
m == 0: q = (D, A, B, C)     m == 1: q = (A, D, B, C)
m == 2: q = (A, B, D, C)     m == 3: q = (A, B, C, D)          # (x, y, z, w) order
```
(`FUN_180138b20`; 16383.5 = `DAT_180288bcc`, 23169.77 = `DAT_180288bc8`.)
Encode: pick the largest-magnitude component as `D`, flip the sign of the whole
quaternion if `D < 0`, quantize the other three with `x = round(c*23169.7676 + 16383.5)`.

**64-bit axis-angle (kind 0x16/0x17, `FUN_180138980`):**
```
angle = (v & 0xFFFFF)        * π/(2^20-1)            # [0, π]
ax    = ((v >> 20) & 0xFFFFF) / (2^20-1)
ay    = ((v >> 40) & 0xFFFFF) / (2^20-1)
az    = 1 - ax - ay
flags = v >> 60 : bit0 → ax = -ax, bit1 → ay = -ay, bit2 → az = -az
axis  = normalize(ax, ay, az)
q     = (axis * sin(angle/2), cos(angle/2))
```

**Half floats (0x19/0x1E/0x1F):** standard IEEE binary16 → binary32 bit expansion
(no denormal/inf special-casing).

**Slerp (`FUN_180190880`):** dot < 0 → negate second quat; if `1 − |dot| ≤ 1e-5`
lerp, else true slerp. Hermite basis `FUN_180138f00` is the standard
`(2t³−3t²+1, −2t³+3t², t³−2t²+t, t³−t²)`.

### 5.3 `.anm` skeletal file, concretely

```
header (0x10) ; offsets {0x1C, 0x70, 0}
0x1C: type-1 chunk: h4 = bone_count; @+8 rel→ u8 (index,parent)[bone_count]; @+0xC rel→ u16 trailer
0x70: type-0 chunk: u32 rel offsets → 16-byte tracks (2–3 per bone: rotation 0x1C, translation 0x1D/0x1E/0x1F, optional scale 10)
      each track's values follow it; 16-byte aligned
```
Uniform tracks carry `frame_count+1` keys; explicit-time tracks are sparse
(`times[]` ascending, last time may be < frame_count). Bone count per file
matches the model's (33 for dancers; stage `.anm`s match their prop model).

---

## 6. `.camanm` — VERIFIED (layout, channel semantics, and the game's projection)

One type-4 chunk with six fixed slots (`target` byte 0..5). Evaluator
`FUN_18013afb0` writes a 0x40-byte camera record: slot 0 → `+0x10` (kind 1
quaternion, rotation), slot 1 → `+0x20` (kind 4 float3, position), slots 2..5 →
four scalars at `+0x2C/+0x30/+0x34/+0x38` (kind 8). Across all 104 files slots
2..5 are single-key constants (two files animate slot 2):

| slot | record | values seen | meaning (from the consumer, below) |
|---|---|---|---|
| 0 | `+0x10` quat (x,y,z,w) | explicit-time keys every 8–30 frames | camera orientation (same row-vector convention as bones) |
| 1 | `+0x20` float3 | `\|pos\| ≈ 230..1700` | camera position **in centimetres** |
| 2 | `+0x2C` | 41.53 (53), 37.85 (27), 40.0, 42.1, 39.8, 70.4 | Maya **vertical** angle of view, **degrees** (37.85° = Maya's default 35 mm lens at film aspect 1.5) |
| 3 | `+0x30` | 0.1 (60), 0.01 (26), 1.0 (17), 2.0 | near clip (× `CameraNode+0x88`, default 1.0) |
| 4 | `+0x34` | 32768 (58), 10000 (23), 20000, 100000, 1e6, 5000, 30000 | far clip |
| 5 | `+0x38` | 1.333 (66), 1.5 (36), 1.409, 1.778 | Maya film aspect ratio |

`fps = header+8 = 60`; `frame_count` = song length in frames.

**Consumer (`camera_node_apply_camanm_record` `FUN_18001cb50`, run by the
`scene::CameraNode` update right after `anim_player_apply_frame`):**

1. The animation player converts the record into the camera object
   (`*(node+0x38)`, 0x3F8 bytes): `R = quat_to_rowmat(q)`;
   `eye (+0x268) = pos`; `target (+0x274) = eye − 1000·R.row2` (the camera looks
   down its local **−Z**; `DAT_180265260 = 1000.0`); `up (+0x280) = normalize(R.row1)`
   (local **+Y**; replaced by world (0,1,0) when the debug flag
   `CAMERA_FORCE_UP_VECTOR` is set); `near/far (+0x2A8/+0x2AC) = slot3/slot4`;
   then `camera_set_perspective_fov_aspect` (`FUN_18001a790`)
   `(cam, fov = slot2·π/180, aspect = slot5, w = 1.0)`, which stores the frustum
   `t = tan(fov/2)`: `bottom/top (+0x298/+0x29C) = ∓t`, `left/right (+0x290/+0x294)
   = ∓t·aspect`, `+0x2A0 = right−left`, `+0x2A4 = top−bottom`, `+0x28C = w`.
2. The CameraNode then **rescales and re-projects for the 16:9 output**:
   `eye *= 0.01` (`DAT_1802921e8 = 0.01f` — the only use of that constant in the
   binary; `target` is shifted by the same delta so the view direction is kept),
   `near *= node+0x88 (1.0)`, and
   ```
   fov'   = atan2f(2.0, w · (right − left))          // = atan(1 / (tan(fovV/2)·aspect_file))
   camera_set_perspective_fov_aspect(cam, fov', 16/9 (DAT_180265264), w · 16/9 · node+0x8C (1.0))
   ```
3. `camera_build_projection` (`FUN_1801a6860`) builds a right-handed D3D
   off-centre perspective from the frustum: `m00 = 2w/(r−l)`, `m11 = 2w/(t−b)`,
   `m22 = −far/(far−near)`, `m23 = −1`, `m32 = −far·near/(far−near)` (ortho when
   `w ≤ 0`). View + projection are copied into all four model passes each frame
   (`FUN_18001d450`).

Net effect (all VERIFIED from the decompiled code; `k = node+0x8C = 1`):

```
H   = tan(fovV_deg·π/360) · aspect_file            # the Maya camera's horizontal half-tangent
t'  = tan( ½ · atan2(2, 2·H) ) = tan( ½ · atan(1/H) )
game horizontal half-tangent = t'        (m00 = 1/t')
game vertical   half-tangent = t'/(16/9) (m11 = (16/9)/t')
camera position (metres)     = pos_cm · 0.01
```

Worked values: `(41.53°, 1.333)` → game hFOV 63.2° / vFOV 38.2°;
`(37.85°, 1.5)` → 62.8° / 37.9°; `(70.4°, 1.333)` → 46.8° / 27.3°. Note the
mapping is monotonically **decreasing** in the file FOV (`atan2(2, r−l)` is
`90° − hFOV_file/2`, not `hFOV_file/2`) — surprising, but that is what the
code does; stock cameras were evidently tuned against the in-game result.
Confidence: VERIFIED by decompilation **and in-game (DDR A3, 2026-09-15)**: a
`.camanm` written by the add-on's exporter — one static camera, game position
`(0, 1.6, 5.0)` m looking at `(0, 0.9, 0)`, file FOV **20°**, aspect 1.333,
near 0.1 / far 32768 — packed as `data/arc/camera/camera_music_lesa.arc`
(the attract HOW TO PLAY demo's song-specific set; the game prefers
`camera_music_<song>.arc` over the stage set whenever the song has a row in
`music_camera_resources.rlist` — `FUN_180059d60`) framed the boom00 stage
EXACTLY like the Blender render made with the formula above (hFOV 76.8°,
dancer ≈ 38 % of the frame height, pad at 2/3 height, all three speaker
clusters in view; a 50/50 blend of the 4K game frame over the render lines
up to a pixel or two). The naive reading (file FOV = vertical FOV → 20°
vertical, dancer filling 90 % of the frame) is ruled out. (The test file kept
the stock header frame count — 6238 — with two explicit-time keys at frames 0
and 6238 on slots 0/1 and single-key constants on 2..5; whether a `.camanm`
SHORTER than the song holds its last key in-game was not exercised.)

Blender mapping: camera object location = `pos_cm · 0.01` (then Y-up→Z-up);
rotation = the slot-0 quaternion applied in the same row-vector convention as
the bones — Blender cameras also look down local −Z with +Y up, so no extra
rotation is needed; `sensor_fit = 'HORIZONTAL'`, `sensor_width = 36`,
`lens = 18 / t'`; render aspect 16:9; `clip_start = slot3`, `clip_end = slot4`
**unscaled** — the code scales only the position by 0.01, near/far are used as
written (so a file near of 1.0 really is 1 m in the metre-scale world). For
**export** invert the FOV: given the desired horizontal half-tangent `t'`,
`fov' = 2·atan(t')`, `H = 1/tan(fov')`, pick `aspect_file = 1.333` and write
`slot2 = 2·atan(H/1.333)·180/π`; write the position in centimetres.

---

## 7. `.sanm` / `.tanm` / node visibility / lights — VERIFIED (semantics), dead in A3 data

Both material formats target **materials by name**: `.tanm` type-6 chunk = `h4`
u64 material identities (§2, folded — matches the `.model` material `+0x00`
field); `.sanm` type-15 chunk = 32-byte entries whose first u64 is the same
identity (game reduces it to FNV-1(decoded name) via `FUN_18013a700`). Tracks
are kind 8 floats.

* **`.sanm` (type 14, evaluator `FUN_18013b400`):** target byte `+6` =
  material slot (index into the type-15 list), byte `+7` = **float index into
  the 32-float (0x80-byte) per-material record = the material's `parameters`
  block (§3.7)**: `[0..3] = m_vTexAnime (scaleU, scaleV, offU, offV)`,
  `[4..7] = vConstatntColor`, `[8..11] = vOffsetColor`, `[12..31]` unused slots.
  The record is seeded from the model's own material params at bind time and
  written back into the render item's material copy every frame (§3.7), so a
  `.sanm` literally animates VS `c24..` / PS `c3..`. Stock `.sanm`s animate
  components 4,5,6 (fade 1 → 0) — effective only on `_c` shader variants.
* **`.tanm` (type 10, object `+0x20`):** target byte `+6` = 0..4 (and 15..17),
  `+7` = 0; frame-0 values `(0,0,1,1,0)` — a legacy UV-transform layout.
  **No evaluator exists for `+0x20`** (see §8) — safe to omit from an exporter.
* **Node visibility (type 9, record type 6, `anim_player_apply_frame` slot 4;
  unused in A3 data):** kind-0x20 bit-stream tracks (§5.1), target byte `+6` =
  **node index** (§3.9). Value byte `1` = visible, `2` = hidden; a node whose
  parent (`GPU resource +0x90` u16 parents) is hidden is hidden too. Applied by
  setting bit 27 (`0x08000000`) on every draw record whose mesh `+0x08` node
  index matches — that bit makes the pass collectors skip the record.
* **Lights (type 11, `FUN_18013b1c0`, unused in A3 data):** the chunk's u64
  names select a light category (`ambient`/`directional`/`point`/`spot`);
  tracks address a 0x4C-byte light record indexed by byte `+7` with channel
  byte `+6`: 0 → `+0` (16 B), 1 → `+0x10` (12 B, position), 2 → `+0x1C`,
  3 → `+0x2C`, 4..7 → scalars at `+0x3C..+0x48`.

---

## 8. Runtime model → animation binding

* **Animation registries** (`FUN_180102fc0`): `.anm` → map `+0x58`, `.vanm` →
  `+0x78`, **everything else (`.sanm`, `.tanm`, `.camanm`) → one shared map
  `+0x98`** (`FUN_180147160`), all keyed by FNV-1 of the *extension-less* base
  name and de-duplicated by refcount. Consequence (VERIFIED from code + arc
  order): a stage's `X_play_loop.tanm` and `X_play_loop.sanm` collide; the
  `.tanm` precedes the `.sanm` in every stock arc, so the `.sanm` payload is
  never registered. Animation sets are 8-slot arrays (`FUN_18001c750`): slot 0
  `.anm`, slot 4 `.vanm`, slot 7 the shared map.
* **Evaluator dispatch** (`FUN_18013a680`) by pose-buffer record type:
  `3` skeleton (0x28/bone) ← chunk 0, `4` camera ← chunk 4, `6` per-node
  scalars ← chunk 9, `9` lights (0x4C/light) ← chunk 11, `10` materials
  (0x80/material) ← chunk 14. **Chunk type 10 (`.tanm`) has no evaluator** —
  together with the registry collision, treat `.tanm` as dead/legacy data and
  `.sanm` as effectively unused in A3. Record buffers are created by
  `FUN_18013b5f0` (`type, count`) with defaults from `FUN_18013b670`.
* **Bone tracks are addressed by index**, not name; the dancer `.anm`s assume
  the 33-bone skeleton in §3.2 order. Bones without a track keep the
  bind-derived local TRS (`FUN_18013ba50`).
* **Character assembly (`FUN_18005d5d0`, the dancer actor):**
  1. Loads the body `pl_<char>NN` model node and reads
     `chara_resources.rlist` (format below) for a per-character **uniform
     scale** (record field 3; default 1.0) applied as an extra transform on the
     model node (`node+0x88..0xC0`, `node+0x68 |= 0x40`) and a **shadow scale**
     (field 4) multiplying the `pl_shadow00` quad.
  2. Opens the body's **`<name>.b2it`** and looks up bone indices **by original
     name**: `Head`, `Hips`, `Spine2`, `LeftForeArmRoll`, `RightForeArmRoll`,
     and the ground-contact set `{Hips, Spine2, Head, LeftToeBase,
     RightToeBase}` (their mean position drives the `pl_shadow00` floor quad;
     the tallest drives its size/alpha).
  3. Attaches part models (`data/arc/%s_%s00.arc` → `pl_<char>_<part>00`) as
     rigid children of **one bone each** via `FUN_18005e560(actor, part,
     bone_index, extra_matrix)` — the string literals at `0x18026b2b4..` are
     `head` → `Head`, `hips` → `Hips` (looked up into `actor+0x120`), `chest` →
     `Spine2`, `forearm` → `LeftForeArmRoll` **and** a second `forearm` instance on
     `RightForeArmRoll`; the three `_face01..03` models are children of the
     `scene::EmotionController` node (`FUN_18001bd10`, `%s_face%02d`, only #1
     visible) which itself hangs off `Head`. A missing part arc is harmless:
     `FUN_18005e560` destroys the node when the model resource is null.
  **Attach transform (VERIFIED 2026-09-15 — `FUN_18005e560` + the TransformNode
  update `FUN_18015c000` + the render-item refresh `FUN_18015b300`):** the part's
  ModelNode gets `+0x28 |= 2`, `+0x2C = bone index`, and `extra_matrix` copied into
  its `+0x88` (flag `+0x68 & 0x40` iff non-identity). Each frame, for a child with
  `+0x28 & 2` under a parent whose `+0xC & 8` (ModelNode) is set and whose bone
  count exceeds `+0x2C`:
  ```
  rot  = quat( childLocalRT(+0x30/+0x40) · boneMatrix[+0x2C] (parent+0x80, MODEL-space animated bone) · parentWorldRT(+0x4C/+0x5C) )
  pos  = translation( childLocalRT · boneMatrix · parentExtra(+0x88) · parentWorldRT )
  draw = childExtra(+0x88) · RT(rot, pos)        (row-vector: extra first, i.e. in the part's own space)
  ```
  (`FUN_18018e8b0(out, A, B)` computes `B·A` — "apply B then A"; the multiplications
  above are written in application order.) Part local RT is the ctor identity, and
  `boneMatrix` at rest is the bind matrix, so **a part model is authored in its
  attach bone's bind frame** (`v_model = v_part · Extra · Bind[bone]`): face verts
  sit at bone-local `z ∈ [0.03, 0.11]` (in front of `Head`), forearm verts along
  bone-local `+x ∈ [−0.04, 0.30]` (down the arm). Extra matrices: body-scale
  `diag(s,s,s,1)` for `head/hips/chest/face/forearm-L`; for `forearm-R` the code
  builds `Scale(s) · MirrorX · RotX(π)` with `MirrorX = diag(DAT_1802626d4 = −1, 1, 1)`
  and the rotation from `sinf(3π/2) = −1` on m[5]/m[10] and `±sinf(π)` on m[6]/m[9]
  (about X) — the product is the point inversion **`diag(−s, −s, −s, 1)`**, so one
  left-forearm model serves both arms (it also flips winding; the stock forearm
  meshes are two-sided `0x06C1`). Because `pos` runs through `parentExtra` while the
  part's own vertices run through `childExtra` (both `diag(s)`), the whole assembled
  character is uniformly scaled about the body origin — in Blender this is the
  armature OBJECT's scale, with each part a bone-parented object whose mesh data is
  the raw file coordinates and whose local matrix is identity (or `diag(−1,−1,−1)`
  for forearm-R). Round-trip verified to 3e-7 m on every `pl_rinon00` part.
  Exporter consequence: part models need no skeleton of their own beyond a
  root; the body's `.b2it` must contain exactly those bone names.
* **Resource lists (`startup.arc` → `data/{chara,map,camera}/*_resources.rlist`,
  VERIFIED — `scripts/ktmdl_dump.py::parse_rlist` / `write_rlist` (byte-identical
  on all four A3 lists) / `upsert_rlist_row`):**
  ```
  0x00  "MRL0" "LE\0\0"   0x08 u32 record_count   0x0C u32 file_size
  0x10  records: { u32 key_off (= 12 + 4·n) ; u32 n ; u32 record_len ; u32 field_off[n] ;
                   NUL key string ; NUL field strings ; pad to 4 }   (offsets relative to the record)
  ```
  * `chara_resources.rlist` (26 entries): key `<char>NN` → `["pl", sex "M"/"F",
    class "A"/"B"/"C", model_scale, shadow_scale, unlock_id]` — e.g. `emi00 →
    pl F A 0.9 0.75 -1.0`, `babylon00 → pl M B 0.4 0.5 0.0`. Model name =
    `"%s_%s"` of fields 0 and the key. `unlock_id` (`-1.0` default, `16..19`
    event characters) is matched against the unlocked-character list; `sex`/class
    filter the random-select pools (`FUN_18005c250`). A new character needs a
    row here (repack `startup.arc` — in-game verified 2026-09-15, §10); the numbers
    are parsed with `atof`, so write them as the stock-shaped strings (`"0.9"`,
    `"-1.0"`). The game reads ONE list — a single-row file would drop every stock
    character. **Row ORDER matters**: CharaActor kinds ≥ 3 address `row kind−3`
    directly, and the attract HOW TO PLAY demo is kind 4 = row 1 (`rage00`) — see
    the pitfall note in §10 before moving rows.
  * `map_resources.rlist` (34 entries): key = stage → `[rgb_hex, rgb_hex,
    part…]`; parts become `gm_<stage>_<part>` models (`FUN_180062450`), an
    optional `:N` suffix sets the node pass mask to `0x10` (LOWPRIO_TRANS) and
    `node+0xE8 = N` as its draw priority (`bg:-2`, `stage:-1`); the two hex
    colours are parsed into the stage actor at `+0xF8` / `+0x108` (consumer not
    traced — OPEN, not needed for the plugin). **Selection (VERIFIED in-game
    2026-09-15):** `StageActor(index)` (`FUN_180061f30`) walks the list to ROW
    `index` (clamped to the last row) and takes the row KEY as the stage name;
    `index` is the song's `<bgstage>` in `musicdb.xml` (the HOW TO PLAY lesson =
    32 = the second `boom00` row, the one with the `footpanel` part). The arc is
    `data/arc/mapset_<key>.arc` (`FUN_1800622a0`), or `mapset_<key>_g.arc` for
    mcode `0x94e7` on cabinet class 6/7 (`FUN_180011b40`: machine type 4 = gold)
    — the `_g` variant differs only in `gm_boom00_footpanel/g/footPanel.dds`.
    A custom stage = a new row (any key), its `mapset_<key>{,_g}.arc` with
    `gm_<key>_<part>` models (copy the stock foot panel renamed for the pad),
    the same index appended to `stage_camera_resources.rlist`, and the song's
    `bgstage` pointed at it — the Griffin living room (`griffin00`, row 34, one
    37 k-triangle `room` part + `footpanel`) ran the attract demo that way.
  * `stage_camera_resources.rlist` / `music_camera_resources.rlist`: stage →
    list of `stNNN_stNN` / `..._nonNN` camera set names; song → three numbers +
    `"music_<song>"` (or an inline `name:time` cue list). Field semantics not
    traced (OPEN); the plugin only needs the file name.
* **Camera object** (0x3F8 bytes, vector at `*DAT_1802eee40+0x38`, first with
  byte `+0x3F4` set is active — `FUN_18001d940`): view matrix `+0x08`,
  projection `+0x1C8` built by `camera_build_projection` (`FUN_1801a6860`) from
  frustum `l,r,b,t` at `+0x290..+0x29C`, near/far at `+0x2A8/+0x2AC` (ortho when
  `+0x28C ≤ 0`), a standard right-handed D3D perspective; both matrices are
  copied into the four model passes each frame (`FUN_18001d450`). The full
  `.camanm` → frustum path is in §6.
* **Render path (VERIFIED):** `ModelNode` (`agcs::scene::ModelNode`) owns a
  render item (`FUN_180175ea0`, model at `item+0x60`, world matrix `+0x00`,
  per-draw 0x30-byte records at `+0x70` = `{colour, →GPU draw record, →material
  copy, →palette, flags, pass mask}`, bone matrices `+0x80`, private 0x168-byte
  material copies `+0x98`). Each frame `FUN_18015b300` refreshes the item, the
  pass driver `FUN_180176b10` collects records (`gs_pass_collect_opaque`
  `FUN_180178e00` / `gs_pass_collect_trans` `FUN_180179330`: node mask ∧ pass
  mask, transparency split, bit-27 hidden test, frustum cull `FUN_180176da0`),
  sorts them (mode 1 front-to-back, 2 back-to-front, 0 none) and
  `gs_pass_draw_sorted` (`FUN_180178aa0`) emits per record: world matrix, VS
  `c22` model params, VS `c23` tint, material bind (§3.7), render states (§3.3),
  vertex declaration, streams, bone texture (stage 3, skinned only),
  `DrawIndexedPrimitive`. GPU draw record (0x48 bytes at resource `+0x68`):
  `+0x00 sphere, +0x10 mask, +0x14 node index, +0x16 D3D primitive type,
  +0x18 stream count, +0x1C npal, +0x20 →stream table, +0x28 vertex decl,
  +0x30 →palette (200 B), +0x38 →material record, +0x40 →u8 palette (global
  bone ids, for the skinned-mesh cull)`.

---

## 9. Blender plugin — practical recipe

**Implemented — import AND export: `tools/blender_ddr_addon/`** — a Blender 4.2+
extension (`blender_manifest.toml`, also loadable as a legacy add-on) whose
format layer is `scripts/ktmdl_dump.py` + `scripts/anm_dump.py` (loaded from
`../../scripts` in a checkout, vendored by `scripts/build_blender_addon.sh`).

* Codec writers (pure Python, host-tested): `ktmdl_dump.write_model(spec)` /
  `model_to_spec` — **byte-identical round-trip on all 284 stock `.model` files**;
  `write_b2it` — byte-identical on all 403 `.b2it`/`.grp2it`; `write_dds_a8r8g8b8`;
  `anm_dump.write_anm(spec)` / `anm_to_spec` — every decoded key of all 222
  `.anm`/`.camanm` files reproduced exactly (99.95 % of 48-bit quaternions
  re-encode to the same bytes; the rest are equal-magnitude ties where the stock
  encoder picked the other component — identical after decode).
* `import_model.py` / `import_anm.py`: armature (game joint frames kept exactly,
  names from `.b2it`, exact file normals kept in a `ddr_normal` point attribute
  because Blender's custom-normal encoding is lossy on slivers), skinned meshes,
  DDS materials, per-frame `.anm` bake through `evaluate_pose`, `.camanm` cameras
  with the §6 projection.
* `export_model.py`: rest pose = bind pose, `.b2it`/`.grp2it`, layouts A/D/B/C
  chosen from skinning/UV/colour presence, one KTMDL mesh per material slot,
  ≤ 52-bone shared palette, ≤ 4 normalized weights (`w0` implicit), stock AABB
  rule, DDS copied from the source `.dds` or written as A8R8G8B8. Round-trip of
  `pl_emi00`: identical vertex counts, zero triangle mismatches
  (position/normal/uv/weights/winding), bind matrices 2e-6, AABBs 1e-6, `.b2it`
  identical. A 32-model sweep (stage props with vertex colours, multi-material,
  static layouts, dancers) matches stock geometry; vertex counts change only
  where duplicate faces were split or identical stock duplicates merged.
* `export_anm.py`: inverse of the import math (local = world · inv(parent world),
  segment-scale compensation undone) → kind 0x1C/0x1D(/10) uniform tracks; stock
  poses re-export within 1e-5 m; `.camanm` via the §6 FOV inverse, positions in cm.
* **Characters (2026-09-15)** — `import_character.py` / `export_character.py`
  (`File > Import/Export > DDR Character`): the body plus every sibling part model
  attached exactly as `FUN_18005d5d0` does (§8: bone-parented objects in the bone's
  bind frame, raw file axes, `head/face → Head`, `hips → Hips`, `chest → Spine2`,
  `forearm → LeftForeArmRoll` + a linked duplicate on `RightForeArmRoll` with local
  `diag(−1,−1,−1)`; `face02/03` imported hidden), the `chara_resources.rlist` row
  found next to the `pl_*` folders / under `startup/data/chara/` (its model scale
  becomes the armature object's uniform scale, the row is kept in
  `arm["ddr_rlist_row"]`). Export writes the game's `data/chara/` layout —
  `pl_<key>/` body (skinned, parts excluded), `pl_<key>_<part>/` per part (raw axes,
  `.model` + `.grp2it` + `.dds`, no `.b2it` like stock; the mirrored right forearm is
  never written), and `chara_resources.rlist` = the source list with this key's row
  upserted (scale field from the armature scale). User-attached meshes bone-parented
  to `Head/Hips/Spine2/LeftForeArmRoll` export as `head00/hips00/chest00/forearm00`
  (>1 object per part merges into one model). `tests/character_test.py` on
  `pl_rinon00` (the one stock body with all 7 parts): attachment, placement
  (3e-7 m), export layout, part vertex data == stock, rlist byte-identical, re-import.
* **Stages (2026-09-15)** — `import_stage.py` (`File > Import > DDR Stage`): pick any
  `gm_<stage>_<part>.model`; every part listed for that stage in `map_resources.rlist`
  is imported (the `:N` priority suffix stripped into `obj["ddr_stage_priority"]`), each
  with its `_play_loop.anm` baked, plus the stage's camera set from
  `stage_camera_resources.rlist` (`camera/long/<set>/*.camanm`, first one active). A
  `boom00` + `pl_rinon00` + `mc_female_hh01_exec` scene rendered through the stock
  `st001_st05` camera (Workbench FLAT/TEXTURE or Emission-routed Eevee = the game's
  unlit look) shows the set, the dancer with all parts and the choreography where
  expected — the whole import stack, end to end, before any in-game test.
* **Bone budget**: `write_model` lays out per-mesh 52-slot palette blocks when a model
  references > 52 bones (§3.4) and the exporter partitions triangles by bone set —
  `tests/synthetic_test.py` (64-bone rig → 2 meshes). Texture stems that fold to the
  same registry key (`lower(name)` minus `_`) are rejected at export.
* **Porting existing models (2026-09-15, both in-game verified — the playbook is in
  `tools/blender_ddr_addon/README.md`, the scripts in `tools/blender_ddr_addon/examples/`):**
  a Fortnite-rip Peter Griffin (FBX, 372-bone UE5 rig, A-pose) was put on the DDR rig by
  POSING the UE rig so its joints land on the DDR joints (T-pose, chain bones scaled along
  their axis to the DDR segment lengths; the UE rig's TWO arm chains — control + `deform_*`
  — both posed), baking, then re-targeting the weights by body class with a position blend
  across each segment's DDR bones (`Arm → ArmRoll → ForeArm` along X etc.; stock rigs weight
  the mid-segment Roll bones ~50 %). He dances the lesson choreography at rlist row 1 with
  no face-part arcs. The Griffin living room (`.blend`, 19 objects, plain-colour materials)
  became one static `gm_griffin00_room` part: evaluated meshes joined, 35 plain colours
  collapsed onto one 128×128 palette texture with per-face UVs pinned to swatches, 10 real
  textures resized to POT, scale 0.135, origin on the rug; plus a 4-shot `music_lesa.camanm`
  authored in Blender (the stock lesson camera spends 70 % of the demo 2–5 m behind the
  dancer — inside the couch). **Pitfall found:** a mesh WITHOUT a colour attribute under a
  `_vc` shader is invisible in-game (COLOR0 reads as `(0,0,0,0)`, the alpha test kills every
  pixel; no log line) — `export_model` now defaults colourless meshes to `mdl_*_constant`
  and warns on an explicit `_vc`; the ports carry an opaque-white colour attribute (stock
  layout A/B + `_vc`, the majority combination).
* **Materials**: node trees follow the decompiled shader table of §3.7 (`_vc` ×
  COLOR0 incl. alpha, `_c` × `p1.rgb` + `p2.rgb`, `m_vTexAnime` as a UV Mapping node
  with the V-flip folded in, `lambert` = texture only + a `ddr_shader_note`).
* Validation: `scripts/validate_blender_addon.sh <unpacked-data-root>` (headless
  Blender: `smoke_test.py` on `pl_emi00` + `mc_female_ne01_loop` + `music_danf`,
  `character_test.py` on `pl_rinon00` + the `boom00` stage/camera set render, `synthetic_test.py` — all PASS 2026-09-15).
  Menu: `File > Import/Export > DDR Model / DDR Character / DDR Animation`.

Delivery (in-game verified, §10): repack the exported folder(s) into
`data/arc/<dir>.arc` (`scripts/arctool pack --output X.arc <dir>/data`; the Python
`scripts/arc_tool.py pack` is byte-equivalent but too slow for multi-MB `.anm` arcs) and,
for a new character key, `startup.arc` with the upserted `chara_resources.rlist` — no
manifest edits anywhere. Not done: `map_resources.rlist` rows for stages (the writer
exists, no stage-export helper yet); a DXT encoder (uncompressed A8R8G8B8 is accepted,
just 4–8× larger); an arc-packing step inside the add-on itself.

**Import `.model`:**
1. Parse header → bones, meshes, bufdescs, elements, materials, texnames, debug block.
2. Armature: one bone per §3.2 entry; head = bind translation (row 3 of
   `+0x10`), orientation = bind rotation (rows 0..2, row-vector convention:
   Blender `Matrix(m).transposed()` gives the column-vector matrix). Parent from
   `+0xAC`. Names from the sibling `.b2it` (fall back to `decode6` of the folded
   identity, which is lossy).
3. Per mesh: read the VB via the element list (positions FLOAT3, normals FLOAT3,
   UV half2 → **`v' = 1 − v`**, colour BGRA), the IB as u16 triangle lists;
   material = `+0x04`; texture = `texnames[textures[mesh.tex[0] & 0xFFFF].name_idx]`
   + `.dds`. Skin weights: `w0 = 1 − Σw`, indices through the mesh palette →
   global bone. Material node: base colour = texture × COLOR0 (× `p1.rgb` for
   `_c` shaders); blend/culling/z-write from `decode_mesh_flags` (§3.3) —
   `transparent_pass` + blend `alpha` → Blender BLEND, `additive` → an add
   shader, `two_sided` → disable backface culling.
4. Units are metres, Y-up (convert to Blender Z-up). Character bodies are
   additionally scaled by `chara_resources.rlist` field 3 in-game (§8) — the
   character importer puts that on the armature object.

**Import a character (`import_character.load_character`):** body as above, then
for each sibling `pl_<char>NN_<part>NN/…model`: import with **raw axes and no
armature** (`load_model(axis_convert=False)`), bone-parent the objects to the §8
bone with `matrix_parent_inverse = Translation(0, −bone.length, 0)` (Blender bone
parenting is tail-relative) and `matrix_basis = I` (or `diag(−1,−1,−1)` on the
`RightForeArmRoll` duplicate); hide `face02/03` with `hide_set` (NOT
`hide_viewport`, which drops the object from the depsgraph so its world matrix
never evaluates). Export the character with `export_character.export_character`:
body meshes = armature children that are not bone-parented; parts = bone-parented
children grouped by `ddr_part` (or inferred from the parent bone), written with
`build_spec(raw_axes=True)` (object → part space = `matrix_basis` with the game's
mirror removed, so user edits on the object bake into the vertices).

**Import `.camanm`:** one camera object; per frame sample slot 0 (quaternion,
slerp) and slot 1 (position × 0.01), apply the same Y-up→Z-up conversion as the
models; lens from §6 (`lens = 18 / t'`, `sensor_fit = HORIZONTAL`, 16:9 render
size), clip from slots 3/4 verbatim.

**Import `.anm`:** fps 60 (or header `+8` when a type-4 chunk exists);
per track, target = bone index, decode by kind (§5.1/§5.2), keys are **local
TRS relative to the parent** with Maya segment-scale compensation; uniform
tracks have one key per frame. Build the world matrix chain exactly as
`FUN_18013bc20` (§5.1) before converting to Blender pose bones.

**Export `.model` (minimum viable):** header v2.2 with the constants in §3.1;
bones with bind + inverse bind (model space, row vectors) and per-bone AABBs
(the skinned-mesh frustum cull reads them — zero AABBs = the mesh is culled
whenever its bounding sphere is; use the corner-through-inverse-bind rule of
§3.2); one palette (≤52 bones per mesh, ≤255 total); meshes with layout A or D
(§3.6), palette-local UBYTE4 indices, 3-float weights, UVs with `v = 1 −
v_blender`, flags from §3.3 (`0x0000` opaque, `0x02C0` alpha, `0x06C0`+`flags2 =
4` additive), node index 0; materials naming an existing `mdl_*` shader via the
debug block (§3.11) with the stock param triple at `+0x20`; texture entries +
20-char packed texnames; `.grp2it` `{model:0}` and `.b2it` with the original
bone names; DDS textures named `[A-Za-z0-9_]+` ≤ 20 alphanumerics (A8R8G8B8
with 3 mips is fine, §3.8). Blend indices are remapped by the loader, so palette
order is free. Exact byte layout (section order, 16-byte alignment, element-list
dedupe, blob padding, 16-byte zero tail) is what `write_model` emits — verified
identical to every stock file.

**Export `.anm`:** header `{0xFF010001, u16 frames, u16 1, u32 1, u32 0}`,
type-1 hierarchy chunk (index/parent byte pairs + `u16 0` trailer), type-0
chunk with per-bone tracks: kind 0x1C rotation (48-bit smallest-three, §5.2),
kind 0x1D translation (float3) or kind 10 scale — uniform keys (`times_off = 0`,
`frames+1` keys) are the simplest; 16-byte align each track and its values.

**Export `.camanm`:** header `{0xFF010001, u16 frames, u16 0, u32 60, u32 1}`,
one type-4 chunk with slots 0 (kind 1 quaternion), 1 (kind 4 float3, position
in **centimetres**), 2 (kind 8, FOV degrees from the §6 inverse), 3/4 (near/far),
5 (aspect, 1.333); single-key constants for 2..5 are fine. Register the file in
`music_camera_resources.rlist` (§8) for a new song.

---

## 10. Open items (next RE steps)

Closed (2026-09-15 pass):
1. ~~Local vs. model-space keys~~ — done (§5.1).
2. ~~DDS registry key~~ — done (§3.8).
3. ~~Part attachment / `.b2it` consumer~~ — done (§8).
4. ~~`.sanm`/`.tanm` registry collision, kind-8 decoder~~ — done (§8, §5.1).
5. ~~Mesh flag bits → render pass / states~~ — done (§3.3): full D3D9
   render-state table + pass selection.
6. ~~Material params → `parameters` remap~~ — done (§3.7): the file's float4s
   live at `+0x20` (the old note was 8 bytes off) and are uploaded verbatim as
   VS `c24..` / PS `c3..`; `.sanm` animates the same block.
7. ~~Camera~~ — done (§6): position ×0.01, `atan2`-based 16:9 re-projection,
   −Z forward / +Y up.
8. ~~Kinds 0x1B / 0x20~~ — done (§5.1): stepped float / 1-bit visibility stream.
9. ~~Unknown header/track constants~~ — anm header `+0x06`, track `+0x02`, the
   type-1 chunk body: NOT read by the game (VERIFIED); node `+0x04`
   (`0x034F1053` / `0x11D23D53`) and KTMDL header `+0x98` (`0x7D931373`) are
   likewise never read by the loader — write the stock values.
10. ~~UV V-orientation~~ — done (§3.6): D3D top-down, Blender `1 − v`.
12. ~~`chara_resources.rlist`~~ — done (§8).

Closed (2026-09-15, second pass):
13. ~~Part attach transform~~ — done (§8): `partExtra · boneMatrix · body`, parts
    authored in the bone's bind frame, forearm-R = `diag(−s,−s,−s)`.
14. ~~`lambert` fallback semantics~~ — done (§3.7): unlit texture × tint, COLOR0 /
    params ignored, stipple dissolve.
15. ~~Per-mesh palettes / >52 bones~~ — done (§3.4): 52-slot blocks, loader-verified.
16. ~~MRL0 writer~~ — done (§8): byte-identical on all four lists.

**In-game (DDR A3 bottle, 2026-09-15) — delivery + the exported character CONFIRMED:**
`export_character` output for `pl_rinon00` (body + all 7 parts, every file our writer's
— re-ordered palettes, one texture record, our `.b2it`/`.grp2it`), renamed to Rage's
slot (`pl_rage00*`, plus NEW `pl_rage00_{head,chest,forearm,hips}00.arc` — the dancer
requests `data/arc/%s_%s00.arc` unconditionally via `FUN_18005d070` and the arc registry
`FUN_180142ad0` creates entries by path on demand, so new arcs need no manifest) and
packed with `scripts/arc_tool.py pack <dir>/data -o <name>.arc` straight into
`data/arc/`, loads, attaches and dances Rage's choreography in the attract HOW TO PLAY
demo at Rage's rlist scale — stock `startup.arc`. Delivery = repack the arc, back up the
stock one (`scripts/arc_tool.py` — Konami LZ77, member order irrelevant; the modpack's
LayeredFS was NOT used: on A3 last night's log showed zero `LayeredFS: using` redirects
and menu/overlay textures did not load, so it is unverified there).
**Pitfall found on the way (root cause corrected 2026-09-15, second pass):** forcing a
character through `chara_resources.rlist` by locking every OTHER row out (unlock id
`999`) made the attract demo show a dancer frozen in the BIND pose (a T-pose); a later
attempt that appended a new row and locked only the four stock male-A rows showed a
DIFFERENT RANDOM FEMALE T-posing on every boot. Neither was the random pool: the demo's
dancer is **rlist ROW 1**. `FUN_180060460` special-cases mcode `0x94e7` to CharaActor
kind **4**, and the ctor `FUN_18005c720` decodes kinds: `-2` random "M","A", `-1` random
"F","A", `0` random any, `1` random "M", `2` random "F", `3` = row 0, and every kind
`k ≥ 3` = **rlist row `k − 3`** (it walks the reader `k−3` records) — row 1 is `rage00`
in the stock list, which is why "the lesson dancer is always Rage" and why the
male-only lesson clip exists. After the row walk the ctor re-checks THAT row's unlock id
(`atoi(field 5) == 0`, else must be in the player's unlocked list) and on failure
re-picks from the WHOLE list (`FUN_18005c250(…, NULL, NULL)`) — a female body under the
male-only clip has no matching tracks and T-poses. Rules: a demo-visible test character
goes in **row 1** (or into Rage's slot); never lock row 1; and for ordinary songs the
random pools (`FUN_18005c250`: sex + class match ∧ `atoi(unlock) == 0`, else unlocked-list
membership; `-1.0` rows are NOT in a no-card pool) must stay non-empty — `rand % 0`.
**Edited geometry CONFIRMED** (same session): Rinon with the Head-weighted body vertices
scaled ×3 about the Head joint (`v' = pivot + (v−pivot)·(1+(F−1)·w_Head)` — smooth across
the neck blend) and the Head-attached parts (`face01..03`, `head00`) scaled ×3 as OBJECTS
— i.e. the exporter's part-transform baking — dances correctly in Rage's slot.
**`.anm` export CONFIRMED**: the attract HOW TO PLAY demo is a `DancePlaySequence` for
"Lesson by DJ" (mcode `0x94e7`, `FUN_180065060`) with both sides flagged entered, and the
in-song dancer path `FUN_180060460` special-cases that mcode to choreography index 4 =
the SONG-SPECIFIC clip `mc_male_lesa.arc/mc_male_lesa_lesa_exec.anm` (male-only ⇒ one
dancer, always male; `mc_male.arc`'s generic `_exec`/`ne01_loop` set is used for ordinary
songs only — an edit there is invisible in attract). That 6598-frame clip re-exported
through the add-on with a continuous 360°/240-frame root spin added on top of the stock
motion makes the dancer pirouette through the whole lesson. Exporter cost note (FIXED the
same day): the first exports were ~4× the stock size because every bone got per-frame
keys; `export_anm` now collapses a channel to ONE key when it never changes (rotation
compared after the 48-bit encoding, translation/scale within 1e-6) — the lesson clip is
1.13× stock, the idle loop 1.08× (stock's remaining edge is the half-float translation
kinds 0x1E/0x1F, deliberately not used: 0x1D float3 keeps the 1e-5 m round-trip).

**Second in-game pass (DDR A3, 2026-09-15) — the four remaining confirmations:**
* **`.camanm` / FOV formula** — see §6: the exporter's static 20°-file-FOV camera framed
  boom00 exactly like the formula render (game frame vs render blend within ~1 px; the
  naive "file FOV is vertical" reading is out). Delivered as
  `camera/camera_music_lesa.arc` — the demo's song has a `music_camera_resources.rlist`
  row, so the song set wins over `stage_camera.arc` (`FUN_180059d60`).
* **Multi-palette-block body (§3.4)** — Rinon with 25 extra COINCIDENT dummy bones
  (same head/tail/roll as a stock bone, parented to it ⇒ identity local; no `.anm` track
  ⇒ the game keeps the bind-derived local ⇒ the dummy's skinning matrix equals its
  parent's) and half of ~10k vertex influences moved onto them ⇒ 58 bones, 3 meshes with
  52/33/21-slot palette blocks, header `palette_count 3`. In Rage's slot she dances
  indistinguishably from the stock export — the loader's per-mesh palette path is real.
* **New character key / rlist row** — `pl_test00*.arc` (8 arcs, a rename of the stock
  export) + `startup.arc` repacked with `test00 → pl M A 1.0 0.8 0.0` inserted at ROW 1
  (`rage00` moved to the end, all unlock ids stock): Rinon dances the lesson as `test00`
  at that row's scale 1.0. New arc names need no manifest; `startup.arc` repacks fine with
  `scripts/arctool pack` (member order irrelevant).
* **Swapped texture** — `pl_test00`'s `jx_rinon01.dds` rewritten by
  `write_dds_a8r8g8b8` (512², 3 mips, channels rotated R→G→B): the body renders with the
  new colours (hair/wings blue, armour cream) while the part models' own textures stay
  stock — the game's DDS loader takes our uncompressed output as-is.
* **First custom character + custom stage (same day):** the Fortnite Peter Griffin port
  (`pl_peter00`, 19 k verts / 32.7 k tris in 3 meshes, 1024² + 1024×512 + 512² A8R8G8B8
  textures, rlist row 1, NO face arcs) dances the HOW TO PLAY lesson; the Griffin living
  room (`mapset_griffin00{,_g}.arc`, row 34, `bgstage` 32→34, custom `camera_music_lesa.arc`)
  is the attract stage — "no weird camera angles or clipping". Details in §9.
  The only failure on the way was the COLOR0/`_vc` invisibility (§9): boot #1 of Peter
  showed an empty dance pad and nothing in the log.

Still open (none blocks the plugin):
* `vanm` (vertex animation?) — no samples in A3; registered under `+0x78`.
* `music_camera_resources.rlist` numeric fields and the two `map_resources`
  colours — consumers not traced (cosmetic for the plugin).
* The `StippleMaskPattern` / `ModelParameters.y` dissolve of the default PS
  (§3.7): which actor path drives `.y` below 1.0 (character fade-in?) — cosmetic.

**Ghidra names added this pass** (project `DDRWorld_Ghidra`, program
`gamemdx_20240402.dll`): `anm_decode_kind1B_float_step` (0x180139aa0),
`anm_decode_kind20_bit_step` (0x18013a470), `gs_model_bind_material_vs24_ps3`
(0x180183070), `gs_apply_mesh_flag_render_states` (0x1801780b0),
`gs_cmdlist_execute_d3d9` (0x18016a800), `anim_player_apply_frame`
(0x180158ad0), `camera_node_apply_camanm_record` (0x18001cb50),
`camera_set_perspective_fov_aspect` (0x18001a790); `anm_decode_kind8_float_lerp`
(0x180139600) from the previous pass.
