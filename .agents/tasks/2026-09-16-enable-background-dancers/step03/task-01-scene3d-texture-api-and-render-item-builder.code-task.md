# Task: `services/scene3d` texture API wrappers and the render-item builder

## Description
Add the two engine-facing pieces the GO/NO-GO spike needs before a node can exist: `texture.rs` (the
engine texture create/release wrappers plus the gs texture-registry lookup under the engine's spin flag)
and `render_item.rs` (the 0xC8-header render item with its trailing arrays, built from a `ResourceView`
exactly as the World collector / upload / draw read it — design §5.2 as corrected by the Step 3 RE,
`docs/background_dancers_research.md` §2). Rigid models are the target of this step (the boom00
footpanel); the skinned branch (two bone textures, mode `0xF`) is scaffolded but exercised in Step 4.

## Background
The engine never constructs, frees or writes a render item; it only READS the header + trailing arrays
(RE §2.4 table). Everything about the item is therefore the DLL's, with three facts that were only
settled by the Step 3 RE: (1) World's model converter resolves material textures at CONVERSION time
through the gs texture registry (`TextureData* @ mat+0xA8+slot*0x18`, default texture on miss) and A3's
setModel-time re-resolve has no World twin — so a `.dds` registered after its `.model` leaves the
material on the default texture unless the DLL re-resolves into its OWN material copies (§2.1); (2) the
200-byte "palette" is the vertex-stream binding block and both it and the 0x168 material are read-only
for the engine — plain `memcpy`, no addref/release (§2.2, supersedes design amendment 7); (3) the frame
stamp `item+0xB4` MUST be seeded non-zero (`0xFFFFFFFF`) — the upload treats 0 as "claimed" and the pass
driver spins until every item reports done (§2.3).

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-16-enable-background-dancers/design/detailed-design.md` (§4.2.3 `texture.rs`, §4.2.4 `render_item.rs`, §5.2 render-item layout, §4.4 threading, §6 error handling)
- RE record: `docs/background_dancers_research.md` §1.4 (texture create/release API + registry facts), §2.1–§2.5 (texture resolution, copies, frame stamp, consumer field table, the new `texture_lookup_site`)

**Additional References (if relevant to this task):**
- `docs/background_dancers_feasibility.md` §5.1.1 (the same ABI table from the consumers' side)
- `src/services/scene3d/model_registry.rs` (`ResourceView` — the builder's only input; the GPU-resource offsets it exposes)
- `src/services/scene3d/mod.rs` (`sites()`, `Inner`, the `with_avs_mutex` shape for lock replication)
- `src/core/signatures.rs` `Scene3dSites::texture_lookup` / `Scene3dTextureLookup` (OPTIONAL trio: `lookup`, `default_texture`, `spin`)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `src/services/scene3d/texture.rs`: `pub fn create_dynamic(width: u32, height: u32, format: u32, usage: u32) -> Option<u32>` = `sites.texture_create(w, h, 1, fmt, usage)` (`unsafe extern "C" fn(u32,u32,u32,u32,u32) -> u32`; 0 ⇒ `None`); `pub fn release(handle: u32)` = `sites.texture_release(handle)` (returned refcount ignored; `-1` = stale, log nothing). `pub fn lookup_gs_texture(hash: u32) -> Option<LookupOutcome>` using `Scene3dSites::texture_lookup` (`None` when the trio is absent): replicate the converter's spin exactly — `while fetch_add(spin, 1) != 0 { SwitchToThread() }`, call `lookup(hash)`, `spin.store(0)` — through an `AtomicU32` view of the flag address; return the `TextureData*` or the default. `pub fn default_texture() -> Option<*const u8>` reads `*default_texture`. GAME THREAD ONLY (both create and lookup take engine locks). Probe every pointer with `memory::is_readable` first.
2. `src/services/scene3d/render_item_layout.rs` (PURE, dependency-free, mountable by the host harness): the named offset consts of the item header (`ITEM_WORLD 0x00`, `ITEM_MODEL_PARAMS 0x40`, `ITEM_TINT 0x50`, `ITEM_RES 0x60`, `ITEM_TRAILING 0x68`, `ITEM_DRAW_RECORDS 0x70`, `ITEM_BONE_TEX 0x78`, `ITEM_BONES 0x80`, `ITEM_SCRATCH 0x88`, `ITEM_MATERIALS 0x98`, `ITEM_PALETTES 0xA0`, `ITEM_MODE 0xA8`, `ITEM_FLAGS 0xAC`, `ITEM_PASS_MASK 0xB0`, `ITEM_FRAME_STAMP 0xB4`, `ITEM_HEADER_SIZE 0xC8`), the draw-record layout (`REC_COLOR 0x00`, `REC_GPU_REC 0x10`, `REC_MATERIAL 0x18`, `REC_PALETTE 0x20`, `REC_FLAGS 0x28`, `REC_PASS_MASK 0x2C`, `REC_SIZE 0x30`, `REC_HIDDEN_BIT 0x0800_0000`, `GPU_REC_FLAG_MASK 0xF000_00FF`), the GPU-resource strides (`GPU_REC_SIZE 0x48`, `MATERIAL_SIZE 0x168`, `PALETTE_SIZE 200`, `MATRIX_SIZE 0x40`, `GPU_REC_MATERIAL_PTR 0x38`, `GPU_REC_PALETTE_PTR 0x30`, `GPU_REC_FLAGS 0x10`), material texture-slot layout (`MAT_TEX_INDEX(slot) = slot*2` u16, `MAT_TEX_MASK 0x14` u32, `MAT_TEX_PTR(slot) = 0xA8 + slot*0x18`), texture-table layout (`TEX_ENTRY_SIZE 0x10`, `TEX_ENTRY_HASH 0`, `TEX_ENTRY_PTR 8`, `RES_TEX_TABLE 0x80`, `RES_TEX_COUNT 0x2C`), mode bits (`MODE_PRIVATE_PALETTES 1`, `MODE_PRIVATE_BONES 2`, `MODE_BONE_TEXTURES 4`, `MODE_PRIVATE_MATERIALS 8`, `MODE_SCRATCH 0x10`; `MODE_RIGID = 0xB`, `MODE_SKINNED = 0xF`), `FRAME_STAMP_SEED 0xFFFF_FFFF`, `BONE_TEX_FORMAT 0x74`, `BONE_TEX_USAGE 0x2001`; `pub struct Counts { draw_records, bones, materials, palettes }`; `pub fn trailing_layout(c: &Counts) -> Trailing { records_off, bones_off, materials_off, palettes_off, total }` (header excluded: records ×0x30, bones ×0x40, materials ×0x168, palettes ×200, in that order, each 16-aligned); `pub fn record_material_index(gpu_mat_ptr, res_mats_base) -> Option<usize>` / `record_palette_index(...)` (the collector's `(ptr − base) / stride` with divisibility + `< count` checks); `pub fn model_params(bone_count) -> [f32; 4] = [bones as f32, 1.0, 0.0, 0.0]`. Doc comment cites RE §1.9 + §2.4 for why literal engine offsets are acceptable here.
3. `src/services/scene3d/render_item.rs` (engine-facing): `pub struct RenderItem { ptr: *mut u8, bone_tex: [u32; 2], counts: Counts }` with `ptr()`; `pub fn build(res: &ResourceView, pass_mask: u32) -> Result<RenderItem, BuildError>` allocating ONE `memory::alloc_zeroed(HEADER + trailing.total)` block and filling: world = identity (row-vector, `[12..15] = {0,0,0,1}`), `+0x40 = model_params`, `+0x50 = (1,1,1,1)`, `+0x60 = res`, `+0x68 = trailing base`, `+0x70` records (each: colour `(1,1,1,1)`, `gpuRec* = res.draw_records() + i*0x48`, `matCopy* = item_mats + record_material_index*0x168`, `palCopy* = item_pals + record_palette_index*200`, `flags = *(gpuRec+0x10) & 0xF00000FF`, `passmask = 0xFFFFFFFF`), `+0x78/+0x7C` = 0 (rigid) or two `create_dynamic(bone_count, 4, 0x74, 0x2001)` handles (skinned; either failing ⇒ release the other, `Err`), `+0x80` = memcpy of `res.bind()` (`bones × 0x40`), `+0x88 = 0`, `+0x98` = memcpy of the materials, `+0xA0` = memcpy of the palettes, `+0xA8 = MODE_RIGID | MODE_SKINNED`, `+0xAC = 0`, `+0xB0 = pass_mask`, `+0xB4 = FRAME_STAMP_SEED`. Then the texture re-resolve into the COPIES (§2.1): read the resource's texture table; for each entry whose `ptr` is null or `== default_texture()` or `*(u32*)ptr != entry.hash`, `lookup_gs_texture(hash)` and substitute when found; then for every material copy and every slot with `mask & (1<<s)`, `*(copy+0xA8+s*0x18) = table[copy.idx(s)].ptr` (index bounds-checked against `res+0x2C`). Record `TextureStats { total, resolved_at_load, re_resolved, still_default }` on the `RenderItem` for the caller's INFO. Every read of engine memory is `is_readable`-probed; any failure ⇒ `Err(BuildError::…)` with the block freed (`VirtualFree` — add `memory::free_alloc(ptr)` if no such helper exists) and any created textures released.
4. `pub fn free(item: RenderItem)`: `texture::release` both non-zero handles, then free the block. Doc: called ONLY from the node dtor (job-graph thread — `texture_release` takes only the registry's own spin lock; `VirtualFree` is thread-safe).
5. Accessors (unsafe, `visit`-side, NO engine calls): `set_world(&[f32;16])`, `set_tint([f32;4])`, `set_bones(&[[f32;16]])` (bounded by `counts.bones`), `set_hidden(bool)` (bit0 of `+0xAC`), `set_pass_mask(u32)`, `set_record_hidden(i, bool)` (bit 27 of `rec+0x28`).
6. Host tests (in `render_item_layout.rs`, run by `scripts/validate_background_dancers.sh` — add the file to `MODULE_NAMES/PATHS`): `trailing_layout` sizes/offsets for the footpanel counts `{1,1,1,1}` and a skinned `{5,33,3,5}` shape; `record_material_index` rejects non-divisible / out-of-range pointers; `model_params(1) == [1.0, 1.0, 0.0, 0.0]`; mode consts; `FRAME_STAMP_SEED != 0`. Plus a `#[cfg(test)]` synthetic-resource test INSIDE `render_item.rs` is NOT possible on the host (engine calls) — instead the builder's pure pieces (offset math, copy sizes) live in the layout module and are tested there.
7. Rust Quality Rules: narrow `unsafe`, no `unwrap`/`expect`/indexing in anything reachable from a hook; game-thread-only functions documented as such; no hardcoded engine offsets outside the layout consts (cite RE §1.9/§2.4).

## Dependencies
- Step 2 (`scene3d::{sites, model_registry::ResourceView}`) — done
- `Scene3dSites::texture_create` / `texture_release` (Step 1) and the OPTIONAL `texture_lookup` trio (added this step, sweep green on 5 builds)
- `core::memory::{alloc_zeroed, is_readable, read_*/write_*}`; a `free` counterpart for `VirtualAlloc`'d blocks (add if absent)

## Implementation Approach
1. Layout consts + pure helpers + tests in `render_item_layout.rs`; mount in the harness script; run it.
2. `texture.rs` wrappers (create/release/lookup/default) over `sites()`.
3. `render_item.rs` builder: allocate, fill header, copy arrays, wire records, re-resolve textures, stats; `free`; accessors.
4. `cargo check` → `cargo fmt` (whole crate) → `./build.sh`.

## Acceptance Criteria

1. **Layout math is exact**
   - Given the footpanel counts `{records 1, bones 1, materials 1, palettes 1}`
   - When `trailing_layout` runs
   - Then records start at 0, bones at 0x30 (16-aligned), materials at 0x70, palettes at 0x1E0 (0x70+0x168 = 0x1D8 → aligned 0x1E0), total 0x1E0+200 aligned; and the harness is green

2. **Builder shape**
   - Given a resident rigid resource on the cabinet (Step 3 harness)
   - When `build(res, 4)` runs
   - Then the INFO line reports `mode=0xB bones=1 records=1 textures total=1 …` and every draw record's `matCopy`/`palCopy` point inside the item's own block (asserted in code, WARN on violation)

3. **Fail-open**
   - Given `texture_lookup` is `None` or a lookup misses
   - When `build` runs
   - Then the copies keep the converter's pointers, `still_default` counts them, and no WARN fires from the builder itself (the caller logs one summary line)

4. **Gates**
   - Given the finished change
   - When `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`, `./build.sh`, `./scripts/validate_background_dancers.sh` run
   - Then all are clean/green

## Metadata
- **Complexity**: Medium
- **Labels**: scene3d, background-dancers, step-3, spike, engine-facing, render-item
- **Required Skills**: Rust FFI in this codebase, the engine ABI facts in RE §2
- **Generated By**: code-task-generator 2026-09-16
- **Source Plan**: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md`
- **Plan Step**: Step 3: Render item, scene node, root attach/destroy, camera slot 0, background hide — static footpanel (spike GO/NO-GO)
