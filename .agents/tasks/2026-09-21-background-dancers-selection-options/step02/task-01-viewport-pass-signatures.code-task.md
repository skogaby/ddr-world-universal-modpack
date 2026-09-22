# Task: `derive_scene3d_viewport` — the viewport-pass compositor's engine sites (optional sub-group)

## Description
Add the AOB signatures and the all-or-nothing derivation that publish every engine address/offset the
preview compositor (Step 3, `scene3d::viewport_pass`) needs: the display object, the RENDER_2D target
list, the attach/detach functions, the four MODEL pass globals and the pass object layout, the render
worker's gd write pointer, the per-viewport setup semantics, and the target list's Clear record shape.
The group is OPTIONAL (like `texture_lookup` / `shader_lookup`): a miss leaves
`Scene3dSites.viewport == None` with one WARN and the Background Dancers mod fully usable without previews.
Never add it to any `required_signatures`.

## Background
Design §3.1/§4.5/§4.9 and `research/preview-compositing.md`. Ghidra-verified on `gamemdx_20260825.dll`
(file-relative to `0x180000000`) during this task's authoring — the byte-exact anchors:

| AOB | Site | What it yields (decode at `match+N`) |
|---|---|---|
| `render_graph_boot_attach` | `FUN_1801f2c30+0x2E5` — three `MOV RDX,[rip+pass]; ADD RDX,0x30; MOV R8D,0x66/67/68; MOV RCX,[rip+display]; MOV RCX,[RCX+0x28]; CALL attach` | display global, RENDER-3D list off (info), OPACITY/LOWPRIO/TRANS pass globals, viewport sub-object off (0x30), attach fn (callee prologue gate → list target off 0x38, dims u16 offs 0x14/0x16, vp rect off 8), priorities attested |
| `render_graph_2d_attach` | `FUN_1801f2c30+0x3AA` — three `MOV R8D,0x65/66/67; MOV RDX,[rip+2dlist]; MOV RCX,[rip+display]; MOV RCX,[RCX+0x38]; CALL attach` | RENDER_2D list off (0x38) — must differ from the 3D one; display + attach must equal |
| `viewport_detach` | `FUN_1801f30b0+0x11E` — three `MOV RCX,[rip+display]; MOV RDX,[rip+pass]; MOV RCX,[RCX+0x28]; ADD RDX,0x30; CALL detach` | detach fn (callee prologue gate: `MOV R8,[RCX+8]; MOV RAX,[RCX]; … ADD RAX,0x10` — 16-byte elements); globals/offsets must equal the boot's |
| `model_pass_ctor` | `FUN_1801f6510+0x159` — `MOV ECX,0xf8; CALL alloc; TEST; JZ; …` store block | pass size, sort/filter/flags/rect/minZ/maxZ/self/items/callbacks/name-hash offsets, the Viewport vftable (identity gate for the stock pass; slot 0 = render fn whose body reads `vp+0xB8` items / `vp+0xB0` outer) |
| `model_pass_enable_tail` | `FUN_1801f6510+0x2C1` — four `MOV RAX,[rip+pass]; AND dword [RAX+0x54],~1` | the four pass globals in ctor order (DISTANT, OPACITY, LOWPRIO, TRANS) — DISTANT is needed for the free-bit check; flags off attested (bit0 = DISABLED) |
| `scene_manager_camera_copy` | `FUN_180023fb0+0xAD` — TRANS view memcpy (`ADD RCX,0x98`) + DISTANT/OPACITY proj memcpy (`ADD RCX,0x58`) | pass view off (0x98), proj off (0x58), 0x40 matrix size attested; globals must equal |
| `viewport_setup_rect` | `FUN_18026cec0+0x5A` — `MOV EAX,[RDI+0xC]; MOVSS [RDI+0x14]; MOVSS [RDI+0x10]; MOV R9D,[RDI+8]; MOV R8D,[RDI+4]; MOV EDX,[RDI]; …; CALL set_viewport; TEST byte [RDI+0x1C],2; …; MOVUPS [RDI+0x20]…` | rect field order `{x,y,w,h,minZ,maxZ}`, flags = rect+0x1C (bit1 = skip camera), proj = rect+0x20, view = rect+0x60 |
| `worker_gd_write` | `FUN_180272d30+0x187` — `LEA RDX,[RBX+8]; TEST RBX; JNZ; MOV RDX,RSI; MOV RCX,RDI; CALL setup; MOV R11,[RBX]; MOV RDX,RDI; MOV RCX,RBX; CALL [R11]; MOV RAX,[RDI+0x218]; MOV dword [RAX],0x4003a; ADD RAX,4; MOV [RDI+0x218],RAX` | worker ctx gd write off (0x218), vp rect off (8), render = vtable slot 0 `(vp, ctx)`, `0x4003a` terminator attested; CALL target must be the setup fn (prologue `53 57 41 55 41 56 41 57 48 81 EC`) and the setup AOB match must lie inside it |
| `target_list_clear` | `FUN_180272600+0x07` — `TEST byte [RDX+0x40],1; …; MOV RDX,[RDX+0x38]; …; TEST byte [RDI+0x40],2; JZ; MOVZX EAX,byte [RDI+0x24]; … MOV dword [RCX],0x140000; MOVSS [RCX+0xC]; MOV [RCX+4],EAX; LEA RAX,[RCX+0x14]; MOV [RCX+8],EDX; MOV [RCX+0x10],R8D` | list flags off (0x40), list target off (0x38, must equal the attach callee's), Clear record `{u16 0, u16 0x14, flags, D3DCOLOR, f32 z, u32 stencil}` shape + size 0x14 attested |

Cross-checks (all must hold): ctor rect off == sub + vp rect off; ctor flags off == sub + vp rect off +
setup's TEST disp; camera-copy proj/view == sub + vp rect off + setup's MOVUPS disps; self/items/callbacks/
view+0x40 ≤ pass size; the four `enable_tail` globals include exactly the boot's three (as slots 1..3);
the Viewport vftable slot 0 body reads items at `vp + (items_off − sub)` and outer at `vp + (self_off − sub)`.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-21-background-dancers-selection-options/design/detailed-design.md` (§4.5, §4.9, §5.2, Appendix A)

**Additional References (if relevant to this task):**
- `.agents/planning/2026-09-21-background-dancers-selection-options/research/preview-compositing.md`
- `src/core/signatures.rs` — `derive_scene3d`, `scene3d_resolve_texture_lookup`, `scene3d_resolve_shader_lookup` (the optional sub-group shape), `Scene3dSites`, `publish_value`
- `scripts/validate_signatures.sh`, `scripts/sig_harness/shape_diff.py`

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Nine `SignatureDefinition`s (names above) in the house style — descriptions name the function, the
   consumer reads at `match+N`, and the uniqueness claim.
2. `pub struct Scene3dViewportSites` (design §4.5 field list + `pass_globals: [*const u8; 4]`,
   `render2d_list_off`, `vp_rect_off`, `vp_flags_off`, `list_flags_off`, `target_dims_w_off/h_off`,
   `pass_vftable`, `pass_sort_off`, `pass_name_off`, `pass_callbacks_off`, `pass_minz_off`,
   `pass_maxz_off`); `Scene3dSites.viewport: Option<Scene3dViewportSites>`.
3. `fn scene3d_resolve_viewport(&self) -> Result<Scene3dViewportSites, String>` (all cross-checks) and
   its wiring into `derive_scene3d` as an optional sub-group: publish addresses via `resolved` and
   values via `publish_value` under `scene3d_vp_*` names; a miss removes every name in
   `SCENE3D_VIEWPORT` and logs `[-] scene3d_viewport (optional) -- <why> -- 3D option previews unavailable`.
4. `scene3d_sites()` fills `viewport` from the published names (all-or-nothing → `Option`).
5. `scene3d::init` logs `scene3d viewport pass: available (…)` / `unavailable`.
6. `./scripts/validate_signatures.sh ~/Desktop/ddr_modules` ALL GREEN; `shape_diff.py` reviewed for the
   nine new signatures (every consumer reads `match+N`).

## Dependencies
- Step 1 complete (no code dependency; the mod exists).

## Implementation Approach
1. Signatures + struct + resolver + wiring; `cargo check`.
2. Sweep + shape diff; fix any build-specific divergence with `_vN` alternates only if needed.

## Acceptance Criteria

1. **Group derives on 20260825**
   - Given the sweep harness over `gamemdx_20260825.dll`
   - When `resolve_derived` runs
   - Then every `scene3d_vp_*` name is published with the values in the table (0x38 RENDER_2D, 0xF8 size,
     0x30/0x38/0x54/0x58/0x98/0xE0/0xE8/0x2C, 0x218, 0x40/0x38/0x14/0x16, 8)

2. **All four builds**
   - Given the sweep over the four supported builds
   - When it runs
   - Then the report is ALL GREEN for the new names and `shape_diff.py` shows no divergence at any read offset

3. **Fail-open**
   - Given any one of the nine signatures missing (simulated by a bad pattern in a scratch run)
   - When `derive_scene3d` runs
   - Then the `scene3d` group itself still derives, `viewport == None`, one WARN

## Metadata
- **Complexity**: High
- **Labels**: signatures, scene3d, reverse-engineering
- **Required Skills**: x86-64 encoding, Ghidra, the signature store
- **Generated By**: code-task-generator 2026-09-21
- **Source Plan**: `.agents/planning/2026-09-21-background-dancers-selection-options/implementation/plan.md`
- **Plan Step**: Step 2: Viewport-pass signature derivations
