# Context — overlay-draw-poc

Task: .agents/tasks/2026-08-24-overlay-menu-rewrite/step02/task-02-overlay-draw-poc.code-task.md
Mode: auto. Approval chain verified (plan + design `Status: Approved 2026-08-24`).

## Facts gathered

- `render_notes_hook::active_command_list()` (src/services/render_notes_hook.rs:230)
  is public, scene-agnostic, index range-checked 0..=8; `write_ptr(cl)` public unsafe.
- Default shader global: `signatures.get_address("default_shader")` (derived at
  signatures.rs:3434; same source pass_rewrite.rs:90 uses). Program count at
  `*(shader+4)`.
- `scene_manager::current_scene() -> i32` (mutex-locked; acceptable — the wrapper path
  already locks several mutexes per call; gated behind a relaxed atomic scene compare).
- Env pattern: latch `std::env::var(X).map(|v| !v.is_empty())` at init
  (movie_sync.rs:622 precedent).
- **wrapper_render fires once per VISIBLE WRAPPER per frame, not once per frame**
  (input_manager::poll runs there unconditionally). POC needs its own frame gate →
  arena per-frame reset: within a frame size only grows, so `same list && size >=
  last_emit_end` ⇒ already emitted this frame.
- Tag 0x07 (2D context) handler `FUN_180268c40` decompiled (20260616): payload
  `{f32 canvas_w @+4, canvas_h @+8, offset_x @+0xC, offset_y @+0x10}`; sets the
  virtual-canvas transform used by the 2D draw handlers (0x03 confirmed reading it).
  POC emits `set_context_2d(1280,720,0,0)` first for deterministic placement.
- Arena capacity: genuinely unknown; `ARENA_SOFT_CAP` (8 MiB) bounds our contribution;
  diagnostics log real sizes to replace the guess with data.

## Requirements

Per task file: always-on bounded per-scene diagnostics; POC behind
`DDR_OVERLAY_DRAW_POC`; full gate ladder (list, bump invariant, soft cap, shader,
program count) with latched WARNs; context+scissor+shader+quad+restore+scissor-off
block; copy + single bump like mine_render; heartbeat INFO per 600 emissions.

## Validation

- Host: encoder tests extended (set_context_2d) — 12 pass via validate_overlay_draw.sh.
- Autonomous boot 1 (no env var): diag lines across the attract band; no behavior
  change; no new WARNs.
- Autonomous boot 2 (env var set): no crash over attract cycles (incl. the demo
  gameplay loop); heartbeat lines advancing; gate WARNs absent (or recorded).
- Maintainer: visual quad confirmation + z-probe in attract / song select / gameplay.
