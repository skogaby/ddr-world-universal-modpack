// mdl_cel — the CEL style and the inverted-hull OUTLINE pair for the
// synthesized model containers, reproducing DANCING STAGE UNLEASHED / DDR
// ULTRAMIX (Xbox, 2004) exactly (2026-09-28; replaces the 2026-09-17 3-band +
// rim-ink cel and the screen-space-width outline). RE:
// docs/dancing_stage_unleashed_dancers_port_feasibility.md §4.4 / §5.
//
// Container layout when the cel style is selected (shader_layout::
// model_programs): program 0 = outline VS + outline PS (bound ONLY for draw
// records carrying flag bit 31 — the DLL's hull items), programs 1..3 = cel
// VS + cel PS (ordinary records; the model pass binds program 2). The
// outline pair is packed with the LIT style too (program 0 is inert without
// bit-31 records). World RE: docs/background_dancers_research.md §4.6.
//
// ── CEL = DSU's ToonLitShadowMapPixelShader.xpu + the "Toon vertex shader
// for K3DModel objects" ──────────────────────────────────────────────────
//   VS: oT1.xy = dp3(N_skinned, c16)  — N·L per VERTEX, on the skinned normal
//       as blended (NOT renormalised), against the model-space light
//       direction; interpolated across the triangle.
//   PS: rgb = tex0 · toon(oT1) · lightColour · (shadowed ? 0.5 : 1)
//       toon.tga: 128 texels, 0..63 = 142/255, 64..127 = 255, sampled
//       BILINEAR + CLAMP (stage-1 states, FUN_0006cd70) — so the band edge is
//       the lerp between texel centres 63.5/128 and 64.5/128, and N·L < 0
//       clamps to the dark band.
//   Reproduced here per pixel on the interpolated per-vertex N·L (the same
//   structure — the step is taken in the PS so it stays hard). Light:
//   DSU's LIGHT POSITION 2 (46 of 47 songs start there, 45 keep it; 1-based into the
//   48-entry table of FUN_0006a440: 30° above, 45° around from the front, on
//   the dancer's RIGHT, aimed at the chest) mapped into World's space (dancer
//   faces +Z, its left is +X): L = normalize(−0.673, 0.305, 0.673). Light
//   colour = LIGHT COLOR 1, white. NOT reproduced: the 1024² self-shadow
//   buffer (× 0.5 where occluded) — World has no depth pass for models.
//   Everything else is the stock gs_model_default PS: `tex2D(s0, uv) ×
//   COLOR0` (the per-draw tint / vertex colour stay multiplied in) and the
//   32×32 stipple dissolve `texkill(c2.y − tex2D(s15, frac(vPos/32)).y)`.
//
// ── OUTLINE = DSU's "Toon outline vertex shader for K3DModel objects" ────
//   pos += N_skinned · (c18.x + c18.y · clip.w · c18.w), c18 = {0.03, 0.3, 0,
//   0.001} in DSU MODEL units (then × the 1.25 model scale), colour
//   c3 = (0, 0, 0, 1), drawn with CULL CCW (the inverted hull). In metres
//   (0.0821 m per DSU world unit — the pelvis-height match of the port):
//   world push = OUTLINE_BASE_M + OUTLINE_DEPTH_K · w, w = view depth (m).
//   The push is applied in MODEL space along the unnormalised skinned
//   normal, divided by the item's uniform World scale so it is metres in the
//   world. ModelParameters.w (`item+0x4C`, read by no stock shader) is a
//   multiplier the DLL writes (1.0 = DSU); 0 reads as 1.
//   No cull-mode control here (render states come from the shared GPU
//   record), so the outline PS emulates the FRONT-FACE CULL per pixel —
//   `clip(dot(n_view, pos_view))` discards every shell fragment whose
//   surface faces the camera; the surviving back-facing shell sits behind
//   the body and pokes out beyond every silhouette. No depth margin (DSU
//   has none; the push is ≥ 3 mm).
//   Colour = COLOR0 (c23 = the collector's `record colour × item tint`),
//   written by the DLL into every hull record (`outline::INK_RGB`, black).
//   Texture alpha is kept so cutout cards (stock hair) stay cut out; DSU's
//   own textures are opaque.
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

// ── DSU constants ────────────────────────────────────────────────────────
// Toward the light, World space (DSU LIGHT POSITION 2, see the header).
#define TOON_LIGHT_DIR   float3(-0.673, 0.305, 0.673)
// toon.tga's two texel values (gamma-space, like every World/Xbox shader).
#define TOON_DARK        (142.0 / 255.0)
#define TOON_LIT         1.0
// Bilinear edge: texel centres 63.5/128 → 64.5/128.
#define TOON_EDGE_LO     (63.5 / 128.0)
#define TOON_EDGE_SCALE  128.0
// Outline push, world metres: 1.25 · 0.03 DSU units · 0.0821 m, and the
// dimensionless 1.25 · 0.3 · 0.001 per metre of view depth.
#define OUTLINE_BASE_M   0.00308
#define OUTLINE_DEPTH_K  0.000375

// ═══════════════════════════ CEL STYLE ═══════════════════════════════════

struct CelVSOut
{
    float4 pos : POSITION;
    float2 uv  : TEXCOORD0;
    float4 col : COLOR0;    // per-draw tint (unlit)
    float  ndl : TEXCOORD1; // DSU oT1: N·L per vertex (skinned normal, not renormalised)
};

CelVSOut cel_out(float3 ps, float3 ns, float2 uv, float4 vcol)
{
    CelVSOut o;
    // DSU dots the model-space normal with a model-space light; with a
    // uniform-scale World that is the world normal / scale against the
    // world light.
    float s = length(World0.xyz);
    o.pos = to_clip(ps);
    o.uv  = anim_uv(uv);
    o.col = Tint * vcol;
    o.ndl = dot(to_world_normal(ns) / s, normalize(TOON_LIGHT_DIR));
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

// toon.tga at u = N·L, bilinear + clamp.
float toon_ramp(float ndl)
{
    return lerp(TOON_DARK, TOON_LIT, saturate((ndl - TOON_EDGE_LO) * TOON_EDGE_SCALE));
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
    float3 rgb = tex.rgb * i.col.rgb * toon_ramp(i.ndl);
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
    // DSU: w of the UN-pushed vertex scales the push (`m4x4 r1, r0, c12`).
    float w = to_clip(ps).w;
    float s = length(World0.xyz);
    float k = ModelParameters.w > 0.0 ? ModelParameters.w : 1.0;
    float push_m = (OUTLINE_BASE_M + OUTLINE_DEPTH_K * w) * k;
    float3 pushed = ps + ns * (push_m / s);

    float4 clip  = to_clip(pushed);
    float4 nclip = to_clip_dir(ns);
    ViewFrame f = view_frame(clip, nclip);
    o.pos = clip;
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
    // Emulated front-face cull (DSU draws the hull with CULL CCW): keep only
    // shell fragments whose surface faces AWAY from the camera.
    clip(dot(i.nv, i.pv));
    float stipple = tex2D(StippleMaskPattern, frac(vpos * (1.0 / 32.0))).y;
    clip(PsModelParameters.y - stipple);
#if defined(NOTEX)
    float alpha = i.col.a;
#else
    // Texture alpha keeps cutout shapes (hair cards) through the stock
    // alpha test; colour is the DLL-written ink (COLOR0).
    float alpha = tex2D(Material, i.uv).a * i.col.a;
#endif
    return float4(i.col.rgb, alpha);
}
