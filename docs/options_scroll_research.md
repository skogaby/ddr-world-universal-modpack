# Options-Menu Scroll & Row Layout Research

Reverse engineering for the scroll driver attached to the per-player options menu (P1/P2 OptionForm), used by the custom-options framework (`20260506-custom-options-support`) to handle tabs whose row count exceeds the native viewport.

**Game binary**: `gamemdx.dll` (MDX-003_20260324)
**Ghidra base**: `0x180000000`
**Prerequisite**: [custom_player_options_research.md](custom_player_options_research.md)

---

## Problem Statement

The options menu's layout container holds a flat vector of `OptionElement<KIND>` rows across ALL tabs (Basic / Arrows / Lane / Judge / Assist / Mods). Per-tab visibility is handled by the custom-options framework's `filter_hook` (already implemented), which flips each row's `+0xB8` "active" byte on tab-switch.

Native DDR World never ships a tab with more rows than fit in the 7-row viewport. Once mods inject rows on the Mods tab — or multi-tag existing rows onto a native tab that's already near capacity — row count exceeds viewport and overflow rows render **outside the visual options panel, unclipped** (no engine-side Y-culling).

Task 7 of the feature needs a scroll driver that:
1. Keeps the visible window at exactly 7 rows
2. Lets the cursor scroll through all rows in a tab
3. Clips off-screen rows out of view
4. Preserves scroll position per-(side, page) within a session

---

## Layout Container Struct (confirmed via live memory + disassembly)

The "layout container" is the object at `*(OptionForm+0x230)`. The `custom_options::filter_hook` already uses this pointer as its scene-layout anchor. One container exists per OptionForm instance (one per player side).

### Field layout (relevant subset)

| Offset | Type | Field | Notes |
|--------|------|-------|-------|
| +0x68 | ptr | `row_vec_begin` | Flat `std::vector<Row*>` begin |
| +0x70 | ptr | `row_vec_end` | End (size = (end-begin)/8) |
| +0x78 | ptr | `row_vec_cap` | Capacity |
| +0xA0 | f64 | `anchor_x` | Layout-base x anchor |
| +0xA8 | f64 | `anchor_y` | Layout-base y anchor |
| +0xD0/+0xD8/+0xE0 | f64×3 | `layout_origin` | Position accumulator seed in `FUN_18004a720` |
| +0x100 | u8 | `animate_scroll_on` | Native scroll-lerp toggle (0 = off, 1 = on) |
| +0x120 | f64 | `lerp_factor` | Lerp coefficient for animated scroll (observed 0.75) |
| +0x12C | i32 | `wrap_mode` | 0 = clamp focus at ends, non-zero = modulo-wrap |
| +0x130 | u8 | `focus_scroll_on` | When non-zero, `FUN_18004b230` adjusts scroll offset on focus cross |
| +0x140 | qword | `orient` | 0 = horizontal layout, non-zero = vertical (options uses vertical) |
| +0x150 | f64 | `scroll_offset_x` | X scroll offset (used when `orient == 0`) |
| +0x158 | f64 | `scroll_offset_y` | **Y scroll offset — added to every row's `+0x90` each frame** |
| +0x168 | i32 | `focus_index` | Currently focused row index into the vector |
| +0x16C | i32 | `prev_focus_index` | Previous focus index (for `+0x30` flip logic) |
| +0x208/+0x210 | f64×2 | `step_delta` | Per-step viewport jump magnitude (0 / 180 for options) |
| +0x238 | i32 | `divisor` | Column count for grid layouts (0 = single column) |

### Row struct (OptionElement<KIND>) — scroll-relevant subset

| Offset | Type | Field | Notes |
|--------|------|-------|-------|
| +0x00 | ptr | primary_vtable | — |
| +0x30 | u8 | `focused` | 1 = cursor on this row; updated by `FUN_18004b230` |
| +0x60 | ptr | `parent_container` | Pointer back to the layout container |
| +0x88 | f64 | `pos_x` | **Layout OUTPUT** — written every frame by `FUN_18004a720` when `+0xB8 != 0` |
| +0x90 | f64 | `pos_y` | **Layout OUTPUT** — same |
| +0xA0..+0xB0 | f64×3 | `anchor_xyz` | Layout INPUT (anchor, not position) |
| +0xB8 | u8 | `active` | **Critical** — when 0, layout engine SKIPS this row entirely |
| +0x118 | ptr | `sub_clip` | AFP sub-MovieClip for rendering |

Full `OptionElement` layout is in [custom_player_options_research.md](custom_player_options_research.md).

---

## Key Functions

### `FUN_18004a720` — Grid layout engine

File addr `0x18004a720`. Invoked by `FUN_18004b4c0` (see below).

Walks `container+0x68..+0x70`. For each row `R` where `*(R+0xB8) != 0`, computes packed position from accumulator state and writes to `R+0x88/+0x90`. Rows with `+0xB8 == 0` are **skipped entirely** — their `+0x88/+0x90` retain stale values, and the position accumulator does NOT advance past them. Visible rows pack contiguously starting at `container+0xA0/+0xA8`.

### `FUN_18004b4c0` — Layout+scroll combined pass

File addr `0x18004b4c0`. Orchestrates:
1. Call `FUN_18004a720` (positions packed).
2. If `container+0x100 != 0`, lerp `+0x150/+0x158` toward the currently focused row's target offset (native smooth-scroll).
3. **Unconditionally** iterate every row and do `row+0x88 += container+0x150; row+0x90 += container+0x158`. Applies the scroll offset to all rows regardless of `+0xB8`.

This is the central insight: the engine already has full native-scroll plumbing that any consumer can drive just by writing `+0x158`.

### `FUN_18004b230` — Focus-update routine

File addr `0x18004b230`. Called by step-focus entrypoints (`FUN_1800495a0` up, `FUN_180049670` down, etc.) after they compute the new `+0x168`. Responsibilities:
- Read new `+0x168`, old `+0x16C`
- Clear old row's `+0x30` byte, set new row's `+0x30` byte
- When `+0x130 != 0` AND `wrap_mode != 0`, adjust `+0x150/+0x158` if focus crossed the viewport edge (this is the engine's built-in auto-scroll, gated by `+0x130`)

### `FUN_180049a40` / `FUN_180049b60` — First/last visible index helpers

`FUN_180049a40` walks the vector forward, returning the first index where `*(row+0xB8) != 0` AND `(row's secondary vtable slot 0 returned non-zero)`. `FUN_180049b60` does the same in reverse. Used by `FUN_18004b4c0` when computing the visible-range reference row for smooth-scroll.

These functions imply the cursor can only land on `+0xB8=1` rows — relevant for our mask: hiding a row via `+0xB8=0` also removes it from cursor navigation.

---

## Runtime Validation (MDX-003_20260324 session)

Captured by execution watchpoint on `FUN_18016f8f0` (`row+0x00` primary slot 7):

- Session #1: Basic tab, 6 native rows visible. Container `0x210F6170`, `+0x168 = 2` (cursor on Scroll Speed = row index 2). Row 2 @ `0x212A4BB0` confirmed Scroll Speed by scanning for the scalar value `580` → `OptionTab+0x10` of row 2.
- Session #2: Mods tab w/ 20 injected `bool_toggle` dummies + 1 autoplay row. Container `0x1925CC20`, 48 rows total in vector (cross-tab), 22 visible on Mods (dummies + autoplay + 1 header).

### Live tests performed

1. **Scroll-offset write**: wrote `container+0x158 = -100`, confirmed all row `+0x90` values shifted by -100 next frame, visually all rows moved up by 100px. Reset to 0, rows returned.
2. **`+0xB8` mask test**: wrote `+0xB8 = 0` on 14 of the 22 Mods-tab rows; confirmed layout engine packed the remaining 8 visible rows contiguously starting at the viewport top. Cursor navigation did NOT reactivate hidden rows.
3. **`+0xB8` re-activation trigger**: after a subsequent tab-switch-like event (exact trigger unclear; likely the native filter path or an OptionForm re-render), all `+0xB8=0` rows were reset to 1 except the one currently under an active hardware watchpoint. Implication: our scroll mask cannot be applied once and left — it must be re-applied after any code path that can write `+0xB8`.

### Visual observations

- No Y-clip on the options panel: rows with `+0x90 > ~210` render below the panel, rows with `+0x90 < 0` render above, both fully visible against the song-select scene underneath.
- When hidden via `+0xB8=0`, rows are removed from BOTH layout AND render (the sub-clip's visibility gets toggled through the chain — no visual residue).

---

## Implementation Strategy for Task 7

Two viable approaches were identified:

### Approach A — `+0xB8` mask only (chosen)

- Leave `container+0x158 = 0` (no scroll-offset writes).
- Maintain a per-(side, page) scroll-window-top index.
- Set `+0xB8=0` on rows outside the window; `+0xB8=1` on rows inside.
- Layout engine auto-packs the visible window at the top of the viewport.
- Cursor is naturally constrained to visible rows (confirmed via `FUN_180049a40/b60` skip logic).
- **Re-apply mask after any event that can rewrite `+0xB8`:**
  - Tab-switch → already runs through our `filter_hook` detour, add a post-phase mask pass
  - Focus-move beyond window edge → detour `FUN_18004b230` (or one of its step-focus callers) and re-apply mask post-original

### Approach B — scroll-offset write + render-level hide

- Leave all rows `+0xB8=1`; layout packs them into one vertical strip.
- Write `container+0x158 = -window_top * row_height` to shift the strip.
- Hide off-screen rows at the sub-clip level (`afp_mc_set_param(row+0x118 mc_id, 0x1007, 0)`) to prevent rendering outside the viewport.
- More moving parts; the sub-clip hide path is separate from the layout path and needs its own re-apply discipline.

**Approach A is chosen** because it uses a single control surface (`+0xB8`) that the engine's cursor navigation already respects, no separate render-level hide needed, and re-apply logic lives in one place (post-filter-detour + post-focus-update).

---

## Cross-Version Notes

All addresses above are from the `MDX-003_20260324` build. The function bodies are hash-anchored to distinctive prologues:

- `FUN_18004b4c0`: entrypoint of the layout+scroll pass; called from the OptionForm update path. Signature target.
- `FUN_18004b230`: focus-update; called from step-focus routines. Signature target.

Per project RE convention, the hook code resolves these via AOB signatures in `core/signatures.rs` rather than hardcoded offsets. Field offsets (`+0x158`, `+0x168`, `+0x16C`, `+0xB8`) are stable across the builds the modpack supports (`20250805`, `20260324`) — they come from the compiler's layout of the same C++ class, not build-specific data.

---

## Gotchas

- **Per-frame layout rewrites `+0x88/+0x90`.** A one-shot write to a row's position field is lost on the next frame. If you need per-frame position control, you must hook inside the layout pass or overwrite after it completes. For Task 7 we avoid this by letting the layout engine do the packing.
- **`+0xB8` gets reset by re-filter events.** Our `custom_options::filter_hook` Phase 3 writes `+0xB8` via `component_set_visible` based on page metadata. Any other code path that runs the filter (tab-switch, scene re-entry) will blow away our scroll mask. Re-apply from the detour tail.
- **Cursor navigation uses vector indices, not visible-subset indices.** `+0x168` is the absolute vector index, and the next/prev-focus helpers skip `+0xB8=0` rows silently. When we shift the scroll window, we don't have to touch `+0x168` — it keeps pointing at the currently-focused row regardless of which rows are visible around it.
- **The cursor CAN land on a currently-hidden row if we hide the row the cursor is already on.** The game will not auto-move focus. Before hiding a row, verify it's not the focus target; adjust the scroll window to KEEP focus visible rather than to a target offset.
