// theme_common — shared passthrough vertex shader for the mod-menu's
// animated background programs (overlay-menu rewrite Step 8).
//
// One VS serves every theme pixel shader. The engine has NO pixel-shader
// constant record (docs/overlay_draw_research.md), so everything a theme
// PS needs is forwarded through interpolators:
//
//   TEXCOORD0 = rect-normalized UV (0..1 across the modal rect — derived
//               from the NDC position + the c48/c49 rect; the untextured
//               tag-0x03 quad path supplies no usable UV stream)
//   TEXCOORD1 = { time_seconds, theme_param0, theme_param1, rect aspect }
//   TEXCOORD2 = { px_in_rect_x, px_in_rect_y, rect_w_px, rect_h_px }
//               (pixel-space position within the rect + the rect dims —
//               the theme PS rounds the modal's corners with these,
//               matching the panel's r=20 rounded-rect coverage)
//   COLOR0    = the quad's vertex color, passed raw (the emitter's
//               master-fade lever — the MENU OPACITY percent rides its
//               alpha; theme PS multiplies its alpha by it)
//
// Constant block (design §5; the emitter re-emits these per pass, so
// sharing the c48/c49 window with player_perspective is safe):
//   c48 = { time_seconds, rect_x_px, rect_y_px, unused }
//   c49 = { rect_w_px, rect_h_px, theme_param0, theme_param1 }
//
// Time arrives wrapped modulo 3600 s — theme shaders must use
// wrap-seamless frequencies (f * 3600 integral) so the hourly wrap is
// invisible.
//
// Build: scripts/build_shaders.sh (fxc golden path, vs_3_0, entry
// vs_theme_main -> theme_passthrough.vs.d3dbc).

float4 ThemeParams0 : register(c48); // { time_s, rect_x_px, rect_y_px, unused }
float4 ThemeParams1 : register(c49); // { rect_w_px, rect_h_px, p0, p1 }

struct VSIn
{
    float3 pos : POSITION; // NDC (matches the stock screencommand decl)
    float2 uv  : TEXCOORD0;
    float4 col : COLOR0;
};

struct VSOut
{
    float4 pos : POSITION;
    float2 uv  : TEXCOORD0; // rect-normalized 0..1
    float4 tp  : TEXCOORD1; // { time, p0, p1, aspect }
    float4 pxr : TEXCOORD2; // { px_in_rect, rect_w, rect_h }
    float4 col : COLOR0;
};

VSOut vs_theme_main(VSIn i)
{
    VSOut o;
    o.pos = float4(i.pos.xy, 0.0, 1.0);

    // NDC -> pixel space (1280x720 canvas), then normalize into the rect.
    float x_px = (i.pos.x + 1.0) * 640.0;
    float y_px = (1.0 - i.pos.y) * 360.0;
    float2 in_rect = float2(x_px - ThemeParams0.y, y_px - ThemeParams0.z);
    o.uv = in_rect / ThemeParams1.xy;

    o.tp = float4(ThemeParams0.x, ThemeParams1.z, ThemeParams1.w,
                  ThemeParams1.x / ThemeParams1.y);
    o.pxr = float4(in_rect, ThemeParams1.x, ThemeParams1.y);
    o.col = i.col;
    return o;
}
