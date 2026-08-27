// gs_screencommand_judge — index-aware bilinear replacement
//
// Same palette-indexed pipeline and same 4-tap fix as
// gs_screencommand_arrow.hlsl (read that file's header first). Bound by
// screen::JudgeEffectRenderer (receptor hit flash / judge family), which
// draws 96x96 cells from the SAME 768x192 sheet as the arrow renderer.
//
// Stock judge PS differs from the arrow PS in exactly two contract points
// (bytecode decode, .agents/planning/20260719-shader-injection/research/
// judge-and-toolchain.md):
//   1. Palette row V is the compile-time constant 0.15625 (palette row 2),
//      not vertexColor.x.
//   2. The palette color is multiplied by the FULL vertex color (rgba), not
//      just alpha: rgb = palette.rgb * vColor.rgb,
//      a = palette.a * vColor.a * atlas.a.
//
// Build: scripts/build_shaders.sh (vkd3d-compiler, ps_3_0/vs_3_0 d3dbc).

// ---------------------------------------------------------------------------
// Shared (see gs_screencommand_arrow.hlsl for the derivations)
// ---------------------------------------------------------------------------

static const float2 TEXEL   = float2(1.0 / 768.0, 1.0 / 384.0);
// Zero — see gs_screencommand_arrow.hlsl for the derivation (VS-projected
// geometry + D3D9 pixel-center convention ⟹ 1:1 samples hit texel centers).
static const float2 UV_BIAS = float2(0.0, 0.0);

// Stock judge palette row (def c0.z in the stock PS): row 2 of 16 -> center
// V = 2.5/16 = 0.15625.
static const float PALETTE_ROW = 0.15625;

// ---------------------------------------------------------------------------
// Vertex shader (functionally identical to stock; same as arrow VS)
// ---------------------------------------------------------------------------

float4 SamplerParameters      : register(c32);
float4 ScreenCommandBaseColor : register(c22);

struct VSIn
{
    float3 pos : POSITION;
    float2 uv  : TEXCOORD0;
    float4 col : COLOR0;
};

struct VSOut
{
    float4 pos : POSITION;
    float2 uv  : TEXCOORD0;
    float4 col : COLOR0;
};

VSOut vs_main(VSIn i)
{
    VSOut o;
    o.pos = float4(i.pos.xy, 0.0, 1.0);
    o.uv  = i.uv * SamplerParameters.xy + SamplerParameters.zw;
    o.col = i.col * ScreenCommandBaseColor;
    return o;
}

// ---------------------------------------------------------------------------
// Pixel shader (index-aware bilinear, judge contract)
// ---------------------------------------------------------------------------

sampler2D MaterialAtlas   : register(s0);
sampler2D MaterialPalette : register(s1);

struct PSIn
{
    float2 uv  : TEXCOORD0;
    float4 col : COLOR0;
};

float4 tap(float2 uv)
{
    float4 tex = tex2D(MaterialAtlas, uv);
    float4 c   = tex2D(MaterialPalette, float2(tex.r, PALETTE_ROW));
    c.a *= tex.a;
    return c;
}

float4 ps_main(PSIn i) : COLOR
{
    float2 t    = i.uv / TEXEL - 0.5 + UV_BIAS;
    float2 f    = frac(t);
    float2 base = (floor(t) + 0.5) * TEXEL;

    float4 c00 = tap(base);
    float4 c10 = tap(base + float2(TEXEL.x, 0.0));
    float4 c01 = tap(base + float2(0.0, TEXEL.y));
    float4 c11 = tap(base + TEXEL);

    float4 c = lerp(lerp(c00, c10, f.x), lerp(c01, c11, f.x), f.y);
    return c * i.col; // full vertex color multiply (stock judge contract)
}
