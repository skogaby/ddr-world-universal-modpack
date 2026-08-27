// theme_squares — SQUARES theme animated background: a drifting field of
// softly rotating squares over a deep-blue fbm smoke glow.
//
// Ported from "squares & smoke bg" — https://www.shadertoy.com/view/MdVXzw
// Attribution retained per the project's Shadertoy-port policy.
// Port notes: direct port — the 60-rect loop stays dynamic (`[loop]`);
// GLSL mat2 rotation rewritten component-wise.
//
// Interpolator contract: see themes/theme_common.hlsl. Time wraps mod
// 3600 s — the square spin rate is snapped wrap-seamless; the fbm smoke
// pans and per-rect drift are linear (no finite period), so the hourly
// wrap is a one-frame cut in the background haze (accepted).
//
// Build: scripts/build_shaders.sh (fxc golden path, ps_3_0, entry
// ps_main -> theme_squares.ps.d3dbc).

struct PSIn
{
    float2 uv  : TEXCOORD0;
    float4 tp  : TEXCOORD1; // { time, p0, p1, aspect }
    float4 pxr : TEXCOORD2; // { px_in_rect, rect_w, rect_h }
    float4 col : COLOR0;
};

static const float WRAP = 0.0017453293; // 2*pi/3600
static const float3 BG_COLOR = float3(0.01, 0.16, 0.42);
static const float3 RECT_COLOR = float3(0.01, 0.26, 0.57);
static const float TOTAL = 60.0;   // number of rectangles
static const float MIN_SIZE = 0.03;
static const float MAX_SIZE = 0.05; // (original 0.08 - min)

float random2(float2 co)
{
    return frac(sin(dot(co, float2(12.9898, 78.233))) * 43758.5453);
}

float noise(float2 p)
{
    p *= 2.8; // noiseIntensity
    float2 i = floor(p);
    float2 f = frac(p);
    float2 u = f * f * (3.0 - 2.0 * f);
    return lerp(
        lerp(random2(i), random2(i + float2(1.0, 0.0)), u.x),
        lerp(random2(i + float2(0.0, 1.0)), random2(i + float2(1.0, 1.0)), u.x),
        u.y);
}

float fbm(float2 uv)
{
    uv *= 5.0;
    // GLSL mat2(1.6, 1.2, -1.2, 1.6) * uv (column-major).
    float f = 0.5 * noise(uv);
    uv = float2(1.6 * uv.x - 1.2 * uv.y, 1.2 * uv.x + 1.6 * uv.y);
    f += 0.25 * noise(uv);
    uv = float2(1.6 * uv.x - 1.2 * uv.y, 1.2 * uv.x + 1.6 * uv.y);
    f += 0.125 * noise(uv);
    uv = float2(1.6 * uv.x - 1.2 * uv.y, 1.2 * uv.x + 1.6 * uv.y);
    f += 0.0625 * noise(uv);
    return 0.5 + 0.5 * f;
}

// The smoke-glow backdrop.
float3 bg(float2 uv, float t)
{
    float velocity = t / 1.6;
    float intensity = sin(uv.x * 3.0 + t * 716.0 * WRAP) * 1.1 + 1.5;
    uv.y -= 2.0;
    float2 bp = uv + float2(-2.0, 0.0); // glowPos
    uv *= 0.6;                          // noiseDefinition

    // Ripple.
    float rb = fbm(float2(uv.x * 0.5 - velocity * 0.03, uv.y)) * 0.1;
    uv += rb;

    // Coloring.
    float rz = fbm(uv * 0.9 + float2(velocity * 0.35, 0.0));
    rz *= dot(bp * intensity, bp) + 1.2;

    float3 col = BG_COLOR / (0.1 - rz);
    return sqrt(abs(col));
}

float rectangle(float2 uv, float2 pos, float size, float blur)
{
    float2 p = (size + 0.01) * 0.5 - abs(uv - pos);
    p = smoothstep(0.0, blur, p);
    return p.x * p.y;
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
    // Shadertoy is y-up: uv in [-1, 1], x scaled by aspect.
    float2 uv = float2(i.uv.x, 1.0 - i.uv.y) * 2.0 - 1.0;
    uv.x *= aspect;

    float3 color = bg(uv, t) * (2.0 - abs(uv.y * 2.0));

    // Drifting, spinning squares.
    float velX = -t / 8.0;
    float velY = t / 10.0;
    float spin = t * 286.0 * WRAP; // ~t/2, wrap-seamless
    [loop]
    for (float k = 0.0; k < TOTAL; k += 1.0)
    {
        float index = k / TOTAL;
        float rnd = random2(float2(index, index));
        float2 pos;
        pos.x = frac(velX * rnd + index) * 4.0 - 2.0;
        pos.y = sin(index * rnd * 1000.0 + velY) * 0.5; // yDistribution
        float size = MAX_SIZE * rnd + MIN_SIZE;

        // Rotate the sampling frame about the square's center.
        float2 uv_rot = uv - pos + size * 0.5;
        float a = k + spin;
        float c = cos(a);
        float s = sin(a);
        uv_rot = float2(c * uv_rot.x - s * uv_rot.y, s * uv_rot.x + c * uv_rot.y);
        uv_rot += pos + size * 0.5;

        float rect = rectangle(uv_rot, pos, size, (MAX_SIZE + MIN_SIZE - size) * 0.5);
        color += RECT_COLOR * rect * size / MAX_SIZE;
    }

    color *= 0.8; // sits behind the panel wash + menu text

    return float4(color, rounded_coverage(i.pxr) * i.col.a);
}
