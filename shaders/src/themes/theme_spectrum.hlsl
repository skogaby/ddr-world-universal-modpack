// theme_spectrum — SPECTRUM theme animated background: a rounded-capsule
// frequency-bar visualizer with multi-layer bloom, mirrored about a gray
// center line, over a near-black navy gradient.
//
// Ported from "Claude's Spectrum" by Marco van Hylckama Vlieg (made with
// Claude 4.0 Sonnet) — https://www.shadertoy.com/view/tcyGDz
// Attribution retained per the project's Shadertoy-port policy. Port
// notes: the original samples an audio FFT texture (iChannel0); this
// pipeline has no texture channels, so the spectrum is SYNTHESIZED — a
// low-frequency-biased envelope, two decorrelated travelling sines per
// bar, and a 128 BPM beat pulse that kicks the low end (maintainer
// approved the fake signal). 48 bars trimmed to 32 for the ps_3_0
// dynamic-loop budget.
//
// Interpolator contract: see themes/theme_common.hlsl. Time wraps mod
// 3600 s — every animation term is snapped to n * (2*pi/3600) rad/s
// (n = 2500 / 4200 / 7680 cycles per hour), so the wrap is seamless.
//
// Build: scripts/build_shaders.sh (fxc golden path, ps_3_0, entry
// ps_main -> theme_spectrum.ps.d3dbc).

struct PSIn
{
    float2 uv  : TEXCOORD0;
    float4 tp  : TEXCOORD1; // { time, p0, p1, aspect }
    float4 pxr : TEXCOORD2; // { px_in_rect, rect_w, rect_h }
    float4 col : COLOR0;
};

static const float WRAP = 0.0017453293; // 2*pi/3600
static const float NUM_BARS = 32.0;
static const float MAX_BAR_HEIGHT = 0.62;
static const float BLOOM_INTENSITY = 0.3;
static const float BLOOM_FALLOFF = 2.0;
static const float CENTER_Y = 0.5;

// Synthesized FFT stand-in: t01 = normalized bar position (0 = lows).
float fake_fft(float t01, float t)
{
    float env = 0.75 * pow(1.0 - t01, 1.6) + 0.12;
    float n = 0.5 + 0.28 * sin(t * 2500.0 * WRAP + t01 * 17.0)
                  + 0.22 * sin(t * 4200.0 * WRAP + t01 * 31.0 + 2.0);
    float beat = pow(0.5 + 0.5 * sin(t * 7680.0 * WRAP), 3.0); // 128 BPM
    return saturate(env * (0.30 + 0.70 * n) + beat * 0.25 * pow(1.0 - t01, 3.0));
}

// Original color scheme 0: red/orange -> yellow -> blue across the bars.
float3 bar_color(float t01)
{
    float3 low = float3(1.0, 0.2, 0.1);
    float3 mid = float3(1.0, 1.0, 0.2);
    float3 high = float3(0.2, 0.4, 1.0);
    float3 a = lerp(low, mid, saturate(t01 * 2.0));
    float3 b = lerp(mid, high, saturate((t01 - 0.5) * 2.0));
    return t01 < 0.5 ? a : b;
}

// Distance to a vertical capsule (rounded bar) centered at (cx, CENTER_Y).
float dist_bar(float2 q, float cx, float width, float height)
{
    float2 p = q - float2(cx, CENTER_Y);
    float radius = width * 0.5;
    float half_h = max(height * 0.5, radius);
    float line_half = half_h - radius;
    float2 to_p = p - float2(0.0, clamp(p.y, -line_half, line_half));
    return length(to_p) - radius;
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

    // Aspect-corrected, y-square space: x in [0, aspect], y in [0, 1].
    // (Bars are vertically symmetric — no y flip needed.)
    float2 q = float2(i.uv.x * aspect, i.uv.y);
    float px = 1.0 / i.pxr.w; // one pixel in y-square units

    // Layout: bars centered across ~78% of the rect width.
    float total = 0.78 * aspect;
    float pitch = total / NUM_BARS;
    float bar_w = pitch * 0.42;
    float span = (NUM_BARS - 1.0) * pitch + bar_w;
    float start_x = (aspect - span) * 0.5;

    // Near-black navy gradient base.
    float3 col = lerp(float3(0.020, 0.028, 0.058), float3(0.008, 0.010, 0.028), i.uv.y);

    // Gray center line with a soft glow.
    float overhang = 0.05 * total;
    float line_d = length(q - float2(clamp(q.x, start_x - overhang, start_x + span + overhang), CENTER_Y));
    float line_core = 1.0 - smoothstep(0.0, 0.006, line_d);
    float line_glow = exp(-line_d * BLOOM_FALLOFF / (6.0 * px)) * (BLOOM_INTENSITY * 0.3);
    col += float3(0.6, 0.6, 0.6) * (line_core + line_glow) * 0.85;

    // Frequency bars (screen-blended so overlapping bloom saturates
    // gracefully).
    [loop]
    for (float b = 0.0; b < NUM_BARS; b += 1.0)
    {
        float t01 = b / (NUM_BARS - 1.0);
        // Logarithmic frequency mapping (LOG_SCALE_FACTOR 0.5).
        float freq = fake_fft(pow(t01, 0.5), t);
        float cx = start_x + b * pitch + bar_w * 0.5;
        float h = max(freq * MAX_BAR_HEIGHT, bar_w);

        float d = dist_bar(q, cx, bar_w, h);
        float bar = 1.0 - smoothstep(-px, px, d);
        float glow_size = 12.0 * px;
        float g1 = exp(-max(d, 0.0) * BLOOM_FALLOFF / glow_size) * BLOOM_INTENSITY;
        float g2 = exp(-max(d, 0.0) * BLOOM_FALLOFF * 0.5 / (glow_size * 2.0)) * (BLOOM_INTENSITY * 0.5);

        float3 layer = bar_color(t01) * (bar + g1 + g2) * 0.8;
        col = col + layer - col * layer; // screen blend
    }

    return float4(col, rounded_coverage(i.pxr) * i.col.a);
}
