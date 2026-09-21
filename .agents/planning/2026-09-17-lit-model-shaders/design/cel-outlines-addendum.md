# Design addendum — Cel shading + inverted-hull outlines ("Phase 2b")

Status: Approved 2026-09-17 (maintainer: "include 2b as well … run experiments now, expose things as
options later"). Extends `detailed-design.md`; RE: `docs/background_dancers_research.md` §4.6.

## 1. Scope

Three additions over the shipped lit path, all operator-configurable for experimentation:

1. **Cel ramp** — quantized lighting (3 bands, soft edges) computed PER PIXEL.
2. **Rim ink** — dark edges where the normal grazes the view (`1 − |N·V|`), per pixel, in the same PS.
3. **Inverted-hull outlines** — every dancer body/part drawn a second time as a black shell offset in
   screen space along the projected normal and pushed back in depth.

The approved `lit` look is untouched (same blobs, same stock-PS pairing).

## 2. Program table of the two lit containers (`mdl_bg_lambert` / `mdl_ch_lambert`)

| index | bound when | pair |
|---|---|---|
| 0 | record has `rec+0x28` bit 31 (**hull records only**) | outline VS + outline PS — when the outline blobs are present, else the style pair |
| 1, 2, 3 | ordinary records (production binds **2**; 3 if `DAT_1806f1548 == 0`) | the STYLE pair: `lit` = lit VS + stock `gs_model_default` PS; `cel` = cel VS + cel PS |

`shader_layout::model_programs(outline: bool)` → `[(0,1,1),(0,0,0)×3]` (VS table `[style, outline]`,
PS table `[style, outline]`) or `[(0,0,0)×4]`. Program 0 is INERT unless a bit-31 record exists, so
the outline pair is always packed when its blobs resolve — which makes DANCER OUTLINES a
**next-song** toggle (the hull items are built or not per session) while DANCER LIGHTING stays
**next-launch** (the style pair is chosen at synthesis).

## 3. Shaders (`shaders/src/mdl_common.hlsli` + `mdl_cel.hlsl`; `mdl_lambert.hlsl` refactored onto the include)

Shared (`mdl_common.hlsli`): register map, `skin()` (the stock bone-texture addressing), `to_clip`,
`to_clip_dir`, `to_world_normal`, `anim_uv`, and `view_frame(clip, nclip) → (pos_view, n_view)`
recovering `P00`/`P11` from `|WVP column j| / |World row 0|` (RE §4.6 — the pass binds no camera).

* `vs_cel_{bg,ch}_main` → `POSITION`, `TEXCOORD0 uv`, `COLOR0 = Tint`, `TEXCOORD1 = (n_view.xyz,
  N·L_world)`, `TEXCOORD2 = pos_view.xyz`.
* `ps_cel_main`: `band = ramp(N·L)` — `L0 + (L1−L0)·smoothstep(T0±S) + (L2−L1)·smoothstep(T1±S)`,
  defaults `L = {0.55, 0.90, 1.20}`, `T = {0.15, 0.55}`, `S = 0.03` (the lit range's ends as the
  outer bands); `ink = smoothstep(INK_LO 0.62, INK_HI 0.78, 1 − |N·V|)`; `rgb = tex·tint·band·(1 −
  ink·INK_STRENGTH 0.85)`, `a = tex.a·tint.a`; the stock stipple `clip(c2.y − tex2D(s15, vPos/32).y)`.
* `vs_outline_{bg,ch}_main`: after skinning/clip, `sd = nclip.xy·clip.w − clip.xy·nclip.w` (the exact
  projected-normal direction), `nd = sd·(16,9)` normalized (aspect baked), pixel width
  `OUTLINE_PX 2.0 · saturate(OUTLINE_REF_DIST 5.0 / clip.w)` (constant 2 px at 720p up to 5 m, thinner
  beyond), `clip.xy += nd·px·(2/1280, 2/720)·clip.w`; depth push `clip.z += clip.w·lerp(PUSH_RIM
  0.0002, PUSH_FACE 0.004, facing²)`, `facing = |n_view·v|`. Outputs `POSITION`, `uv`, `COLOR0 = Tint`.
* `ps_outline_main`: `rgb = OUTLINE_RGB (0.03) · tint.rgb`, `a = tex.a·tint.a` (cutouts keep their
  shape through the stock alpha test), same stipple.

All tunables are `#define`s; blob-only redeploys re-synthesize (blob hashes are in the fingerprint).

## 4. Synthesis + config + rows

* `ShaderFixesConfig`: `dancer_lighting: Option<String>` (`"stock" | "lit" | "cel"`),
  `dancer_outlines: bool` (default `true`), `lit_models` kept as the MIGRATION source (absent
  `dancer_lighting` ⇒ `lit_models ? lit : stock`). Effective style helper `dancer_style()`.
* `shader_synthesis`: `Plan { style: DancerStyle, outlines: bool }` replaces `lit_models`; blob
  resolution per style (`lit`: the two lit VS; `cel`: two cel VS + cel PS) soft-degrades to STOCK
  with one WARN; outline blobs soft-degrade to `outlines = false` with one WARN. Fingerprint
  `"v6 … style=<s> outlines=<b> …"`. On BOTH success paths publish `OUTLINE_PROGRAMS: AtomicBool`
  (`outline_programs_available()`) — the dancers mod's gate.
* `shader_fixes.rs`: rows DANCER LIGHTING (STOCK/LIT/CEL, next launch) + DANCER OUTLINES (OFF/ON,
  next song); `persist_section()` writes `anti_aliasing`, `dancer_lighting`, `dancer_outlines` (drops
  the legacy `lit_models`); `pub fn dancer_outlines_live() -> bool` from the live atomic.

## 5. Dancers mod — hull instances (`src/mods/background_dancers/session.rs`, `render_item.rs`)

* `InstanceKind::Hull { of: usize }` for every `Dancer`/`Part` instance (never stage parts/shadow —
  their `constant` containers have 4 identical programs, a hull there is a duplicate draw), appended
  after the parts, `slot = instances[of].slot` (**shares the board slot** — two nodes read one
  seqlocked snapshot; the `MAX_INSTANCES` check counts slot OWNERS only), same `model_name`,
  `pass_mask`, `bone_count`, `mirror`; sort key = the body's.
* `Session::new(…, hulls: bool)`; `hulls = shader_fixes::dancer_outlines_live() ∧
  shader_synthesis::outline_programs_available()` evaluated by the lifecycle at session creation.
* `build_one`: after `render_item::build`, a Hull calls `item.mark_hull_records()` — for every record
  `REC_FLAGS |= HULL_BIT (1<<31)`; records whose blend group (`flags & 0xE0`) is non-zero ALSO get bit
  27 (hidden): a z-write-off blended mesh would be darkened by its own shell. Pure
  `render_item_layout::hull_record_flags(flags) -> u32`, host-tested. Body items now MASK bit 31 OUT
  (`GPU_REC_FLAG_MASK` loses it) so a stock record carrying it can never turn into a hull.
* `initial_world(Hull{of})` = `initial_world(of)`; `children` unchanged (hulls are not published —
  they read `of`'s slot); `tag()` = `"hull"`; `built_counts` gains a hull count; teardown/`node_shown`
  loops already cover every built instance.

## 6. Fail-open

| failure | outcome |
|---|---|
| cel/outline blob missing | style ⇒ STOCK / outlines ⇒ off, one WARN each; lit-only deploys keep working |
| `outline_programs_available() == false` | no hull instances; the two rows still show the config |
| a hull `render_item::build` fails | that hull skipped (existing per-instance WARN), body unaffected |
| projection not the standard D3D shape | ink/facing terms distort (thicker rims off-centre), nothing crashes; lit style unaffected |

## 7. Cabinet checklist (deploy #3)

DLL + `data_mods/shader_fixes/blobs/` (8 model blobs now). `mod-config.json`: `shader_fixes.
dancer_lighting = "cel"`, `dancer_outlines = true`. Log: `synthesizing (… style=cel outlines=true)`,
`mdl_*_lambert → … (4 programs, 2 VS, 2 PS)`, per song `… [hull] item built …` lines (one per
body/part), `built … hulls=N`. Visual: 3 flat bands on skin/costume, dark rims on grazing surfaces,
a ~2 px black outline around every dancer and part that survives limb motion; no black interior
patches (depth push too small) and no outline gaps at the silhouette (push too big) — tune
`OUTLINE_PUSH_*`; hair/veils have no outline by design. Then `dancer_outlines=false` next song ⇒
outlines gone; `dancer_lighting="lit"` + relaunch ⇒ the approved smooth look with outlines.
