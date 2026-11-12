# Progress: network-pack-downloads

Task: `.agents/tasks/2026-11-11-optional-content-packs/step08/task-01-network-pack-downloads.code-task.md`

## Record
- `install::obtain`, `packs/space.rs`, download rules in `step.rs`, the test-only
  loopback switch.
- `packs_e2e.rs` n01–n07 against an in-process listener.
- NOT DONE: a real download from GitHub — needs the packs published (Step 4). The
  Windows free-space call is compiled by the Win7 build but has not run on Windows.

## Deviations
- Steps 3–12 were implemented in one continuous pass on 2026-11-11/12. This record and
  the task file were written afterwards; there is no separate `context.md` / `plan.md`
  for this task. The feature-level record is
  `.agents/planning/2026-11-11-optional-content-packs/progress.md`.

Status: Complete (uncommitted — maintainer commits manually)
