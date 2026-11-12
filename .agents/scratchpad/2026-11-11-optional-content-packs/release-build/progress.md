# Progress: release-build

Task: `.agents/tasks/2026-11-11-optional-content-packs/step12/task-01-release-build.code-task.md`

## Record
- `build_packs.py base-image` with tests; `scripts/build_release_archive.sh`;
  `scripts/build_release_distribution.bat`.
- Built with `PACKS_CATALOG`: base zip 30.5 MB (131.8 MB uncompressed), was 119.4 MB —
  `zip -9` now compresses the `.arc` files. A planted stray source was left out.
  Without a catalog the build failed as intended.
- The zip was installed into a scratch folder with the host-built updater and packs
  chosen through the chooser.
- NOT DONE: `build_release_distribution.bat` was edited but cannot be run here (Windows
  only).

## Deviations
- Steps 3–12 were implemented in one continuous pass on 2026-11-11/12. This record and
  the task file were written afterwards; there is no separate `context.md` / `plan.md`
  for this task. The feature-level record is
  `.agents/planning/2026-11-11-optional-content-packs/progress.md`.

Status: Complete (uncommitted — maintainer commits manually)
