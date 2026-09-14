# Plan — Task 03: Build and ship the updater from the release archive script

Status: Approved 2026-09-13 (auto mode — approved plan/design chain)

## Verification scenarios (shell; no unit-test framework for the script)
| # | Scenario | Expected |
|---|----------|----------|
| S1 | `bash -n` | ok |
| S2 | Guard on the Win7 updater exe and the Win7 DLL | passes silently |
| S3 | Guard on a default-msvc-target updater build (contains `ProcessPrng`) | fails with a message naming the repo-relative binary path, exit ≠ 0 |
| S4 | Guard with `strings` unavailable (simulate via `PATH=` to an empty dir inside a subshell) | fails closed with a message |
| S5 | Full script run | exit 0; zip root has `ddr_world_hook.dll`, `ddr_world_hook_updater.exe`, `mod-config.json`, `judgement_offsets.csv`, `README.md`, `data_mods/`; entry count = previous (407) + 1 |
| S6 | Comments | no `_cache`/`*_ifs`/`*.arc` exclusion claim; header mentions the updater |

## Implementation shape
- Extract the guard into a function `assert_win7_safe()` defined near the top;
  it takes a path, checks `command -v strings`, runs
  `if strings "$1" | grep -q ProcessPrng; then … exit 1; fi` (grep's non-zero
  "no match" is inside the `if`, so `pipefail` does not trip the script).
- Updater build block mirrors the DLL block; run in `(cd updater && …)`.
- `cp "$UPDATER" "$STAGE/"`.
- Guards called for both binaries right before `zip`.
- Comments rewritten; final listing greps for both binaries.
- For S3 the guard is exercised by sourcing the function: `bash -c 'source <(sed -n "/^assert_win7_safe()/,/^}/p" scripts/build_release_archive.sh); assert_win7_safe <path>'`.
