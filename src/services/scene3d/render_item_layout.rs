//! Render-item layout — the PURE half of `render_item.rs` (design §5.2 as
//! corrected by the Step 3 RE, `docs/background_dancers_research.md` §2).
//!
//! Dependency-free on purpose (no `crate::` imports, no `unsafe`) so
//! `scripts/validate_background_dancers.sh` can `#[path]`-mount it and run
//! the `#[cfg(test)]` suite on a non-x86 host. Everything that touches the
//! engine lives in `render_item.rs`.
//!
//! ## Why literal engine offsets are acceptable HERE
//!
//! The render item is a MOD-OWNED object the engine only READS: the opaque /
//! transparent collectors, the bone-texture upload, the draw and the item push
//! read the fields below at these exact offsets on every supported build
//! (Step 1 shape diff, RE §1.9 — each consumer is byte-shape identical on
//! 20250805 / 20260224 / 20260721 / 20260825 / 20260915 with identical
//! `[reg+disp]` operand sets; the per-reader field table is RE §2.4). The
//! GPU-resource strides (`0x48` draw record, `0x168` material, `200` palette)
//! and the material texture-slot layout come from the same readers plus the
//! model converter (§2.1). They are kept as named consts so a future
//! derivation can replace them in one place.

// ── Item header (0xC8 bytes) ─────────────────────────────────────────

/// `f32[16]` world matrix, row-vector convention (translation in row 3).
pub const ITEM_WORLD: usize = 0x00;
/// `f32[4]` ModelParameters `{bone_count, 1.0, 0, 0}` → VS c22 / PS c2.
pub const ITEM_MODEL_PARAMS: usize = 0x40;
/// `ModelParameters.w` (`item+0x4C`): read by NO stock shader (`.x/.z` = the
/// bone texture height, `.y` = the stipple threshold), so the DLL uses it as
/// a per-item constant — the synthesized outline VS reads it as the hull's
/// rim width in 720p pixels (0 ⇒ the shader's default).
pub const ITEM_OUTLINE_PX: usize = 0x4C;
/// `f32[4]` tint, multiplied into every draw record's colour → VS c23.
pub const ITEM_TINT: usize = 0x50;
/// `gs::ModelData*` GPU model resource.
pub const ITEM_RES: usize = 0x60;
/// Allocation base of the trailing arrays (unused by the engine; A3's dtor
/// freed it — ours keeps the whole item in ONE block, see `trailing_layout`).
pub const ITEM_TRAILING: usize = 0x68;
/// → draw records, `REC_SIZE` each.
pub const ITEM_DRAW_RECORDS: usize = 0x70;
/// `u32[2]` bone-texture handles (frame parity); 0 for rigid items.
pub const ITEM_BONE_TEX: usize = 0x78;
/// → `f32[16] × bone_count` MODEL-space bone matrices.
pub const ITEM_BONES: usize = 0x80;
/// → scratch matrices (only with `MODE_SCRATCH`, which we never set).
pub const ITEM_SCRATCH: usize = 0x88;
/// → private material copies, `MATERIAL_SIZE` each.
pub const ITEM_MATERIALS: usize = 0x98;
/// → private palette copies, `PALETTE_SIZE` each.
pub const ITEM_PALETTES: usize = 0xA0;
/// `u32` mode bits (`MODE_*`); the engine reads only `MODE_SCRATCH`.
pub const ITEM_MODE: usize = 0xA8;
/// `u32` flags, bit0 = hidden (collector skips the item).
pub const ITEM_FLAGS: usize = 0xAC;
/// `u32` node pass mask (`&` the pass filter: 0x56 OPACITY / 0x46 TRANS /
/// 0x10 LOWPRIO_TRANS / 0x01 DISTANTVIEW).
pub const ITEM_PASS_MASK: usize = 0xB0;
/// `u32` frame stamp the bone-texture upload CASes (RE §2.3).
pub const ITEM_FRAME_STAMP: usize = 0xB4;
/// Header bytes (A3 allocated exactly 200).
pub const ITEM_HEADER_SIZE: usize = 0xC8;

/// `item+0xAC` bit0.
pub const ITEM_FLAG_HIDDEN: u32 = 1;

/// Seed for `ITEM_FRAME_STAMP`. MUST be non-zero: the upload treats 0 as
/// "claimed by another thread" and the pass driver spins until every item
/// reports done — a 0 seed hangs the render thread (RE §2.3). World's frame
/// counter skips 0 (`-1 → 1`), so this collides only on the wrap frame.
pub const FRAME_STAMP_SEED: u32 = 0xFFFF_FFFF;

// ── Draw record (0x30 bytes, one per GPU draw record) ────────────────

/// `f32[4]` per-record colour (× item tint at collect time).
pub const REC_COLOR: usize = 0x00;
/// → the GPU draw record inside the resource (`GPU_REC_SIZE` stride).
pub const REC_GPU_REC: usize = 0x10;
/// → this item's material copy for the record.
pub const REC_MATERIAL: usize = 0x18;
/// → this item's palette (vertex-stream block) copy for the record.
pub const REC_PALETTE: usize = 0x20;
/// `u32` flags: `gpuRec+0x10 & GPU_REC_FLAG_MASK` at build; bit 27 hides.
pub const REC_FLAGS: usize = 0x28;
/// `u32` per-record pass mask (`0xFFFFFFFF` = every pass).
pub const REC_PASS_MASK: usize = 0x2C;
pub const REC_SIZE: usize = 0x30;
/// `REC_FLAGS` bit the collector tests to skip a record.
pub const REC_HIDDEN_BIT: u32 = 0x0800_0000;
/// `REC_FLAGS` bit the model pass's material bind tests to select PROGRAM 0
/// of the material's shader container instead of the ordinary stage (2)
/// (`FUN_1801f63f0` / `FUN_1801f6100`, RE §4.6). The dancers mod's
/// inverted-hull twin items set it on every record so the synthesized
/// `mdl_*_lambert` containers' outline pair draws them. Nothing else in the
/// draw path reads it.
pub const REC_HULL_BIT: u32 = 0x8000_0000;
/// `REC_FLAGS` bits the draw-state emitter (`FUN_180261c80`) reads as the
/// blend group (`& 0xE0`, 0 = opaque); a non-zero group means the mesh is
/// alpha-blended (usually z-write off) — its hull would darken the mesh
/// itself, so hulls hide such records.
pub const REC_BLEND_GROUP_MASK: u32 = 0x0000_00E0;
/// The bits of the GPU record's flag word A3 copied into `REC_FLAGS`. The
/// stock top nibble is kept EXCEPT [`REC_HULL_BIT`]: with the outline pair
/// at program 0 a stock record carrying bit 31 would turn into a hull draw,
/// so body items clear it and only [`hull_record_flags`] sets it.
pub const GPU_REC_FLAG_MASK: u32 = 0x7000_00FF;

/// Flags of a HULL item's draw record, from the body record's flags: the
/// hull bit set; alpha-blended records (non-zero blend group) additionally
/// hidden (bit 27) — a z-write-off translucent mesh (hair cards, veils)
/// would be darkened by its own shell in either draw order.
pub fn hull_record_flags(body_flags: u32) -> u32 {
    let mut f = body_flags | REC_HULL_BIT;
    if body_flags & REC_BLEND_GROUP_MASK != 0 {
        f |= REC_HIDDEN_BIT;
    }
    f
}

// ── GPU resource pieces the builder reads ────────────────────────────

/// GPU draw record stride (`res+0x68` array).
pub const GPU_REC_SIZE: usize = 0x48;
/// GPU draw record: `u32` flags word (`0x100` = uses the bone texture).
pub const GPU_REC_FLAGS: usize = 0x10;
/// GPU draw record: pointer INTO the resource's palette array.
pub const GPU_REC_PALETTE_PTR: usize = 0x30;
/// GPU draw record: pointer INTO the resource's material array.
pub const GPU_REC_MATERIAL_PTR: usize = 0x38;
/// Material stride (`res+0x78` array).
pub const MATERIAL_SIZE: usize = 0x168;
/// Vertex-stream binding block ("palette") stride (`res+0x88` array).
pub const PALETTE_SIZE: usize = 200;
/// One `f32[16]`.
pub const MATRIX_SIZE: usize = 0x40;

// ── Material shader object (RE §4.7) ─────────────────────────────────

/// `gs::Shader*` the converter resolved for the material (`FUN_1802745b0`'s
/// result stored by `FUN_180274070`); the draw binds
/// `*(*(obj + SHADER_PROGRAMS) + stage*4)`. Re-pointing this field in an
/// item's PRIVATE material copy restyles that material for that item.
pub const MAT_SHADER_OBJ: usize = 0x20;
/// `gs::Shader`: `u32 name_hash @0` (the GSPW header hash), `u32 program
/// count @4`, `u32* program handles @8`.
pub const SHADER_NAME_HASH: usize = 0x00;
pub const SHADER_PROGRAM_COUNT: usize = 0x04;
pub const SHADER_PROGRAMS: usize = 0x08;

/// Per-material eligibility for the whole-scene restyle: a material copy is
/// re-pointed only when EVERY draw record using it is opaque / alpha-tested
/// (blend group 0 — additive glows and alpha-blended translucents keep their
/// authored stock look) AND the instance allows it (not the shadow, not the
/// skydome part). `record_material_idx` maps each record to its material
/// index; `record_flags` are the records' `REC_FLAGS`.
pub fn restyle_eligible_materials(
    material_count: usize,
    record_material_idx: &[usize],
    record_flags: &[u32],
    instance_allows: bool,
) -> Vec<bool> {
    let mut ok = vec![instance_allows; material_count];
    for (mi, flags) in record_material_idx.iter().zip(record_flags.iter()) {
        if *mi < material_count && flags & REC_BLEND_GROUP_MASK != 0 {
            ok[*mi] = false;
        }
    }
    ok
}

// ── Material texture slots (RE §2.1) ─────────────────────────────────

/// `u32` mask of the 8 texture slots a material uses.
pub const MAT_TEX_MASK: usize = 0x14;
/// Number of texture slots per material.
pub const MAT_TEX_SLOTS: usize = 8;
/// `u16` texture-table index of slot `s`.
pub const fn mat_tex_index(slot: usize) -> usize {
    slot * 2
}
/// `TextureData*` bound for slot `s` (what the draw reads).
pub const fn mat_tex_ptr(slot: usize) -> usize {
    0xA8 + slot * 0x18
}

// ── Resource texture table (RE §2.1) ─────────────────────────────────

/// `res+0x80` → table of `TEX_ENTRY_SIZE` entries.
pub const RES_TEX_TABLE: usize = 0x80;
/// `res+0x2C` u32 entry count.
pub const RES_TEX_COUNT: usize = 0x2C;
pub const TEX_ENTRY_SIZE: usize = 0x10;
/// `u32` gs hash of the texture name.
pub const TEX_ENTRY_HASH: usize = 0;
/// `TextureData*` resolved at conversion (default texture on miss).
pub const TEX_ENTRY_PTR: usize = 8;
/// `TextureData`: `u32` gs hash at +0, `u32` texture handle at +4, `u16`
/// width at +8, `u16` height at +0xA.
pub const TEXDATA_HASH: usize = 0;
pub const TEXDATA_W: usize = 0x8;
pub const TEXDATA_H: usize = 0xA;

/// Per material: whether any of its MASKED texture slots samples the
/// resource texture-table entry hashed `target_hash`.
/// `material_tex_indices[m]` = the table indices of material `m`'s masked
/// slots (`u16 mat + slot*2` under the mask at `mat + MAT_TEX_MASK`);
/// `table_hashes` = the table's `TEX_ENTRY_HASH`es. Out-of-range indices
/// are ignored. The whole-scene restyle keeps such materials stock — the
/// `offscreen1` stage screens (the movie render target) stay unlit.
pub fn materials_sampling(
    material_tex_indices: &[Vec<u16>],
    table_hashes: &[u32],
    target_hash: u32,
) -> Vec<bool> {
    material_tex_indices
        .iter()
        .map(|slots| {
            slots
                .iter()
                .any(|&i| table_hashes.get(i as usize) == Some(&target_hash))
        })
        .collect()
}

// ── Mode bits (A3 semantics; the engine reads only `MODE_SCRATCH`) ───

pub const MODE_PRIVATE_PALETTES: u32 = 0x1;
pub const MODE_PRIVATE_BONES: u32 = 0x2;
pub const MODE_BONE_TEXTURES: u32 = 0x4;
pub const MODE_PRIVATE_MATERIALS: u32 = 0x8;
/// Scratch-matrix mode — the ONLY bit the World engine reads (upload). Never set.
pub const MODE_SCRATCH: u32 = 0x10;
/// Rigid item: private palettes + bones + materials.
pub const MODE_RIGID: u32 = MODE_PRIVATE_PALETTES | MODE_PRIVATE_BONES | MODE_PRIVATE_MATERIALS;
/// Skinned item: rigid + the two bone textures.
pub const MODE_SKINNED: u32 = MODE_RIGID | MODE_BONE_TEXTURES;

// ── Bone textures (Step 4) ───────────────────────────────────────────

/// `A32B32G32R32F`.
pub const BONE_TEX_FORMAT: u32 = 0x74;
/// Dynamic + sysmem staging (the ArrowPalette factory's `0x2002` minus the
/// render-target bit).
pub const BONE_TEX_USAGE: u32 = 0x2001;
/// Bone texture WIDTH in texels: one bone per ROW, 3 float4 texels used per
/// row (`invBind·bone` as 3 rows of a 3×4), 4 allocated. The upload writes
/// bone `i` at `data + i·pitch` (RE §2.3/§2.8), so the texture is
/// `create(w = 4, h = bone_count)` — A3's `FUN_1801765d0` wrote registry
/// `+0xC = 4`, `+0xE = bones`, and World's `create(w, h, …)` stores `w` at
/// `+0xC`. The design's `(bone_count, 4)` was swapped: a 33×4 texture with a
/// 33-row upload overran the staging buffer by ~15 KB per frame (cabinet
/// 2026-09-16: magenta flashing, no dancer, heap-corruption crash at exit).
pub const BONE_TEX_WIDTH: u32 = 4;

/// Number of pass-mask bits meaning "stage part" (design §5.2).
pub const PASS_MASK_STAGE: u32 = 4;
/// Dancers, parts and the shadow quad.
pub const PASS_MASK_DANCER: u32 = 2;
/// `:N` low-priority stage parts.
pub const PASS_MASK_LOWPRIO: u32 = 0x10;

// ── Pure helpers ─────────────────────────────────────────────────────

/// The per-resource counts the builder sizes its block from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counts {
    pub draw_records: usize,
    pub bones: usize,
    pub materials: usize,
    pub palettes: usize,
}

/// Offsets (from the trailing base) of each array and the total size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trailing {
    pub records_off: usize,
    pub bones_off: usize,
    pub materials_off: usize,
    pub palettes_off: usize,
    pub total: usize,
}

const fn align16(v: usize) -> usize {
    (v + 15) & !15
}

/// Lay the trailing arrays out — draw records, bones, materials, palettes,
/// each 16-aligned — after the 0xC8 header. `total` excludes the header.
pub fn trailing_layout(c: &Counts) -> Trailing {
    let records_off = 0;
    let bones_off = align16(records_off + c.draw_records * REC_SIZE);
    let materials_off = align16(bones_off + c.bones * MATRIX_SIZE);
    let palettes_off = align16(materials_off + c.materials * MATERIAL_SIZE);
    let total = align16(palettes_off + c.palettes * PALETTE_SIZE);
    Trailing {
        records_off,
        bones_off,
        materials_off,
        palettes_off,
        total,
    }
}

/// Whole-block size: header + trailing.
pub fn block_size(c: &Counts) -> usize {
    ITEM_HEADER_SIZE + trailing_layout(c).total
}

/// Index of the array element a GPU draw record points at: the collector's
/// `(ptr − base) / stride`, refused when the pointer is not on an element
/// boundary or past `count`.
pub fn element_index(ptr: usize, base: usize, stride: usize, count: usize) -> Option<usize> {
    if stride == 0 || ptr < base {
        return None;
    }
    let delta = ptr - base;
    if delta % stride != 0 {
        return None;
    }
    let idx = delta / stride;
    if idx < count {
        Some(idx)
    } else {
        None
    }
}

/// Material index a GPU draw record's `GPU_REC_MATERIAL_PTR` names.
pub fn record_material_index(
    gpu_mat_ptr: usize,
    res_mats_base: usize,
    count: usize,
) -> Option<usize> {
    element_index(gpu_mat_ptr, res_mats_base, MATERIAL_SIZE, count)
}

/// Palette index a GPU draw record's `GPU_REC_PALETTE_PTR` names.
pub fn record_palette_index(
    gpu_pal_ptr: usize,
    res_pals_base: usize,
    count: usize,
) -> Option<usize> {
    element_index(gpu_pal_ptr, res_pals_base, PALETTE_SIZE, count)
}

/// `ModelParameters` for a resource with `bone_count` bones: `.z` is 0 for
/// BOTH rigid and skinned items (A3 set it to the bone count only in the
/// scratch mode — RE §2.4).
pub fn model_params(bone_count: u32) -> [f32; 4] {
    [bone_count as f32, 1.0, 0.0, 0.0]
}

/// Row-vector identity.
pub const IDENTITY: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
];

/// Row-vector translation matrix.
pub fn translation(x: f32, y: f32, z: f32) -> [f32; 16] {
    let mut m = IDENTITY;
    m[12] = x;
    m[13] = y;
    m[14] = z;
    m
}

/// Row-vector uniform-scale-then-translate matrix (`diag(s,s,s,1) · T`).
pub fn scale_translation(s: f32, x: f32, y: f32, z: f32) -> [f32; 16] {
    let mut m = translation(x, y, z);
    m[0] = s;
    m[5] = s;
    m[10] = s;
    m
}

/// What the item build learned about the resource's material textures
/// (RE §2.1). `still_default` > 0 after a re-resolve means the DDS is not
/// registered (yet) — the model renders with the engine's default texture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextureStats {
    /// Texture-table entries.
    pub total: usize,
    /// Entries whose pointer already named a `TextureData` carrying the
    /// entry's own hash (resolved when the model was converted).
    pub resolved_at_load: usize,
    /// Entries the DLL re-resolved through the gs registry at build.
    pub re_resolved: usize,
    /// Entries left on the default / an unresolvable pointer.
    pub still_default: usize,
}

/// Classify one texture-table entry: `entry_hash` vs the hash stored in the
/// `TextureData` it points at (`None` = null pointer / default sentinel).
pub fn entry_resolved(entry_hash: u32, pointee_hash: Option<u32>) -> bool {
    matches!(pointee_hash, Some(h) if h == entry_hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footpanel_layout() {
        let c = Counts {
            draw_records: 1,
            bones: 1,
            materials: 1,
            palettes: 1,
        };
        let t = trailing_layout(&c);
        assert_eq!(t.records_off, 0);
        assert_eq!(t.bones_off, 0x30);
        assert_eq!(t.materials_off, 0x70);
        // 0x70 + 0x168 = 0x1D8 → aligned 0x1E0
        assert_eq!(t.palettes_off, 0x1E0);
        // 0x1E0 + 200 = 0x2A8 → aligned 0x2B0
        assert_eq!(t.total, 0x2B0);
        assert_eq!(block_size(&c), ITEM_HEADER_SIZE + 0x2B0);
    }

    #[test]
    fn skinned_layout_is_monotonic_and_aligned() {
        let c = Counts {
            draw_records: 5,
            bones: 33,
            materials: 3,
            palettes: 5,
        };
        let t = trailing_layout(&c);
        assert!(t.records_off < t.bones_off);
        assert!(t.bones_off < t.materials_off);
        assert!(t.materials_off < t.palettes_off);
        assert!(t.palettes_off < t.total);
        for off in [t.bones_off, t.materials_off, t.palettes_off, t.total] {
            assert_eq!(off % 16, 0);
        }
        assert!(t.bones_off >= 5 * REC_SIZE);
        assert!(t.materials_off >= t.bones_off + 33 * MATRIX_SIZE);
        assert!(t.palettes_off >= t.materials_off + 3 * MATERIAL_SIZE);
        assert!(t.total >= t.palettes_off + 5 * PALETTE_SIZE);
    }

    #[test]
    fn bone_texture_orientation_is_one_row_per_bone() {
        // 3 texels written per bone row; width must hold them.
        assert!(BONE_TEX_WIDTH >= 3);
        assert_eq!(BONE_TEX_WIDTH, 4);
        // 16 bytes per A32B32G32R32F texel ⇒ a 64-byte pitch per bone row.
        assert_eq!(BONE_TEX_WIDTH as usize * 16, 64);
    }

    #[test]
    fn skinned_block_size_covers_bone_textures() {
        // The two u32 handles never run into the bone-array pointer.
        assert!(ITEM_BONE_TEX + 8 <= ITEM_BONES);
        // pl_emi00: 2 records, 33 bones, 2 materials, 2 palettes.
        let t = trailing_layout(&Counts {
            draw_records: 2,
            bones: 33,
            materials: 2,
            palettes: 2,
        });
        assert!(t.bones_off + 33 * MATRIX_SIZE <= t.materials_off);
        assert!(t.materials_off + 2 * MATERIAL_SIZE <= t.palettes_off);
        assert!(t.palettes_off + 2 * PALETTE_SIZE <= t.total);
    }

    #[test]
    fn element_index_rules() {
        let base = 0x1000;
        assert_eq!(record_material_index(base, base, 1), Some(0));
        assert_eq!(
            record_material_index(base + 2 * MATERIAL_SIZE, base, 3),
            Some(2)
        );
        // out of range
        assert_eq!(
            record_material_index(base + 3 * MATERIAL_SIZE, base, 3),
            None
        );
        // not on a boundary
        assert_eq!(record_material_index(base + 1, base, 3), None);
        // below base
        assert_eq!(record_material_index(base - MATERIAL_SIZE, base, 3), None);
        assert_eq!(record_palette_index(base + PALETTE_SIZE, base, 2), Some(1));
        assert_eq!(element_index(base, base, 0, 1), None);
    }

    #[test]
    fn model_params_z_is_zero() {
        assert_eq!(model_params(1), [1.0, 1.0, 0.0, 0.0]);
        assert_eq!(model_params(33), [33.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn mode_consts() {
        assert_eq!(MODE_RIGID, 0xB);
        assert_eq!(MODE_SKINNED, 0xF);
        assert_eq!(MODE_RIGID & MODE_SCRATCH, 0);
        assert_eq!(MODE_SKINNED & MODE_SCRATCH, 0);
        assert_ne!(FRAME_STAMP_SEED, 0);
    }

    #[test]
    fn header_offsets_match_design_table() {
        assert_eq!(ITEM_MODEL_PARAMS, 0x40);
        assert_eq!(ITEM_TINT, 0x50);
        assert_eq!(ITEM_RES, 0x60);
        assert_eq!(ITEM_DRAW_RECORDS, 0x70);
        assert_eq!(ITEM_BONE_TEX, 0x78);
        assert_eq!(ITEM_BONES, 0x80);
        assert_eq!(ITEM_MATERIALS, 0x98);
        assert_eq!(ITEM_PALETTES, 0xA0);
        assert_eq!(ITEM_MODE, 0xA8);
        assert_eq!(ITEM_FLAGS, 0xAC);
        assert_eq!(ITEM_PASS_MASK, 0xB0);
        assert_eq!(ITEM_FRAME_STAMP, 0xB4);
        assert_eq!(ITEM_HEADER_SIZE, 0xC8);
        assert_eq!(mat_tex_ptr(0), 0xA8);
        assert_eq!(mat_tex_ptr(7), 0xA8 + 7 * 0x18);
        assert_eq!(mat_tex_index(3), 6);
    }

    #[test]
    fn matrices() {
        let t = translation(1.0, 2.0, 3.0);
        assert_eq!(&t[12..15], &[1.0, 2.0, 3.0]);
        assert_eq!(t[15], 1.0);
        assert_eq!(t[0], 1.0);
        let s = scale_translation(0.9, 0.0, 0.0, 0.0);
        assert_eq!(s[0], 0.9);
        assert_eq!(s[5], 0.9);
        assert_eq!(s[10], 0.9);
        assert_eq!(s[15], 1.0);
    }

    #[test]
    fn texture_entry_classification() {
        assert!(entry_resolved(0x1234, Some(0x1234)));
        assert!(!entry_resolved(0x1234, Some(0x9999)));
        assert!(!entry_resolved(0x1234, None));
    }

    #[test]
    fn restyle_eligibility_follows_blend_groups_and_instance() {
        // 3 materials; records: m0 opaque, m1 opaque + additive, m2 opaque.
        let idx = [0usize, 1, 1, 2];
        let flags = [0x01u32, 0x00, 0x40, 0x00];
        assert_eq!(
            restyle_eligible_materials(3, &idx, &flags, true),
            vec![true, false, true]
        );
        // Instance veto (shadow / skydome) wins for every material.
        assert_eq!(
            restyle_eligible_materials(3, &idx, &flags, false),
            vec![false, false, false]
        );
        // Out-of-range material indices are ignored.
        assert_eq!(
            restyle_eligible_materials(1, &[5], &[0x20], true),
            vec![true]
        );
    }

    #[test]
    fn materials_sampling_matches_masked_slots_only() {
        const SCREEN: u32 = 0x5C4E_E0A1;
        let table = [0x1111u32, SCREEN, 0x3333];
        // Single slot on the screen entry; single slot elsewhere.
        assert_eq!(
            materials_sampling(&[vec![1], vec![0]], &table, SCREEN),
            vec![true, false]
        );
        // Multi-slot: any slot on the screen entry counts.
        assert_eq!(
            materials_sampling(&[vec![0, 2], vec![2, 1]], &table, SCREEN),
            vec![false, true]
        );
        // No masked slot ⇒ not a screen; out-of-range indices are ignored.
        assert_eq!(
            materials_sampling(&[vec![], vec![7, 300]], &table, SCREEN),
            vec![false, false]
        );
        // A hash that appears twice in the table (two names folding alike):
        // either index matches.
        let dup = [SCREEN, 0x2222, SCREEN];
        assert_eq!(
            materials_sampling(&[vec![2], vec![1], vec![0]], &dup, SCREEN),
            vec![true, false, true]
        );
        // Several materials over an empty table.
        assert_eq!(
            materials_sampling(&[vec![0], vec![1]], &[], SCREEN),
            vec![false, false]
        );
        assert!(materials_sampling(&[], &table, SCREEN).is_empty());
        assert_eq!(TEXDATA_W, 8);
        assert_eq!(TEXDATA_H, 0xA);
    }

    #[test]
    fn hull_record_flags_select_program_zero_and_hide_blended() {
        // The bit-31 program selector is never inherited from a stock record.
        assert_eq!(GPU_REC_FLAG_MASK & REC_HULL_BIT, 0);
        assert_eq!(GPU_REC_FLAG_MASK & REC_HIDDEN_BIT, 0, "bit 27 is ours too");
        assert_eq!(GPU_REC_FLAG_MASK & 0xFF, 0xFF, "state bits 0..7 kept");
        // Opaque body record → hull bit only.
        let opaque = 0x0000_0001; // two-sided, blend group 0
        assert_eq!(hull_record_flags(opaque), opaque | REC_HULL_BIT);
        assert_eq!(hull_record_flags(opaque) & REC_HIDDEN_BIT, 0);
        // Alpha-blended body record (group 0x20) → hull bit + hidden.
        let blended = 0x0000_0021;
        let h = hull_record_flags(blended);
        assert_eq!(h & REC_HULL_BIT, REC_HULL_BIT);
        assert_eq!(h & REC_HIDDEN_BIT, REC_HIDDEN_BIT);
        // The state/blend bits themselves pass through untouched.
        assert_eq!(h & 0xFF, blended);
        // Additive (0x40) / subtractive (0x60) groups hide too.
        assert_ne!(hull_record_flags(0x40) & REC_HIDDEN_BIT, 0);
        assert_ne!(hull_record_flags(0x60) & REC_HIDDEN_BIT, 0);
    }
}
