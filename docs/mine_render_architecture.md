# Mine Render Pass — Architecture Reference

This document describes the rendering architecture used by `NoteTypesExpansion` for mines, and how to extend it for additional note types (lifts, rolls).

## Why a Dedicated Pass

The engine's per-pass note collector filters on `note.kind == ARROW` and silently drops everything else. Our mine notes carry `kind = 20 (MINE)` — outside the vanilla range — so they never reach the collector.

Two earlier approaches were considered and rejected:

1. **Mid-function patch of the kind filter** — would let mines flow through the vanilla pipeline, but they'd inherit the colored arrow palette and single-atlas binding. No way to give them distinct visual identity (silver glyph + lightning overlay).
2. **Kind-swap around the collector** — the Task 5 stopgap. Temporarily rewrite `kind = MINE → kind = ARROW` around the collector call so they're emitted as colored arrows. Same visual identity problem, plus the double-pass coordination was fragile.

The shipped approach: hook the outer draw function and append a dedicated mine pass after the vanilla shock + normal passes complete. The mine pass calls the engine's own helpers for the hard math, so we reuse ~95% of the vanilla rendering infrastructure.

## Hook Point

We detour the arrow renderer's per-frame note draw member (AOB: `render_notes`). Its body runs the vanilla shock pass, then the vanilla normal pass, then returns. Our detour:

1. Calls the original (vanilla draws complete)
2. Walks the Results vector for mine-kind entries
3. Emits per-mine quads via CommandList commands

## Per-Mine Pass (Layers)

Each mine is drawn as **two stacked quads**:

### Layer 1 — Silver glyph
- Shader: the engine's default shader (palette-animated silver shimmer)
- Atlas: the arrow atlas's shock-slot columns (same tiles the engine's shock-arrow pass uses)
- Rotation: each panel (left/down/up/right) gets a distinct orientation via the engine's `set_direction`
- Position: computed by the engine's `get_offset_y` (inherits speed/boost/brake/wave/HIDDEN/SUDDEN from the arrow renderer)

### Layer 2 — Lightning overlay
- Blend mode: additive (SRC_ONE)
- Texture: the mine PNG (s/m/l variant matching the arrow-shape option), loaded via the engine's file pipeline
- UV animation: shock-cadence (`frame = (musicCount / 33) % 8`), 2×4 grid on the 192×384 mine texture
- Rotation: always upright (explicit `twist = 0.0`), regardless of panel
- State restoration: blend mode reset to normal + render state flushed + arrow shader/atlas rebound so downstream renderers (spot renderer, etc.) see the same CommandList state vanilla would have left

## Engine Functions We Reuse

| Function | Purpose |
|----------|---------|
| `get_offset_y` | Scroll-Y math (all boost modes, beat interpolation) |
| `set_direction` | Panel index → quarter-turn rotation |
| `render_sprite_final` | Per-quad vertex fill (rotation, appearance alpha, reverse) |
| `set_render_state` | CommandList blend-mode flush |

All four are called via signature-resolved function pointers. We read the arrow renderer's state fields (speed, boost, beat_count, music_count, blend_mode, arrow_shader, arrow_texture, results_ref, offset_y) by direct memory access using offsets verified from Ghidra disassembly.

## CommandList Emission

The engine's CommandList API (`SetShader`, `SetTexture`, `DrawRotateSprites`) is fully inlined — commands are written as raw byte sequences into the CommandList buffer. We match the engine's exact layout:

| Command | Tag | Size | Payload |
|---------|-----|------|---------|
| SetTexture | 0x11 | 0x1C | slot (u32), texture handle (u32), parameter (4× f32) |
| SetShader | 0x13 | 0x18 | shader pointer (u64), param (u32) |
| DrawRotateSprites | 0x04 | 0x10 + count×0x34 | count (u32), sprite array pointer (u64) |

Each emission advances the CommandList's `size` (+0x0C) and write pointer (+0x10) fields. The sprite array is allocated from the same arena, directly after the command header.

## Adding a New Note Type

To add lifts or rolls:

1. Implement the `NoteType` trait in a new file under `src/mods/note_types_expansion/`.
2. Register it in `NoteTypesExpansionMod::enable`.
3. Parse your SSQ chunk in `on_chart_loaded`; inject notes with your distinct `kind` value.
4. Implement `render_binding()` to return your texture name and UV rectangle.
5. Extend `mine_render.rs`'s per-quad loop to dispatch on kind:
   - For a new single-frame note type (e.g. lifts): emit the same two-layer pattern with different texture/shader bindings.
   - For a multi-quad note type (e.g. rolls needing head + body + tail): call `render_sprite_final` three times per note, matching how the engine's `renderNote` lambda emits freeze-arrow bodies.

Key invariants to preserve:
- Always restore CommandList state (shader + texture + blend mode) at pass end.
- Always pass `twist = 0.0` explicitly for any element that should stay upright regardless of panel (e.g. lightning overlays).
- Read arrow renderer state via the verified offsets in `mine_render.rs::actor::*`.
- Don't reimplement engine math — call `get_offset_y` and `render_sprite_final` for Y position and quad fill.

## Speed Is Stored As Integer

Worth noting: the arrow renderer stores speed as `speed × 100` as an `i32` at +0xA0 (not as a float, despite the field's apparent purpose). The engine's `get_offset_y` expects this integer directly — verify in Ghidra: `MOVSXD RAX, EDX; IMUL RAX, RCX; ... CVTSI2SS XMM0, RAX; DIVSS XMM0, [100.0]`. If you compute Y yourself instead of calling `get_offset_y`, divide by 100 to get the float multiplier.

## Arrow Shape Resolution

The Layer 2 lightning overlay comes in three size variants (s/m/l) keyed by the player's arrow-shape option. The render hook receives the `ArrowRenderer` pointer, not the `GamePlayActor`, so the pointer chain that starts with `actor[+0x84] = playSide` isn't directly accessible from the render path. Instead the mod's pre-judge callback (which *does* receive `GamePlayActor`) primes an `AtomicU32` cache once per chart:

```
GamePlayActor[+0x84]    = playSide (i32, 0 or 1)
player_work_table[playSide * 8]
                        = wrapper*
*wrapper                = PlayerWork*
PlayerWork[+0xE0]       = Option (inlined struct)
Option[+0x60]           = arrow_shape (i32 in 0..=7)
```

The engine's asset-load path for `shock_effect%02d_%c` uses the same chain — the chain terminates at an Option vtable method that literally reads `MOV EAX, [RCX+0x60]; RET`. The per-side player-work table is a RIP-relative global resolved via the `player_work_table_anchor` signature (a short two-slot accessor function whose first two instructions load the 1P and 2P entries back-to-back).

Caching strategy:
- First pre-judge callback of the chart walks the chain and stores the result in `CACHED_ARROW_SHAPE`; the sentinel `u32::MAX` marks "unresolved".
- Subsequent pre-judge callbacks short-circuit on a single atomic load.
- `emit_mine_pass` reads the cache; if the pre-judge hasn't run yet (theoretical — pre-judge precedes render within a frame), it falls back to shape `0` (LARGE).
- The mod's scene-exit callback resets the cache to the sentinel, so the next chart re-resolves freshly.

The option is locked the moment a chart starts — the engine exposes no in-song option editor — so cache-once-per-chart is sufficient; no per-frame re-resolution is needed.
