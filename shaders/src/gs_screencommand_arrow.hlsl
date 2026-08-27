// gs_screencommand_arrow — index-aware bilinear replacement
//
// DDR World's lane art (arrows, receptors, freeze bodies, shock effects) is
// PALETTE-INDEXED: the atlas RED channel is a U index into a 256x16 palette
// texture (s1) whose rows the game rewrites every frame (note-color beat
// animation). The stock PS takes ONE atlas tap, so scaled art (playfield
// styling) shows nearest-texel staircase aliasing — and hardware LINEAR on
// the atlas is NOT an option (blends palette INDICES -> banding;
// cabinet-proven, see docs/playfield_styling_research.md §7).
//
// Fix: 4 atlas taps at the surrounding texel centers, palette-lookup EACH,
// bilinearly blend the resulting COLORS. Collapses to the exact stock texel
// at 1:1 (see UV_BIAS note), so stock play is unaffected.
//
// Contracts (engine binds by register, not CTAB name — see
// docs/shader_replacement_research.md):
//   VS: c32 = SamplerParameters (live-verified identity (1,1,0,0)),
//       c22 = ScreenCommandBaseColor. Semantics POSITION/TEXCOORD0/COLOR0.
//   PS: s0 = Material[0] atlas, s1 = Material[1] palette.
//   Stock arrow PS output: rgb = palette.rgb (vertex color rgb NOT applied),
//   a = palette.a * atlas.a * vertexColor.a; palette row V = vertexColor.x.
//
// Build: scripts/build_shaders.sh (vkd3d-compiler, ps_3_0/vs_3_0 d3dbc).

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

// Texel size. 768/384 are common multiples of every sheet this shader binds
// (arrow%02d 768x192, shock_effect00 768x384, lane_notice00 192x384), so the
// 1:1 collapse below holds for all of them; smaller sheets just get a
// narrower blend window when scaled (crisper, never banded).
static const float2 TEXEL = float2(1.0 / 768.0, 1.0 / 384.0);

// UV_BIAS re-centers the filter grid if a target ever samples off texel
// centers at 1:1. For DDR World it is ZERO: c32 is identity (live CE read)
// and the shader draws VS-PROJECTED geometry (not pre-transformed/RHW), so
// under D3D9's pixel-center convention the interpolated uv at each pixel
// center lands on a texel CENTER at 1:1 — frac==0, the 4 taps collapse to
// the stock texel. Confirmed two ways: stock POINT sampling is stable/crisp
// at 1:1 (edge-aligned samples would seam-flicker under floor()), and the
// LINEAR-sampled shock/lane sheets are crisp at 1:1 (edge samples would be
// permanently 50/50-blurred). Left as a knob for future shaders/targets.
static const float2 UV_BIAS = float2(0.0, 0.0);

// ---------------------------------------------------------------------------
// Vertex shader (functionally identical to stock)
// ---------------------------------------------------------------------------

float4 SamplerParameters      : register(c32);
float4 ScreenCommandBaseColor : register(c22);

struct VSIn
{
    float3 pos : POSITION;
    float2 uv  : TEXCOORD0;
    float4 col : COLOR0;
};

struct VSOut
{
    float4 pos : POSITION;
    float2 uv  : TEXCOORD0;
    float4 col : COLOR0;
};

VSOut vs_main(VSIn i)
{
    VSOut o;
    o.pos = float4(i.pos.xy, 0.0, 1.0);
    o.uv  = i.uv * SamplerParameters.xy + SamplerParameters.zw;
    o.col = i.col * ScreenCommandBaseColor;
    return o;
}

// ---------------------------------------------------------------------------
// Perspective vertex shader (program 1 — the StepMania perspective family)
//
// One parameterized map serves every preset (HALLWAY / DISTANT): the quad's
// NDC position (already flat 2D, w=1 from the game) is mapped back to pixel
// space, put through the hyperbolic map about the preset's anchor, and
// re-emitted with a REAL w — the rasterizer's perspective divide +
// perspective-correct interpolation then handle foreshortening and UV
// correctness (single-quad freeze bodies stay straight and correctly
// tiled). Which end of the lane recedes, the base zoom, and the receptor
// realignment shift are all constant choices made by the hook DLL (design
// §Data Models of the perspective-expansion feature).
//
// Constants (uploaded per side per frame by the hook DLL as a tag-0x14
// SetVSConstantF record ahead of the lane pass):
//   c48 = { anchorY_px, cx_px, k, dir }  s=z0 anchor row Y (receptor row
//                                        for HALLWAY, mid-field for
//                                        DISTANT), X convergence target
//                                        (lane center), focal length (px),
//                                        effective receding direction
//                                        (preset tilt x reverse flag)
//   c49 = { d_min, z0, ty, 0 }           approach-distance clamp (= -0.5*k:
//                                        caps growth past the anchor at
//                                        2x*z0 and guarantees w > 0), base
//                                        zoom about the anchor (1.0 for the
//                                        no-zoom presets — NEVER 0), rigid
//                                        vertical realignment shift putting
//                                        the mapped receptor row back at
//                                        its stock height (0 for HALLWAY)
//
// Pixel-space reconstruction uses the standard full-screen layer context
// (canvas 1280x720, scale={1/1280,1/720}, offset={0,0} — engine-verified).
//
// Linearity argument (why the projective map is exact per quad): d is linear
// in y_px, so w = (k+d)/(k*z0) is linear; x'*w = cx*w + (x_px-cx) and
// (y'+ty)*w carry no x*y products — all clip-space outputs are affine in the
// input vertex, which is exactly the premise of perspective-correct
// rasterization.
// ---------------------------------------------------------------------------

float4 PerspParams0 : register(c48); // { anchorY_px, cx_px, k, dir }
float4 PerspParams1 : register(c49); // { d_min, z0, ty, reserved }

VSOut vs_persp_main(VSIn i)
{
    VSOut o;

    // NDC -> pixel space (1280x720 canvas).
    float x_px = (i.pos.x + 1.0) * 640.0;
    float y_px = (1.0 - i.pos.y) * 360.0;

    float anchorY = PerspParams0.x;
    float cx      = PerspParams0.y;
    float k       = PerspParams0.z;
    float dir     = PerspParams0.w;

    // Perspective map: signed receding distance from the anchor (clamped),
    // hyperbolic scale with base zoom, converge x/y toward (cx, anchorY),
    // then the rigid receptor-realignment shift.
    float d = max((y_px - anchorY) * dir, PerspParams1.x);
    float s = PerspParams1.y * k / (k + d);
    float xq = cx      + (x_px - cx)      * s;
    float yq = anchorY + (y_px - anchorY) * s + PerspParams1.z;

    // Real w so the rasterizer divides and interpolates perspective-correctly.
    float w = 1.0 / s;
    o.pos = float4((xq / 640.0 - 1.0) * w, (1.0 - yq / 360.0) * w, 0.0, w);
    o.uv  = i.uv * SamplerParameters.xy + SamplerParameters.zw;
    o.col = i.col * ScreenCommandBaseColor;
    return o;
}

// ---------------------------------------------------------------------------
// Pixel shader (index-aware bilinear)
// ---------------------------------------------------------------------------

sampler2D MaterialAtlas   : register(s0);
sampler2D MaterialPalette : register(s1);

struct PSIn
{
    float2 uv  : TEXCOORD0;
    float4 col : COLOR0;
};

// One palette-resolved tap: atlas index -> palette color, alpha composed
// per-tap (palette.a * atlas.a) so edge alpha blends smoothly too.
float4 tap(float2 uv, float row)
{
    float4 tex = tex2D(MaterialAtlas, uv);
    float4 c   = tex2D(MaterialPalette, float2(tex.r, row));
    c.a *= tex.a;
    return c;
}

float4 ps_main(PSIn i) : COLOR
{
    float2 t    = i.uv / TEXEL - 0.5 + UV_BIAS;
    float2 f    = frac(t);
    float2 base = (floor(t) + 0.5) * TEXEL;

    float row = i.col.x; // palette row V rides in vertex color .x (stock)

    float4 c00 = tap(base,                        row);
    float4 c10 = tap(base + float2(TEXEL.x, 0.0), row);
    float4 c01 = tap(base + float2(0.0, TEXEL.y), row);
    float4 c11 = tap(base + TEXEL,                row);

    float4 c = lerp(lerp(c00, c10, f.x), lerp(c01, c11, f.x), f.y);
    c.a *= i.col.a; // vertex alpha applied once (stock contract)
    return c;
}
