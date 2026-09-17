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
those songs). `created == released` on every teardown (10..42), teardowns 7–33 ms.

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
