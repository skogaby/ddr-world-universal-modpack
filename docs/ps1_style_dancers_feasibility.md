# PS1-Style Background Dancers — Feasibility (2026-09-23)

**Question.** Can the Background Dancers scene (dancers + stage) be rendered with a PlayStation-1
look — low resolution, jagged edges, vertex jitter, affine texture wobble, point-sampled textures,
dithered 15-bit colour — ideally paired with DDR SELECTION's 1st–5th Mix skin?

**Scope.** Feasibility only; nothing here is implemented or cabinet-tested. Everything below is
grounded in the shipped Background Dancers / shader-fixes machinery and its RE notes
(`docs/background_dancers_research.md` §4–§8, `docs/custom_resolution.md` §3a,
`docs/arbitrary_resolution_research.md` §3–§5, `docs/custom_arrow_renderer_research.md` §3).
Addresses follow those notes' conventions (file-relative, build named where it matters).

## TL;DR

**Feasible. The work splits into two tiers, and the first tier is cheap.**

| Tier | What it gives | Mechanism | New engine RE | Detours |
|---|---|---|---|---|
| **1 — `_ps1` shader variants** | vertex jitter, affine texture wobble, nearest/low-res textures without mips, 15-bit colour with the PS1's own ordered dither, Gouraud (per-vertex) lighting, optional depth-cue fog | a THIRD style-variant family next to `_lit` / `_cel`: 9 more synthesized containers + the existing material re-point | **none** | none |
| **2 — true low resolution** | real low-res pixels and stair-stepped silhouettes (the "jagged edges"), for the whole 3D image | a mod-owned viewport at the head of the RENDER_2D target list issuing two engine **StretchRect** records (POINT): full frame → e.g. 426×240 → full frame | 4 small items (§6 R1–R4) | none |

- Tier 1 alone reads like a PS1 emulator running at a high internal resolution: wobbly, jittery,
  chunky-textured and dithered, but with smooth polygon edges. On AA-3 cabinets (every stock HD
  cabinet and spice2x) the stock `sys_copy_aa` edge blur softens those edges further.
- Tier 2 supplies the resolution and the edges. Point-decimating the finished full-resolution 3D
  frame gives the same image as rasterizing at the low resolution with one sample per block
  (§4.2), so the scene never has to be *rendered* small. It runs before any 2D list, so lanes, HUD
  and options previews stay crisp.
- No shader can do these: fewer polygons (stock dancer bodies are 3.5k–4.7k triangles; a PS1
  character was a few hundred), and the PS1's lack of a Z-buffer (its polygon-sorting glitches).
  The first has a content route (§1, row 9). Nobody wants the second.
- **DDR SELECTION pairing works without new ordering constraints.** The dancers decide the style
  when the parse lands (`lifecycle::drive_live` → `drive_assets` → `style::effective()`). That is
  after DDR SELECTION armed the skin at the 25→26 edge, so `ddr_selection::armed_skin() == 1` is
  already valid there. The period fits: 1st–5th Mix (and MAX–EXTREME) ran on Konami's System 573,
  which is PlayStation-based hardware.

## 1. The PS1 look, effect by effect

| # | Effect | Why the PS1 did it | Technique here | Tier | Verdict |
|---|---|---|---|---|---|
| 1 | **Vertex jitter / snapping** | the GTE wrote integer screen coordinates (no sub-pixel precision) | VS: snap `clip.xy / clip.w` to a virtual-pixel lattice, multiply back by `w` | 1 | trivial |
| 2 | **Affine texture wobble** | no perspective-correct interpolation | VS outputs `(uv·w, w)`, PS divides (§3.3). Perspective-correct hardware then interpolates `uv` linearly in *screen* space | 1 | trivial; strong on the stage's big polygons, subtle on dancers (small triangles) |
| 3 | **Nearest-neighbour, low-res textures, no mipmaps** | no bilinear, small VRAM pages, no mips | PS: snap `uv` to a fixed texel grid (e.g. 128 across the texture) + `tex2Dlod(…, 0)`. SM3 has no in-shader sampler state; snapping emulates POINT exactly | 1 | trivial. Stock body textures are 512² with 3 mips, faces 128² |
| 4 | **15-bit colour + ordered dither** | 5:5:5 frame buffer, 4×4 dither on shaded polygons | PS: add the PS1 GPU's own 4×4 dither offsets (psx-spx: `-4 0 -3 1 / 2 -2 3 -1 / -3 1 -4 0 / 3 -1 2 -2`, in 8-bit units) at the **virtual**-pixel coordinate, truncate to 5 bits | 1 | trivial |
| 5 | **Gouraud lighting** | per-vertex lighting only | reuse `mdl_lambert`'s `lit_factor` in the VS (it is already per vertex) | 1 | trivial. True flat shading is `D3DRS_SHADEMODE`, engine state, and is not needed |
| 6 | **Depth-cue fog** | fade to a colour with distance | VS fog factor from `clip.w`, PS lerp | 1 | trivial, optional |
| 7 | **Low render resolution** (320×240-class) | the frame buffer | downsample + upscale of the 3D image (§4) | 2 | feasible, 4 RE items |
| 8 | **Jagged polygon edges** | no AA at 240p | follows from 7. Tier 1 alone keeps native-resolution edges, softened by `sys_copy_aa` on AA-3 | 2 | follows from 7 |
| 9 | **Low-poly models** | budget | not a shader problem. Route: decimate a model in Blender and export with the add-on into `data_mods/custom_models/` (personal installs only; stock-derived meshes are never committed) | — | content, optional |
| 10 | No Z-buffer (ordering-table sort glitches), whole-polygon near-plane dropouts | hardware | would need per-triangle sorting and would break the dancers | — | not feasible, and not wanted |
| 11 | 4/8-bit CLUT textures | VRAM | approximated by effect 4 (5-bit quantize + dither). Exact palettes = offline texture conversion | — | approximate |
| 12 | Composite video / CRT look | display | out of scope; could ride a shader-based upscale later (§4.6) | — | later |

## 2. Where it plugs in — machinery that already exists

- **Style variants per song, no detour** (`background_dancers_research.md` §4.7). The shader
  synthesis (`services/avs_layeredfs/shader_synthesis.rs`) emits `<name>_lit` and `<name>_cel`
  for the 9 model shader NAMES the scene uses (`shader_layout::MODEL_VARIANTS`). At build time
  the dancers mod re-points each render item's PRIVATE material copy (`mat+0x20` =
  `gs::Shader*`) at `lookup(fnv1("<name>_<style>"))`. Nothing in the draw path re-resolves.
  A `_ps1` family is the same pattern with one more suffix.
- **The 9 names cover the shipped custom content too.** `ktmdl_dump.py` over
  `data_mods/custom_models/`: every material of Peter Griffin / Kasane Teto / Hatsune Miku uses
  `mdl_ch_constant_vc`, and the Griffin House room uses `mdl_bg_constant_vc`.
- **Register map** (`shaders/src/mdl_common.hlsli`). VS: c14–c17 World, c18–c21 WVP, c22
  ModelParameters, c23 tint, c24 `m_vTexAnime`, c25/c26 const/offset colour. PS: s0 texture,
  s15 stipple, c2 ModelParameters. Skinning (s3 bone texture), `to_clip`, `anim_uv` and
  `view_frame` are reusable as-is.
- **A free per-item channel.** `ModelParameters.w` (`item+0x4C`, VS c22.w / PS c2.w) is read by
  no stock shader and is written by the DLL only on hull twins (`render_item::set_outline_width`).
  The PS1 look has no hulls, so `.w` can carry per-item PS1 parameters (affine strength, lighting
  on/off) chosen per instance kind at build time.
- **Container budget.** The shader registry holds 256 objects; the stock arc uses 36, the current
  variant set adds 18, and 9 more is nothing (RE §4.1).
- **Frame structure** (RE §5.1, §7.2; `custom_resolution.md` §3a). Target lists run as OFFSCREEN1
  (0x65) → RENDER-3D {0x64, 0x65 OFFSCREEN0 = the fullscreen movie, 0x66–0x68 MODEL passes, 0x6D}
  → AFTER-RENDER-3D (AA-3: the in-place `sys_copy_aa` edge-blur overlay; AA-0: StretchRect
  RENDER → `render_color`) → **RENDER_2D {0x65 BACK, 0x66 MIDDLE, 0x67 FRONT}** → DISPLAY →
  PRESENT.

## 3. Tier 1 — the `_ps1` variant family

### 3.1 Containers

- For each of the 9 `MODEL_VARIANTS`: `<material_name>_ps1` = our PS1 VS + our PS1 PS. The
  program table is 4×`(0,0,0)` (`shader_layout::model_programs(false)` — no hull slot).
- The PS1 PS must **replace** each name's stock PS, because the affine divide happens in the pixel
  shader. It therefore reproduces each shape's stock colour equation, which is exactly what the
  cel PS already does (RE §4.7):
  - `gs_model_default` (the dancers' `lambert` fallback): `tex × COLOR0` + the 32×32 stipple dissolve.
  - `mdl_*_constant*`: `tex × COLOR0` from TEXCOORD3.
  - `_c`: `rgb·c4.rgb + c5.rgb`.
  - `_notex`: `COLOR0`, with const/offset colour applied in the VS.

  Define-driven like `mdl_cel.hlsl` (`VCOLOR`, `CCOLOR`, `NOTEX`, `STIPPLE`, plus `LAMBERT` for the
  two dancer fallbacks, which get Gouraud lighting).
- Build: new `mdl_ps1.hlsl` entries in the `scripts/build_shaders.sh` manifest (vs_3_0 / ps_3_0).
  `shader_layout` gains a `ps1_vs` / `ps1_ps` column (or a parallel table). Bump the synthesis
  fingerprint `v7 → v8` so cabinets re-synthesize.
- Deploy the blobs WITH the DLL. A DLL-only deploy carries no `data_mods/shader_fixes/blobs/`, and
  missing variants degrade to stock with one WARN (the `variants_available()` gate).

### 3.2 Vertex shader (sketch)

```hlsl
// after skinning: ps/ns in model space, uv, vcol
float4 clip = to_clip(ps);
float  w    = clip.w;
// (1) snap to the virtual-pixel lattice (VIRT = e.g. 426x240); even VIRT => the
//     screen centre is a lattice line, so vertices land on virtual-pixel corners.
float2 half_grid = 0.5 * float2(VIRT_W, VIRT_H);
float2 ndc  = clip.xy / max(w, 1e-4);
float2 snap = floor(ndc * half_grid + 0.5) / half_grid;
clip.xy = lerp(clip.xy, snap * w, step(1e-4, w));    // leave behind-camera verts alone
o.pos  = clip;
// (2) affine UV: the PS divides; hardware interpolates (uv*w)/w and w/w
//     perspective-correctly, and the ratio is uv interpolated linearly on screen.
o.uvw  = float3(anim_uv(uv) * w, w);
o.uvp  = anim_uv(uv);                               // perspective copy (strength lerp)
o.scr  = float3(clip.xy, w);                        // screen position; PS divides
// (3) Gouraud: LAMBERT shapes multiply lit_factor(to_world_normal(ns)) in, gated by
//     the per-item lighting flag packed into ModelParameters.w
o.col  = Tint * vcol * lerp(1.0, lit_factor(to_world_normal(ns)), item_lit);
```

The affine trick is exact under D3D9's interpolation rule. For vertex attribute `a`, the
rasterizer produces `Σλᵢ(aᵢ/wᵢ) / Σλᵢ(1/wᵢ)` (λ = screen barycentrics). With `a = uv·w` the
numerator becomes `Σλᵢ·uvᵢ`; with `a = w` it becomes `Σλᵢ`. Their ratio is the screen-linear
(affine) UV. The same rule makes `o.scr.xy / o.scr.z` the exact per-pixel NDC, so the PS gets a
resolution-independent screen position without knowing the render size (§3.6). Clipping
preserves both results, because it interpolates in clip space.

### 3.3 Pixel shader (sketch)

```hlsl
float2 uv  = lerp(i.uvp, i.uvw.xy / i.uvw.z, item_affine);   // per-item strength
uv = (floor(uv * TEX_GRID) + 0.5) / TEX_GRID;                 // nearest texel, TEX_GRID^2 page
float4 tex = tex2Dlod(Material, float4(uv, 0, 0));            // LOD 0: no mips, PS1-style shimmer
float3 rgb = tex.rgb * i.col.rgb;                             // (+ CCOLOR / NOTEX shapes)
float2 vp  = floor((i.scr.xy / i.scr.z * float2(0.5, -0.5) + 0.5) * float2(VIRT_W, VIRT_H));
rgb = floor(saturate(rgb) * 255.0 + psx_dither(vp)) / 8.0;    // PS1 table, 8-bit units
rgb = saturate(rgb / 31.0);                                   // 5:5:5
return float4(rgb, tex.a * i.col.a);                          // alpha untouched
```

- `psx_dither(vp)` selects one of the 16 offsets with arithmetic. The table lives in four `def`
  float4 rows; one-hot vectors from `fmod(vp, 4)` pick the entry via dot products. ps_3_0 allows
  relative constant addressing only inside loops, and the CrossOver D3DMetal rule (see the
  mod-menu themes) is to keep flow control shallow. The whole PS stays branch-free and loop-free.
- **Alpha is never dithered or snapped**, so the mesh-flag alpha test (threshold 127) and blend
  groups behave exactly as authored.
- The fixed `TEX_GRID` needs no texture size. Across a 512² body atlas, a grid of 128 gives 4×4
  source texels per PS1 texel; 128² faces stay native. Per-texture exact grids are possible
  (`TextureData {hash, handle, u16 w, u16 h}` is known at build time) but need a shader-visible
  slot for the size (§6 R6).

### 3.4 Which materials get restyled

The `_lit`/`_cel` rule restyles only blend-group-0 materials, and never the shadow or a `_bg`
skydome part (`render_item_layout::restyle_eligible_materials`, `instance_plan::restyle_allowed`).
Lighting must not change a glow's authored look. That rule is wrong for PS1. A monitor stage draws
the same 380 vertices once opaque and once as an ADDITIVE glow copy (RE §8.2); snapping one copy
and not the other would visibly tear them apart. PS1 restyles **every** material, with each
shape's stock colour math kept:

- Lighting only on the two `lambert` fallbacks (the dancers' bodies and parts), and only when
  enabled. The stage's `constant*` materials are baked/emissive and stay unlit, as authored.
- Affine strength per instance kind, via `.w`: full on props and dancers, low or zero on a `_bg`
  skydome, whose huge triangles would swim.
- Hulls off. PS1 had no outlines, and the hull VS does not snap.

A pure, host-testable `ps1_eligible(kind, model_name, record_flags)` next to the existing rules
(`validate_background_dancers.sh`).

### 3.5 Tunables — where each can live

| Knob | Default idea | Home |
|---|---|---|
| virtual resolution (snap lattice, dither grid) | 426×240 (240 lines ≈ PS1); 320×180 as the integer-everywhere option (§3.6) | `def` literal baked at synthesis (precedent: shader-fixes' baked texel sizes). Synthesis runs when `shader.arc` opens, so **next launch** |
| texel grid | 128 | same |
| dither + 15-bit on/off | on | same (or two PS variants) |
| affine strength, dancer lighting | per kind / per option | `ModelParameters.w`, written at build, so **per song** |
| jitter on/off | on | baked, or fold into `.w` |

A truly live constant would need either a spare material-parameter slot in the private material
copy (§6 R6) or a constant emission into the model pass (a detour, RE §4.5). v1 needs neither.

### 3.6 Virtual resolution vs. the render size

The lattice is defined in NDC, so Tier 1 does not care about the render size (`custom_resolution`
720p / 1080p / 1440p / 4K, or the stock 1280×720 SD render). Virtual lines per render height:

| Render height | 180 lines | 240 lines | 360 lines |
|---|---|---|---|
| 720 | 4 px | 3 px | 2 px |
| 1080 | 6 px | **4.5 px** | 3 px |
| 1440 | 8 px | 6 px | 4 px |
| 2160 | 12 px | 9 px | 6 px |

The fractional block only matters for Tier 2 (uneven 4/5 px blocks on 1080p). Offer 240 as the
authentic default and 180/360 as integer-exact alternatives.

### 3.7 What Tier 1 alone looks like — honestly

Jitter, wobble (dramatic on stage floors/walls, mild on dancers), chunky mip-less textures and
PS1 dither are all genuinely there. The resolution is not. Triangle edges and silhouettes stay
native-resolution, and on AA-3 cabinets the stock `sys_copy_aa` pass (a texkill-gated
edge-directed blur, RE: `.agents/planning/2026-09-05-arbitrary-resolution/prototypes/shader_dump/REPORT.md`
§5) smooths them after the model passes. The fair comparison is "a PS1 emulator at 4× internal
resolution with nearest texture filtering and no geometry correction".

Optional fallback when Tier 2 is unavailable: evaluate the texture and the dither at the
**centre of the virtual pixel** in the PS. Extrapolate `uv` with `ddx`/`ddy` by the NDC offset to
the block centre; this is exact for the screen-linear affine UV. That gives blocky texels inside
each triangle, but blocks straddling a triangle edge split in two and silhouettes stay smooth.
It is worth having only as a degraded mode.

### 3.8 Cost and hazards

- A handful of VS ALU and ~25 PS ALU plus one fetch, on background geometry only. Negligible next
  to `sys_copy_aa` (57 instr, 9 texld, full screen).
- Guard `w ≤ 0` in the snap (above). Z is untouched, so depth behaviour, z-fighting and the
  alpha test are unchanged.
- The options-menu previews (RE §5, a 170×150 box viewport) get a lattice relative to the box. At
  240 lines over 150 px the snap is sub-pixel, so previews show little of the effect. That is
  cosmetic only; exempt previews or accept it.

## 4. Tier 2 — true low resolution by downsample + upscale

### 4.1 Mechanism

A mod-owned viewport object shaped like the cabinet-proven `ClearViewport`: a 0x40
`gs::Viewport::Base` header, flags `+0x24 = 2` (skip camera upload), and a 2-slot vtable whose
slot 0 runs on the render worker under the `node_visit` rules (RE §5.3). It is attached into the
**RENDER_2D target list** (`display+0x38`) at a priority **below 0x65**, so it runs before BACK.
Its slot-0 callback appends **two gd `0x31` StretchRect records** at `*(workerCtx+0x218)`:

1. `StretchRect(target colour, full rect → lowres RT, {0,0,VIRT_W,VIRT_H}, POINT)`
2. `StretchRect(lowres RT → target colour, full rect, POINT)`

`lowres` is one small render-target surface created once. `target colour` is read at runtime from
the RENDER_2D list's own target (`list+0x38` → RT struct `+0x08` colour id): `render_color` on
AA-0, `display` on AA-3 (`custom_resolution.md` §3a). The viewport's DISABLED bit (flags bit 0)
is toggled from the game thread each frame: set unless the dancers scene is visible AND the PS1
look is active for the song. This is the `PassSet` pattern.

### 4.2 Why this is enough — decimation ≡ low-res rasterization

A POINT StretchRect from W×H down to VIRT_W×VIRT_H keeps one full-resolution pixel per block.
That pixel's colour is the scene's coverage and shading sampled at one point inside the block,
which is what a VIRT-resolution rasterizer computes at its pixel centre. The only differences:

- The sample sits a fraction of a full-res pixel off the block centre.
- Mip LOD would be chosen at full resolution, which is moot because Tier 1 samples LOD 0.
- The GPU shades 9–16× more pixels than needed, which is irrelevant for background dancers.

Thin features vanish and reappear, silhouettes stair-step, and a sub-block triangle either covers
the sample point or not, exactly like 240p. With Tier 1's lattice equal to VIRT, polygon edges
land on block boundaries and the dither is constant per block, so the two tiers compose exactly.

### 4.3 Why RENDER_2D, before BACK

- The 3D image is complete there on both AA modes. The MODEL passes, the fullscreen movie
  (OFFSCREEN0) and AFTER-RENDER-3D have all run.
- Crucially, **AA-3's `sys_copy_aa` has already run**, so it cannot soften the blocks. If the
  pixelation ran inside RENDER-3D instead, that edge blur would round off the staircase.
- No 2D has been drawn yet. Lanes, arrows, HUD, the thumbnail movie (a 2D layer, RE §7.2) and the
  options-modal previews (RENDER_2D 0x68+) all stay full resolution.
- The RENDER_2D list's own clear touches depth only (`custom_resolution.md` §3a), so the colour
  arrives intact.

### 4.4 Pieces and status

| Piece | Status |
|---|---|
| Attach/detach a mod viewport into a target list | proven (`viewport_pass`, RE §5.1) |
| Append gd records from a viewport's slot-0 callback | proven (`ClearViewport`, the gd Clear record, RE §5.3) |
| gd `0x31` = `IDirect3DDevice9::StretchRect(src, srcRect, dst, dstRect, filter)` with a FILTER field | engine uses it every frame on AA-0 (BEGINVIEWPORT) and in COPYVIEWPORT with filter 1 = POINT / 2 = LINEAR (`arbitrary_resolution_research.md` §5.1–§5.2). **Record byte layout: R1** |
| A small colour render-target surface | `surface_create(w, h, fmt, msaa 0, &opts) -> id` (`FUN_180250950` on 20260825) — the idiom custom-resolution's retired depth swap used, cabinet-proven inside `graphics_init`'s post-original. **Callable post-boot? R2** |
| RENDER_2D colour surface id at runtime | target list `+0x38` → RT struct `{+0x08 colour id, +0x10 depth id, +0x14/+0x16 dims}`. **Confirm AA-3 = `display`: R3** |
| POINT StretchRect with scaling under CrossOver / D3DMetal | the engine's POINT copies are 1:1; scaled POINT is untested. **R4** (one smoke build) |

Cost: one ~57k-pixel downsample plus one full-screen upscale write per visible frame. That is the
order of ONE of the extra blits the AA-0 chain already pays (`custom_resolution.md` §3a), and far
below the `sys_copy_aa` pass.

### 4.5 Consequences to accept (or design around)

- **The fullscreen movie is pixelated too.** It is part of the RENDER-3D image (OFFSCREEN0 at
  0x65). The RENDER colour surfaces are X8R8G8B8 (fmt 0x16), so no alpha channel can mask it out.
  For the "dancers over a 5th Mix movie" look this is arguably on-theme; if not, see 4.6's
  alternative (2b).
- **The whole 3D scene** (stage, dancers, shadow, skydome) is pixelated together. That is the
  authentic PS1 behaviour; "pixelated dancers on a sharp stage" needs 2b.
- 1080p with 240 lines gives uneven 4/5 px blocks (§3.6).

### 4.6 Alternatives considered

- **2b — models into a separate low-res target, then composite.** Clone the MODEL passes into
  OFFSCREEN1's list or a new target, stamp the items with the last free node-mask bit `0x80`
  (RE §5.2), and composite with a quad. This keeps a fullscreen movie sharp and allows dancers-only
  pixelation, but it needs:
  - a depth surface ≥ the target (OFFSCREEN1's RT struct has none; create one and bind it via the
    depth-swap idiom);
  - a composite draw with alpha;
  - an answer to the stage-monitor movie route, which wants OFFSCREEN1 itself (RE §8.6).

  Defer until 4.5 is actually a problem.
- **2c — shrink every RENDER-3D viewport to a corner, then upscale.** A true low-res raster with
  no new depth, but it must shrink OFFSCREEN0 and the MODEL passes consistently. On AA-3, the edge
  blur would then run over the tiny corner image and smear whole virtual pixels unless
  BEGINVIEWPORT is disabled. It is more invasive and gains nothing over 4.2.
- **Shader upscale instead of the second StretchRect.** Sample the low-res surface as a texture
  with a mod PS (needs a texture view of the surface plus a quad draw). This opens CRT, scanline
  and palette post effects. It is an extra, not a requirement.

## 5. Pairing with DDR SELECTION's 1st–5th Mix skin

- **Period fit.** DDR 1st Mix through EXTREME ran on Konami System 573 (PlayStation-based), so a
  PS1 rendering look is era-correct for skin 1, and arguably skin 2 (MAX–EXTREME). The
  maintainer's FULLSCREEN movie mode (dancers over the movie, no stage, "the DDR 5th Mix look",
  RE §7) is the natural companion.
- **Decision point.** `style::effective()` is evaluated in `lifecycle::drive_live`'s
  `drive_assets` closure when the parse lands. That is after `ddr_selection` wrote the skin at the
  25→26 edge, so:

  ```text
  retro = mode == Always || (mode == WithSelection && ddr_selection::armed_skin() ∈ {1})
  ```

  is correct per song with no new scene-callback ordering. Consequences:
  - Course and event chains stay stock (DDR SELECTION refuses them), so only ALWAYS applies there.
  - Versus is one skin (DDR SELECTION is versus-mirrored, P1 governs), so it is one scene.
  - Options previews at scene 25 see skin 0 and show the base style, unless ALWAYS.
- **Operator surface (proposal).** GLOBAL SETTINGS, under the BACKGROUND DANCERS header:
  - "RETRO 3D LOOK" = OFF / WITH 1ST-5TH MIX / ALWAYS, per song.
  - Children: "RETRO RESOLUTION" 240 / 180 / 360 LINES (next launch, §3.5), "RETRO DANCER
    LIGHTING" UNLIT / GOURAUD (per song).
  - While retro is active: outlines forced off, and LIGHTING STYLE CEL treated as GOURAUD (per-pixel
    bands contradict the look).
  - Tier 2 on whenever retro is active and its derivation resolved; otherwise Tier 1 only, with
    one WARN.

## 6. Open items / RE spikes

| # | Question | How to answer | Blocks |
|---|---|---|---|
| R1 | gd `0x31` record layout (size, field order, rect encoding, filter enum) and the gd buffer's capacity rules for two ~0x30-byte records from one callback | decompile COPYVIEWPORT's emitter (`FUN_1801f4790` in the arbitrary-resolution notes' build) and the executor case `0x31` (device vtbl +0x110); sweep an AOB on all five builds | Tier 2 |
| R2 | Is `surface_create` safe from the game thread after boot, or only inside `graphics_init`? | decompile for render-thread or device-lock assumptions. Fallback: create inside `graphics_init`'s post-original. custom-resolution owns that detour, so it becomes a shared dispatcher (one-detour rule) | Tier 2 |
| R3 | RENDER_2D target colour id per AA mode | read `list+0x38` → RT struct at runtime; log once per boot; cross-check AA-0 vs AA-3 | Tier 2 |
| R4 | Scaled POINT StretchRect on CrossOver/D3DMetal and real D3D9 | one smoke build (dev knob, like `DDR_DANCERS_VIEWPORT_SMOKE`) | Tier 2 |
| R5 | Do model-pass segments also get VS c13 / PS c1 = viewport rect (proven for ScreenCommandList segments by `sys_copy_aa`)? | `/dumpbin` is useless here; a probe PS that outputs `c1`. Nice to have (§3.2's NDC interpolant already avoids needing it) | nothing |
| R6 | Spare parameter capacity in the 0x168 private material record (bump `param_count`, put texture dims / live tunables in c27+/PS c6+) | decompile the converter's param copy and the bind's upload bound | only live tunables / exact per-texture grids |
| R7 | Where model-texture sampler filters come from (a per-slot flag the private copy could set to POINT) | model-pass bind path | nothing (shader snapping suffices) |
| R8 | Reproduce every stock PS equation exactly (stipple on the `gs_model_default` shapes only, TEXCOORD3 on `mdl_*`) | `/dumpbin` the 9 stock containers again when writing `mdl_ps1.hlsl` | Tier 1 correctness |

## 7. Suggested phasing

1. **Tier 1.**
   - `mdl_ps1.hlsl` + manifest entries.
   - `shader_layout` columns + tests.
   - Synthesis `v8`.
   - `style` decision + the `ps1_eligible` rule + `.w` packing in `render_item`.
   - Host tests via `validate_background_dancers.sh` / `validate_overlay_draw.sh`.
   - One cabinet deploy (DLL + blobs).

   About one session. Nothing new in `signatures.rs`.
2. **Tier 2 spike.** R1–R4, then the viewport object: a `viewport_pass_layout`-style pure layout
   with `const` offset asserts, plus the optional signature group (never `required_signatures`;
   a miss means Tier 1 only). One RE session plus one or two deploys; the smoke build answers R4.
3. **DDR SELECTION trigger and rows.** Small, pure decision function.
4. **Optional.** 2b (sharp movie / dancers-only pixelation), a shader upscale with CRT options,
   decimated "PS1-poly" personal content through the add-on.

## 8. Verdict

Everything on the wish list except true low-poly geometry is reachable.

- Vertex jitter, affine wobble, point-sampled mip-less textures, the PS1's own dither and Gouraud
  lighting are a pure shader addition on a cabinet-proven, detour-free seam.
- True low resolution and jagged edges come from two engine StretchRect records in a mod viewport,
  built entirely from primitives the modpack already uses. Its open items are layout and platform
  checks, not unknown engine behaviour.
- The DDR SELECTION 1st–5th Mix pairing costs one per-song predicate at a point where the skin is
  already known.
