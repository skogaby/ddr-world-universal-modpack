# DDR World Widget System — Reverse Engineering Research

**Game:** Dance Dance Revolution WORLD (DDR World) — August 2025 build
**Process:** spice64.exe (spice2x loader) → gamemdx.dll
**Ghidra base:** `0x180000000`

---

## §1 Module Discovery & System Architecture

### Module Discovery

| Module | Base Address (Ghidra) | Size | Role |
|--------|----------------------|------|------|
| spice64.exe | — | ~6.8 MB | Spice2x loader |
| gamemdx.dll | `0x180000000` | 19,456,000 B (~18.5 MB) | Main game logic, all UI/rendering |

**ASLR note:** The gamemdx.dll runtime base address changes every launch. All addresses in this document are expressed as **offsets relative to gamemdx.dll base** (e.g., `+0x1F46E0`) or as Ghidra addresses (base `0x180000000`).

**Address translation (for any session):**
```
offset       = runtime_addr - gamemdx_base
ghidra_addr  = 0x180000000 + offset
runtime_addr = gamemdx_base + (ghidra_addr - 0x180000000)
```

### System Architecture

DDR World's UI rendering uses a **layered architecture** with four key subsystems:

```
┌─────────────────────────────────────────────────────┐
│  Actor System (me::task::actor, agcs::Actor)        │  Game logic, scene transitions
│  └── Application::RootActor → ArkActor → ...        │
├─────────────────────────────────────────────────────┤
│  Scene Graph (agcs::scene::Node, SceneGraphManager) │  High-level scene management
├─────────────────────────────────────────────────────┤
│  Bitmap Font System (kt::Bmpfont*)                  │  Text widget rendering
│  ├── BmpfontString (container, no rendering)        │
│  ├── BmpfontSimpleString (text widget, renders)     │
│  └── BmpfontImpl (font renderer/glyph cache)        │
├─────────────────────────────────────────────────────┤
│  Render Command Lists (gs::Renders::ScreenCommandList) │  GPU draw command submission
│  └── gs::Viewport::Template<Scene, Render>          │
└─────────────────────────────────────────────────────┘
```

**Key namespaces:**
| Namespace | Full Name | Role |
|-----------|-----------|------|
| `me` | Game engine core | Memory, framework, hooks, math |
| `agcs` | Arcade Game Common System | Scene graph, actors, resources, heaps |
| `kt` | Konami Technology | Bitmap fonts, text rendering |
| `gs` | Graphics System | Viewport, GPU resources, render commands |

### Thread Safety

From execution breakpoint analysis of the render function:
- All hits share the same RSP and RBP — single thread
- The render loop runs on the **main game thread** (not a separate render thread)
- Widget field modifications (position, visibility) persist across frames without synchronization
- **Implication:** Widget creation and field modification can be done from a hooked function on the game thread without locking. Avoid modifying widget data from a separate thread.

---

## §2 Widget Struct Layouts

### Class Hierarchy

| RTTI Class Name | Role | Vtable Offset |
|----------------|------|---------------|
| `kt::BmpfontSimpleString` | Text widget entry — owns descriptor + render state | gamemdx.dll+0x3668A8 |
| `kt::BmpfontString` | Container — groups child widgets, no rendering | gamemdx.dll+0x366948 |
| `agcs::BmpString` | Wrapper — owns child array, dispatches render with font globals | gamemdx.dll+0x367088 |
| `kt::anonymous namespace::BmpfontImpl` | Bitmap font renderer — referenced by render state +0x70 | (TBD) |

### kt::BmpfontSimpleString — Widget Entry

| Offset | Type | Field | Evidence |
|--------|------|-------|----------|
| +0x00 | ptr | vtable | gamemdx.dll+0x3668A8 — 16 virtual methods |
| +0x08 | ptr | text line descriptor | Points to descriptor struct |
| +0x10 | ptr | render state | Points to render state struct |
| +0x18 | 8 bytes | (zero) | — |
| +0x20 | 8 bytes | (zero) | — |
| +0x28 | 8 bytes | packed ID + flags | Pattern: 4-byte hash + 2-byte index + 2-byte flags (0x89xx) |
| +0x30 | 8 bytes | (zero) | — |
| +0x38 | ptr | intrusive list node A | Points to self+0x30 when unlinked |
| +0x40 | ptr | code pointer | callback/destructor |
| +0x48 | ptr | intrusive list node B | Points to self+0x40 when unlinked |
| +0x50 | qword | size/count | 0x18 (24) |
| +0x58 | 8 bytes | (zero) | — |

**Stride:** 0x60 bytes per entry (confirmed by repeating pattern in contiguous memory).

### Vtable — kt::BmpfontSimpleString (gamemdx.dll+0x3668A8)

| Index | Offset | Description | Evidence |
|-------|--------|-------------|----------|
| 0 | +0x1F4300 | Destructor | Decompiled: calls cleanup, optionally frees memory |
| 1 | +0x1F46E0 | **Text render/draw** | Decompiled: reads position, iterates lines, decodes UTF-8, renders glyphs |
| 2 | +0x1F7330 | getText | Rebuild text lines, return string ptr |
| 3 | +0x1FAB70 | stub | Returns 0 (no-op) |
| 4 | +0x1F7830 | getFontScaleX | descriptor+0x58 × global_factor |
| 5 | +0x1F7860 | getFontScaleY | descriptor+0x5C × global_factor |
| 6 | +0x1F7890 | getTotalHeight | line_count × (line_height + spacing) |
| 7 | +0x1F7930 | setFontScaleX | descriptor+0x58 = val / global_factor |
| 8 | +0x1F7970 | setFontScaleY | descriptor+0x5C = val / global_factor |
| 9 | +0x1F79B0 | getLineCount | (rs+0x10 - rs+0x08) / 0x18 |
| 10 | +0x1F79E0 | getGlyphCount | (rs+0x30 - rs+0x28) / 0x18 |
| 11 | +0x1F7A20 | getLineText | Copy line N into output buffer |
| 12 | +0x1F7AF0 | getLineWidth | Pixel width of line N |
| 13 | +0x1F7B60 | getLineHeight | descriptor+0x5C × line_height_factor |
| 14 | +0x1F6480 | getRenderStateFlag | render_state[0] |
| 15 | +0x1F6490 | setRenderStateFlag | render_state[0] = val |

### Text Line Descriptor (~0xC0 bytes)

The descriptor is NOT a polymorphic class (no RTTI/vtable). It is a plain struct owned by the widget entry at +0x08.

| Offset | Type | Field | Confirmed |
|--------|------|-------|-----------|
| +0x00 | ptr | string begin | ✅ CE scan + Ghidra decompile |
| +0x08 | ptr | string end | ✅ |
| +0x10 | ptr | string capacity | ✅ |
| +0x18 | 8 bytes | (zero) | |
| +0x20 | float | **color R** | **✅ Ghidra decompile of FUN_1801f4ff0** |
| +0x24 | float | **color G** | **✅** |
| +0x28 | float | **color B** | **✅** |
| +0x2C | float | **color A (alpha)** | **✅** |
| +0x30 | ptr | function pointer 1 | ✅ gamemdx.dll+0x201CC0 |
| +0x38 | ptr | function pointer 2 | ✅ gamemdx.dll+0x201D20 |
| +0x40 | ptr | function pointer 3 | ✅ gamemdx.dll+0x201E10 |
| +0x48 | byte | (unknown flag) | |
| **+0x49** | **byte** | **visibility** | **✅ CE write + Ghidra** |
| +0x4A | 2 bytes | (padding/flags) | |
| **+0x4C** | **float** | **X position** | **✅ CE read/write + Ghidra** |
| **+0x50** | **float** | **Y position** | **✅ CE read + Ghidra** |
| +0x54 | float | (zero) | |
| **+0x58** | **float** | **font scale X** | **✅ Ghidra decompile** |
| **+0x5C** | **float** | **font scale Y** | **✅ Ghidra decompile** |
| +0x60 | int | draw parameter | ✅ Ghidra |
| +0x64 | int | line height | ✅ Ghidra |
| +0x68 | float | clip left | ✅ Ghidra |
| +0x6C | float | clip right | ✅ Ghidra |
| +0x78 | dword | value | 15 |
| +0x7C | dword | value | 1 |
| +0x80 | float | (secondary color R?) | Ghidra: color processing |
| +0x84 | float | (secondary color G?) | Ghidra: color processing |
| +0x88 | float | (secondary color B?) | Ghidra: color processing |
| +0x8C | float | (secondary color A?) | Ghidra: color processing |
| +0x94 | float | param | ~0.2 |
| +0xA4 | float | param | 1.0 |
| **+0xA8** | **int** | **text direction** | **✅ Ghidra decompile** |
| **+0xAC** | **int** | **alignment** | **✅ Ghidra decompile** |
| +0xB0 | dword | render param | ✅ Ghidra |
| +0xB4 | int | clipping mode | ✅ Ghidra |

### Field Modification Tests (Visual Confirmation)

Performed on the "EVENT MODE" widget.

| Field | Change | Result |
|-------|--------|--------|
| X position (+0x4C) | 640→200 | Text moved from bottom-center to bottom-left | ✅ |
| Visibility (+0x49) | 1→0 | Text disappeared from screen | ✅ |
| Restore both | 0→1, 200→640 | Text reappeared at original position | ✅ |

**Key finding:** Position (+0x4C/+0x50) and visibility (+0x49) modifications persist across frames — the game does not overwrite these fields. However, the text string buffer at +0x00 is continuously refreshed by the game from source data each frame.

### Render State (plain struct, no RTTI)

Referenced by widget entry +0x10. Contains 3 std::vector-like containers and a font pointer.

| Offset | Type | Field |
|--------|------|-------|
| +0x00 | dword | (zero) |
| +0x04 | dword | sentinel (0xFFFFFFFF) |
| +0x08 | ptr | text lines vector begin |
| +0x10 | ptr | text lines vector end |
| +0x18 | ptr | text lines vector capacity |
| +0x20 | 8 bytes | (zero) |
| +0x28 | ptr | glyph data vector begin |
| +0x30 | ptr | glyph data vector end |
| +0x38 | ptr | glyph data vector capacity |
| +0x40 | 8 bytes | (zero) |
| +0x48 | ptr | control data vector begin |
| +0x50 | ptr | control data vector end |
| +0x58 | ptr | control data vector capacity |
| +0x60 | 8 bytes | (zero) |
| +0x68 | 8 bytes | (zero) |
| **+0x70** | **ptr** | **font object → `kt::anonymous namespace::BmpfontImpl`** |
| +0x78 | ptr | (null or second font) |

**Vector element layout (0x18 bytes each):**
| Offset | Type | Field |
|--------|------|-------|
| +0x00 | ptr | string pointer |
| +0x08 | qword | string length |
| +0x10 | qword | extra data (0 in text lines, float width in glyph data) |

---

## §3 Screen Coordinate System

### Coordinate System Properties

| Property | Value | Evidence |
|----------|-------|----------|
| **Internal resolution** | **1280 × 720** | X=640 is center; Y=700 is 20px from bottom of 720 |
| **Origin** | **(0, 0) = top-left** | X increases left→right; Y=700 is near screen bottom |
| **X axis** | **Left → Right (0 → 1280)** | X=640 is center of screen |
| **Y axis** | **Top → Bottom (0 → 720)** | Y=700 places text near bottom edge |
| **Units** | **Pixels (float)** | Positions stored as IEEE 754 single-precision floats |
| **Aspect ratio** | **16:9** | 1280/720 = 16/9 |

### Dual Confirmation

- **Cheat Engine (runtime):** Float values read directly from text line descriptors at +0x4C and +0x50.
- **Ghidra (static):** Decompiled `FUN_1801F46E0` (text rendering function at +0x1F46E0) reads `*(float *)(descriptor + 0x4c)` as the X position for clipping and layout calculations. The function also performs UTF-8 character decoding (byte range checks for 0xC0, 0xE0, 0xF0, 0xF8) confirming multi-byte text support.

---

## §4 Render Tree Structure

### Architecture Overview

DDR World uses a **command-list rendering** architecture, not a traditional parent-child scene graph tree.

| System | Namespace | Role |
|--------|-----------|------|
| **Actor system** | `me::task::actor::Actor`, `agcs::Actor` | Game logic lifecycle (scenes, sequences) |
| **Scene graph** | `agcs::scene::Node`, `SceneGraphManager` | High-level scene management |
| **Render command lists** | `gs::Renders::ScreenCommandList` | Low-level draw command submission |
| **Bitmap font system** | `kt::Bmpfont*` | Text widget rendering |

### Class Hierarchy (from RTTI)

```
kt::Bmpfont (base)
├── kt::BmpfontString (container/group — no rendering, null descriptor)
│   └── kt::BmpfontSimpleString (text widget — renders via descriptor)
└── kt::anonymous namespace::BmpfontImpl (font renderer)

agcs::Actor
└── Application::RootActor
    └── Application::ArkActor
        └── sequence::common::BgMovieActor (found near widget memory)

agcs::scene::Node (scene graph base)
SceneGraphManager::NodeUpdateJob (scene update)
gs::Viewport::Template<ScreenCommandList::Scene, ScreenCommandList::Render> (render viewport)
```

### Scene Graph Observations

- BmpfontSimpleString widgets are **NOT linked** into a tree via their intrusive list nodes (+0x38, +0x48) — all nodes are self-referencing (unlinked).
- Widgets are registered with the render system through a **container node** mechanism, which holds a widget pointer at +0x20.
- The render function (vtable[1]) is called via **virtual dispatch** — no direct callers found in static analysis.
- `gs::Viewport::Template<ScreenCommandList>` objects exist in the descriptor memory region, suggesting a command-list-based rendering pipeline.

### Render Loop (FUN_1801FFBA0, offset +0x1FFBA0)

This is the **BmpfontString container render method** — the function that iterates child widgets and renders them each frame.

**Input**: `param_1` = render state of the BmpfontString container

**Algorithm**:
1. Iterate a **linked list** starting at `render_state + 0x28`
2. For each node, check visibility flag at `node_entry + 0x28`
3. Extract widget pointer from `node_entry + 0x20`
4. Check widget flags at `widget + 0x10` (must be 0) and `widget + 0x12` (must be non-zero)
5. Collect visible widgets into a `std::vector`
6. Sort the vector via `FUN_180200440` (offset +0x200440, calls merge sort at +0x200670 — sorts by priority at entry+0xC)
7. Set up coordinate transforms from render state (+0x48/+0x4C position, +0x50/+0x54 scale)
8. Write render commands to the `ScreenCommandList` (command type 7 = transform, type 0x10000C = clip)
9. Call `vtable[5]` (+0x28) on each sorted widget — the actual draw dispatch

**Linked list node layout** (at render_state + 0x28):
```
node[0] = pointer to child entry
node[1] = next node pointer (NULL = end of list)
```

**Child entry layout** (pointed to by node[0]):
```
+0x00: unknown
+0x10: byte flag (must be 0 for rendering)
+0x12: byte flag (must be non-zero for rendering)
+0x20: pointer to widget (kt::BmpfontSimpleString)
+0x28: byte visibility flag (0 = visible, non-zero = hidden)
```

**Render state fields used by the loop**:
| Offset | Type | Field |
|--------|------|-------|
| +0x28 | ptr | linked list head (child entries) |
| +0x38 | int | initial vector capacity |
| +0x48 | float | container X position |
| +0x4C | float | container Y position |
| +0x50 | float | container X scale |
| +0x54 | float | container Y scale |
| +0x58 | float | X transform multiplier |
| +0x5C | float | Y transform multiplier |
| +0x60 | byte | clipping enabled flag |

### Widget Ownership Architecture

The rendering system registers `agcs::BmpString` wrappers with a **scene manager** at `DAT_1806B5AA8` (offset +0x6B5AA8). Each wrapper is inserted into a linked list accessible at `scene_manager + 0xB0`, which the render loop iterates each frame.

Wrappers are stored as fields of scene actor objects and registered with the scene manager's linked list — there is no flat global array.

**Wrapper registration pattern** (observed in FUN_1800812e0, FUN_1800a57a0):
1. Create wrapper via `FUN_180201E90` (BmpString wrapper constructor)
2. Store wrapper pointer in actor object field
3. Get scene manager linked list head from `DAT_1806B5AA8 + 0xB0`
4. Allocate a linked list node, set `node+0x20 = wrapper`, `node+0x10 = wrapper`
5. Append node to the linked list tail

---

## §5 Text Widget Lifecycle

### Constructor: FUN_1801F4260 (offset +0x1F4260)

Creates a new `kt::BmpfontSimpleString` widget. Signature:
```c
BmpfontSimpleString* FUN_1801F4260(void* widget_mem, ?, ?, heap_param);
```

**Construction sequence:**
1. Set vtable to `kt::BmpfontString::vftable` (base class, +0x366948)
2. Set descriptor pointer (+0x08) to NULL
3. Allocate text line descriptor (0xC0 bytes) via `FUN_18022DF40(heap, 0xC0, 0, param4, -2)`
4. Initialize descriptor via `FUN_1801F8170(descriptor)`
5. Store descriptor pointer at widget+0x08
6. Upgrade vtable to `kt::BmpfontSimpleString::vftable` (+0x3668A8)
7. Set render state pointer (+0x10) to NULL
8. Allocate render state (0x128 bytes) via `FUN_18022DF40(heap, 0x128, 0, param4, -2)`
9. Initialize render state via `FUN_1801F3EF0(render_state)`
10. Store render state pointer at widget+0x10

**Key functions:**
| Offset | Function | Role |
|--------|----------|------|
| +0x1F4260 | Constructor | Creates BmpfontSimpleString |
| +0x1F8170 | Descriptor init | Initializes 0xC0-byte text line descriptor |
| +0x1F3EF0 | Render state init | Initializes 0x128-byte render state |
| +0x22DF40 | Memory allocator | `alloc(heap, size, 0, param, -2)` |

**Heap pointer:** `PTR_DAT_18042E1E8` (global, offset +0x42E1E8) — the game's memory allocator

### Descriptor Initializer: FUN_1801F8170 (offset +0x1F8170)

Sets default values for a 0xC0-byte text line descriptor:
| Offset | Default | Type | Field |
|--------|---------|------|-------|
| +0x00-0x10 | 0 | ptr×3 | string begin/end/capacity (empty) |
| +0x20 | 1.0f | float | scale factor A |
| +0x24 | 1.0f | float | scale factor B |
| +0x28 | 1.0f | float | scale factor C |
| +0x2C | 1.0f | float | scale factor D |
| +0x48 | 0x0100 | word | flags (byte +0x49 = 1 = visible) |
| +0x4C | 0.0f | float | X position |
| +0x50 | 0.0f | float | Y position |
| +0x58 | 1.0f | float | font scale X |
| +0x5C | 1.0f | float | font scale Y |
| +0x74 | 1 | int | flag |
| +0x84 | 1.0f | float | opacity? |
| +0x8C | 0.2f | float | default parameter |
| +0x9C | 1.0f | float | default parameter |
| +0xA4 | 1 | int | flag |
| +0xAC | 0 | int | alignment (0=left) |
| +0xB0 | 1 | int | flag |

### Render State Initializer: FUN_1801F3EF0 (offset +0x1F3EF0)

Sets default values for a 0x128-byte render state:
| Offset | Default | Type | Field |
|--------|---------|------|-------|
| +0x00 | 0xFFFFFFFFFFFFFFFF | qword | sentinel (game later sets +0x00 to 0) |
| +0x08-0x18 | 0 | ptr×3 | text lines vector (begin/end/capacity) |
| +0x28-0x38 | 0 | ptr×3 | glyph data vector |
| +0x48-0x58 | 0 | ptr×3 | control data vector |
| +0x68 | 1 | byte | initialized flag |
| +0x70 | 0 | ptr | font object (set later) |
| +0x78 | 0 | ptr | second font slot |
| +0x80 | 1 | qword | flag |
| +0x94 | 1.0f | float | scale |
| +0xB0 | resource | ptr | "gs_screencommand_default" |
| +0xB8 | resource | ptr | "gs_screencommand_font" |
| +0xC0 | resource | ptr | "scr_distancefont" |
| +0xC8 | resource | ptr | "scr_distancefont_border" |
| +0xE0-0x10C | globals | float×12 | 3 sets of RGBA color values |
| +0x110 | 1.0f | float | scale X |
| +0x114 | 1.0f | float | scale Y |

**Resource lookup:** `(*DAT_1806B4BB0)(name_string, name_length)` — resolves named resources (screen commands, fonts).

### Widget Creation Recipe

```
1. Get heap pointer from PTR_DAT_18042E1E8 (offset +0x42E1E8)
2. Allocate 0x60 bytes for widget entry
3. Call FUN_1801F4260(widget_mem, 0, 0, heap_param) — constructor
   - This internally allocates and initializes descriptor (0xC0) and render state (0x128)
4. Set descriptor+0x4C = X position (float)
5. Set descriptor+0x50 = Y position (float)
6. Set descriptor+0x58/0x5C = font scale (float, default 1.0)
7. Write UTF-8 text to a buffer, set descriptor+0x00/0x08/0x10 = begin/end/capacity
8. Set render_state+0x70 = font object pointer (copy from existing widget)
9. Render the widget by hooking wrapper vtable[5]
```

**Important**: Do NOT manually allocate and copy widget data — the render state's internal vectors use ref-counted allocators that crash if not properly initialized. Always use the game's constructor.

### Widget Destruction

**Destructor: FUN_1801F4300 (vtable[0], offset +0x1F4300)**

```c
void destructor(widget* this, uint flags) {
    cleanup_helper(this);           // FUN_1801F4330
    if (flags & 1) {
        free_memory(this);          // FUN_1801DBD10
    }
    return this;
}
```

**Cleanup helper: FUN_1801F4330 (offset +0x1F4330)**

Performs full cleanup of the widget's render state vectors and descriptor:
1. Sets vtable back to `kt::BmpfontSimpleString::vftable`
2. Reads render state from `widget[2]` (widget+0x10)
3. Frees control data vector (rs+0x48) — releases backing storage via ref-counted allocator
4. Zeros rs+0x48/0x50/0x58
5. Frees glyph data vector (rs+0x28) — same ref-counted release
6. Zeros rs+0x28/0x30/0x38
7. Frees text lines vector (rs+0x08) — same ref-counted release
8. Zeros rs+0x08/0x10/0x18
9. Frees the render state itself via ref-counted release
10. Sets widget+0x10 = 0
11. Calls FUN_1801F8260 to clean up the descriptor (BmpfontString base destructor)

Each vector's backing storage uses a **ref-counted allocator** pattern: the allocation header is at `buffer - 0x20`, containing a vtable and ref count at +0xC. The cleanup decrements the ref count and frees when it reaches 0.

### Inline Text Formatting (Control Codes)

The render function (+0x1F46E0) supports **inline control codes** embedded in the text stream via the control data vector (rs+0x48).

**Control code types** (from render function decompile):

| Code | Effect | Mechanism |
|------|--------|-----------|
| 1 | **Font switch** | Calls descriptor+0x30 function pointer with the control byte, then FUN_1801F5ED0 to recalculate metrics |
| 2 | **Color change** | Reads RGBA floats, converts to 0-255 bytes, writes to render state +0xA0-0xA3. Each channel has an enable flag. |
| 3 | **Color reset** | Restores color from render state +0x98 |
| 4 | **Inline image/icon** | Calls descriptor+0x40 function pointer, renders an inline sprite using the secondary font at rs+0x78 |

**Control data vector entry layout** (0x18 bytes, at rs+0x48):

| Offset | Type | Field |
|--------|------|-------|
| +0x00 | ptr | marker byte pointer (position in text stream) |
| +0x08 | word | character advance (bytes to skip in text) |
| +0x0A | short | control type (1=font, 2=color, 3=reset, 4=icon) |
| +0x0C | float | width parameter |
| +0x10 | float | height parameter |
| +0x18 | ptr | next marker (for type 4 icon chaining) |

**Practical text formatting:**

For simple use cases (different colors per widget, different sizes), modify the descriptor fields directly:
- **Color**: descriptor +0x20-0x2C (RGBA floats, 0.0-1.0)
- **Font scale**: descriptor +0x58/0x5C (X/Y scale)
- **Alignment**: descriptor +0xAC (0=left, 1=center, 2=right)
- **Text direction**: descriptor +0xA8 (0=default, 1=reversed width calc, 2=negative width)

For inline formatting (color changes within a single text string), the control data vector must be populated with the appropriate entries.

---

## §6 Font System & Text Rendering Pipeline

### Complete Vtable Map (kt::BmpfontSimpleString, +0x3668A8)

| Index | Offset | Signature | Role |
|-------|--------|-----------|------|
| 0 | +0x1F4300 | `void destructor(widget*)` | Cleanup and free |
| 1 | +0x1F46E0 | `void render(widget*)` | Main text rendering |
| 2 | +0x1F7330 | `char* getText(widget*)` | Rebuild text lines, return string ptr |
| 3 | +0x1FAB70 | `int stub()` | Returns 0 (no-op) |
| 4 | +0x1F7830 | `float getFontScaleX(widget*)` | descriptor+0x58 × global_factor |
| 5 | +0x1F7860 | `float getFontScaleY(widget*)` | descriptor+0x5C × global_factor |
| 6 | +0x1F7890 | `float getTotalHeight(widget*)` | line_count × (line_height + spacing) |
| 7 | +0x1F7930 | `float setFontScaleX(widget*, float)` | descriptor+0x58 = val / global_factor |
| 8 | +0x1F7970 | `float setFontScaleY(widget*, float)` | descriptor+0x5C = val / global_factor |
| 9 | +0x1F79B0 | `int getLineCount(widget*)` | (rs+0x10 - rs+0x08) / 0x18 |
| 10 | +0x1F79E0 | `int getGlyphCount(widget*)` | (rs+0x30 - rs+0x28) / 0x18 |
| 11 | +0x1F7A20 | `void getLineText(widget*, out*, idx)` | Copy line N into output buffer |
| 12 | +0x1F7AF0 | `float getLineWidth(widget*, idx)` | Pixel width of line N |
| 13 | +0x1F7B60 | `float getLineHeight(widget*)` | descriptor+0x5C × line_height_factor |
| 14 | +0x1F6480 | `int getRenderStateFlag(widget*)` | render_state[0] |
| 15 | +0x1F6490 | `int setRenderStateFlag(widget*, int)` | render_state[0] = val |

### Text Setting (String Ownership)

There is **no SetText vtable method**. Text is set by directly writing to the descriptor's std::string fields:

```
descriptor+0x00 = pointer to UTF-8 string start
descriptor+0x08 = pointer to string end (exclusive)
descriptor+0x10 = pointer to buffer capacity end
```

The game refreshes text content every frame from source data — direct buffer writes are overwritten. For newly created widgets (not modifying existing ones), this is not a problem: the creator owns the string buffer and nothing else writes to it.

After setting the string, set `render_state+0x68 = 1` (dirty flag) to trigger text line rebuild on next render.

### Text Line Splitting (FUN_1801F7160, offset +0x1F7160)

Called by vtable[2] (getText at +0x1F7330) and internally by the render function. Splits the descriptor's string into lines:

1. Reads UTF-8 string from descriptor+0x00
2. Scans for line break characters: `\r\n` (2 chars), `\n` (1 char), `\r` (1 char)
3. For each line, appends a 0x18-byte entry to the text lines vector (render_state+0x08)

### Glyph Lookup System

Three global font managers handle character-to-glyph mapping:

| Global | Ghidra Offset | Descriptor Fn | Role |
|--------|--------------|---------------|------|
| DAT_1806B5D68 | +0x6B5D68 | +0x201CC0 | Glyph metrics (advance width, bearing) |
| DAT_1806B5D70 | +0x6B5D70 | +0x201D20 | Glyph UV/texture coordinates |
| DAT_1806B5D78 | +0x6B5D78 | +0x201E10 | Distance font glyph data (SDF) |

Each lookup function:
1. Takes a character code and font type (1=normal, 8=alternate)
2. Calls the font manager's vtable[1] method
3. Returns glyph data (metrics, UVs, or SDF data)

UV coordinates are normalized by dividing by `DAT_18036D284` (offset +0x36D284, texture atlas size).

### Font Scale System

Font scale values in the descriptor (+0x58, +0x5C) are stored in **internal units** — divided by a global screen resolution factor. The vtable getters/setters handle the conversion:
- `setFontScaleX(widget, pixels)` → stores `pixels / global_factor` at descriptor+0x58
- `getFontScaleX(widget)` → returns `descriptor+0x58 × global_factor`

For direct descriptor field writes, use the raw internal values. Copy from an existing widget's descriptor+0x58/+0x5C for a matching scale, or set to 1.0f for the default.

### Initialization Timing & Readiness Signal

**Global dependency analysis:**

| Global | Ghidra Offset | Section | When Available |
|--------|--------------|---------|----------------|
| Heap pointer | +0x42E1E8 | .data (PE-initialized) | Immediately at DLL load |
| Resource manager fn | +0x6B4BB0 | .bss (zero-init) | Runtime — after game startup |
| Render context | +0x6B5D40 | .bss (zero-init) | Runtime — after game startup |
| Color globals | +0x6B60B0 | .bss (zero-init) | Runtime — after game startup |
| BmpfontImpl (font) | heap-allocated | heap | Runtime — after `FontInitActor` completes |

**PE section layout:**
- `.data` raw size: 0x6CC00 (445,440 bytes) — PE-initialized portion
- `.data` virtual size: 0xE31678 (~14.8 MB) — includes BSS (zero-initialized at runtime)
- Heap pointer at .data offset 0x140E8 → within raw size → **baked into PE image**
- Resource manager at .data offset 0x29A9A8 → beyond raw size → **BSS, written at runtime**

**Readiness strategies:**

```
Strategy A — Hook render function (safest):
  1. AOB-scan for render function (+0x1F46E0) or render loop (+0x1FFBA0)
  2. Hook on first match
  3. On first invocation:
     a. Read existing widget's render_state+0x70 to get font pointer
     b. Call constructor (+0x1F4260) to create new widget
     c. Copy font pointer into new widget's render_state+0x70
  4. On every invocation: call new widget's vtable[1] after original returns
  → Guaranteed safe: if the render function is running, ALL dependencies are initialized.

Strategy B — Poll for readiness (for early injection):
  1. Poll DAT_1806B4BB0 (+0x6B4BB0) until non-NULL → core systems ready
  2. Poll for BmpfontImpl vtable in heap → fonts loaded
  3. Then safe to call constructor
  → Allows earlier initialization but requires polling loop.
```

---

## §7 AOB Signatures

All signatures validated as unique within gamemdx.dll via AOB scan. Wildcards (`??`) replace RIP-relative displacements that change between builds.

### Critical Functions

| Function | Offset | AOB Signature | Offset to Target |
|----------|--------|---------------|-----------------|
| **Wrapper render** | +0x202170 | `48 83 EC 28 48 8B 49 18 48 8B 41 08 48 89 05 ?? ?? ?? ?? 48 8B 41 10 48 89 05 ?? ?? ?? ?? 48 8B 41 18 48 8B 09 48 89 05 ?? ?? ?? ?? 48 8B 01 FF 50 08` | +0 |
| **Render function** | +0x1F46E0 | `4C 8B DC 55 53 49 8D AB 68 FF FF FF 48 81 EC 88 01 00 00 48 8B 05 ?? ?? ?? ?? 48 33 C4 48 89 45 C8 48 8B 41 08 48 8B D9 80 78 49 00` | +0 |
| **Widget factory** | +0x1F41B0 | `40 57 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 40 48 89 6C 24 48 48 89 74 24 50 41 8B` | +0 |
| **Constructor** | +0x1F4260 | `48 89 4C 24 08 57 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 50` | +0 (preceded by `CC` padding) |

### Supporting Functions

| Function | Offset | AOB Signature | Offset to Target |
|----------|--------|---------------|-----------------|
| **Descriptor init** | +0x1F8170 | `33 C0 48 89 01 48 89 41 08 48 89 41 10 C7` | +0 |
| **Render state init** | +0x1F3EF0 | `48 89 4C 24 08 57 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 48 48 8B D9 33 FF 48 89 79 08 48 89 79 10 48 89 79 18 48 89 79 28` | +0 |
| **Memory allocator** | +0x22DF40 | `48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 48 89 7C 24 20 41 54 48 83 EC 20 48 8B 01 49 8B D8` | +0 |
| **BmpString wrapper ctor** | +0x201E90 | `48 89 4C 24 08 57 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 48 48 89 6C 24 50 48 89 74 24 58 49 8B F8 8B F2 48 8B D9 33 ED 48 C7 41 08` | +0 |
| **Render loop** | +0x1FFBA0 | `48 8B C4 55 53 56 57 41 54 41 55 41 56 41 57 48 8D 68 98 48 81 EC 28 01 00 00 48 C7 44 24 38 FE FF FF FF 0F 29 70 A8 0F 29 78 98 44 0F 29 40 88` | +0 |
| **Text line splitter** | +0x1F7160 | `40 53 56 57 48 83 EC 40 48 8B 79 08 48 8B 47 08 48 39 07` | +0 |

### Derived Addresses (no AOB needed)

| Item | How to Derive |
|------|--------------|
| **Destructor** (+0x1F4300) | Read vtable[0] from any BmpfontSimpleString widget |
| **BmpfontSimpleString vtable** (+0x3668A8) | Scan for RTTI string `.?AVBmpfontSimpleString@kt@@`, trace TypeDescriptor → COL → vtable |
| **agcs::BmpString vtable** (+0x367088) | Scan for RTTI string `.?AVBmpString@agcs@@`, trace TypeDescriptor → COL → vtable |
| **Font manager globals** (+0x6B5D68/70/78) | Read from wrapper render function's `mov [rip+disp]` instructions |
| **Render context** (+0x6B5D40) | Read from render loop's `mov rcx,[rip+disp]` instruction |
| **Heap pointer** (+0x42E1E8) | Read from constructor's `mov rax,[rip+disp]` instruction |

---

## §7a Key Functions Reference

| Ghidra Address | Offset | Description |
|----------------|--------|-------------|
| `0x1801F46E0` | +0x1F46E0 | Main text rendering function (vtable[1]) |
| `0x1801F55B0` | +0x1F55B0 | Text segment draw (called from render function) |
| `0x1801F7720` | +0x1F7720 | Line height/spacing calculator |
| `0x180201CC0` | +0x201CC0 | Glyph lookup fn 1 (descriptor +0x30) |
| `0x180201D20` | +0x201D20 | Glyph lookup fn 2 (descriptor +0x38) |
| `0x180201E10` | +0x201E10 | Glyph lookup fn 3 (descriptor +0x40) |
| `0x1801F4260` | +0x1F4260 | Constructor (BmpfontSimpleString) |
| `0x1801F8170` | +0x1F8170 | Descriptor initializer |
| `0x1801F3EF0` | +0x1F3EF0 | Render state initializer |
| `0x18022DF40` | +0x22DF40 | Memory allocator |
| `0x1801F41B0` | +0x1F41B0 | Widget factory |
| `0x180201E90` | +0x201E90 | BmpString wrapper constructor |
| `0x1801FFBA0` | +0x1FFBA0 | Render loop (container render) |
| `0x180200440` | +0x200440 | Sort function (wrapper) |
| `0x1801F7160` | +0x1F7160 | Text line splitter |
| `0x180202170` | +0x202170 | Wrapper render method (agcs::BmpString vtable[5]) |

---

## §8 Integration Findings

### Rendering Context Requirement

Draw commands are ONLY visible when issued from within the wrapper's vtable[5] context. Calling the render function from any other hook point (render loop post-dispatch, timer, etc.) produces invisible output. This was validated via Cheat Engine proof-of-concept.

The render pipeline requires: render loop → wrapper vtable[5] (sets up ScreenCommandList context + font globals) → widget vtable[1] (renders text). Skipping the wrapper context makes draw commands invisible.

### agcs::BmpString Wrapper (0x20 bytes)

| Offset | Type | Field |
|--------|------|-------|
| +0x00 | ptr | `agcs::BmpString` vtable |
| +0x08 | qword | count/flag (1) |
| +0x10 | word | flags (0x0100 = visible) |
| +0x12 | byte | enabled flag (1) |
| +0x18 | ptr | child array (4 × qword, each a widget pointer or NULL) |

### Widget Factory: FUN_1801F41B0 (offset +0x1F41B0)

Creates a `kt::BmpfontSimpleString` widget (0x18 bytes only):
```c
BmpfontSimpleString* createWidget(font_name, font_size, font_type, heap_param);
// font_type: 1=normal, 2=bold?, 4=?, 6=?, 8=alternate
```

1. Validates font_type (must be 1, 2, 4, 6, or 8)
2. Allocates 0x18 bytes via game heap
3. Calls constructor (+0x1F4260)
4. Calls vtable[2] to initialize font
5. Calls vtable[15] to set render state flag

### BmpString Wrapper Constructor: FUN_180201E90 (offset +0x201E90)

Creates a full wrapper with widget:
```c
BmpStringWrapper* createBmpString(wrapper_mem, font_size, font_name, heap_param);
```

1. Initializes wrapper fields (vtable, flags, visibility)
2. Allocates child array (0x20 bytes, 4 slots)
3. Calls widget factory to create BmpfontSimpleString
4. Stores widget in child array slot 0
5. Sets glyph lookup function pointers on the widget's descriptor

### agcs::BmpString Wrapper Render Method (+0x202170)

```c
void BmpString_render(BmpStringWrapper* this) {  // vtable[5], offset +0x202170
    child_array = this->child_array;              // [rcx+18]
    DAT_font_mgr_1 = child_array->fn1;            // [child+08] → global +0x6B5D68
    DAT_font_mgr_2 = child_array->fn2;            // [child+10] → global +0x6B5D70
    DAT_font_mgr_3 = child_array->fn3;            // [child+18] → global +0x6B5D78
    widget = child_array->widget;                  // [child+00]
    widget->vtable[1](widget);                     // call render function
    DAT_font_mgr_3 = 0;                           // clear globals
    DAT_font_mgr_2 = 0;
    DAT_font_mgr_1 = 0;
}
```

### Key Constraint: Child Array Slot Writing Crashes

Writing a widget pointer to an empty child slot in an existing wrapper CRASHES the game. The child array is not directly iterated by the render loop — an intermediate structure (linked list) is built from it. Simply writing to the array corrupts the render pipeline.

---

## §9 Image/Sprite Widget System

### Class Hierarchy

| RTTI Class | Vtable Offset | Role |
|-----------|---------------|------|
| `agcs::Sprite` | +0x367038 | Sprite wrapper — analogous to agcs::BmpString for text |
| `agcs::BM2DGroup` | +0x366DF8 | BM2D animation group container |
| `BM2D::CSprite` | +0x36C7D8 | Low-level BM2D sprite (Flash-like animation system) |

### agcs::Sprite Struct Layout

The sprite object is rendered by the same dispatch loop as text widgets (render loop calls wrapper vtable[5]).

| Offset | Type | Field |
|--------|------|-------|
| +0x28 | int | texture ID (GPU texture handle, 0 = no texture) |
| +0x2C | int | blend mode (0-16) |
| +0x30 | float | X position |
| +0x34 | float | Y position |
| +0x38 | float | width |
| +0x3C | float | height |
| +0x40 | float | scale X |
| +0x44 | float | scale Y |
| +0x48 | float | anchor X (0.0-1.0) |
| +0x4C | float | anchor Y (0.0-1.0) |
| +0x50 | float | rotation (radians) |
| +0x54 | float | UV left |
| +0x58 | float | UV top |
| +0x5C | float | UV right |
| +0x60 | float | UV bottom |
| +0x64 | dword | color/alpha (vertex color) |

### Sprite Render Method (vtable[5], +0x2015F0)

Writes two ScreenCommandList commands per frame:
1. **Blend/texture setup** (type 8): selects blend mode, binds texture ID from +0x28
2. **Quad vertices** (type 4): computes 4 rotated corner positions from position/size/anchor/rotation, writes UV coords and color

Rotation uses sin/cos of angle at +0x50 to compute rotated quad corners.

---

## Appendix A: RTTI Strings (Ghidra addresses)

| String | Ghidra Address |
|--------|---------------|
| `kt::BmpfontSimpleString` | `0x180483640` |
| `kt::BmpfontString` | `0x180483618` |
| `kt::anonymous namespace::BmpfontImpl` | `0x1804835E0` |
| `agcs::BmpString` | `0x180483A40` |
| `agcs::BmpString::debug` | `0x180483130` |

## Appendix B: Resource Strings (Ghidra addresses)

| String | Ghidra Address |
|--------|---------------|
| `gs_screencommand_default` | `0x18033D700` |
| `gs_screencommand_font` | `0x180366138` |
| `scr_distancefont` | `0x180366758` |
| `scr_distancefont_border` | `0x180366770` |
