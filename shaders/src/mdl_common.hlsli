// mdl_common.hlsli — shared register map + helpers for the synthesized MODEL
// containers (mdl_lambert.hlsl: the lit style; mdl_cel.hlsl: the cel style
// + the inverted-hull outline pair). Included, never compiled on its own.
//
// Register convention (fxc /dumpbin of the World 20260915 stock model blobs,
// docs/background_dancers_research.md §4.3):
//
//   c14..c17  World                (bound per draw — command 0xE, 4 regs)
//   c18..c21  WorldViewProjection  (command 0x12; row-vector convention:
//                                   clip = x·c18 + y·c19 + z·c20 + c21)
//   c22       ModelParameters      { bone_count, 1.0, 0, 0 } — .x + .z is
//                                   the bone texture's height
//   c23       ModelUnitParameters.m_color — the per-draw tint
//   c24       parameters.m_vTexAnime { scaleU, scaleV, offU, offV } —
//                                   uv_out = uv / xy + zw
//   s3        BoneMatrices (skinned only): a 4×bone_count texture, one bone
//             per ROW: u = 0.125 / 0.375 / 0.625 select matrix rows 0/1/2,
//             v = (bone_index + 0.5) / (c22.x + c22.z), LOD 0; output
//             component k = dot(row_k, (pos, 1)) (3×4 `invBind·bone`, MODEL
//             space). BLENDINDICES arrive as raw floats 0..255 (UBYTE4),
//             BLENDWEIGHT.xyz weight indices 1..3, w0 = 1 − Σ.
//
// The pass binds NO camera / View / Projection constant. `view_frame`
// recovers what the ink / facing / outline terms need from World + WVP
// (RE §4.6): for an affine World and a rigid View,
// (WVP)3×3 = World3×3 · V3×3 · diag(P00, P11, P22), so with a uniform-scale
// World (true for every item of this scene) |column j of WVP3×3| / |World
// row 0| = P_jj; a direction n·WVP has w = n_view.z; and the view-space
// position is ∝ (clip.x/P00, clip.y/P11, clip.w). Handedness only flips the
// z sign of BOTH vectors, so their dot product is invariant.

#ifndef MDL_COMMON_HLSLI
#define MDL_COMMON_HLSLI

float4 World0 : register(c14);
float4 World1 : register(c15);
float4 World2 : register(c16);
float4 World3 : register(c17);
float4 WVP0   : register(c18);
float4 WVP1   : register(c19);
float4 WVP2   : register(c20);
float4 WVP3   : register(c21);
float4 ModelParameters : register(c22); // { bone_count, 1, 0, 0 }
float4 Tint            : register(c23); // ModelUnitParameters.m_color
float4 TexAnime        : register(c24); // parameters.m_vTexAnime
float4 ConstColor      : register(c25); // parameters.vConstatntColor (the _c / _notex variants)
float4 OffsetColor     : register(c26); // parameters.vOffsetColor
// s_ConstantParameters (stock mdl_* VS: fog = w·c49.x − c49.y; unused by every
// PS, replicated for fidelity in the UV3 variants).
float4 ConstParams0    : register(c48);
float4 ConstParams1    : register(c49);

sampler2D BoneMatrices : register(s3);

// Model-space position (w = 1) → clip space, stock mad chain.
float4 to_clip(float3 p)
{
    return p.x * WVP0 + p.y * WVP1 + p.z * WVP2 + WVP3;
}

// Model-space DIRECTION (w = 0) → clip space (no c21 term).
float4 to_clip_dir(float3 d)
{
    return d.x * WVP0 + d.y * WVP1 + d.z * WVP2;
}

// Model-space normal → world space (World's upper 3×3, row-vector).
float3 to_world_normal(float3 n)
{
    return n.x * World0.xyz + n.y * World1.xyz + n.z * World2.xyz;
}

float2 anim_uv(float2 uv)
{
    return uv / TexAnime.xy + TexAnime.zw;
}

// ── Palette skinning (stock addressing) ─────────────────────────────────
struct SkinIn
{
    float4 bi; // BLENDINDICES as floats
    float3 bw; // BLENDWEIGHT (indices 1..3)
};

// Fetch the three float4 rows of one bone (t = its normalized v coordinate).
void fetch_bone(float t, out float4 r0, out float4 r1, out float4 r2)
{
    r0 = tex2Dlod(BoneMatrices, float4(0.125, t, 0.0, 0.0));
    r1 = tex2Dlod(BoneMatrices, float4(0.375, t, 0.0, 0.0));
    r2 = tex2Dlod(BoneMatrices, float4(0.625, t, 0.0, 0.0));
}

// Blend the 4 bones' 3×4 rows into R0..R2 (stock v = (idx + 0.5)/(count + c22.z)).
void skin_rows(SkinIn s, out float4 R0, out float4 R1, out float4 R2)
{
    float inv = 1.0 / (ModelParameters.x + ModelParameters.z);
    float4 t = s.bi * inv + 0.5 * inv;
    float4 w = float4(1.0 - s.bw.x - s.bw.y - s.bw.z, s.bw);

    float4 a0, a1, a2, b0, b1, b2, c0, c1, c2, d0, d1, d2;
    fetch_bone(t.x, a0, a1, a2);
    fetch_bone(t.y, b0, b1, b2);
    fetch_bone(t.z, c0, c1, c2);
    fetch_bone(t.w, d0, d1, d2);

    R0 = a0 * w.x + b0 * w.y + c0 * w.z + d0 * w.w;
    R1 = a1 * w.x + b1 * w.y + c1 * w.z + d1 * w.w;
    R2 = a2 * w.x + b2 * w.y + c2 * w.z + d2 * w.w;
}

// Apply blended rows to a position (w = 1) and a normal (3×3 only).
void skin_apply(float4 R0, float4 R1, float4 R2, float3 pos, float3 nrm,
                out float3 ps, out float3 ns)
{
    float4 p1 = float4(pos, 1.0);
    ps = float3(dot(R0, p1), dot(R1, p1), dot(R2, p1));
    ns = float3(dot(R0.xyz, nrm), dot(R1.xyz, nrm), dot(R2.xyz, nrm));
}

// ── View-frame recovery (RE §4.6) ────────────────────────────────────────
struct ViewFrame
{
    float3 pos_view; // camera → vertex, "looks down +z" frame (unnormalized)
    float3 n_view;   // the normal in the same frame (unnormalized)
    float  p00;      // projection x scale (1/tan(fovX/2))
    float  p11;      // projection y scale (1/tan(fovY/2))
    float  p22;      // projection z slope (Q = f/(f−n)): Δclip.z per metre of view depth
};

// `clip` = the vertex's clip position, `nclip` = the (post-skin, MODEL-space)
// normal pushed through to_clip_dir.
ViewFrame view_frame(float4 clip, float4 nclip)
{
    ViewFrame f;
    float s = length(World0.xyz); // uniform World scale
    f.p00 = length(float3(WVP0.x, WVP1.x, WVP2.x)) / s;
    f.p11 = length(float3(WVP0.y, WVP1.y, WVP2.y)) / s;
    f.p22 = length(float3(WVP0.z, WVP1.z, WVP2.z)) / s;
    f.pos_view = float3(clip.x / f.p00, clip.y / f.p11, clip.w);
    f.n_view   = float3(nclip.x / f.p00, nclip.y / f.p11, nclip.w);
    return f;
}

#endif // MDL_COMMON_HLSLI
