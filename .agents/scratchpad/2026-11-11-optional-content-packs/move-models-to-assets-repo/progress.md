# Progress: move-models-to-assets-repo

## Checklist
- [x] Copy to the assets checkout (rsync, `.DS_Store` excluded)
- [x] Blob-id verification
- [x] Originals removed from the code checkout's working tree
- [x] `.gitignore` rules, probed
- [x] Assets checkout: `.gitattributes`, `README.md`
- [x] `examples/assets_path.py`; eleven port examples use it; docstring defaults updated
- [x] Add-on README note
- [x] Validation

## Record
- Verification command, run from the code checkout before the removal (A = the assets
  checkout, T = a scratch folder):
  `git ls-files -s -- data_mods/custom_models` reduced to `blob-id<TAB>path` with the
  `data_mods/custom_models/` prefix stripped and `Custom` rows dropped, sorted; against
  `find dancers stages -type f | sort | git hash-object --no-filters --stdin-paths`
  pasted with its paths, sorted, in A. `cmp` reported no difference: 31 536 entries each.
- After removal: `data_mods/custom_models` is 107 MB; git shows 31 536 unstaged
  deletions and nothing else under that path.
- Probe folder under `dancers/` is ignored; a `Custom` file is not.
- `assets_root()` returns `../ddr-world-universal-modpack-assets` by default and the
  expanded override when `DDR_WORLD_ASSETS` is set. All examples byte-compile. They were
  not run: that needs Blender and the source game data.
- `scripts/validate_background_dancers.sh`: OK.
- `scripts/build_release_archive.sh`: OK. Zip is 119.4 MB, 790 files; `custom_models`
  holds 18 dancer and 5 stage entries, all under `Custom`.

## Findings for later steps
- The base zip is 119 MB, not the few tens of megabytes the plan's demo expected. The
  release script's `zip` STORES `.arc` files uncompressed (Info-ZIP skips that suffix
  unless run at `-9`), and the `Custom` models ship pre-packed as `.arc`. This predates
  the task. Compressing them belongs to the release-build step.

## Deviations
- The task file and these notes were written after the move had been carried out and
  verified, not before. The verification was designed and run before any original was
  removed.

Status: Complete (uncommitted — maintainer commits manually)
