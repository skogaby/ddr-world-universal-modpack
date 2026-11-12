# Progress: publish-packs

Task: `.agents/tasks/2026-11-11-optional-content-packs/step04/task-01-publish-packs.code-task.md`

## Record
- `catalog` subcommand with tests; `scripts/publish_packs.sh`.
- `--dry-run` against the real (empty) shelf: "the release does not exist yet", 23 zips
  listed, nothing uploaded.
- NOT DONE: the real publish. It uploads about 1 GB to a public release, which is the
  maintainer's to run. Until it is, release builds need `PACKS_CATALOG` and cabinets
  cannot download packs.

## Deviations
- Steps 3–12 were implemented in one continuous pass on 2026-11-11/12. This record and
  the task file were written afterwards; there is no separate `context.md` / `plan.md`
  for this task. The feature-level record is
  `.agents/planning/2026-11-11-optional-content-packs/progress.md`.

Status: Code complete; the publish itself is pending (maintainer)
