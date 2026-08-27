# Arrow / Receptor Render Path — Playfield Styling RE Notes

Reverse-engineering notes for the `playfield-styling` mod (per-player arrow
scale + opacity). All addresses are file-relative to `gamemdx.dll` @
`0x180000000`. Primary build: **20260616**; cross-checked on **20260324**
where noted. Distilled from the feature research at
`.agents/planning/20260716-arrow-receptor-styling/research/arrow-render-re.md`.

## 1. Renderer class family (RTTI)

| Class (RTTI string) | Role |
|---|---|
| `screen::ArrowSprite` | Shared sprite base for lane renderers. vbptr at +0x80 (virtual-base reverse-flag lookup). |
| `screen::ArrowRenderer` | Scrolling notes (the repo's `render_notes` owner). Mode (0 single / 1 double) at +0xB0. |
| `screen::SpotRenderer` | Stationary receptor row. Mode at +0x98. |
| `screen::JudgeEffectRenderer` | Receptor hit flash (sprite-based; distinct from the BM2D `dance_effect` clips). **No verified mode field.** |
| `screen::GuidelineRenderer` | Measure guideline. Does **not** draw through the shared quad fill (§4). |

Each class has exactly one CompleteObjectLocator with `offset == 0` and one
vtable meta-pointer, so the repo's `find_vtable_by_rtti` walk resolves each
class's offset-0 vftable unambiguously (published as
`arrow_renderer_vtable` / `spot_renderer_vtable` /
`judge_effect_renderer_vtable`). The constructors store exactly these
vftables at object+0, so `[this]` comparison classifies live instances.

Each active side owns its own renderer set (two of each in versus; doubles =
one ArrowRenderer/SpotRenderer spanning 8 panels). The renderer objects carry
**no play-side field** — side is attributed externally (presence read +
posX@+0x30 < 640 split; doubles binds to side 0).

## 2. The shared quad fill — one detour covers the lane

`render_sprite_final` (existing repo AOB; 20260616 @ 0x180025900) is reached
by **real `CALL`s from every lane renderer** — it is not inlined:

| Caller | Quads |
|---|---|
| `render_notes` (0x180026b00) | Normal arrows + shock "electric" overlay |
| Shock pass (0x1800275b0) | Shock-arrow glyphs |
| Freeze pass (0x1800278a0) | Freeze heads/tails; bodies via a thin wrapper (0x180025860) that also ends in a real `CALL` to the fill |
| SpotRenderer draw (0x180025e30) | Receptor row (4/8 panels via mode@+0x98) |
| JudgeEffectRenderer draw (0x180029290) | Expanding hit flash (center-preserving grow math) |
| `note_types_expansion::mine_render` | Mine glyph + lightning overlay (calls the same entry point → **inherits the detour transform automatically**) |

Fill semantics (`fill(this, out_quad, x, y, w, h, uv[4], twist, color*)`):

- `x, y` are **lane-relative** (x = 96·dir; y = scroll offset from the
  receptor row). The original adds `posX/posY@this+0x30/+0x34` AFTER reverse
  mirroring — so transforming `(x, y, w, h)` before the original runs scales
  the lane about its origin without touching screen placement, and commutes
  with reverse scroll and with `center_arrows_single`'s lane shift.
- Rotation (`twist`) is applied about the quad center — relocated
  consistently by the transform; no angle change needed.
- Final alpha = `color.a × appearance_alpha` (HIDDEN/SUDDEN/STEALTH piecewise
  lerp on the incoming lane-relative y, fields at +0x6C..+0x78). A per-side
  opacity multiplier composes naturally on the `color` argument's alpha byte
  (byte 3), preserving the game's own alpha animations (shock-damage flash,
  game-over fade, freeze fades).
- The `color` pointer may point into game memory (e.g. the lane tint at
  `this+0x64`) — the hook copies 4 bytes to a stack local and passes that.

## 3. The note collector and its 720.0 culling window

The per-pass note collector (20260616 `FUN_180024b40`, 20260324
`FUN_1800240c0`) is called from `render_notes` twice per frame (shock pass,
then normal pass) and emits 0x28-stride records from the judge Results
vector.

**Derivation warning (Ghidra-verified on both builds):** the collector is
NOT the first `CALL rel32` in `render_notes` — stray `0xE8` bytes occur
earlier as MOV displacement bytes, and the true first CALL targets a
per-pass helper (0x180028780 on 20260616). The collector is instead the
unique `render_notes` callee whose first 0x100 bytes contain the top-cull
load below (`derive_note_collector` in `core/signatures.rs`).

### Culling

- **Top cull (loop break):** at collector+0xA6 on BOTH builds —
  `MOVSS XMM15, [RIP+disp32]` (`F3 44 0F 10 3D + disp32`) loading **720.0f**
  (20260616: insn @ 0x180024be6 → `DAT_18038eb38`). The per-note loop breaks
  when `get_offset_y(...) > 720.0`.
  - The 720.0 constant is **shared by 14 functions** — never patch the
    constant. The mod redirects the **instruction's disp32** to a mod-owned
    float slot (int3-cave near the collector, `alloc_near` fallback), set to
    `720 / min(scale_p1, scale_p2, 1.0)` per song.
- **Bottom cull:** raw (unscaled) offsets; self-consistent at shrink scales
  (mines mirror it deliberately so mines pop out exactly when arrows do).
- `render_notes` itself also loads 720.0 once (reverse-scroll
  `offsetY = 720 − posY` computation) — unrelated to culling; untouched.

### Scale-vs-cull analysis

- **s < 1:** notes become visible at lane offset `y = (720−posY)/s > 720` —
  the stock window would pop arrows in at screen y ≈ `720·s + posY`. The
  window must extend to `720/s` (25 % → 2880.0).
- **s > 1 (≤150 %):** both bounds already conservative — no change needed.
- **Cost:** worst case (window ×4) equals the stock 0.25× speed mod's note
  density, which the game supports natively.

## 4. GuidelineRenderer draw path

The guideline draw (20260616 `FUN_180026210`, 20260324 `FUN_180025760`)
does not use the shared fill. Its prologue AOB
(`48 8B C4 55 41 54 41 55 41 56 41 57 48 8D 68 98 48 81 EC 40 01 00 00`)
matches **3 functions on both builds**, so it is classified by content
(`derive_guideline_targets`): the real one uniquely contains, within its
first 0x800 bytes, both the XMM9-form 720.0 load and a
`CALL get_offset_y`.

Object layout (NOT ArrowSprite-based): mode (1 = double) at +0x78;
**X base (lane left) +0x80 / Y base (receptor screen Y) +0x84**; color RGB +
alpha bytes at +0x88..+0x8B; vbptr (reverse flag) at +0x20.

### Emission — a private, hookable bulk emitter

Lines accumulate into a temp vector of **0x14-byte records**
`{x = lane left, y = screen Y, w = numPanels·96, h = 3.0, color u32 (alpha
pre-composed in the MSB)}` and are submitted in ONE call to the bulk
emitter (20260616 `FUN_18000c7b0`, 20260324 `FUN_18000bca0`; byte-identical
body): writes a tag-0x01 DRAWSPRITES command with `count·0x14` stride math,
then memcpys the records. **Exactly one caller module-wide (verified at
derivation)** → detouring it is a de-facto private hook where records can be
transformed in place (x about `x + w/2`; y/w/h scaled; alpha MSB
multiplied).

### Guideline culling

- Normal scroll: breaks when screen y > the same shared 720.0
  (`MOVSS XMM9, [RIP+disp]`, 20260616 @ 0x180026448) — patched with the same
  disp32 redirect to the same float slot.
- Reverse scroll: bound is a literal 0.0 compare (not patchable). The
  capture detour on the guideline draw instead pre-scales `Ybase@+0x84` to
  `Y/s` around the original call (restored after): the emitted
  `y = ±(offset+adj) + Ybase/s` then reconstructs the exact receptor-anchored
  scale under the emitter-side `y' = s·y` — for BOTH scroll directions — and
  both cull bounds cover the extended window.

## 4b. Lane background + filter + cover (AFP clips, NOT the fill)

The lane **filter** (the translucent darkening band from the FILTER option),
the lane **cover** (SUDDEN/HIDDEN panel), the **danger flash**, and the
**receptor hit flash** are AFP movie-clip objects — they do NOT flow through
`render_sprite_final`, so the fill hook never sees them. The bands are
scaled horizontal-only + scale-only per the feature spec; the hit flash
scales uniformly and repositions.

The visible per-lane **background** needs NO clip handling: it scales
correctly through the fill quads + filter band alone (cabinet-confirmed).
An earlier revision captured the `1p_lane_usr`/`2p_lane_usr`/
`double_lane_usr` find-child clips for it — WRONG on two counts, and the
path was removed: (1) those clips are the HUD LAYOUT CONTAINERS (their
children are the `judge_usr`/`combo_usr`/`arrow_usr`… position markers the
HUD builder reads), not lane art — scaling them would shift the HUD; (2)
the find-child handle is a slot in a global scoped STACK
(`DAT_180cae330`-indexed, 0x210 stride — every HUD builder pushes/pops it)
and the pre-gameplay dance movie its ids refer into is torn down before the
deferred apply, so every apply attempt failed harmlessly on a stale id
(`afp_mc_get_param` error on all cabinet runs).

None of the live elements is `CMovieClip::Create`'d by its lane name, so
they are captured at two acquisition points (both hooked directly —
`CMovieClip::Create` itself is owned by `overlay_element_styling`, and
these bypass it, so there is no one-detour-per-target conflict):

| Element | Names (cabinet-diagnosed) | Capture point | Id |
|---|---|---|---|
| Lane filter | `dance_filter_%s` (single/double) | `cmovieclip_pool_create` (`FUN_1802575a0` @ 20260616, `FUN_18021b4d0` @ 20260324) — pool-slot wrapper around `CMovieClip::Create` | AFP layer id at slot+0x08 |
| Lane cover | `hidden_cover_%s` / `sudden_cover_%s` | same | same |
| Danger flash (red low-life lane overlay) | `danger_single` / `danger_double` (EXACT match — the HUD builder also find-childs `danger_gauge_%dp_usr`, a gauge readout that must not match) | same | same |
| Receptor hit flash (`dance_effect`, per panel) | none — created via `afp_layer_create_with_property` directly (bypasses both Create and the pool wrapper) and stored in `NoteResultActor`'s `vector<CMovieClip*>` @ **actor+0xE8..+0xF0** (each element's layer id at clip+0x08; mode at actor+0x90; the setup also resolves each clip's root MC via `Ordinal_103(layer_id, "/")`) | `note_result_setup` detour (`FUN_18007a230` @ 20260616, `FUN_18007af20` @ 20260324) — walk the vector after the original runs | AFP layer id at clip+0x08 |

The pool-create prologue AOB is unique on both 2026 builds.

### The MC param tables (libafp `afp_mc_set_param` / `afp_mc_get_param`)

`afp_mc_{set,get}_param(mc_id, param, value_ptr)` dispatch through parallel
jump tables indexed `param − 0x1000` (Flash-property style; verified in
libafp 20260324, function-identical on 20250805-lineage). Mapped entries:

| Param | Handler | Reads/Writes |
|---|---|---|
| 0x1000 | `pw_set_position` / get | 2 floats → obj+0xD0/+0xD4 (local x/y) |
| 0x1001 | `pw_set_rotate` | 1 double (degrees) → obj+0x134 |
| **0x1003** | **`pw_set_scale` / `pw_set_get_scale`** | **2 floats → obj+0x124/+0x128 (sx, sy — 1.0-normalized). COMPONENT-based: rebuilds the local matrix from components; position is untouched, so no translation-preservation concern.** |
| 0x1004 / 0x1005 | `pw_set_color` / `pw_set_acolor` | 4 floats (mult/add RGBA) |
| 0x1008 | `pw_set_global_position` / get | world-space point |

The set-value argument is a POINTER to the floats (the dispatcher spills it
and hands the handler `&value`; the handler dereferences it as `float*`).

### Apply mechanism (implemented in `lane_hook.rs`)

Captures are QUEUED and applied on the **first `render_sprite_final` call
after capture** — by then the HUD build (which reads child-marker global
positions like `…/arrow_usr` out of the lane container clips) is complete
and all clip positions are final. An apply whose prerequisite isn't ready
(the flash needs its side's renderer bound in the fill registry) re-queues
and retries on a later fill call.

- Filter/cover/danger (layer id): `afp_layer_get_matrix` → write back
  `{s·a, b, s·c, d, tx, ty}` — horizontal-only about the layer origin,
  preserving the game's own scale (the filter may be a width-scaled unit
  quad) and translation.
- Receptor hit flash (`dance_effect`, layer id): UNLIKE the lane bands it
  must **track the transformed receptors** — the fill's fixed points are
  the lane center X (`cx` = centroid of the panel flashes' `tx`; they span
  the lane symmetrically) and the renderer's `posY` (+0x34 — TOP of the
  receptor row; the fill's quad `y` args are offsets from it). Two-part
  apply, cabinet-converged over four rounds:
  - **Translation-only matrix RMW**: `{a, b, c, d, cx + s·(tx−cx),
    posY + s·(ty−posY)}` — a/d are NOT scaled (scaling them displaces the
    art by its internal offset from the layer origin).
  - **Uniform component scale on the clip's root MC**:
    `layer_find_child(layer_id, "/")` (the same root-MC resolve the game's
    setup performs) → `afp_mc_set_param(root, 0x1003, &{sx·s, sy·s})` —
    scales about the MC registration point, position untouched.
  `posY` comes from the fill registry (`fill_hook::side_anchor_y`, Spot
  renderer preferred), captured at renderer bind alongside posX.

Side attribution: `*_double` filter/cover and mode≠0 flashes → side 0
(doubles uses P1's values); `*_single` in versus (both sides create the
same name, possibly from the same package — a create-order heuristic is
NOT reliable) via the apply-time matrix `tx < 640` split;
single-active-side via the presence read. The flash must NOT use a
centroid `< 640` split: with center-arrows-1P the single lane centers
EXACTLY at 640.0 and the tie misclassifies (cabinet-diagnosed).

## 5. Dead ends verified

- The HUD named-layout coord map (`arrow`/`arrow_raw`) carries scaleX/scaleY
  fields, but the lane renderers consume only x/y — writing scale there has
  no effect.
- AFP-layer scaling (the `overlay_element_styling` mechanism) does not
  apply to the SCROLLING elements: arrow/spot/judge renderers are
  CommandList sprite emitters, not CMovieClips. (It DOES apply to the lane
  filter/cover/danger/flash — see §4b — which are genuine AFP-layer clips.)
- Hooking the collector instead of patching its cull load: the bound is a
  register loaded once; a detour can't change the loop comparison without
  reimplementing it.
- `CMovieClip::Create` dispatcher for the lane clips: the lane bands are
  pool-slot creations and the hit flash is `afp_layer_create_with_property`
  — a Create hook would never see either; the pool-create + note-result
  capture points are required instead.
- Scaling the receptor flash via its layer matrix a/d: displaces the art by
  its internal offset from the layer origin (cabinet round below/above the
  receptors) — root-MC component scale + translation-only matrix is the
  correct decomposition.
- `1p_lane_usr`-style lane-container capture — see the §4b correction
  (layout containers on transient ids; path removed).

## 6. Known characteristics (accepted)

- HIDDEN/SUDDEN fade zones keep stock screen distances from the receptor row
  (the fade thresholds evaluate on the scaled y) — accepted behavior, not a
  bug.
- In a mixed-scale versus match, quads for the larger-scale side are
  over-collected (shared window) and GPU-clipped — cosmetic non-issue.

## 7. Texture sampling — why scaled arrows alias (and why it CANNOT be
## fixed with sampler filters)

The cabinet renders through D3D9 (spice2x log: `graphics::d3d9`). Scaled
arrows/receptors/freeze bodies show nearest-texel aliasing at any scale ≠
100 % — and this is INHERENT to the pipeline, not a fixable sampler
setting. Full investigation record (one failed fix, reverted):

### The engine keeps one sampler desc PER TEXTURE

- gs texture registry: heap array, entry stride **0xA0**, index =
  `texture_id >> 0x11`, entry+0x00 = the id (generation check). Base
  pointer global `DAT_1806f0a30`, spinlock `DAT_1806f0a38` — 20260616.
- Sampler desc at **entry+0x34**, 13 dwords in `D3DSAMPLERSTATETYPE`
  order: addrU/V/W (+0x34/38/3C), border (+0x40), **MAG @ +0x44, MIN @
  +0x48, MIP @ +0x4C**, … Engine filter enums identity-map to `D3DTEXF_*`
  (tables filled by `FUN_18024a750`): 1=POINT, 2=LINEAR, 3=ANISOTROPIC.
- The gs command executor (`FUN_18024c310`, case 8/9) re-applies the desc
  through the applier `FUN_18024b7c0` (device vtable +0x228 =
  `SetSamplerState`) with a per-stage shadow cache on every bind.
- Live-verified descs (CE session, 2026-07-19): base sheet
  (receptors/freeze/hit-flash, renderer `this+0x20`, 768×192) =
  wrap/wrap, POINT/POINT/POINT; arrow atlas (`this+0xD0`, 768×384) =
  clamp, LINEAR ×3; electric (`this+0xE0`) = clamp, LINEAR/LINEAR/NONE.

### Why POINT on the arrow art is LOAD-BEARING (the failed fix)

Flipping the base sheet POINT→LINEAR (tried as the aliasing fix) produced
severe color banding on arrows, freeze bodies AND receptors at any scale
≠ 100 % (`capture/capture_20260719_1054*.jpg`, `…_1410*.jpg`), confirmed
bidirectionally by a live CE A/B (POINT restored → banding gone,
stair-step aliasing back).

Root cause: the arrow pipeline is **PALETTE-INDEXED**. The lane renderers
bind the shader `gs_screencommand_arrow` (string @ 0x18035b9f8, bound by
the arrow/spot state ctors `FUN_1800268d0` / `FUN_180025ca0`). Its pixel
shader samples the atlas and uses the **red channel as a U coordinate
into a palette texture on stage 1** (the palette is 256×16, point-only,
rewritten per frame — this is how note colors animate by beat). LINEAR
filtering on the atlas interpolates palette INDICES; midway indices hit
unrelated palette entries → banding aligned with the art's axes. POINT
sampling is required for index fidelity.

Consequences:
- The pixelated look of scaled arrows/receptors is INHERENT to
  palette-indexed art rendered off its native 1:1 grid. No sampler state
  fixes it; a real fix would need shader replacement (index-aware
  filtering) or higher-resolution art — both far out of scope.
- `texture_filter.rs` and its `derive_texture_registry` derivation were
  REVERTED entirely (POINT restored). Do not reintroduce LINEAR on the
  lane atlases.
- The half-texel UV inset added for LINEAR edge bleed was also reverted
  (motivation gone with the revert).

### Accepted characteristic — SUPERSEDED by the shader fix (2026-07-19)

~~Scaled playfield elements keep nearest-texel aliasing.~~ This was resolved by
replacing the palette-family pixel shaders (`gs_screencommand_arrow`,
`gs_screencommand_judge`) with **index-aware bilinear** variants: 4 atlas taps
→ palette-lookup each → blend the resulting COLORS (safe; blending indices was
the banding trap). Shipped as `data_mods/shader_fixes/` via LayeredFS (no DLL
code). Cabinet-accepted: 100 % scale unchanged, scaled play markedly smoother,
no banding. Full record: `docs/shader_replacement_research.md` and
`.agents/planning/20260719-shader-injection/`.

Label correction to the live-CE section above: the renderer texture slots are
`this+0x20` = arrow/receptor/freeze sheet (768×192, also the judge renderer's
sheet), `this+0xD0` = shock-arrow electric crackle (768×384), `this+0xE0` =
lane notice (192×384) — confirmed from install data (`data/arc/2d/`). The CE
session's dims were right; the "+0xD0 arrow atlas / +0xE0 electric" labels
were guesses.
