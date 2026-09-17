# Task: The mod-owned `SceneNode` (vtable, `visit`, dtor) and `scene_graph.rs` (root attach, deferred destroy, camera slot 0)

## Description
Create the ONE flat node type the DLL hands to `agcs::scene::SceneGraph` and the three engine contact
points that place it: `node.rs` (`#[repr(C)] SceneNode`, 0x100 bytes, a mod-owned 2-slot vtable whose
`visit` implements the pass protocol of RE §1.3 — pass 2 no-op/copy, pass 4 flag refresh + push onto the
visible vector through `**ctx`, everything else returns 0 — and a dtor that frees the render item + node
block), and `scene_graph.rs` (`attach_under_root` head-insertion + `queue_destroy` under the manager's avs
mutex exactly as `FUN_180024250` locks, and `write_camera0` into camera slot 0 with the two dirty bytes).

## Background
`SceneGraph::update` (`FUN_180214570`) walks the root's children each frame on a job-graph worker: pass 2
(`node+0xC & 0x10`, ctx `&f32 dt`), pass 3 (`& 0x4` — we leave that bit CLEAR), pass 4 (`& 0x8`, ctx =
`&p` where `p = graph+0x58` → the visible vector `{begin,end,cap}` of `Node*`; the node must push ITSELF),
then `std::sort` of the visible vector by `*(i32*)(node+0xE8)` (REQUIRED field), then the item push
`*(node+0x78)` per visible node, then pass 5 culling (we return 0 ⇒ never culled). The destroy flush
(`FUN_180024250`, under the manager lock, on the DebugRenderJob thread) unlinks each queued node and calls
`(*vtable[0])(node, 1)` — OUR dtor. The engine never frees a node or item on its own; a hierarchy would
orphan children (the flush clears child links without calling their dtors) — nodes stay FLAT under the
root. Camera slot 0 of `graph+0x38` (stride `camera_stride`) is what the tick rebuilds and copies into
the passes; the write protocol is fields + `+0x2B0 = 1` (view dirty) + `+0x2B1 = 1` (projection dirty);
`+0x2A0/+0x2A4` are read by nothing (design amendment 5). Every offset comes from `scene3d::sites()`.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-16-enable-background-dancers/design/detailed-design.md` (§4.2.5 `node.rs`, §4.2.6 `scene_graph.rs`, §5.1, §5.3 camera slot, §4.4 threading)
- RE record: `docs/background_dancers_research.md` §1.1 (object graph), §1.2 (lock + A3 attach sequence), §1.3 (pass protocol, `+0xE8` sort key, `**ctx` push), §1.7 camera-field row + the write protocol paragraph after the table

**Additional References (if relevant to this task):**
- `src/services/custom_options/rows.rs::build_mod_vtable` and `src/services/foot_panel_swap/layout.rs::bot_vtable_image` (mod-owned vtable image: COL at `[-1]`, installed pointer = `image + 1`)
- `src/services/scene3d/mod.rs::with_avs_mutex` (the lock helper to reuse for the manager lock)
- `src/services/scene3d/render_item.rs` (task 01 — `free`, the `set_*` accessors)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `src/services/scene3d/node.rs`: `#[repr(C)] pub struct SceneNode` exactly 0x100 bytes: `vtable: *const *const u8` @0x00, `flags: u32` @0x08 (bit0 enabled), `pass_mask: u32` @0x0C (`NODE_PASS_UPDATE 0x10 | NODE_PASS_COLLECT 0x8`), `parent/first_child/next_sibling: *mut SceneNode` @0x10/0x18/0x20, engine-shaped padding to `item: *mut u8` @0x78 (the engine reads it), private fields `destroyed: AtomicBool` @0x80, `instance: u32`, `hidden: AtomicBool`, then padding so that `sort_key: i32` lands EXACTLY at @0xE8, padding to 0x100. `offset_of!` tests pin `flags 0x08`, `pass_mask 0x0C`, `parent 0x10`, `first_child 0x18`, `next_sibling 0x20`, `item 0x78`, `sort_key 0xE8`, `size_of == 0x100` (these run on the host — the struct file must be `#[path]`-mountable: put the struct + consts + the two `extern "C"` bodies' PURE logic in `node_layout.rs` if the engine imports make `node.rs` unmountable; at minimum the layout struct and its tests are host-run).
2. Vtable: ONE process-lifetime image built lazily (`OnceLock<usize>`) in `memory::alloc_zeroed`'d memory: `[COL = null, slot0 = node_dtor, slot1 = node_visit]` (+ 6 spare null slots is fine); the installed pointer is `image + 8`. `unsafe extern "C" fn node_dtor(this: *mut SceneNode, free: u8)`: `catch_unwind`; if `this` non-null: take `item` (`+0x78`) and null it, `render_item::free` when non-null, `destroyed.store(true)`, then if `free != 0` free the node block (`VirtualFree`). NO logging, NO locks, NO engine calls other than `texture_release` inside `render_item::free` (runs inside the manager flush under the manager lock on a job thread). `unsafe extern "C" fn node_visit(this, pass: i32, ctx: *mut u8) -> u32`: `catch_unwind` → 0 on panic; pass 2: `return 0` (Step 3: static; Step 7 copies the FrameState); pass 4: if enabled and `item != null`: `item.set_hidden(hidden)`, `item.set_pass_mask(node_pass_mask_for_item)` (stored on the node at build), then `let vec = **(ctx as *const *const *mut VisibleVec)`; if `vec.end < vec.cap { *vec.end = this; vec.end += 1 }` (never grow); return 0; any other pass: return 0. Both bodies panic-free (no `unwrap`, no allocation, no logging).
3. `pub fn new_node(item: RenderItem, item_pass_mask: u32, sort_key: i32) -> Option<*mut SceneNode>`: `alloc_zeroed(0x100)`, install the vtable, `flags = 1`, `pass_mask = 0x18`, `item = item.into_raw()`, `sort_key`, `item_pass_mask` stored privately; returns null-safe `Option`. `pub unsafe fn set_enabled(node, bool)` (bit0 of `+0x08`), `set_hidden(node, bool)`, `is_destroyed(node) -> bool`.
4. `src/services/scene3d/scene_graph.rs`: `pub fn attach_under_root(node: *mut SceneNode) -> bool` — `mgr = *(sites.scene_graph_manager)`, `graph = *mgr` (both `is_readable`-probed), under `with_avs_mutex(mgr+mgr_mutex_off, mgr+mgr_depth_off, …)`: `node.parent = graph; node.next_sibling = *(graph + graph_root_child_off); *(graph + graph_root_child_off) = node` (head insertion — A3 `FUN_18001c300`). `pub fn queue_destroy(node: *mut SceneNode) -> bool` — under the same lock push onto the manager's `std::vector<Node*>` at `mgr + mgr_destroy_vec_off` (`begin @+0, end @+8, cap @+16`): `if end < cap { *end = node; end += 8; true } else { false }` — NEVER grow the engine vector; the caller retries next frame on `false`. `pub struct CamSample { eye: [f32;3], target: [f32;3], up: [f32;3], l, r, b, t, near, far: f32 }`; `pub fn write_camera0(c: &CamSample) -> bool` — `cam = *(graph + graph_camera_vec_off)` (slot 0; also check `cam + camera_active_off` readable), write eye/target/up (`cam_eye/target/up_off`), `cam_w_off = 1.0`, l/r/b/t, near/far, then `*(cam + cam_view_dirty_off) = 1u8` and `*(cam + cam_proj_req_off) = 1u8`. `pub fn is_available() -> bool` = `scene3d::is_available()`. GAME THREAD ONLY (documented). Every dereference probed; any failure ⇒ `false` (callers WARN once).
5. Host tests: the `offset_of!`/`size_of` layout tests; a pure `visible_push` helper (`fn push_visible(vec: &mut VisibleVec, node) -> bool` operating on a `{begin,end,cap}` struct) tested for the full/non-full cases; `CamSample` default-frustum helper `CamSample::perspective(eye, target, half_tangent_x, aspect, near, far)` producing `l/r = ∓t, b/t = ∓t/aspect` tested numerically.
6. Rust Quality Rules: `extern "C"` bodies wrapped in `catch_unwind`, zero engine calls / locks / allocation / logging inside them; narrow `unsafe`; no hardcoded engine offsets except the layout consts of the node itself (the node is OUR object — the engine-read offsets `0x08/0x0C/0x10/0x18/0x20/0x78/0xE8` are cross-checked against `sites().node_item_off` / `node_sort_key_off` at `new_node` and the mismatch logged + refused).

## Dependencies
- Task 01 (`render_item::{RenderItem, free, set_*}`)
- Step 1/2 (`scene3d::sites()`, `with_avs_mutex`)
- `core::memory::{alloc_zeroed, is_readable, read_ptr, write_*}` + the block free helper from task 01

## Implementation Approach
1. `node.rs`: struct + consts + offset tests; vtable image; dtor + visit; `new_node`/setters.
2. `scene_graph.rs`: attach / queue_destroy / write_camera0 / `CamSample`.
3. Mount the pure parts in `scripts/validate_background_dancers.sh`; run it.
4. `cargo check` → `cargo fmt` → `./build.sh`.

## Acceptance Criteria

1. **Layout pinned**
   - Given the host harness
   - When the `offset_of!` tests run
   - Then `item == 0x78`, `sort_key == 0xE8`, `size_of == 0x100`, and `new_node` refuses (logging once) if `sites().node_item_off != 0x78` or `node_sort_key_off != 0xE8`

2. **Attach/destroy protocol**
   - Given a built node on the cabinet
   - When `attach_under_root` then (at window exit) `set_enabled(false)` + `queue_destroy` run
   - Then the next manager tick calls our dtor (`is_destroyed` flips within a frame), the log shows `node destroyed after N ms`, and no WARN

3. **Visible push never grows the engine vector**
   - Given the pure `push_visible` test with `end == cap`
   - When called
   - Then it returns `false` and leaves the vector untouched

4. **Gates**
   - Given the finished change
   - When `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`, `./build.sh`, `./scripts/validate_background_dancers.sh` run
   - Then all are clean/green

## Metadata
- **Complexity**: High
- **Labels**: scene3d, background-dancers, step-3, spike, engine-facing, scene-graph
- **Required Skills**: mod-owned vtables in this codebase, `extern "C"` discipline, the RE §1.3 protocol
- **Generated By**: code-task-generator 2026-09-16
- **Source Plan**: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md`
- **Plan Step**: Step 3: Render item, scene node, root attach/destroy, camera slot 0, background hide — static footpanel (spike GO/NO-GO)
