//! Model-registry readiness (design §4.2.2).
//!
//! The engine's `ModelFileCallback` converts every `.model` arc member into a
//! GPU model resource and registers it in the `ResourceManager`'s model
//! `std::map<u32 hash, …>` under **FNV-1 of the bare file stem** (the leaf
//! filename with `.model` stripped — `gm_boom00_footpanel`; plain FNV-1, NOT
//! the texture hasher's lowercase/underscore-stripped variant). World deleted
//! A3's lookup-by-hash (`FUN_180146590` — its only callers were the removed
//! scene layer), so this module walks the red-black tree itself, read-only,
//! under the map's own avs mutex, exactly as the surviving `release`
//! (`FUN_180203b60`) does — every offset comes from the derived
//! [`Scene3dSites`] (RE: `docs/background_dancers_research.md` §1.5).
//!
//! [`ResourceView`] exposes the GPU resource's fields the render-item builder
//! consumes. Those offsets are the ONE literal engine layout in this service:
//! they are attested identical on all five supported builds by the Step 1
//! consumer shape diff (§1.9 — the collector / upload / draw / item-push read
//! `res+0x1C/+0x20/+0x24/+0x28/+0x30/+0x48/+0x50/+0x68/+0x78/+0x88` on every
//! build), kept as named consts so a future derivation can replace them.
//!
//! [`model_resource`] is GAME-THREAD ONLY (it takes the engine's lock).

use crate::core::memory;

use super::sites;

pub use super::pure::fnv1_name_hash;

/// Look a converted model up by its bare stem. `Some(gpu_resource)` once the
/// engine has registered it, `None` while it is still loading / absent / the
/// service is unavailable. Fail-open on every dereference (a torn map or a
/// stale pointer yields `None`, never a fault). GAME THREAD ONLY.
pub fn model_resource(name: &str) -> Option<*const u8> {
    model_resource_by_hash(fnv1_name_hash(name))
}

/// [`model_resource`] with a precomputed hash (per-frame polls).
pub fn model_resource_by_hash(hash: u32) -> Option<*const u8> {
    let s = sites()?;
    // The ResourceManager is a plain global POINTER: `*global` = the object.
    if !memory::is_readable(s.resource_manager, 8) {
        return None;
    }
    let rm = unsafe { memory::read_ptr(s.resource_manager) };
    if !memory::is_readable(rm, s.rm_model_mutex_off + 8) {
        return None;
    }
    let mutex_field = unsafe { rm.add(s.rm_model_mutex_off) } as *const i32;
    let depth_field = unsafe { rm.add(s.rm_model_mutex_off + 4) } as *mut i32;
    // SAFETY: `rm` was probed readable through the mutex field; the walk
    // probes every node before touching it; we are on the game thread (the
    // only caller), where the engine itself takes this lock.
    unsafe { super::with_avs_mutex(mutex_field, depth_field, || walk(rm, hash)) }.flatten()
}

/// The red-black-tree lower-bound walk (`std::map::find` as MSVC inlines it),
/// caller holds the map mutex. Every node is probed readable first.
///
/// # Safety
/// `rm` must be the live ResourceManager and the caller must hold its lock.
unsafe fn walk(rm: *const u8, hash: u32) -> Option<*const u8> {
    let s = sites()?;
    let node_span = s
        .rm_node_value_off
        .max(s.rm_node_nil_off)
        .max(s.rm_node_key_off + 4)
        .max(s.rm_node_right_off + 8)
        + 8;
    let map = rm.add(s.rm_model_map_off);
    if !memory::is_readable(map, 16) {
        return None;
    }
    let head = memory::read_ptr(map.add(8));
    if !memory::is_readable(head, node_span) {
        return None;
    }
    let readable_node = |n: *const u8| memory::is_readable(n, node_span);
    let is_nil = |n: *const u8| memory::read_u8(n.add(s.rm_node_nil_off)) != 0;
    let key = |n: *const u8| memory::read_u32(n.add(s.rm_node_key_off));

    let mut node = memory::read_ptr(head.add(8)); // head->parent = root
    let mut best = head;
    // Bound the walk: a well-formed red-black tree over ≤ 64k models is < 40
    // deep; anything longer is a corrupt/torn structure.
    for _ in 0..64 {
        if !readable_node(node) || is_nil(node) {
            break;
        }
        if key(node) < hash {
            node = memory::read_ptr(node.add(s.rm_node_right_off));
        } else {
            best = node;
            node = memory::read_ptr(node); // left child at +0
        }
    }
    if best == head || hash < key(best) {
        return None;
    }
    let value = memory::read_ptr(best.add(s.rm_node_value_off));
    if value.is_null() {
        None
    } else {
        Some(value)
    }
}

// ── GPU model resource view ──────────────────────────────────────────

/// `gs::ModelData` GPU-resource field offsets — read by the engine's collector
/// / bone-texture upload / draw / item push on every supported build
/// identically (Step 1 shape diff, RE §1.9).
const RES_FLAGS: usize = 0x1C; // bit0 = skinned
const RES_BONE_COUNT: usize = 0x20;
const RES_DRAW_RECORD_COUNT: usize = 0x24;
const RES_MATERIAL_COUNT: usize = 0x28;
const RES_PALETTE_COUNT: usize = 0x30;
const RES_BIND: usize = 0x48; // f32[16] × bones
const RES_INVERSE_BIND: usize = 0x50; // f32[16] × bones
const RES_DRAW_RECORDS: usize = 0x68; // × 0x48
const RES_MATERIALS: usize = 0x78; // × 0x168
const RES_PALETTES: usize = 0x88; // × 200 bytes
/// Bytes of the resource header the view reads.
const RES_HEADER_SPAN: usize = 0x90;

/// Typed, probe-guarded accessors over a GPU model resource.
#[derive(Clone, Copy, Debug)]
pub struct ResourceView {
    res: *const u8,
}

impl ResourceView {
    /// Wrap a resource pointer; `None` if its header is not readable.
    pub fn new(res: *const u8) -> Option<Self> {
        if memory::is_readable(res, RES_HEADER_SPAN) {
            Some(Self { res })
        } else {
            None
        }
    }
    pub fn ptr(&self) -> *const u8 {
        self.res
    }
    fn u32_at(&self, off: usize) -> u32 {
        // SAFETY: `new` probed RES_HEADER_SPAN bytes readable.
        unsafe { memory::read_u32(self.res.add(off)) }
    }
    fn ptr_at(&self, off: usize) -> *const u8 {
        unsafe { memory::read_ptr(self.res.add(off)) }
    }
    pub fn is_skinned(&self) -> bool {
        self.u32_at(RES_FLAGS) & 1 != 0
    }
    pub fn bone_count(&self) -> u32 {
        self.u32_at(RES_BONE_COUNT)
    }
    pub fn draw_record_count(&self) -> u32 {
        self.u32_at(RES_DRAW_RECORD_COUNT)
    }
    pub fn material_count(&self) -> u32 {
        self.u32_at(RES_MATERIAL_COUNT)
    }
    pub fn palette_count(&self) -> u32 {
        self.u32_at(RES_PALETTE_COUNT)
    }
    /// MODEL-space bind matrices, `f32[16] × bone_count`.
    pub fn bind(&self) -> *const u8 {
        self.ptr_at(RES_BIND)
    }
    pub fn inverse_bind(&self) -> *const u8 {
        self.ptr_at(RES_INVERSE_BIND)
    }
    /// GPU draw records, 0x48 bytes each.
    pub fn draw_records(&self) -> *const u8 {
        self.ptr_at(RES_DRAW_RECORDS)
    }
    /// Materials, 0x168 bytes each.
    pub fn materials(&self) -> *const u8 {
        self.ptr_at(RES_MATERIALS)
    }
    /// Bone palettes, 200 bytes each.
    pub fn palettes(&self) -> *const u8 {
        self.ptr_at(RES_PALETTES)
    }
}
