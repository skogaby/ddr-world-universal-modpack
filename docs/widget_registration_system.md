# Widget Registration System — Native Render List Architecture

**Binary**: gamemdx.dll build 2025-07-29 (MDX-68889b2b), Ghidra base: `0x180000000`

---

## Overview

DDR World's rendering pipeline uses a scene-managed linked list of widget wrappers. Each frame, the render loop walks this list and calls `wrapper_render` on each entry, which sets up font globals and dispatches to the inner widget's `render_function`. By creating proper `agcs::BmpString` wrappers and inserting them into this list, the game renders custom widgets natively — no manual render calls, no once-per-frame guards, no ScreenCommandList lifecycle issues.

---

## Architecture

### Rendering Hierarchy

```
render_loop (FUN_1801FFBA0, +0x1FFBA0)          — once per frame per layer
  ├── Walks linked list of registered wrappers
  ├── Filters by visibility flags
  ├── Sorts by priority
  ├── Sets up coordinate transform on ScreenCommandList
  └── For each visible wrapper:
      └── wrapper_render (FUN_180202170, +0x202170)  — vtable[5] on wrapper
          ├── Reads child_array from wrapper+0x18
          ├── Sets 3 font globals from child_array[1..3]
          ├── Calls render_function (vtable[1]) on child_array[0] (the widget)
          └── Clears font globals
```

### Object Hierarchy

```
Scene Manager (global at +0x6B5AA8)
  └── Render List Manager (at scene_manager + 0xB0)
      ├── Free node pool (pre-allocated)
      └── Active list (singly-linked)
          └── Node → Data Area → agcs::BmpString wrapper
                                    └── child_array[0] → kt::BmpfontSimpleString widget
                                                            ├── line_desc (0xC0 bytes)
                                                            └── render_state (0x128 bytes)
```

---

## Key Structures

### agcs::BmpString Wrapper (0x20 bytes)

Created by `FUN_180201E90` (+0x201E90).

| Offset | Size | Type | Field | Notes |
|--------|------|------|-------|-------|
| +0x00 | 8 | ptr | vtable | `agcs::BmpString::vftable` at +0x367088 |
| +0x08 | 4 | i32 | ref_count | Set to 1 on creation, incremented on registration |
| +0x10 | 2 | u16 | flags | 0x0100 — render_loop checks byte at +0x10 must be 0 |
| +0x12 | 1 | u8 | enabled | 1 — render_loop checks this must be non-zero |
| +0x18 | 8 | ptr | child_array | Pointer to 4-qword array |

The `flags` field at +0x10 is a u16 with value 0x0100. In little-endian, byte +0x10 = 0x00 and byte +0x11 = 0x01. The render_loop checks `*(byte*)(wrapper + 0x10) == 0` which passes.

### Child Array (0x20 bytes = 4 × qword)

Allocated separately, pointed to by wrapper+0x18. Read by `wrapper_render`.

| Index | Offset | Field | Notes |
|-------|--------|-------|-------|
| [0] | +0x00 | widget_ptr | `kt::BmpfontSimpleString*` — the actual renderable widget |
| [1] | +0x08 | font_global_1 | Written to `DAT_6B5D68` by wrapper_render. Can be 0. |
| [2] | +0x10 | font_global_2 | Written to `DAT_6B5D70` by wrapper_render. Can be 0. |
| [3] | +0x18 | font_global_3 | Written to `DAT_6B5D78` by wrapper_render. Can be 0. |

**Font globals can be zero.** The `render_function` reads the font pointer from the widget's own `render_state+0x70`, NOT from these globals. The globals appear to be legacy/unused in the current build. Game-created wrappers also leave child_array[1..3] as zero (confirmed via constructor analysis of `FUN_180201E90`).

### kt::BmpfontSimpleString Widget (0x18 bytes)

Created by `widget_factory` (`FUN_1801F41B0`, +0x1F41B0).

| Offset | Size | Type | Field |
|--------|------|------|-------|
| +0x00 | 8 | ptr | vtable (`kt::BmpfontSimpleString::vftable`) |
| +0x08 | 8 | ptr | line_desc (0xC0 bytes — text buffer, position, color, outline, etc.) |
| +0x10 | 8 | ptr | render_state (0x128 bytes — font pointer at +0x70, glyph cache) |

### Render List Manager

Accessed via `*(*(gamemdx_base + 0x6B5AA8) + 0xB0)`.

| Offset | Size | Type | Field |
|--------|------|------|-------|
| +0x18 | 8 | ptr | free_pool_head — head of free node singly-linked list |
| +0x20 | 8 | ptr | free_pool_sentinel — when head == sentinel, pool is empty |
| +0x28 | 8 | ptr | active_list_head — first node in the render list |
| +0x30 | 8 | ptr | active_list_tail — last node in the render list |
| +0x3C | 4 | i32 | active_count — number of registered wrappers |

### Free Pool Node

Pre-allocated by the game. Each node wraps a data area.

| Offset | Size | Type | Field |
|--------|------|------|-------|
| +0x00 | 8 | ptr | data_area_ptr — pointer to the data area struct |
| +0x08 | 8 | ptr | next — next node in free list (0 when in active list) |

### Data Area (Active List Node)

When a node is moved from the free pool to the active list, its data area becomes the active list entry.

| Offset | Size | Type | Field |
|--------|------|------|-------|
| +0x00 | 8 | ptr | entry_self_ptr — self-reference, used by render_loop |
| +0x08 | 8 | ptr | next — next data area in active list (0 = end) |
| +0x10 | 8 | ptr | wrapper_ptr_1 — `agcs::BmpString*` (copy 1) |
| +0x20 | 8 | ptr | wrapper_ptr_2 — `agcs::BmpString*` (copy 2, read by render_loop) |
| +0x28 | 1 | u8 | visibility — 0 = visible, non-zero = hidden |

The render_loop reads `data_area[0]` (the self-pointer), then checks:
1. `*(self_ptr + 0x28) == 0` — visibility check
2. `*(*(self_ptr + 0x20) + 0x10) == 0` — wrapper flags check (byte)
3. `*(*(self_ptr + 0x20) + 0x12) != 0` — wrapper enabled check (byte)

---

## Global Addresses (relative to gamemdx.dll base)

| Symbol | Offset | Description |
|--------|--------|-------------|
| `agcs::BmpString::vftable` | +0x367088 | Vtable for the wrapper object |
| `scene_manager_ptr` | +0x6B5AA8 | Global pointer to scene manager instance |
| `font_global_1` | +0x6B5D68 | Set by wrapper_render from child_array[1] |
| `font_global_2` | +0x6B5D70 | Set by wrapper_render from child_array[2] |
| `font_global_3` | +0x6B5D78 | Set by wrapper_render from child_array[3] |
| `widget_factory` | +0x1F41B0 | Creates `kt::BmpfontSimpleString` widgets |
| `wrapper_constructor` | +0x201E90 | Creates `agcs::BmpString` wrappers |
| `wrapper_render` | +0x202170 | vtable[5] — sets font globals, dispatches to render_function |
| `render_function` | +0x1F46E0 | vtable[1] on widget — renders glyphs to ScreenCommandList |
| `render_loop` | +0x1FFBA0 | Per-layer render dispatch — walks active list |

---

## Registration Procedure

This is the exact sequence the game uses to register a wrapper (observed in `FUN_1800812E0` and `FUN_1800A57A0`):

### Step 1: Create the Widget

```
widget = widget_factory(font_ptr, group=0, count=1)
```

This allocates the `kt::BmpfontSimpleString` (0x18 bytes), its `line_desc` (0xC0 bytes), and its `render_state` (0x128 bytes). The font pointer is stored in `render_state+0x70`.

### Step 2: Create the Wrapper

Allocate 0x20 bytes for the `agcs::BmpString` wrapper and 0x20 bytes for the child array:

```
wrapper = alloc(0x20)
wrapper[0x00] = gamemdx_base + 0x367088    // agcs::BmpString vtable
wrapper[0x08] = 1                           // ref_count
*(u16*)(wrapper + 0x10) = 0x0100           // flags
*(u8*)(wrapper + 0x12) = 1                 // enabled

child_array = alloc(0x20)
memset(child_array, 0, 0x20)
child_array[0] = widget                    // the BmpfontSimpleString pointer

wrapper[0x18] = child_array
```

### Step 3: Patch the Line Descriptor Callbacks

The `wrapper_constructor` patches 3 vtable entries on the widget's `line_desc`. These are callbacks used by the wrapper for text measurement and layout:

```
line_desc = widget[0x08]                    // widget+0x08 = line_desc pointer
line_desc_vtable = line_desc[0x08]          // line_desc+0x08 = vtable pointer (NOT offset 0!)

*(line_desc_vtable + 0x30) = gamemdx_base + 0x201CC0   // callback 1
*(line_desc_vtable + 0x38) = gamemdx_base + 0x201D20   // callback 2
*(line_desc_vtable + 0x40) = gamemdx_base + 0x201E10   // callback 3
```

These callbacks are set by the `wrapper_constructor` at +0x201E90 (see disassembly at `0x180201F29`–`0x180201F5C`). They enable the wrapper to query text dimensions from the inner widget. Without these, text measurement calls through the wrapper vtable crash.

### Step 4: Get the Render List Manager

```
scene_manager = *(gamemdx_base + 0x6B5AA8)
if scene_manager == 0: FAIL (scene manager not initialized yet)

render_list_mgr = *(scene_manager + 0xB0)
if render_list_mgr == 0: FAIL
```

### Step 5: Pop a Node from the Free Pool

```
free_head = render_list_mgr[3]              // +0x18: free pool head
sentinel  = render_list_mgr[4]              // +0x20: free pool sentinel

if free_head == sentinel:
    render_list_mgr[3] = 0
    render_list_mgr[4] = 0
    FAIL (no free nodes)
else:
    render_list_mgr[3] = free_head[1]       // advance head to next
    free_head[1] = 0                         // detach node from free list

node = free_head
data_area = node[0]                          // pre-allocated data area
```

### Step 6: Initialize the Data Area

```
data_area[0x10] = wrapper                   // wrapper pointer (copy 1)
data_area[0x20] = wrapper                   // wrapper pointer (copy 2)
data_area[0x28] = 0                         // visibility = visible (u8)

wrapper[0x08] += 1                          // increment ref_count
```

Note: `data_area[0x00]` is pre-initialized (likely a self-pointer or type tag) and should NOT be modified.

### Step 7: Append to Active List

```
render_list_mgr[0x3C] += 1                 // increment active count (i32 at +0x3C)

data_area[0x08] = 0                         // new tail's next = NULL

if render_list_mgr[6] == 0:                // +0x30: tail
    render_list_mgr[5] = data_area          // +0x28: head = new node (list was empty)
else:
    *(render_list_mgr[6] + 0x08) = data_area  // old tail.next = new node

render_list_mgr[6] = data_area             // +0x30: tail = new node
```

After this, the render_loop picks up the wrapper on the next frame and renders it through the normal pipeline.

---

## Widget Lifecycle

### Visibility Control

To show/hide a registered widget, set the visibility byte on the data area:

```
data_area[0x28] = 0    // visible
data_area[0x28] = 1    // hidden
```

The render_loop checks this each frame. No need to remove from the list.

Alternatively, the `line_desc+0x49` visibility byte can be used. The render_function checks this internally and skips rendering if hidden. Both approaches work; the data_area visibility is checked first (by render_loop), and the line_desc visibility is checked second (by render_function).

### Text Updates

Text and position fields are written directly to the `line_desc` struct. The render_function reads these each frame and regenerates glyphs when the dirty flag is set.

**Thread safety**: Since the game's render loop handles rendering, widget mutations should ideally happen on the game thread. However, the game's own text layout engine processes dirty flags during the render pass, so mutations between frames (from any thread) should be safe as long as they complete before the next render_loop call.

### Deregistration

To remove a widget from the render list, the node must be unlinked from the active list and returned to the free pool. For simplicity, hiding via `data_area[0x28] = 1` is sufficient — the widget stays in the list but is skipped each frame.

---

## References

- Ghidra: FUN_1800812E0 (+0x812E0) — game's own widget registration pattern
- Ghidra: FUN_180201E90 (+0x201E90) — `agcs::BmpString` wrapper constructor
- Ghidra: FUN_1801FFBA0 (+0x1FFBA0) — render_loop linked list traversal
- Ghidra: FUN_180202170 (+0x202170) — wrapper_render font global setup
