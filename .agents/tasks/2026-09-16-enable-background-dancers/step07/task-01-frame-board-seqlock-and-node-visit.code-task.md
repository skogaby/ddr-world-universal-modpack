# Task: `scene3d::frame_board` — the seqlocked per-instance pose channel, consumed by the node's `visit(2)`

## Description
Add the publication channel between the game thread (director) and the engine's scene-graph update job:
a process-wide, statically allocated `FrameBoard` of `MAX_INSTANCES` seqlocked slots (world matrix, tint,
hidden bit, bone count, up to `MAX_BONES` MODEL-space bone matrices — every payload word an `AtomicU32` so
the lock-free read is defined behaviour), written by `publish(slot, …)` on the game thread and copied into
the render item by the node's `visit(2)` (job thread: no engine API, no locks, no allocation, no logging).
The node learns its slot through the reserved `SceneNode.instance` field (`+0x90`, `u32::MAX` = no board —
the Step 3/4 static-item behaviour).

## Background
Design §4.4 threading model: item memory of an ATTACHED node is written only inside `visit` on the scene
update job; the game thread produces the per-frame `FrameState` and publishes it through a seqlock. The
node already reserves `instance: u32` at `+0x90` (node_layout.rs) and `visit(2)` currently returns 0 (Step
3's static item). The engine's bone-texture upload computes `invBind[i]·bone[i]` from the item's bone
array, so the array holds MODEL-space bone WORLD matrices (Step 4 proved the bind copy = identity skin).
Stock bone counts: 33 dancers, ≤ 43 stage props (`gm_floor00_kanban`, `gm_disco00_light00`); a stage row
has ≤ 8 parts; a full scene (Step 8) is ≤ 8 + 2 + 10 + 2 = 22 instances.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-16-enable-background-dancers/design/detailed-design.md` (§3.3 per-frame
  flow, §4.2.5 node, §4.3.4 FrameState, §4.4 threading, NFR-5/NFR-6)
- Plan: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md` Step 7

**Additional References (if relevant to this task):**
- `src/services/scene3d/node.rs` (`node_visit`, `new_node`), `src/services/scene3d/node_layout.rs`
  (`SceneNode.instance`), `src/services/scene3d/render_item.rs` (raw setters), `docs/background_dancers_research.md`
  §1.3 (pass protocol), §2.3/§2.4 (upload reads)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `src/services/scene3d/frame_board.rs`: `pub const MAX_INSTANCES: usize = 32; pub const MAX_BONES: usize
   = 64; pub const NO_SLOT: u32 = u32::MAX;` `struct Slot { seq: AtomicU32, hidden: AtomicU32, bone_count:
   AtomicU32, world: [AtomicU32; 16], tint: [AtomicU32; 4], bones: [AtomicU32; MAX_BONES * 16] }`, `static
   BOARD: [Slot; MAX_INSTANCES]` (const-initialised). Game-thread API: `publish(slot: u32, world: &Mat4,
   tint: [f32; 4], hidden: bool, bones: &[Mat4])` (seq → odd, Release; payload Relaxed stores; seq → even,
   Release; bones beyond `MAX_BONES` truncated, `bone_count = min`), `clear(slot)` (seq reset to 0 = "never
   published"), `reset_all()`. Job-thread API: `pub unsafe fn apply_to_item(slot: u32, item: *mut u8) ->
   bool` — bounded seqlock read (≤ 8 retries; `seq == 0` or odd-after-retries ⇒ `false`, item untouched),
   copies into a stack buffer (`[f32; 16]`, `[f32; 4]`, `[[f32; 16]; MAX_BONES]` — ~4 KB, fine on the job
   thread), then `set_world_raw` / `set_tint_raw` / `set_bones_raw(item, &bones[..n], n)` /
   `set_hidden_raw`. Memory ordering: seq load Acquire before, `fence(Acquire)` after the payload loads,
   seq load again; equal + even ⇒ consistent.
2. `node.rs`: `new_node(item, item_pass_mask, sort_key, instance: u32)`; `visit(2)`: enabled check first,
   `instance != NO_SLOT` ⇒ `frame_board::apply_to_item(instance, item)`; the pass-4 path is unchanged
   except that a board-driven node's `hidden` is ALSO honoured from the board (the item hidden bit written
   by `apply_to_item` is what pass 4 must not overwrite — make pass 4 write `node.hidden || board_hidden`
   by keeping `apply_to_item`'s hidden write and having pass 4 only SET the bit when `node.hidden`, never
   clear it, for board-driven nodes). Update `spike.rs`'s `new_node` call to pass `NO_SLOT` (it is deleted
   in task 03, but the crate must build after this task).
3. Host tests (`frame_board.rs` is std-only apart from the `render_item` raw setters — keep the payload
   store/load in a pure inner type `SlotData`/`read_slot(slot) -> Option<(world, tint, hidden, bones)>` so
   the harness can mount `frame_board.rs` behind a `#[cfg(not(test))]`-free seam: put the item write in
   `node.rs`/a thin wrapper instead and keep `frame_board.rs` free of `crate::`): publish/read round trip,
   `seq == 0` ⇒ `None`, bone truncation at `MAX_BONES`, a writer-in-progress (odd seq) is never returned,
   `clear` invalidates, slot index ≥ `MAX_INSTANCES` is a no-op / `None`. Mount in
   `scripts/validate_background_dancers.sh`.
4. Rust Quality Rules: `apply_to_item`/`read_slot` are panic-free (no indexing on untrusted values — slot
   bounds-checked), no allocation, no logging.

## Dependencies
- Steps 3/4 (`node.rs`, `render_item.rs`).

## Implementation Approach
1. `frame_board.rs` (pure `read_slot` + `publish`), then the `node.rs` wiring (`instance` param, `visit(2)`).
2. Tests + harness mount; `cargo check --target x86_64-pc-windows-msvc`; `cargo fmt`.

## Acceptance Criteria

1. **Round trip**
   - Given a published slot with 33 bones
   - When `read_slot` runs
   - Then world/tint/hidden/bone_count/bones equal the published values bit-for-bit, and an unpublished
     slot reads `None`

2. **Torn reads never leak**
   - Given a slot whose `seq` is odd
   - When `read_slot` runs
   - Then it returns `None` after its bounded retries

3. **Gates**
   - Given the finished change
   - When `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`, `./scripts/validate_background_dancers.sh` run
   - Then all are clean/green

## Metadata
- **Complexity**: Medium
- **Labels**: scene3d, background-dancers, step-7, threading, seqlock
- **Required Skills**: Rust atomics / memory ordering, the repo's node/item layout
- **Generated By**: code-task-generator 2026-09-16
- **Source Plan**: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md`
- **Plan Step**: Step 7: Director, session and lifecycle — first fully animated random song
