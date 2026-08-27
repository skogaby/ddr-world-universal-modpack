# Custom Arrow Renderer — Feasibility RE Notes (Hallway Perspective)

Reverse-engineering feasibility study for a wholly custom note renderer,
motivated by porting StepMania/ITGMania-style **player perspective options**
(first target: the "hallway" view — notes recede toward a vanishing point,
shrinking and converging as they scroll away from the player).

All addresses are file-relative to `gamemdx.dll` @ `0x180000000`. Primary
build: **20260616**; every load-bearing pipeline claim cross-checked on
**20260324** (addresses given as `616 / 324` where they differ). Builds on
`playfield_styling_research.md` (lane render path), `mine_render_architecture.md`
(custom-quad emission), and `shader_replacement_research.md` (GSPW/.gsp
replacement).

---

## 1. Verdict

**Feasible — and a true perspective renderer is cheaper than expected.**
Three findings from this study change the picture from "CPU-warp everything"
to "let the GPU do it":

1. **The 2D sprite pipeline carries a per-vertex `z` all the way to the GPU.**
   The command-list translator converts every sprite command into 0x18-stride
   vertices `{float3 pos, float2 uv, D3DCOLOR}` (§4). Quads get `z = 0`, but
   the `DRAWVERTICES` commands (tags 5/6) pass caller `z` through untouched
   (§4.3). The stock 2D vertex shader simply ignores `v0.z`
   (`mov o0, (v0.x, v0.y, 0, 1)` — `shader_replacement_research.md` §5).
2. **A replacement vertex shader can output real `w`** — the rasterizer then
   performs the perspective divide and **perspective-correct UV
   interpolation**. This eliminates the two hard problems of a CPU-side
   trapezoid transform: affine texture warp across quads, and the inability
   of one linear trapezoid to represent the hyperbolic compression of a long
   freeze body. Perspective projection preserves straight lines, so even a
   single full-lane freeze-body quad stays both shape- and texture-correct
   under a real-w VS.
3. **Shader delivery and selection need no new engine hooks.** `.gsp`
   containers support multiple programs per shader; the GSPW parser creates
   every program and stores the handles in an array (§5.2), and the
   `SetShader` command's second field is an **index into that array** (§5.1).
   A LayeredFS-extended `gs_screencommand_arrow.gsp` with a second
   (perspective) program is selectable per-pass from the DLL by rewriting
   the program field of the pass's emitted `SetShader` records in the list
   arena (§8, Option C1) — record emission/rewriting is the same mechanism
   `mine_render` already ships.

The recommended path (§8) is a two-phase build: a cheap CPU-trapezoid
prototype on the existing `playfield-styling` fill hook to validate feel and
culling, then the production custom-VS renderer.

---

## 2. The lane emission layer (recap + new findings)

The lane renderers emit into the active screen command list
(`list = *(DAT_1806f1fb8 + 0x40 + *(int*)(DAT_1806f1fb8+0x68)*8)`; fields:
`+0x0C` size, `+0x10` write ptr, `+0x18` arena base). Records are
`{u16 tag, u16 size, payload}`. Already documented for tags 0x11/0x13/0x04
in `mine_render_architecture.md`; this study adds the full tag map (§3).

### 2.1 Pass structure (per frame, per `ArrowRenderer`)

`render_notes` (`FUN_180026b00`, called from `ArrowRenderer::onDraw`
`FUN_180026a50`) runs:

1. per-pass helper `FUN_180028780`, collector `FUN_180024b40` (shock pass),
   shock draw `FUN_1800275b0`
2. collector again (normal pass), sort `FUN_180029070`, then the combined
   tap+freeze pass `FUN_1800278a0`

Collector record stride 0x28: `{dir i32@0, y1 f32@4, y2 f32@8, result*@0x10,
alpha1@0x18, alpha2@0x1C, alpha3@0x20}`; top-cull at collector+0xA6 against
the shared 720.0 (`DAT_18038eb38`) — the disp32-redirect from
`playfield_styling_research.md` §3 applies unchanged.

### 2.2 The shock pass (`FUN_1800275b0`)

Binds the **default** shader (`DAT_1806f0558`), one 0x34-byte quad per
collected note; UV column selected by panel-pair grouping
(`u0 = (variant*3+6)*0x20`), `set_direction(dir_group*3)` for rotation.
Fills the batch back-to-front (`sprite[count-1]` downward).

### 2.3 The tap + freeze pass (`FUN_1800278a0`) — decoded this session

Emits one `SetShader` (the arrow shader from `renderer+0xC0`) + ONE
`DrawRotateSprites` batch sized `count`, then fills it from **both ends**:
tap arrows from the back (`sprite[count-1]` downward), freeze quads from the
front (`sprite[0]` upward). Unused middle slots are zeroed at the end
(`FUN_18027b530` memset — zeroed 0x34 records rasterize as degenerate).

Per collected record, `len = *(int*)(note + 0x3C + dir*4)` (per-direction
freeze length array — confirmed):

- **`len == 0` (tap):** `set_direction(dir)`; palette column via
  `FUN_180028130(renderer, note_beat@note+4)` — two modes keyed off the
  arrow-color option field @ `renderer+0xE8` (values 0/5 → beat-division
  rows 1..4; otherwise a beat-cycling 1..4); UV = atlas cell
  `[0..96]×[0..96]`; color = `{R = paletteRowByte, 0, 0, A = missFade ·
  alpha1 · tint.a@+0x67}`. The **red byte is the palette row selector**
  consumed by the arrow PS (palette V = vertexColor.x) — the glyph cell is
  constant and color identity comes entirely from the palette row.
- **`len ≥ 1` (freeze):** head-y recomputed via `get_offset_y`
  (`FUN_180024a00(dBeat, speed@+0xA0 int×100, boost@+0xA4, music@+0xAC)`)
  with the per-direction hold counter (`result + 0x14 + dir*4`) pinning an
  active hold's head to the receptor row, then `floorf` (`FUN_180289af8`).
  Emits:
  1. **Head quad** — UV cell `[96..192]×[0..96]`, alpha2.
  2. **Long body segment** (only when `bodyLen = y2−y1 > 48.0`
     (`DAT_180399340` = 48.0)) — height `bodyLen − 48`, UV set via the
     px-space UV helper `FUN_180219400(this, u_px, v_px, w_px, h_px)` with
     `u_px = col·96 + 384.0` and `v_px = (1 − (bodyLen−48)/192)·192`, i.e.
     the **UV v-range equals the on-screen height in texels and goes
     negative for long bodies — the body TILES via the sheet's wrap
     addressing on a single quad at 1:1 texel density.** It is *not* a
     stretched quad. (Sheet sampler = wrap/wrap POINT —
     `playfield_styling_research.md` §7.)
  3. **Bottom cap quad** — height `min(96, bodyLen+48)`, art rows
     `[96..192]` of body column `col·96`, top-clipped for short bodies.
  - Body art column: `col = {0,0,1,2,2,1,3,3}[dir·2 + reverseFlag] & 3` —
    direction variants are baked into the art; body twist forced to 0.
  - Bodies route through the thin wrapper `FUN_180025860` =
    `freeze_body_fill(this, sprite, x, y, w, h, paletteRow_i32,
    alphaMult_f32)` which builds `color = {R = (row+0.5)·(1/16)·255, 0, 0,
    A = tint.a·alphaMult}` and calls the shared fill `0x180025900`
    (per-record alpha3 drives the body alphaMult).

**Consequence for perspective work:** a freeze is 1 (head) + 0–2 (body)
quads; the long segment can span most of the lane as ONE quad. Any CPU
linear warp of its 4 corners is doubly wrong (affine UV + linear-vs-
hyperbolic edge) — under a real-w VS it is exactly right with no
subdivision.

### 2.4 Receptors / judge flash / guideline

- SpotRenderer draw `FUN_180025e30`: one `SetShader` (spot shader @ +0xA0) +
  one tag-4 batch of 4/8 quads (mode @ +0x98), `set_direction(i)` + fill per
  panel, UV cell `[0..96]²`.
- JudgeEffectRenderer draw `FUN_180029290` and GuidelineRenderer
  `FUN_180026210` (+ private bulk emitter `FUN_18000c7b0`, which writes one
  **tag-1 DRAWSPRITES** record of 0x14-stride entries
  `{x, y, w, h, color}`): unchanged from `playfield_styling_research.md`
  §2/§4.

---

## 3. The screen command list — full tag map (verified from the walker)

The consumer is a worker-thread **walker** with an explicit tag switch:
`FUN_180269d40` / `FUN_18022dc40` (616/324; structurally identical). Records
flow: layer draws (`FUN_18002b530`, 11 layer slots @ `DAT_1806f1d20`, active
list index set per layer) → frame end (`FUN_180003010` → `FUN_1801efd00`
appends tag 0x1A to the 8 global lists at `DAT_1806f0620..` and kicks
`FUN_18026abe0`) → per-list job queue (`FUN_180272250` → 16-slot ring
`FUN_180272740` @ `DAT_1806f10c0`) → worker `FUN_180272980` → segment header
setup `FUN_18026c9f0` + the walker.

| Tag | Payload | Walker handler (616) | Meaning |
|---|---|---|---|
| 0x00 | `{u32, u8[4] rgba, u32, u32}` | `FUN_180267640` | Clear (forwards gd tag 0) |
| 0x01 | `{u32 count, u64 ptr}` → count × **0x14** `{x,y,w,h,color}` | `FUN_180267710` | DrawSprites (axis-aligned; guideline emitter uses this) |
| 0x02 | count × **0x24** `{x,y,w,h, u0,v0,u1,v1, color}` | `FUN_180267b80` | DrawSprites + UV rect |
| 0x03 | count × **0x24** `{x0,y0..x3,y3, color}` | `FUN_180268090` | Draw quads, untextured |
| 0x04 | count × **0x34** `{x0,y0..x3,y3, u0,v0,u1,v1, color}` | `FUN_1802684d0` | **DrawRotateSprites** (the lane path) |
| 0x05 | `{u32 prim, u32 count, u64 ptr}` → count × **0x18** `{x,y,z,u,v,color}` | `FUN_1802689b0` | **DrawVertices — carries per-vertex z** |
| 0x06 | `{u32 prim, u32 count, u32 stride, u64 ptr}` | `FUN_180268ae0` | DrawVertices, custom stride (memcpy + pos rewrite; z preserved) |
| 0x07 | `{f32 sx?, f32 sy?, f32 ox, f32 oy}` | `FUN_180268c40` | Set 2D context (offset/scale; zeroes ctx z) |
| 0x08 | `{u32 1, u32 blendBits}` | `FUN_180268d30` | Blend state (forwards gd 0x13 + 0x1F) |
| 0x0C | `{u16 enable, u16 x,y,w,h}` | `FUN_180269080` | Scissor (gd RS 0xAE + gd tag 0x18 rect) |
| 0x0F | `{f32[4]}` | `FUN_180269310` | Base color → **VS c22** (`ScreenCommandBaseColor`) |
| 0x10 | model draw | `FUN_1802693a0` → `FUN_18026c730` | 3D model path |
| 0x11 | `{u32 stage, u32 texId, f32[4]}` | `FUN_180269450` | SetTexture (gd tag 8) + the 4 floats → **VS c32+stage** (`SamplerParameters`) |
| 0x12 | `{u32 stage, u64 texPtr}` | `FUN_180269530` | SetTexture by object |
| 0x13 | `{u64 shaderObj, u32 programIdx}` | `FUN_180269620` | **SetShader — `programIdx` indexes the shader's program-handle array** (§5.1) |
| 0x14 | `{u32 regOff, u32 nRegs, u64 ptr}` | `FUN_1802696e0` | **SetVSConstantF(c48+regOff, nRegs)** — generic constant upload |
| 0x17 | `{u32 rtId}` | `FUN_180269880` | SetRenderTarget (gd tag 0xD; 0 = default `DAT_1806f226c`). Per-segment RT dims/constants (c13 + PS c1) are set at segment start via `FUN_18026c4d0`/`FUN_18026c440` |
| 0x18 / 0x19 / 0x1A | `{u64 sublist}` / — / — | inline | Call sub-list / return / end-of-list (16-deep stack at walker+0x38) |

Emitter-side APIs for tags 5/7/0x14 and friends live in a callback table
(static copy `0x180388940`, installed to `DAT_1804607e0` by `FUN_18021cc30`)
that is **registered with `libafp-win64.dll` via its Ordinal_3** — i.e. the
AFP/BM2D pipeline is the in-game consumer of the DrawVertices and matrix
paths. Notable entries: `FUN_18021a960` (DrawVertices emitter, chunked at
0xAA9 verts) and `FUN_18021b040` (projection helper: caller matrix or
`D3DXMatrixOrthoOffCenterRH`, composed with half-pixel correction, emitted
via the generic array emitter `FUN_18002af70` as a tag-0x14 upload). Scene
transforms (scale/offset) ride tag 0x07 (`FUN_1802178d0`).

**Do not conflate this tag set with the downstream gd/device tag set**
(0..0x3B) executed by `FUN_18024c310` on the render thread
(`FUN_18024d940`): there, 8/9 = SetSamplerState+SetTexture, 0xB =
SetStreamSource, 0xE = program select, 0x13 = alpha-blend RS, **0x2E =
SetVertexShaderConstantF** (device vtable +0x2F0), 5 = DrawPrimitive, 0x11 =
render-state toggles (ZENABLE 7 / ZWRITE 0xE / ALPHATEST 0xF / STENCIL 0x34
/ SCISSOR 0xAE). The walker produces this stream; mods never touch it.

---

## 4. What the GPU actually receives (the load-bearing facts)

### 4.1 Vertex format and CPU transform

Every 2D draw handler allocates from a shared batch
(`FUN_180267410`, gd tag 0xB stream bind, **stride 0x18**) and writes
vertices `{float3 pos, float2 uv, D3DCOLOR}`. Positions are CPU-converted:

```
ndc = (v * ctx.scale + ctx.offset) * NDC_SCALE + NDC_OFFSET
NDC_SCALE  = {2.0, -2.0, 1.0}   @ 0x18035a990 / 0x180358990
NDC_OFFSET = {-1.0, 1.0, 0.0}   @ 0x18035a9a0 / 0x1803589a0
```

with `ctx.scale ≈ {1/1280, 1/720, 1}` and `ctx.offset` from tag 0x07. So
**quads arrive at the VS already in NDC** — consistent with the stock arrow
VS's position passthrough.

### 4.2 Tag 4 quads: independent corners, rect UV, z pinned to 0

`FUN_1802684d0` emits each 0x34 quad as 6 vertices (two triangles,
TL-TR-BR / BR-BL-TL): corners are **fully independent** (arbitrary
quadrilaterals OK), UV is assigned per-corner from the min/max **rect only**,
and `z = ctx.offsetZ` (tag 0x07 zeroes it). A CPU-built trapezoid therefore
renders with **affine UV per triangle** — the classic diagonal-seam warp.

### 4.3 Tags 5/6: per-vertex z survives to the vertex stream

`FUN_1802689b0` / `FUN_18022c810` (616/324): input verts
`{x,y,z,u,v,color}` × 0x18; output `z' = (z·ctx.scaleZ + ctx.offZ)·1.0 + 0` —
with the tag-0x07 context that is **`z' = z`**. The declaration describes a
float3 position, so a replacement VS sees the caller's z in `v0.z`. The
stock 2D VS discards it — nothing in the stock pipeline breaks if a custom
pass emits meaningful z.

**This is the escape hatch:** emit tag-5 triangle lists (prim 4; the flush
`FUN_1801f25c0` maps prim→gd primitive count) with `z = along-track
distance`, and bind a custom VS that outputs
`o0 = (x_ndc·w, y_ndc·w, 0, w)` — the rasterizer divides by w and
interpolates UV perspective-correctly. `ps_3_0`-era hardware does this for
free; no depth buffer involvement (z output stays 0, ZENABLE stays off,
painter's order preserved).

### 4.4 Constants available to a custom shader

Per segment the translator uploads c0–c11 (two matrices + product), c12,
c14–c21 (`FUN_18026c9f0`) — unused by the 2D screencommand shaders. The
useful, mod-controllable channels: **c22** (tag 0x0F), **c32+stage** (tag
0x11's 4 floats), and the wide-open **c48+** window via tag 0x14
(`{regOff, nRegs, data}` — data is memcpy'd into the list arena by the
emitter, so a raw-record emission is self-contained). Perspective parameters
(anchor Y, focal length, per-side vanishing X, enable flags) fit trivially.

---

## 5. Shader-side facts

### 5.1 `SetShader` selects a program INDEX

The walker's tag-0x13 handler reads
`gdHandle = *(u32*)(*(u64*)(shaderObj + 8) + programIdx*4)` — the record's
second field indexes a per-shader **array of program handles**. All stock
lane emissions pass index 0.

### 5.2 The GSPW parser creates every program in the container

`FUN_18025ee30` iterates the program table (count byte @ hdr+0x18), dedupes
identical VS/PS index pairs, creates each unique program via `FUN_180256150`
(blob memcpy → render-thread CreateShader; see
`shader_replacement_research.md` §3), and stores the handles **in
program-table order** in the array at `shaderObj+8`. The `mdl_*` shaders
already ship 4 programs each — multi-program containers are a first-class,
exercised path.

**Consequence:** extending the (already LayeredFS-replaced)
`gs_screencommand_arrow.gsp` with a second program {perspective VS,
index-aware PS} gives the DLL a selectable perspective pipeline with **zero
loader hooks**: stock passes keep index 0 (byte-identical behavior when the
mod is off), and the mod's pass — or a bracketing detour around a stock pass
— emits `SetShader{arrowShader@renderer+0xC0, program=1}`. The existing
`shader_fixes` overlay and this extension are the same file; build both from
one source (a name-keyed overlay can only supply one `.gsp`).

---

## 6. Reimplementation inventory (what a fully custom pass must cover)

Everything below is already RE'd; repo code (`mine_render.rs`) exercises the
starred items today.

| Piece | Source of truth | Status |
|---|---|---|
| Results vector walk, note kinds | `renderer+0xB8` → vector; `note_types_expansion` | *shipped |
| Scroll math (NORMAL/BOOST/BRAKE/WAVE) | `get_offset_y` = `FUN_180024a00` (signature-resolved) | *shipped |
| Per-note collect windows | collector `FUN_180024b40` semantics + 720-window redirect (`playfield_styling` `cull_patch`) | *shipped |
| Quad fill semantics (reverse, appearance alpha, twist, posX/Y) | `render_sprite_final` `0x180025900`; `playfield_styling_research.md` §2 | *shipped hook |
| Direction → twist | `set_direction` = `FUN_180025c10` (writes `+0x50`) | *shipped |
| Tap palette column | `FUN_180028130(renderer, note_beat)` | this doc §2.3 |
| Palette-row color encoding | R byte = `(row+0.5)·(1/16)·255`; freeze rows via result state | this doc §2.3 |
| Freeze head/body/cap geometry + UV wrap tiling | `FUN_1800278a0`, wrapper `FUN_180025860`, UV helper `FUN_180219400` | this doc §2.3 |
| Shock pass UVs/rotation | `FUN_1800275b0` | this doc §2.2 |
| Receptor row | `FUN_180025e30` | this doc §2.4 |
| Judge flash grow math | `FUN_180029290` | prior docs |
| Guideline batch | `FUN_180026210` + `FUN_18000c7b0` (tag 1) | prior docs |
| Renderer fields | textures `+0x20/+0xD0/+0xE0`, shaders `+0xA0/+0xC0`, UV `+0x54`, tint `+0x64`, appearance `+0x6C..+0x78`, pos `+0x30/+0x34`, speed `+0xA0`, boost `+0xA4`, beats `+0xA8/+0xAC`, mode `+0xB0`, results `+0xB8`, judged `+0xEC`, offsetY `+0xF4`, vbptr(reverse) `+0x80` | prior docs + `mine_render.rs` |
| Raw record emission (0x11/0x13/0x04) | `mine_render.rs` CommandList helpers | *shipped |
| Raw record emission (0x05, 0x14) | formats verified this doc §3/§4 | new, mechanical |

---

## 7. Hallway math (for reference)

Let `y` = lane-relative scroll offset (0 at the receptor row, receptor
anchor `posY@+0x34`), `d(y) = y` treated as depth along the track, focal
`k`, per-lane center `cx`:

```
scale(y)  = k / (k + y)                        (hyperbolic, NOT linear)
y_screen  = posY + y·scale(y)                  (approaches horizon posY + k·… asymptote)
x_screen  = cx + (x_lane − cx)·scale(y)
```

- A note quad's 4 corners under this map form a trapezoid; straight lane
  edges stay straight (projective map) — but **screen-y spacing along the
  lane is hyperbolic**, so any single quad spanning a large `y` range (the
  freeze long segment) cannot be represented by linearly-interpolated
  corners.
- Reverse scroll: the fill negates `y` before adding `posY`; the transform
  commutes if applied to lane-relative coordinates (same argument as the
  shipped scale mod).
- **Cull window:** the visible track length becomes `y_max` where
  `y·k/(k+y) = 720 − posY` — unbounded as the horizon is approached, so the
  window must be clamped by a "draw distance" chosen with the option (SM
  exposes this implicitly). Delivered exactly like `playfield_styling`: the
  shared mod-owned float behind the collector/guideline disp32 redirects set
  to `y_max` per song. Density cost equals a slow speed-mod, same argument
  as `playfield_styling_research.md` §3.

---

## 8. Implementation options

### Option A — CPU trapezoid in the existing fill hook (prototype tier)

Transform each quad's `(x, y, w, h)` into 4 explicit corners in the
`playfield_styling` fill-hook path (the fill's output is the 0x34 record —
corners are independent, §4.2; the hook already owns this callsite; twist ≠ 0
quads need the rotation applied first, i.e. post-transform of the record
rather than the args).

- **Pros:** smallest delta; rides shipped infra (registry, side binding,
  cull redirect, mine inheritance); no shader work.
- **Cons (all inherent):** affine UV → diagonal-seam warp on every quad
  (tolerable at 96 px, ugly on freeze bodies); the freeze long segment is
  additionally shape-wrong (linear vs hyperbolic, §7) — fixing it means
  intercepting the freeze pass and re-emitting N subdivided quads; the
  receptor hit-flash/lane AFP clips don't follow (as with the scale mod).
- **Use:** validate feel, tune `k`, prove the extended cull window — then
  keep as the fallback rendering mode.

### Option B — full custom pass (mine_render pattern, everything)

Suppress the stock passes (the `render_notes` detour is already a
dispatcher), re-emit all taps/freezes/shocks (+receptors) via raw records
using §6. Freeze bodies subdivided (if tag 4) or emitted as tag-5 strips
with real z (if combined with C).

- **Pros:** total control (also unlocks other SM mods: multi-column effects,
  x-mods per note, note skins).
- **Cons:** largest surface; must keep bit-parity with stock passes for the
  off state; duplicated engine math beyond what `mine_render` reuses
  (freeze palette-row state machine, hold pinning, both-ends batch packing).
  Not needed for hallway alone.

### Option C — custom perspective vertex shader (recommended production tier)

Two sub-variants:

- **C1 — transform stock geometry in the VS.** Extend
  `gs_screencommand_arrow.gsp` with program 1 whose VS reconstructs
  `y_px` from incoming NDC (affine, exact), computes `scale(y)` per §7 with
  parameters from c48+ (tag 0x14), and outputs real w. Program selection:
  the stock passes emit their own `SetShader{…, program=0}` records
  mid-pass, so a naive before/after bracket is overwritten — instead the
  existing `render_notes` / fill detours **capture the pass's list window
  (write-ptr before/after the original) and rewrite the `program` field of
  the tag-0x13 records inside it to 1** (plus a constant upload ahead of the
  window). Records are consumed later on the worker thread, so post-hoc
  rewriting within the frame is safe. Per-side parameters (vanishing
  center, enable) keyed off the NDC x window inside the VS (the same
  640-split the mods use; doubles = one window) — this handles versus with
  one program.
  - Solves UV warp AND the freeze single-quad problem with **no geometry
    changes at all** — the stock CPU quads are already independent-corner;
    the VS's per-vertex w makes their interpolation projective.
  - Caveat: a quad is 4 samples of the curve — its **edges** are straight
    while the true track edge between head and cap of a *very* long body is
    the only place the hyperbola matters… and that IS per-vertex-correct
    because the body's corners sit at the endpoints and the interior is
    projectively interpolated (correct: a planar quad under projection).
    Net: geometrically exact for quads lying flat on the track plane.
- **C2 — custom pass emits tag-5 vertices with real z** + program 1 VS that
  does a plain projective transform of `(x_lane, z)`; combine with a partial
  Option B (freeze/tap emission only). More moving parts than C1 for the
  same visual result; keep in reserve for effects C1 can't express
  (e.g. per-note rotation into the screen plane).

**Effort estimate:** C1 = two shader programs (arrow + default `.gsp`, built
by the shipped `build_shaders.sh`/`gsp_pack.py` toolchain), ~4 constant
uploads/frame, a pass-window record rewrite in existing detours, cull-window
reuse. Meaningfully *less* new RE-risk code than the playfield_styling
lane_hook was.

### Companion work (any option)

- Lane AFP clips (filter band, covers, danger) stay rectangular — a hallway
  lane wants a trapezoid there. Not expressible via the AFP matrix RMW
  (affine only). Accept, narrow to receptor width via the shipped
  `lane_hook` horizontal scale, or hide the filter during hallway.
- Judge font (`gs_screencommand_judge`) and combo are screen-anchored
  overlay elements in SM too — leave untouched.
- Guideline: tag-1 records have no UV; a trapezoid guideline needs either
  the tag-3 quad form or C1-style shader treatment of… nothing — guidelines
  draw with the **default** shader, not the arrow shader. Simplest: transform
  the 0x14 records CPU-side in the shipped emitter detour (guidelines are
  3 px tall — affine is irrelevant), with `y' = posY + y·scale(y)` and
  x/w converged.
- Mines (`mine_render`) call the shared fill → inherit Option A transforms
  automatically. Under C1 the story is shader-scoped: the record rewrite
  targets tag-0x13 records by shader object, and mines bind the **default**
  shader (glyph) + their own texture (overlay) — so hallway mines need the
  same program-1 treatment in `gs_screencommand_default.gsp` (and the shock
  pass, which also binds the default shader, gets it for free). C1's window
  capture extends the existing `render_notes` detour (one-detour-per-target:
  that detour is `mine_render`'s — shared-dispatcher extension, not a new
  hook).

---

## 9. Dead ends & non-starters verified

- **Stock path cannot do perspective UV**: quads reach the GPU pre-flattened
  (`z=0`, VS forces `w=1`), and tag-4 UV is a rect — no per-corner UV
  channel exists in the 0x34 record. CPU-side "perspective" is affine-only.
- **The tag-0x14 constant window is NOT a projection override for the 2D
  path**: it writes c48+; the 2D screencommand VS reads only c22/c32. Only a
  replacement VS makes those constants matter. The per-segment matrices
  (c0–c11) come from segment headers, not from any lane-reachable API.
- **Piggybacking the 3D model pipeline** (tag 0x10 → `FUN_18026c730` /
  `FUN_18026cf80`, real matrix uploads at c48+0x30): requires model-object
  containers, per-object visibility flags, and the model vertex/decl
  registry — far heavier than the tag-5 + custom-VS route for identical
  visual capability. Deprioritized, not disproved.
- **Depth-buffer sorting for the lane** (gd tag 0x11 ZENABLE exists): the
  palette-indexed art relies on alpha blending; enabling Z for the lane pass
  would need draw-order-independent alpha, which it can't have. Painter's
  order (far-to-near = collector order top-down) is the correct tool; the
  stock sort `FUN_180029070` already orders the batch.
- **Editing the 720.0 constant**: still shared by 14 readers — the
  disp32-redirect remains the only sanctioned mechanism (shipped).
- **Guideline via the arrow shader**: guidelines bind the default shader and
  use tag-1 (no UV) — shader-side treatment can't see them distinctly;
  CPU transform in the existing emitter detour is the right tool.

## 10. Key addresses (20260616 unless noted)

| What | Address |
|---|---|
| render_notes / onDraw | `FUN_180026b00` / `FUN_180026a50` |
| collector (×2/frame) | `FUN_180024b40` (324: `FUN_1800240c0`) |
| shock pass / tap+freeze pass | `FUN_1800275b0` / `FUN_1800278a0` |
| shared quad fill / freeze-body wrapper | `0x180025900` / `FUN_180025860` |
| px-space UV helper | `FUN_180219400` |
| beat→palette column | `FUN_180028130` |
| get_offset_y / floorf | `FUN_180024a00` / `FUN_180289af8` |
| spot / judge / guideline / guideline emitter | `FUN_180025e30` / `FUN_180029290` / `FUN_180026210` / `FUN_18000c7b0` |
| batch sort | `FUN_180029070` |
| **walker (tag switch)** | `FUN_180269d40` (324: `FUN_18022dc40`) |
| tag-4 / tag-5 handlers | `FUN_1802684d0` / `FUN_1802689b0` (324: `FUN_18022c330` / `FUN_18022c810`) |
| tag-0x13 SetShader handler (program index) | `FUN_180269620` (324: `FUN_18022d480`) |
| tag-0x14 VS-const handler / VS-const writer | `FUN_1802696e0` / `FUN_18026bf70` |
| vertex batch alloc (0x18 stride) | `FUN_180267410` (324: `FUN_18022b270`) |
| draw flush (prim map) | `FUN_1801f25c0` |
| segment ctx + matrix upload | `FUN_18026c9f0` |
| frame submit chain | `FUN_180003010` → `FUN_1801efd00` → `FUN_18026abe0` → `FUN_180272250` → `FUN_180272740` → worker `FUN_180272980` |
| gd executor / render thread | `FUN_18024c310` / `FUN_18024d940` |
| screen renderer global / ctor | `DAT_1806f1fb8` / `FUN_180213450` |
| layer dispatcher (11 slots) | `FUN_18002b530`, `DAT_1806f1d20` |
| emitter callback table (static → live) | `0x180388940` → `DAT_1804607e0` (`FUN_18021cc30`; registered with libafp Ordinal_3 in `FUN_18025cd10`) |
| DrawVertices emitter / matrix emitter / array emitter | `FUN_18021a960` / `FUN_18021b040` / `FUN_18002af70` |
| GSPW parser (program array @ shaderObj+8) | `FUN_18025ee30` |
| NDC scale/offset consts | `0x18035a990` / `0x18035a9a0` (324: `0x180358990` / `0x1803589a0`) |
| cell consts 384/192/48 | `0x180399338` / `0x18039933c` / `0x180399340` |
| default shader handle | `DAT_1806f0558` |
