# Progress — Task 03: Build and ship the updater from the release archive script

Status: Complete (uncommitted — maintainer commits manually)

## Checklist
- [x] script edited: `WIN7_TARGET` variable, `assert_win7_safe()` (fails closed without `strings`), updater build block in `(cd updater && …)`, existence check, guard on both binaries before staging, `cp` of the exe, comments rewritten (header + data_mods block), final listing greps both binaries + total line
- [x] S1 `bash -n` ok
- [x] S2 guard silent on the Win7 DLL + Win7 updater exe
- [x] S3 guard fires (exit 1, names the binary) on the REAL default-target DLL `target/x86_64-pc-windows-msvc/release/ddr_world_hook.dll` (1 `ProcessPrng` hit) and on a synthetic file carrying the import name
- [x] S4 guard fails closed (exit 1) with `strings` unavailable
- [x] S5 full run exit 0 → `ddr-world-universal-modpack-20260913.zip`: root = `data_mods/`, `ddr_world_hook.dll` (10 484 224 B), `ddr_world_hook_updater.exe` (216 576 B), `judgement_offsets.csv`, `mod-config.json`, `README.md`; 408 entries = previous archive's 407 + exactly the exe (entry-list diff)
- [x] S6 comments: no exclusion claim for `_cache`/`*_ifs`/`*.arc` (they are now described as never-present / committed shipping content); header mentions the updater

## Findings worth keeping
- The default-target build of the CURRENT updater does NOT import `ProcessPrng`
  (nothing in it needs an RNG yet). It will as soon as `serde_json`'s
  `preserve_order` (IndexMap → `RandomState`) lands in Step 2 — which is exactly
  why the guard exists; verified against the DLL instead, whose default-target
  build does import it.
- The generated zip is untracked and left in the repo root for the maintainer
  (upload or delete); `*.zip` is not gitignored — do not `git add` it.

## Deviations
- None.
