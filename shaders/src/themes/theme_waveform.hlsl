// theme_waveform — WAVEFORM theme animated background: a raymarched
// mirrored sine-wave ocean sweeping through a slowly cycling neon
// palette, receding to a glowing horizon.
//
// Ported from "Waveform" by @XorDev — https://www.shadertoy.com/view/Wcc3z2
// Attribution retained per the project's Shadertoy-port policy. Port
// notes: the golfed GLSL is expanded (sequenced side effects unrolled
// into statements — note GLSL's left-to-right `max(d = p.z+3., -d*.1)`
// becomes `max(v, -v*0.1)`); the commented-out audio-texture term stays
// out (no texture channels in this pipeline); 90 raymarch steps tuned
// to 60 for the menu (tonemap divisor rescaled to match).
//
// Interpolator contract: see themes/theme_common.hlsl. Time wraps mod
// 3600 s — the per-octave wave phases (2*t*cos(d)) have no common
// period, so the hourly wrap is a one-frame scene cut (accepted: an
// abstract ocean cut reads as motion; menus are open for minutes).
//
// Build: scripts/build_shaders.sh (fxc golden path, ps_3_0, entry
// ps_main -> theme_waveform.ps.d3dbc).

struct PSIn
{
    float2 uv  : TEXCOORD0;
    float4 tp  : TEXCOORD1; // { time, p0, p1, aspect }
    float4 pxr : TEXCOORD2; // { px_in_rect, rect_w, rect_h }
    float4 col : COLOR0;
};

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
    float2 R = i.pxr.zw;
    // Shadertoy fragCoord is y-up; our rect pixels are y-down.
    float2 I = float2(i.pxr.x, R.y - i.pxr.y);

    float3 dir = normalize(float3(I + I, 0.0) - float3(R.x, R.y, R.y));

    float3 O = float3(0.0, 0.0, 0.0);
    float z = 0.0;
    [loop]
    for (float k = 0.0; k < 60.0; k += 1.0)
    {
        // Raymarch sample point.
        float3 p = z * dir;
        // Shift camera; reflect below the waterline (softened mirror).
        p += 1.0;
        float r = max(-p.y, 0.0);
        p.y += r + r;
        // Octaves of travelling sines.
        [unroll]
        for (float w = 1.0; w < 30.0; w += w)
            p.y += cos(p.x * w + 2.0 * t * cos(w) + z) / w;
        // Step forward (reflections are softer).
        float v = p.z + 3.0;
        float d = (0.1 * r + abs(p.y - 1.0) / (1.0 + r + r + r * r)
                   + max(v, -v * 0.1)) / 8.0;
        z += d;
        // Pick color and attenuate.
        O += (cos(z * 0.5 + t + float3(0.0, 2.0, 4.0)) + 1.3) / d / z;
    }

    // Tanh tonemap (divisor rescaled for 60 steps), dimmed for the menu.
    float3 e = exp(-2.0 * O / 600.0);
    float3 col = (1.0 - e) / (1.0 + e) * 0.8;

    return float4(col, rounded_coverage(i.pxr) * i.col.a);
}
