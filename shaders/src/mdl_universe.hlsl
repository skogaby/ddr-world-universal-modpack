// mdl_universe — DanceDanceRevolution Universe (Xbox 360, 2007)'s per-song character render
// styles as Background Dancers LIGHTING STYLE values (dancers only; the stage takes CEL). RE and
// the Universe sources this reproduces: docs/ddr_universe_render_styles_research.md.
//
// Container layout (shader_layout::model_programs): program 0 = the existing outline pair
// (mdl_cel.hlsl; bound only for the DLL's bit-31 hull records — NEGATIVE / MONOCHROME / SEPIA
// keep Universe's black outline), programs 1..3 = this file's VS + one PS family.
//
// ── VS ── one per material shape. Besides the stock outputs it hands the PS:
//   TEXCOORD1  the world normal (normalised in the PS: Universe's per-pixel N·L)
//   TEXCOORD2  the light-space sphere-map uv, 0.5 + 0.5 · (n·X, n·Y) — Universe's sphere VS
//              (`n_light = n · lookAt(light)`, `oT0 = n_light.xy · 0.5 + 0.5`), per vertex
//              like Universe. Light basis: Z = −L (from the light toward the dancer),
//              X = normalize(up × Z), Y = Z × X, up = +Y; L = CEL's TOON_LIGHT_DIR.
//   TEXCOORD3  the clip position's (x, y, w) — the PS turns it into a screen uv (STENCIL
//              RAINBOW: Universe's full-screen quad spans u 0→1, v 0→1, top-left origin).
//
// ── PS families (one define each; scripts/build_shaders.sh) ──
//   UNI_CONTRAST   saturate(N·L) > 0.5 ? white : black                   (Universe id 10)
//   UNI_HIGHLIGHT  white where > 0.5, discarded elsewhere                (11; Universe blends
//                  alpha 0 there under SRCALPHA/INVSRCALPHA — the same picture)
//   UNI_SHADOW     white where ≤ 0.5, discarded elsewhere                (12)
//   UNI_NEGATIVE   c = min(tex · ramp · COLOR0, 1); 1 − c                (19)
//   UNI_MONO       luma(c) with (0.299, 0.587, 0.114)                    (20)
//   UNI_SEPIA      YCbCr → RGB of (luma(c), −0.2, 0.1)                   (21)
//   UNI_STENCIL    flat fill in COLOR0 (the tint: the DLL cycles Universe's
//                  STENCIL palette colours per bar)                     (8)
//   UNI_INVISIBLE  OUTLINE ONLY's body: rgb 0 under the DLL's additive record flags, depth
//                  written, so only the hull's black ink shows          (4)
//   UNI_SCREENTEX  the style texture (s1) at the screen uv               (9, rainbow.tga)
//   UNI_SPHERE     the style texture (s1) at the sphere uv × COLOR0      (13–18; the DLL binds
//                  the sphere map and, for 14 / 16–18, makes the records additive)
//   ramp = Universe's toon.tga row 0 at u = saturate(N·L), bilinear (toon_ramp below).
//   Every family keeps the MATERIAL texture's alpha (cut-out hair cards stay cut out; Universe's
//   own models have none) and the stock 32×32 stipple dissolve.
//   NOTEX  (PS) the untextured `_notex` shape: tex = (1, 1, 1, COLOR0.a)
//   CCOLOR (PS) `rgb = rgb·c4.rgb + c5.rgb` before the NEGATIVE / MONO / SEPIA transform
//               (the `_c` shapes, like mdl_cel.hlsl)
//   VCOLOR (VS) multiply COLOR0 into the tint — the `_vc` shapes
//
// Build entries: vs_uni_bg_main / vs_uni_ch_main (vs_3_0) [+VCOLOR] -> mdl_{bg,ch}_uni[_vc].vs,
//                ps_uni_main (ps_3_0) + UNI_<FAMILY> [+NOTEX | +CCOLOR] -> mdl_uni_<fam>[...].ps

#include "mdl_common.hlsli"

// CEL's key light (mdl_cel.hlsl TOON_LIGHT_DIR: DSU's LIGHT POSITION 2 in World space). Universe
// uploads its own light from the generic model draw; that value was not traced.
#define UNI_LIGHT_DIR float3(-0.673, 0.305, 0.673)

struct UniVSOut
{
    float4 pos : POSITION;
    float2 uv  : TEXCOORD0;
    float4 col : COLOR0;
    float3 nw  : TEXCOORD1; // world normal (unnormalised)
    float2 sph : TEXCOORD2; // light-space sphere-map uv
    float3 scr : TEXCOORD3; // clip (x, y, w)
};

float2 sphere_uv(float3 n_world)
{
    float3 z = -normalize(UNI_LIGHT_DIR);
    float3 x = normalize(cross(float3(0.0, 1.0, 0.0), z));
    float3 y = cross(z, x);
    float3 n = normalize(n_world);
    return float2(dot(n, x), dot(n, y)) * 0.5 + 0.5;
}

UniVSOut uni_out(float3 ps, float3 ns, float2 uv, float4 vcol)
{
    UniVSOut o;
    float s = length(World0.xyz);
    float3 nw = to_world_normal(ns) / s;
    o.pos = to_clip(ps);
    o.uv  = anim_uv(uv);
    o.col = Tint * vcol;
    o.nw  = nw;
    o.sph = sphere_uv(nw);
    o.scr = float3(o.pos.x, o.pos.y, o.pos.w);
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

UniVSOut vs_uni_bg_main(VSInBg i)
{
#if defined(VCOLOR)
    float4 vc = i.vc;
#else
    float4 vc = float4(1.0, 1.0, 1.0, 1.0);
#endif
    return uni_out(i.pos, i.nrm, i.uv, vc);
}

UniVSOut vs_uni_ch_main(VSInCh i)
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
    return uni_out(ps, ns, i.uv, vc);
}

// PS registers (stock gs_model_default convention + the mdl_* params, as mdl_cel.hlsl).
sampler2D Material           : register(s0);
sampler2D StyleTexture       : register(s1); // the DLL-bound style texture (material slot 1)
sampler2D StippleMaskPattern : register(s15);
float4 PsModelParameters     : register(c2); // .y = dissolve threshold (1 ⇒ none)
float4 PsConstColor          : register(c4);
float4 PsOffsetColor         : register(c5);

// Universe's toon.tga, row 0, as sampled bilinearly at u (texel coordinate t = 128·u − 0.5):
// 142 to texel 38, a slow fall to 135 at 63, then 143 / 192 / 202 / 229 / 250 / 255 at
// 64 / 67 / 68 / 72 / 76 / 78 (within 1/255 of every texel).
float toon_ramp(float u)
{
    float t = clamp(u * 128.0 - 0.5, 0.0, 127.0);
    float v = 255.0;
    v = t <= 78.0 ? lerp(250.0, 255.0, saturate((t - 76.0) / 2.0)) : v;
    v = t <= 76.0 ? lerp(229.0, 250.0, saturate((t - 72.0) / 4.0)) : v;
    v = t <= 72.0 ? lerp(202.0, 229.0, saturate((t - 68.0) / 4.0)) : v;
    v = t <= 68.0 ? lerp(192.0, 202.0, saturate(t - 67.0)) : v;
    v = t <= 67.0 ? lerp(143.0, 192.0, saturate((t - 64.0) / 3.0)) : v;
    v = t <= 64.0 ? lerp(135.0, 143.0, saturate(t - 63.0)) : v;
    v = t <= 63.0 ? lerp(142.0, 135.0, saturate((t - 38.0) / 25.0)) : v;
    return v / 255.0;
}

float n_dot_l(UniVSOut i)
{
    return saturate(dot(normalize(i.nw), normalize(UNI_LIGHT_DIR)));
}

float4 ps_uni_main(UniVSOut i, float2 vpos : VPOS) : COLOR
{
    // Stock stipple dissolve, verbatim.
    float stipple = tex2D(StippleMaskPattern, frac(vpos * (1.0 / 32.0))).y;
    clip(PsModelParameters.y - stipple);

#if defined(NOTEX)
    float4 tex = float4(1.0, 1.0, 1.0, 1.0);
#else
    float4 tex = tex2D(Material, i.uv);
#endif
    float alpha = tex.a * i.col.a;

#if defined(UNI_CONTRAST)
    float3 rgb = n_dot_l(i) > 0.5 ? float3(1.0, 1.0, 1.0) : float3(0.0, 0.0, 0.0);
#elif defined(UNI_HIGHLIGHT)
    clip(n_dot_l(i) - 0.5 - 1e-6);
    float3 rgb = float3(1.0, 1.0, 1.0);
#elif defined(UNI_SHADOW)
    clip(0.5 - n_dot_l(i));
    float3 rgb = float3(1.0, 1.0, 1.0);
#elif defined(UNI_NEGATIVE) || defined(UNI_MONO) || defined(UNI_SEPIA)
    float3 c = tex.rgb * toon_ramp(n_dot_l(i)) * i.col.rgb;
#if defined(CCOLOR)
    c = c * PsConstColor.rgb + PsOffsetColor.rgb;
#endif
    float luma = dot(c, float3(0.29900, 0.58700, 0.11400));
#if defined(UNI_NEGATIVE)
    float3 rgb = 1.0 - min(c, 1.0); // Universe clamps only here
#elif defined(UNI_MONO)
    float3 rgb = luma.xxx;
#else
    // Universe's YCbCr2RGB rows applied to (Y, Cb, Cr) = (luma, −0.2, 0.1).
    float3 rgb = float3(luma + 1.40200 * 0.1,
                        luma - 0.34414 * -0.2 - 0.71414 * 0.1,
                        luma + 1.77200 * -0.2);
#endif
#elif defined(UNI_STENCIL)
    // The fill colour rides the tint: the DLL cycles Universe's palette per bar.
    float3 rgb = i.col.rgb;
#elif defined(UNI_INVISIBLE)
    // OUTLINE ONLY's body: adds nothing under the additive blend the DLL sets
    // but still writes depth, so only the hull's ink shows. Cut-out texels
    // write no depth.
    clip(alpha - 0.5);
    float3 rgb = float3(0.0, 0.0, 0.0);
    alpha = 1.0;
#elif defined(UNI_SCREENTEX)
    float2 ndc = i.scr.xy / i.scr.z;
    float2 suv = float2(0.5 + 0.5 * ndc.x, 0.5 - 0.5 * ndc.y);
    float3 rgb = tex2D(StyleTexture, suv).rgb;
#elif defined(UNI_SPHERE)
    float3 rgb = tex2D(StyleTexture, i.sph).rgb * i.col.rgb;
#else
    // (the VS entries compile this file without a family define)
    float3 rgb = tex.rgb * i.col.rgb;
#endif
    return float4(rgb, alpha);
}
