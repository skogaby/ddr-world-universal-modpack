# Context — widget-pool-diagnostic

Task: .agents/tasks/2026-08-24-overlay-menu-rewrite/step01/task-02-widget-pool-diagnostic.code-task.md
Mode: auto. Approval chain identical to task-01 (verified there; same plan/design,
both `Status: Approved 2026-08-24`).

## Facts

- Pool structure (docs/widget_registration_system.md "Render List Manager" +
  `register_in_render_list` in src/services/widget_renderer.rs): manager at
  `*(*(scene_manager_global)+0xB0)`; `+0x18` free head, `+0x20` sentinel; free node
  `+0x08` = next. Pool empty when head == sentinel.
- `wrapper_render_hook` fires every render frame for the game's own wrappers (it hosts
  `input_manager::poll`) — the manager is guaranteed live there, and
  `resolve_wrapper_derived` (which fills `scene_manager_global`) runs during `init()`
  before the hook installs, so at first fire the global is resolved or permanently null.
- extern "C" frame: panic-free discipline; poison-recover lock pattern already used in
  the same function.

## Requirements

One-shot walk on first wrapper render; iteration cap 4096; every read null-guarded;
one INFO line (count or unavailable+reason); no change to registration behavior.
