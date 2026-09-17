//! Scene-node layout — the PURE half of `node.rs` (design §4.2.5 / §5.1 as
//! amended by the Step 1 RE: `+0xE8` sort key, `**ctx` visible push).
//!
//! Dependency-free (no `crate::` imports, no `unsafe`) so the host harness
//! can `#[path]`-mount it and pin the layout with `offset_of!` tests.
//!
//! The node is a MOD-OWNED object; the engine reads exactly these fields of
//! it (`SceneGraph::update` `FUN_180214570`, the destroy flush
//! `FUN_180024250`, the visible-vector sort `FUN_180215830` — RE §1.3, all
//! byte-shape identical on the five supported builds, RE §1.9):
//!
//! | offset | field | reader |
//! |---|---|---|
//! | `+0x00` | vtable (`[0]` dtor, `[1]` visit) | update (visit), flush (dtor) |
//! | `+0x08` | u32 flags, bit0 enabled | update (gate result when the pass bit is clear) |
//! | `+0x0C` | u32 pass mask (`0x10` update, `0x8` collect) | update (per-pass gate) |
//! | `+0x10/+0x18/+0x20` | parent / first child / next sibling | update (walk), flush (unlink) |
//! | `+0x78` | render item | update (item push) — `scene3d_node_item_off` |
//! | `+0xE8` | i32 sort key | visible-vector std::sort — `scene3d_node_sort_key_off` |
//!
//! `node.rs` cross-checks `+0x78`/`+0xE8` against the derived values at
//! `new_node` and refuses on a mismatch.

use std::sync::atomic::{AtomicPtr, AtomicU32, AtomicU8};

/// `flags` bit0.
pub const NODE_FLAG_ENABLED: u32 = 1;
/// `pass_mask` bit for pass 2 (update; ctx = `&f32 dt`).
pub const NODE_PASS_UPDATE: u32 = 0x10;
/// `pass_mask` bit for pass 3 (refresh) — deliberately NOT set.
pub const NODE_PASS_REFRESH: u32 = 0x4;
/// `pass_mask` bit for pass 4 (collect; ctx = `&&visible_vec`).
pub const NODE_PASS_COLLECT: u32 = 0x8;
/// What our nodes carry.
pub const NODE_PASS_MASK: u32 = NODE_PASS_UPDATE | NODE_PASS_COLLECT;

/// Engine pass ids as `visit(this, pass, ctx)` receives them.
pub const PASS_UPDATE: i32 = 2;
pub const PASS_REFRESH: i32 = 3;
pub const PASS_COLLECT: i32 = 4;
pub const PASS_CULL: i32 = 5;

pub const NODE_SIZE: usize = 0x100;
/// The engine-read offsets (cross-checked against the derivation).
pub const NODE_ITEM_OFF: usize = 0x78;
pub const NODE_SORT_KEY_OFF: usize = 0xE8;

/// The mod-owned scene node. `#[repr(C)]`, exactly `NODE_SIZE` bytes.
#[repr(C)]
pub struct SceneNode {
    /// `+0x00` mod-owned vtable (`[-1]` COL null, `[0]` dtor, `[1]` visit).
    pub vtable: *const *const u8,
    /// `+0x08` bit0 = enabled. Atomic on OUR side (the game thread toggles
    /// it while the job thread reads it); the engine reads the same u32.
    pub flags: AtomicU32,
    /// `+0x0C` pass mask (`NODE_PASS_MASK`).
    pub pass_mask: u32,
    /// `+0x10`
    pub parent: *mut SceneNode,
    /// `+0x18`
    pub first_child: *mut SceneNode,
    /// `+0x20`
    pub next_sibling: *mut SceneNode,
    /// `+0x28..+0x78` — A3's TransformNode kept local/world RT here; the
    /// engine never reads it for foreign nodes.
    _engine: [u8; 0x50],
    /// `+0x78` the render item (REQUIRED — the item push reads it). Atomic on
    /// our side: written at build and nulled by the dtor, read by `visit`.
    pub item: AtomicPtr<u8>,
    // ── private (never read by the engine) ──
    /// `+0x80` set by the dtor; polled by the lifecycle, which then frees the
    /// node block itself (the dtor never frees the node — a poll after the
    /// free would read dead memory).
    pub destroyed: AtomicU8,
    /// `+0x81` copied into the item's hidden bit on every pass 4.
    pub hidden: AtomicU8,
    _pad0: [u8; 2],
    /// `+0x84` the item pass mask `visit(4)` writes (2 / 4 / 0x10).
    pub item_pass_mask: AtomicU32,
    /// `+0x88` the item's bone-texture handles (released by the dtor).
    pub bone_tex: [u32; 2],
    /// `+0x90` index into the session's FrameState instance table (Step 7).
    pub instance: u32,
    _pad1: [u8; 0x54],
    /// `+0xE8` sort key (0 dancers, the `:N` priority for stage parts).
    pub sort_key: i32,
    _tail: [u8; 0x14],
}

impl SceneNode {
    /// A zeroed node (null vtable — `node.rs` installs the real one).
    pub const fn zeroed() -> Self {
        SceneNode {
            vtable: std::ptr::null(),
            flags: AtomicU32::new(0),
            pass_mask: 0,
            parent: std::ptr::null_mut(),
            first_child: std::ptr::null_mut(),
            next_sibling: std::ptr::null_mut(),
            _engine: [0; 0x50],
            item: AtomicPtr::new(std::ptr::null_mut()),
            destroyed: AtomicU8::new(0),
            hidden: AtomicU8::new(0),
            _pad0: [0; 2],
            item_pass_mask: AtomicU32::new(0),
            bone_tex: [0; 2],
            instance: 0,
            _pad1: [0; 0x54],
            sort_key: 0,
            _tail: [0; 0x14],
        }
    }
}

/// The engine's visible-node vector as `visit(4)` sees it through `**ctx`
/// (`graph+0x58/+0x60/+0x68`): `{begin, end, cap}` of `Node*`.
#[repr(C)]
pub struct VisibleVec {
    pub begin: *mut *mut SceneNode,
    pub end: *mut *mut SceneNode,
    pub cap: *mut *mut SceneNode,
}

/// Push `node` onto the visible vector WITHOUT growing it (the engine's grow
/// helper must never run on the job thread). `false` = full, skipped — the
/// node is invisible this frame (fail-open).
///
/// # Safety
/// `vec` is the live engine vector and `vec.end < vec.cap` implies `end`
/// is a writable slot (true for the engine's reserved 0x200-entry buffer).
pub unsafe fn push_visible(vec: &mut VisibleVec, node: *mut SceneNode) -> bool {
    if vec.end.is_null() || vec.end >= vec.cap {
        return false;
    }
    *vec.end = node;
    vec.end = vec.end.add(1);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn engine_read_offsets() {
        assert_eq!(offset_of!(SceneNode, vtable), 0x00);
        assert_eq!(offset_of!(SceneNode, flags), 0x08);
        assert_eq!(offset_of!(SceneNode, pass_mask), 0x0C);
        assert_eq!(offset_of!(SceneNode, parent), 0x10);
        assert_eq!(offset_of!(SceneNode, first_child), 0x18);
        assert_eq!(offset_of!(SceneNode, next_sibling), 0x20);
        assert_eq!(offset_of!(SceneNode, item), NODE_ITEM_OFF);
        assert_eq!(offset_of!(SceneNode, sort_key), NODE_SORT_KEY_OFF);
        assert_eq!(size_of::<SceneNode>(), NODE_SIZE);
    }

    #[test]
    fn private_offsets() {
        assert_eq!(offset_of!(SceneNode, destroyed), 0x80);
        assert_eq!(offset_of!(SceneNode, hidden), 0x81);
        assert_eq!(offset_of!(SceneNode, item_pass_mask), 0x84);
        assert_eq!(offset_of!(SceneNode, bone_tex), 0x88);
        assert_eq!(offset_of!(SceneNode, instance), 0x90);
    }

    #[test]
    fn pass_mask_bits() {
        assert_eq!(NODE_PASS_MASK, 0x18);
        assert_eq!(NODE_PASS_MASK & NODE_PASS_REFRESH, 0);
    }

    #[test]
    fn visible_push_respects_capacity() {
        let mut slots: [*mut SceneNode; 2] = [std::ptr::null_mut(); 2];
        let base = slots.as_mut_ptr();
        let mut vec = VisibleVec {
            begin: base,
            end: base,
            cap: unsafe { base.add(2) },
        };
        let a = 0x10usize as *mut SceneNode;
        let b = 0x20usize as *mut SceneNode;
        let c = 0x30usize as *mut SceneNode;
        unsafe {
            assert!(push_visible(&mut vec, a));
            assert!(push_visible(&mut vec, b));
            // full: refused, untouched
            assert!(!push_visible(&mut vec, c));
        }
        assert_eq!(slots, [a, b]);
        assert_eq!(vec.end, vec.cap);
    }

    #[test]
    fn visible_push_null_vector() {
        let mut vec = VisibleVec {
            begin: std::ptr::null_mut(),
            end: std::ptr::null_mut(),
            cap: std::ptr::null_mut(),
        };
        assert!(!unsafe { push_visible(&mut vec, 0x10usize as *mut SceneNode) });
    }
}
