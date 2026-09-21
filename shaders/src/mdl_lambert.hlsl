// mdl_lambert — lit vertex shaders for the Background Dancers scene
// (program 0 of the runtime-synthesized `mdl_bg_lambert.gsp` /
// `mdl_ch_lambert.gsp` model containers; "Dancer Lighting").
//
// Why these two names: 158 stock character materials NAME `mdl_ch_lambert`
// (every dancer body) / `mdl_bg_lambert` (every attached part) — the artists'
// "lit" tag — but no container of either name ships, so the engine's
// material→shader lookup (FNV-1 of the debug-info name against the registry
// filled from shader.arc) falls back to the UNLIT `gs_model_*_default`
// programs. Synthesizing exactly these two containers lights exactly the
// surfaces the artists marked; the `_constant*` stage props / glows /
// skydome and the floor shadow keep their stock programs.
// (docs/background_dancers_research.md §4.)
//
// Contract: BOTH entry points pair with the game's OWN stock
// `gs_model_default` pixel shader (sliced out of shader.arc at synthesis —
// byte-identical to the skinning variant's PS), whose inputs are
//
//     dcl_texcoord v0.xy      TEXCOORD0 = uv
//     dcl_color    v1         COLOR0    = colour
//     oC0 = tex2D(s0, uv) * v1; texkill(c2.y − stipple(vPos/32).y)
//
// so the lighting factor is folded into COLOR0's rgb here (it multiplies the
// per-draw tint linearly) and alpha / the stipple dissolve / the c2 reads
// stay bit-exact by construction. Everything else follows the stock model VS
// (fxc /dumpbin of the World 20260915 blobs, RE §4.3):
//
//   c14..c17  World                (bound per draw — command 0xE, 4 regs)
//   c18..c21  WorldViewProjection  (command 0x12; row-vector convention:
//                                   o0 = x·c18 + y·c19 + z·c20 + c21)
//   c22       ModelParameters      { bone_count, 1.0, 0, 0 } — .x + .z is
//                                   the bone texture's height
//   c23       ModelUnitParameters.m_color — the per-draw tint
//   c24       parameters.m_vTexAnime { scaleU, scaleV, offU, offV } —
//                                   uv_out = uv / xy + zw (identity on every
//                                   stock lambert material)
//   s3        BoneMatrices (skinned only): a 4×bone_count texture, one bone
//             per ROW: u = 0.125 / 0.375 / 0.625 select matrix rows 0/1/2,
//             v = (bone_index + 0.5) / (c22.x + c22.z), LOD 0; output
//             component k = dot(row_k, (pos, 1)) (3×4 `invBind·bone`, MODEL
//             space). BLENDINDICES arrive as raw floats 0..255 (UBYTE4),
//             BLENDWEIGHT.xyz weight indices 1..3, w0 = 1 − Σ.
//
// Lighting: one fixed WORLD-space key light (a stage rig — the moving stage
// cameras orbit it). `World` is uniform-scale + translation for every item of
// this scene (stage parts identity; the mirrored right forearm's
// diag(−1,−1,−1) inverts normals along with its point-inverted geometry —
// correct), so `normalize(n · World3x3)` is the exact world normal.
//
//     lit = AMBIENT + DIFFUSE · saturate(dot(n_w, L)·(1 − WRAP) + WRAP)
//
// lit ∈ [AMBIENT, AMBIENT + DIFFUSE]. Cabinet history: deploy #1 shipped the
// conservative "shading pass" 0.65 + 0.35 (≤ stock, top-down key) and read
// as SUBTLE — arms/torsos are near-vertical cylinders, so a top-down key
// gives little gradient around them. Deploy #2 = preset B: a lower, more
// lateral key and a range that goes past stock on the lit side (1.2×) so
// it reads as a real key light over A3's fairly dark baked textures. Tune
// the defines below and rebuild the two blobs; the synthesis fingerprint
// includes the blob hashes, so a blob-only redeploy re-synthesizes the
// containers. (COLOR0 is a full-range float interpolator in SM3; hardware
// that still clamps the COLOR semantic to [0,1] would merely cap the lit
// side at stock — never worse than deploy #1.)
//
// Build: scripts/build_shaders.sh (fxc golden path, vs_3_0) — entries
// vs_bg_main / vs_ch_main, compiled once per variant define set (see the
// defines block below and the manifest): mdl_{bg,ch}_lambert.vs (no defines —
// the dancers), mdl_{bg,ch}_lit_uv3[_vc].vs and mdl_ch_lit_notex_vc.vs (the
// stage's `mdl_*_constant*` shapes, paired with each name's own stock PS).

// ── Tunables ─────────────────────────────────────────────────────────────
#define LIT_AMBIENT 0.55
#define LIT_DIFFUSE 0.65
#define LIT_WRAP    0.0        // 0 = Lambert max(N·L,0); 0.5 = half-Lambert
// World space: +Y up, +Z toward the fallback camera (eye (0,1.6,5) → (0,0.9,0)).
// Key from the front-right, ~37° above the horizon (lateral enough to model limbs).
#define LIT_KEY_DIR float3(0.7, 0.6, 0.5)   // normalized in the shader

#include "mdl_common.hlsli"

// ── Variant defines (scripts/build_shaders.sh /D …) ──────────────────────
// (none)      pairs with the stock gs_model_default PS: uv → TEXCOORD0
//             (the dancers' `mdl_*_lambert` fallback shape)
// UV3         pairs with a stock `mdl_*` PS: uv → TEXCOORD3 + the stock fog
//             output (`w·c49.x − c49.y`)
// VCOLOR      multiply COLOR0 into the tint (the `_vc` shapes)
// NOTEX       the `_notex` shape: no uv; COLOR0 = (tint·vc·lit)·c25 + c26
//             (the stock `mdl_*_notex` VS applies vConstatntColor /
//             vOffsetColor in the VERTEX shader; its PS is `oC0 = v0`)

struct VSOut
{
    float4 pos : POSITION;
#if defined(NOTEX)
    float4 col : COLOR0;
#elif defined(UV3)
    float2 uv  : TEXCOORD3;
    float4 col : COLOR0;
    float  fog : FOG;
#else
    float2 uv  : TEXCOORD0;
    float4 col : COLOR0;
#endif
};

float lit_factor(float3 n_world)
{
    float3 L = normalize(LIT_KEY_DIR);
    float ndl = dot(normalize(n_world), L);
    float wrapped = saturate(ndl * (1.0 - LIT_WRAP) + LIT_WRAP);
    return LIT_AMBIENT + LIT_DIFFUSE * wrapped;
}

VSOut lit_out(float4 clip, float3 ns, float2 uv, float4 vcol)
{
    VSOut o;
    o.pos = clip;
#if !defined(NOTEX)
    o.uv  = anim_uv(uv);
#endif
    float lit = lit_factor(to_world_normal(ns));
    float4 col = float4(Tint.rgb * lit, Tint.a);
#if defined(VCOLOR)
    col *= vcol;
#endif
#if defined(NOTEX)
    o.col = float4(col.rgb * ConstColor.rgb + OffsetColor.rgb, col.a);
#else
    o.col = col;
#endif
#if defined(UV3) && !defined(NOTEX)
    o.fog = clip.w * ConstParams1.x - ConstParams1.y;
#endif
    return o;
}

// ── Static (`bg` — the attached parts / static stage props) ─────────────
struct VSInBg
{
    float3 pos : POSITION;
    float3 nrm : NORMAL;
#if !defined(NOTEX)
    float2 uv  : TEXCOORD0;
#endif
#if defined(VCOLOR)
    float4 vc  : COLOR0;
#endif
};

VSOut vs_bg_main(VSInBg i)
{
#if defined(NOTEX)
    float2 uv = float2(0.0, 0.0);
#else
    float2 uv = i.uv;
#endif
#if defined(VCOLOR)
    float4 vc = i.vc;
#else
    float4 vc = float4(1.0, 1.0, 1.0, 1.0);
#endif
    return lit_out(to_clip(i.pos), i.nrm, uv, vc);
}

// ── Skinned (`ch` — the dancer bodies / animated stage parts) ───────────
struct VSInCh
{
    float3 pos : POSITION;
    float4 bi  : BLENDINDICES; // raw palette-local indices as floats
    float3 bw  : BLENDWEIGHT;  // weights of bi.yzw; bi.x gets 1 − Σ
    float3 nrm : NORMAL;
#if !defined(NOTEX)
    float2 uv  : TEXCOORD0;
#endif
#if defined(VCOLOR)
    float4 vc  : COLOR0;
#endif
};

VSOut vs_ch_main(VSInCh i)
{
    SkinIn s;
    s.bi = i.bi;
    s.bw = i.bw;
    float4 R0, R1, R2;
    skin_rows(s, R0, R1, R2);
    float3 ps, ns;
    skin_apply(R0, R1, R2, i.pos, i.nrm, ps, ns);
#if defined(NOTEX)
    float2 uv = float2(0.0, 0.0);
#else
    float2 uv = i.uv;
#endif
#if defined(VCOLOR)
    float4 vc = i.vc;
#else
    float4 vc = float4(1.0, 1.0, 1.0, 1.0);
#endif
    return lit_out(to_clip(ps), ns, uv, vc);
}
