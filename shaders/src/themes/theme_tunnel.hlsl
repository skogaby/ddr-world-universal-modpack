// theme_tunnel — TUNNEL theme animated background: an endless raymarched
// ring tunnel of counter-rotating color bands laced with glowing spokes,
// flying forward forever.
//
// Ported from "Tunnel" by bal-khan — https://www.shadertoy.com/view/MlsfWS
// (License: CC BY-NC-SA 3.0 Unported; noncommercial use approved by the
// maintainer, attribution retained. Includes Leon's polar-mod from
// https://www.shadertoy.com/view/XsByWd). Port notes: the LIGHT variant
// (glow accumulation) with FUNKY/HOLES off; GLSL floor-mod replaced
// (HLSL fmod truncates — z runs negative here); `var != 0.` gates
// dropped (cos(...) is never exactly 0); 200 raymarch steps tuned to
// 120 with the under-relaxed step factor raised 0.2 -> 0.25 and the
// step-count AO rescaled to match.
//
// Interpolator contract: see themes/theme_common.hlsl. Time wraps mod
// 3600 s — the camera travels forever down -z (no finite scene period),
// so the hourly wrap is a one-frame jump to another stretch of tunnel
// (accepted: reads as a cut; menus are open for minutes).
//
// Build: scripts/build_shaders.sh (fxc golden path, ps_3_0, entry
// ps_main -> theme_tunnel.ps.d3dbc).

struct PSIn
{
    float2 uv  : TEXCOORD0;
    float4 tp  : TEXCOORD1; // { time, p0, p1, aspect }
    float4 pxr : TEXCOORD2; // { px_in_rect, rect_w, rect_h }
    float4 col : COLOR0;
};

static const float TAU = 6.2831853;
static const float I_MAX = 120.0;
static const float FAR = 50.0;

// Scene outputs beyond the distance (the original's globals).
static float3 ret_col;
static float3 glow;

// GLSL floor-mod (HLSL fmod truncates toward zero).
float glsl_mod(float x, float y)
{
    return x - y * floor(x / y);
}

// Leon's polar-mod: fold p into one of `count` angular sectors.
float2 modA(float2 p, float count)
{
    float an = TAU / count;
    float a = atan2(p.y, p.x) + an * 0.5;
    a = glsl_mod(a, an) - an * 0.5;
    return float2(cos(a), sin(a)) * length(p);
}

float scene(float3 p, float t)
{
    float3 op = p;
    float var = atan2(p.x, p.y);
    // Alternate band spin direction per z-slice: sgn = 1 - 2*mod(floor(z),2).
    float sgn = 1.0 - 2.0 * glsl_mod(floor(p.z), 2.0);
    var = cos(var + floor(p.z) + t * sgn);
    ret_col = 1.0 - float3(0.5 - var * 0.5, 0.5, 0.3 + var * 0.5);

    // Tunnel shell (hollow ring, radius ~1 rippled by the band phase).
    float mind = length(p.xy) - 1.0 + 0.1 * var;
    mind = max(mind, -(length(p.xy) - 0.9 + 0.1 * var));

    // Radial spokes, folded into 50-100 sectors along z.
    float2 folded = modA(p.yx, 50.0 + 50.0 * sin(p.z * 0.25));
    float pz = frac(p.z * 3.0) - 0.5;
    float cyl = length(float2(pz, folded.y)) - 0.0251 - 0.25 * sin(op.z * 5.5);
    cyl = max(cyl, -folded.x + 0.4 + clamp(var, 0.0, 1.0));
    mind = min(mind, cyl);

    // Light accumulation near surfaces.
    float m = max(mind - var * 0.1, 0.0001);
    glow += float3(0.5, 0.8, 0.5) * 0.0125 / (0.01 + m * m);

    return mind;
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
    // Shadertoy fragCoord is y-up; center + normalize by height.
    float2 uv = float2(i.pxr.x - R.x * 0.5, (R.y - i.pxr.y) - R.y * 0.5) / R.y;

    float3 dir = normalize(float3(uv.x, uv.y, -1.0));
    float3 pos = float3(0.0, 0.0, 4.5 - t * 2.0);

    ret_col = float3(0.0, 0.0, 0.0);
    glow = float3(0.0, 0.0, 0.0);

    // March (under-relaxed stepping keeps the band artefacts away).
    float steps = 0.0;
    float dist = 0.0;
    [loop]
    for (float k = 0.0; k < I_MAX; k += 1.0)
    {
        float d = scene(pos + dir * dist, t);
        dist += d * 0.25;
        if (d < 0.0001 || dist > FAR)
            break;
        steps += 1.0;
    }

    float3 col = float3(0.004, 0.008, 0.012); // faint deep-blue floor
    if (dist <= FAR)
        col = ret_col * (1.0 - steps * 0.004);
    col += glow * 0.005125;
    col *= 0.7; // sits behind the panel wash + menu text

    return float4(col, rounded_coverage(i.pxr) * i.col.a);
}
