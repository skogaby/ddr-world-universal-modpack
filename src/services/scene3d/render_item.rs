//! Render-item builder (design §4.2.4 / §5.2; RE `docs/background_dancers_
//! research.md` §2) — the object the engine's `MODEL:*` passes draw.
//!
//! One `memory::alloc_zeroed` block per item: the 0xC8 header followed by
//! the trailing arrays (`render_item_layout::trailing_layout`). The engine
//! only READS an item (RE §2.4), so every field is the DLL's: the world,
//! tint, bones and flags are written by the node's `visit` on the job
//! thread, everything else here at build on the game thread.
//!
//! Material textures: World's converter resolves them at conversion time
//! (`mat+0xA8+slot*0x18 = TextureData*`, default on miss) and NOTHING in
//! World re-resolves — so a `.dds` that registered after its `.model`
//! leaves the material on the default texture. The builder therefore
//! re-resolves into its OWN material copies through the gs registry when the
//! optional `texture_lookup` trio is available (RE §2.1); the resource is
//! never written.
//!
//! Palette / material copies are plain `memcpy`s — read-only for the engine,
//! no addref, no release; the resource owns every handle and an item's
//! lifetime is strictly inside its arc's (RE §2.2).
//!
//! Thread contract: `build` is GAME THREAD ONLY (registry lookups + texture
//! creates); `free` is called from the node dtor on a job-graph worker and
//! makes exactly one kind of engine call (`texture::release`).

use crate::core::memory;

use super::model_registry::ResourceView;
use super::render_item_layout::{self as layout, Counts, TextureStats};
use super::{sites, texture};

/// Why a build was refused. Every variant is fail-open: nothing engine-side
/// was left behind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildError {
    ServiceUnavailable,
    /// A resource header field the builder needs is unreadable / implausible.
    BadResource(&'static str),
    /// The block allocation failed.
    Alloc,
    /// A GPU draw record points outside the resource's material/palette arrays.
    RecordIndex {
        record: usize,
        which: &'static str,
    },
    /// A bone texture create failed (skinned items).
    BoneTexture,
}

/// A built item. The engine holds raw pointers to `ptr` from the moment the
/// node is attached until the node dtor runs `free` — never drop it early.
pub struct RenderItem {
    ptr: *mut u8,
    bone_tex: [u32; 2],
    counts: Counts,
    skinned: bool,
    pub textures: TextureStats,
}

// Fixed heap block, valid until `free`.
unsafe impl Send for RenderItem {}

impl RenderItem {
    pub fn ptr(&self) -> *mut u8 {
        self.ptr
    }
    pub fn counts(&self) -> &Counts {
        &self.counts
    }
    pub fn is_skinned(&self) -> bool {
        self.skinned
    }
    pub fn bone_textures(&self) -> [u32; 2] {
        self.bone_tex
    }
    /// The mode word the builder wrote (`MODE_RIGID` / `MODE_SKINNED`).
    pub fn mode(&self) -> u32 {
        // SAFETY: our own block.
        unsafe { memory::read_u32(self.ptr.add(layout::ITEM_MODE)) }
    }

    /// Hand the block to a node (`node+0x78`); `free` must later be called
    /// with the pieces via [`free_raw`].
    pub fn into_raw(self) -> (*mut u8, [u32; 2]) {
        (self.ptr, self.bone_tex)
    }

    // ── visit-side accessors: NO engine calls, NO allocation ─────────

    /// # Safety
    /// `self.ptr` is live (the node still owns it).
    pub unsafe fn set_world(&self, m: &[f32; 16]) {
        set_world_raw(self.ptr, m);
    }
    /// # Safety
    /// As [`set_world`](Self::set_world).
    pub unsafe fn set_tint(&self, rgba: [f32; 4]) {
        set_tint_raw(self.ptr, rgba);
    }
    /// Copy up to `counts.bones` matrices into the bone array.
    /// # Safety
    /// As [`set_world`](Self::set_world).
    pub unsafe fn set_bones(&self, bones: &[[f32; 16]]) {
        set_bones_raw(self.ptr, bones, self.counts.bones);
    }
    /// # Safety
    /// As [`set_world`](Self::set_world).
    pub unsafe fn set_hidden(&self, hidden: bool) {
        set_hidden_raw(self.ptr, hidden);
    }
    /// # Safety
    /// As [`set_world`](Self::set_world).
    pub unsafe fn set_pass_mask(&self, mask: u32) {
        set_pass_mask_raw(self.ptr, mask);
    }
    /// Hide / show one draw record (bit 27 of its flags).
    /// # Safety
    /// As [`set_world`](Self::set_world).
    pub unsafe fn set_record_hidden(&self, i: usize, hidden: bool) {
        if i >= self.counts.draw_records {
            return;
        }
        let recs = memory::read_ptr(self.ptr.add(layout::ITEM_DRAW_RECORDS)) as *mut u8;
        if recs.is_null() {
            return;
        }
        let f = recs.add(i * layout::REC_SIZE + layout::REC_FLAGS);
        let v = memory::read_u32(f);
        let v = if hidden {
            v | layout::REC_HIDDEN_BIT
        } else {
            v & !layout::REC_HIDDEN_BIT
        };
        memory::write_u32(f, v);
    }

    /// Per-item outline rim width (720p px) for a HULL twin — `ModelParameters.w`,
    /// a constant no stock shader reads (`layout::ITEM_OUTLINE_PX`).
    /// # Safety
    /// As [`set_world`](Self::set_world).
    pub unsafe fn set_outline_width(&self, px: f32) {
        memory::write_f32(self.ptr.add(layout::ITEM_OUTLINE_PX), px);
    }

    /// Write `rgba` into EVERY draw record's own colour (`REC_COLOR`, 1.0
    /// white at build). The collector multiplies it into the item tint per
    /// frame and uploads the product as VS c23 — the outline PS emits that
    /// colour verbatim, so this is how a HULL twin gets its layer colour
    /// (the frame board republishes only the TINT, never the records, so the
    /// value written here holds for the item's life). Keep alpha at 1.0: the
    /// collector forces an entry with colour alpha < 1 into blend group 0x20.
    /// # Safety
    /// As [`set_world`](Self::set_world); call BEFORE the item is attached.
    pub unsafe fn set_record_colors(&self, rgba: [f32; 4]) {
        let recs = memory::read_ptr(self.ptr.add(layout::ITEM_DRAW_RECORDS)) as *mut u8;
        if recs.is_null() {
            return;
        }
        for i in 0..self.counts.draw_records {
            std::ptr::copy_nonoverlapping(
                rgba.as_ptr() as *const u8,
                recs.add(i * layout::REC_SIZE + layout::REC_COLOR),
                16,
            );
        }
    }

    /// The per-record material index (into this item's private copies) and
    /// `REC_FLAGS`, for the restyle eligibility rule.
    /// # Safety
    /// As [`set_world`](Self::set_world).
    unsafe fn record_material_map(&self) -> (Vec<usize>, Vec<u32>) {
        let recs = memory::read_ptr(self.ptr.add(layout::ITEM_DRAW_RECORDS)) as *mut u8;
        let mats = memory::read_ptr(self.ptr.add(layout::ITEM_MATERIALS)) as usize;
        let mut idx = Vec::with_capacity(self.counts.draw_records);
        let mut flags = Vec::with_capacity(self.counts.draw_records);
        if recs.is_null() || mats == 0 {
            return (idx, flags);
        }
        for i in 0..self.counts.draw_records {
            let rec = recs.add(i * layout::REC_SIZE);
            let mat_ptr = memory::read_ptr(rec.add(layout::REC_MATERIAL)) as usize;
            idx.push(
                layout::record_material_index(mat_ptr, mats, self.counts.materials)
                    .unwrap_or(usize::MAX),
            );
            flags.push(memory::read_u32(rec.add(layout::REC_FLAGS)));
        }
        (idx, flags)
    }

    /// Per material copy: whether it samples the resource texture-table
    /// entry hashed `texture_hash` through any masked slot
    /// (`layout::materials_sampling`).
    /// # Safety
    /// As [`set_world`](Self::set_world).
    pub unsafe fn materials_sampling(&self, texture_hash: u32) -> Vec<bool> {
        let mats = memory::read_ptr(self.ptr.add(layout::ITEM_MATERIALS));
        let res = memory::read_ptr(self.ptr.add(layout::ITEM_RES));
        if mats.is_null() {
            return vec![false; self.counts.materials];
        }
        layout::materials_sampling(
            &material_slot_indices(mats, self.counts.materials),
            &tex_table_hashes(res),
            texture_hash,
        )
    }

    /// The `(width, height, gs hash)` of the `TextureData` the first
    /// material sampling `texture_hash` binds (the pointer the draw reads,
    /// `mat + mat_tex_ptr(slot)`), or `None` when no material samples it or
    /// the bound texture is unreadable. Diagnostic: 1280 × 1280 (or the
    /// Custom Resolution square) = the stage-screen render target, not a
    /// shipped placeholder DDS.
    /// # Safety
    /// As [`set_world`](Self::set_world).
    pub unsafe fn sampled_texture_info(&self, texture_hash: u32) -> Option<(u16, u16, u32)> {
        let mats = memory::read_ptr(self.ptr.add(layout::ITEM_MATERIALS));
        let res = memory::read_ptr(self.ptr.add(layout::ITEM_RES));
        if mats.is_null() {
            return None;
        }
        let hashes = tex_table_hashes(res);
        for m in 0..self.counts.materials {
            let mat = mats.add(m * layout::MATERIAL_SIZE);
            let mask = memory::read_u32(mat.add(layout::MAT_TEX_MASK));
            for slot in 0..layout::MAT_TEX_SLOTS {
                if mask & (1u32 << slot) == 0 {
                    continue;
                }
                let idx =
                    (mat.add(layout::mat_tex_index(slot)) as *const u16).read_unaligned() as usize;
                if hashes.get(idx) != Some(&texture_hash) {
                    continue;
                }
                let td = memory::read_ptr(mat.add(layout::mat_tex_ptr(slot)));
                if !memory::is_readable(td, layout::TEXDATA_H + 2) {
                    return None;
                }
                return Some((
                    (td.add(layout::TEXDATA_W) as *const u16).read_unaligned(),
                    (td.add(layout::TEXDATA_H) as *const u16).read_unaligned(),
                    memory::read_u32(td.add(layout::TEXDATA_HASH)),
                ));
            }
        }
        None
    }

    /// Whole-scene restyle (RE §4.7): re-point eligible PRIVATE material
    /// copies at style-variant shader objects. `variant_for(stock_hash)`
    /// returns the replacement `gs::Shader*` for a material whose current
    /// object hashes to `stock_hash` (`None` = leave stock). Eligibility =
    /// `layout::restyle_eligible_materials` (every record using the material
    /// opaque/alpha-tested, and the instance allows it) and the material does
    /// not sample `keep_stock_texture` (`layout::materials_sampling` — the
    /// `offscreen1` stage screens stay unlit in every style). Returns the
    /// counts; the per-record `restyled` mask is what a HULL twin needs to
    /// hide the records whose material stayed stock (their program 0 would
    /// be the body again) — so a kept screen gets no outline either.
    /// # Safety
    /// As [`set_world`](Self::set_world); call BEFORE the item is attached.
    pub unsafe fn restyle_materials(
        &self,
        instance_allows: bool,
        keep_stock_texture: u32,
        variant_for: &dyn Fn(u32) -> Option<*mut u8>,
    ) -> RestyleStats {
        let mut st = RestyleStats::default();
        let mats = memory::read_ptr(self.ptr.add(layout::ITEM_MATERIALS)) as *mut u8;
        if mats.is_null() {
            return st;
        }
        let (rec_mat, rec_flags) = self.record_material_map();
        let eligible = layout::restyle_eligible_materials(
            self.counts.materials,
            &rec_mat,
            &rec_flags,
            instance_allows,
        );
        let screens = self.materials_sampling(keep_stock_texture);
        let mut restyled_mat = vec![false; self.counts.materials];
        for (mi, ok) in eligible.iter().enumerate() {
            let mat = mats.add(mi * layout::MATERIAL_SIZE);
            let obj_slot = mat.add(layout::MAT_SHADER_OBJ);
            let obj = memory::read_ptr(obj_slot);
            if screens.get(mi).copied().unwrap_or(false) {
                st.kept_screen += 1;
                continue;
            }
            if !*ok {
                st.kept_blend += 1;
                continue;
            }
            if obj.is_null() || !memory::is_readable(obj, layout::SHADER_PROGRAMS + 8) {
                st.kept_no_variant += 1;
                continue;
            }
            let hash = memory::read_u32(obj.add(layout::SHADER_NAME_HASH));
            match variant_for(hash) {
                Some(v) if !v.is_null() => {
                    memory::write_ptr(obj_slot, v);
                    restyled_mat[mi] = true;
                    st.restyled += 1;
                }
                _ => st.kept_no_variant += 1,
            }
        }
        st.record_restyled = rec_mat
            .iter()
            .map(|&mi| mi < restyled_mat.len() && restyled_mat[mi])
            .collect();
        st
    }

    /// Turn this item into an inverted-hull twin: every draw record gets
    /// [`layout::REC_HULL_BIT`] (the model pass then binds PROGRAM 0 of the
    /// material's container — the synthesized outline pair); alpha-blended
    /// records (`layout::hull_record_flags`) and records whose material was
    /// NOT restyled (`record_restyled` from [`restyle_materials`]) are hidden.
    /// Returns `(records marked, records hidden)`.
    /// # Safety
    /// As [`set_world`](Self::set_world); call BEFORE the item is attached.
    pub unsafe fn mark_hull_records(&self, record_restyled: &[bool]) -> (usize, usize) {
        let recs = memory::read_ptr(self.ptr.add(layout::ITEM_DRAW_RECORDS)) as *mut u8;
        if recs.is_null() {
            return (0, 0);
        }
        let mut hidden = 0usize;
        for i in 0..self.counts.draw_records {
            let f = recs.add(i * layout::REC_SIZE + layout::REC_FLAGS);
            let mut v = layout::hull_record_flags(memory::read_u32(f));
            // A record whose material stayed STOCK has no outline pair at
            // program 0 — its hull draw would be the body drawn again.
            if !record_restyled.get(i).copied().unwrap_or(false) {
                v |= layout::REC_HIDDEN_BIT;
            }
            if v & layout::REC_HIDDEN_BIT != 0 {
                hidden += 1;
            }
            memory::write_u32(f, v);
        }
        (self.counts.draw_records, hidden)
    }
}

// ── Raw accessors shared with the node's `visit` (which holds only the
//    raw item pointer). All panic-free, allocation-free, lock-free. ─────

/// Header offset of the bone-array pointer (for `visit(2)`'s direct copy).
pub const BONES_PTR_OFF: usize = layout::ITEM_BONES;
/// Header offset of the tint.
pub const TINT_OFF: usize = layout::ITEM_TINT;

/// The item's own bone count as `build` recorded it in `ModelParameters.x`
/// (an `f32`; 0 when unreadable/implausible).
/// # Safety
/// `item` is a live block from `build`.
pub unsafe fn item_bone_count(item: *mut u8) -> usize {
    let f = memory::read_f32(item.add(layout::ITEM_MODEL_PARAMS));
    if f.is_finite() && (1.0..=1024.0).contains(&f) {
        f as usize
    } else {
        0
    }
}

/// # Safety
/// `item` is a live block from `build`.
pub unsafe fn set_world_raw(item: *mut u8, m: &[f32; 16]) {
    std::ptr::copy_nonoverlapping(m.as_ptr() as *const u8, item.add(layout::ITEM_WORLD), 64);
}
/// # Safety
/// As [`set_world_raw`].
pub unsafe fn set_tint_raw(item: *mut u8, rgba: [f32; 4]) {
    std::ptr::copy_nonoverlapping(rgba.as_ptr() as *const u8, item.add(layout::ITEM_TINT), 16);
}
/// # Safety
/// As [`set_world_raw`]; `bone_count` is the item's own count.
pub unsafe fn set_bones_raw(item: *mut u8, bones: &[[f32; 16]], bone_count: usize) {
    let dst = memory::read_ptr(item.add(layout::ITEM_BONES)) as *mut u8;
    if dst.is_null() {
        return;
    }
    let n = bones.len().min(bone_count);
    std::ptr::copy_nonoverlapping(bones.as_ptr() as *const u8, dst, n * layout::MATRIX_SIZE);
}
/// # Safety
/// As [`set_world_raw`].
pub unsafe fn set_hidden_raw(item: *mut u8, hidden: bool) {
    let f = item.add(layout::ITEM_FLAGS);
    let v = memory::read_u32(f);
    let v = if hidden {
        v | layout::ITEM_FLAG_HIDDEN
    } else {
        v & !layout::ITEM_FLAG_HIDDEN
    };
    memory::write_u32(f, v);
}
/// # Safety
/// As [`set_world_raw`].
pub unsafe fn set_pass_mask_raw(item: *mut u8, mask: u32) {
    memory::write_u32(item.add(layout::ITEM_PASS_MASK), mask);
}

/// Build an item for `res` with the node pass mask `pass_mask` (2 dancers,
/// 4 stage, 0x10 `:N` parts). Rigid resources get `MODE_RIGID`; skinned ones
/// `MODE_SKINNED` + two bone textures. GAME THREAD ONLY.
pub fn build(res: &ResourceView, pass_mask: u32) -> Result<RenderItem, BuildError> {
    let s = sites().ok_or(BuildError::ServiceUnavailable)?;
    let res_ptr = res.ptr();
    let counts = Counts {
        draw_records: res.draw_record_count() as usize,
        bones: res.bone_count() as usize,
        materials: res.material_count() as usize,
        palettes: res.palette_count() as usize,
    };
    // Plausibility: a real model is small; anything huge is a torn header.
    if counts.draw_records == 0 || counts.draw_records > 4096 {
        return Err(BuildError::BadResource("draw record count"));
    }
    if counts.bones == 0 || counts.bones > 1024 {
        return Err(BuildError::BadResource("bone count"));
    }
    if counts.materials == 0 || counts.materials > 4096 {
        return Err(BuildError::BadResource("material count"));
    }
    if counts.palettes == 0 || counts.palettes > 4096 {
        return Err(BuildError::BadResource("palette count"));
    }
    let bind = res.bind();
    let gpu_recs = res.draw_records();
    let mats = res.materials();
    let pals = res.palettes();
    let readable = |p: *const u8, len: usize| !p.is_null() && memory::is_readable(p, len);
    if !readable(bind, counts.bones * layout::MATRIX_SIZE) {
        return Err(BuildError::BadResource("bind array"));
    }
    if !readable(gpu_recs, counts.draw_records * layout::GPU_REC_SIZE) {
        return Err(BuildError::BadResource("draw records"));
    }
    if !readable(mats, counts.materials * layout::MATERIAL_SIZE) {
        return Err(BuildError::BadResource("materials"));
    }
    if !readable(pals, counts.palettes * layout::PALETTE_SIZE) {
        return Err(BuildError::BadResource("palettes"));
    }
    let skinned = res.is_skinned();
    let trailing = layout::trailing_layout(&counts);
    let size = layout::ITEM_HEADER_SIZE + trailing.total;

    // SAFETY: every engine pointer above was probed for the span we copy;
    // the block is ours and zero-filled.
    unsafe {
        let item = memory::alloc_zeroed(size);
        if item.is_null() {
            return Err(BuildError::Alloc);
        }
        let base = item.add(layout::ITEM_HEADER_SIZE);
        let rec_base = base.add(trailing.records_off);
        let bones_base = base.add(trailing.bones_off);
        let mats_base = base.add(trailing.materials_off);
        let pals_base = base.add(trailing.palettes_off);

        // Header.
        set_world_raw(item, &layout::IDENTITY);
        let params = layout::model_params(counts.bones as u32);
        std::ptr::copy_nonoverlapping(
            params.as_ptr() as *const u8,
            item.add(layout::ITEM_MODEL_PARAMS),
            16,
        );
        set_tint_raw(item, [1.0, 1.0, 1.0, 1.0]);
        memory::write_ptr(item.add(layout::ITEM_RES), res_ptr);
        memory::write_ptr(item.add(layout::ITEM_TRAILING), base);
        memory::write_ptr(item.add(layout::ITEM_DRAW_RECORDS), rec_base);
        memory::write_ptr(item.add(layout::ITEM_BONES), bones_base);
        memory::write_ptr(item.add(layout::ITEM_SCRATCH), std::ptr::null());
        memory::write_ptr(item.add(layout::ITEM_MATERIALS), mats_base);
        memory::write_ptr(item.add(layout::ITEM_PALETTES), pals_base);
        memory::write_u32(
            item.add(layout::ITEM_MODE),
            if skinned {
                layout::MODE_SKINNED
            } else {
                layout::MODE_RIGID
            },
        );
        memory::write_u32(item.add(layout::ITEM_FLAGS), 0);
        memory::write_u32(item.add(layout::ITEM_PASS_MASK), pass_mask);
        memory::write_u32(item.add(layout::ITEM_FRAME_STAMP), layout::FRAME_STAMP_SEED);

        // Trailing arrays: bones = bind, materials/palettes = verbatim copies.
        std::ptr::copy_nonoverlapping(bind, bones_base, counts.bones * layout::MATRIX_SIZE);
        std::ptr::copy_nonoverlapping(mats, mats_base, counts.materials * layout::MATERIAL_SIZE);
        std::ptr::copy_nonoverlapping(pals, pals_base, counts.palettes * layout::PALETTE_SIZE);

        // Draw records.
        for i in 0..counts.draw_records {
            let gpu_rec = gpu_recs.add(i * layout::GPU_REC_SIZE);
            let mat_ptr = memory::read_ptr(gpu_rec.add(layout::GPU_REC_MATERIAL_PTR)) as usize;
            let pal_ptr = memory::read_ptr(gpu_rec.add(layout::GPU_REC_PALETTE_PTR)) as usize;
            let Some(mi) = layout::record_material_index(mat_ptr, mats as usize, counts.materials)
            else {
                memory::free_alloc(item);
                return Err(BuildError::RecordIndex {
                    record: i,
                    which: "material",
                });
            };
            let Some(pi) = layout::record_palette_index(pal_ptr, pals as usize, counts.palettes)
            else {
                memory::free_alloc(item);
                return Err(BuildError::RecordIndex {
                    record: i,
                    which: "palette",
                });
            };
            let rec = rec_base.add(i * layout::REC_SIZE);
            std::ptr::copy_nonoverlapping(
                [1.0f32, 1.0, 1.0, 1.0].as_ptr() as *const u8,
                rec.add(layout::REC_COLOR),
                16,
            );
            memory::write_ptr(rec.add(layout::REC_GPU_REC), gpu_rec);
            memory::write_ptr(
                rec.add(layout::REC_MATERIAL),
                mats_base.add(mi * layout::MATERIAL_SIZE),
            );
            memory::write_ptr(
                rec.add(layout::REC_PALETTE),
                pals_base.add(pi * layout::PALETTE_SIZE),
            );
            let gpu_flags = memory::read_u32(gpu_rec.add(layout::GPU_REC_FLAGS));
            memory::write_u32(
                rec.add(layout::REC_FLAGS),
                gpu_flags & layout::GPU_REC_FLAG_MASK,
            );
            memory::write_u32(rec.add(layout::REC_PASS_MASK), 0xFFFF_FFFF);
        }

        // Material textures (RE §2.1): re-resolve into OUR copies.
        let textures = resolve_material_textures(
            res_ptr,
            mats_base,
            counts.materials,
            s.texture_lookup.is_some(),
        );

        // Bone textures (skinned only).
        let mut bone_tex = [0u32; 2];
        if skinned {
            // ONE BONE PER ROW: width 4 texels (3 used), height = bones — the
            // upload strides `data + i·pitch` per bone (RE §2.8; the swapped
            // shape overran the staging buffer every frame).
            let make = || {
                texture::create_dynamic(
                    layout::BONE_TEX_WIDTH,
                    counts.bones as u32,
                    layout::BONE_TEX_FORMAT,
                    layout::BONE_TEX_USAGE,
                )
            };
            match (make(), make()) {
                (Some(a), Some(b)) if a != b => bone_tex = [a, b],
                (a, b) => {
                    if let Some(a) = a {
                        texture::release(a);
                    }
                    if let Some(b) = b {
                        texture::release(b);
                    }
                    memory::free_alloc(item);
                    return Err(BuildError::BoneTexture);
                }
            }
            memory::write_u32(item.add(layout::ITEM_BONE_TEX), bone_tex[0]);
            memory::write_u32(item.add(layout::ITEM_BONE_TEX + 4), bone_tex[1]);
        }

        Ok(RenderItem {
            ptr: item,
            bone_tex,
            counts,
            skinned,
            textures,
        })
    }
}

/// Outcome of [`RenderItem::restyle_materials`].
#[derive(Default, Clone, Debug)]
pub struct RestyleStats {
    pub restyled: usize,
    /// Materials kept stock because a record using them is blended (or the
    /// instance vetoed the restyle).
    pub kept_blend: usize,
    /// Materials kept stock because no variant object exists for their shader.
    pub kept_no_variant: usize,
    /// Materials kept stock because they sample the kept texture (the
    /// `offscreen1` stage screens) — counted before the blend rule.
    pub kept_screen: usize,
    /// Per draw record: its material was restyled.
    pub record_restyled: Vec<bool>,
}

/// Re-run the material-texture resolve on a BUILT item (game thread), for
/// the case the cabinet showed on 2026-09-16: a large `.dds` registers a
/// few hundred ms after its `.model` converted, so the build found nothing
/// to re-resolve and the material stayed on the default texture. Each
/// corrected slot is one aligned 8-byte pointer store into our own material
/// copy — the draw (render thread) reads either the old default or the new
/// `TextureData*`, both valid — so this is safe on an ATTACHED item. Returns
/// the fresh stats; call until `still_default == 0` or a deadline.
///
/// # Safety
/// `item` is a live block from [`build`] whose node dtor has not run.
pub unsafe fn retry_texture_resolve(item: *mut u8, material_count: usize) -> TextureStats {
    let res = memory::read_ptr(item.add(layout::ITEM_RES));
    let mats = memory::read_ptr(item.add(layout::ITEM_MATERIALS)) as *mut u8;
    if res.is_null() || mats.is_null() {
        return TextureStats::default();
    }
    resolve_material_textures(res, mats, material_count, texture::lookup_available())
}

/// How the resource's texture table resolves RIGHT NOW without touching any
/// item: `resolved_at_load` = entries the converter resolved, `re_resolved`
/// = entries the gs registry can supply now, `still_default` = entries the
/// registry does not have (yet). The lifecycle's residency gate: a model is
/// ready to build when `still_default == 0`. GAME THREAD ONLY.
pub fn texture_readiness(res: &ResourceView) -> TextureStats {
    let mut stats = TextureStats::default();
    let res = res.ptr();
    // SAFETY: `ResourceView::new` probed the header; the table is probed by
    // `tex_table`.
    unsafe {
        let Some((table, count)) = tex_table(res) else {
            return stats;
        };
        stats.total = count;
        let default_tex = texture::default_texture();
        for i in 0..count {
            let entry = table.add(i * layout::TEX_ENTRY_SIZE);
            let hash = memory::read_u32(entry.add(layout::TEX_ENTRY_HASH));
            let ptr = memory::read_ptr(entry.add(layout::TEX_ENTRY_PTR));
            if layout::entry_resolved(hash, pointee_hash(ptr, default_tex)) {
                stats.resolved_at_load += 1;
            } else if texture::lookup_gs_texture(hash)
                .is_some_and(|p| layout::entry_resolved(hash, pointee_hash(p, default_tex)))
            {
                stats.re_resolved += 1;
            } else {
                stats.still_default += 1;
            }
        }
    }
    stats
}

/// The gs hash stored in the `TextureData` at `p`, or `None` for null / the
/// default texture / unreadable memory.
///
/// # Safety
/// Any pointer is acceptable (probed).
unsafe fn pointee_hash(p: *const u8, default_tex: Option<*const u8>) -> Option<u32> {
    if p.is_null() || Some(p) == default_tex || !memory::is_readable(p, 8) {
        None
    } else {
        Some(memory::read_u32(p.add(layout::TEXDATA_HASH)))
    }
}

/// The resource's texture table and entry count, probed (`None` when the
/// header or the table is unreadable, or the count is implausible).
///
/// # Safety
/// Any pointer is acceptable (probed).
unsafe fn tex_table(res: *const u8) -> Option<(*const u8, usize)> {
    if !memory::is_readable(res, layout::RES_TEX_TABLE + 8) {
        return None;
    }
    let count = memory::read_u32(res.add(layout::RES_TEX_COUNT)) as usize;
    let table = memory::read_ptr(res.add(layout::RES_TEX_TABLE));
    if count == 0 || count > 256 || !memory::is_readable(table, count * layout::TEX_ENTRY_SIZE) {
        return None;
    }
    Some((table, count))
}

/// The texture table's entry hashes (empty when unreadable).
///
/// # Safety
/// Any pointer is acceptable (probed).
unsafe fn tex_table_hashes(res: *const u8) -> Vec<u32> {
    let Some((table, count)) = tex_table(res) else {
        return Vec::new();
    };
    (0..count)
        .map(|i| memory::read_u32(table.add(i * layout::TEX_ENTRY_SIZE + layout::TEX_ENTRY_HASH)))
        .collect()
}

/// Per material copy: the table indices of its MASKED texture slots.
///
/// # Safety
/// `mats` is our material block with `material_count` materials.
unsafe fn material_slot_indices(mats: *const u8, material_count: usize) -> Vec<Vec<u16>> {
    (0..material_count)
        .map(|m| {
            let mat = mats.add(m * layout::MATERIAL_SIZE);
            let mask = memory::read_u32(mat.add(layout::MAT_TEX_MASK));
            (0..layout::MAT_TEX_SLOTS)
                .filter(|slot| mask & (1u32 << slot) != 0)
                .map(|slot| (mat.add(layout::mat_tex_index(slot)) as *const u16).read_unaligned())
                .collect()
        })
        .collect()
}

/// Walk the resource's texture table, re-resolve the entries the converter
/// left on the default texture (or on nothing), and rewrite every masked
/// slot of every material COPY from the corrected table. Never writes into
/// the resource. Without the lookup trio the copies keep the converter's
/// pointers and only the stats are computed.
///
/// # Safety
/// `res` is a live GPU resource; `mat_copies` is our block with
/// `material_count` materials.
unsafe fn resolve_material_textures(
    res: *const u8,
    mat_copies: *mut u8,
    material_count: usize,
    lookup_available: bool,
) -> TextureStats {
    let mut stats = TextureStats::default();
    let Some((table, count)) = tex_table(res) else {
        return stats;
    };
    stats.total = count;
    let default_tex = texture::default_texture();
    // Corrected table: what each material slot should point at.
    let mut resolved: Vec<*const u8> = Vec::with_capacity(count);
    for i in 0..count {
        let entry = table.add(i * layout::TEX_ENTRY_SIZE);
        let hash = memory::read_u32(entry.add(layout::TEX_ENTRY_HASH));
        let ptr = memory::read_ptr(entry.add(layout::TEX_ENTRY_PTR));
        if layout::entry_resolved(hash, pointee_hash(ptr, default_tex)) {
            stats.resolved_at_load += 1;
            resolved.push(ptr);
            continue;
        }
        let re = if lookup_available {
            texture::lookup_gs_texture(hash)
        } else {
            None
        };
        match re {
            Some(p) if layout::entry_resolved(hash, pointee_hash(p, default_tex)) => {
                stats.re_resolved += 1;
                resolved.push(p);
            }
            _ => {
                stats.still_default += 1;
                resolved.push(ptr);
            }
        }
    }
    if stats.re_resolved == 0 {
        // Nothing changed: the copies already hold the converter's pointers.
        return stats;
    }
    for m in 0..material_count {
        let mat = mat_copies.add(m * layout::MATERIAL_SIZE);
        let mask = memory::read_u32(mat.add(layout::MAT_TEX_MASK));
        for slot in 0..layout::MAT_TEX_SLOTS {
            if mask & (1u32 << slot) == 0 {
                continue;
            }
            let idx =
                (mat.add(layout::mat_tex_index(slot)) as *const u16).read_unaligned() as usize;
            if let Some(p) = resolved.get(idx) {
                memory::write_ptr(mat.add(layout::mat_tex_ptr(slot)) as *mut u8, *p);
            }
        }
    }
    stats
}

/// Release an item's engine resources and free its block. Called ONLY from
/// the node dtor (job-graph thread): `texture::release` is the single engine
/// call, everything else is our own memory.
///
/// # Safety
/// `(ptr, bone_tex)` came from [`RenderItem::into_raw`] and nothing else
/// references the block any more.
pub unsafe fn free_raw(ptr: *mut u8, bone_tex: [u32; 2]) {
    for h in bone_tex {
        texture::release(h);
    }
    memory::free_alloc(ptr);
}

/// [`free_raw`] for an item that was never handed to a node.
pub fn free(item: RenderItem) {
    let (ptr, tex) = item.into_raw();
    // SAFETY: we own the block; no node ever saw it.
    unsafe { free_raw(ptr, tex) }
}
