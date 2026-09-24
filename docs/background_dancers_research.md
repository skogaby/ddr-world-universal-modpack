# Background Dancers — Implementation RE Notes (World 20260825)

Companion to `docs/background_dancers_feasibility.md` (what survived) and the planning research under
`.agents/planning/2026-09-16-enable-background-dancers/research/`. This file records the
implementation-time reverse engineering done while building the `scene3d` signature group and the
engine-facing service, step by step. Addresses are file-relative to `gamemdx.dll` at image base
`0x180000000`; World build **20260825** unless a build is named; "A3" = `gamemdx_20240402` (DDR A3 final).
No Ghidra symbols were added.

---

## 1. Step 1 — the `scene3d` signature group

### 1.1 SceneGraphManager / SceneGraph object graph (World)

```
DAT_1806f2d08  (global)  ──►  SceneGraphManager (mgr)
  mgr+0x00  SceneGraph*               ──►  graph (0x78 bytes, IS the root node)
  mgr+0x08  deferred-destroy vector begin   (std::vector<Node*>)
  mgr+0x10  deferred-destroy vector end
  mgr+0x18  deferred-destroy vector cap
  mgr+0x28  i32 avs mutex id   (Ordinal_14 = avs_mutex_create at ctor; > 0 ⇒ lock is live)
  mgr+0x2C  i32 lock nesting depth (INC/DEC inside the lock — debug guard, replicated by us)
  mgr+0x30  i32 second mutex id (unused by us), +0x34 depth
  mgr+0x38  f32 playback rate (ctor 1.0) — multiplies the frame dt fed to SceneGraph::update
  mgr+0x40  GraphUpdateJob vftable (job ctx = graph), mgr+0x50.. DebugRenderJob (job ctx = mgr)

graph+0x00  agcs::scene::SceneGraph::vftable   (node header: the graph is the ROOT node)
graph+0x08  u32 flags   — bit0 ENABLED (ctor 1; DPS onInitialize clears it, DPS step 5 sets it)
graph+0x0C  u32 pass mask (ctor 1 | 2)
graph+0x10  parent (0)     graph+0x18  FIRST CHILD (head-insertion point)     graph+0x20  next sibling (0)
graph+0x28  u32 misc flags — bit0 set by the manager ctor ⇒ SceneGraph::update SORTS the visible vector
graph+0x30  render-item list*  {begin, end, cap, ?, u32 drawRecordTotal @+0x20}  (cap 0x200, FUN_180275740)
graph+0x38  camera vector begin (2 × 0x3F8 `me::scene::camera::Camera`), +0x40 end, +0x48 cap
graph+0x58  visible-node vector begin (reserved 0x200 entries), +0x60 end, +0x68 cap
```

Sources: manager ctor `FUN_1800238a0`, graph ctor `FUN_180213dc0`, camera sizing `FUN_1802148f0` →
`FUN_180214ce0` (`+0x3F0 = -1`, `+0x3F4 = 1` active byte per camera), active-camera finder
`FUN_1800243a0`.

### 1.2 The manager lock (RE item c) — RESOLVED

`FUN_180024250` (destroy flush, `this` = mgr passed through from the tick `FUN_180023fb0`, itself the
DebugRenderJob's `run(this, mgr)`):

```
LEA RDI,[RCX+0x28]            ; mutex id field
MOV ECX,[RDI]; TEST; JLE      ; only when id > 0
CALL qword [rip+IAT 0x1802d88c8]   ; = LIBAVS-WIN64.DLL Ordinal_16  (avs_mutex_lock(id))
INC dword [RDI+4]             ; depth++
 … for each queued node: unlink children (FUN_1802159e0 clears links only), unlink from parent,
   (*node->vtable[0])(node, 1)                 ; OUR dtor, free = 1
 … vector cleared
DEC dword [RDI+4]
MOV ECX,[RDI]; TEST; JLE
CALL qword [rip+IAT 0x1802d88d8]   ; = LIBAVS-WIN64.DLL Ordinal_17  (avs_mutex_unlock(id))
```

The IAT slots resolve to **libavs-win64 ordinals 16 / 17** (the external-location table names them;
A3 imported the same pair by name `XCnbrep700000f/10`). Calling convention: `void fn(i32 mutex_id)` in
ECX. The DLL does NOT hard-code the ordinal: `derive_scene3d` decodes the two `FF 15 disp32` slots
from the flush body and publishes the SLOT addresses (`scene3d_mutex_lock_iat` / `_unlock_iat`); the
service reads the loader-patched pointer out of the slot at call time (exactly what the game calls).

A3's attach sequence (`FUN_18001c300`, model handle create) under the same lock:
`lock; mgr+0x2C++; node+0x10 = graph; node+0x20 = graph+0x18; graph+0x18 = node; mgr+0x2C--; unlock`
— i.e. HEAD insertion under the root, which is what `scene_graph::attach_under_root` replicates.

Thread note: the flush (and therefore OUR node dtor) runs inside the DebugRenderJob on a job-graph
thread, not necessarily the game thread. The dtor only frees mod-owned memory and calls
`texture_release` (which takes the texture registry's own spin lock), so that is fine — but do not add
anything thread-affine to the dtor.

### 1.3 SceneGraph::update pass protocol (RE item b) — RESOLVED

`FUN_180214570(graph, f32 dt)` (World) — reached from `GraphUpdateJob::run` `FUN_180024430`
(`if graph+8 & 1: dt = frameDelta × mgr+0x38; update(graph, dt)`):

| Pass | Gate on node | ctx passed to `visit(node, pass, ctx)` | Return 1 ⇒ |
|---|---|---|---|
| 2 (update) | `node+0xC & 0x10` | `&dt` (f32*) | recurse into children with the same gate |
| 3 (refresh) | `node+0xC & 0x4` | `{&DAT_18047ccf0, &DAT_1806f3320, u8 0}` | recurse |
| 4 (collect) | `node+0xC & 0x8` | **`&p` where `p = graph + 0x58`** — a pointer to a pointer to the visible vector `{begin, end, cap}` of `Node*` | recurse |
| 5 (cull) | `node+0xC != 0`, only for nodes ON the visible vector | `{Camera*, u32 camera+0x3F0}` per active camera | recurse (`FUN_180215a60`) |

When the gate bit is clear the "result" is `node+0x08 & 1` (enabled) — so a node with mask 0 but
enabled still recurses. Between pass 3 and 4 the item list (`graph+0x30`) and the visible vector are
reset. After pass 4: if `graph+0x28 & 1` (always, set by the manager ctor) the visible vector is
`std::sort`ed by **`*(i32*)(node + 0xE8)` ascending** (`FUN_180214ba0` → insertion sort
`FUN_180215830`); then for every visible node `FUN_180267190(itemList, *(node+0x78))` pushes the render
item (and adds `*(*(item+0x60)+0x24)` draw records to the list's counter).

**A3 `ModelNode::visit` (`FUN_18015b220`) return protocol, ported to our node:**

- pass 3: if enabled: `node+0x68 & 4` ⇒ `node+0x28 |= 1`; `(ctx.u8 != 0 || node+0x28 & 1)` ⇒
  `node+0x68 |= 8` (dirty); then `FUN_18015bf00` (TransformNode refresh) — **we return 0** (we run our
  own refresh in pass 4, mask bit 0x4 stays clear).
- pass 4: if enabled and `node+0x78 != 0`: `FUN_18015b300(node, ctx)` — copies world RT → `item+0x00`,
  tint → `item+0x50`, bones → `item+0x80` (`bone_count × 0x40`), pass mask → `item+0xB0`, hidden bit →
  `item+0xAC` bit0 (`= !(node+0x68 bit0)`), then **`push_back(**ctx, node)`** — inline MSVC vector push
  (grow via `FUN_180148640` when `end == cap`) — and returns **1** (recurse into children).
- pass 5: if item and `node+0x68 & 2`: frustum test of the bounding sphere at `node+0xC8` against
  `ctx.camera`; if OUTSIDE ⇒ `node+0xF4 &= ~ctx.mask; item+0xB0 = node+0xF4` (culls by clearing the
  item's pass-mask bits). Returns 0. **We return 0 without touching the mask ⇒ never culled** (fine for
  ≤ ~24 items).
- pass 6: returns `enabled`. Anything else: 0.

**Consequences for the mod-owned node (design §4.2.5, amended):**

1. `+0xE8` MUST hold a valid `i32` sort key (the engine's `std::sort` reads it from EVERY visible node —
   uninitialised memory would only mis-order, never crash, but the 0x100-byte node must cover it).
   A3 used it for the `:N` stage-part priority; we store the same (0 for dancers).
2. `visit(4)`: `let vec = **(ctx as *const *const *mut [*mut Node; 3])`; if `end < cap` ⇒ `*end = this;
   end += 1` else skip (fail-open: invisible this frame). Never call the engine's grow helper from the
   job thread. Return 0 (flat nodes — nothing to recurse into).
3. `visit(2)`/`visit(4)` are the ONLY writers of item memory.

Node vtable (A3 `agcs::scene::ModelNode::vftable` @ `0x180281fc0`): **exactly 2 slots** — `[0]`
`dtor(this, free: u8)`, `[1]` `u32 visit(this, pass: i32, ctx: *mut u8)`; COL pointer at `[-1]`. The
design's 8-slot table is harmless over-allocation; only slots 0/1 are ever called.

### 1.4 Render item destruction / texture release (RE item a) — RESOLVED

A3 `ModelNode` dtor `FUN_18015af10` → `FUN_18015af40` → `FUN_18001a980(item)`:
`FUN_1801761e0(item)` then gs-free the 0xC8 header. `FUN_1801761e0`:

- `item+0xA8 & 1` ⇒ for every private palette copy (`item+0xA0`, 200 B stride, `res+0x30` count):
  `FUN_180186ff0(palette)` — releases up to 16 texture handles flagged in the palette's u16 mask at
  `+4` (`FUN_18016efb0(handle)`) and a resource at `+0` (`FUN_18016fb60`). **For our copies these are
  plain `memcpy`s of the GPU resource's palettes; the A3 ctor's palette-copy helper (`FUN_1801762a0`)
  zeroes the mask/pointer fields (`*p = 0; *(u16*)(p+4) = 0; 16 × {p+8,+0xC,+0x10} = 0`) BEFORE
  `FUN_180187070(copy, src)` fills them** — so a palette copy's release only touches handles the copy
  itself registered. The DLL's builder must reproduce the same zero-then-copy shape (see Step 3 notes).
- `item+0xA8 & 4` ⇒ `FUN_180176870(item)`: for both `item+0x78` / `+0x7C` handles: registry slot =
  `(handle >> 17) * 0xA0 + registryBase`; if `*slot == handle` ⇒ `FUN_180166560(slot)` (refcount−−,
  free on 0, queues command `{7, 0|2, id, 4}`).
- then the trailing-array block `item+0x68` is gs-freed.

**World twins:**

| Role | World 20260825 | Notes |
|---|---|---|
| texture create `u32 (w, h, mips, fmt, usage)` | `FUN_180249c20` | called by the ArrowPalette factory `FUN_180024d00` as `(0x100, 0x20, 1, 0x15, 0x2002)` → AOB `texture_create_site` (CALL rel32 at match+29). Inline twin of A3's `FUN_1801765d0` body (registry alloc `FUN_18024b850`, sysmem for `usage & 0x2000`, command `{7, handle, 0xD}` queued) |
| texture release `u32 (handle)` | **`FUN_18024a170`** | byte-identical prologue on all five builds; slot = `(h>>17)*0xA0 + DAT_1806f1a58`, `*slot == h` ⇒ `FUN_180249570(slot)` (== A3 `FUN_180166560` line-for-line). Returns the new refcount, or `-1` when the handle is stale |
| texture addref | `FUN_18024a0d0` | same registry walk, `slot[0x1E]++` |
| by-pointer release | `FUN_1801f4c30(u32*)` | same body with `MOV EBX,[RCX]` — the AOB's `8B D9` (by value) excludes it |
| registry spin flag | `DAT_1806f1a60` | `XADD.LOCK` in create AND release — the identity gate (both decode the same global) |

### 1.5 Model registry (ResourceManager) — lookup must be OURS

`DAT_1806f2f68` → `ResourceManager`. Maps (all `std::map<u32 hash, …>` with an avs mutex each):

| Map object | mutex / depth | Node layout | Content |
|---|---|---|---|
| `+0x30` (head at `+0x38`) | `+0x130 / +0x134` | key u32 `@0x18`, kind ptr `@0x20`, **value = GPU model resource `@0x28`**, refcount i32 `@0x30`, isnil u8 `@0x39` | **MODELS** — register `FUN_1802030b0(kind, name)` (FNV-1 of the raw name), release `FUN_180203b60(hash)` (wraps `agcs::Resource::GpuResource<gs::ModelData>` for deferred free) |
| `+0x50/+0x58` | `+0x138` | isnil `@0x41`, ptr `@0x28`, copy `@0x30`, refcount `@0x38` | raw buffers (`FUN_180203220` register) |
| `+0x70/+0x78` | `+0x140` | same | raw buffers (`FUN_1802033b0`) |
| `+0x90/+0x98` | `+0x148` | same | raw buffers (`FUN_180203540`) |
| `+0xB0/+0xB8` | `+0x150` | isnil `@0x41`, value `@0x28` | **textures** — lookup `FUN_180202d50(hash)` (the feasibility doc's "`FUN_180202d50` family" is this, NOT the model map), register `FUN_1802036d0` |
| `+0xF0/+0xF8` | `+0x160` | isnil `@0x39` | shaders? (`FUN_180203890` register, `FUN_180202e30` lookup) |

**A3's model lookup-by-hash `FUN_180146590` has NO World twin** (its only callers were the deleted
scene layer); no function in World walks the `+0x30` map except register/release. Therefore
`model_registry::model_resource(name)` performs the red-black-tree walk ITSELF, read-only, under the
map's own avs mutex (`+0x130`, same ordinal-16/17 slots), exactly as `FUN_180203b60` does:

```
lock(mgr+0x130); mgr+0x134++
node = *(*(mgr+0x38) + 8)            ; head->parent = root
best = head
while !node.isnil(0x39): if node.key(0x18) < hash: node = node.right(0x10) else { best = node; node = node.left(0x00) }
found = best != head && !(hash < best.key)
value = found ? best.value(0x28) : null
mgr+0x134--; unlock
```

Every offset above is decoded from the release function's own instruction stream by
`derive_scene3d` (see 1.7) and the function is identity-gated by its `LEA RCX,[rip+vftable]` of
`.?AV?$GpuResource@VModelData@gs@@@Resource@agcs@@` (RTTI) — the AOB shape matches the three
raw-buffer releases too, so the gate selects exactly one of the three hits.

### 1.6 Background objects

- `FUN_1800320a0` (BgMovieActor readiness): `MOV RAX,[rip+DAT_1806f2d38]; MOV RBX,[RAX+0x58]` — the
  global and the `BackgroundFrame` offset. Called from `DancePlaySequence::onUpdate` case 3 as
  `CMP qword [rip+DAT_1806f2d38],0; JZ; CALL FUN_1800320a0; TEST AL,AL; JZ` — the identity chain (same
  global, same callee) selects that site out of the four `CMP/JZ/CALL/TEST/JZ` look-alikes.
- `FUN_18003e5b0` (bg_root create, via the unique `"bg_root"` string xref): pool walk
  `LEA R14,[rip+DAT_1806f9b20]; …; CALL [vt+0x138] (slot-free probe); …; ADD RDI,0x240; CMP EBX,0x400`,
  then `CALL FUN_180257af0(slot, pkg, "bg_root", 0)` (== the DLL's `cmovieclip_create` — identity gate),
  then `MOV RCX,[RBP+0x28] (frame); ADD RCX,0x140; CALL FUN_18010f960(&shared_ptr)` — the clip slot.

### 1.7 The `scene3d` group — signatures, derivations, published values

All-or-nothing: any miss un-resolves every `scene3d_*` name and logs one WARN naming the site.

| AOB (SIGNATURES) | Hits (all 5 builds incl. 20260915) | What it yields |
|---|---|---|
| `sg_enable_bit_site` `48 8B 05 ?? ?? ?? ?? 48 8B 08 83 49 08 01` | 3 (DPS step 5, MatchingDPS ×2) — **all must decode the same global** | `scene3d_scene_graph_manager` (RIP @+3), `scene3d_graph_flags_off` (imm8 @+12 = 0x08) |
| `sg_manager_tick` (`FUN_180023fb0` prologue through the proj-rebuild CALL) | 1 | flush fn (CALL @+6), active-camera fn (CALL @+11), view rebuild (CALL @+41; the CALL @+53 is the SAME view rebuild called again on the same camera), proj rebuild (CALL @+70, after the `CMP [RDI+0x2B3],0; JZ`), `scene3d_camera_view_off` (imm8 @+52 = 0x08), `scene3d_camera_proj_dirty_off` (disp32 @+60 = 0x2B3) |
| `sg_active_camera` (`FUN_1800243a0`) | 1; must == tick's CALL @+11 | global (must == above), `scene3d_graph_camera_vec_off` (imm8 @+30 = 0x38; end imm8 @+26 must be +8), `scene3d_camera_stride` (IMUL imm32 scanned forward = 0x3F8), `scene3d_camera_active_off` (`80 BC 08 disp32 00` = 0x3F4) |
| `sg_destroy_flush` (`FUN_180024250` prologue) | 1; must == tick's CALL @+6 | `scene3d_mgr_mutex_off` (imm8 @+36 = 0x28), `scene3d_mutex_lock_iat` (RIP disp32 @+50), `scene3d_mgr_depth_off` (mutex + imm8 @+56 = 0x2C), `scene3d_mgr_destroy_vec_off` (imm8 @+60 = 0x08; end @+64 must be +8), `scene3d_mutex_unlock_iat` (the single further `FF 15` in the body), the dtor call shape `48 8B 01 BA 01 00 00 00 FF 10` must be present |
| `sg_update_job_run` (`FUN_180024430`) | 1 | global (must == above), `scene3d_mgr_rate_off` (imm8 @+72 = 0x38), `scene3d_scene_graph_update` (CALL @+73) whose prologue must be `40 53 56 57 41 54 41 56 41 57 48 83 EC 48 48 8B 59 ??` → `scene3d_graph_root_child_off` (imm8 @+15 = 0x18); in its body: `48 8B 52 ?? E8` → `scene3d_node_item_off` (0x78); `41 F6 44 24 ?? 01 74` → `scene3d_graph_sort_flag_off` (0x28) followed by the sort-dispatcher CALL |
| `sg_insertion_sort` `48 8B 3B 48 8B 06 4C 8B C3 44 8B 8F ?? ?? ?? ?? 44 3B 88 ?? ?? ?? ?? 7D` | 1 (the comparator loop, entry+0x40); a CALL inside the sort dispatcher must target ≤ 0x100 bytes before it | `scene3d_node_sort_key_off` (disp32 @+12 = 0xE8, == the one @+19) |
| `model_registry_release` (`FUN_180203b60` prologue, offsets wildcarded) | 3 shape twins; **exactly one** LEAs the `GpuResource<gs::ModelData>` RTTI vtable | `scene3d_resource_manager` (RIP @+0x1B), `scene3d_rm_model_mutex_off` (disp32 @+0x22 = 0x130), `scene3d_rm_model_map_off` (imm8 @+0x3D = 0x30), `scene3d_rm_node_nil_off` (imm8 @+0x48 = 0x39, the second occurrence @+0x63 must agree), `scene3d_rm_node_key_off` (imm8 @+0x52 = 0x18), `scene3d_rm_node_right_off` (imm8 @+0x58 = 0x10), `scene3d_rm_node_value_off` (`48 8B 77 ??` = 0x28), `scene3d_rm_node_refcount_off` (`FF 4F ??` = 0x30); its lock IAT slot must equal the flush's |
| `texture_create_site` `BA 20 00 00 00 48 8B ?? 44 8D 4A F5 44 8D 42 E1 B9 00 01 00 00 C7 44 24 20 02 20 00 00 E8` | 1 | `scene3d_texture_create` (CALL @+29) |
| `texture_release` (`FUN_18024a170` prologue) | 1 | `scene3d_texture_release`; its `XADD.LOCK [rip]` spin flag (@+13) must equal the create body's |
| `bgmovie_readiness` (`FUN_1800320a0` entry — starts with the `40 53` REX PUSH so the match IS the entry the poll CALLs) | 1 | `scene3d_bgmovie_actor` (RIP @+9), `scene3d_bgframe_off` (imm8 @+16 = 0x58) |
| `bgmovie_ready_call_site` `48 83 3D ?? ?? ?? ?? 00 74 ?? E8 ?? ?? ?? ?? 84 C0 0F 84` | 4; ≥1 must decode (global, callee) == above | identity only |
| `bg_root_create_site` (pool walk in `FUN_18003e5b0`) | 1 | `scene3d_cmovieclip_pool` (RIP @+3), `scene3d_cmovieclip_pool_stride` (`48 81 C7 imm32` = 0x240), `scene3d_cmovieclip_pool_count` (`81 FB imm32` = 0x400), `scene3d_bg_clip_slot_off` (`48 81 C1 imm32` after the create CALL = 0x140); the CALL rel32 preceded by `LEA R8,[rip+"bg_root"]` must == `cmovieclip_create` |
| camera field block (from the tick's view/proj rebuild callees) | — | view rebuild (`FUN_180220b80`): first `80 B9 disp32 00` = `scene3d_cam_view_dirty_off` (0x2B0); `scene3d_cam_proj_req_off` = +1 (0x2B1), attested by a `CMP byte [reg+0x2B1],0` with ANY base (the body moves `this` to RDI) and the `MOV word [reg+0x2B2],0x0101` store; first `MOVSS xmm,[rcx+disp32]` = `scene3d_cam_eye_off` (0x268); first `SUBSS xmm,[rcx+disp32]` = `scene3d_cam_target_off` (0x274, must be eye+0xC); `scene3d_cam_up_off` = target+0xC (0x280, attested by `MOVSS xmm,[rdi+0x280]` in the body); proj rebuild (`FUN_1802376e0`): the first seven `MOVSS xmm,[rcx+disp32]` in order = `scene3d_cam_w_off` (0x28C), `_near_off` (0x2A8), `_far_off` (0x2AC), `_l_off` (0x290), `_r_off` (0x294), `_b_off` (0x298), `_t_off` (0x29C); l/r/b/t contiguous, near+4 == far |

Publishing convention: non-address results go through `SignatureStore::publish_value` (boot log
`name (derived) = 0x…`); the bundle getter `SignatureStore::scene3d_sites() -> Option<Scene3dSites>`
returns `None` unless every field resolved.

Camera write protocol (from `FUN_180220b80` / `FUN_1802376e0`): write eye/target/up + w(=1.0)/l/r/b/t
/near/far, then set `+0x2B0 = 1` (view dirty) and `+0x2B1 = 1` (projection dirty). The next tick's view
rebuild consumes both (`*(u16*)(cam+0x2B0) = 0`), writes the view matrix (`+0x08`) and the inverse
(`+0x48`), and sets `+0x2B2/+0x2B3 = 1`; the tick then sees `+0x2B3` and runs the projection rebuild
(`+0x1C8`: `m00 = 2w/(r−l)`, `m11 = 2w/(t−b)`, `m20 = (r+l)/(r−l)`, `m21 = (t+b)/(t−b)`,
`m22 = −far/(far−near)`, `m23 = −1`, `m32 = −far·near/(far−near)`, `m33 = 0`) and clears `+0x2B3`.
`+0x2A0/+0x2A4` (r−l, t−b) from the design are NOT read by any World function — dropped.

### 1.8 Cross-build sweep

Recorded after the harness run (Step 1 task 2): `./scripts/validate_signatures.sh ~/Desktop/ddr_modules`
+ `scripts/sig_harness/shape_diff.py` over the consumers `FUN_180263430` (opaque collector),
`FUN_180261780` (bone-texture upload), `FUN_180262670` (draw), `FUN_180214570` (update),
`FUN_180023fb0` (tick), `FUN_180024250` (flush), `FUN_180220b80` / `FUN_1802376e0` (camera rebuilds),
`FUN_18003e5b0` (bg_root create). See §1.9 for the results table.

### 1.9 Sweep results (2026-09-16)

Commands (all from the repo root; the sweep directory is `~/Desktop/ddr_modules` with gamemdx
20250805 / 20260224 / 20260721 / 20260825 / 20260915 + one `libafp-win64.dll`):

```
./scripts/validate_signatures.sh ~/Desktop/ddr_modules --json <tmp>/sweep.json
python3 scripts/sig_harness/shape_diff.py --json <tmp>/sweep.json --dir ~/Desktop/ddr_modules \
    --window 0x400 --names sg_enable_bit_site,sg_manager_tick,sg_active_camera,sg_destroy_flush,\
sg_update_job_run,sg_insertion_sort,model_registry_release,texture_create_site,texture_release,\
bgmovie_readiness,bgmovie_ready_call_site,bg_root_create_site
# + a throwaway capstone script (same normaliser as shape_diff.py) over the NON-AOB'd consumers,
#   located per build by walking from the sweep's anchors (see the table below)
```

**Sweep:** `RESULT: ALL GREEN`. All 12 raw AOBs hit exactly once (`sg_enable_bit_site` 3 hits all
agreeing, `model_registry_release` 3 shape hits with exactly one naming the ModelData vtable,
`bgmovie_ready_call_site` 4 hits with ≥1 identity match) and all 46 `scene3d_*` names publish on every
build. **Every derived VALUE is identical across the five builds** (graph 0x08/0x18/0x28/0x38, node
0x78/0xE8, mgr 0x08/0x28/0x2C/0x38, camera 0x3F8/0x3F4/0x08/0x2B3 + fields 0x268/0x274/0x280/0x28C/
0x290/0x294/0x298/0x29C/0x2A8/0x2AC/0x2B0/0x2B1, registry 0x130/0x30/0x39/0x18/0x10/0x28/0x30,
background 0x58/0x140/0x240/0x400); only the addresses move.

**AOB shape diff (window 0x400, ref 20260721)** — first divergence vs the bytes the derivation reads:

| AOB | reads ≤ match+ | diverges (20250805 / 20260224 / 20260825 / 20260915) | verdict |
|---|---|---|---|
| `sg_enable_bit_site` | +13 | +0x35 / +0x35 / = / = | OK (past every read; the divergence is the DPS step-5 tail) |
| `sg_manager_tick` | +74 | = / = / = / = | OK |
| `sg_active_camera` | +0x60 (forward scans) | +0x190 / +0x190 / +0x190 / = | OK (the function is 0x6C bytes; +0x190 is the next function) |
| `sg_destroy_flush` | +0x1E5 (body scans to match+65+0x180) | +0x2E0 / +0x2E0 / +0x2E0 / = | OK (function ends at +0x144) |
| `sg_update_job_run` | +78 | +0x100 / +0x100 / +0x100 / = | OK (function is 0x55 bytes) |
| `sg_insertion_sort` | +23 | = / = / = / = | OK |
| `model_registry_release` | +0xC0 (tail scans) | = / = / = / = | OK |
| `texture_create_site` | +33 | = / = / = / = | OK |
| `texture_release` | +21 | = / = / = / = | OK |
| `bgmovie_readiness` | +17 | +0x180 / +0x180 / +0x180 / +0x180 | OK (function is 0x7F bytes) |
| `bgmovie_ready_call_site` | +15 | +0x17 / +0x17 / = / = | OK (JZ rel32 target differs; nothing past +15 is read) |
| `bg_root_create_site` | +0x1A0 (forward scans) | +0x384 / +0x384 / = / = | OK |

**Non-AOB engine consumers** (the functions the feature hands objects TO — located per build by
walking from the sweep anchors: tick CALL@+41/+70 → camera rebuilds; `scene3d_scene_graph_update`;
the `bg_root` creator = CC-padded entry before the pool walk; the pass driver by its unique prologue
`4D 85 C0 0F 84 … 48 8D 68 A1 48 81 EC F0 00 00 00` → CALL[2] dispatch → CALL[1] opaque collector /
CALL[4] transparent collector, driver CALL[3] bone-texture upload, driver last CALL draw; the item push
= the CALL after `48 8B 52 78` in the update). **Every one is byte-shape-identical through its whole
body on all five builds, and the set of `[reg + disp]` memory operands each touches is identical:**

| Function (World 20260825) | 20250805 | 20260224 | 20260721 | 20260825 | 20260915 | window | shape | item/node/camera fields read |
|---|---|---|---|---|---|---|---|---|
| `SceneGraph::update` `FUN_180214570` | 1FBFB0 | 200360 | 214510 | 214570 | 2142C0 | 0x380 | = | node `+0x08/+0x0C/+0x18/+0x20/+0x78`, graph `+0x18/+0x28/+0x30/+0x38/+0x40/+0x58/+0x60`, cam `+0x3F0/+0x3F4` |
| manager tick `FUN_180023fb0` | 23AF0 | 23940 | 23FB0 | 23FB0 | 243F0 | 0x200 | = | cam `+0x08/+0x1C8/+0x2B3`, pass `+0x58/+0x98` |
| destroy flush `FUN_180024250` | 23D90 | 23BE0 | 24250 | 24250 | 24690 | 0x150 | = | node `+0x10/+0x18/+0x20`, mgr `+0x08/+0x10/+0x28/+0x2C` |
| camera view rebuild `FUN_180220b80` | 22F700 | 20C9A0 | 2301E0 | 220B80 | 247920 | 0x700 | = | cam `+0x08..+0x48`, `+0x268..+0x288`, `+0x2B0/+0x2B1/+0x2B2/+0x2B8` |
| camera proj rebuild `FUN_1802376e0` | 246320 | 223730 | 246D40 | 2376E0 | 25E620 | 0x1F0 | = | cam `+0x1C8..+0x204`, `+0x28C..+0x29C`, `+0x2A8/+0x2AC` |
| `bg_root` creator `FUN_18003e5b0` | 3E380 | 3DCA0 | 3DFA0 | 3E5B0 | 3EB10 | 0x2A0 | = | frame `+0x140`, clip vt `+0x138/+0xE0/+0xE8` |
| pass driver `FUN_1802606d0` | 217370 | 243F80 | 258300 | 2606D0 | 21F7C0 | 0x290 | = | list `+0x20`, pass `+0x28/+0x2C/+0x158/+0x218` |
| collect dispatch `FUN_180261430` | 2180C0 | 244CE0 | 259050 | 261430 | 220510 | 0x340 | = | — |
| bone-texture upload `FUN_180261780` | 218410 | 245030 | 2593A0 | 261780 | 220860 | 0x700 | = | item `+0x60/+0x80/+0x88/+0xA8/+0xB4`, res `+0x20/+0x50` |
| draw `FUN_180262670` | 219300 | 245F20 | 25A290 | 262670 | 221750 | 0x800 | = | item `+0x40/+0x48/+0x50/+0x60/+0x70/+0x78/+0x80/+0xAC/+0xB0`, rec `+0x10/+0x18/+0x20/+0x28/+0x2C` |
| opaque collector `FUN_180263430` | 21A0C0 | 246CE0 | 25B050 | 263430 | 222510 | 0x500 | = | item `+0x00/+0x50/+0x60/+0x70/+0x80/+0xAC/+0xB0`, rec `+0x10..+0x2C`, res `+0x24/+0x48/+0x50` |
| transparent collector `FUN_180262f00` | 219B90 | 2467B0 | 25AB20 | 262F00 | 221FE0 | 0x500 | = | as opaque + rec colour alpha `+0x0C` |
| item push `FUN_180267190` | 21DF00 | 24AA10 | 25ED50 | 267190 | 226350 | 0xA0 | = | list `+0x10/+0x20`, item `+0x60` → res `+0x24` |

**Verdict:** the render-item layout (design §5.2), the node protocol (§4.2.5 + the `+0xE8` sort
key from §1.3) and the camera slot (§5.3) are read identically by every World build the DLL supports;
the `scene3d` group is safe to build on. No design amendment needed. The one implementation-side
correction from this step: the node MUST carry a valid `i32` sort key at `+0xE8` (engine
`std::sort`), and `visit(4)` pushes onto `**ctx` (see §1.3).

---

## 2. Step 3 — the render item the engine accepts (RE before the GO/NO-GO spike)

Three questions had to be answered before hand-building an item (plan Step 3, design §5.2): where
material textures come from, whether the palette/material copies need A3's addref shape, and what the
frame stamp must be seeded with. All three were answered from the World consumers themselves.

### 2.1 Material textures — World resolves them at CONVERSION time; A3's re-resolve has no twin

A3's `ModelNode::setModel` (`FUN_18015afb0`) called `FUN_180175b80(res)` before the item ctor: for every
entry of the resource's texture table (`res+0x80`, stride 0x10, count `res+0x2C`) a registry lookup by the
entry's hash (`FUN_180186100`, default texture `DAT_1802ef2e8` on miss) stored `TextureData*` at
`entry+8`; then every material (`res+0x78`, 0x168 stride, count `res+0x28`) received, for each of its 8
slots whose mask bit (`u32 mat+0x14`) is set, `TextureData*` at `mat+0xA8 + slot*0x18` from the table
index `u16 mat+slot*2`.

World does the SAME resolution inside the model converter (`FUN_180272f80`, reached synchronously from
the `ModelFileCallback` → `FUN_1802030b0` register → `FUN_18026f140` bind):

| Converter helper (World 20260825) | What it writes |
|---|---|
| `FUN_180273e20(res, ctx)` — texture table | per entry: `entry+0 = gs_hash(name)` (`(*DAT_1806f2040)(name, len)` — the gs hasher, NOT the ResourceManager's), `entry+8 = FUN_18026f9e0(hash)` under the gs texture-registry spin flag `DAT_1806f2090`, **default texture `DAT_1806f3298` on miss**; `res+0x2C = count` |
| `FUN_180274070(res, &mats, ctx)` — materials | per material: resets the 0x158-byte sub-object at `mat+0x10` (`FUN_180267270`), shader hash `mat+0x10`, shader object `mat+0x20`, u16 param count `mat+0x18`, params from `mat+0x28`; per texture slot `s` (from the semantic, `FUN_180273f50`): `u16 mat+s*2 = texIdx`, **`mat+0xA8+s*0x18 = table[texIdx].ptr`**, `mat+0xB0/+0xB4 = 1.0`, `mat+0xB8/+0xBC = 0`, `mat+0x14 |= 1<<s` |

`FUN_18026f9e0` (the gs registry lookup: sorted `std::vector<TextureData*>` at `*DAT_1806f3290` — begin
`+0`, end `+8`, sorted-flag byte `+0x80`, lazily `std::sort`ed on the first lookup — binary-searched on
`*(u32*)TextureData == hash`) has EXACTLY ONE caller in World: the converter's table fill. **A3's
setModel-time re-resolve therefore has no World twin**, and the registry is a pure lookup (no
"find-or-create placeholder").

Consequence: a material's texture is whatever the gs registry held at the instant its `.model` was
converted. `mapset_boom00.arc` lists every part's `.model` BEFORE that part's `.dds` members, and the
FileManager converts a model synchronously inside its member callback, so the DDS may or may not be
registered yet — if it is not, the material holds the DEFAULT texture forever (no re-resolve anywhere).
Whether the FileManager's member dispatch order (the sorted pending vector in `FUN_1801fe370`) saves
us is NOT knowable statically; the spike measures it.

**What the draw actually reads** (`FUN_180262670` → material bind `FUN_1801f63f0(?, pass, rec, item,
flags)` → `FUN_18026cce0(pass, mat+0x10, *(pass+0x168))`): for each masked slot `FUN_18026cc00(pass,
s, mat+0xA8+s*0x18)` binds texture id `*(TextureData+4)` (null pointer ⇒ id 0); then the shader program
`*(*(mat+0x20)+8)[stage]` (command 0xE), then the params (`mat+0x28`, count `mat+0x18`) as VS c24.. /
PS c3... `TextureData` = `{u32 gs_hash @0, u32 texture handle @4, …}`.

**DLL mitigation (Step 3, fail-open):** at item build the DLL re-resolves INTO ITS OWN material
copies (never the resource): for each texture-table entry whose pointer is null or whose
`*(u32*)ptr != entry.hash` (⇒ default/unresolved), call the World lookup under the World spin flag and
substitute the result when found; then rewrite each copy's masked slots from the corrected table. New
signature `texture_lookup_site` (the table fill's tail — see §2.5) yields the lookup fn, the default
texture global and the spin flag; when it is missing the copies keep the converter's pointers. The
build logs `textures: total/resolved-at-load/re-resolved/still-default` per model so the cabinet
answers the ordering question with one line.

### 2.2 Palette and material copies — plain `memcpy`, no addref, no release

The 200-byte "palette" (`res+0x88`, one per draw record) is the **vertex-stream binding block**:
`u32 vertex-declaration handle @0`, `u16 slot mask @4`, 16 × `{u32 vb handle, u32, u32} @8`. The draw
binds it read-only (`FUN_18026ce00`: command 0xD SetVertexDeclaration, then 0xB SetStreamSource per
masked slot). The 0x168 material is read-only too (§2.1). A3's ctor helpers (`FUN_1801762a0` palette:
zero mask/handle fields then `FUN_180187070(copy, src)` which **addrefs** the declaration
(`FUN_18016fac0`) and every masked stream handle (`FUN_180187110` → `FUN_18016ef00`);
`FUN_1801763f0` material: `FUN_18017d5e0` reset + `FUN_18017d7f0` field copy) exist only because A3's
item dtor (`FUN_1801761e0` → `FUN_180186ff0`) RELEASED those handles. The DLL's items never release
them — the resource owns every handle and an item's lifetime is strictly inside its arc's lifetime
(arcs are freed only after every node dtor ran) — so both copies are plain `memcpy`s and design
amendment 7 (§1.4's zero-then-copy) is moot. The engine never frees an item, so no engine path can
ever run a release over our copies.

### 2.3 Frame stamp `item+0xB4` — seed `0xFFFFFFFF`; a ZERO seed hangs the render thread

Upload `FUN_180261780(&{u32 frame_id, u32 parity}, passItem)` protocol:

```
if passItem.done == 0:
    stamp = item+0xB4
    if stamp != frame_id:
        loop: if stamp == 0            → return 0      ("claimed by another thread — not done yet")
              if CAS(item+0xB4, stamp → 0) → upload; item+0xB4 = frame_id; return 1
              stamp = item+0xB4; until stamp == frame_id
    passItem.done = 1
return 1                                                 (stamp == frame_id ⇒ done)
```

and the pass driver (`FUN_1802606d0`) **loops over the item list until every upload returns 1** — an
item whose stamp is 0 with nobody uploading it spins the render thread forever. `frame_id` =
`DAT_180461778`, World's twin of A3's `DAT_1802db148`: incremented once per frame in `FUN_18026af10`
with `-1 → 1` (it is never 0). Seeding with `0xFFFFFFFF` needs no derivation and collides only on the
wrap frame (`stamp == frame_id` ⇒ "done", one frame of stale bone texture, then normal). Rigid items
(both `+0x78/+0x7C` handles 0) skip the upload body but still get stamped.

### 2.4 Item fields as the World consumers read them (confirms design §5.2)

| Reader | Item fields | Notes |
|---|---|---|
| opaque collector `FUN_180263430` | `+0xAC & 1` hidden ⇒ skip; `+0xB0 & pass.filter` (`*(pass+0x10)`: 0x01/0x56/0x10/0x46 for DISTANTVIEW/OPACITY/LOWPRIO_TRANS/TRANS — mask 2 and 4 both hit OPACITY+TRANS, 0x10 hits LOWPRIO_TRANS+OPACITY); rigid (`res+0x1C bit0 == 0`) ⇒ `world = bone[0](+0x80) · invBind[0](res+0x50) · item.world(+0x00)`; per record (0x30): skip `rec+0x28 & 0x8000000`, `rec+0x2C & filter`, per-pass material filter callback, then the clip-space AABB test `FUN_180260970` on `gpuRec+0..+0xC` (centre, radius) — **a camera that does not frame the model culls everything silently**; the sorted entry gets `color = rec.color(+0) × tint(+0x50)` | `+0xA8` NOT read |
| bone-texture upload `FUN_180261780` | `+0xB4` (§2.3), `+0x78/+0x7C` handle by parity, `+0x60`→`res+0x20/+0x50`, `+0x80` bones, `+0xA8 & 0x10` (scratch mode — must be CLEAR), `+0x88` only in that mode | the ONLY engine read of `+0xA8` |
| draw `FUN_180262670` | `+0x40` → VS c22 / PS c2, `+0x78+parity*4` → stage-3 texture when `gpuRec+0x10 & 0x100`; via the record: `rec+0x10` gpuRec (`+0x16` prim type, `+0x18` stream count, `+0x20` streams ×0xC, `+0x28` index buffer), `rec+0x18` material copy, `rec+0x20` palette copy | sorted-entry colour → VS c23 |
| item push `FUN_180267190` | `*(item+0x60)+0x24` (draw-record total); refuses when the 0x200-entry list is full | — |

Rows of the world matrix are row-vectors (A3's refresh `FUN_18015b300` writes rotation rows 0–2 and
`{tx, ty, tz, 1}` as row 3). `ModelParameters` (`+0x40`) = `{bone_count, 1.0, 0, 0}` for BOTH rigid and
skinned items — the A3 ctor sets `.z = bone_count` only in the `0x10` scratch mode (design §5.2's
"bone_count-or-0" resolves to 0). A3's mode bits (`1` private palettes, `2` private bones, `4` bone
textures, `8` private materials, `0x10` scratch) are otherwise only ever read by A3's own ctor/dtor;
the DLL keeps their meaning for its builder — `0xB` rigid, `0xF` skinned (A3's `setModel` passed 9 and
shared the resource's bind array as `+0x80`, then its refresh `memcpy`'d the pose INTO that shared
array; the DLL gives every item a private bone array instead, which is what per-instance poses need).

Footpanel geometry for the spike camera (`ktmdl_dump.py` on `gm_boom00_footpanel.model`): 1 bone
(bind = identity), 1 material (`lambert2`, shader `mdl_bg_constant`, alpha-test, OPACITY), 1 texture
(`footpanel` = `footPanel.dds`), 1 mesh, sphere centre `(0, 0.527, −0.13)` r `0.99`, bbox
`x ±0.534, y 0.001..1.053, z −0.778..0.518`. The design camera (eye `(0, 1.2, 4)`, target `(0, 0.8, 0)`,
half-tangent 0.5) sees ±2 m × ±1.1 m at the model, so the whole panel is in frame.

### 2.5 New signature: `texture_lookup_site`

`8B CF E8 ?? ?? ?? ?? 48 85 C0 48 0F 44 05 ?? ?? ?? ?? 33 C9 87 0D ?? ?? ?? ?? 48 89 43 08 FF C6 48 83 C5 50 48 83 C3 10`
— the texture-table fill's tail (`FUN_180273e20+0xC7` on 20260825). Exactly 1 hit on all five builds
(20250805 `0x18022ac37`, 20260224 `0x1802577b7`, 20260721 `0x18026bab7`, 20260825 `0x180273ee7`,
20260915 `0x180233257`), byte-shape identical from the `LOCK XADD` at match−0x23 through the loop
tail. Yields `scene3d_texture_lookup` (CALL rel32 @+2 — `TextureData* fn(u32 gs_hash)`, null on miss),
`scene3d_texture_default` (CMOVZ RIP disp32 @+14, instruction ends @+18 — the global holding the default
`TextureData*`), `scene3d_texture_spin` (XCHG `87 0D disp32` @+20, disp @+22, ends @+26 — the u32 spin
flag). Identity gates: the two `LOCK XADD dword [rip+disp32]` at match−0x23 and match−0x0C must decode
the same global as the XCHG; the CALL target's prologue must be
`40 53 48 83 EC 20 48 8B 05 ?? ?? ?? ?? 8B D9 80 B8 80 00 00 00 00 75 2A 48 8B 50 08 48 8B 08` (the
sorted-flag check at `+0x80` of the registry object). Spin protocol (replicated exactly):
`while fetch_add(spin, 1) != 0 { SwitchToThread }` … `spin.store(0)`.

### 2.6 What Step 3 shipped against these facts (implementation notes)

- `services/scene3d/render_item_layout.rs` (pure, host-tested) carries every item / draw-record /
  material-slot / texture-table offset above as named consts; `render_item.rs` builds ONE
  `alloc_zeroed` block (0xC8 header + 16-aligned trailing arrays: records, bones, materials, palettes),
  mode `0xB` rigid / `0xF` skinned, stamp `0xFFFFFFFF`, `ModelParameters {bones, 1, 0, 0}`, identity
  world, tint 1, bones = bind copy, material/palette copies = `memcpy`, then the §2.1 re-resolve into the
  material COPIES (stats `total / resolved-at-load / re-resolved / still-default` logged per build).
- `services/scene3d/node_layout.rs` (pure, `offset_of!`-pinned) + `node.rs`: the 0x100 node with the
  engine-read fields at `+0x08/+0x0C/+0x10/+0x18/+0x20/+0x78/+0xE8` (cross-checked against the derived
  `scene3d_node_item_off` / `scene3d_node_sort_key_off` at `new_node`), a process-lifetime 2-slot
  vtable image (`[-1]` null COL), `visit` = pass-4 refresh + `**ctx` push (never grows), 0 otherwise;
  **the dtor frees the ITEM but never the NODE block** — the game-thread lifecycle polls `destroyed`
  and a block freed under that poll would be dead memory; it frees the node itself afterwards.
- `services/scene3d/scene_graph.rs`: attach (head insertion) / `queue_destroy` (no grow) under the
  manager lock via `with_avs_mutex`; `write_camera0` (fields + `+0x2B0`/`+0x2B1` = 1);
  `graph_stats()` / `item_listed()` diagnostics over `graph+0x58` (visible vector) and `*(graph+0x30)`
  (item list) — the two per-frame vectors `SceneGraph::update` rebuilds (RE §1.1/§1.9).
- **Teardown ordering guard** (spike + future lifecycle): a node is first DISABLED (skipped by every
  pass ⇒ dropped from the item list at the next update), the destroy is queued only after the engine's
  item list has been observed WITHOUT the item for 2 consecutive frames, and the arc is freed only after
  our dtor ran (5 s cap ⇒ leak + WARN). This makes the free independent of the intra-frame order of
  tick (flush) / update / passes, and refuses to free while the graph is DISABLED with a stale list
  (the DPS clears `graph+0x08` bit0 at its `onInitialize` and re-sets it at step 5 — between the two,
  `SceneGraph::update` does not run and the item list keeps its last content).
- `mods/background_dancers/background_hide.rs`: the §1.6 chain `bgmovie_actor → +bgframe_off →
  +bg_clip_slot_off`, pool-slot validation, `layer_set_color_raw(*(clip+0x08), 1,1,1,0)` per frame,
  alpha 1 restored once on disarm, one WARN after 60 consecutive misses.

### 2.7 Cabinet result — GO (2026-09-16, build 20260915 / CrossOver)

The engine's `MODEL:*` passes drew the hand-built item: the boom00 foot panel rendered behind the lane
in the expected spot on three songs, survived quick-restart (node persists across the 28→27→28 scene
hop — the new DPS's `onInitialize` disables the graph, step 5 re-enables it, the item is collected
again) and quick-fail, 2D background hidden (`bg-hide: bg_root layer 0x… alpha 0` / `alpha restored`
each song). Timeline per song: arc accepted → all six models resident at 22–102 ms → item built +
node attached + camera written at the same tick → `first frame after attach: graph enabled=false`
(the DPS has not reached step 5 — READY banner) → `item collected by SceneGraph::update ~4.9 s after
attach -- enabled=true visible-nodes=1 items=1 records=1`. **Option A is validated.**

Two facts the deploy added:

1. **Texture registration lags model conversion by up to a few hundred ms on a COLD arc.** Song 1:
   `textures total=1 load=0 re=0 default=1` — the footpanel rendered in the engine's default texture
   (magenta) for the whole song; the build-time re-resolve found nothing because `footPanel.dds`
   (1.3 MB, the arc's second-to-last member) had not been registered yet. Songs 2–3: `load=0 re=1
   default=0` — the converter STILL resolved to the default (arc order), but the DDS was resident from
   the previous load (texture release is deferred/refcounted) and the build-time re-resolve fixed the
   copies. So §2.1's mitigation is necessary AND insufficient on a cold load: the spike now retries
   `render_item::retry_texture_resolve` per frame on the attached item until `still_default == 0`
   (aligned 8-byte pointer stores into OUR material copies — safe under the concurrent draw) and logs
   the latency; **Step 7's residency gate must be "model resident AND `texture_readiness(res).
   still_default == 0`"** (`render_item::texture_readiness` — a table walk + registry lookups, no
   item needed), with a timeout that builds anyway.
2. **World's SceneGraphManager destroy vector has ZERO capacity forever.** The ctor (`FUN_1800238a0`)
   zeroes `{begin, end, cap}`, the flush only drains (`end = begin`), and no World function pushes
   (A3's pushers were the deleted scene layer) — so a no-grow push can never succeed (`queue_destroy
   refused 60 frames in a row` every song; the Detaching phase then never timed out and the next
   window parked the node). Fix: `scene_graph::queue_destroy` installs a mod-owned 256-entry
   process-lifetime buffer into the EMPTY vector under the manager lock on its first call (the engine
   never grows the storage, so the flush's `begin..end` semantics hold), and the Detaching phase times
   out into "leak the disabled node, free the arc" if a push still fails. **CORRECTION (2026-09-16,
   §3.4): the manager SHUTDOWN `FUN_180023b30` DOES free a non-null `begin` — through the engine
   allocation header 0x20 bytes in front of it — so the buffer must carry that header** (it does now).
   Cabinet-observed since Step 4: `destroy queued` → `destroyed by the engine flush` → `arcs freed`.

Everything else — collector filter, clip-space cull with the design camera, material bind, draw,
`SceneGraph::update` enable gating — behaved exactly as the RE predicted. No crash, no engine WARN.

### 2.8 Bone textures are `create(4, bone_count)` — one bone per ROW (Step 4 deploy #1, 2026-09-16)

The first skinned deploy (`pl_emi00`, 33 bones) produced magenta flashing across the screen, no
dancer, and an ACCESS_VIOLATION at a wild address when the teardown freed things (`W:DDR: EXCEPTION
CATCH` then `0x00006FFFFFF8F6D0`). Cause: the bone textures were created as `(bone_count, 4)` per
design §4.2.3 / §5.2, but the upload `FUN_180261780` (§2.3) writes bone `i` at `data + i·pitch` —
3 `A32B32G32R32F` texels (48 B) per bone, then advances by the row PITCH — i.e. **one bone per row**.
A3's own creator `FUN_1801765d0` sets registry `+0xC = 4`, `+0xE = bones`, and World's
`create(w, h, mips, fmt, usage)` → `FUN_180249640(reg, 0, w, h, …)` stores `w` at `+0xC` / `h` at `+0xE`.
So the correct call is **`create(4, bone_count, 1, 0x74, 0x2001)`**: pitch 64 B, `bone_count` rows,
`FUN_18024ad60` sizes the sysmem staging buffer (`usage & 0x2000`) to exactly `4·bones·16` bytes. The
swapped shape (33 wide × 4 high: pitch 528, 2112-byte staging buffer) had the upload write 33 rows =
17 424 bytes through the buffer every frame — heap corruption of neighbouring gs allocations
(other textures' staging ⇒ the magenta flashes), garbage skinning (no dancer), and a corrupted heap
block freed at teardown (the crash). `render_item_layout::BONE_TEX_WIDTH = 4` replaces the former
`BONE_TEX_HEIGHT`; the skinning VS addresses rows by `(bone + 0.5) / ModelParameters.x` (the bone
count), consistent with height = bones. Design §4.2.3 / §5.2 / Step 4 text are wrong on this point.

Also from the same deploy: the Step 3 teardown fixes are confirmed — `installed a 256-entry
destroy-vector buffer` → `3 destroy(s) queued 18 ms after window exit` → `all 3 node(s) destroyed by
the engine flush 30 ms after window exit` → `3 arc handle(s) freed … created=2 released=2 stale=0`
(the crash came afterwards, on the game's own side, from the corrupted heap); the texture-readiness
gate held the builds until every DDS registered (`load=0 re=N default=0` for all three at 80–119 ms);
`items=3 records=4` were collected once the DPS enabled the graph. The shadow quad at y = 0.02 is
inside the opaque foot panel (pad surface at y ≈ 0.109) and z-tested away — the spike now stands Emi
on the pad surface with the shadow in between.

### 2.9 Step 4 result — PASS (deploy #2, 2026-09-16, CrossOver)

With `create(4, bone_count)` Emi rendered in bind pose (T-pose, body texture, no face — the face is a
separate part arc attached in Step 8) standing on the boom00 foot panel with the shadow quad under her,
three songs in a row, no flashing, clean quick-exit. Per song: three builds 76–128 ms after the load
(readiness gate held every one until its DDS registered: `load=0 re=N default=0`), `items=3 records=4`
collected at DPS step 5 (~4.9 s), teardown `3 destroy(s) queued 7–10 ms` → `all 3 node(s) destroyed
by the engine flush 15–20 ms after window exit` → arcs freed, `scene3d textures: created=2k released=2k
stale=0` (k = 1, 2, 3). The skinned pipeline — two bone textures, the engine's per-frame upload, the
skinning VS, stage-3 binding — works end to end on the hand-built item. Windows not yet run (no
platform-specific code in the path; the design's §7.3 exit criterion still asks for it).

## 3. Step 5 — the pure `core/anm` format layer (host-validated 2026-09-16)

`src/core/anm/{mod,anm,sample,pose,camera,b2it,rlist,ktmdl}.rs` is a std-only port of the verified
Python codecs (`scripts/anm_dump.py`, `scripts/ktmdl_dump.py`; format record
`docs/3d_model_format_research.md` §3–§6) plus the one A3 runtime behaviour the Python side does not
model — the bind-pose seed of untracked bones. `scripts/gen_anm_fixtures.py` dumps VALUES-ONLY JSON
(`tests/fixtures/anm/`, 2.1 MB) from the install; the Rust suite re-reads the same arcs at test time
(`scripts/validate_background_dancers.sh`, fixture leg gated on `DDR_WORLD_INSTALL`). Result: every
stock clip matches — 30 dance clips (7 920 world matrices at 8 fractional frames, 2e-5), 69 stage
`_play_loop`s (65 fully tracked → world matrices; 4 partial → per-track samples, 1e-5), 93 stage
camanms (slot samples + the §6 recipe algebra), the four `startup.arc` rlists, `pl_emi00.b2it` and the
33-bone table (bind·inverse ≈ I, seed reproduces the bind pose within 1e-4).

### 3.1 A3 bind seed `FUN_18013ba50` — decompiled precisely (corrects the handoff wording)

Per bone, the pose record `{quat @0, pos @0x10, scale @0x1C}` is seeded as:
- root (`parent == 0xFFFF`): `FUN_180190ce0(bindWorld[i])` → scale = row lengths of the upper 3×3,
  translation = row 3; `FUN_180190a90(bindWorld[i])` → quaternion. **The matrix→quaternion routine is fed
  the RAW bind matrix, not a normalised rotation** — exact only for unit-scale roots (all 2 236 stock
  bones' roots are unit scale; 233 NON-root bones on 11 mapsets carry non-unit, mostly non-uniform
  scale).
- child: `M = bindWorld[i] · inverse(bindWorld[parent])` (`FUN_18018ff10` inverse, `FUN_18018e8b0(out,
  a, b) = b·a` so the row-vector order is `bind_i · inv(parent)`); translation = `M.row3`;
  `P = FUN_18013c000(M, parentScale) = diag(parentScale) · M` (ROWS scaled by the PARENT's seed
  scale); scale = row lengths of `P`; quaternion = `FUN_180190a90(P)` (again unnormalised).
- `FUN_180190a90` is the classic trace test with the three diagonal fallbacks in the row-vector
  convention (`x = (m12−m21)/4w`, `y = (m20−m02)/4w`, `z = (m01−m10)/4w`), branch order: trace > 0;
  else `m00` largest (`m11 < m00 && m22 < m00`); else `m11` (`m22 < m11`); else `m22`.

Why the row prescale: the pose chain (`FUN_18013bc20`) builds `local = diag(s)·R(q)` then divides the
3×3 COLUMNS by the parent's scale (`FUN_18013be60(out, 1/ps, M) = M · diag(1/ps)`) before
`world = local · world[parent]`. A child's bind matrix already contains that compensation, so for a
uniformly scaled parent `P` is exactly the child's own unit-scale local rotation — the child's seed
(scale AND rotation) is exact. The one inexact seed is the SCALED bone's own rotation (mat→quat of
`2·R`). Stock data never relies on it: every scaled bone has a rotation track and a scale track, and
every child of a scaled bone has a rotation track (checked over all 69 stock `_play_loop`s). The port
reproduces A3 exactly rather than "fixing" it (`pose::seed_local_trs`; pinned by
`scaled_bone_seed_and_compensation_a3_semantics`).

### 3.2 Facts the fixtures surfaced (matter for Steps 7–8)

- 4 of the 69 stage loops are PARTIAL: `gm_boom00_stage_play_loop.anm` (also `boom00_g`) tracks ONE
  translation on bone 5 of a 10-bone model, `gm_boom01_stage` 4 tracks / 11 bones,
  `gm_crystaldium00_bg` 6 / 7. With the Python identity seed those props collapse; with the A3 bind
  seed they hold their authored pose — Step 7 MUST seed stage parts from the resident model's bind
  table (or the `.model` file), never identity.
- Every dance clip is fully tracked (33 rotation + 33 translation tracks; scale tracks only on a few
  bones), so the dancer path is insensitive to the seed; `mc_*_ne01_loop` is the only looping dance
  clip, every `_exec` has the loop bit clear.
- Every camanm carries all six slots (kinds `1,4,8,8,8,8`); position magnitudes reach ~2 500 cm, so the
  f32 ulp there is ~2.4e-4 — comparisons use a relative tolerance.
- `startup.arc` members and most arc members are Konami-LZ77 compressed (`pl_emi00.model`, 67 of the
  93 camanms, the `_loop` clips); the test-only arc reader ports `unpack_arc.py`'s decoder. On the DLL
  side `arc_set::read_bytes` returns the WHOLE `.arc` file — Step 7's parse thread must extract members
  with the existing `core::arc::{parse, extract}` (which already runs `avslz::decompress`) before
  handing bytes to `core::anm`.

### 3.3 Step 7 deploy #1 findings (2026-09-16, CrossOver) — visibility edge, camera, the shutdown fault

Three songs (`boom04`, `boom02`, `replicant04`), each fully built (6–8 stage parts + 1 dancer, `default=0`
everywhere, 7–9 items collected, teardown 19–33 ms, `created == released`). What the log + the maintainer
saw:

1. **"visible" fired at scene 26, not at DPS step 5.** `graph_stats().enabled` is still TRUE while the
   song loader runs (scenes 26/27 — the bit is only cleared by the NEW `DancePlaySequence::onInitialize`,
   which does not exist yet), so song 1's nodes rendered during the interstitial and the 2D hide armed
   before `bg_root` existed (`bg-hide: no live bg_root clip for 60 frames` WARN). Songs 2/3 showed the
   real edge (`enabled=false` at 180 frames → collected at ~5 s). Fix: the FR-9 edge is
   `graph enabled ∧ current_scene == GAMEPLAY ∧ live_dps().is_some()`.
2. **The fixed Step 3 camera (4 m, hFOV 53°) sits inside the larger stages** — the maintainer saw the
   upper part of the boom stage and never the floor. Interim camera until Step 9 = the add-on's
   cabinet-verified test camera (§6 of the format doc: eye `(0, 1.6, 5.0)` m → `(0, 0.9, 0)`, in-game
   hFOV 76.8°, which framed the whole boom00 stage with a dancer at 38 % of the frame height).
3. **ACCESS_VIOLATION at game close mid-song** (`0x00006FFFFA724178`, an unnamed thread, "in game /
   other module") — the SAME code offset as the fault in the Step 4 deploy #2 boot (`…A764178`; ASLR
   moved the module by 0x40000), which the maintainer also closed with a live scene; the Step 3 boot
   (rigid footpanel only, no bone textures) never faulted. Log order: `PremiumFree: ghost cache keep`
   (= the result commit of the dying sequence) → FAULT → `Option::SetHispeed` → `Work::GetTickCount`
   (logout accounting) → `CNetworkManager::onTerminate` → `W:DDR: EXCEPTION CATCH` (the game's own
   handler). Decompiled shutdown order (`FUN_180023e70`, the SceneGraphManager shutdown): `FUN_1800241b0`
   pushes the boot-time `DAT_1806f2d00` node (0x458, the graph's first child) + its subtree onto the
   deferred-destroy vector (`FUN_1800a23f0` = `std::vector::push_back`, growth via `FUN_1801f4210`), then
   `FUN_180023b30`: unregister jobs 6/7, clear the four pass pointers `+0xE8`, flush (dtors of the queued
   nodes), `FUN_1802159e0(child, 1)` = recursively UNLINK the root's children (no dtor, no free — our
   nodes are simply abandoned), delete the graph, **then free the destroy vector's storage**. The
   deploy-#1 session attributed the fault to our still-listed skinned items being drawn after the
   sequence died and added the `visit(4)` liveness guard (kept — harmless and still a sensible
   shutdown guard); **that attribution was WRONG — the real site is the vector free, see §3.4** (the
   offset was decoded against the 20260825 program while the cabinet runs 20260915). Also from this
   deploy: `visit(2)` copies the board slot STRAIGHT into the item (no 4 KB stack buffer on the engine
   worker) with the bone count clamped to the item's own `ModelParameters.x`; a node the engine destroys
   outside our teardown is detected (`is_destroyed` poll) and parked; the crash handler names the
   faulting MODULE (`GetModuleHandleEx(FROM_ADDRESS)` + `GetModuleFileNameA`) so the next fault says
   `gamemdx.dll+0x…` instead of "other module" — which is what made §3.4 possible.
4. Song 3 (`replicant04`, 8 RIGID parts + `ruby00`) showed a BLACK background: the 2D hide worked and
   nothing 3D was in view — consistent with the too-close camera inside the dark `sp`/`floor` geometry;
   to be re-judged with the interim camera.

### 3.4 Step 7 deploy #2 findings (2026-09-16, CrossOver, gamemdx 20260915) — the three fixes

Nine windows. Songs 1–2 (`monitor00` 2P `zero00`+`alice01`, `replicant03` `alice00`) built BEFORE the
`visible` frame (177/69 ms vs 882/824 ms) and rendered with dancers DANCING — the feature works end to
end. From song 3 on, `visible -- graph enabled 33–43 ms after request` fired BEFORE `built … 90–134 ms`
and the maintainer saw "stage partially there / black, no dancers ever again"; closing the game
mid-song faulted at `gamemdx.dll+0x24178`. Three independent root causes:

1. **Nodes built after the first visible frame stayed hidden forever (`lifecycle.rs`).** Every node is
   attached with the node-level "force hidden" flag (`session::build_one` → `node::set_hidden(n,
   true)`; `visit(4)` forces the item hidden while it is set) and the flag was cleared ONLY on the one
   frame `visible` first became true (`arm_hide`). With `visible` firing at 40 ms, the 5 nodes built at
   79–134 ms (the dancer is always the LAST instance) kept the flag for the whole song. Fix: the flag is
   dropped per instance right after its FIRST frame-board publish (`Instance::node_shown`; the board's
   own hidden bit — republished every frame by `director::produce` — is the visibility control from then
   on). The one-shot `arm_hide` is now only the 2D-hide arm.

2. **The visibility gate fired at 33–43 ms.** The gate `graph enabled ∧ current_scene == GAMEPLAY ∧
   live_dps().is_some()` is satisfied by ANY live TransitionSequence child: the scene callbacks fire
   BEFORE the game's `createNextSequence`, so for the first frames of scene 28 the active child is still
   the stage-indicator sequence (and the enable bit is still set from the previous song) — songs 1–2
   only escaped because their scenes 26/27 took ~800 ms and the fresh DPS had already cleared the bit.
   Fix: NEW optional RTTI signature `dance_play_sequence_vtable` (`.?AVDancePlaySequence@dance@sequence@@`
   — the class vftable, stored at `this+0` by the ctor `FUN_1800572f0` on 20260825; resolves on all five
   sweep builds: 20250805 `+0x340F48`, 20260224 `+0x348188`, 20260721 `+0x360AB8`, 20260825 `+0x360AD8`,
   20260915 `+0x360AF8`) → `song_reset::dps_step()` returns the active child's `agcs::StackStep` value
   ONLY when its vtable is the DPS's; gate = `graph_stats().enabled ∧ dps_step() >= 5`
   (`song_reset::DPS_STEP_GRAPH_ENABLE` — the onUpdate step that sets the bit; A3's 0x1046 edge). A
   `finish`-path quick restart's fresh DPS starts at step 0 ⇒ hidden until ITS step 5, exactly the
   `clock.rs` rule-1 branch. `MatchingDancePlaySequence` has a different vtable ⇒ no dancers in matching
   sessions (out of scope for v1).

3. **The close crash is the SceneGraphManager shutdown freeing OUR destroy-vector buffer, not the camera
   and not the render side.** `gamemdx.dll+0x24178` decoded against the cabinet's real build
   (20260915; the manager shutdown there starts at `+0x23F70`, the tick at `+0x243F0`) is
   `MOV RDI,[RBX-0x20]` with `RBX = *(mgr+0x08)` = the destroy vector's `begin` — the tail of
   `FUN_180023b30` (20260825 numbering):

   ```
   lVar12 = mgr->destroy_begin;                 // *(mgr+0x08)
   if (lVar12 != 0) {                            // stock World: always 0 (never allocated)
       alloc = *(lVar12 - 0x20);  raw = *(lVar12 - 0x18);
       alloc->vt[+0x20](alloc);  alloc->vt[+0x18](alloc, raw);      // lock, free
       if (--alloc->refcount@0xC == 0 && alloc->owned@8) delete = 1;
       alloc->vt[+0x28](alloc);  if (delete) alloc->vt[0](alloc, 0);  // unlock, dtor
   }
   mgr->destroy_{begin,end,cap} = 0;
   ```

   = the inlined engine free `FUN_1801de6e0(ptr)`. Every buffer the engine's `me::` allocator hands out
   (`FUN_18021f300(allocator, size, align)`, the allocator object `DAT_180466068`) is preceded by a
   0x20-byte header `{allocator* @-0x20, raw block @-0x18, requested size @-0x10, pad}`, and every free
   walks it. Our `queue_destroy` buffer was a bare `VirtualAlloc` block — page-aligned, so `begin - 0x20`
   is the unmapped page before it ⇒ the read faults. It reproduced on EVERY close after the first song
   of a boot (the buffer installs on the first `queue_destroy`), which is why the Step 3 boot (no buffer
   yet) never faulted and the Step 4 deploy #2 and Step 7 deploy #1/#2 boots all did. Fix
   (`scene_graph::destroy_reserve_begin`): the buffer is allocated with the header in front of it,
   `[-0x20]` pointing at a mod-owned fake allocator `{vtable, owned = 0, refcount = i32::MAX/2}` whose
   six vtable slots are all the same `extern "C"` no-op — the shutdown's lock/free/unlock land on our
   no-ops, the count never reaches zero, the buffer is leaked (it lived for the process anyway). The
   `FUN_1801f4150` regrow path frees the old storage through the same header, so an engine-side growth
   (World never grows it) would be harmless too.

   The previous attribution — "our `write_camera0` activates camera slot 0 and the tick then copies
   into destroyed passes" — is wrong twice over: the active-camera byte is `+0x3F4` per slot
   (`scene3d_camera_active_off`, `FUN_1800243a0` walks `cam+0x3F4`), NOT `+0x2B0/+0x2B1` (those are the
   view/projection DIRTY requests), and **stock World already runs with slot 0 active** —
   `FUN_180023f10` (the manager's per-boot camera init) sets `slot[0]+0x3F4 = 1` and every other slot 0,
   the Camera ctor `FUN_180214ce0` sets `+0x3F4 = 1` — so the tick's view/proj copy block runs every
   frame in stock World and is not a mod-induced hazard. Nothing about the camera changed; NEVER
   deactivate slot 0 (it would only make the passes keep stale matrices).

Also observed: a movie song renders mostly black — the 2D alpha-0 hide covers the movie plane too;
Step 10's `Customize+0x30` thumbnail override is the intended answer (until then movies vanish on
those songs). **Corrected 2026-09-22 (§7):** the hide never covered it — a FULLSCREEN movie is drawn
into the 3D target BEFORE the model passes, and the stage geometry painted over it. `created == released` on every teardown (10..42), teardowns 7–33 ms.

### 3.5 A3's scene-clock rate setter — BPM sync + STOP slow-motion (2026-09-16, `gamemdx_20240402.dll`)

Maintainer request after the Steps 7–10 PASS: dance at the song BPM by default, and slow down through
STOPs. Both are the two `ConfigBank` switches of A3's `DancePlaySequence` vtable slot 7 `FUN_18003a1b0`
(twin `FUN_180041cf0` for the matching sequence), decompiled exactly:

```
min = max = +inf/-inf over the live GamePlayActors (DPS+0x100..+0x110, FUN_18003b3e0(actor) == 0 = alive):
    bpm = *(float*)(actor + 0x16c)            // current chart BPM at the render-offset time
if (min < DAT_180264a58 /* 10.0f */ && ConfigBank && ConfigBank["MOTION_STOP_SLOW"])
    *(float*)(SceneGraphManager + 0x38) = 0x3daaaaab   /* 1/12 */ ; return
if (ConfigBank && ConfigBank["MOTION_BPM_DEPENDENCY"])
    *(float*)(SceneGraphManager + 0x38) = max / DAT_180265258 /* 120.0f */ ; return
*(float*)(SceneGraphManager + 0x38) = 1.0f
```

`mgr+0x38` is the playback rate the GraphUpdateJob multiplies into the update dt (`scene3d_mgr_rate_off`,
§1.1) — so A3 scaled the WHOLE scene (dancers, stage loops, camera clips and therefore the cut timing),
not only the dancers. Retail `ConfigBank.csv`: `MOTION_STOP_SLOW = TRUE`, `MOTION_BPM_DEPENDENCY = FALSE`
(research `a3-runtime-rules.md` §2). The 120 divisor + the clip lengths (`mc_*_ne01_loop` = 242 frames ≈ 8
beats at 120 BPM; every `_exec` ≈ 9.7–11.6 measures) confirm the choreography is authored at 120 BPM.

**Port (`mods/background_dancers/tempo.rs`, pure).** The modpack's clock is the content-domain music
count, not a per-frame dt, so the rule is integrated into a piecewise-linear dance time `τ(mc)` over the
chart's SSQ tempo chunk (nodes normalized `round(td·1000/TPS + 0.5)` like the game's `FUN_1801ca230`):
per tempo segment `(Δtick, Δms)`, `bpm = 60000·Δtick/(1024·Δms)`; `bpm < 10 ∧ stop_slow ⇒ Δτ = Δms/12000`;
`bpm_sync ⇒ Δτ = (bpm/120)·Δms/1000 = Δtick/2048` (half a second of dance per beat — exact, no bpm
arithmetic); else `Δτ = Δms/1000`; a warp (`Δms = 0`) lands its `Δτ` at once. Two deliberate additions over
A3 (which only matched the tempo, with arbitrary phase): `τ = 0` is pinned to the chart MEASURE boundary
nearest music 0, so the 120-BPM grid of the clips lands on the chart's beats/downbeats, and the dance
schedule snaps its segment lengths to whole beats (`DanceSchedule::with_quantum(0.5)`) so every cut falls
on a beat. Both A3 switches are exposed as `background_dancers.{bpm_sync, stop_slow}` (default both
`true`; `bpm_sync = false, stop_slow = true` = A3 retail; both `false` = the pre-2026-09-16 real-time
behaviour). Verified against real charts: `dind2` 140.0 BPM (3 nodes), `goli` 150.0, `anan` 175 with 60
STOPs (0.17 s each), `aeth` 384/192 with 2 STOPs (TPS 150). Source of the SSQ: the live DPS basename
(`song_reset::live_dps_basename`, vtable-verified), read from disk mod-folders-first, unsplit then `_1..5`
(`tempo_source.rs`, std thread). Because `τ` is a pure function of `mc`, training rewinds/loops and in-place
restarts need no latch at all — `clock.rs` now reports the music count and re-latches nothing (the first
cut's per-jump origin re-latch made a rewind restart the dance from clip 0).

---

## 4. Phase 2 — lit model shaders for the scene (2026-09-17, World 20260825 + 20260915)

The maintainer's Phase-2 question (progress.md "Deviations → Phase-2 idea"): real lighting for the
dancers/stage. The seam is the artists' own "lit" tag — the `mdl_*_lambert` material shader NAMES that
ship in every character model but have no `.gsp` container — so the answer hinged on three RE questions,
all settled against World itself before any design.

### 4.1 The shader registry is filled from the arc BY EXTENSION and looked up BY HASH — no allowlist

`Application::onBoot` (`FUN_180002060` on 20260825) registers seven `*FileCallback` factories with the
FileManager (`FUN_1801ffd30`): `ShaderFileCallback` (vtable `0x1802dde98`, RTTI `agcs::ShaderFileCallback`)
answers extension `"gsp"` (`FUN_1802095f0` strcmp against `DAT_18035a458`) and its slot 4 (`FUN_180209630`)
creates a `ShaderFileTask` per member → `FUN_1802094b0` wraps the member bytes in an `AsyncRegisterJob` →
`FUN_1802093e0` → `FUN_18025f700(bytes)` = the shader-registry acquire:

* pop a shader object from the free pool (`DAT_1806f3260 + 0x20..0x28`; capacity = gs config `+0xC4` =
  **0x100 = 256** objects, `FUN_1801f0920`'s `local_34` — the stock arc uses 36, so two more are nothing);
* `FUN_18025f190(obj, gspw)` parses the GSPW: `obj+0 = *(u32*)(gspw+4)` (the header's name hash),
  `obj+4 = program count`, `obj+8 = u32 program-handle array` (one `FUN_1802541a0(vs, vsz, ps, psz)`
  create per program entry, **identical `{u32 @+0, u8 @+4, u8 @+5}` triples reuse the earlier handle +
  `FUN_180254370` addref**);
* push into the sorted `std::vector<obj*>` at `DAT_1806f3260[0..1]`, clear the sorted flag (`+0x64`).

Lookup `FUN_18025f8f0(hash)` → `FUN_18025f470`: lazily `std::sort` (`FUN_18025fca0`) then binary-search on
`*(u32*)obj == hash`. Nothing in the path knows a shader NAME; identity is purely the header hash of
whatever `.gsp` members the arc carries. **A brand-new `data/shader/mdl_bg_lambert.gsp` member is
registered exactly like a stock one.** (20260915: acquire `FUN_180209630`-twin chain identical; registry
lookup `FUN_18021eb70`, hasher slot `DAT_1806f1850`.)

**Material → shader selection (World twin of A3 `FUN_18018af30`): `FUN_1802745b0` (20260825) /
`FUN_180233920` (20260915)**, called from the model converter's material pass (`FUN_180274070`) at
CONVERSION time (when the `.model` member is loaded — always after `shader.arc`): `material+0x14` (shader
id) → the model's debug-info block (`res+0x70`: id table `+0x10`, string table `+0x08`, count `+0x0C`) →
shader name string → `(*DAT_1806f2040)(str, len)` → `FUN_18025f8f0(hash)`; **only on a miss** the fallback
`"gs_model_default"` (0x10 chars) / `"gs_model_skinning_default"` (0x19) keyed on `mesh+0x2C != 0`
(BLENDWEIGHT in the decl). The hasher `DAT_1806f2040` defaults to `LAB_180260290` (`FUN_18026aad0` leaves
the config override at `+0xB8` null) whose 10 instructions are **FNV-1 32-bit** (`eax = 0x811C9DC5; for
each byte: eax *= 0x01000193; eax ^= byte`) — byte-for-byte `scripts/gsp_pack.py::fnv1_32`, the same hash
`gsp_pack.py pack --name` writes into the header. `fnv1("gs_screencommand_arrow") = 0x9E93AC7B` matches
the stock header; `fnv1("gs_model_default") = 0x6CD7F817`, `fnv1("mdl_bg_constant") = 0xBDFE3C7B`
(headers of the World 20260915 arc, `gsp_pack.py inspect --expect-name`).

### 4.2 The model pass indexes `programs[stage]` UNCHECKED — a lit container needs FOUR program entries

`FUN_18026cce0(pass, mat, stage)` (material bind, both model-pass callbacks `FUN_1801f6100` DISTANTVIEW and
`FUN_1801f63f0` OPACITY/LOWPRIO_TRANS/TRANS) emits command `0xE` with
`*(u32*)(*(shaderObj+8) + stage*4)` — **no bounds check against `obj+4`**. `stage = *(pass+0x168)`, and
the callbacks set it per record: `rec+0x28` bit 31 clear ∧ `DAT_1806f2d89 != 0` (set to 1 in
`FUN_1801f2c30` graphics init, never cleared) ⇒ `FUN_1801f68d0(pass, DAT_1806f1548 == 0 ? 3 : 2)` — World
inits `DAT_1806f1548 = 1`, so ordinary records bind **program 2**; bit-31 records reset to **program 0**.
(The debug-view remap `FUN_1801f5c90` reaches 9..0x13, only under `DAT_1806f8244` debug bits.) That is why
EVERY stock model container (`gs_model_*`, all ten `mdl_*`) carries **4 identical `(0,0,0)` program
entries** (`gsp_pack.py inspect`: `progs=[(0,0,0)]×4`) — the parser dedupes them to one created program.
**Rule: a synthesized model container ships 4 identical `(0,0,0)` entries, exactly like stock.** A
1-program container would read `programs[2]` past the 4-byte handle array.

### 4.3 Register map on World (fxc 9.29 `/dumpbin` of the 20260915 arc) — and World IS bound at c14

The A3-verified map (format doc §3.7) holds; two additions matter for lighting:

| VS reg | Source (World 20260825) | Notes |
|---|---|---|
| **c14..c17 `World`** | `FUN_18026ca40(pass, mtx)` per draw: copies the sorted entry's matrix to `pass+0xC0`, computes `WVP = World × VP(pass+0x80)` into `pass+0x100`, then emits **`0xE` (c14, 4 regs) = World** and `0x12` (c18, 4 regs) = WVP | `mdl_bg_constant`/`mdl_ch_constant` VS read `c14` (`_gs_vs_parameter_World`, `o2.xyz = pos·World` TEXCOORD5 — dead: their PS ignores it); `gs_model_*_default` do not. **The Phase-2 idea's "only WVP is bound" was wrong** — a lit VS can transform normals into WORLD space |
| c18..c21 WVP | same call | row-vector convention: `o0 = v.x·c18 + v.y·c19 + v.z·c20 + c21` |
| c22 `ModelParameters` | `FUN_180262670`: `0x16` ← `item+0x40` (VS), `FUN_18026c520(…, 2, item+0x40)` (PS c2) | `{bone_count, 1.0, 0, 0}` for the DLL's items |
| c23 `ModelUnitParameters.m_color` | `0x17` ← sorted-entry colour | per-draw tint |
| c24.. `parameters` | `FUN_18026cce0`: `0x18` ← `mat+0x18` × `param_count` (VS), `3` (PS) | lambert materials: `param_count = 2` — c24 `m_vTexAnime` = `(1,1,0,0)` on all 158, c25 `vConstatntColor` = `(1,1,1,0)` (135) / `(1,1,1,1)` (23) |
| s3 `BoneMatrices` | render item bone texture, `create(4, bone_count)` (§2.8) | **u = (row + 0.5)/4 → 0.125 / 0.375 / 0.625 for matrix rows 0/1/2; v = (bone_idx + 0.5)/(c22.x + c22.z)**; `texldl` with LOD 0; `w0 = 1 − v3.x − v3.y − v3.z` for `v2.x`, `v3.xyz` for `v2.yzw`; output component k = `dot(row_k, (pos,1))` (3×4 matrices, `invBind·bone`, MODEL space — then WVP) |
| c48..c49 `s_ConstantParameters` | (fog `o4.x = pos.w·c49.x − c49.y` in the `mdl_*` VS; no PS reads `dcl_fog`) | dead — not used |

Stock blobs (identical on 20260825/20260915): `gs_model_default` VS 6 instr (`o0 = pos·WVP; o1.xy = uv;
o2 = c23`), `gs_model_skinning_default` VS 73 instr (the palette skinning above + the same outputs),
**their PS is byte-identical**: `texld r0, v0, s0; mul_pp oC0, r0, v1;` + the 32×32 stipple
`texkill(c2.y − tex2D(s15, vPos·(1/32)).y)`. The ten `mdl_*_constant*` PS are 2 instructions
(`tex × color`, NO stipple) and their VS apply `m_vTexAnime` (`uv·rcp(c24.xy) + c24.zw`) and output
`dcl_texcoord3 o1` / `dcl_color o3`.

Consequence for the design: because the lit factor multiplies `tint.rgb` linearly, it can be folded
into the VS's COLOR0 output and the lit containers can pair **our VS with the game's OWN
`gs_model_default` PS** (sliced from the arc at synthesis like every other container) — alpha, the
stipple dissolve and the `c2` reads stay bit-exact by construction and no Konami bytecode enters the
repo. What a lambert surface renders TODAY is exactly that PS behind the `gs_model_*_default` VS.

### 4.4 Which shipped World models name `lambert` — dancers only, never stages

`ktmdl_dump.py` over ALL 141 `mapset_*`/`pl_*` arcs of the World install (289 models, 484 materials):

| Arc class | `mdl_bg_lambert` | `mdl_ch_lambert` | everything else |
|---|---|---|---|
| 26 `mapset_*` stages (94 models) | 0 | 0 | 325 (`ch_constant_vc` 217, `bg_constant_vc` 96, `ch_constant_c_vc` 7, `bg_constant` 2, `ch_constant_c`/`bg_constant_c_vc`/`ch_constant_vc_notex` 1 each) |
| 27 `pl_<key>` bodies | 0 | **68 — every material of all 26 dancer bodies** | `pl_shadow00` = `mdl_bg_constant` (the floor shadow) |
| 88 `pl_<key>_<part>` parts (head/face/chest/hips/forearm) | **90 — every material of every part** | 0 |

No model mixes lambert and constant materials. Layouts: `mdl_bg_lambert` = layout C (`POSITION@0,
NORMAL@12, TEXCOORD0 FLOAT16_2@24`, stride 28, no COLOR0), `mdl_ch_lambert` = layout D (`POSITION@0,
BLENDINDICES UBYTE4@12, BLENDWEIGHT FLOAT3@16, NORMAL@28, TEXCOORD0@40`, no COLOR0). Render modes of the
lambert meshes: 105 opaque alpha-test-127, 30 alpha-blended (hair/translucent parts, zwrite mixed),
1 additive — all RENDER STATES from the mesh flags, untouched by a shader that keeps the stock alpha
output. So "lit models" means, on a cabinet: **every dancer body and every attached part shades with
the light; the shadow blob and the WHOLE stage (all `constant*`, baked/emissive by authoring) stay
exactly stock.** The first operator row was therefore named DANCER LIGHTING. (Superseded 2026-09-21 by
§4.7: the material re-point restyles the stage's `constant*` materials too — LIGHTING STYLE, whole scene.)

### 4.5 Light space

`World` (c14..c16 3×3) makes a WORLD-space light direction available. For this scene the item worlds
are `body_world = diag(s)·T(x,0,0)` (uniform scale + translation — inverse-transpose ∝ the matrix,
so `n_w = normalize(n_skinned · World3x3)` is exact), stage parts are identity, and the mirrored right
forearm's `diag(−1,−1,−1)` inverts the normals along with the point-inverted geometry — which is the
correct normal for that geometry. The light is a compile-time constant (world +Y up, +Z toward the
fallback camera `(0,1.6,5) → (0,0.9,0)`): a key from above-front. A per-frame adjustable light would
need a constant emission into the model pass (a new detour) — out of scope; the stage cameras moving
around a fixed world light is physically the right behaviour anyway (a fixed rig).

### 4.6 Phase 2b — a second draw per mesh WITHOUT a detour (cel outlines, 2026-09-17)

Inverted-hull outlines need every lambert mesh drawn twice (body, then a black extruded shell). Two
World facts make that possible with zero engine changes:

* **The bit-31 program selector.** Both model-pass material-bind callbacks (`FUN_1801f6100`
  DISTANTVIEW, `FUN_1801f63f0` OPACITY/LOWPRIO_TRANS/TRANS) choose the program index per DRAW RECORD:
  `rec+0x28 & 0x8000_0000 == 0 ∧ DAT_1806f2d89 != 0` ⇒ `stage = DAT_1806f1548 == 0 ? 3 : 2`, else
  **program 0** (`FUN_1801f68d0(pass, idx)` emits command `0xE` with `handles[idx]`). Every other
  reader of the record flags ignores bit 31: the collector `FUN_180263430` tests bit 27 (skip),
  `& 0xE0` (blend group — forced to `0x20` when the entry's tint alpha < 1) and `rec+0x2C & pass
  filter`; the draw-state emitter `FUN_180261c80(pass, flags)` reads bits 1–4 (`~(f>>k)&1` → render
  states 0/4/6/8 of command `0x11`) and `& 0xE0` (blend group → commands `0x13`/`0x1F`/`0x21`).
  The DLL builds its items' record arrays itself (`render_item.rs`, `REC_FLAGS = gpuFlags &
  0xF000_00FF`), so it owns bit 31: a record with it set binds **program 0 of the material's
  container**; ordinary records bind program 2. A lit container laid out as `{0: outline VS/PS,
  1..3: style VS/PS}` therefore draws the same mesh as a body (program 2) or as its hull (program 0)
  purely by record flag. Because the mask keeps the stock top nibble, body items now CLEAR bit 31
  (a stock record carrying it would have been invisible with 4 identical programs but would turn
  into a hull under the new layout); stock containers are unaffected either way.
* **Records are counted from the RESOURCE.** The collector walks `item+0x70` (the item's private
  record array) for `*(item+0x60)+0x24` entries (`res+0x24`, the resource's draw-record count) —
  an item cannot carry more records than its resource has meshes. So the hull is a **second render
  item** per dancer body / part, built from the same `ResourceView` with its own material/palette
  copies and bone array, every record bit-31'd. Its node reads the SAME frame-board slot as the
  body's node (`visit(2)` copies world/tint/bones/hidden from `node.instance` into whichever item
  the node owns — two nodes on one slot are two readers of a seqlocked snapshot), so the director,
  the publish path and the slot budget are untouched; only the session's instance list grows
  (`InstanceKind::Hull { of }`) and the item/node teardown covers it like any other instance. Stage
  parts and the shadow name `constant` shaders whose stock containers have 4 identical programs — a
  hull there would be a plain duplicate draw — so hulls exist only for `Dancer`/`Part` instances.

**Runtime seam for a live style switch (not used yet):** `DAT_1806f1548` (u32, set to 1 once in
`FUN_1801f2c30`; read only by the two bind callbacks + the debug-view remap `FUN_1801f5c90`) selects
program 2 vs 3 for every ordinary record. A container `{0: outline, 1: lit, 2: lit, 3: cel}` plus a
DLL write of that one global would switch every dancer between styles at runtime, cabinet-wide (the
3D background is one scene for both players), without a detour or relaunch. Needs one derivation
(the `MOV EAX,[rip+DAT_1806f1548]; TEST` pair inside either callback) — deferred until the
presentation layer settles.

**Outline geometry without cull control.** Render states come from the shared GPU record (cull
mode included), so the classic "cull front faces" inverted hull is unavailable. The hull VS instead
offsets each vertex in SCREEN space along the projected normal (constant pixel width, aspect
16:9 baked — the render size is always 16:9 or the 1280×720 SD render) and pushes its depth back by
`lerp(PUSH_RIM, PUSH_FACE, facing²)`: grazing (silhouette) vertices barely move in depth so the rim
survives beyond the body's edge, camera-facing vertices are pushed well behind the body so the
hull's interior always loses the depth test. Order-independent for z-writing meshes; alpha-blended
z-write-off meshes (hair/veils) would be darkened by their own hull in either order, so hull records
whose blend group (`flags & 0xE0`) is non-zero are hidden (bit 27) by default. The projection
constants the ink/facing terms need (`P00`, `P11`) are recovered in the VS from the two matrices the
pass DOES bind: `(WVP)3×3 = World3×3 · V3×3 · diag(P00, P11, P22)` for an affine World and a rigid
View, so `|column j of WVP3×3| / |World row 0| = P_jj` (uniform-scale World — true for every item of
this scene), `n_view.z = (n · WVP)_w`, `pos_view ∝ (clip.x/P00, clip.y/P11, clip.w)`.

### 4.7 Phase 2c — whole-scene restyle by re-pointing the PRIVATE material copies (2026-09-21)

The stage cannot ride the by-name seam of §4.1: its materials name the ten `mdl_*_constant*` shaders that
DO ship, and one stage mixes opaque props with additive glows and an inward-facing skydome under the same
names. The seam that handles all of it is the material record itself:

* `FUN_180274070` (the converter's material pass) writes into each 0x168 material record: `*(u32*)(mat+0x10)
  = FUN_180275a40(maya_name)` (the MATERIAL identity hash — not the shader's) and **`*(mat+0x20) =
  FUN_1802745b0(mat, mesh, res)` = the resolved `gs::Shader*`** (§4.1's selection incl. the fallback), then
  `u16 mat+0x18 = param_count`, params from `mat+0x28`, texture slots. The draw (`FUN_18026cce0(pass,
  mat+0x10, stage)`) reads the object back from `*(param_2+0x10)` and binds `*(*(obj+8) + stage*4)`; nothing
  in the draw path reads a shader NAME or re-resolves. The DLL's render items carry PRIVATE copies of those
  records (`render_item.rs`, plain `memcpy` — §2.2), so **re-pointing `copy+0x20` at another resident shader
  object restyles that material for that item only**, per record, per instance, at build time — no detour,
  no per-pass rewrite, and a next-SONG decision instead of next-launch.
* The object to point at comes from the registry lookup the converter itself uses: `FUN_18025f8f0(u32
  fnv1_hash) -> gs::Shader*` (§4.1; null on miss; spins the registry flag, lazy sort, binary search on
  `*(u32*)obj == hash`). New OPTIONAL signature `model_shader_select_site` (unique + byte-shape identical on
  all five sweep builds; the CALL at match+6 and the one at match+53 must agree, the two `LEA RCX` must name
  `gs_model_skinning_default` / `gs_model_default`, the callee prologue `40 53 48 83 EC 30 48 C7 44 24 20 FE
  FF FF FF 8B D9`) → derived `scene3d_shader_lookup` (`+0x2165C0 / +0x2431E0 / +0x2574A0 / +0x25F8F0 /
  +0x21EB70` on 20250805 / 0224 / 0721 / 0825 / 0915). `gs::Shader = {u32 name_hash @0, u32 program_count
  @4, u32* program_handles @8}`.
* Consequence for the container set: the synthesis emits **style VARIANTS under new names** — for each stock
  model shader name `N` the scene uses, `N_lit` and `N_cel`, each with the outline pair at program 0 — and no
  longer the by-name `mdl_*_lambert` overrides (which made the style a boot decision). A material copy whose
  object hashes to `N` (for the dancers' lambert materials: to the FALLBACK `gs_model_(skinning_)default`,
  since no `mdl_*_lambert` exists) is re-pointed at `lookup(fnv1("N_<style>"))`. Every variant is always
  synthesized when `shader-fixes ∧ background-dancers` are on; the STYLE lives in the dancers mod
  (`background_dancers.style` / `.outlines`) and is applied per session.
* **Stock interpolator contracts the lit variants must reproduce** (fxc `/dumpbin` of the World 20260915
  containers): every `mdl_*` PS reads UV from **`TEXCOORD3`** (`dcl_texcoord3 v0.xy`) and colour from
  `COLOR0` — unlike `gs_model_default` (`TEXCOORD0`); `_c` PS: `oC0.rgb = tex·color·c4.rgb + c5.rgb`,
  `oC0.a = tex.a·color.a`; `_notex` PS: `oC0 = COLOR0` with the VS having applied `(c23·COLOR0)·c25 + c26`
  (`vConstatntColor`/`vOffsetColor` in the VERTEX shader for that variant); `_vc` VS: `COLOR0 = c23·v_color`.
  The LIT style therefore pairs a define-driven VS (`UV3`, `VCOLOR`, `NOTEX`) with EACH name's own stock
  PS sliced from the arc; the CEL style's own PS takes `TEXCOORD0` + `CCOLOR`/`NOTEX`/`STIPPLE` defines.
* **Which stage materials get restyled (DLL rule, pure + host-tested):** a material copy is re-pointed iff
  every draw record using it has blend group 0 (`REC_FLAGS & 0xE0 == 0` — opaque/alpha-tested; additive
  glows and alpha-blended translucents stay stock, exactly as authored), the instance is not the shadow
  (`pl_shadow00`) and not a stage part whose model name ends in `_bg` (the skydome/backdrop — lighting an
  inward-facing dome puts a gradient across the sky). Stage-part hulls follow the same eligibility (the
  skydome's shell would be all front-facing anyway). Names present in the shipped stage/character arcs:
  `mdl_ch_constant_vc` 217, `mdl_bg_constant_vc` 96, `mdl_ch_constant_c_vc` 7, `mdl_bg_constant` 3 (2 stage
  + the shadow), `mdl_ch_constant_c` 1, `mdl_bg_constant_c_vc` 1, `mdl_ch_constant_vc_notex` 1, plus the
  dancers' `mdl_ch_lambert` 68 / `mdl_bg_lambert` 90 — nine names, 18 variant containers (the three unused
  stock names `mdl_ch_constant`, `mdl_bg_constant_c`, `mdl_bg_constant_vc_notex` get no variant).

**Outline fix (the 2026-09-21 screenshot — no line where an arm crosses the torso).** The first hull hid
its interior with a facing-dependent DEPTH push (`PUSH_FACE` ≈ 20 mm at 5 m); where an arm rests on the
chest the parts are 0–20 mm apart, so the arm's shell fell behind the chest and only the whole-figure
silhouette (nothing behind it) survived. The classic inverted hull avoids this by CULLING the shell's front
faces; without cull-mode control the outline PS now does the same per pixel — `clip(dot(n_view, pos_view))`
discards front-facing shell fragments (both vectors interpolated from the VS) — and the depth push shrinks
to a 1 mm constant in WORLD metres (`Δclip.z = P22·Δ`, `P22 = |WVP column z| / |World row 0|`, the same
recovery as `P00/P11`). Back-facing shell fragments sit behind the body by its own thickness and poke out 2
px at every silhouette — including an arm's edge over the chest (the arm's centre is ~3 cm in front of it).

**Deploy #5 (2026-09-21) — stage hulls built but invisible: the distance falloff.** The log proved the
twins existed (`built … 8 hull`, every `gm_*_<prop> [hull] N record(s) marked bit-31 … restyled=…`) and the
props' meshes are closed (edge-sharing survey: `gm_club00_speaker` 1–7 % boundary edges, `gm_boom01_dodai`
0 %, the dancer body 3 %) with well-formed unit normals — so the shell was drawn, just not seen. Cause: the
rim width was `OUTLINE_PX · saturate(OUTLINE_REF_DIST / w)` with `REF_DIST = 5 m`, i.e. constant 2 px only up
to 5 m and ∝ 1/w beyond; the A3 stages span ±10–50 m (`gm_boom01_back` x ±30 m, the stock cameras sit
8–30 m from most props), so a prop at 20 m got 0.5 px — a sub-pixel rim that rasterizes to nothing. The
dancers (≈ 5 m from every stock shot) were always inside the constant zone. Fix: `OUTLINE_REF_DIST = 25 m`,
and the rim width is now a PER-ITEM constant: the hull VS reads `ModelParameters.w` (`item+0x4C` — read by no
stock shader: `.x/.z` feed the bone-texture height, `.y` the stipple), which the DLL sets on each twin
(`render_item::set_outline_width`) from `background_dancers.outline_px` (dancers, default 2.0) /
`.outline_px_stage` (props, default 1.5 — large flat props read heavier than a figure); 0 falls back to
the shader's `OUTLINE_PX`.

### 4.8 Layered (DDR World text-style) outlines — colour per hull, stacking by z-test (2026-09-21, experimental)

The World UI's headline text is drawn with several strokes stacked — black nearest the glyph, then red,
then blue. The same look on the 3D scene needs two things the single hull lacked: a per-hull COLOUR and a
way to stack several rims with the narrowest on top. Neither needed a new detour or shader constant:

* **Colour rides the draw RECORD, not the tint.** The frame board republishes only the item's TINT
  (`item+0x50`) every `visit(2)`, and a hull node reads its BODY's slot — so a tint written at build is
  overwritten with the body's white on the first update. The per-record colour word (`rec+0x00`, 1.0
  white at build, §2.4) is never touched by the board; the collector multiplies `rec.color × tint` into
  the sorted entry and the draw uploads the product as VS c23. `render_item::set_record_colors` writes
  the layer colour into every record of a hull twin once, at build, and the outline PS now emits COLOR0
  VERBATIM (the first build's `OUTLINE_RGB (0.03) × tint` became a DLL-side default:
  `outline::INK_RGB = 0.03` grey, so INK is pixel-identical). Alpha stays 1.0 — the collector forces the
  blend group to 0x20 for an entry whose colour alpha is below 1 (§4.6). One outline pair (program 0)
  therefore serves every colour; nothing in `shader_layout`/`shader_synthesis` changed except the two
  outline PS blobs (`mdl_outline[_notex].ps.d3dbc`, in the `v7` fingerprint ⇒ the cache re-synthesizes).
  Mismatch behaviour: new DLL + OLD blobs ⇒ ink stays black (0.03·0.03), the coloured layers come out very
  dark; old DLL + NEW blobs ⇒ white outlines (the old DLL leaves the records white). Deploy both.
* **Stacking is the z-test's.** Each layer is its own hull ITEM (records are counted from the resource,
  §4.6, so one item is one draw per mesh), rim = `(k + 1) × base` (`outline::HullPlan::width`; `base` =
  the kind's ink width — equal strokes like the text: 2/4/6 px on dancers, 1.5/3/4.5 on props before the
  §4.7 distance falloff, which scales every layer alike; a separate band knob was tried and retired the
  same day). At a screen pixel inside a stroke both the narrow and the wide hull's BACK-facing shells
  survive the PS's emulated front cull; the narrow hull's fragment comes from a vertex nearer the
  silhouette (the screen offset is `+k·base` along the projected normal, so the wide hull samples the mesh
  further onto its far side), i.e. shallower in view depth — so the narrowest layer always wins the depth
  test, in any draw order, without touching the 1 mm push. Concave regions and thin limbs over a body can
  still interleave (the same limits as the single hull); it is an experiment.
* Plumbing: `InstanceKind::Hull { of, layer }`, `Session.hulls: outline::HullPlan` (frozen per song from
  `style::hull_plan(&effective())`), `build_one` looks the layer up for width + colour and logs
  `[hull L<k> #rrggbb] … rim N px`. Config `outline_style` (`ink` default / `layered`) and
  `outline_layer_colors` (operator palette override, 1..=4 `[r,g,b]` entries — each layer is a full extra
  draw of every outlined mesh, hence the cap); one row, OUTLINE STYLE, under the Background Dancers
  header. Pure layer math host-tested in `background_dancers/outline.rs`
  (`validate_background_dancers.sh`).

## 5. Viewport passes / RENDER_2D compositing — the options-menu previews (2026-09-21, `gamemdx_20260825.dll`)

The BACKGROUND DANCER / BACKGROUND STAGE option rows preview their pick LIVE inside the options modal's
preview box. The 3D of §1–§4 is the frame FLOOR (the MODEL passes render into RENDER-3D before every 2D
layer), so a preview needs a way to draw 3D ABOVE the modal, clipped to a 170×150 box, without touching
the stock passes or camera slot 0. The mechanism (`services/scene3d/viewport_pass.rs`, design
`.agents/planning/2026-09-21-background-dancers-selection-options/design/detailed-design.md` §3.1/§4.5;
Ghidra evidence `research/preview-compositing.md`) is **mod-owned byte-clones of the engine's MODEL pass
objects attached into the RENDER_2D target list with their own D3D viewport rect and camera matrices**.
Cabinet-proven 2026-09-21 by a colour-clear smoke (a solid rectangle exactly over the box, above the UI).

### 5.1 Target lists and the render dispatch

* Display object `DAT_1806f2ef0`; target lists at `display+0x08` OFFSCREEN1, `+0x28` RENDER-3D, `+0x30`
  AFTER-RENDER-3D, **`+0x38` RENDER_2D**, `+0x40` DISPLAY, `+0x48` PRESENT (render-graph boot
  `FUN_1801f2c30`). A target list (ctor `FUN_180266660`) is a plain `std::vector<{viewport*, u32 prio}>`
  at `+0x00/+0x08/+0x10` (16-byte elements), clear flags `+0x20` / rgba `+0x24` / z `+0x28` / stencil
  `+0x2C`, its target surface at `+0x38` (u16 dims at `+0x14/+0x16` — the display back buffer for
  RENDER_2D, output-sized under custom resolution), flags `+0x40` (bit0 disabled, bit1 clear-at-start).
* **Attach `FUN_1802666c0(list, viewport, prio)`** fills the viewport rect from the target dims only when
  it is all zero, then `push_back` + `std::sort` by prio (ascending). **Detach `FUN_1802667d0(list,
  viewport)`** erases. Neither is an intrusive list — a mod-allocated viewport is a first-class member.
* Stock RENDER_2D contents: the three AFP layer-list viewports (FRONT/MIDDLE/BACK — every AFP layer incl.
  the options modal and the mod-menu widget layer) at prios `0x65/0x66/0x67`. A viewport at `≥ 0x68` draws
  AFTER all of them; the SYSTEM text list still draws later in DISPLAY. Ours: P1 `0x68/0x69/0x6A`
  (clear / OPACITY clone / TRANS clone), P2 `0x6B/0x6C/0x6D`.
* Frame end `FUN_18026af10` → per list `FUN_180272600`: the optional list Clear, then per viewport with
  flags bit0 clear: a `0x100039` marker record + a job `{rec, viewport, list}` for the render worker
  `FUN_180272d30`, which calls **`FUN_18026cec0(ctx, viewport+8 /*rect*/, list)`** — D3D `SetViewport(x,
  y, w, h, minZ, maxZ)` from THE VIEWPORT'S OWN RECT, then, iff `viewport+0x1C & 2 == 0` (outer `+0x54`
  bit1), uploads THE VIEWPORT'S OWN proj (`+0x28` = outer `+0x58`) / view (`+0x68` = outer `+0x98`) as VS
  c0..3 / c4..7 / c8..11 view×proj / c12 camera position / c14..17 identity / c18..21 wvp plus a default
  render-state block — then `vp->vft[0](vp, ctx)`, then the `0x4003a` terminator. So each pass renders with
  its own rect AND its own matrices; camera slot 0 is not involved.
* Thread safety: the dispatch spins on the workers at the end of `FUN_18026af10`, so game-thread writes
  from `input_manager::on_frame` (mid-frame) never race a worker. Attach/detach are game-thread-only
  there; a detached object outlives its frame (`viewport_pass::reap()` frees ≥ 2 frames later — called
  ONCE per frame from `background_dancers/mod.rs`).

### 5.2 The MODEL pass object (0xF8) and the clone recipe

Ctor `FUN_1801f6510`: `+0x08..+0x20` four render callbacks (shared by OPACITY/LOWPRIO/TRANS), `+0x28`
sort mode (OPACITY 0x15, LOWPRIO 0x12, TRANS 0x1A, DISTANT 0x10), **`+0x2C` node-mask FILTER** (OPACITY
0x56, LOWPRIO 0x10, TRANS 0x46, DISTANT 0x01 ⇒ bits `0x08`, `0x20`, `0x80` FREE), `+0x30` the
`gs::Renders::Model::Viewport<Render>` vftable (slot 0 render `FUN_1801f68a0`, slot 1 dtor), `+0x38..+0x44`
rect, `+0x48/+0x4C` minZ/maxZ, `+0x50` name hash, **`+0x54` flags (bit0 DISABLED — the dispatcher skips
it; bit1 skip the camera upload)**, `+0x58` proj, `+0x98` view, `+0xE0` self back-pointer (render reads
`viewport+0xB0`), `+0xE8` render-item list (`*(graph+0x30)`, shared), `+0xF0` callback block. The
SceneGraphManager tick (`FUN_180023fb0`) memcpys camera slot 0 into the FOUR STOCK passes only — a clone is
never touched by the engine after construction.

Clone: `alloc_zeroed(0xF8)`, `memcpy` from the live stock pass (probed + identity-gated: `+0x30 ==
vftable`, `+0xE0 == self`, filter `∈ {0x56, 0x46}`), then `+0xE0 = clone`, `+0x38.. = box rect (RT px)`,
`+0x2C = 0x08 (P1) / 0x20 (P2)`, `+0x54 = 0`; `+0x58/+0x98` written every frame by the owner; attach
`clone+0x30`. The render entry `FUN_1802606d0(outer, ctx, items)` is pass-agnostic: it collects over the
whole shared item list with `outer+0x2C` as the filter (`item+0xB0 & filter`) + a clip-space AABB cull
against the WORKER CTX's view×proj (= this pass's matrices), so a clone draws exactly the items stamped
with its private bit, correctly culled for its own camera. Preview scene nodes stamp their items with the
side's bit (`Session::new(.., item_pass_mask = Some(bit))` → `node.item_pass_mask`, re-stamped every
`visit(4)`), so the stock passes never draw them.

### 5.3 The mod-owned ClearViewport and the gd Clear record

Only the target list clears (once, at list start). A preview needs its own depth clear (the AFP quads
under it may have written depth) and — design choice — a colour backdrop. `viewport_pass_layout::ClearViewport`
(0x40, `#[repr(C)]`, offsets pinned by `const` asserts) reproduces the engine's `gs::Viewport::Base`
header `{vtable, +8 rect, +0x18 minZ, +0x1C maxZ, +0x20 name, +0x24 flags = 2 (skip camera upload)}` and
carries the payload `{clear_flags, color, z, stencil}`. Its 2-slot RWX vtable's slot 0 (render worker,
`node_visit` rules: no engine API / alloc / lock / log, `catch_unwind`) appends ONE record at
`*(workerCtx+0x218)`: **`{u16 tag 0, u16 size 0x14, u32 D3DCLEAR flags (1 target | 2 zbuffer | 4
stencil), u32 D3DCOLOR, f32 z = 1.0, u32 stencil}`** — the exact shape `FUN_180272600` emits for the
list Clear — and advances the pointer by 0x14. D3D9 `Clear(0, NULL, …)` clears the CURRENT viewport,
which the worker set to our rect one call earlier. **The colour is a D3DCOLOR `0xAARRGGBB`**: the first
smoke shipped `0xFF20A0FF` meaning to be violet and the cabinet showed AZURE (R 0x20 G 0xA0 B 0xFF) —
which is how the record's ARGB consumption was confirmed. The previews use `0xFF0C0C14`.

### 5.4 Camera matrices (`scene3d/camera_math.rs`, pure)

The passes consume the camera's `+0x08` VIEW (`FUN_180220b80` = `D3DXMatrixLookAtRH`, row-major, ROW
vectors: `f = normalize(eye − target)`, `s = normalize(up × f)`, `u = normalize(f × s)`, rows `(s.x,
u.x, f.x, 0) (s.y, u.y, f.y, 0) (s.z, u.z, f.z, 0) (−eye·s, −eye·u, −eye·f, 1)`) and `+0x1C8` PROJ
(`FUN_1802376e0`, `w > 0`: `[0][0] = 2w/(r−l)`, `[1][1] = 2w/(t−b)`, `[2][0] = (r+l)/(r−l)`, `[2][1] =
(t+b)/(t−b)`, `[2][2] = −far/(far−near)`, `[2][3] = −1`, `[3][2] = −far·near/(far−near)`; depth in
`[0, 1]`). `cam+0x88` is a second GL-range projection for the frustum planes — unused by the passes.
`CamSample::frustum()` → `camera_math::view_proj` → `PassSet::set_camera` writes the clones directly.

### 5.5 What the graph does NOT do

`SceneGraph::update FUN_180214570` pass 4 pushes EVERY visible node's item onto the item list; pass 5
only calls `visit(5)` per active camera and its result gates child recursion — **the item list is not
culled there**. Record culling happens in the collector against the PASS's matrices (§5.2). Hence a clone
with its own view/proj culls correctly and camera slot 0 needs no write for a preview (FR-13).

### 5.6 Correction: 2D-list tag `0x10` is SetTexture, not a model draw

The feasibility doc's "Option C" (drive model draws through ScreenCommandList tag-0x10 records) is
closed: walker `FUN_18026a040` case `0x10` → `FUN_180269600` → `FUN_18026cc00(ctx, stage, texObj)` emits gd
tag 8 SetTexture from a `{TextureData*, f32[4]}` object; `0x11` = SetTexture by id, `0x12` = gd 0xA with a
u64, `0x17` = gd 0xD SetRenderTarget. No model-draw record exists in the 2D list.

### 5.7 Signatures (`scene3d_resolve_viewport`, OPTIONAL all-or-nothing sub-group of `Scene3dSites`)

Nine AOBs, published as `scene3d_vp_*` (8 addresses + 25 values), swept ALL GREEN on 20250805 / 20260224 /
20260721 / 20260825 / 20260915 with every decoded value equal to the table above; `shape_diff.py` clean:
`render_graph_boot_attach` (display global, RENDER-3D offset, attach fn, the three pass globals + `+0x30`
sub-object; prios `0x66/0x67/0x68` attested), `render_graph_2d_attach` (the RENDER_2D list offset `0x38`),
`viewport_detach` (anchored on the preceding `ADD RDX,imm32` block; the MODEL detach blocks start at
`+30 + 27k`), `model_pass_ctor` (0xF8 alloc; self/items/callbacks/name/filter/sort offsets; the vftable
identity gate), `model_pass_enable_tail` (hits TWICE by design — pass ctor tail + SceneGraphManager ctor;
both must agree; the DISTANTVIEW global for the live free-bit check), `scene_manager_camera_copy`
(proj/view offsets cross-checked against `camera_view_off`), `viewport_setup_rect` (rect `+8` / flags
`+0x24` inside the sub-object; the bit1 semantics), `worker_gd_write` (`+0x218`; `0x4003a` attested),
`target_list_clear` (`0x140000` store + the four payload copies; target `+0x38`, dims `+0x14/+0x16`).
Never in any `required_signatures`: a miss ⇒ `viewport_pass::is_available() == false`, the rows keep
working, the box shows the RANDOM badge / chrome only. `is_available()` also reads the four live stock
filters and refuses when a private bit is taken.

### 5.8 The preview scene shape

`background_dancers/preview/`: per side a `PreviewSlot` — `state::SlotState` (focus / wanted / 150 ms
settle re-armed on value CHANGE / live), a `PassSet` created lazily (kept attached, DISABLED when nothing
is live), a `scene_window::SceneWindow` (the gameplay window's load → parse → residency-gated build →
publish → three-phase teardown, extracted in Step 4) over a stage-only (`Pick::stage_only`, no dancers,
synthetic 9 s dance schedule for the camera cuts) or dancer-only (`Pick::dancer_only`, no stage, no shadow —
`ParseOptions::PREVIEW`) pick, `Session::new(.., TempoOptions::REAL_TIME, style::effective().style,
HullPlan::none(), slot_base = 0 (P1) / 16 (P2), Some(0x08 / 0x20))`. Box = the row template's green
marker (`(191, 11, 170, 150)`) at the panel origin `(185, 463)` / `(742, 463)`, mapped to render-target
pixels by the RENDER_2D target's dims. Camera: the stage's `.camanm` director sample with its VERTICAL
half-tangent kept and the horizontal set to `t × box_aspect` (a centre crop of the 16:9 frame — design §4.7
amendment: the box is 170×150), the cropped gameplay fallback `(0, 1.6, 5) → (0, 0.9, 0)` hFOV 76.8° when
the row has no camera set, or the fixed dancer camera `(0, 1.05, 3.4) → (0, 0.95, 0)`, vertical
half-tangent 0.32. Time base = wall clock from the first built frame. Passes enabled iff built ∧ the 0-0-0
menu is closed. Teardown on focus loss / modal close / leaving scene 25 (the graph stays enabled through
scenes 26/27, so it completes while the gameplay window loads). Dev knob `DDR_DANCERS_VIEWPORT_SMOKE`
(developer_mode): a violet colour+depth clear over the P1 box for 3 s at every song-select entry.

## 6. Custom dancers & stages from `data_mods` (2026-09-22)

No new RE — the loader, the formats and the row/preview machinery are all the ones above; what this
section records is the DATA contract a custom model must meet, derived from what the engine and the
mod actually read. Everything lives under ONE base, `data_mods/custom_models/{dancers,stages}/`
(maintainer call: no per-character mod folders — one mechanism, one base).

**Where the key lives.** The stock rlist key is the arc stem AND the model directory/file stem inside
the arc (`pl_peter00.arc` → `data/chara/pl_peter00/pl_peter00.model`; `mapset_griffin00.arc` →
`data/map/gm_griffin00_room/gm_griffin00_room.model`). The engine's `ModelFileCallback` registers each
converted model under its FILE STEM's FNV-1 hash (`pure::fnv1_name_hash`, §1.5), and the DLL asks the
ResourceManager for `pl_<key>` / `gm_<key>_<part>` by that name — so the key of a custom arc is
NON-NEGOTIABLE: it is whatever the add-on exported, and the discovery reads it off the FILENAME and
verifies the members carry the same stem (`custom_content::body_model_present` /
`stage_parts_from_members`). The human-readable name is therefore free: the arc's parent FOLDER
(`Peter Griffin/`), ASCII upper-cased, ≤ 15 bytes (the scalar row's SSO budget, §4.2 of the options
design). A flat arc falls back to the stock key rule with `_` → space.

**What the A3 proof of concept needed that World does not.** On A3 the game itself walked
`chara_resources.rlist` / `map_resources.rlist` from `startup.arc`, so adding `peter00` / `griffin00`
meant repacking `startup.arc` with the rows (`peter00 → [pl, M, A, 1.0, 0.8, 0.0]` at chara row 1,
`griffin00 → [000000, 000000, room, footpanel]` at map row 34 with the `boom00` camera set on camera row
34). On World the DLL is the only reader of those lists, so the rows are optional SIDECARS beside the
arc — the same three filenames, binary MRL0 or a `.rlist.txt` twin — and the defaults reproduce the A3
rows without them: dancer `M, A, 1.0, 0.8` (the modal stock male row), stage parts = every
`gm_<key>_<part>.model` member of the arc (the shipped living room: `room` alone — no `:N` priority, so
it draws in the opaque pass with the default sort). The A3 PoC also carried a `footpanel` part, the stock
`gm_boom00_footpanel` dance pad copied in for the lesson demo — stock World stages carry `footpanel` ONLY on
the lesson-only `boom00` row 32, never in normal play, so it was dropped from the shipped folder
(cabinet test #2, 2026-09-22: deleting `gm_griffin00_footpanel/` was the whole fix; parts are derived from
the folder, the cache arc repacked itself), camera set = the arc's own `*.camanm` members if
any, else stock camera row 0 (`boom00`'s `st001_*` set — exactly what the A3 test assigned). The `_g`
arc (`mapset_<key>_g.arc`, the gold-cabinet lesson-demo variant) is ignored: the World mod loads
`mapset_<key>.arc` only.

**Folders, not arcs (maintainer 2026-09-22).** Users never pack an arc: a model is a FOLDER
(`pl_<key>/`, `mapset_<key>/`) holding the add-on's flat export, or a literally unpacked arc. The scanner
maps every file to the member path the engine expects (`custom_content::folder_member_path` — body/part
files → `data/chara/<folder>/<rel>`, stage files → `data/map/<rel>`, a stage's `*.camanm` → 
`data/camera/<key>/<basename>`, anything already under `data/` verbatim) and packs the folder with
`core::arc::ArcArchive` (uncompressed, 64-byte aligned — the LayeredFS `arc_handler` repack shape, which
the engine's FileManager has read since the first mod arcs) into
`data_mods/_cache/custom_models/<name>-<fnv1a8(source path)>.arc`, behind a `CacheHasher` fingerprint of
the member paths + source mtimes (a later boot only stats). The engine never learns the difference: a
mount points the logical `data/arc/pl_<key>.arc` at the cache arc. The 2.2 + 4.6 MB PoC pack costs one
~50 ms pack on the first boot and two `stat` sweeps afterwards.

**How the arcs reach the engine.** `scene3d::arc_set` grew a MOUNT registry (`mount(game_rel, fs_path)`):
`resolve()` checks it before the LayeredFS override and the stock file, and reports a mount as
`Resolved::ModOverride`, so `load()` hands the FileManager the filesystem path (the same path the
LayeredFS override case already used — the FileManager opens through the CRT, §4.2.1 of the design) and
`read_bytes()` / `resolve_path()` (the parse thread, `parts_present`) see the same file. Nothing
downstream of `Pick::arcs_for` changed. Mounts never shadow stock: the planner refuses a key that exists
in the stock tables or an earlier folder (one WARN), so the stock candidates are byte-identical with the
toggle on or off.

**Row indices.** Custom stages get `row = len(map_resources) + i` with their camera row placed at the
same index (the row-parallel invariant `assemble_pick_opt` relies on: `camera_rows.get(stage.row)`);
custom dancers `row = len(chara_resources) + i`. The catalog keeps the stock block first (sorted by key,
unchanged) and appends the custom block sorted by label, so a cached option value keeps meaning the
same stock entry, and with the toggle OFF a custom value clamps to RANDOM at load.

**Camera clips of a custom stage — and why the first cabinet test filmed the house from outside.**
`parse_pick` looks each camera name up in the STAGE arc first (any `<name>.camanm` member, any directory)
and only then in `camera/stage_camera.arc`'s `long/<name[..5]>/<name>.camanm` layout, so a stage ships
its own shots. The A3 PoC could not: A3's lesson demo ran the stage under the SONG camera set
(`camera_music_lesa.arc` — `lesa` = the lesson song's mcode; one 6238-frame `music_lesa.camanm` with the
four shots baked in as hard cuts, `examples/stage_camera.py`), a mode the World mod does not have (stage
mode only — a LIST of short clips, stock 360–450 frames each, main clips cycled, `_non` clips cut away at
dance changes, `schedule::CameraSchedule`). Without a stage set the planner borrowed `boom00`'s
`st001_*` shots, whose 3–5 m orbit lands in the living-room walls. `scripts/split_camanm.py` converts the
song-camera authoring into stage clips: `--shot name:F0-F1` slices the long clip, re-times the shot to
`--frames` (450 = the stock cadence) by game-equivalent sampling (`anm_dump.sample_track`: linear on
translation/FOV, slerp on the rotation quaternion; the single-key near/far/aspect slots copied), writes
through `write_anm` and re-parses the output to verify every sample (max error 1.5e-5 on the Griffin
shots). The four resulting `griffin_st01..04.camanm` live in the stage folder's `camera/`; a `_non`
suffix would make one a cut-away. Song-camera sets (`camera_music_*.arc`) stay unread — per-song cameras
are a possible follow-up, not a stage property.

**Discovery cost.** The scan runs once at enable on the enabling thread: per model folder a directory
walk + fingerprint stat (a full read + pack only when stale), per ready arc a 64 KiB prefix read (header
+ cue table + string table sit at the front; the whole file is read only when the string table runs past
the prefix), per directory the sidecar files.

## 7. Background movies — where World draws a gameplay movie (2026-09-22, 20260825 + 20250805)

Drives the GLOBAL SETTINGS row **Background Movies** (`background_dancers.movie_mode` = `off` /
`thumbnail` / `fullscreen`, `mods/background_dancers/movie_mode.rs`). The headline: **a FULLSCREEN
gameplay movie is already drawn UNDER the 3D model passes** — the "movie songs render mostly black"
observation of §3.4 (deploy #2) was not the 2D hide covering the movie plane, it was the 3D STAGE painted
over it. So "dancers in front of a fullscreen movie, no stage" (the DDR 5th Mix look) needs no compositor,
no detour and no new pass: leave the movie fullscreen and do not draw the stage.

### 7.1 The movie actor and its two layers

- `SceneManageActor` (vftable `0x180363408`, RTTI `.?AVSceneManageActor@dance@sequence@@`) is created at
  DPS step 2 (`FUN_180057e10` → ctor `FUN_18007d480(this, DPS+0xA0 basename, DPS+0xC8 movie path,
  courseFlag, movie_size, &rect)`) and added as a DIRECT CHILD of the DPS (`FUN_18021f230` = the actor
  tree's add-child: `child+0x08` parent, `parent+0x18` first child, `child+0x10` next sibling).
- Its onInitialize `FUN_18007d700` looks the song up (`FUN_1801b3fa0(basename)`, the basename twin of the
  derived `find_music_by_mcode` = `FUN_1801b3f30`; same entry vector `DAT_1806f2d80`) and, when the entry
  has a movie (`+0x141 ∉ {5,0}`, or `+0x141 == 5` and `+0x140 ∉ {5,0}`): movie size 1 ⇒ message `0x1011`
  to the DPS; size ∉ {1,2} ⇒ **return, no MovieActor** (VIDEO SIZE OFF). Otherwise it creates the 0x150-byte
  `MovieActor` (vftable `0x180363398`, RTTI `.?AVMovieActor@dance@sequence@@`; ctor `FUN_18007c960`) with
  `+0x148 = (movie_size == 2)` — the THUMBNAIL flag — stores it at `SceneManageActor+0xD8` and adds it as a
  child. **20250805 has no size-3 early return** (`FUN_1800799f0`: the 0x1011 send, then the MovieActor
  unconditionally) — so writing VIDEO SIZE OFF alone is not "no movie" on that build; the OFF mode's
  BuildGraph suppressor is the backstop.
- `MovieActor::onInitialize` `FUN_18007cc20`: empty path (`+0xD8` std::string size `+0xE8` == 0) ⇒ step 4;
  else copies the entry's movie offset (`+0x144 → +0x140`), creates the `agcs::Movie` wrapper
  (`FUN_180216c70(player, path, 4)` → `+0x138`) and registers it into the LAYER TABLE (`FUN_18007cf90`):
  `entry = *(DAT_1806f2d20 + 8 + slot·0x18)` with **slot 9 for fullscreen, slot 0 when `+0x148` (thumbnail)**
  (20250805 `FUN_180079280`: identical), `movie+0x0C = 0x7FFFFFFF` (draw priority), tail-appended to the
  render-list manager's active list (the widget_renderer node shape).
- StackStep (`+0x58` values / `+0x82` index — the `agcs::Actor` shape): onUpdate `FUN_18007d160` step 0
  waits for the Movie impl's opened flag (`*(*(+0x138)+0x18)+0x0C`), then state `+0x10 == 1` ⇒ step 1 (ready)
  else step 4 (no movie); message `0x1044` (the song anchor) 1 → 2; `0x1045` with count ≥ offset ⇒ play,
  2 → 3. Identical on 20250805 (`FUN_180079450`).
- `SceneManageActor::onUpdate` `FUN_18007d850` step 0 returns while the MovieActor is at step 0; then, if
  the MovieActor's step ≠ 4 and movie size == 1, `FUN_180031e70` — resets the BgMovieActor's
  MoviePlayerFrame and sends `0x1006` (disable) to the BgMovieActor and its children, i.e. the whole
  `BackgroundFrame` (bg_root) is switched off for a fullscreen movie; otherwise the usual gameplay
  background context (`+0x2D0/+0x378 = 0`, `+0x128 = 1`, `FUN_180031f60`, `FUN_180031d20`). Its message
  handler `FUN_18007d970` answers the DPS step-3 readiness poll `0x1001` NOT READY until its own step 2 —
  so by DPS step 5 (the dancers' visibility edge) the movie is settled.

### 7.2 Where each layer lands in the frame

- Layer table (`DAT_1806f2d20`, 11 × `{override, layer, list_index}`, built by `FUN_18002aab0`; the
  overlay-draw service's derived `layer_table`): list indices `[1,0,0,0,0,0,3,3,1,4,5]`; entries 7–10 get
  PRIVATE command lists `DAT_1806f1620[idx]` — entry 7 → 3, 8 → 1, **9 → 4**, 10 → 5 (20250805 same table).
- The 8 ScreenCommandList viewports (`FUN_1801f6dc0`, names in order FRONT, MIDDLE, BACK, SYSTEM,
  OFFSCREEN0, OFFSCREEN1, DEBUG_DIALOG, RENDER_CAPTURE; viewport `i` at `DAT_1806f1568 + i·0x18`, command
  list `i` at `DAT_1806f1620 + i·8`). Render-graph boot `FUN_1801f2c30` (20250805 `FUN_1801da730`, same
  shape): **RENDER-3D = {prio 0x64 `DAT_1806f2f18+8`, 0x65 OFFSCREEN0, 0x66/0x67/0x68 MODEL passes, 0x6D
  `DAT_1806f2f18+0x188`}**; RENDER_2D = {0x65 BACK, 0x66 MIDDLE, 0x67 FRONT}; DISPLAY = {0x65, 0x66 SYSTEM}.
  So layer entry 9 (the fullscreen movie) → command list 4 → **OFFSCREEN0 → RENDER-3D ahead of the MODEL
  passes**; entry 0 (the thumbnail movie) has no override → the ordinary 2D lists, above the 3D.
- Depth: the ScreenCommandList viewport's render (`FUN_1801f05c0` → `FUN_180267480`) emits gd tag `0x11`
  records `id 2 = 0` (ZENABLE false), `id 3 = 0` (ZWRITEENABLE false), `id 0 = 0` (cull none) before the
  list (gd executor `FUN_18024d650` case `0x11`: `id = (v << 23) >> 24` → 0 CULLMODE, 1 SCISSORTEST,
  2 ZENABLE, 3 ZWRITEENABLE, 4 ALPHATEST, 5 STENCIL, 6 TWOSIDEDSTENCIL, bit 0 = value); every viewport's
  setup `FUN_18026cec0` re-enables Z (`id 2 = 1`, `id 3 = 1`) for the next one. The movie quad writes no
  depth, the RENDER-3D list clear ran before it, so the model passes draw over the movie with a clean
  Z-buffer — the dancers stand in front of it by construction.
- The movie draw itself (`agcs::Movie` vftable `0x180389678`, slot 5 → `FUN_180216930`): into the ACTIVE
  command list (`*(DAT_1806f2fb8 + 0x40 + *(DAT_1806f2fb8+0x68)·8)`): tag 0x12 (the movie texture), tag 8
  blend, one tag-5 6-vertex quad at the marker rect, tag 8, tag 0x10 — only once a frame was delivered
  (`+0x4C`), which is why a faked-open player draws nothing.

### 7.3 What the three modes do

| Mode | Window entry (`movie_size::apply`) | Per song |
|---|---|---|
| OFF | every movie-showing side (0/1/2) → 3; `MovieSuppressor::BackgroundDancers` set (cleared at window exit) | stage as usual; no MovieActor on 20260224+; 20250805's MovieActor is faked open and draws nothing |
| THUMBNAIL | 0/1 → 2 (the original rule) | stage as usual; the thumbnail is a 2D layer above the 3D |
| FULLSCREEN | 2 → 1 (0/1 already fullscreen) | `movie_backdrop::probe` each visible frame: live DPS → SceneManageActor child → MovieActor child → step; step 1–3 ∧ no live suppression ∧ `movie_policy::last_build() == RealOpened` ⇒ **Active**: stage parts (+ their hull twins) and floor shadows published hidden, 2D bg hide disarmed (the game disabled the frame). No MovieActor / step 4 / faked ⇒ stage shown |

The OFF mode's value 3 is exactly what a player with VIDEO SIZE OFF gets (the maintainer's 2026-09-16
cabinet run: "ON/OFF: no override lines, movie as configured" with dancers). FULLSCREEN falls back to
THUMBNAIL (one WARN) when the movie-size override or the probe (the two RTTI vtables, the DPS identity
gate, the BuildGraph hook) is unavailable. Both RTTI names resolve on 20250805 / 20260224 / 20260721 /
20260825 / 20260915 (`validate_signatures.sh`, 2026-09-22). Correction to §3.4: "the 2D alpha-0 hide
covers the movie plane too" is wrong — bg_root never covers a fullscreen movie (the game disables it);
the stage geometry did.

### 7.4 The movie camera set (2026-09-22)

Behind a fullscreen movie the stage cameras are the wrong tool: the stock sets frame a room (survey of all
87 stock `stage_camera.arc` clips: most at 5–17 m, visible height at the dancer 4–12 m, the figure 15–40 %
of the frame), and the Griffin House set spends three of its four shots behind the dancers (eye `z < 0`;
the dancers FACE +Z — the bind toes sit at `z = +0.098` relative to the feet) plus one resampled shot whose
quaternion stream swings through the ceiling. `scripts/gen_movie_cameras.py` authors a set for the
stage-less scene instead, and `lifecycle::camera_tick` switches the camera director to it while the scene
mask is DANCERS_ONLY.

Framing limits measured from the stock choreography (every A3 clip of both sexes, the tu01 exception
excluded, sampled every 15 frames, `pl_emi00` ×0.9 / `pl_afro00` ×1.0): hips travel x ±0.55–0.75 m and
z −1.1…+1.1 m (the dancers walk towards and away from the camera), the highest point (hands up) reaches
1.68 / 1.81 m at p99, the head top 1.46 / 1.57 m at p50, and up to ~10 % of the poses face away. Keeping the whole
figure inside the HUD-safe frame (|x| ≤ 0.94, −0.80 ≤ y ≤ 0.80 NDC) for 96 % of the poses therefore needs a
visible height of ~2.5 m at the look target with the target at 0.76–0.82 m (not the obvious mid-body 0.9 —
the feet are what leave the frame first), i.e. the figure reads 55–62 % of the frame height; a medium shot
(head + hips kept, feet free) is ~1.7 m / target 1.05 m, figure ~85–90 %. The set mixes both (solo: 8 full
+ 4 medium main clips), every main shot within ±80° of the front, upright, in-game hFOV 46–54°. Two-dancer
variants (`_2p`, dancers at ±0.8 m) need ~2.7–3.3 m and drop the mediums and the deep side angles (the pair
would line up in depth). The Blender add-on's `load_camanm` (in-game-verified projection) reproduced the
intended framing on every clip.

## 8. Movies on the stage monitors — A3's OFFSCREEN1 route (2026-09-23, A3 `gamemdx_20240402` + World 20250805…20260915)

The question: how A3 played a song's background movie on the screens INSIDE some 3D stages (ENDYMION on
`replicant05`), and what it takes to put any song's movie on those screens in the World revival. Static RE
only (Ghidra on A3 final + World 20260825, capstone sweeps over the five World builds, a parser survey of
every A3 stage arc) — nothing here is cabinet-tested yet. **Implemented 2026-09-23** as Background Movies
= STAGE SCREENS (+ MOVIE ONLY (NO DANCERS)) — §8.8.

**Headline: there is no per-stage or per-song screen code.** A3 draws the movie into a dedicated 1280×1280
render target that the engine publishes at boot as the NAMED TEXTURE `offscreen1`; the screen meshes of the
`monitor*` / `replicant*` stages simply have a material whose texture is named `offscreen1`, so the model
converter binds the render target exactly like any DDS. A song only decides WHERE its movie draws: movie type
3 ⇒ into that offscreen target instead of the screen. **World kept every piece except the routing**: the
render target, both names, its command list, its render-graph slot and the converter binding are unchanged;
only the MovieActor's layer choice went from A3's `monitor ? 10 : 9` to World's `thumbnail ? 0 : 9`. Nothing
ever draws into OFFSCREEN1 on World, so a revived monitor stage should currently show BLACK screens (the
target list clears the RT to `0xFF000000` every frame) — a static prediction, see §8.7.

### 8.1 The render target and its two names (A3 ≡ World)

| Piece | A3 `gamemdx_20240402` | World 20260825 |
|---|---|---|
| Display ctor: `+0xDC = create(0x500, 0x500, fmt 0x15 A8R8G8B8)`, `+0x10C = texture view(+0xDC, 0x804)`, `register(+0x10C, "OFFSCREEN1")`; render-target object `display[0x12]` (u16 dims `0x0500_0500`) bound to `+0xDC`; target list `display+0x08` "OFFSCREEN1": clear flags 1 (colour only), colour `0xFF000000`, clear-at-start bit, render prio **0x65** (first target list of the frame; RENDER-3D is 0x66) | `FUN_180131f80` | `FUN_1801f10e0` |
| Render-graph boot: `register(display+0x10C, "offscreen1", 0)` (the lowercase alias), then attach ScreenCommandList viewport 5 (OFFSCREEN1) into `display+0x08` at 0x65 | `FUN_180133c30` (viewport `DAT_1802ed7c0`) | `FUN_1801f2c30` (viewport `DAT_1806f15e0`) |
| ScreenCommandList viewport table (8 lists; OFFSCREEN1 = list 5, **0x500 × 0x500** — the coordinate space the movie is fitted in) | `FUN_180137e70` | `FUN_1801f6dc0` |
| Named-texture register | `FUN_1801472f0` | `FUN_1802036d0` |
| DDS file callback (texture from a `.dds` member) | `FUN_18014c560` | `FUN_1802088e0` |
| Teardown releases `"offscreen1"` by name | `FUN_1801340b0` | (`FUN_1801f2170`, see `docs/custom_shader_backgrounds_research.md` §5.3) |

Register semantics (`FUN_1802036d0`): the key is the ResourceManager name hash (lower-case, `_` stripped,
then the gs hasher — `FUN_180202340`; KTMDL texture names are stored already folded, so a model's
`offscreen1` hashes to the same key). On a NEW key it inserts into the RM map at `RM+0xB8` AND
(`FUN_18026ff00`) pushes a `TextureData {u32 hash, u32 handle, u16 w, u16 h}` into the gs texture registry
`*DAT_1806f3290` (clearing its sorted flag) — the exact registry the model converter's lookup
`FUN_18026f9e0` binary-searches when it fills a model's texture table (§2.1). On an EXISTING key it only
bumps the refcount. The DDS callback creates its texture, calls the register, then releases its own
reference — so **the first registration of a name wins**: `offscreen1` is registered at boot, a stage arc's
own `offscreen1.dds` (§8.2) is decoded and immediately discarded, and every material naming `offscreen1`
resolves to the render target at conversion time (the §2.1 "resolved at load" case — nothing for the DLL's
re-resolve to do).

### 8.2 Which stages sample it (survey of every A3 stage arc)

10 of the 26 stock `mapset_*.arc` (A3 and World ship the same 26; `mapset_monitor00.arc` is byte-identical
between the two installs) have at least one material whose texture is `offscreen1`: `mapset_monitor00..03`
and `mapset_replicant00..05`. The other 16 (boom00, boom00_g, boom01–06, club00, crystaldium00, cyber00,
dawnstreet00, disco00, floor00, lovesweets00, speaker00) have none, nor does the `griffin00` PoC. Each of the
ten carries a placeholder `offscreen1.dds` (512×512 A8R8G8B8, a coloured test image, identical bytes in every
arc) in exactly ONE part directory — an authoring stand-in (so the exporter/viewers had a texture), never
bound in game (§8.1). Its presence is therefore a free header-only test: *"arc lists an `…/offscreen1.dds`
member"* ⇔ *"stage has screens"* holds on all 26 stock arcs.

| Stage (rlist rows) | Parts whose meshes sample `offscreen1` | Shader | Screen UV band u / v |
|---|---|---|---|
| `monitor00` (18, 24) | `monitor` mesh 2 (228 v) | `mdl_ch_constant_vc` | 0.000–1.000 / **0.125–0.874** (exactly the 4:3 fit band) |
| `monitor01` (1) | `monitor1` (4 v), `monitor2` (120 v), `monitor3` (24 v) | `mdl_ch_constant_vc` | 0.000–1.000 / 0.200–0.800 |
| `monitor02` (22), `monitor03` (23) | `bg` (355 v); `monitor` meshes 0 (10 v, two-sided) and 1 (1170 v) | `mdl_bg_constant_vc` | bg 0.007–0.993 / 0.254–0.747; monitor 0.007–0.995 / 0.130–0.857 |
| `replicant00..05` (19–21, 25, 26, 33) | `bg` (29 v); `monitor1` (16 v, ALPHA-blended, vertex α 0.8); `monitor2` mesh 0 (380 v opaque) + mesh 2 (the same 380 v as an ADDITIVE glow copy) | `mdl_bg_constant_vc` | ≈0.005–0.995 / ≈0.01–0.99 (the whole square); monitor1 v ≈0.13–0.87 |

Every screen is unlit `*_constant_vc` (texture × vertex colour); the opaque ones are alpha-tested at 127, so
the RT's alpha must stay ≥ 0x80 wherever the movie is drawn (the clear writes 0xFF; the movie quad draws with
blending over it).

### 8.3 How A3 routes a song's movie — the only code involved

- **Data.** musicdb `<movie>` (u8 type) and `<bgstage>` (u16 stage row). A3's music entry keeps the type at
  `+0xB1` (with `+0xB0` as the fallback when `+0xB1 == 5`; 5 or 0 = no movie). A3 musicdb census (1221
  songs): type 1 ×141, 4 ×100, **3 ×43**, 2 ×2, 5 ×1, absent ×934.
- **`SceneManageActor::onInitialize`** (A3 vtable `0x18026b868` slot 4, `FUN_180060090`): the monitor flag
  `+0x120` = **movie type == 3** (forced off in one special case: game mode `*(*DAT_1802ed6d0 + 0x14) == 13`
  with a `litp_w` / `<name>_sel` select-movie present, `FUN_18005f100` — that path passes `+0x119` instead).
  It creates the 0x120-byte MovieActor `FUN_18005f370(this, basename, movie path, monitor flag → +0x118,
  select flag → +0x119)` (vtable `0x18026b4b8`).
- **`MovieActor::onInitialize`** (`FUN_18005f5f0`): resolves the file (`FUN_18005f740`:
  `data/mdb_apx/movie/<movieoverride | basename>` + `_w`, then no suffix, `_vj`, `_m`, `.wmv`), copies the
  entry's type (`+0x114`) and offset (`+0x110`), creates the `agcs::Movie` (`FUN_18015d6a0(…, 4)` → `+0x108`)
  and registers it (`FUN_18005f960`) into **`layer_table[+0x118 ? 10 : 9]`** (`DAT_1802eee58 + 8 +
  (flag·3 + 0x1B)·8`, draw priority 0x7FFFFFFF — the World shape of §7.1).
- **Layer table** (A3 `FUN_180023730`, World `FUN_18002aab0`, same list indices `[1,0,0,0,0,0,3,3,1,4,5]`):
  entries 7–10 own PRIVATE ScreenCommandLists — **entry 9 → list 4 OFFSCREEN0** (RENDER-3D prio 0x65, under
  the model passes, §7.2), **entry 10 → list 5 OFFSCREEN1** (the offscreen target, drawn before RENDER-3D in
  the same frame, so the screens show the current frame's movie image).
- **Per-frame fit** (A3 msg `0x1048` in `FUN_18005fba0` — A3's anchor/per-frame ids are 0x1047/0x1048, World
  shifted them to 0x1044/0x1045): `FUN_18005f9f0(this, fitW, fitH, boxW, boxH)` = scale `s = min(fitW/movieW,
  fitH/movieH)`, size `movie·s`, position `(box − size)/2`. Monitor movies use **fit = box = (1280, 1280)**
  (`DAT_180288894 = 1280.0`): aspect-preserved and centred in the square — a 4:3 movie lands in v ∈ [0.125,
  0.875] (monitor00's band exactly), a 16:9 one in [0.219, 0.781], a square one fills the RT. (Screen movies:
  type 4 or an HD cabinet — `FUN_180011c30`, machine type ∉ {0,1} — fit `(1280, 720)`; otherwise the SD 4:3
  fit `(960, 720)` pillarboxed in `(1280, 720)`.)
- **`SceneManageActor::onUpdate`** step 0 (`FUN_180060460`): the StageActor (`FUN_180061f30(row)`, row from
  entry `+0xE5`, `+0xE4` when negative) is created only when there is **no MovieActor, the MovieActor ended at
  step 4 (no file), or the monitor flag is set**; the two CharaActors are created in step 1 only if a stage
  exists. So on A3 an ordinary movie song showed the movie INSTEAD of the 3D scene, and a type-3 song showed
  the full scene with the movie on the screens.
- **The stage pairing is pure data.** All 43 type-3 songs carry a `<bgstage>` in {18–26, 33} = monitor00
  (18, 24), replicant00–02 (19–21), monitor02 (22), monitor03 (23), replicant03/04 (25/26), replicant05 (33),
  and no other song uses those rows. ENDYMION (`endy`, mcode 38149) = type 3 + bgstage 33 = `replicant05`;
  its `endy.wmv` is **512×512** — a square movie made for replicant05's full-square screens (Eon Break, the
  other row-33 song, is 1280×720). The rest are mostly 640×480 (4:3 — monitor00's band, and roughly the
`monitor` meshes of monitor02/03), a few
  640×360 / 852×480 / 480×360 / 448×336 / 320×240. Two use `_vj` "VJ" movies via `<movieoverride>`
  (`umu/miku_vj.wmv`, `umu/miso_vj.wmv`).

### 8.4 What World changed

- **Layer select** (World `FUN_18007cf90`; 20250805 `FUN_180079280`): `CMP byte [RCX+0x148],R8B ; MOV
  EAX,9 ; CMOVNZ RAX,R8 ; LEA RAX,[RAX+RAX*2] ; MOV RDX,[RDX+RAX*8+8]` = `thumbnail ? 0 : 9`. **Entry 10
  is unreachable.** The only fixed-index reference to entry 10 (`[layer_table + 0xF8]`) on 20250805,
  20260825 and 20260915 is the table builder's own init (20260825 `0x18002ac68` in `FUN_18002aab0`; capstone
  scan of every `MOV r64,[rip+layer_table]` followed by a `[r+0xF8]` operand). Computed-index users (AFP
  layers created by layer id) cannot be ruled out statically.
- World **kept the type byte** — its musicdb still flags the same 43 songs `<movie>3` (entry
  `+0x141/+0x140`, copied to `MovieActor+0x144`, read by no MovieActor method) — but **dropped `<bgstage>`**:
  none of its 1484 songs carries one.
- **Fit** (`FUN_18007d250` case 0x1045 → `FUN_18007d030(this, &size, &origin)`) is the generalised A3 fit:
  `size` = f64 `(w, h)` at `MovieActor+0x120/+0x128`, `origin` = f64 `(x, y)` at `+0x108/+0x110` (ctor
  `FUN_18007c960` copies both from the SceneManageActor's marker rect), movie pixel dims = f32 at
  `*(*(MovieActor+0x138) + 0x18) + 0x24/+0x28`; `s = min(w/mw, h/mh)`, position `origin + (size −
  movie·s)/2` (rounded), re-applied every frame while playing. Size `(1280, 1280)` at origin `(0, 0)` is
  A3's monitor fit bit-for-bit. (Field offsets read on 20260825 only — derive and sweep before use.)

### 8.5 The routing signature (swept 2026-09-23; `movie_layer_select` in `signatures.rs`, §8.8)

`44 38 81 48 01 00 00 B8 09 00 00 00 49 0F 45 C0 48 8D 04 40 48 8B 54 C2 08` — `CMP [RCX+0x148],R8B` (the
thumbnail flag, same displacement on every build) through the table load; **unique and byte-identical on all
five builds**; the entry imm is the byte at **match+8** (`09`); the layer-table global is the `MOV
RDX,[rip+disp32]` 0x1B bytes before the match.

| Build | Match |
|---|---|
| 20250805 | `0x1800792a2` |
| 20260224 | `0x1800783e2` |
| 20260721 | `0x18007cbd2` |
| 20260825 | `0x18007cfb2` |
| 20260915 | `0x18007d122` |

(One-off capstone sweep, then `./scripts/validate_signatures.sh` ALL GREEN with the signature added. The
fit-field offsets of §8.4 have their own AOB, `movie_actor_fit_case` — the 0x1045 case's
`CMP [..+0x58],2; JNZ; MOVUPS XMM0,[RCX+origin]; MOVSD XMM1,[RCX+origin+0x10]; …; MOVUPS XMM0,[RCX+size];
…; MOVSD XMM1,[RCX+size+0x10]` with the d32s at +14 / +22 / +44 / +58 = 0x108 / 0x118 / 0x120 / 0x130 on
all five builds, matches 0x180079570 / 0x1800786b0 / 0x18007cea0 / 0x18007d280 / 0x18007d3f0.)

### 8.6 Reusing it in the revival (the proposal — implemented, see §8.8 for what shipped)

Per song, scoped to the GAMEPLAY window like the existing Background Movies modes (§7.3):

1. **Know the stage has screens — at window entry.** The layer choice happens in `MovieActor::onInitialize`
   (DPS step 2, scene 28), before the parse thread is guaranteed to be done, so decide from the pick at the
   25→26 edge: the `offscreen1.dds` member test (header only, §8.2) or a precomputed per-stage flag. Custom
   stages opt in by naming the screen image `offscreen1` in Blender — the add-on derives the KTMDL texture
   name from the image stem (`export_model.sanitize_texture_stem`; case and `_` fold away) and writes an
   `offscreen1.dds` next to the part, which is harmless (§8.1 first-wins) and makes the header test work.
2. **Route.** Force VIDEO SIZE = FULLSCREEN (1) for the window (`movie_size.rs` — keeps `+0x148 = 0`; with
   size 1 the SceneManageActor also disables the 2D BackgroundFrame, §7.1, which is right with a stage) and
   write the layer-select imm `09 → 0A` for the window (game thread, restored at window exit and at disable;
   the imm is read once per song by that one call). The Movie then registers into entry 10, draws into
   OFFSCREEN1 at prio 0x65, and RENDER-3D samples it in the same frame.
3. **Frame it.** When the `movie_backdrop` walk (§7.3's probe) finds the MovieActor at step ≤ 2, write origin
   `(0, 0)` / size `(1280, 1280)` — A3's exact result (bars on screens whose band is not the movie's aspect,
   e.g. a 16:9 movie on monitor00). Optional per-stage "cover": for band `[v0, v1]` (u spanning the RT width) and movie `mw × mh`, `s =
   max(1280/mw, (v1 − v0)·1280/mh)`, size `(mw·s, mh·s)`, origin `(640 − mw·s/2, 640·(v0 + v1) − mh·s/2)` —
   the fit then reproduces `s` and crops the overflow at the RT edge. A pure, host-testable function of
   (movie dims, stage band).
4. **Keep the stage.** Scene mask FULL (not FULLSCREEN mode's DANCERS_ONLY), stage camera set.
5. **Restyle exemption.** `render_item_layout::restyle_eligible_materials` (blend group 0) would re-point the
   OPAQUE screen materials at `_lit` / `_cel` (§4.7) and build hull twins over them — N·L shading, cel bands
   and ink rims on a video. Exempt every material whose texture is `offscreen1` (keep the stock unlit
   `*_constant_vc`) and give it no hull records.
6. **Movie-less songs.** A3 never put a monitor stage under a song without a monitor movie; with the RT
   unused the screens stay black. Choices: drop the ten screen stages from the random pool when the song has
   no movie (A3-faithful), accept black screens, or later draw something else into entry 10 / list 5 (e.g.
   the jacket through `overlay_draw`).
7. **Interplay.** A faked or suppressed movie (`movie_policy`: song rate without sync, the Wine suppress
   mode) draws nothing ⇒ black screens — gate the route on the same `movie_policy::last_build() ==
   RealOpened` test the FULLSCREEN probe uses, falling back to the non-monitor behaviour. `movie_sync` and the
   rate clock proxy are unaffected (same graph; only the draw list changes). Custom resolution rescales
   OFFSCREEN1 and its viewport to `render_w²` (`rt_dims_square` + the square viewport pair,
   `docs/custom_resolution.md`); the screens sample by UV so the RT size is transparent, but whether entry
   10's 2D canvas follows that viewport (the fit's 1280-unit coordinates) is unverified — test at stock
   1280×720 first. The 1280² RT is already cleared every frame on every World boot (§8.1), so the feature
   adds only the movie quad.

Rejected alternatives: re-linking the Movie's node from entry 9's render list into entry 10's after
onInitialize (no code patch, but intrusive surgery on the manager's node pool — free list `+0x18/+0x20`,
count `+0x3C`, active head/tail `+0x28/+0x30` — with an unexamined removal path); drawing the movie ourselves
into list 5 (needs the movie's texture object and a quad emitter, for no gain over the game's own draw).

### 8.7 To confirm on the cabinet

1. A revived monitor/replicant stage shows black screens today (the §8.4 prediction; a non-black screen
   means something else draws into entry 10).
2. With the imm patch + rect write: the movie appears on the screens, alpha-tested screens stay opaque.
3. ENDYMION on `replicant05` fills the square screens; a 4:3 song on `monitor00` fills its screen exactly.
4. A 1080p custom-resolution run keeps the same framing.
5. The per-item INFO names the screen material's bound texture as 1280 × 1280 with hash `0x3420C1B9`
   (FNV-1 of `offscreen1`) — the render target, not the arc's placeholder DDS.

### 8.8 What shipped (2026-09-23; code `mods/background_dancers/{movie_mode,screen_route,lifecycle}.rs`)

- **Row.** Background Movies gains **STAGE SCREENS** (row value 3, key `stage_screens`) and **MOVIE ONLY (NO
  DANCERS)** (4, `movie_only`); display order OFF / THUMBNAIL / STAGE SCREENS / FULLSCREEN / MOVIE ONLY; the
  old values keep 0/1/2. Default THUMBNAIL at first; STAGE SCREENS became the default after the cabinet pass
  the same day (it plays as THUMBNAIL on every stage without screens).
- **Has screens** (§8.6 step 1): decided once at mod enable, per distinct stage key, from the arc the engine
  will load (`arc_set` — custom mounts, LayeredFS, stock): a member whose file name is `offscreen1.dds`.
  Songs without a movie keep the screen stages in the rotation (black screens, maintainer decision — the
  A3-faithful "drop them" alternative of step 6 was not taken).
- **Route** (step 2): `derive_movie_screen_route` publishes `movie_layer_select_imm` (+ the two fit
  offsets). DEVIATION: the byte is armed at the 25 → 26 window entry BEFORE the VIDEO SIZE write, and a
  refused checked write (the byte not `09`) turns the song into THUMBNAIL before anything else is written,
  so a song can never end up with a fullscreen-size movie under a full stage. Restored (checked `0A → 09`) in
  the window-exit branch of the scene callback and at mod disable. A song where no entered side's VIDEO
  SIZE shows a movie is not routed at all (20250805's SceneManageActor builds a MovieActor even for VIDEO
  SIZE OFF, which the patch would put on the screens; residual edge: 2P on 20250805 with the GOVERNING side
  OFF and the other ON). The step-7 `RealOpened` gate was NOT
  added: a routed movie that fails or is faked draws nothing into the RT, which shows the same black the
  thumbnail-less stage would.
- **Fit** (step 3): A3's contain fit, written per MovieActor instance every frame while its step is 0 / 1 /
  2. DEVIATION: the writer runs from the mod's frame callback, not the scene driver (`drive_live` returns
  early until the scene is built, and the fit must land before the 2 → 3 transition). No per-stage cover.
- **Stage kept** (step 4): scene mask ALL, stage camera set.
- **Restyle exemption** (step 5): ALWAYS, in every mode and style (previews included) — a material whose
  masked slots index the texture-table entry hashed FNV-1(`offscreen1`) keeps its stock shader, and its hull
  twin records are hidden (no outline). The KTMDL texture name is a 6-bit packed lowercase/digit string
  (20260825 `FUN_180275ad0`) hashed by the gs hasher (`DAT_1806f2040`, the same FNV-1 as shader names) at
  `FUN_180273e20`, so the table key is computable.
- **Diagnostics** (added): the enable INFO listing the stages with screens; the per-song INFO `stage screens:
  yes/no, routed: yes/no`; the imm write; one INFO per framed MovieActor with the movie's pixel size; a
  one-shot INFO of layer entry 10 (override, layer, list index, walk gate `+0x10/+0x12`, active node count
  `+0x3C`); per built item with screen materials the bound `TextureData` size + hash.
- **MOVIE ONLY** (added, A3's default for ordinary movie songs, §8.3): VIDEO SIZE untouched; the FULLSCREEN
  probe (§7.3) runs, and while the backdrop is not None every 3D instance is published hidden (`SceneMask::
  NOTHING` — the mask gained a `dancers` field) and the 2D background is left to the game.
- **Custom stages**: the Blender add-on writes an 8 × 8 black `offscreen1.dds` for an image whose stem folds
  to `offscreen1`; the shipped Griffin House TV maps the 16:9 band of the square (D3D v 0.21875–0.78125,
  unmirrored u) — a 16:9 movie fills the TV with a ~3 % horizontal squeeze onto its 1.72:1 panel, a 4:3 one
  is cropped 12.5 % top and bottom.
- Cabinet results (2026-09-23, maintainer): STAGE SCREENS, MOVIE ONLY, unlit screens and the Griffin House TV
  all work as designed; training-mode scrubs keep the movie on the screens in sync (movie_sync needs nothing —
  entries 9 and 10 are the same `agcs::ScreenRoot` class, prepared and walked every frame). Details: the planning
  `progress.md` deploy log.
