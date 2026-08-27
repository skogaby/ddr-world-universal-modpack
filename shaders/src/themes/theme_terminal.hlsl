// theme_terminal — TERMINAL theme animated background: a wall of glitchy
// green phosphor dot-matrix digits with rolling scanline brightness and
// occasional horizontal signal tearing, straight off a broken CRT.
//
// Ported from "Terminal screen" — https://www.shadertoy.com/view/MlsGDs
// (fbm-driven dot-matrix intensity + scanline displacement). Attribution
// retained per the project's Shadertoy-port policy. Port notes:
// - GLSL int dot-grid math rewritten as float (ps_3_0 has no int ALU);
// - the original's 3x3 neighbor supersample glow (9 extra full pattern
//   evaluations) is replaced by a soft second threshold on the same
//   intensity field — visually equivalent halo at ~1/10 the cost;
// - the fbm/pattern intensity is hoisted out of the dot sampling (it is
//   constant per character cell), so the pattern runs once per pixel.
//
// Interpolator contract: see themes/theme_common.hlsl. Time wraps mod
// 3600 s — the slow fbm phases are snapped to n * (2*pi/3600) rad/s so
// the hourly wrap is seamless; the fast glitch flicker terms are
// erratic-by-design and mask their own wrap.
//
// Build: scripts/build_shaders.sh (fxc golden path, ps_3_0, entry
// ps_main -> theme_terminal.ps.d3dbc).

struct PSIn
{
    float2 uv  : TEXCOORD0;
    float4 tp  : TEXCOORD1; // { time, p0, p1, aspect }
    float4 pxr : TEXCOORD2; // { px_in_rect, rect_w, rect_h }
    float4 col : COLOR0;
};

// One wrap-seamless cycle: n * WRAP rad/s completes exactly n cycles
// per 3600 s time wrap.
static const float WRAP = 0.0017453293;

float2 rot(float2 v, float a)
{
    float c = cos(a);
    float s = sin(a);
    // GLSL mat2(c,-s,s,c) * v (column-major).
    return float2(c * v.x + s * v.y, -s * v.x + c * v.y);
}

// tn = the precomputed sin(time/11)-style wobble term.
float noise(float2 p, float tn)
{
    return sin(p.x * 10.0) * sin(p.y * (3.0 + tn)) + 0.2;
}

// tr = per-octave rotation angle (time/50 analog, wrap-snapped).
float fbm(float2 p, float tr, float tn)
{
    p *= 1.1;
    float f = 0.0;
    float amp = 0.5;
    [unroll]
    for (int i = 0; i < 3; i++)
    {
        f += amp * noise(p, tn);
        p = rot(p, tr * float(i * i));
        p *= 2.0;
        amp /= 2.2;
    }
    return f;
}

// Domain-warped fbm: the per-cell "digit intensity" field.
float pattern(float2 p, float tr, float tn, float trq)
{
    float2 q = float2(fbm(p + 1.0, tr, tn), fbm(rot(p, trq) + 1.0, tr, tn));
    float2 r = float2(fbm(rot(q, 0.1), tr, tn), fbm(q, tr, tn));
    return fbm(p + r, tr, tn);
}

float onOff(float t, float a, float b, float c)
{
    return step(c, sin(t * 573.0 * WRAP + a * cos(t * b * 573.0 * WRAP)));
}

// Horizontal tear: a narrow window that sweeps down the screen and
// shears the sampling x when the glitch gate is open.
float displace(float2 look, float t)
{
    float y = look.y - frac(t * 0.25); // 900 cycles / 3600 s
    float window = 1.0 / (1.0 + 50.0 * y * y);
    return sin(look.y * 20.0 + t * 573.0 * WRAP) / 80.0 * onOff(t, 4.0, 2.0, 0.8)
        * (1.0 + cos(t * 34377.0 * WRAP)) * window;
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
    float time = t / 3.0; // the original's slow clock

    // Wrap-snapped slow phases (see header).
    float tn = sin(t * 17.0 * WRAP);  // ~ sin(time/11)
    float tr = t * 4.0 * WRAP;        // ~ time/50 octave rotation
    float trq = t * 19.0 * WRAP;      // ~ 0.1*time domain rotation

    // Shadertoy is y-up; our rect UV is y-down.
    float2 p = float2(i.uv.x, 1.0 - i.uv.y);

    // Rolling scanline brightness bar (pre-displacement y, as original).
    float bar = frac(p.y + time * 20.0) < 0.2 ? 1.4 : 1.0;
    p.x += displace(p, t);

    // Character grid (45 columns at 16:9 in the original; scale with the
    // rect aspect so the dots stay square).
    float2 grid = float2(aspect * 25.3125, 15.0);
    float2 g = p * grid;
    float2 s = floor(g) / grid;
    float intensity = pattern(s / 10.0, tr, tn, trq) * 1.3 - 0.03;

    // 5x5 dot matrix inside the cell (1/1.2 of it — the rest is margin).
    float2 pc = frac(g) * 1.2;
    float x = frac(pc.x * 5.0);
    float y = frac((1.0 - pc.y) * 5.0);
    float row = floor((1.0 - pc.y) * 5.0);
    float col_i = floor(pc.x * 5.0);
    float f = ((row - 2.0) * (row - 2.0) + (col_i - 2.0) * (col_i - 2.0)) / 16.0;
    float inside_cell = step(pc.x, 1.0) * step(pc.y, 1.0);
    float lum = (0.2 + y * 0.8) * (0.75 + x * 0.25);

    float core = step(0.1, intensity - f) * lum * inside_cell;
    // Soft halo threshold in place of the original's 9-tap neighbor sum.
    float glow = smoothstep(0.0, 0.2, intensity - f) * lum * inside_cell;

    // Faint dark-green base so the field never reads as dead-black.
    float3 col = lerp(float3(0.012, 0.030, 0.016), float3(0.004, 0.012, 0.006), i.uv.y);
    col += float3(0.9, 0.9, 0.9) * core * 0.55;
    col += float3(0.0, 1.0, 0.0) * glow * bar * 0.6;
    col *= 0.8; // sits behind the panel wash + menu text

    return float4(col, rounded_coverage(i.pxr) * i.col.a);
}
