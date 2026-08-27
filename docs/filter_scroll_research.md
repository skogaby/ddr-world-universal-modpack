# Filter Panel Scroll & Layout Research

Reverse engineering document for the series filter overflow when custom entries are added beyond the panel's display capacity.

**Game binary**: `gamemdx.dll` (MDX-003_20260324)
**Ghidra base**: `0x180000000`
**Prerequisite**: [filter_ui_extension.md](filter_ui_extension.md), [series_filter_internals.md](series_filter_internals.md)

---

## Problem Statement

The VERSION filter panel uses a 2-column grid layout with 26px row height. With 9 vanilla entries + 3 GROUP tabs, the panel fits within its bounds. When custom entries are added (19+ in testing), they overflow below the panel's visible area — rendering outside the window with no scrolling or clipping. The game was never designed for more than 9 version filter entries.

---

## Live Memory Analysis: Grid Dimensions (Confirmed)

Scanned for FilterButton vtable (`gamemdx+0x36FA48`) pointers in writable memory.

### FilterButton Object Layout (relevant fields)

| Offset | Type | Field | Notes |
|--------|------|-------|-------|
| +0x00 | ptr | vtable | `gamemdx+0x36FA48` |
| +0x30 | u32 | selection_state | `0x0`=unselected, `0x1`=cursor here, `0x100`=selectable |
| +0x60 | ptr | parent_container | Points to the panel container object |
| +0x88 | f64 | base_x | Column position |
| +0x90 | f64 | base_y | Row position |
| +0xA0 | f64 | cell_width | 108.0 for version entries |
| +0xA8 | f64 | cell_height | 26.0 |
| +0xD0 | string | key | SSO std::string, entry key for texture lookup |
| +0xF0 | u32 | category_index | Filter category (2=version, 3=group tabs) |

### VERSION Entry Grid (cat=2)

| Property | Value |
|----------|-------|
| Columns | 2 |
| Column width | 108.0 |
| Row height | 26.0 |
| X positions | 0.0, 108.0 |
| Y start | 27.0 (below GROUP tabs row at Y=0) |
| Panel content width | 216.0 |

### GROUP Tab Grid (cat=3)

3 columns, 72.0 wide each, row height 26.0.

### Parent Container (at FilterButton+0x60)

| Offset | Type | Value | Notes |
|--------|------|-------|-------|
| +0x168 | u32 | 22 | Total entry count (matches custom entry count) |

---

## Key Functions in gamemdx.dll

| Symbol | Ghidra Address | Offset | AOB Signature | Description |
|--------|---------------|--------|---------------|-------------|
| `entry_builder` | `0x1801239c0` | `+0x1239C0` | — | Builds filter panel UI (GROUP tab loop + VERSION entry loop) |
| `panel_config` | `0x180133f20` | `+0x133F20` | `48 8B C4 55 57 41 54 48 8D 68 A1 48 81 EC B0 00 00 00 48 C7 45 E7 FE FF FF FF 48 89 58 10 48 89 70 18` | Called for EVERY FilterButton. Sets category at +0xF0, finds BM2D template. Params: (RCX=FilterButton*, EDX=category_index) |
| `filter_button_vtable` | `0x18036FA48` | `+0x36FA48` | — | FilterButton vtable |
| `filter_button_render` | `0x180132750` | `+0x132750` | — | FilterButton render (vtable[0]) |
| `filter_switch_fmt` | `0x18036f628` | `+0x36F628` | — | `"filter_switch_base%02d"` format string |
| `bm2d_object_pool` | `0x1806F2180` | `+0x6F2180` | — | BM2D MovieClip object pool (stride 0x240, max 0x400) |
| `music_scroll_setup` | `0x18018f1e0` | `+0x18F1E0` | — | Music select scroll init (reference for AFP scroll pattern) |
| `choice_scroll_setup` | `0x18016f8f0` | `+0x16F8F0` | — | Option/choice panel scroll setup (reference for AFP scroll pattern) |

### Entry Builder Loop Structure (`FUN_1801239c0`)

**Loop 1 — GROUP tabs** (3 iterations, indices 2→0):
- Calls `FUN_1801237a0` directly to create FilterButton
- Then calls `FUN_180133f20` (panel_config) with category=3

**Loop 2 — VERSION entries** (8+N iterations, indices 8+N→0):
- Calls `[functor vtable+0x8]` to create FilterButton (NOT `FUN_1801237a0`)
- Then calls `FUN_180133f20` (panel_config) with category=2

**Important**: VERSION entries are created through a functor, not by direct function call. Hooking `FUN_1801237a0` only captures GROUP tabs. Hook `FUN_180133f20` (panel_config) to capture ALL FilterButtons.

---

## AFP/BM2D System

### The Game's Scroll Mechanism

The option/choice panel (`option_choice` AFP template) has built-in scroll support via a BM2D MovieClip child hierarchy:

```
choice_usr
├── choice_usr/choice_usr     (nested choice container)
├── choice_usr/scroll_usr     (scroll container)
│   └── choice_usr/scroll_usr/move_usr  (moveable content area)
├── choice_usr/tri_l_usr      (left/up scroll indicator triangle)
└── choice_usr/tri_r_usr      (right/down scroll indicator triangle)
```

The game code at `FUN_18016f8f0` (gamemdx.dll) drives this scroll mechanism by:
1. Finding `scroll_usr` and `move_usr` children via `afp_mc_refer` (libafp Ordinal 104)
2. Moving `move_usr` position via `afp_layer_set_position` (libafp Ordinal 47)
3. Showing/hiding `tri_l_usr`/`tri_r_usr` based on scroll state

The filter panel templates (`filter_switch_base01` through `05`) do NOT have these scroll children.

### AFP File Structure

- Magic: `0xC1D0B208` (big-endian at offset 0)
- AFP system version: 2.13.7 (from source path in libafputils debug strings)
- Three file types per animation: AFP (main data), BSI (bytecode stream instructions), GEO (geometry/shapes)
- Child element names are stored in BSI bytecode, resolved at runtime into layer objects
- `filter_switch_base01` through `05` are all 11,828 bytes — identical templates for different filter categories
- `option_choice` is 17,020 bytes — has the scroll mechanism

### Extracted Assets

The filter screen ARC contains:
- `afp/` — AFP animation files (binary, proprietary format)
- `afp/bsi/` — BSI bytecode files
- `afp/afplist.xml` — AFP manifest listing all animations and their GEO references
- `geo/` — Geometry/shape data files
- `tex/` — Texture PNGs (including `sefi_version_*`, `common_tri.png`, etc.)

---

## libafp-win64.dll API (KEY FINDING)

libafp has a complete runtime API for manipulating BM2D MovieClip instances. This is the path to native scrolling.

### Critical Exports

| Export | Ordinal | libafp Offset | Signature | Purpose |
|--------|---------|---------------|-----------|---------|
| `afp_layer_set_mask` | 48 | `+0x13BD0` | `(layer_id: u32, x: i32, y: i32, w: i32, h: i32) -> i32` | **Set rectangular clip mask** |
| `afp_layer_set_position` | 47 | `+0x135E0` | `(layer_id: u32, x: i32, y: i32) -> i32` | Move layer position |
| `afp_mc_refer` | 104 | `+0x38380` | `(parent_id: u32, name: *const c_char) -> i32` | Find child by name → layer_id |
| `afp_mc_search` | 105 | `+0x38780` | `(parent_id: u32, path: *const c_char) -> i32` | Find child by path |
| `afp_mc_create` | 107 | `+0x39C70` | `(parent_id: u32, name: *const c_char, ...) -> i32` | **Create new child MovieClip** |
| `afp_mc_attach_movie` | 110 | `+0x3A8E0` | `(parent_id: u32, ...) -> i32` | **Attach MovieClip from library** |
| `afp_mc_destroy` | 108 | `+0x3A4A0` | `(mc_id: u32) -> i32` | Destroy MovieClip |
| `afp_layer_set_color` | 49 | `+0x13670` | `(layer_id: u32, r, g, b, a: i32) -> i32` | Set RGBA color |
| `afp_layer_set_attribute` | 56 | `+0x13A30` | `(layer_id: u32, attr: u32) -> i32` | Set attribute flags |
| `afp_layer_set_matrix` | 45 | `+0x13030` | `(layer_id: u32, matrix: *const f32) -> i32` | Set 2D transform |
| `afp_layer_get_name` | 32 | `+0x14FB0` | `(layer_id: u32) -> *const c_char` | Get layer name |
| `afp_layer_get_info` | 71 | `+0x14D00` | `(layer_id: u32, info: *mut) -> i32` | Get layer info struct |
| `afp_mc_op` | 114 | `+0x3B210` | `(mc: *mut, name: *const c_char, op: i32, data: *mut) -> *mut` | MovieClip operations |
| `afp_mc_get_param` | 115 | `+0x3E370` | `(mc_id: u32, param: i32) -> varies` | Get MC parameter |
| `afp_mc_set_param` | 116 | `+0x40E10` | `(mc_id: u32, param: i32, value) -> i32` | Set MC parameter |
| `afp_mc_mc_list` | 122 | `+0x44040` | `(mc_id: u32) -> ?` | List child MCs |
| `afp_stream_get_info` | 70 | `+0x15BA0` | `(stream_id: u32, info: *mut) -> i32` | Get stream info |
| `afp_layer_call_function` | 55 | `+0x342B0` | `(layer_id: u32, ...) -> i32` | Call function on layer |

### `afp_layer_set_mask` Internals (Disassembly Confirmed)

```
Stores clip rect as floats:
  [layer+0x140] = (float)x            // clip left
  [layer+0x144] = (float)(x + width)  // clip right
  [layer+0x148] = (float)y            // clip top
  [layer+0x14C] = (float)(y + height) // clip bottom
Sets flags:
  [layer+0x00] |= 0x800               // mask enabled
  [layer+0x14] |= 0x1                 // dirty/needs update
```

### `afp_mc_refer` Internals (Disassembly Confirmed)

Child elements in linked list at `[parent+0x58]`. Each child:
- `+0x70`: pointer to name string (e.g., `"item_usr"`)
- `+0x78`: 32-bit name hash (fast lookup)
- `+0x68`: next sibling pointer

Handles `/` path separators for nested lookups.

### Strings Referenced in gamemdx.dll (BM2D child names)

| String | Ghidra Address | Context |
|--------|---------------|---------|
| `"scroll_usr"` | `0x180363310` | Music select scroll child |
| `"music_card_scroll_root"` | `0x18037ade0` | Music select scrollable grid |
| `"choice_usr/scroll_usr"` | `0x180373e08` | Option panel scroll container |
| `"choice_usr/scroll_usr/move_usr"` | `0x180373e20` | Option panel moveable content |
| `"choice_usr/tri_l_usr"` | `0x180373e40` | Left scroll indicator |
| `"choice_usr/tri_r_usr"` | `0x180373e58` | Right scroll indicator |
| `"filter_item"` | `0x18036f5b0` | Filter entry MovieClip template |
| `"filter_switch_base%02d"` | `0x18036f628` | Filter panel template format |

### RTTI Classes

| Class | Has Vtable? | Notes |
|-------|-------------|-------|
| `screen::Scrollable` | No (abstract interface) | Base class, never instantiated |
| `sequence::selectmusic::ScrollBar` | Yes (vtable at +0x37AE50) | Music select specific |

---

## Lessons from Initial Scroll Attempt

An initial scroll approach using direct Y-position manipulation on FilterButton objects was attempted. Issues encountered:

- Direct Y-position manipulation is fragile — the game may overwrite positions, and there is no clipping so off-screen entries remain visible
- Custom scroll logic is fundamentally a band-aid — the game has a native scroll system that should be leveraged instead

**Key insight**: The correct approach is to use the AFP runtime APIs (`afp_mc_create`, `afp_mc_attach_movie`, `afp_layer_set_mask`) to inject scroll children into the filter panel template at runtime, then let the game's existing scroll handler code drive the behavior.

---

## Scroll Indicator Approaches

### Approach 1: Triangle Arrows (Simple)
Up/down arrow sprites using `common_tri.png` from the filter ARC. Show when content overflows in that direction.

### Approach 2: Track + Thumb Scrollbar (Polished)
Vertical scrollbar track with proportional thumb, matching the music select screen style.

Both approaches can be implemented once the core scroll mechanism works.
