// gs_screencommand_default — perspective vertex shader (program 1 of the
// runtime-synthesized extended container; program 0 is the game's own stock
// VS/PS blob pair, bit-identical).
//
// The default shader is *the* general 2D screencommand shader — the lane
// passes that bind it (shock-arrow glyphs, Note Types Expansion's mine pass)
// are what this perspective variant exists for. It applies the identical
// parameterized perspective map as gs_screencommand_arrow's vs_persp_main,
// but preserves the STOCK pixel-shader contract, because program 1 pairs
// with the stock PS:
//
//   stock VS outputs (decoded from the stock blob):
//     o0     = float4(pos.xy, 0, 1)
//     o1.xy  = uv * c32.xy + c32.zw      (SamplerParameters)
//     o2     = c22 * color               (ScreenCommandBaseColor)
//     o3     = c23                       (ScreenCommandTextureCheckColor)
//   stock PS: texld r0,v0,s0; max r1,r0,v2; mul oC0,r1,v1
//     — v2 is the TEXCOORD1 = c23 feed; OMITTING o3 breaks the max() term.
//
// Constants c48/c49 are the hook DLL's per-side perspective block (identical
// layout to the arrow variant — see gs_screencommand_arrow.hlsl).
//
// Build: scripts/build_shaders.sh (fxc golden path, vs_3_0).

float4 SamplerParameters               : register(c32);
float4 ScreenCommandBaseColor          : register(c22);
float4 ScreenCommandTextureCheckColor  : register(c23);

float4 PerspParams0 : register(c48); // { anchorY_px, cx_px, k, dir }
float4 PerspParams1 : register(c49); // { d_min, z0, ty, reserved }

struct VSIn
{
    float3 pos : POSITION;
    float2 uv  : TEXCOORD0;
    float4 col : COLOR0;
};

struct VSOut
{
    float4 pos  : POSITION;
    float2 uv   : TEXCOORD0;
    float4 col  : COLOR0;
    float4 chk  : TEXCOORD1;
};

VSOut vs_persp_main(VSIn i)
{
    VSOut o;

    // NDC -> pixel space (1280x720 canvas).
    float x_px = (i.pos.x + 1.0) * 640.0;
    float y_px = (1.0 - i.pos.y) * 360.0;

    float anchorY = PerspParams0.x;
    float cx      = PerspParams0.y;
    float k       = PerspParams0.z;
    float dir     = PerspParams0.w;

    // Perspective map (see gs_screencommand_arrow.hlsl for the derivation).
    float d = max((y_px - anchorY) * dir, PerspParams1.x);
    float s = PerspParams1.y * k / (k + d);
    float xq = cx      + (x_px - cx)      * s;
    float yq = anchorY + (y_px - anchorY) * s + PerspParams1.z;

    float w = 1.0 / s;
    o.pos = float4((xq / 640.0 - 1.0) * w, (1.0 - yq / 360.0) * w, 0.0, w);
    o.uv  = i.uv * SamplerParameters.xy + SamplerParameters.zw;
    o.col = i.col * ScreenCommandBaseColor;
    o.chk = ScreenCommandTextureCheckColor;
    return o;
}
