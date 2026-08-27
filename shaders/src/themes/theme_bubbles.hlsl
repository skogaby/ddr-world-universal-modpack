// theme_bubbles — BUBBLES theme animated background (overlay-menu
// rewrite Step 8): translucent circles drifting slowly upward with a
// gentle horizontal bob, over a dark-teal gradient with a faint warm
// rim on each bubble. Low contrast — sits behind the menu panel.
//
// Interpolator contract: see themes/theme_common.hlsl. Time is wrapped
// mod 3600 s — drift speeds and bob frequencies are wrap-seamless
// (value * 3600 integral).
//
// Build: scripts/build_shaders.sh (fxc golden path, ps_3_0, entry
// ps_main -> theme_bubbles.ps.d3dbc).

struct PSIn
{
    float2 uv  : TEXCOORD0;
    float4 tp  : TEXCOORD1; // { time, p0, p1, aspect }
    float4 pxr : TEXCOORD2; // { px_in_rect, rect_w, rect_h }
    float4 col : COLOR0;
};

float hash2(float2 p)
{
    return frac(sin(dot(p, float2(127.1, 311.7))) * 43758.5453);
}

// One bubble layer: hash-grid of drifting circles. Returns
// { body_alpha, rim_alpha } packed in a float2.
float2 layer(float2 uv, float aspect, float t, float cells, float speed)
{
    float2 g = uv * float2(cells * aspect, cells);
    float colid = floor(g.x);
    // Per-column upward drift with phase jitter.
    g.y += t * speed * cells + hash2(float2(colid, 3.0)) * 8.0;
    float2 cell = floor(g);
    float2 p = frac(g) - 0.5;

    float h = hash2(cell);
    float keep = step(0.45, h);
    // Bubble center wanders inside the cell; horizontal bob at 0.25 Hz
    // (900 cycles per 3600 s — wrap-seamless), phase from the cell hash.
    float2 center = (float2(hash2(cell + 11.0), hash2(cell + 23.0)) - 0.5) * 0.4;
    center.x += sin((t * 0.25 + h) * 6.2831853) * 0.06;
    float r = lerp(0.10, 0.30, hash2(cell + 37.0));

    float d = length(p - center);
    float body = smoothstep(r, r * 0.55, d);
    float rim = smoothstep(r * 0.16, 0.0, abs(d - r * 0.88));
    return float2(body, rim) * keep;
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

    // Dark-teal vertical gradient base (matches the BUBBLES palette).
    float3 top = float3(0.039, 0.157, 0.173);
    float3 bot = float3(0.016, 0.063, 0.078);
    float3 col = lerp(top, bot, i.uv.y);

    // Two parallax bubble layers. speeds 0.01 (36/3600) and 0.02 (72/3600).
    float2 b1 = layer(i.uv, aspect, t, 3.0, 0.01);
    float2 b2 = layer(i.uv + float2(0.47, 0.29), aspect, t, 5.0, 0.02);

    // Translucent teal bodies + a faint warm rim (the palette's accent).
    float3 body_tint = float3(0.10, 0.26, 0.27);
    float3 rim_tint = float3(0.42, 0.30, 0.16);
    col = lerp(col, body_tint, b1.x * 0.45);
    col += rim_tint * b1.y * 0.22;
    col = lerp(col, body_tint, b2.x * 0.30);
    col += rim_tint * b2.y * 0.14;

    return float4(col, rounded_coverage(i.pxr) * i.col.a);
}
