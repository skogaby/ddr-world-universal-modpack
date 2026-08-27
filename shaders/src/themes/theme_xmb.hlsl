// theme_xmb — XMB theme animated background: the PS3 XrossMediaBar
// ribbon — a translucent glowing wave sheet undulating across a classic
// XMB blue gradient, with drifting dust sparkles hugging the ribbon.
//
// Ported from "XMB Wave" by int_45h — https://www.shadertoy.com/view/fcf3Dn
// ("just do whatever you want with this shader" — MIT-spirited; also
// based on xaot88's starfield https://www.shadertoy.com/view/Md2SR3).
// Attribution retained per the project's Shadertoy-port policy. Port
// notes: the uint Weyl hash has no ps_3_0 analog — replaced with the
// project's sin-dot hash over a mod-289-wrapped lattice (keeps the hash
// argument small so fp precision holds at large star coordinates); the
// 2x2 AA loop is dropped (the ribbon is soft; the final dither stays);
// the iDate day/night mix becomes a wrap-seamless 1-cycle-per-hour
// drift; 100 raymarch steps tuned to 80.
//
// Interpolator contract: see themes/theme_common.hlsl. Time wraps mod
// 3600 s — the wave phases are snapped to n * (2*pi/3600) rad/s; the
// dust-field pans are linear (no finite period), so the wrap is a
// sparkle-field cut hidden under the unchanged wave (accepted).
//
// Build: scripts/build_shaders.sh (fxc golden path, ps_3_0, entry
// ps_main -> theme_xmb.ps.d3dbc).

struct PSIn
{
    float2 uv  : TEXCOORD0;
    float4 tp  : TEXCOORD1; // { time, p0, p1, aspect }
    float4 pxr : TEXCOORD2; // { px_in_rect, rect_w, rect_h }
    float4 col : COLOR0;
};

static const float WRAP = 0.0017453293; // 2*pi/3600
static const float THRESHOLD = 0.99;
static const float MIN_DIST = 0.04;
static const float MAX_DIST = 40.0;
static const float MAX_DRAWS = 40.0;

float hash12(float2 p)
{
    // Lattice inputs get large (star coords + time pan); wrap them so
    // the sin-dot hash keeps its precision.
    p = p - 289.0 * floor(p / 289.0);
    return frac(sin(dot(p, float2(127.1, 311.7))) * 43758.5453);
}

// Cubic-hermite value noise.
float value2d(float2 p)
{
    float2 pg = floor(p);
    float2 pc = p - pg;
    pc *= pc * pc * (3.0 - 2.0 * pc);
    return lerp(
        lerp(hash12(pg), hash12(pg + float2(1.0, 0.0)), pc.x),
        lerp(hash12(pg + float2(0.0, 1.0)), hash12(pg + float2(1.0, 1.0)), pc.x),
        pc.y);
}

// xaot88-style sparse starfield: only near-1 hash cells light up.
float stars_rough(float2 p)
{
    float s = smoothstep(THRESHOLD, 1.0, hash12(p));
    return s >= THRESHOLD ? pow((s - THRESHOLD) / (1.0 - THRESHOLD), 10.0) : s;
}

float get_stars(float2 p, float a, float tt, float t)
{
    float2 pg = floor(p);
    float2 pc = p - pg;
    pc *= pc * pc * (3.0 - 2.0 * pc);
    float s = lerp(
        lerp(stars_rough(pg), stars_rough(pg + float2(1.0, 0.0)), pc.x),
        lerp(stars_rough(pg + float2(0.0, 1.0)), stars_rough(pg + float2(1.0, 1.0)), pc.x),
        pc.y);
    return smoothstep(a, a + tt, s) * pow(value2d(p * 0.1 + t) * 0.5 + 0.5, 8.3);
}

// Dust sparkles: three drifting star layers, kept near the screen edges
// and masked by the ribbon's closest-approach glow (f).
float get_dust(float2 p, float size, float aspect, float f, float t)
{
    float2 pp = p * size * float2(aspect, 1.0);
    return pow(0.64 + 0.46 * cos(p.x * 6.28), 1.7) * f *
        (get_stars(0.1 * pp + t * float2(20.0, -10.1), 0.11, 0.71, t) * 4.0 +
         get_stars(0.2 * pp + t * float2(30.0, -10.1), 0.10, 0.31, t) * 5.0 +
         get_stars(0.32 * pp + t * float2(40.0, -10.1), 0.10, 0.91, t) * 2.0);
}

// The ribbon: a tilted plane displaced by layered waves + value noise.
// Wave phases wrap-snapped: 0.25 -> 143*WRAP, 1.0 -> 573*WRAP,
// 0.2 -> 115*WRAP.
float sdf(float3 p, float t)
{
    p *= 2.0;
    float o = 8.2 * sin(0.05 * p.x + t * 143.0 * WRAP) +
        (0.04 * p.z) *
        sin(p.x * 0.11 + t * 573.0 * WRAP) *
        2.0 * sin(p.z * 0.2 + t * 115.0 * WRAP) *
        value2d(float2(0.03, 0.4) * p.xz + float2(t * 0.5, 0.0));
    return abs(dot(p, normalize(float3(0.0, 1.0, 0.05))) + 2.5 + o * 0.5);
}

float dither(float2 pos)
{
    return frac(52.9829189 * frac(dot(pos, float2(0.06711056, 0.00583715))));
}

// Volumetric glow march through the ribbon shell. Returns
// { accumulated_glow, dust_mask }.
float2 raymarch(float3 d, float jitter, float t)
{
    float tt = jitter * 2.0;
    float a = 0.0;
    float g = MAX_DIST;
    float dr = 0.0;

    [loop]
    for (float i = 0.0; i < 80.0; i += 1.0)
    {
        float3 p = d * tt;
        float ndt = sdf(p, t);
        if (tt > 10.0)
            g = min(g, abs(ndt)); // closest approach (for the dust mask)
        if (tt >= MAX_DIST)
            break;
        if (abs(ndt) < MIN_DIST)
        {
            if (dr > MAX_DRAWS)
                break;
            dr += 1.0;
            // Fade the volume in over depth; accumulate constant glow.
            a += 0.015 * smoothstep(0.0, 0.3, (p.z * 0.9) / 100.0);
            tt += 0.05;
        }
        else
        {
            tt += abs(ndt) * 0.8;
        }
    }
    return float2(a, max(1.0 - g / 3.0, 0.0));
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
    float aspect = i.tp.w;
    float2 R = i.pxr.zw;
    // Shadertoy fragCoord is y-up.
    float2 U = float2(i.pxr.x, R.y - i.pxr.y);
    float2 uv = U / R;

    float3 d = float3((U - 0.5 * R) / R.y, 1.0);
    float2 mg = raymarch(d, dither(U), t);

    // Day/night blend: 1 seamless cycle per hour wrap in place of the
    // original's wall-clock iDate drift.
    float p = sin(t * WRAP + 1.5707963);
    float3 l1 = lerp(float3(0.149, 0.471, 0.569), float3(0.231, 0.231, 0.231), p);
    float3 l2 = lerp(float3(0.075, 0.333, 0.412), float3(0.129, 0.129, 0.129), p);
    float3 l3 = lerp(float3(0.063, 0.329, 0.412), float3(0.149, 0.149, 0.149), p);
    float3 l4 = lerp(float3(0.169, 0.482, 0.580), float3(0.251, 0.251, 0.251), p);

    // Corner gradient (Shadertoy y-up: l3/l4 are the top row).
    float3 c = lerp(lerp(l1, l2, uv.x), lerp(l3, l4, uv.x), uv.y);

    c = lerp(c, float3(1.0, 1.0, 1.0), saturate(mg.x));
    c += get_dust(uv, 2000.0, aspect, mg.y, t) * 0.3;
    c += (dither(U) - 0.5) / 255.0;
    c *= 0.85; // sits behind the panel wash + menu text

    return float4(c, rounded_coverage(i.pxr) * i.col.a);
}
