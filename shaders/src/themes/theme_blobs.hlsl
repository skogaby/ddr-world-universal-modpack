// theme_blobs — BLOBS theme animated background: fifteen gooey metaballs
// wobbling around the rect center, glowing mint where they merge over a
// deep purple body.
//
// Ported from a metaball shader — https://www.shadertoy.com/view/WctXD4
// Attribution retained per the project's Shadertoy-port policy. Port
// notes: direct port (the lightest of the pack) — the ball loop stays
// dynamic (`[loop]`).
//
// Interpolator contract: see themes/theme_common.hlsl. Time wraps mod
// 3600 s — every wobble phase (0.6/0.7/0.8/1.0 rad/s) is snapped to
// n * (2*pi/3600), so this theme is FULLY wrap-seamless.
//
// Build: scripts/build_shaders.sh (fxc golden path, ps_3_0, entry
// ps_main -> theme_blobs.ps.d3dbc).

struct PSIn
{
    float2 uv  : TEXCOORD0;
    float4 tp  : TEXCOORD1; // { time, p0, p1, aspect }
    float4 pxr : TEXCOORD2; // { px_in_rect, rect_w, rect_h }
    float4 col : COLOR0;
};

static const float WRAP = 0.0017453293; // 2*pi/3600
static const float BALL_COUNT = 15.0;

// Anti-aliased rounded-rect coverage for the modal's r=20 corners
// (mirrors the panel synthesis' SDF in src/mods/mod_menu/chrome.rs).
float rounded_coverage(float4 pxr)
{
    float r = 20.0;
    float2 half_wh = pxr.zw * 0.5;
    float2 q = abs(pxr.xy - half_wh) - (half_wh - r);
    float outside = length(max(q, 0.0));
    float inside = min(max(q.x, q.y), 0.0);
    float d = outside + inside - r;
    return saturate(0.5 - d);
}

float4 ps_main(PSIn i) : COLOR
{
    float t = i.tp.x;
    float aspect = i.tp.w;
    // Centered, aspect-corrected (y flip is cosmetic here, kept for
    // parity with the original).
    float2 uv = (float2(i.uv.x, 1.0 - i.uv.y) * 2.0 - 1.0);
    uv.x *= aspect;

    // Wrap-snapped wobble rates: 0.6 -> 344, 0.7 -> 401, 0.8 -> 458,
    // 1.0 -> 573 cycles per hour.
    float w06 = t * 344.0 * WRAP;
    float w07 = t * 401.0 * WRAP;
    float w08 = t * 458.0 * WRAP;
    float w10 = t * 573.0 * WRAP;

    float sum = 0.0;
    [loop]
    for (float fi = 0.0; fi < BALL_COUNT; fi += 1.0)
    {
        float radius = 0.9 + 0.1 * sin(w08 + fi);
        float2 offset = float2(
            sin(w06 + fi * 1.3 + sin(w10 + fi)) * radius,
            cos(w07 + fi * 1.7 + cos(w10 + fi)) * radius);

        float d = length(uv - offset);
        sum += 0.04 / (d * d + 0.01);
    }

    float intensity = smoothstep(1.2, 2.0, sum);
    float3 base = float3(0.5, 0.2, 0.5);
    float3 glow = float3(0.7, 1.0, 0.8);
    float3 col = lerp(base, glow, intensity) * intensity;
    col *= 0.8; // sits behind the panel wash + menu text

    return float4(col, rounded_coverage(i.pxr) * i.col.a);
}
