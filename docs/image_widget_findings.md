# Image Widget — Render List Registration Findings

---

## Working Approach: Direct Render List Registration

Sprites (`agcs::Sprite`) can be registered directly in the game's render list, the same way text widgets (`agcs::BmpString` wrappers) are registered. The render_loop calls `vtable[5]` on each registered object — for sprites, this is the sprite render method (FUN_1802015f0), which writes blend/texture/quad commands to the ScreenCommandList.

### Why This Works

The render_loop (FUN_1801ffba0) is type-agnostic. It checks:
1. `data_area+0x28 == 0` (visibility)
2. `*(wrapper+0x10) == 0` (flags byte — sprite has 0x0100, byte at +0x10 = 0x00 ✓)
3. `*(wrapper+0x12) != 0` (enabled byte — sprite has 1 ✓)

Then calls `wrapper->vtable[5](wrapper)`. For sprites, vtable[5] = FUN_1802015f0:
```
FUN_1802015f0(sprite):
  FUN_180201610(sprite)  — blend mode switch + texture bind to ScreenCommandList
  FUN_180201980(sprite)  — quad vertex computation from position/size/anchor/rotation/UV/color
```

Both sub-functions only need:
- The sprite struct fields (+0x28..+0x64)
- The ScreenCommandList global (DAT_1806b5d40, set up by the render pass)

No parent container, no font globals, no game allocator metadata required.

### Allocation Strategy

Sprites should be allocated via `VirtualAlloc` (or equivalent), NOT the game's allocator (`FUN_18022df40`). The game allocator tracks allocations with metadata at `(result - 0x20, -0x18, -0x10)` including a back-pointer to the heap. Scene cleanup iterates these tracked allocations. Externally-allocated sprites, not being properly managed game objects, would crash during cleanup if allocated from the game heap.

`VirtualAlloc` memory is invisible to the game's cleanup system. The render list nodes come from the game's pre-allocated pool (which is correct — those MUST be used).

### ref_count = 0x7FFFFFFF

Set to max value to prevent any cleanup code from decrementing it to zero and calling the destructor. The sprite lives for the entire session.

### Visibility via enabled byte (+0x12)

The `enabled` byte at sprite+0x12 controls visibility. The render_loop checks this natively — when 0, the sprite is skipped entirely (no vtable call, no ScreenCommandList writes). This is cleaner and more efficient than rendering a fully transparent sprite.

---

## agcs::Sprite Object Layout (0x68 bytes)

| Offset | Size | Type | Field | Default |
|--------|------|------|-------|---------|
| +0x00 | 8 | ptr | vtable | `agcs::Sprite::vftable` (found via RTTI) |
| +0x08 | 4 | i32 | ref_count | 0x7FFFFFFF |
| +0x10 | 2 | u16 | flags | 0x0100 |
| +0x12 | 1 | u8 | enabled | 1 |
| +0x18 | 8 | ptr | child | null |
| +0x28 | 4 | i32 | texture_id | 0 (no texture) |
| +0x2C | 4 | i32 | blend_mode | 1 (alpha blend) |
| +0x30 | 4 | f32 | x | 0.0 |
| +0x34 | 4 | f32 | y | 0.0 |
| +0x38 | 4 | f32 | width | 0.0 |
| +0x3C | 4 | f32 | height | 0.0 |
| +0x40 | 4 | f32 | scale_x | 1.0 |
| +0x44 | 4 | f32 | scale_y | 1.0 |
| +0x48 | 4 | f32 | anchor_x | 0.0 |
| +0x4C | 4 | f32 | anchor_y | 0.0 |
| +0x50 | 4 | f32 | rotation | 0.0 (radians) |
| +0x54 | 4 | f32 | uv_left | 0.0 |
| +0x58 | 4 | f32 | uv_top | 0.0 |
| +0x5C | 4 | f32 | uv_right | 1.0 |
| +0x60 | 4 | f32 | uv_bottom | 1.0 |
| +0x64 | 4 | u32 | color (ABGR) | 0xFFFFFFFF |

### Sprite Vtable (found via RTTI `.?AVSprite@agcs@@`)

| Index | Offset | Purpose |
|-------|--------|---------|
| [0] | +0x00 | destructor |
| [1] | +0x08 | set_size (NOT render!) |
| [2] | +0x10 | set_color |
| [3] | +0x18 | unknown |
| [4] | +0x20 | no-op (base class) |
| [5] | +0x28 | **render method** (FUN_1802015f0) |

### Blend Modes (+0x2C)

| Value | Description |
|-------|-------------|
| 0 | No blend |
| 1 | Standard alpha blend (default) |
| 2 | Additive |
| 3 | Additive variant |
| 4 | Multiply |
| 6 | Screen |
| 7 | Overlay |

---

## Crash Root Cause Analysis

All crashes during earlier image widget attempts were caused by `texture_resolver::resolve()` being called from the render thread, NOT by sprite allocation or render list registration.

The `get_bitmap_info` callback in libafp triggers AVS logging on failure. AVS logging crashes when called from within the render pass. It also holds game-internal locks that conflict with scene transition locks.

### Safe texture resolution rules:
- NOT from within the render pass (AVS assertion crash)
- NOT while holding any mutex that scene callbacks need (deadlock)
- From a separate thread context with state locks dropped, only when the asset loader has confirmed loading is complete

---

## References

- Ghidra: FUN_1801ffba0 (render_loop) — walks render list, calls vtable[5]
- Ghidra: FUN_1802015f0 (sprite render) — blend setup + quad vertex computation
- Ghidra: FUN_180201610 (blend/texture bind) — switch on +0x2C, texture command 0x11
- Ghidra: FUN_180201980 (quad vertices) — position/size/anchor/rotation math
- Ghidra: FUN_180201210 (sprite_init) — field defaults
