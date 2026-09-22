// mdl_cel — the CEL style (banded lighting + rim ink) and the inverted-hull
// OUTLINE pair for the synthesized `mdl_bg_lambert` / `mdl_ch_lambert`
// model containers ("Dancer Lighting" experiments, 2026-09-17).
//
// Container layout when the cel style is selected (shader_layout::
// model_programs): program 0 = outline VS + outline PS (bound ONLY for draw
// records carrying flag bit 31 — the DLL's hull items), programs 1..3 = cel
// VS + cel PS (ordinary records; the model pass binds program 2). The
// outline pair is packed with the LIT style too (program 0 is inert without
// bit-31 records). RE: docs/background_dancers_research.md §4.6.
//
// Why the ramp lives in the PIXEL shader: a stepped value interpolated
// across a triangle smears back into gradients, so the VS passes N·L and
// the view-frame vectors in TEXCOORD1/2 and the PS quantizes per pixel.
// The PS otherwise reproduces the stock gs_model_default PS exactly:
// `tex2D(s0, uv) × COLOR0` and the 32×32 stipple dissolve
// `texkill(c2.y − tex2D(s15, frac(vPos/32)).y)` (c2 = ModelParameters,
// .y = 1.0 ⇒ no kill).
//
// Outline geometry (no cull-mode control — render states come from the
// shared GPU record): each hull vertex is offset in SCREEN space along the
// projected normal by a constant pixel width, and the outline PS emulates the
// classic inverted hull's FRONT-FACE CULL per pixel — `clip(dot(n_view,
// pos_view))` discards every shell fragment whose surface faces the camera.
// The surviving back-facing shell sits behind the body by the body's own
// thickness (no depth push needed to hide it — only a 1 mm constant margin
// in world metres against z-fighting) and pokes out OUTLINE_PX beyond EVERY
// silhouette, including an arm's edge over the chest (the 2026-09-21 fix:
// the first hull used a facing-dependent depth push of up to ~20 mm, which
// lost to a chest 0–20 mm behind a crossed arm; RE §4.7).
//
// Outline COLOUR = COLOR0 (c23 = the collector's `record colour × item
// tint`) verbatim — the DLL owns it. It writes the layer colour into every
// hull record's own colour at build (`render_item::set_record_colors`; the
// frame board republishes only the item TINT per frame, which stays the
// body's white), so ONE outline pair serves every layer of the LAYERED style
// (black, then red, then blue — `background_dancers/outline.rs`): the layers
// are separate hull items, each as wide again as the base width, and the
// z-test stacks them
// (a narrower hull's back-facing shell fragment comes from a vertex nearer
// the silhouette — shallower on the far side — than a wider hull's at the
// same pixel, so the narrowest is always on top, in any draw order). The
// ink default lives DLL-side (`outline::INK_RGB` = 0.03 grey, the same value
// the first hull baked in here as OUTLINE_RGB × a white tint).
//
// Variant defines (scripts/build_shaders.sh `/D`):
//   VCOLOR  (VS) multiply COLOR0 into the tint — the `_vc` stage shapes
//   CCOLOR  (PS) `rgb = rgb·c4.rgb + c5.rgb` — the `_c` shapes (PS c3..c5 =
//                the material params: c4 vConstatntColor, c5 vOffsetColor)
//   NOTEX   (PS) no texture: colour = COLOR0 (+ CCOLOR); alpha = COLOR0.a
//
// Build: scripts/build_shaders.sh (fxc golden path) — entries
//   vs_cel_bg_main / vs_cel_ch_main (vs_3_0) [+VCOLOR] -> mdl_{bg,ch}_cel[_vc].vs
//   ps_cel_main (ps_3_0) [+CCOLOR | +NOTEX +CCOLOR]      -> mdl_cel[_c|_notex].ps
//   vs_outline_bg_main / vs_outline_ch_main            -> mdl_{bg,ch}_outline.vs
//   ps_outline_main [+NOTEX]                           -> mdl_outline[_notex].ps

#include "mdl_common.hlsli"

// ── Tunables ─────────────────────────────────────────────────────────────
// World-space key (same rig as the lit style — preset B).
#define CEL_KEY_DIR    float3(0.7, 0.6, 0.5)
// Three-band ramp over N·L: levels (multipliers on the texture) + the two
// thresholds + the half-width of each soft edge.
#define CEL_LEVEL0     0.55    // shadow band
#define CEL_LEVEL1     0.90    // mid band
#define CEL_LEVEL2     1.20    // lit band (past stock, like preset B)
#define CEL_THRESH0    0.15    // shadow → mid
#define CEL_THRESH1    0.55    // mid → lit
#define CEL_SOFT       0.03
// Rim ink: darken where 1 − |N·V| passes INK_LO..INK_HI.
#define INK_LO         0.62
#define INK_HI         0.78
#define INK_STRENGTH   0.85    // 1 = pure black rim
// Inverted hull.
// Rim width in 720p pixels — the DEFAULT when the item carries none: the DLL
// writes a per-item width into ModelParameters.w (`item+0x4C`, read by no
// stock shader — `.x/.z` feed the bone texture, `.y` the stipple), so stage
// props and dancers can differ (`background_dancers.outline_px[_stage]`).
#define OUTLINE_PX       2.0
// Metres; the width is CONSTANT on screen up to here and shrinks ∝ 1/w
// beyond (deploy #4/#5 used 5 m and stage props 8–30 m away got a
// sub-pixel rim — invisible — RE §4.7).
#define OUTLINE_REF_DIST 25.0
#define OUTLINE_PUSH_M   0.001  // constant depth margin, WORLD metres (z-fight guard only)
// (No colour constant: the outline colour is COLOR0, written per hull item by
// the DLL — see the header comment.)

// ═══════════════════════════ CEL STYLE ═══════════════════════════════════

struct CelVSOut
{
    float4 pos  : POSITION;
    float2 uv   : TEXCOORD0;
    float4 col  : COLOR0;    // per-draw tint (unlit)
    float4 nv   : TEXCOORD1; // xyz = n_view (unnormalized), w = N·L (world)
    float3 pv   : TEXCOORD2; // pos_view (camera → vertex, unnormalized)
};

CelVSOut cel_out(float3 ps, float3 ns, float2 uv, float4 vcol)
{
    CelVSOut o;
    float4 clip  = to_clip(ps);
    float4 nclip = to_clip_dir(ns);
    ViewFrame f = view_frame(clip, nclip);
    float3 L = normalize(CEL_KEY_DIR);
    o.pos = clip;
    o.uv  = anim_uv(uv);
    o.col = Tint * vcol;
    o.nv  = float4(f.n_view, dot(normalize(to_world_normal(ns)), L));
    o.pv  = f.pos_view;
    return o;
}

struct VSInBg
{
    float3 pos : POSITION;
    float3 nrm : NORMAL;
    float2 uv  : TEXCOORD0;
#if defined(VCOLOR)
    float4 vc  : COLOR0;
#endif
};

struct VSInCh
{
    float3 pos : POSITION;
    float4 bi  : BLENDINDICES;
    float3 bw  : BLENDWEIGHT;
    float3 nrm : NORMAL;
    float2 uv  : TEXCOORD0;
#if defined(VCOLOR)
    float4 vc  : COLOR0;
#endif
};

CelVSOut vs_cel_bg_main(VSInBg i)
{
#if defined(VCOLOR)
    float4 vc = i.vc;
#else
    float4 vc = float4(1.0, 1.0, 1.0, 1.0);
#endif
    return cel_out(i.pos, i.nrm, i.uv, vc);
}

CelVSOut vs_cel_ch_main(VSInCh i)
{
    SkinIn s;
    s.bi = i.bi;
    s.bw = i.bw;
    float4 R0, R1, R2;
    skin_rows(s, R0, R1, R2);
    float3 ps, ns;
    skin_apply(R0, R1, R2, i.pos, i.nrm, ps, ns);
#if defined(VCOLOR)
    float4 vc = i.vc;
#else
    float4 vc = float4(1.0, 1.0, 1.0, 1.0);
#endif
    return cel_out(ps, ns, i.uv, vc);
}

// PS registers (stock gs_model_default PS convention + the mdl_* params).
sampler2D Material           : register(s0);
sampler2D StippleMaskPattern : register(s15);
float4 PsModelParameters     : register(c2); // .y = dissolve threshold (1 ⇒ none)
float4 PsConstColor          : register(c4); // parameters.vConstatntColor (_c shapes)
float4 PsOffsetColor         : register(c5); // parameters.vOffsetColor

float cel_band(float ndl)
{
    float b = CEL_LEVEL0;
    b += (CEL_LEVEL1 - CEL_LEVEL0) * smoothstep(CEL_THRESH0 - CEL_SOFT, CEL_THRESH0 + CEL_SOFT, ndl);
    b += (CEL_LEVEL2 - CEL_LEVEL1) * smoothstep(CEL_THRESH1 - CEL_SOFT, CEL_THRESH1 + CEL_SOFT, ndl);
    return b;
}

float4 ps_cel_main(CelVSOut i, float2 vpos : VPOS) : COLOR
{
    // Stock stipple dissolve, verbatim.
    float stipple = tex2D(StippleMaskPattern, frac(vpos * (1.0 / 32.0))).y;
    clip(PsModelParameters.y - stipple);

#if defined(NOTEX)
    float4 tex = float4(1.0, 1.0, 1.0, 1.0);
#else
    float4 tex = tex2D(Material, i.uv);
#endif
    float band = cel_band(i.nv.w);
    float ndv  = abs(dot(normalize(i.nv.xyz), normalize(i.pv)));
    float ink  = smoothstep(INK_LO, INK_HI, 1.0 - ndv) * INK_STRENGTH;
    float3 rgb = tex.rgb * i.col.rgb * band * (1.0 - ink);
#if defined(CCOLOR)
    rgb = rgb * PsConstColor.rgb + PsOffsetColor.rgb;
#endif
    return float4(rgb, tex.a * i.col.a);
}

// ═══════════════════════════ OUTLINE (HULL) ══════════════════════════════

struct OutlineVSOut
{
    float4 pos : POSITION;
    float2 uv  : TEXCOORD0;
    float4 col : COLOR0;
    float3 nv  : TEXCOORD1; // n_view (unnormalized)
    float3 pv  : TEXCOORD2; // pos_view (unnormalized)
};

OutlineVSOut outline_out(float3 ps, float3 ns, float2 uv)
{
    OutlineVSOut o;
    float4 clip  = to_clip(ps);
    float4 nclip = to_clip_dir(ns);
    ViewFrame f = view_frame(clip, nclip);

    // Exact screen-space direction of the normal at this vertex:
    // d/dt of (clip.xy + t·nclip.xy) / (clip.w + t·nclip.w) at t = 0.
    float2 sd = nclip.xy * clip.w - clip.xy * nclip.w;
    // Pixel space is 16:9 — weight before normalizing so the width is
    // isotropic in pixels, then back to NDC per axis.
    float2 nd = normalize(sd * float2(16.0, 9.0) + 1e-6);
    float px_base = ModelParameters.w > 0.0 ? ModelParameters.w : OUTLINE_PX;
    float px = px_base * saturate(OUTLINE_REF_DIST / max(clip.w, 1e-3));
    float2 ndc_off = nd * px * float2(2.0 / 1280.0, 2.0 / 720.0);

    // Constant depth margin in WORLD metres: Δclip.z = P22 · Δ (clip.z is
    // linear in view depth with slope P22; w untouched so xy stay put).
    o.pos = float4(clip.xy + ndc_off * clip.w, clip.z + f.p22 * OUTLINE_PUSH_M, clip.w);
    o.uv  = anim_uv(uv);
    o.col = Tint;
    o.nv  = f.n_view;
    o.pv  = f.pos_view;
    return o;
}

OutlineVSOut vs_outline_bg_main(VSInBg i)
{
    return outline_out(i.pos, i.nrm, i.uv);
}

OutlineVSOut vs_outline_ch_main(VSInCh i)
{
    SkinIn s;
    s.bi = i.bi;
    s.bw = i.bw;
    float4 R0, R1, R2;
    skin_rows(s, R0, R1, R2);
    float3 ps, ns;
    skin_apply(R0, R1, R2, i.pos, i.nrm, ps, ns);
    return outline_out(ps, ns, i.uv);
}

float4 ps_outline_main(OutlineVSOut i, float2 vpos : VPOS) : COLOR
{
    // Emulated front-face cull: keep only shell fragments whose surface
    // faces AWAY from the camera (the far side of the character, which the
    // body hides everywhere except the OUTLINE_PX beyond each silhouette).
    clip(dot(i.nv, i.pv));
    float stipple = tex2D(StippleMaskPattern, frac(vpos * (1.0 / 32.0))).y;
    clip(PsModelParameters.y - stipple);
#if defined(NOTEX)
    float alpha = i.col.a;
#else
    // Texture alpha keeps cutout shapes (hair cards) through the stock
    // alpha test; colour is the DLL-written layer colour (COLOR0).
    float alpha = tex2D(Material, i.uv).a * i.col.a;
#endif
    return float4(i.col.rgb, alpha);
}
