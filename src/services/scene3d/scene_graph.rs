//! Root attach, deferred destroy and camera slot 0 (design §4.2.6 / §5.3;
//! RE §1.1 object graph, §1.2 lock + A3 attach sequence, §1.7 camera fields).
//!
//! Every function here is GAME-THREAD ONLY and fail-open: every engine
//! pointer is probed with `memory::is_readable` before it is dereferenced,
//! every offset comes from `scene3d::sites()`, and `false` means "nothing
//! was written" so the caller can WARN once and carry on without the scene.
//!
//! The attach / destroy paths take the SceneGraphManager's avs mutex through
//! [`with_avs_mutex`](super::with_avs_mutex) — the exact sequence the
//! engine's own flush (`FUN_180024250`) uses — because `SceneGraph::update`
//! walks the child list on a job-graph worker while we splice it.

use std::sync::OnceLock;

use crate::core::memory;
use crate::log_info;

use super::node_layout::SceneNode;
use super::sites;

/// Whether the group resolved (alias of the service's availability).
pub fn is_available() -> bool {
    super::is_available()
}

/// `(mgr, graph)` — the live manager and its graph (the root node), or
/// `None` when either pointer is unreadable/null.
fn manager_and_graph() -> Option<(*mut u8, *mut u8)> {
    let s = sites()?;
    if !memory::is_readable(s.scene_graph_manager, 8) {
        return None;
    }
    let mgr = unsafe { memory::read_ptr(s.scene_graph_manager) } as *mut u8;
    // The fields we touch: graph ptr @0, destroy vec, mutex/depth.
    let span = s
        .mgr_destroy_vec_off
        .max(s.mgr_mutex_off)
        .max(s.mgr_depth_off)
        .max(s.mgr_rate_off)
        + 24;
    if mgr.is_null() || !memory::is_readable(mgr, span) {
        return None;
    }
    let graph = unsafe { memory::read_ptr(mgr) } as *mut u8;
    let gspan = s
        .graph_root_child_off
        .max(s.graph_camera_vec_off)
        .max(s.graph_flags_off)
        + 24;
    if graph.is_null() || !memory::is_readable(graph, gspan) {
        return None;
    }
    Some((mgr, graph))
}

/// Head-insert `node` under the graph root under the manager lock (A3
/// `FUN_18001c300`): `node.parent = graph; node.next_sibling = root.first;
/// root.first = node`. `false` ⇒ nothing was linked.
///
/// The node must be fully initialised (vtable, flags, item) BEFORE this call
/// — the job thread may visit it on the very next frame.
pub fn attach_under_root(node: *mut SceneNode) -> bool {
    if node.is_null() {
        return false;
    }
    let Some(s) = sites() else { return false };
    let Some((mgr, graph)) = manager_and_graph() else {
        return false;
    };
    // SAFETY: `mgr`/`graph` were probed for the fields used; `node` is ours.
    unsafe {
        let mutex = mgr.add(s.mgr_mutex_off) as *const i32;
        let depth = mgr.add(s.mgr_depth_off) as *mut i32;
        super::with_avs_mutex(mutex, depth, || {
            let head_slot = graph.add(s.graph_root_child_off) as *mut *mut SceneNode;
            let n = &mut *node;
            n.parent = graph as *mut SceneNode;
            n.next_sibling = *head_slot;
            // Publish the fully-linked node with a single pointer store AFTER
            // its `next` is in place: a concurrent walker (SceneGraph::update
            // does not take this lock) sees either the old head or a node
            // whose chain is already valid.
            std::sync::atomic::fence(std::sync::atomic::Ordering::Release);
            std::ptr::write_volatile(head_slot, node);
        })
        .is_some()
    }
}

/// Entries of the mod-owned destroy-vector buffer (see [`queue_destroy`]).
/// The flush drains it every frame; a song never queues more than a couple
/// dozen nodes.
const DESTROY_RESERVE_ENTRIES: usize = 256;
/// The `begin` pointer of the buffer installed into the manager's destroy
/// vector (process lifetime; 0 = allocation failed).
static DESTROY_RESERVE: OnceLock<usize> = OnceLock::new();

/// The engine's allocation header (RE §3.4). Every buffer the engine's
/// `me::` allocator hands out (`FUN_18021f300`) is preceded by
/// `{allocator* @-0x20, raw block @-0x18, size @-0x10, pad}`, and every free
/// (`FUN_1801de6e0`, inlined into the SceneGraphManager shutdown
/// `FUN_180023b30` for the destroy vector's storage) walks that header:
/// `alloc->vt[+0x20](alloc)` (lock), `alloc->vt[+0x18](alloc, raw)` (free),
/// `--alloc->refcount @+0xC`, `alloc->vt[+0x28](alloc)` (unlock), and
/// `alloc->vt[0](alloc, 0)` when the count hit zero AND `alloc->owned @+8`.
const ENGINE_ALLOC_HEADER: usize = 0x20;
/// Slots of the fake allocator's vtable (dtor, size, alloc, free, lock,
/// unlock — the engine reads `+0x00..=+0x28`).
const FAKE_ALLOC_VTABLE_SLOTS: usize = 6;
/// Allocator object: `{vtable @0, owned u8 @8, refcount i32 @0xC}`.
const FAKE_ALLOC_OBJECT_SIZE: usize = 0x10;

/// Every slot of the fake allocator: accept anything, do nothing, return 0.
/// Called by the engine's free path at process shutdown (game thread, our
/// DLL still mapped) and never anywhere else.
unsafe extern "C" fn fake_alloc_noop(_this: *mut u8, _arg: usize) -> usize {
    0
}

/// Allocate the destroy-vector buffer WITH a valid engine allocation header
/// in front of it, pointing at a mod-owned allocator whose every method is a
/// no-op with a refcount that never reaches zero. Returns the `begin`
/// pointer (0 on allocation failure).
///
/// Why: the cabinet close crash (`gamemdx.dll+0x24178` on 20260915 = the
/// shutdown's `MOV RDI,[RBX-0x20]`, 2026-09-16 deploy #2) — the manager
/// shutdown frees the vector's storage through the header of whatever
/// `begin` points at; a bare `VirtualAlloc` block has an unmapped page
/// before it. With the header in place the shutdown's lock/free/unlock land
/// on our no-ops and the buffer is simply leaked (it lived for the process
/// anyway). The `FUN_1801f4150` regrow path frees the old storage the same
/// way, so an engine-side growth would also be harmless.
fn destroy_reserve_begin() -> usize {
    *DESTROY_RESERVE.get_or_init(|| {
        // SAFETY: two fresh zeroed blocks, fully written before either is
        // published; neither is ever freed.
        unsafe {
            let meta = memory::alloc_zeroed(FAKE_ALLOC_VTABLE_SLOTS * 8 + FAKE_ALLOC_OBJECT_SIZE);
            let buf = memory::alloc_zeroed(ENGINE_ALLOC_HEADER + DESTROY_RESERVE_ENTRIES * 8);
            if meta.is_null() || buf.is_null() {
                return 0;
            }
            let vtable = meta as *mut *const u8;
            for i in 0..FAKE_ALLOC_VTABLE_SLOTS {
                *vtable.add(i) = fake_alloc_noop as *const u8;
            }
            let alloc = meta.add(FAKE_ALLOC_VTABLE_SLOTS * 8);
            *(alloc as *mut usize) = meta as usize; // vtable
            *alloc.add(8) = 0; // owned = 0 ⇒ never deleted
            *(alloc.add(0xC) as *mut i32) = i32::MAX / 2; // refcount never hits 0
            let begin = buf.add(ENGINE_ALLOC_HEADER);
            *(begin.sub(0x20) as *mut usize) = alloc as usize;
            *(begin.sub(0x18) as *mut usize) = buf as usize;
            *(begin.sub(0x10) as *mut usize) = DESTROY_RESERVE_ENTRIES * 8;
            begin as usize
        }
    })
}

/// Queue `node` for the engine's deferred destroy (the next manager tick
/// unlinks it and calls OUR dtor). Pushes onto the manager's
/// `std::vector<Node*>` under the lock. `false` when the manager is
/// unreadable or the vector is full (retry next frame).
///
/// **World leaves this vector with ZERO capacity forever** (cabinet, 2026-09-16):
/// the manager ctor `FUN_1800238a0` zeroes `{begin, end, cap}`, the flush
/// `FUN_180024250` only iterates and resets `end = begin`, and no other World
/// function writes them — A3's `push_back` callers were the deleted scene
/// layer. So on the first push into an EMPTY vector (`begin == cap`, both
/// null) this installs a mod-owned process-lifetime buffer as the vector's
/// storage; the engine never grows it, and the flush's semantics
/// (`begin..end` = queued nodes, `end = begin` after draining) hold. The
/// manager SHUTDOWN (`FUN_180023b30`) does free a non-null `begin` through
/// the engine allocation header in front of it — which is why the buffer
/// carries a fake header ([`destroy_reserve_begin`]). A non-null full vector
/// is never grown (that would be the engine's allocation).
pub fn queue_destroy(node: *mut SceneNode) -> bool {
    if node.is_null() {
        return false;
    }
    let Some(s) = sites() else { return false };
    let Some((mgr, _graph)) = manager_and_graph() else {
        return false;
    };
    let reserve = destroy_reserve_begin();
    // SAFETY: as `attach_under_root`; the vector words are inside the probed
    // manager span.
    unsafe {
        let mutex = mgr.add(s.mgr_mutex_off) as *const i32;
        let depth = mgr.add(s.mgr_depth_off) as *mut i32;
        super::with_avs_mutex(mutex, depth, || {
            let vec = mgr.add(s.mgr_destroy_vec_off);
            let begin_slot = vec as *mut *mut *mut SceneNode;
            let end_slot = vec.add(8) as *mut *mut *mut SceneNode;
            let cap_slot = vec.add(16) as *mut *mut *mut SceneNode;
            let mut begin = *begin_slot;
            let mut end = *end_slot;
            let mut cap = *cap_slot;
            if begin.is_null() && end.is_null() && cap.is_null() && reserve != 0 {
                // Take ownership of the never-allocated vector.
                begin = reserve as *mut *mut SceneNode;
                end = begin;
                cap = begin.add(DESTROY_RESERVE_ENTRIES);
                *begin_slot = begin;
                *end_slot = end;
                *cap_slot = cap;
                log_info!(
                    "scene3d: installed a {}-entry destroy-vector buffer (engine allocation header + no-op allocator) into the SceneGraphManager (World never allocates one)",
                    DESTROY_RESERVE_ENTRIES
                );
            }
            if end.is_null() || end >= cap {
                return false;
            }
            *end = node;
            *end_slot = end.add(1);
            true
        })
        .unwrap_or(false)
    }
}

/// Snapshot of the graph's per-frame vectors for the spike diagnostic
/// (`visible` = visible-node vector length after the last update, `items` =
/// render-item list length, `records` = its draw-record total).
#[derive(Clone, Copy, Debug, Default)]
pub struct GraphStats {
    pub enabled: bool,
    pub visible: usize,
    pub items: usize,
    pub records: u32,
}

/// Read [`GraphStats`] (probed; zeros when unreadable). The visible vector
/// sits at `graph + 0x58` (RE §1.1) — derived only as the `**ctx` target of
/// pass 4, so it is read here through the SAME layout facts: `graph+0x58 /
/// +0x60` begin/end; the item list is the object at `*(graph+0x30)` with
/// `begin @+0, end @+8, records @+0x20`.
pub fn graph_stats() -> Option<GraphStats> {
    let s = sites()?;
    let (_mgr, graph) = manager_and_graph()?;
    const VISIBLE_BEGIN: usize = 0x58;
    const ITEM_LIST: usize = 0x30;
    if !memory::is_readable(graph, VISIBLE_BEGIN + 24) {
        return None;
    }
    // SAFETY: probed.
    unsafe {
        let enabled = memory::read_u32(graph.add(s.graph_flags_off)) & 1 != 0;
        let vb = memory::read_ptr(graph.add(VISIBLE_BEGIN)) as usize;
        let ve = memory::read_ptr(graph.add(VISIBLE_BEGIN + 8)) as usize;
        let visible = if ve >= vb { (ve - vb) / 8 } else { 0 };
        let list = memory::read_ptr(graph.add(ITEM_LIST));
        let (items, records) = if !list.is_null() && memory::is_readable(list, 0x28) {
            let b = memory::read_ptr(list) as usize;
            let e = memory::read_ptr(list.add(8)) as usize;
            let n = if e >= b { (e - b) / 8 } else { 0 };
            (n, memory::read_u32(list.add(0x20)))
        } else {
            (0, 0)
        };
        Some(GraphStats {
            enabled,
            visible,
            items,
            records,
        })
    }
}

/// Whether the graph's render-item list (rebuilt by every `SceneGraph::update`)
/// currently holds `item`. `None` when the list is unreadable. Teardown
/// waits for `Some(false)` before queueing a node's destroy so the flush can
/// never free an item a pass may still read this frame (the update rebuilds
/// the list without a disabled node; if the graph is DISABLED the list is
/// stale and this stays `true` until it re-enables — the caller leaks
/// rather than frees on its timeout).
pub fn item_listed(item: *const u8) -> Option<bool> {
    let (_mgr, graph) = manager_and_graph()?;
    const ITEM_LIST: usize = 0x30;
    if !memory::is_readable(graph, ITEM_LIST + 8) {
        return None;
    }
    // SAFETY: probed.
    unsafe {
        let list = memory::read_ptr(graph.add(ITEM_LIST));
        if list.is_null() || !memory::is_readable(list, 0x28) {
            return None;
        }
        let b = memory::read_ptr(list) as usize;
        let e = memory::read_ptr(list.add(8)) as usize;
        if e < b || b == 0 {
            return None;
        }
        let n = ((e - b) / 8).min(0x200);
        if n > 0 && !memory::is_readable(b as *const u8, n * 8) {
            return None;
        }
        for i in 0..n {
            if memory::read_ptr((b + i * 8) as *const u8) == item {
                return Some(true);
            }
        }
        Some(false)
    }
}

/// A camera sample for slot 0 (design §5.3). `l/r/b/t` are the frustum
/// extents at `w = 1` (the near-plane tangents), `near/far` the clip planes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CamSample {
    pub eye: [f32; 3],
    pub target: [f32; 3],
    pub up: [f32; 3],
    pub l: f32,
    pub r: f32,
    pub b: f32,
    pub t: f32,
    pub near: f32,
    pub far: f32,
}

impl CamSample {
    /// The pure `camera_math` view of this sample (`w = 1`) — what the
    /// viewport-pass compositor turns into a clone's view/proj.
    pub fn frustum(&self) -> super::camera_math::Frustum {
        super::camera_math::Frustum {
            eye: self.eye,
            target: self.target,
            up: self.up,
            w: 1.0,
            l: self.l,
            r: self.r,
            b: self.b,
            t: self.t,
            near: self.near,
            far: self.far,
        }
    }

    /// Symmetric perspective: `half_tangent_x` = half the horizontal extent
    /// per unit depth, `aspect` = width / height.
    pub fn perspective(
        eye: [f32; 3],
        target: [f32; 3],
        up: [f32; 3],
        half_tangent_x: f32,
        aspect: f32,
        near: f32,
        far: f32,
    ) -> Self {
        let ty = if aspect > 0.0 {
            half_tangent_x / aspect
        } else {
            half_tangent_x
        };
        CamSample {
            eye,
            target,
            up,
            l: -half_tangent_x,
            r: half_tangent_x,
            b: -ty,
            t: ty,
            near,
            far,
        }
    }
}

/// Write `c` into camera slot 0 and raise the view + projection dirty bytes
/// (the next manager tick rebuilds `+0x08` / `+0x1C8` and copies them into
/// the passes). `false` ⇒ nothing was written.
pub fn write_camera0(c: &CamSample) -> bool {
    let Some(s) = sites() else { return false };
    let Some((_mgr, graph)) = manager_and_graph() else {
        return false;
    };
    // SAFETY: `graph` probed through the camera-vector field; the slot is
    // probed for its whole stride before any write.
    unsafe {
        let cam = memory::read_ptr(graph.add(s.graph_camera_vec_off)) as *mut u8;
        let end = memory::read_ptr(graph.add(s.graph_camera_vec_off + 8)) as *const u8;
        if cam.is_null() || (end as usize) < cam as usize + s.camera_stride {
            return false;
        }
        if !memory::is_readable(cam, s.camera_stride) {
            return false;
        }
        let write3 = |off: usize, v: [f32; 3]| {
            memory::write_f32(cam.add(off), v[0]);
            memory::write_f32(cam.add(off + 4), v[1]);
            memory::write_f32(cam.add(off + 8), v[2]);
        };
        write3(s.cam_eye_off, c.eye);
        write3(s.cam_target_off, c.target);
        write3(s.cam_up_off, c.up);
        memory::write_f32(cam.add(s.cam_w_off), 1.0);
        memory::write_f32(cam.add(s.cam_l_off), c.l);
        memory::write_f32(cam.add(s.cam_r_off), c.r);
        memory::write_f32(cam.add(s.cam_b_off), c.b);
        memory::write_f32(cam.add(s.cam_t_off), c.t);
        memory::write_f32(cam.add(s.cam_near_off), c.near);
        memory::write_f32(cam.add(s.cam_far_off), c.far);
        memory::write_u8(cam.add(s.cam_view_dirty_off), 1);
        memory::write_u8(cam.add(s.cam_proj_req_off), 1);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::CamSample;

    #[test]
    fn perspective_extents() {
        let c = CamSample::perspective(
            [0.0, 1.2, 4.0],
            [0.0, 0.8, 0.0],
            [0.0, 1.0, 0.0],
            0.5,
            16.0 / 9.0,
            0.1,
            100.0,
        );
        assert_eq!(c.l, -0.5);
        assert_eq!(c.r, 0.5);
        assert!((c.t - 0.28125).abs() < 1e-6);
        assert_eq!(c.b, -c.t);
        assert_eq!(c.near, 0.1);
        assert_eq!(c.far, 100.0);
    }
}
