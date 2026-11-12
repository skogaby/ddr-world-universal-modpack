# Plan: move-models-to-assets-repo

Status: Approved 2026-11-11 (auto mode; stands on the approved plan and design).

## Checks (each fails before the work is done)
| # | Check | Expect |
|---|---|---|
| 1 | sorted `(blob id, path)` of `git ls-files -s` for the ported sources vs `git hash-object --no-filters` over the copy | identical, 31 536 entries |
| 2 | listing of `data_mods/custom_models/{dancers,stages}` | `Custom` only; git shows deletions only |
| 3 | `git check-ignore` on a probe source folder and on a `Custom` file | ignored / not ignored |
| 4 | `assets_path.assets_root()` with and without `DDR_WORLD_ASSETS` | override / sibling |
| 5 | `validate_background_dancers.sh`, `build_release_archive.sh` | pass; zip holds only `Custom` models |

## Order
Copy → check 1 → remove originals → ignore rules → checks 2–3 → lookup module and
script defaults → check 4 → check 5.
