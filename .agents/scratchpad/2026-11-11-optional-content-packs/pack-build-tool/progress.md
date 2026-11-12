# Progress: pack-build-tool

Task: `.agents/tasks/2026-11-11-optional-content-packs/step03/task-01-pack-build-tool.code-task.md`

## Record
- `scripts/build_packs.py`, `scripts/test_build_packs.py` (26 tests incl. later steps'
  additions), wired into `scripts/validate_background_dancers.sh`.
- Real run over the assets checkout: 23 packs, 777 models, 1 036.4 MB of zips, 3 450.3
  MB installed, largest zip 184.1 MB; a second build gave the same 23 content ids.
  Table in `research/hosting-and-downloads.md` §5.
- Fixture committed under `updater/tests/fixtures/packs/`; `.gitattributes` keeps its
  line endings.

## Deviations
- Steps 3–12 were implemented in one continuous pass on 2026-11-11/12. This record and
  the task file were written afterwards; there is no separate `context.md` / `plan.md`
  for this task. The feature-level record is
  `.agents/planning/2026-11-11-optional-content-packs/progress.md`.

Status: Complete (uncommitted — maintainer commits manually)
