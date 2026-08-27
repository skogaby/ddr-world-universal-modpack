// theme_ps2 — PS2 theme animated background: seven icy-blue orbs looping
// on nested rotating orbits, dragging smooth glowing trails — the PS2
// startup constellation.
//
// Ported from "PS2 Orbs" — https://www.shadertoy.com/view/33KBz1
// (rotation helpers credited therein to
// https://www.shadertoy.com/view/ldc3z4). Attribution retained per the
// project's Shadertoy-port policy. Port notes: the GLSL `float[11]`
// center table + indexed break-loop has no ps_3_0 analog — the
// piecewise ease (`variableTime`) is unrolled into a constant bracket
// chain; the 24-segment trail is thinned to 12 (still smooth — the
// segments are short); GLSL floor-mod replaced (past-time samples go
// negative right after boot).
//
// Interpolator contract: see themes/theme_common.hlsl. Time wraps mod
// 3600 s — the master cycle is 60 s (divides 3600) and the three free
// orbit rotations are snapped to n * (2*pi/3600), so this theme is
// FULLY wrap-seamless (including the trail's past-time samples).
//
// Build: scripts/build_shaders.sh (fxc golden path, ps_3_0, entry
// ps_main -> theme_ps2.ps.d3dbc).

struct PSIn
{
    float2 uv  : TEXCOORD0;
    float4 tp  : TEXCOORD1; // { time, p0, p1, aspect }
    float4 pxr : TEXCOORD2; // { px_in_rect, rect_w, rect_h }
    float4 col : COLOR0;
};

static const float WRAP = 0.0017453293; // 2*pi/3600
static const float PI = 3.14159265;
static const float DOTS = 7.0;
static const float3 COLOR = float3(0.56, 0.86, 1.0);

float glsl_mod(float x, float y)
{
    return x - y * floor(x / y);
}

// Polynomial smooth max.
float smax(float a, float b, float k)
{
    float h = saturate(0.5 + 0.5 * (b - a) / k);
    return lerp(a, b, h) + k * h * (1.0 - h);
}

float sdSegment(float2 p, float2 a, float2 b)
{
    float2 pa = p - a;
    float2 ba = b - a;
    float h = saturate(dot(pa, ba) / dot(ba, ba));
    return length(pa - ba * h);
}

// The original's float[11] center table, unrolled: piecewise cosine
// ease between consecutive checkpoints {0, .2, .25, 1/3, .4, .5, .6,
// 2/3, .75, .8}; identity above 0.8 (the table's last entry wraps to 0
// and never matches — preserved behavior).
float variableTime(float u)
{
    float lo = 0.75;
    float hi = 0.8;
    if (u <= 0.75) { lo = 0.6666667; hi = 0.75; }
    if (u <= 0.6666667) { lo = 0.6; hi = 0.6666667; }
    if (u <= 0.6) { lo = 0.5; hi = 0.6; }
    if (u <= 0.5) { lo = 0.4; hi = 0.5; }
    if (u <= 0.4) { lo = 0.3333333; hi = 0.4; }
    if (u <= 0.3333333) { lo = 0.25; hi = 0.3333333; }
    if (u <= 0.25) { lo = 0.2; hi = 0.25; }
    if (u <= 0.2) { lo = 0.0; hi = 0.2; }
    float x = saturate((u - lo) / (hi - lo));
    float eased = lerp(lo, hi, -(cos(PI * x) - 1.0) / 2.0);
    return u > 0.8 ? u : eased;
}

// Orbit position of dot `idx` at global time `g`: the eased 60 s cycle
// drives the ring phase, then three nested free rotations (rates
// snapped: 1.1 -> 630, 2.15 -> 1232, 0.52 -> 298 cycles per hour).
float2 dot_pos(float g, float idx)
{
    float anim = variableTime(glsl_mod(g, 60.0) / 60.0);
    float rt = anim * 2.0 * PI * idx;
    float3 p = float3(sin(rt) * 0.5, cos(rt) * 0.5, 0.0);

    float a = g * 630.0 * WRAP; // rY
    float c = cos(a);
    float s = sin(a);
    p = float3(c * p.x + s * p.z, p.y, -s * p.x + c * p.z);

    a = g * 1232.0 * WRAP; // rZ
    c = cos(a);
    s = sin(a);
    p = float3(c * p.x - s * p.y, s * p.x + c * p.y, p.z);

    a = g * 298.0 * WRAP; // rX
    c = cos(a);
    s = sin(a);
    p = float3(p.x, c * p.y - s * p.z, s * p.y + c * p.z);

    return p.xy;
}

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
    // Shadertoy fragCoord is y-up; center + normalize by the short axis.
    float2 U = float2(i.pxr.x, R.y - i.pxr.y);
    float2 st = 2.0 * (U - 0.5 * R) / min(R.x, R.y);

    float dots = 0.0;
    [loop]
    for (float k = 1.0; k <= DOTS; k += 1.0)
    {
        float2 pos = dot_pos(t, k);
        float dist = length(st - pos);
        float d = 2.0 * exp(-dist * 10.0); // core + identical glow

        // Trail: segments between successive past positions (12 taps).
        float2 prev = pos;
        [loop]
        for (float tau = 0.05; tau < 1.0; tau += 0.08)
        {
            float2 past = dot_pos(t - tau, k);
            float trail = exp(-sdSegment(st, past, prev) * 10.0);
            float fade = 1.25 - tau;
            dots = smax(dots, trail * fade * 0.5, 0.75);
            prev = past;
        }

        dots = smax(dots, d * 1.3, 0.8);
    }

    dots -= 0.8;

    float3 col = max(dots, 0.0) * COLOR;
    col *= 0.85; // sits behind the panel wash + menu text

    return float4(col, rounded_coverage(i.pxr) * i.col.a);
}
