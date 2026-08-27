# Context — task-03 emitter-promotion (Step 8, overlay-menu rewrite)

## Task
.agents/tasks/2026-08-24-overlay-menu-rewrite/step08/task-03-emitter-promotion.code-task.md

## Approval chain (verified)
Same as tasks 01/02. Mode: auto. Depends on task-01 (blobs, Complete)
and task-02 (index export, Complete — cabinet-validated).

## Build & test commands
- Harnesses: validate_overlay_draw.sh (17), validate_mod_menu.sh (36)
- Gates: cargo check → cargo fmt → ./build.sh
- Cabinet: deploy DLL (+ blobs already deployed), launch via game_nav,
  keypad-drive the menu, log-verify; screenshots archived for the
  maintainer.

## Key facts (verified in-repo)
- `wrapper_render_hook(this)` fires only for **BmpString TEXT wrappers**
  (widget_renderer hooks the wrapper class render; ImageWidgets are raw
  sprite nodes on the game's own vtable — never call the hook).
- Render-list z = registration order; records append to the command
  list in node-render order (later = on top).
- **Layer-identity refinement (approved mechanism, adapted):** the menu
  panel sprite precedes every menu text wrapper in the render list, so
  emitting pre-original at ANY existing menu wrapper would place the
  quad ABOVE the panel. Fix: a dedicated ANCHOR text widget (single
  space, offscreen) created FIRST in `allocate_widgets` — before the
  panel — shown/hidden with the menu; its WRAPPER pointer is the
  emission identity. Pre-original emission at its render lands the quad
  above all earlier content (game + other mods' widgets) and beneath
  the panel + every menu widget. Documented fallback if a blank-text
  wrapper's render never fires (walker may skip): first-list-of-frame.
- `create_text_widget` returns the INNER widget (child_array[0]); the
  hook's `this` is the WRAPPER — a new create variant must return the
  wrapper address too.
- POC gate ladder to KEEP verbatim: null list / null arena / bump
  invariant / per-list re-emit gate / 8 MiB soft cap / shader null /
  progs 1..=64. POC emission shape (context→scissor→SetShader→quad→
  restore→scissor-off) extends with `set_vs_const_f(0, [c48,c49])`
  before SetShader(theme idx).
- `theme_program_indices()` (task-02) — None ⇒ no emission + greyed row.
- Modal rect: render.rs MODAL_X/Y/W/H = 60/60/1160/600.
- tabs.rs:87 hardcoded `false` = the availability-gate seam.
- theme.rs `Background::Static` + guard test to replace.

## Decisions (auto mode)
- BackgroundParams via atomics (ACTIVE bool, PROGRAM u32, RECT packed
  u64, 2 param f32-bits u32s) — written from menu paths, read on the
  render thread; no locks in the hot path.
- Time: `Instant` since emitter init, `(elapsed_ms % 3_600_000)/1000`
  as f32 (wrap-seamless shaders per task-01).
- Failure latch: 60 consecutive gate failures while ACTIVE ⇒ session
  latch + one WARN.
- Quad color 0xFF000000 (opaque black base; PS alpha 0.92 composes).
- theme_params [0.0, 0.0] for now (reserved knobs).
- Anchor INFO once on first emission (cabinet validation signal).
