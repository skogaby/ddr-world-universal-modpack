# Context — task-02 theme-integration (Step 7, overlay-menu rewrite)

## Task
.agents/tasks/2026-08-24-overlay-menu-rewrite/step07/task-02-theme-integration.code-task.md

## Approval chain (verified — same as task-01)
Task Generated-By code-task-generator 2026-08-25; source plan Status:
Approved 2026-08-24; design Status: Approved 2026-08-24. Mode: auto.
Depends on task-01 (Complete, uncommitted).

## Build & test commands
- `./scripts/validate_mod_menu.sh` (host tests; 36 passing after task-01)
- Gates: `cargo check --target x86_64-pc-windows-msvc` → `cargo fmt` →
  `./build.sh`
- Runtime: deploy DLL to the CrossOver bottle, launch via
  `scripts/game_nav/launch.sh`, drive via `scripts/game_nav/lib.sh`,
  harvest log.txt. Visual verification is the maintainer's.

## Requirements
Per the task file: config fields; chrome_loader appearance state +
full-section kick read + generation-tokened resynthesize; render palette
lookups + creation-only re-coloring + fallback tint; tabs.rs real arm;
input.rs Theme arm + whole-section persistence.

## Facts verified in-repo (beyond the task file)
- `compute_new_value` enum arm returns `values[new_index]`; theme row
  values are `0..n` so value == table index. Boolean returns 0/1.
- `activate_selected` runs on the input callback / repeat thread with NO
  state lock held at the Theme arm (selected_row() releases before the
  match) — safe to call `tabs::rebuild_and_refresh()` (locks internally)
  and chrome_loader fns (atomics + fs + thread spawn).
- `rebuild_and_refresh()` (tabs.rs:161) = rebuild under lock + marshaled
  refresh_all; the post-edit repaint path.
- Rust allows `const &'static str` paths as match patterns — the
  `model::THEME_ROW_KEY` consts work directly in the key match.
- render.rs color sites: creation (allocate_widgets) + refresh
  (refresh_all); six creation-only widgets listed in the task. Locals
  typed as the tuple type today become `[f32;3]`.
- chrome_loader failure paths that need the generation guard: synthesis
  failure (ensure_piece_file false), load refusal, fault injection —
  each may race a newer resynthesize.

## Assumptions / decisions (auto mode)
- Appearance state (theme index, animate, opacity) lives in
  chrome_loader (it already owns opacity; one-stop store, pub(super)
  accessors + setters). persist_overlay_menu() also lives there.
- The strip is synthesized only at kick (theme/opacity-invariant);
  resynthesize regenerates the panel only.
- Generation guard: `PendingFile`/`Loading` gain `generation: u32`;
  Panel publishes/marks-failed only when generation is current; Strip
  ignores generations (kick-only).
- kick() unknown-theme WARN is a plain log_warn (kick runs once).
- Engine-facing behavior is validated on the cabinet (no host-test
  harness covers chrome_loader/render/input/tabs — repo standard).
