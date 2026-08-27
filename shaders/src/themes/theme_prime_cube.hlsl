// theme_prime_cube — PRIME CUBE theme animated background: a tumbling
// translucent voxel lattice where cells light up by the primality of
// their grid coordinates — all-prime cells blaze green, any-prime cells
// glow blue-violet, composites stay pale.
//
// Ported from "Prime Cube" — https://www.shadertoy.com/view/w3V3DG
// Attribution retained per the project's Shadertoy-port policy. Port
// notes: ps_3_0 has no integer ALU — the grid coordinates only span
// -10..10 inside the box, so `isPrime` collapses to an exact float
// lookup (n ∈ {2,3,5,7}); GLSL floor-mod replaced for the wall lattice
// (p goes negative); the marcher's `continue` becomes a predicated
// straight-line body with ONE top-level break — dynamic flow stays
// SHALLOW (the MANDELBULB lesson: deep-nested dynamic flow fails
// D3DMetal's buildPipelineState under CrossOver and freezes the game);
// 256 x 0.01 steps re-tuned to 150 x 0.012 (per-sample alpha rescaled)
// for the menu budget.
//
// Interpolator contract: see themes/theme_common.hlsl. Time wraps mod
// 3600 s — the cube tumble (0.5 rad/s) is snapped to 286 cycles per
// hour, so this theme is FULLY wrap-seamless.
//
// Build: scripts/build_shaders.sh (fxc golden path, ps_3_0, entry
// ps_main -> theme_prime_cube.ps.d3dbc).

struct PSIn
{
    float2 uv  : TEXCOORD0;
    float4 tp  : TEXCOORD1; // { time, p0, p1, aspect }
    float4 pxr : TEXCOORD2; // { px_in_rect, rect_w, rect_h }
    float4 col : COLOR0;
};

static const float WRAP = 0.0017453293; // 2*pi/3600
static const float MAX_STEPS = 150.0;
static const float STEP_SIZE = 0.012;
static const float SAMPLE_ALPHA = 0.1; // 0.08 rescaled for the step size
static const float GRID_SIZE = 0.05;
static const float WALL_THICKNESS = 0.01;
static const float BOX_SIZE = 0.4999;

float3 glsl_mod3(float3 x, float y)
{
    return x - y * floor(x / y);
}

// Slab test: returns 1 on hit and writes the entry/exit distances.
float intersect_box(float3 ro, float3 rd, float b, out float t0, out float t1)
{
    float3 inv_dir = 1.0 / rd;
    float3 tmin = (-b - ro) * inv_dir;
    float3 tmax = (b - ro) * inv_dir;
    float3 t1v = min(tmin, tmax);
    float3 t2v = max(tmin, tmax);
    t0 = max(max(t1v.x, t1v.y), t1v.z);
    t1 = min(min(t2v.x, t2v.y), t2v.z);
    return t1 >= max(t0, 0.0) ? 1.0 : 0.0;
}

// Exact primality for the reachable coordinate range: |floor(p/G)| is
// at most 10 inside the box, so the primes are just {2, 3, 5, 7}.
float is_prime(float n)
{
    return (n == 2.0 || n == 3.0 || n == 5.0 || n == 7.0) ? 1.0 : 0.0;
}

// 1 where p sits within WALL_THICKNESS of a lattice plane.
float is_wall(float3 p)
{
    float3 local = glsl_mod3(p + 0.5 * GRID_SIZE, GRID_SIZE) - 0.5 * GRID_SIZE;
    float3 d = abs(local);
    return step(min(d.x, min(d.y, d.z)), WALL_THICKNESS);
}

// Pale composite -> blue-violet (any prime coord) -> green (all prime).
float3 prime_color(float3 p)
{
    float3 c = abs(floor(p / GRID_SIZE));
    float px = is_prime(c.x);
    float py = is_prime(c.y);
    float pz = is_prime(c.z);
    float any_p = max(px, max(py, pz));
    float all_p = px * py * pz;
    float3 col = lerp(float3(0.8, 0.8, 0.8), float3(0.2, 0.1, 0.7), any_p);
    return lerp(col, float3(0.0, 4.0, 0.0), all_p);
}

// GLSL `v.xy *= mat2(c,-s,s,c)` (row vector times matrix).
float2 rot2(float2 v, float c, float s)
{
    return float2(c * v.x - s * v.y, s * v.x + c * v.y);
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
    // Shadertoy is y-up.
    float2 uv = float2(i.uv.x, 1.0 - i.uv.y) * 2.0 - 1.0;
    uv.x *= aspect;

    // Tumble (~0.5 rad/s, wrap-seamless).
    float ang = t * 286.0 * WRAP;
    float c = cos(ang);
    float s = sin(ang);

    float3 ro = float3(0.0, 0.0, 1.1);
    float3 rd = normalize(float3(uv, -1.0));
    rd.xz = rot2(rd.xz, c, s);
    ro.xz = rot2(ro.xz, c, s);
    rd.yx = rot2(rd.yx, c, s);
    ro.yx = rot2(ro.yx, c, s);

    float t_enter, t_exit;
    float hit = intersect_box(ro, rd, BOX_SIZE, t_enter, t_exit);
    // Miss ⇒ start at the exit so the march breaks immediately and the
    // flat backdrop survives (no early return / no branch around the
    // loop — flow stays shallow).
    float ray_t = lerp(t_exit, max(t_enter, 0.0), hit);

    float4 color = float4(0.1, 0.1, 0.1, 0.1);
    [loop]
    for (float k = 0.0; k < MAX_STEPS; k += 1.0)
    {
        if (ray_t >= t_exit || color.a >= 0.99)
            break;
        float3 pos = ro + ray_t * rd;
        // Predicated in place of the original's `continue`.
        float a = SAMPLE_ALPHA * is_wall(pos);
        color.rgb += (1.0 - color.a) * a * prime_color(pos);
        color.a += (1.0 - color.a) * a;
        ray_t += STEP_SIZE;
    }

    float3 col = color.rgb * 0.85; // sits behind the panel wash + menu text

    return float4(col, rounded_coverage(i.pxr) * i.col.a);
}
