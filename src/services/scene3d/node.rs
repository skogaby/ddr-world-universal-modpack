//! The mod-owned scene-graph node (design §4.2.5; layout in
//! [`node_layout`](super::node_layout); pass protocol RE §1.3).
//!
//! ONE flat node type: it carries a render item, is attached directly under
//! the graph root (never nested — the engine's destroy flush clears child
//! links without calling a child's dtor), and is torn down ONLY through the
//! manager's deferred-destroy vector, which makes the flush call our dtor
//! with `free = 1` on a job-graph thread under the manager lock.
//!
//! ## The two `extern "C"` entry points
//!
//! Both run on the engine's job-graph worker: NO engine API, NO locks, NO
//! allocation, NO logging, panic-free, `catch_unwind`-wrapped.
//!
//! * `visit(this, pass, ctx)`: pass 2 → for a board-driven node (`instance
//!   != frame_board::NO_SLOT`) copy the newest consistent
//!   [`frame_board`](super::frame_board) snapshot (world, tint, hidden, bones)
//!   into the item — the ONLY writer of an attached item's pose (design §4.4);
//!   a static node (the Step 3/4 shape) does nothing here; pass 4 → refresh
//!   the item's hidden bit (static nodes: the node flag; board nodes: the flag
//!   can only FORCE hidden, the board's own hidden bit from pass 2 otherwise
//!   stands) + pass mask, push `this` onto `**ctx` (the visible vector) if
//!   there is room, return 0 (flat: nothing to recurse into); every other
//!   pass → 0.
//! * `dtor(this, free)`: release the item's bone textures + free the item
//!   block (`render_item::free_raw`), mark `destroyed`. The NODE block is NOT
//!   freed here even though the flush passes `free = 1`: the game-thread
//!   lifecycle polls `destroyed` and a block freed under that poll would be
//!   dead memory. The lifecycle frees the node with [`free_node_block`] once
//!   it has observed the flag (the engine holds no reference after the flush
//!   unlinked it).

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::Ordering;
use std::sync::OnceLock;

use crate::core::memory;
use crate::log_warn;
use crate::services::song_reset;

use super::frame_board::{self, SlotRead, NO_SLOT};
use super::node_layout::{
    push_visible, SceneNode, VisibleVec, NODE_FLAG_ENABLED, NODE_ITEM_OFF, NODE_PASS_MASK,
    NODE_SIZE, NODE_SORT_KEY_OFF, PASS_COLLECT, PASS_UPDATE,
};
use super::render_item::{self, RenderItem};
use super::sites;

/// `[COL, slot0 dtor, slot1 visit, 6 spare]` — the installed vtable pointer
/// is `image + 8` (MSVC keeps the RTTI locator at `vtable[-1]`; ours is null
/// — the engine never walks RTTI on scene nodes).
const VTABLE_IMAGE_SLOTS: usize = 1 + 8;

static VTABLE: OnceLock<usize> = OnceLock::new();

/// Build (once) and return the installed vtable pointer, or null when the
/// allocation failed.
fn vtable() -> *const *const u8 {
    let p = *VTABLE.get_or_init(|| {
        // SAFETY: fresh RWX block, written before publication.
        unsafe {
            let raw = memory::alloc_zeroed(VTABLE_IMAGE_SLOTS * 8);
            if raw.is_null() {
                return 0;
            }
            let slots = raw as *mut *const u8;
            *slots = std::ptr::null(); // [-1] COL
            *slots.add(1) = node_dtor as *const u8;
            *slots.add(2) = node_visit as *const u8;
            slots.add(1) as usize
        }
    });
    p as *const *const u8
}

/// Slot 0: `dtor(this, free)`. Job-graph thread, under the manager lock.
/// `free` is deliberately ignored for the node block (see the module doc).
unsafe extern "C" fn node_dtor(this: *mut SceneNode, _free: u8) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if this.is_null() {
            return;
        }
        let node = &*this;
        let item = node.item.swap(std::ptr::null_mut(), Ordering::AcqRel);
        let bone_tex = node.bone_tex;
        if !item.is_null() {
            render_item::free_raw(item, bone_tex);
        }
        node.destroyed.store(1, Ordering::Release);
    }));
}

/// Slot 1: `u32 visit(this, pass, ctx)`. Job-graph thread.
unsafe extern "C" fn node_visit(this: *mut SceneNode, pass: i32, ctx: *mut u8) -> u32 {
    catch_unwind(AssertUnwindSafe(|| {
        if this.is_null() {
            return 0;
        }
        let node = &*this;
        if node.flags.load(Ordering::Relaxed) & NODE_FLAG_ENABLED == 0 {
            return 0;
        }
        let item = node.item.load(Ordering::Acquire);
        if item.is_null() {
            return 0;
        }
        let instance = node.instance;
        if pass == PASS_UPDATE {
            // Board-driven node: copy the newest consistent snapshot. A
            // torn/unpublished read leaves the item exactly as it was.
            if instance != NO_SLOT {
                let mut snap = SlotRead::zeroed();
                if frame_board::read_slot_into(instance, &mut snap) {
                    render_item::set_world_raw(item, &snap.world);
                    render_item::set_tint_raw(item, snap.tint);
                    render_item::set_bones_raw(
                        item,
                        &snap.bones[..snap.bone_count],
                        snap.bone_count,
                    );
                    render_item::set_hidden_raw(item, snap.hidden);
                }
            }
            return 0;
        }
        if pass != PASS_COLLECT || ctx.is_null() {
            // Pass 3/5/6: 0.
            return 0;
        }
        // Shutdown guard (cabinet 2026-09-16, RE §3.3): closing the game
        // mid-song tears the sequence down FIRST and the graphics/resource
        // side afterwards while this job still runs — an item of ours left
        // in the collected list is then uploaded/drawn against freed
        // resources (fault inside d3d/ntdll). No game-thread frame runs in
        // that window, so the job thread itself refuses to stay listed once
        // the live DancePlaySequence is gone: nothing of ours is collected
        // from the next update on. Probed reads only (no engine API).
        if instance != NO_SLOT && !song_reset::live_dps_probed() {
            return 0;
        }
        let node_hidden = node.hidden.load(Ordering::Relaxed) != 0;
        if instance == NO_SLOT {
            render_item::set_hidden_raw(item, node_hidden);
        } else if node_hidden {
            render_item::set_hidden_raw(item, true);
        }
        render_item::set_pass_mask_raw(item, node.item_pass_mask.load(Ordering::Relaxed));
        // ctx = &p, p = graph+0x58 (the visible vector).
        let p = *(ctx as *const *mut VisibleVec);
        if p.is_null() {
            return 0;
        }
        let _ = push_visible(&mut *p, this);
        0
    }))
    .unwrap_or(0)
}

/// Whether the derived engine offsets agree with our `#[repr(C)]` layout.
/// Checked once per `new_node`; a mismatch means the build reads the node
/// differently and NOTHING may be attached.
fn layout_matches_derivation() -> bool {
    match sites() {
        Some(s) => s.node_item_off == NODE_ITEM_OFF && s.node_sort_key_off == NODE_SORT_KEY_OFF,
        None => false,
    }
}

/// Allocate + initialise a node owning `item`: enabled, pass mask
/// `0x10 | 0x8`, `item_pass_mask` = the mask `visit(4)` stamps into the
/// item (2 dancers / 4 stage / 0x10 `:N`), `sort_key` as given, `instance` =
/// the [`frame_board`] slot `visit(2)` copies from (`NO_SLOT` = static item).
/// `Err` (one WARN) when the vtable / allocation failed or the derived
/// offsets disagree with the layout — the caller frees the item.
pub fn new_node(
    item: RenderItem,
    item_pass_mask: u32,
    sort_key: i32,
    instance: u32,
) -> Result<*mut SceneNode, RenderItem> {
    if !layout_matches_derivation() {
        log_warn!(
            "scene3d: node layout (+0x{:X}/+0x{:X}) disagrees with the derived offsets -- refusing to build nodes",
            NODE_ITEM_OFF,
            NODE_SORT_KEY_OFF
        );
        return Err(item);
    }
    let vt = vtable();
    if vt.is_null() {
        log_warn!("scene3d: node vtable allocation failed");
        return Err(item);
    }
    // SAFETY: fresh zeroed block of exactly NODE_SIZE bytes; SceneNode is
    // repr(C) and valid when all-zero except the vtable.
    unsafe {
        let raw = memory::alloc_zeroed(NODE_SIZE);
        if raw.is_null() {
            log_warn!("scene3d: node allocation failed");
            return Err(item);
        }
        let node = raw as *mut SceneNode;
        std::ptr::write(node, SceneNode::zeroed());
        let n = &mut *node;
        n.vtable = vt;
        n.flags.store(NODE_FLAG_ENABLED, Ordering::Relaxed);
        n.pass_mask = NODE_PASS_MASK;
        n.item_pass_mask.store(item_pass_mask, Ordering::Relaxed);
        n.sort_key = sort_key;
        n.instance = instance;
        let (ptr, tex) = item.into_raw();
        n.bone_tex = tex;
        n.item.store(ptr, Ordering::Release);
        Ok(node)
    }
}

/// Enable/disable (`flags` bit0). Disabled nodes are skipped by every pass.
/// # Safety
/// `node` came from [`new_node`] and its dtor has not run.
pub unsafe fn set_enabled(node: *mut SceneNode, enabled: bool) {
    if node.is_null() {
        return;
    }
    let n = &*node;
    if enabled {
        n.flags.fetch_or(NODE_FLAG_ENABLED, Ordering::Release);
    } else {
        n.flags.fetch_and(!NODE_FLAG_ENABLED, Ordering::Release);
    }
}

/// Hide/show the item (applied by the next `visit(4)`).
/// # Safety
/// As [`set_enabled`].
pub unsafe fn set_hidden(node: *mut SceneNode, hidden: bool) {
    if node.is_null() {
        return;
    }
    (*node).hidden.store(hidden as u8, Ordering::Relaxed);
}

/// Whether the engine's flush has run our dtor on this node (the item is
/// freed, the node block is still ours to free with [`free_node_block`]).
/// # Safety
/// `node` came from [`new_node`] and has not been passed to
/// [`free_node_block`].
pub unsafe fn is_destroyed(node: *const SceneNode) -> bool {
    if node.is_null() {
        return true;
    }
    (*node).destroyed.load(Ordering::Acquire) != 0
}

/// Free the node block after [`is_destroyed`] returned `true` (the engine
/// unlinked it before running the dtor, so nothing references it).
/// # Safety
/// `is_destroyed(node)` was observed `true`; never call twice.
pub unsafe fn free_node_block(node: *mut SceneNode) {
    memory::free_alloc(node as *mut u8);
}

/// Destroy a node that was NEVER attached (build-path cleanup): runs our own
/// dtor, then frees the node block. Never call this on an attached node —
/// the engine still links it.
/// # Safety
/// `node` came from [`new_node`] and `attach_under_root` never saw it.
pub unsafe fn destroy_unattached(node: *mut SceneNode) {
    node_dtor(node, 1);
    free_node_block(node);
}

/// The node's item pointer (for diagnostics).
/// # Safety
/// As [`set_enabled`].
pub unsafe fn item_ptr(node: *const SceneNode) -> *mut u8 {
    if node.is_null() {
        std::ptr::null_mut()
    } else {
        (*node).item.load(Ordering::Acquire)
    }
}
