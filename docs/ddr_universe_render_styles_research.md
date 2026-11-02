# DanceDanceRevolution Universe (Xbox 360) — the per-song character render styles (2026-11-02)

**Question.** Universe draws its dancers in ~20 "render styles" (thermal camera, chrome, x-ray,
negative, silhouettes…) that the per-song direction CSVs switch bar by bar. Which of them can
become World Lighting Style options, and exactly what does each one draw?

**Status.** RE complete for every style. Fifteen are ported as World Lighting Style values, plus
RANDOM (EVERY BAR) (a different one per chart bar, §4.2)
(`shaders/src/mdl_universe.hlsl`, `services/avs_layeredfs/shader_layout.rs` `SceneStyle`, planning
`.agents/planning/2026-11-02-universe-render-styles/`). Implemented and host-tested (layout, gate,
instance plan); not cabinet-tested yet.

**Sources.**
- `default.xex` (NTSC-U) loaded in Ghidra with the XEX loader (PowerPC BE, image base
  `0x82000000`). Addresses below are virtual addresses as Ghidra shows them.
- The x_data rip (`docs/dancing_stage_unleashed_dancers_port_feasibility.md` §14):
  83 song CSVs (`<song>.csv`), the ramp / sphere textures.
- **Ghidra gotcha.** The loader marked the PPC register save helpers `__savegprlr_14..27`
  (`0x82487250..0x82487288`, a chain of `std rN` that falls into `stw r12; blr`) as *no-return*
  and left a `CALL_RETURN` flow override on every call to them, so almost every function was
  truncated after its prologue. Clearing the no-return flags, then each function's call-site
  overrides and re-creating it, gives full bodies. Done for the functions in this note only.

## 1. The style name table and its parser

The song CSVs (`Bar, Note, Movie File, …, Light Color, Camera Type, Render Style, Render Param
1..4`) name a style per row. Parser `0x820FEC58` compares the cell against string constants and
stores a style id at `event+0x25` (unknown names: `event+0x3a = 0`, no change). Several names are
aliases:

| id | names | rows in the 83 CSVs |
|---|---|---|
| 1 | `NONE`, `NORMAL`, `DEFAULT`, `TOONOUTLINE` | 1784 |
| 2 | `TOONNOOUTLINE`, `TOON`, `FLAT`, `GOURAND` | 6 |
| 4 | `OUTLINE` | 27 |
| 6 | `SILLHOUETTE`, `FILL OUT` (+ 4 params) | 499 |
| 7 | `SILLHOUETTE RAINBOW`, `RAINBOW` | 18 |
| 8 | `STENCIL`, `FILL IN` (+ 4 params) | 263 |
| 9 | `STENCIL RAINBOW` | 139 |
| 10 | `CONTRAST` | 101 |
| 11 | `HIGHLIGHT` | 23 |
| 12 | `SHADOW` | 2 |
| 13 | `THERMO` | 123 |
| 14 | `CANDY` | 54 |
| 15 | `MERCURY`, `T1000` | 63 |
| 16 | `BUBBLE`, `STONE` | 148 |
| 17 | `CRYSTAL` | 102 |
| 18 | `XRAY`, `ALPHA LIGHT` | 132 |
| 19 | `NEGATIVE`, `BLACKGLOSS` | 56 |
| 20 | `MONOCHROME` | 4 |
| 21 | `SEPIA` | 3 |

The (`NONE`, `NORMAL`) pair is a 4- / 6-char prefix compare; the rest are whole-string compares.

**Render Params** (styles 6 and 8): four colours stored at `event+0x28..+0x34` through
`0x82192E50`. A cell `-N` (N ≤ 152) indexes a 17 × 9 palette at `0x82DCEA88`; any other text is
matched to its nearest palette colour. The palette is generated at boot by `0x82180978`
(the video editor's colour picker): row r, column c, `HSV→RGB` `0x822521F8` with a byte-scale hue
(`sector = h·6 >> 8`):
- columns 0–15: hue `15·c`; rows 0–3 saturation `50·(r+1)`, value 255; rows 4–8 saturation 255,
  value 255 / 161 / 111 / 61 / 25;
- column 16: grey, value `255 − (255·r) / 8` (row 0 white … row 8 black).

So `-16` is white (the most common STENCIL / SILLHOUETTE setting, 255 rows), `-152` black,
`-68` pure red.

## 2. Draw dispatch

`0x8209B818(model, material, pass)` switches on the dancer's current style id and calls one draw
routine per style. A routine copies the material record (0xC4 bytes) with a **draw kind** at
`+0x5c` (the same `0xA00000nn` word the `.ddm` material flags carry), sets render states through
the device wrapper `0x8224D550(mgr, state, value)` (360 `D3DRS_*` codes, cached at
`mgr+0x12e0+state·4`), binds the ramp / sphere textures (`[0x8253835C]+0xAC..0xC8`, loaded by
`0x82096FE8` in the order `rainbow, toon, specular, crystal, sphmap2, xray1, xray2,
sphere_thermograph`), and queues the mesh. The mesh draw `0x82242F90` then picks the VS by kind
(`0x8224CFF0`) and the PS by kind (below).

| id | routine | kind(s) / passes | textures / states |
|---|---|---|---|
| 1 NORMAL | `0x8209B1B0` | 0xa0000002 toon, then 0xa0000003 outline (cull CW, VS c18 = {0.005, 0.5, 0, 0.001}) | stage 1 = `toon` |
| 2 TOON | `0x8209AFD8` | 0xa0000002 | stage 1 = `toon` |
| 4 OUTLINE | `0x8209A5E0` | 0xa0000001 with colour writes off, then the outline | — |
| 6 / 7 SILLHOUETTE | `0x8209ABD8` | 0xa0000001, colour writes off, stencil written; flag `mgr+0x2215` | fill texture `mgr+0x2210` = 0 / `rainbow` |
| 8 / 9 STENCIL | `0x8209A948` | as 6 / 7 but flag `mgr+0x2214` | as 6 / 7 |
| 10 CONTRAST | `0x82099640` | 0xa0000010 | blend off |
| 11 HIGHLIGHT | `0x82099858` | 0xa0000011 | alpha blend (SRCALPHA, INVSRCALPHA) |
| 12 SHADOW | `0x82099A70` | 0xa0000012 | alpha blend |
| 13 THERMO, 15 MERCURY | `0x8209B738` | 0xa0000005 sphere-mapped | stage 0 = `sphere_thermograph` / `sphmap2`, blend off |
| 14 CANDY, 16 BUBBLE, 17 CRYSTAL, 18 XRAY | `0x8209B738` | 0xa0000005 | stage 0 = `sphere_thermograph` / `sphmap2` / `crystal` / `xray1`; **additive** (blend ONE, ONE), z-write off, cull none |
| 19 NEGATIVE | `0x8209A278` | 0xa0000019, then the outline | stage 1 = `toon` |
| 20 MONOCHROME | `0x82099C88` | 0xa000001a, then the outline | stage 1 = `toon` |
| 21 SEPIA | `0x82099F80` | 0xa000001b, then the outline | stage 1 = `toon` |

**Full-screen fill for 6–9** (`0x8224E708`, run after the dancers). It draws a screen quad
(0, 0)–(640, 480) whose four corners take the four Render Param colours (white when the rainbow
texture is bound, which then spans the screen u 0→1, v 0→1):
- `mgr+0x2214` (STENCIL, "FILL IN"): z-func GREATER, i.e. the quad lands where the dancer wrote
  depth: **the dancer becomes a flat colour / rainbow shape**.
- `mgr+0x2215` (SILLHOUETTE, "FILL OUT"): stencil NOTEQUAL: **everything except the dancer is
  filled**, and the dancer shows the movie behind it.

## 3. Shaders

Every Universe shader is **HLSL or `vs_1_1` assembly source text** in `.data`
(`0x82507930..0x8251C188`), compiled at boot with the runtime compiler. The pixel-shader manager
`klPixelShaders` (singleton `0x82E7A5B8`, loader `0x8225BF20`) and the vertex-shader manager
(`0x82E7A648`, loader `0x8225CDD0`) name each one. The PS setter `0x8224C9E0(mgr, id)` maps ids
2–38 through a byte table at `0x82023EE8`.

**kind → VS** (`0x8224CFF0`, skinned path):

| kind | VS (source address) | outputs |
|---|---|---|
| 0xa0000001 | `0x82516708` | uv; COLOR0 = max(N·L × lightColour, 0.5) |
| 0xa0000002, 0x10..0x12, 0x19..0x1b | `0x825196D8` | uv; the skinned normal and the model-space light direction, normalised per pixel |
| 0xa0000003 | `0x8251A2F8` | outline: pos += n · (c18.x + c18.y · clip.w · c18.w), colour c3 = black |
| 0xa0000005 | `0x82517D10` | **uv = 0.5 + 0.5 · (n_light.x, n_light.y)**, n_light = the skinned normal through c23 (the light's look-at matrix in model space); COLOR0 = lightColour |

**kind → PS** (`0x82242F90`):

| kind | PS | what it computes (N, L normalised per pixel; ramp = `toon.tga` at u = saturate(N·L)) |
|---|---|---|
| 0xa0000001 | `s3DDiffuseTexture` (6) | tex0 × COLOR0 |
| 0xa0000002 | `s3DTextureModulate` (8), `…Specular` (9) for specular materials | tex0 × ramp × lightColour |
| 0xa0000003 | `sDefault` (5) | COLOR0 |
| 0xa0000005 | `s3DDiffuseTexture` (6); costume `sphere` materials use `s3DTextureSphere[2]` | sphere map × lightColour (the body texture is NOT sampled: stage 0 is the sphere map) |
| 0xa0000010 | `s3DTextureContrast` (32) | saturate(N·L) > 0.5 ? white : black (opaque, no texture) |
| 0xa0000011 | `s3DTextureHighlight` (33) | > 0.5 ? white : transparent black |
| 0xa0000012 | `s3DTextureShadow` (34) | > 0.5 ? transparent black : white |
| 0xa0000019 | `s3DTextureNegative` (35) | c = min(tex0 × ramp × lightColour, 1); rgb = 1 − c, a = 1 |
| 0xa000001a | `s3DTextureMono` (36) | rgb = dot(c, (0.299, 0.587, 0.114)), a = 1 |
| 0xa000001b | `s3DTextureSepia` (37) | Y = luma(c); rgb = YCbCr→RGB(Y, −0.2, 0.1), a = 1 |

`toon.tga` (128 × 32) is DSU's ramp with a softened edge: 142 up to texel ~56, a short ramp, 255
from ~76. `rainbow.tga` (128²) is a vertical hue band (red at the top through magenta, blue,
cyan, green, yellow, back to red). The sphere maps (128², `sphmap2` 256²): `sphere_thermograph` is
radial (magenta centre → red → yellow → green → blue rim), `crystal` a thin bright ring at r ≈ 0.8,
`xray1` / `xray2` a bright centre fading to black at the rim, `sphmap2` a chrome photo of a
lighting truss.

The light direction (VS c16) and the light look-at (c23) are uploaded by the generic model draw, not
the dancer module; their values were not traced. LIGHT COLOR (c17) is per song (palette index,
mostly `-16` = white).

## 4. The World port

What a World Lighting Style can do: re-point a dancer's private material copies at a synthesized
`<material>_<suffix>` container (its own VS/PS, `docs/background_dancers_research.md` §4.7), set a
material texture slot, and rewrite its draw records' flag word (blend group, transparent pass,
z-write), and the node's pass mask / sort key. It cannot switch off colour writes, use the stencil
buffer or draw a full-screen pass. Therefore:

| World style | from | how |
|---|---|---|
| CONTRAST / HIGHLIGHT / SHADOW | 10 / 11 / 12 | per-pixel N·L against CEL's light; HIGHLIGHT / SHADOW `clip()` the transparent half (Universe's alpha 0 under SRCALPHA blending is the same picture) |
| NEGATIVE / MONOCHROME / SEPIA | 19 / 20 / 21 | Universe's math on tex × ramp (`toon.tga`'s two levels, CEL's light); World's DSU hull for the outline |
| OUTLINE ONLY | 4 | colour writes off ≈ an additive body that outputs rgb 0 with z-write ON, drawn before the DSU hull (§4.1) |
| STENCIL | 8 | flat fill in the item tint, one colour per bar from the palette entries Universe's charts use for STENCIL (`universe_logic::STENCIL_PALETTE`: white, black, red, …) |
| STENCIL RAINBOW | 9 | `rainbow.tga` at the pixel's screen position (u, v ∈ [0, 1], top-left origin) |
| THERMO / MERCURY | 13 / 15 | sphere map at 0.5 + 0.5 · n_light.xy, light basis: z = −L, x = up × z, y = z × x, up = +Y |
| CANDY / BUBBLE / CRYSTAL / XRAY | 14 / 16 / 17 / 18 | as THERMO, records made additive (TRANS, z-write off) |

Common to all: the material texture's alpha still alpha-tests (World's stock hair cards are
cut-outs; Universe's own models have none). Not ported: NORMAL / TOON (≈ World's CEL), SILLHOUETTE
and SILLHOUETTE RAINBOW (fill outside the dancer).

### 4.1 How the DLL applies them

- **Containers.** `<material>_u<family>` for the ten families (`con hil sha neg mon sep ste scr
  sph inv`) × every `MODEL_VARIANTS` shape, next to `_lit` / `_cel`; program 0 is the cel outline pair.
  A missing `mdl_uni_*` blob drops only these (`shader_synthesis::universe_available`), and the
  styles fall back to CEL.
- **Runtime switching** (`background_dancers/style_rig.rs`). At build every restyle-eligible
  instance captures, per eligible material copy, its stock shader object and the container of
  every family, plus every record's flags. `Session::style_tick` (each frame, before the director)
  resolves the live style and writes only what changed: shader-object pointers, record flags, the
  slot-1 texture, the node's item pass mask / sort key, a hull's node-level hidden flag. So
  LIGHTING STYLE edits apply at once, in gameplay and in the song-select previews.
- **Per instance.** Dancers (bodies, parts, Winx twins) take the Universe style; stage props and
  their hulls take CEL (`SceneStyle::stage_style`), including CEL's outlines when SCENE OUTLINES is
  on. Hulls are built per song wherever a look MAY need one (`style::hull_plan`) and shown per look:
  dancers' for LIT / CEL / NEGATIVE / MONOCHROME / SEPIA with SCENE OUTLINES on, and always for
  OUTLINE ONLY. Dancer previews build dancer hulls whenever the outline pair is served. A Winx dancer
  that transforms draws through the CEL wipe containers, so it shows CEL.
- **Style texture.** `data/arc/bdfxuniverse.arc` (`scripts/gen_universe_style_textures.py`; stems
  `bdfxunirainbow / thermo / chrome / crystal / xray`) is loaded first with every dancer pick while
  the Universe containers are served (any look may become a texture style). The look switch writes
  the registered `TextureData*` into **slot 1** of the materials the family re-points (pointer, the
  converter's slot defaults, table index `0xFFFF` so the re-resolve skips it, then mask bit 1). The
  build waits for the texture with the model-texture budget (10 s); after that the look switch keeps
  retrying (WARN after 20 s). The texture styles degrade to CEL when the arc does not resolve.
- **Additive.** CANDY / BUBBLE / CRYSTAL / X-RAY rewrite each restyled record's flags to
  transparent `0x01` | two-sided `0x02` | z-write off `0x08` | additive group `0x40`
  (`render_item_layout::additive_record_flags`; logged before → after per item).
- **OUTLINE ONLY.** The body's styled records: additive group `0x40`, z-write forced ON, the
  `_uinv` PS outputs rgb 0 (`clip` drops cut-out texels) — it adds nothing but writes depth. The
  hull (always shown in this look) must draw AFTER it. World's model passes
  (`gamemdx_20260825`): the render-graph boot `FUN_1801f2c30` attaches OPACITY / LOWPRIO_TRANS /
  TRANS at 0x66 / 0x67 / 0x68; the collector dispatch `FUN_180261430` gives OPACITY the opaque
  collector `FUN_1802629d0` (record bit 0 clear) and LOWPRIO / TRANS the transparent one
  `FUN_180262f00` (bit 0 set); OPACITY sorts front-to-back (`FUN_180262310`), TRANS back-to-front
  (`FUN_1802624c0`), LOWPRIO not at all (node sort-key order). The draw-state emitter
  `FUN_180261c80` applies the record's blend group in every pass. So in gameplay the body's records
  get bit 0 and its node mask becomes LOWPRIO_TRANS (`0x10`, also seen by OPACITY, which skips bit-0
  records) with sort key `0x4000_0000` (after every `:N` stage part), and the hull's records get
  bit 0 (TRANS). A preview has no LOWPRIO clone and no stage: its body stays in the OPACITY clone
  (bit 0 clear), its hull goes to the TRANS clone. Known artefact: a TRANS-pass stage part behind
  the dancer is cut out of the silhouette (the body's depth is already written).
- **Limits.** Only blend-group-0 materials restyle (as in every style), so alpha-blended hair and
  veils keep their stock look (also in OUTLINE ONLY). Confirmed on the cabinet (2026-11-02): the
  slot-1 → sampler s1 binding and the additive records (flags `0x00 → 0x4B`).

### 4.2 RANDOM (EVERY BAR) and STENCIL's colours

Universe's songs switch styles by hand-written per-bar scripts; World has none, so RANDOM picks
per chart bar (`floor(measure ticks / 4096)` from the song's tempo map, a 2-s bar of real time in
the previews) from every preset but STOCK (`SceneStyle::random_pool`, minus what this boot cannot
serve). `universe_logic::shuffled_pick`: per block of N bars a Fisher–Yates shuffle seeded by the
song seed and the block, so each preset shows once per N bars and no bar repeats the previous one.
STENCIL takes its colour from the same kind of cycle over the 14 palette entries its charts use
(§1), through the item tint (the `_uste` PS outputs COLOR0).

## 5. Address index

| Address | What |
|---|---|
| `0x820FEC58` | Render Style parser (name → id at `event+0x25`, params → `+0x28..+0x34`) |
| `0x82192E50` / `0x82180118` | Render Param cell → palette index / palette read |
| `0x82180978` | palette builder (17 × 9 HSV) |
| `0x822521F8` | byte-scale HSV → ARGB |
| `0x82096FE8` | ramp / sphere texture loader (8 names) |
| `0x8209B818` | per-style draw dispatcher |
| `0x8209B1B0` … `0x8209B738` | per-style draw routines (§2) |
| `0x82242F90` | mesh draw: kind → PS |
| `0x8224CFF0` | kind → VS + vertex declaration |
| `0x8224C9E0` | PS setter (id → `klPixelShaders` member) |
| `0x8224E708` | STENCIL / SILLHOUETTE full-screen fill |
| `0x8225BF20` / `0x8225CDD0` | PS / VS manager loaders (source text → compiled shader) |
| `0x82E7A5B8` / `0x82E7A648` | PS / VS manager singletons |
