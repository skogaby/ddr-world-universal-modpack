# Task: `scene3d::viewport_pass` — cloned MODEL passes + a clear viewport in RENDER_2D

## Description
The compositor primitive (design §4.5): a `#[repr(C)] ClearViewport` whose render callback appends one
gd Clear record, byte-clones of the stock OPACITY and TRANS pass objects with their own rect / filter /
camera matrices, attach/detach into the RENDER_2D target list through the engine's own functions, a
DISABLED-bit toggle, a 2-frame reaper for detached objects, and the render-target pixel size. Everything
engine-facing runs on the game thread from an `input_manager::on_frame` callback; the render callback
runs on the engine's render worker and follows the `node_visit` rules (no engine API / allocation /
locks / logging; `catch_unwind`-wrapped).

## Background
`Scene3dSites.viewport` (Step 2) carries every offset. Engine facts (research `preview-compositing.md`,
Ghidra 2026-09-21): a viewport sub-object is `{vtable*, +8 rect{x,y,w,h}, +0x18 minZ, +0x1C maxZ,
+0x20 name, +0x24 flags (bit0 DISABLED, bit1 skip camera upload), +0x28 proj[16], +0x68 view[16]}`;
the worker calls `setup(ctx, vp+8, list)` (SetViewport(rect) then, iff bit1 clear, the matrix upload)
then `vp->vft[0](vp, ctx)` then writes `0x4003A`; the dispatcher skips a viewport with bit0 set;
attach fills w/h from the target's u16 dims only when the rect is all-zero and then push+sorts
`{vp*, prio}`; detach erases. The pass object is the sub-object at `pass + sub_off` (0x30); the pass's
render fn reads `vp+0xB8` (items) and `vp+0xB0` (outer). A Clear record is `{u32 0x00140000, u32 flags,
u32 D3DCOLOR ARGB, f32 z, u32 stencil}` written at `*(workerCtx + 0x218)` and the pointer advanced by
0x14. Free node-mask bits: the four live filters `{0x01, 0x56, 0x10, 0x46}` leave `0x08`/`0x20` clear.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-21-background-dancers-selection-options/design/detailed-design.md` (§3.1, §4.5, §5.2, §6)

**Additional References (if relevant to this task):**
- `.agents/planning/2026-09-21-background-dancers-selection-options/research/preview-compositing.md` §1, §2, §5
- `src/services/scene3d/node.rs` (RWX vtable pattern, callback rules), `src/services/scene3d/scene_graph.rs` (probed engine reads), `src/core/memory.rs`

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `src/services/scene3d/viewport_pass.rs`: `is_available()` (sites present ∧ the four live stock filters leave
   `FILTER_BIT_P1 = 0x08` / `FILTER_BIT_P2 = 0x20` clear — checked once, WARN once); `RtRect`, `ClearSpec`,
   `PassSet { create(filter_bit, rect, clear, base_prio) -> Option<PassSet>, set_rect, set_camera(view, proj),
   set_enabled, detach(self) }`, `reap()`, `render_target_dims() -> Option<(u32,u32)>`, `pub const PRIO_BASE:
   [u32; 2] = [0x68, 0x6B]`, `FILTER_BIT: [u32; 2]`.
2. `#[repr(C)] ClearViewport` (0x40 bytes, offsets asserted with `offset_of!`): vtable, rect, min_z 0.0, max_z 1.0,
   name 0, flags (`2` | disabled bit), payload `clear_flags`, `color_argb`, `z`, `stencil`. Its 2-slot RWX vtable
   built once (`node.rs` pattern; COL at [-1]); slot 0 `extern "C" fn clear_render(vp, ctx)` writes the record at
   `*(ctx + gd_write_off)` and advances (reads the offset from a `static` captured at init — no `sites()` call in
   the callback), slot 1 a no-op dtor. A pure `encode_clear_record(spec) -> [u8; 20]` for the byte pin test.
3. Clone: `memory::is_readable(stock, pass_size)`, identity gate `*(stock+sub_off) == pass_vftable` and
   `*(stock+filter_off) ∈ {0x56, 0x46}`, `alloc_zeroed(pass_size)` + `copy_nonoverlapping`, patch self back-pointer
   (`+self_off = clone`), rect, filter bit, zero the flags word then set bit0 per `enabled`; view/proj written by
   `set_camera`. Attach `clone + sub_off` at `base_prio + 1` (opaque) / `+ 2` (trans); the clear viewport at
   `base_prio`. Every engine call is game-thread only; `create/detach` document "call from `on_frame`".
4. Reaper: `detach` pushes `(ptrs, frame_no)` onto a `Mutex<Vec<…>>`; `reap()` (called once per frame by the
   owner) frees entries ≥ 2 frames old with `memory::free_alloc`.
5. `render_target_dims`: `*(*(display + render2d_list_off) + list_target_off)` probed, u16 at `target_w_off/h_off`.
6. Host tests (pure parts, in-file `#[cfg(test)]`, run by `cargo test`): `ClearViewport` size/offset asserts,
   `encode_clear_record` byte-exact, `RtRect::from_canvas(rect, dims)` at 1280×720 / 1920×1080 / 640×480,
   `PRIO_BASE`/`FILTER_BIT` tables.

## Dependencies
- Step 2 (`Scene3dSites.viewport`), task-01 (`camera_math::Mat4`).

## Implementation Approach
Tests for the pure parts first; then the struct/vtable/callback; then the clone + attach + toggle + reaper.
`cargo check`, `cargo test` (in-crate tests compile on the host? — the crate does not build on ARM hosts
because of `retour`; mount the pure parts into `scripts/validate_background_dancers.sh` by splitting them into
`viewport_pass_layout.rs` (std-only) like `node_layout.rs`).

## Acceptance Criteria
1. **Layout** — `size_of::<ClearViewport>() == 0x40`, rect at +8, flags at +0x24 (== derived `vp_flags_off` on the cabinet — asserted at `create`), payload at +0x28.
2. **Clear record bytes** — `encode_clear_record(ClearSpec { depth: true, color: Some(0xFF20A0FF) })` == `00 00 14 00 | 03 00 00 00 | FF A0 20 FF | 00 00 80 3F | 00 00 00 00`.
3. **Rect mapping** — canvas (185+191, 463+11, 170, 150) at 1920×1080 ⇒ (564, 711, 255, 225).
4. **Fail-open** — without `sites.viewport` or with a private bit set in a live filter, `is_available()` is false and `create` returns `None` (one WARN).

## Metadata
- **Complexity**: High
- **Labels**: scene3d, compositor, engine-facing
- **Required Skills**: Rust FFI, the engine's render dispatch
- **Generated By**: code-task-generator 2026-09-21
- **Source Plan**: `.agents/planning/2026-09-21-background-dancers-selection-options/implementation/plan.md`
- **Plan Step**: Step 3: Compositor primitive + cabinet clear-rectangle smoke
