// theme_card_swirl — CARD SWIRL theme animated background: a pixelated
// two-tone paint vortex (the Balatro card-back look) churning red and
// blue smoke around the rect center.
//
// Ported from a Balatro-style swirl — https://www.shadertoy.com/view/w3lGzH
// Attribution retained per the project's Shadertoy-port policy. Port
// notes: the original runs `time = iTime + 10`, which permanently
// saturates its `min(6, speed)` / `min(10, time*1.2)` ramp-in terms —
// the port bakes the saturated values in (the intro ramp never shows in
// a menu that opens mid-session); `mid_flash`/`vort_offset` are the
// stock 0 constants, folded.
//
// Interpolator contract: see themes/theme_common.hlsl. Time wraps mod
// 3600 s — the vortex rotation and both smoke phases are snapped to
// n * (2*pi/3600) rad/s, so this theme is FULLY wrap-seamless.
//
// Build: scripts/build_shaders.sh (fxc golden path, ps_3_0, entry
// ps_main -> theme_card_swirl.ps.d3dbc).

struct PSIn
{
    float2 uv  : TEXCOORD0;
    float4 tp  : TEXCOORD1; // { time, p0, p1, aspect }
    float4 pxr : TEXCOORD2; // { px_in_rect, rect_w, rect_h }
    float4 col : COLOR0;
};

static const float WRAP = 0.0017453293; // 2*pi/3600
static const float PIXEL_SIZE_FAC = 700.0;
static const float3 COLOUR_1 = float3(0.996, 0.373, 0.333); // red
static const float3 COLOUR_2 = float3(0.0, 0.616, 1.0);     // blue
static const float3 BLACK_COL = float3(0.186, 0.233, 0.242); // 0.6*(79,99,103)/255

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

    // Centered, aspect-corrected, pixel-snapped UV (the card-art look),
    // then the original's scale=2 zoom-out.
    float2 uv = float2(i.uv.x, 1.0 - i.uv.y) - 0.5;
    uv.x *= aspect;
    uv = floor(uv * (PIXEL_SIZE_FAC / 2.0)) / (PIXEL_SIZE_FAC / 2.0);
    uv /= 2.0;
    float uv_len = length(uv);

    // Center swirl. Saturated form (see header): angle spins at
    // 0.17 rad/s — snapped to 97 cycles/hour; the +10 s epoch shift is a
    // constant phase (-1.7).
    float spin = t * 97.0 * WRAP + 1.7;
    float new_angle = atan2(uv.y, uv.x) + 4.6 * uv_len - 1.0 - spin;
    float2 mid = (R / length(R)) / 2.0;
    float2 sv = float2(uv_len * cos(new_angle) + mid.x,
                       uv_len * sin(new_angle) + mid.y) - mid;

    // Smoke: 5 feedback folds. Phases 0.7867/0.678 rad/s snapped to
    // 451/388 cycles per hour; the 65 s epoch offsets fold to constants.
    sv *= 30.0;
    float ph_a = t * 451.0 * WRAP + 8.523;
    float ph_b = t * 388.0 * WRAP + 7.345;
    float2 uv2 = float2(sv.x + sv.y, sv.x + sv.y);

    [unroll]
    for (int k = 0; k < 5; k++)
    {
        uv2 += sin(max(sv.x, sv.y)) + sv;
        sv += 0.5 * float2(cos(5.1123314 + 0.353 * uv2.y + ph_a),
                           sin(uv2.x - ph_b));
        sv -= cos(sv.x + sv.y) - sin(sv.x * 0.711 - sv.y);
    }

    // Smoke field -> two-tone paint mix (ramp-in saturated: -1.7).
    float smoke_res = min(2.0, max(-2.0, 1.5 + length(sv) * 0.12 - 1.7));
    if (smoke_res < 0.2)
        smoke_res = (smoke_res - 0.2) * 0.6 + 0.2;

    float c1p = max(0.0, 1.0 - 2.0 * abs(1.0 - smoke_res));
    float c2p = max(0.0, 1.0 - 2.0 * smoke_res);
    float cb = 1.0 - min(1.0, c1p + c2p);

    float3 ret_col = COLOUR_1 * c1p + COLOUR_2 * c2p + cb * BLACK_COL;
    float mod_flash = max(0.0, max(c1p, c2p) * 5.0 - 4.4);
    float3 col = ret_col * (1.0 - mod_flash) + mod_flash;
    // The original's pow-1.5 easing.
    col = pow(saturate(col), float3(1.5, 1.5, 1.5));
    col *= 0.55; // sits behind the panel wash + menu text (paint is bright)

    return float4(col, rounded_coverage(i.pxr) * i.col.a);
}
