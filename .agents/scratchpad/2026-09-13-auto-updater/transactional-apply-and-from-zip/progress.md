# Progress — Step 2 Task 03: transactional apply + --from-zip
Status: Complete (uncommitted — maintainer commits manually)
- [x] `apply.rs` (Journal write/read, Summary + install_line, ApplyError, `execute` with rename-based moves, backup mirror, reverse rollback, cleanup of download/stage on success AND on a clean rollback, journal kept only on RollbackFailed) + `fault.rs` (`DDR_UPDATER_FAULT` = `apply-after:N` | `rollback`) — 3 unit tests
- [x] `main.rs`: `run_from_zip` → shared `install(...)` pipeline (identity → manifest read → needs_update/--force → `--check` exit 3 → extract → hash tree → merge inputs (copy-if-absent) → plan → execute → §4.13 messages); Step-2 `#[allow(dead_code)]` markers removed (remaining allows: `Journal::read`, `RelPath::parent` for Step 5)
- [x] `tests/apply_e2e.rs` A1–A9 (8 tests; A4 folded into A3 via `--tag v2`)
- [x] fmt; 61 unit + 8 apply e2e + 9 cli e2e green; host + Win7 builds warning-free; exe 533 504 B, no `ProcessPrng`, `SystemFunction036` present (RandomState via IndexMap — the Win7 recipe matters from here on)
- [x] REAL INSTALL smoke (`$DDR_WORLD_INSTALL`, CrossOver): `--check` → exit 3 "update available (installed: nothing)"; install of the 20260913 release zip → 372 extracted, **369 written** (= 372 − 2 merged − 1 updater exe), DLL sha unchanged (zip's DLL byte-identical), `mod-config.json`/`judgement_offsets.csv` untouched, README.md added, data_mods file count unchanged (1213), 368 originals in `backup/`, manifest 369 files, stage/download/journal gone, ~4.5 s; rerun → "up to date"; `DDR_UPDATER_FAULT=apply-after:3 --force` on the real install → tree digest identical before/after, exit 0.

## TDD cycles
1. Red: A1–A9 against the Task-02 binary (no --from-zip) → all failed; unit tests compile-red.
2. Green after fixes: (a) `Fault::Rollback` fired after the first action even when it had no backup (first sorted file `README.md` is a Created) → now fires after the first action WITH a backup; (b) e2e `snapshot()` must skip fixture zips and the manifest (timestamp) to compare runs.
3. Real-install rollback left `stage/` behind → clean download/stage on a clean rollback too (+ A5 assertion).

## Deviations
- `--check` is honoured in `--from-zip` mode (exit 3 / 0) — cheap and useful for testing; design lists `--check` for the GitHub path only.
- Manifest write failure is NON-fatal (design §7) — `Summary.manifest_written` records it; the folder is left updated and the next run repeats the install.
