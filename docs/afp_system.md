# AFP System — Research Reference

Research findings for Konami's AFP (Animation Flash Player) runtime as used in DDR World, covering the binary format, libafp API, BM2D object pool, and the filter panel rendering pipeline.

**Game binary**: `gamemdx.dll` (primary research on MDX-003_20260324, cross-verified on MDX-003_20250805)
**AFP library**: `libafp-win64.dll` (version 2.13.7)

---

## Table of Contents

1. [AFP Runtime Patching](#1-afp-runtime-patching)
2. [AFP Format Key Facts](#2-afp-format-key-facts)
3. [libafp ID System](#3-id-system)
4. [libafp API — Verified Signatures](#4-verified-api-signatures)
5. [Choice Panel Scroll — Reference Implementation](#5-choice-scroll-reference)
6. [Filter Panel Layout](#6-filter-panel-layout)
7. [BM2D Object Pool](#7-bm2d-object-pool)
8. [Filter Panel Rendering Pipeline](#8-filter-panel-rendering-pipeline)
9. [Known Pitfalls](#9-known-pitfalls)

---

## 1. AFP Runtime Patching

### Key Discovery: libafputils Pre-Descrambles

**libafputils applies BSI descrambling AND string table decoding BEFORE calling `afp_stream_do_create`.** The data arriving at the function entry is fully descrambled:
- Binary header fields are in native LE byte order
- String table contains plaintext ASCII
- Template name readable directly from `st_offset + name_offset`

**Implication**: Hooking `afp_stream_do_create` receives descrambled data. Patched output can use empty BSI (`\x00\x00`) — no need to re-scramble.

### Data Flow

```
IFS file → libafputils reads AFP + BSI
         → libafputils DESCRAMBLES (applies BSI + decodes string table)
         → afp_stream_do_create called with descrambled data
         → libafputils calls afp_stream_do_set_name with the template name
```

### Child Injection

New children can be injected into AFP templates by inserting tags at the frame 0 insertion point:

1. **DefineSprite tags** (empty sprites with unique character IDs) — MUST come first
2. **PlaceObject tags** (place named children at specified depths) — reference the DefineSprite character IDs

Frame counts and subsequent frame offsets must be updated accordingly.

### Critical Tag Ordering

DefineSprite tags MUST precede PlaceObject tags that reference them. AFP processes tags sequentially — a PlaceObject referencing an undefined character ID silently fails.

### Depth Assignment

Each child needs a unique depth that doesn't conflict with existing objects. The vanilla `filter_switch_base` templates use depths up to 10.

---

## 2. AFP Format Key Facts

- **BSI = Byte Swap Instructions** — a list of byte-reversal operations that descramble AFP data. NOT bytecode. Self-inverse (apply same BSI to descramble or re-scramble).
- **Empty BSI** (`\x00\x00`) is valid — means "no byte swaps needed", data is in descrambled LE form.
- **String table must be 4-byte aligned** — every string offset must be divisible by 4, pad with null bytes. The game fatals with "string alignment error" otherwise.
- **PlaceObject `source_tag_id` is a character ID** (sprite_id/shape_id), NOT a tag array index — appending new DefineSprite tags doesn't break existing references.
- **PlaceObject flags `0x22`** = has source character (0x2) + has instance name (0x20). Minimal but sufficient for placing a named child.
- **filter_switch_base01-05** are all 11828 bytes AFP. BSI varies (two groups: 01/04/05 share one, 02/03 share another). Irrelevant for runtime patching since data is pre-descrambled.
- **VERSION filter uses base02**, GROUP tabs use base03.

### AFP Header Layout (first 56 bytes)

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | Magic/version (`0x08 0xB2 0xD0 0xC1` when descrambled — AP2 with high bits) |
| 4 | 4 | Total length (u32 LE) |
| 10 | 2 | Name offset in string table (u16 LE) |
| 36 | 4 | Tags section offset (u32 LE) |
| 48 | 4 | String table offset (u32 LE) |
| 52 | 4 | String table size (u32 LE) |

### External Reference

The bemaniutils project contains a complete AFP parser (~11,000 lines Python) in `bemani/format/afp/swf.py` — covers BSI descrambling, string table decoding, header parsing, all tag types, and bytecode parsing.

---

## 3. ID System

AFP uses 32-bit IDs with encoded type information:

```
Bits 31-27: Type field (5 bits, masked to 4 after shift)
Bits 26-16: Generation counter (prevents stale ID reuse)
Bits 15-0:  Pool index
```

### ID Types

| Type | Bits 27-30 | Prefix | Name | Where Found |
|------|-----------|--------|------|-------------|
| 1 | 1 | `0x08-0x0F` | **Layer ID** | BM2D pool at +0x08 |
| 2 | 2 | `0x10-0x17` | **Layer ID (alt)** | Some layer functions accept both 1 and 2 |
| 3 | 3 | `0x18-0x1F` | **Stream/Internal** | Parent refs in `afp_layer_get_info` |
| 4 | 4 | `0x20-0x27` | **MovieClip ID** | Returned by `afp_layer_mc_refer` |

### Function Type Requirements

| Function | Accepts | Notes |
|----------|---------|-------|
| `afp_layer_mc_refer` | **Type 1 (layer)** | Returns type-4 children. Use this for BM2D pool lookups. |
| `afp_mc_refer` | Type 4 (MC) only | Rejects type 1 with "invalid" error |
| `afp_mc_search` | Type 4 (MC) only | Same restriction |
| `afp_mc_create` | Type 4 (MC) only | Parent must be MC ID |
| `afp_layer_set_position` | Types 1, 2 | Rejects type 4 |
| `afp_layer_set_mask` | Types 1, 2 | Rejects type 4 |
| `afp_mc_set_param` | Type 4 | Used for visibility, etc. |
| `afp_mc_op` | Type 4 | Used for scroll position |
| `afp_layer_get_name` | Types 1, 4 | Works on both |

**Critical**: The BM2D pool stores type-1 layer IDs. To get type-4 MC IDs (needed for `afp_mc_op`, `afp_mc_set_param`, etc.), call `afp_layer_mc_refer(type1_id, "child_name")`.

---

## 4. Verified API Signatures

All signatures verified via Ghidra disassembly of `libafp-win64.dll`. Resolved by named export at runtime — no ordinals.

### Layer Functions

| Export | Ghidra Address | Signature | Notes |
|--------|---------------|-----------|-------|
| `afp_layer_mc_refer` | `0x180037fa0` | `fn(layer_id: u32, name: *const c_char) -> i32` | Type-1/2 input, returns type-4 MC ID |
| `afp_layer_set_position` | `0x1800135e0` | `fn(layer_id: u32, xy: *const [i32; 2]) -> i32` | **Pointer to {x,y} pair**, NOT two ints. Stores to `[obj+0x130]`, `[obj+0x134]`. |
| `afp_layer_set_mask` | `0x180013bd0` | `fn(layer_id: u32, x: i32, y: i32, w: i32, h: i32) -> i32` | 5 params. Converts to floats, stores clip rect at `[obj+0x140..0x14C]`. Sets flags `0x800` + dirty `0x1`. |
| `afp_layer_set_attribute` | `0x180013a30` | `fn(layer_id: u32, attr: u32) -> i32` | Sets attribute flags |

### MovieClip Functions

| Export | Ghidra Address | Signature | Notes |
|--------|---------------|-----------|-------|
| `afp_mc_refer` | `0x180038380` | `fn(mc_id: u32, name: *const c_char) -> i32` | Type-4 input only |
| `afp_mc_search` | `0x180038780` | `fn(mc_id: u32, path: *const c_char) -> i32` | Type-4 input, handles `/` path separators |
| `afp_mc_op` | `0x18003b210` | `fn(mc_id: u32, op: i32, arg1: u64, arg2: u64) -> i32` | Type-4 check. Op range `0xF00..0xF0F`, jump table dispatch. Op `0x0F04` = set scroll position. |
| `afp_mc_set_param` | `0x180040e10` | `fn(mc_id: u32, param: i32, value: u64, _: u64) -> i32` | Type-4 check. Param range `0x1000..0x1033`, jump table dispatch. Param `0x1007` = visibility. |

### Stream Functions

| Export | Ghidra Address | Signature | Notes |
|--------|---------------|-----------|-------|
| `afp_stream_do_create` | (resolved by name) | `fn(data: *const u8, size: i32, flags: i32) -> i32` | Entry point for AFP template loading |

---

## 5. Choice Panel Scroll — Reference Implementation

`FUN_18016f8f0` (gamemdx+0x16F8F0) is the game's scroll update for the option/choice panel. It demonstrates the AFP API pattern for scrollable content:

### Scroll API Calls

```
1. Find children:
   scroll_id = afp_layer_mc_refer(content_layer, "choice_usr/scroll_usr")
   move_id   = afp_layer_mc_refer(content_layer, "choice_usr/scroll_usr/move_usr")
   tri_l_id  = afp_layer_mc_refer(content_layer, "choice_usr/tri_l_usr")
   tri_r_id  = afp_layer_mc_refer(content_layer, "choice_usr/tri_r_usr")

2. Set scroll position:
   afp_mc_op(scroll_id, 0x0F04, scroll_pixels)

3. Set content position:
   afp_mc_op(move_id, 0x0F04, content_pixels)

4. Show/hide indicators:
   afp_mc_set_param(tri_l_id, 0x1007, visible)  // 0x1007 = visibility
   afp_mc_set_param(tri_l_id, 0x101E, 1)        // 0x101E = enable flag

5. Scroll easing:
   new_pos = old_pos + (target - old_pos) * 0.5  // exponential easing
```

### Key Insight

The game does NOT create scroll children dynamically. It expects them to already exist in the AFP template. It only looks them up, sets positions, and controls visibility.

---

## 6. Filter Panel Layout

### VERSION Entry Grid (category=2)

| Property | Value |
|----------|-------|
| Columns | 2 |
| Column width | 108.0 |
| Row height | 26.0 |
| X positions | 0.0, 108.0 |
| Y start | ~26.0 (below GROUP tabs) |
| Panel content width | 216.0 |
| Visible rows | 9 (fits in panel content area) |

### GROUP Tab Grid (category=3)

3 columns, 72.0 wide each, row height 26.0.

### FilterButton Object Layout

| Offset | Type | Field | Notes |
|--------|------|-------|-------|
| +0x00 | ptr | vtable | `gamemdx+0x36FA48` on 20260324 |
| +0x30 | u8 | selection_state | 0x0=unselected, 0x1=cursor. **Read as u8** — adjacent bytes at +0x31..+0x33 contain unrelated data on some builds. |
| +0x60 | ptr | parent_container | |
| +0x88 | f64 | base_x | |
| +0x90 | f64 | base_y | Overwritten by grid layout engine every frame |
| +0xA0 | f64 | cell_width (108.0) | |
| +0xA8 | f64 | cell_height (26.0) | |
| +0xD0 | string | key (SSO std::string) | |
| +0xF0 | u32 | category_index | 2=version, 3=group |
| +0x178 | ptr | BM2D object pointer | Points to BM2D pool entry; layer_id at [ptr+0x08] |

### Key Functions (Ghidra addresses for 20260324)

| Ghidra Address | Offset | Description |
|---------------|--------|-------------|
| `0x1801239c0` | `+0x1239C0` | Entry builder (GROUP tab loop + VERSION entry loop) |
| `0x180133f20` | `+0x133F20` | panel_config — called for every FilterButton |
| `0x1801343a0` | `+0x1343A0` | Filter category panel builder |
| `0x1801353e0` | `+0x1353E0` | Panel show/hide wrapper — calls panel builder conditionally |
| `0x18016f8f0` | `+0x16F8F0` | Choice scroll setup (reference for scroll pattern) |

---

## 7. BM2D Object Pool

### Discovery (Version-Agnostic)

Pool base is derived from the `bm2d_pool_iter` AOB signature:
```
FF C3 48 81 C7 40 02 00 00 81 FB 00 04 00 00
```
This matches `INC EBX; ADD RDI,0x240; CMP EBX,0x400`. A `LEA Rxx,[pool_base]` instruction with a RIP-relative displacement is within 64 bytes before the match.

### Pool Layout

- **Stride**: `0x240` (read from signature match, not hardcoded)
- **Max entries**: 1024 (read from signature match)
- **Vtable at +0x00**: BM2D::CMovieClip vtable — pre-initialized for all slots
- **Layer ID at +0x08**: Type-1 AFP layer ID. Zero = unused slot.

All slots have the vtable pre-set. Active entries are distinguished by non-zero layer_id at +0x08.

### BM2D Vtable Methods (relevant entries)

| Offset | Method | Signature |
|--------|--------|-----------|
| +0x30 | set_position | `fn(this: *mut BM2D, pos: *mut [i32; 2])` — wrapper that reads layer_id from `[this+0x08]`, converts int x,y to float, calls libafp's internal set_position |

The set_position wrapper at vtable offset 0x30 (`gamemdx+0x21CD70` on 20260324) is called ~4800 times/sec for all active BM2D objects. It can be hooked to intercept position updates for specific layer IDs.

---

## 8. Filter Panel Rendering Pipeline

### Grid Layout Engine

Three functions continuously write to `[FilterButton+0x90]` (base_y) every frame:

| Ghidra Address | Instruction | Notes |
|---------------|-------------|-------|
| `0x18004AA3D` | `movsd [rsi+0x90], xmm1` | RSI = FilterButton |
| `0x18004B17F` | `movsd [r8+0x90], xmm1` | R8 = FilterButton |
| `0x18004B6EF` | `movsd [rax+0x90], xmm1` | RAX = FilterButton |

These recalculate positions from grid parameters (column count, row height, Y start) every frame. Any external write to `base_y` is immediately overwritten.

### Position Calculation Flow

```
Grid layout engine
  → writes base_x/base_y to [FilterButton+0x88/+0x90] every frame

Position computation (FUN_180132750)
  → reads [this+0xA0/+0xA8] (offset_x/y), multiplies by constant
  → reads [this+0x88/+0x90] (base_x/y)
  → reads [this+0x60] (parent_container), gets parent offset
  → final_pos = base_xy + parent_offset + offset_xy * constant
  → converts to int, calls [bm2d_vtable+0x30] (set_position) on BM2D object
```

**Important**: `FUN_180132750` is NOT called via the FilterButton vtable at runtime despite appearing in xref data. The actual set_position calls arrive through a recursive UI tree walker (`gamemdx+0x46280`) that traverses nodes via `[node+0x60]`. The BM2D vtable[0x30] method is the reliable interception point.

### Approaches to Position Modification

| Approach | Result | Why |
|----------|--------|-----|
| Write to `[FilterButton+0x90]` (base_y) | No effect | Grid layout engine overwrites 774 times/frame |
| `afp_layer_set_position(layer_id, xy)` | No visible effect | Game's render pipeline calls vtable[0x30] every frame, overwriting |
| Hook `FUN_180132750` | No effect | Function is not called at runtime for FilterButton rendering |
| **Hook BM2D vtable[0x30] (set_position)** | **Works** | Intercepts the actual position write; modify Y for target layer IDs before passing through |

### Visibility Control

`afp_layer_set_mask` effectively hides/shows individual BM2D layers:
- Zero-size mask `(0, 0, 0, 0)` hides the layer
- Large mask `(-1000, -1000, 3000, 3000)` shows it (effectively no clipping)

---

## 9. Known Pitfalls

### AFP Format
- Data passed to `afp_stream_do_create` is **already descrambled** — do NOT apply BSI or string cipher.
- AFP string table entries **must be 4-byte aligned** — game fatals with "string alignment error" otherwise.
- DefineSprite tags **must precede** PlaceObject tags that reference them — undefined character IDs silently fail.
- Each PlaceObject depth **must be unique** — duplicate depths overwrite the previous object.
- `afp_stream_do_create` takes **3 params** (data, size, flags) — wrong signature corrupts the stack.

### libafp API
- `afp_mc_refer` requires **type-4 MC IDs** — BM2D pool has type-1 layer IDs. Use `afp_layer_mc_refer` instead.
- `afp_layer_set_position` takes a **pointer to {x,y}** — not direct x,y values.
- `afp_mc_op` and `afp_mc_set_param` are **variadic** — pass unused args as 0. Both require type-4 MC IDs.
- libafp functions **must be called from the game thread** — calling from background threads causes hangs/crashes.

### BM2D Pool
- Pool stride is **0x240**, not 0x48.
- BM2D vtable[0x30] (set_position) is the **actual interception point** for position modification — not the FilterButton render function.

### FilterButton
- `selection_state` at +0x30 is a **u8** — bytes at +0x31..+0x33 contain unrelated data on some game versions. Reading as u32 causes missed cursor detection on older builds.
- Hooking `panel_config` (`FUN_180133f20`) with retour **breaks FilterButton keyboard navigation** — hook the panel builder (`FUN_1801343a0`) instead.
- `base_y` at +0x90 is **overwritten every frame** by the grid layout engine — direct writes have no visible effect.

### Tooling
- Cheat Engine's `executeCodeEx` **crashes the game** on the filter screen — use breakpoints or memory reads instead.
- Cheat Engine uses runtime addresses, Ghidra uses static base `0x180000000` — keep address spaces separate to avoid confusion.
