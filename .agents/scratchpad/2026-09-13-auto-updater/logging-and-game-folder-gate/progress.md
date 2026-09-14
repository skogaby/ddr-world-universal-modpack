# Progress — Task 02: Add logging and the game-folder safety gate

Status: Complete (uncommitted — maintainer commits manually)

## Checklist
- [x] gamedir tests G1–G8 red → `gamedir.rs` green
- [x] log tests L1–L5 (+ global façade/macros) red → `log.rs` green
- [x] e2e E7–E9 red → `main.rs` wiring green (E4/E5 updated to pass a valid `--game-dir`, since a Run now goes through the gate)
- [x] fmt, host tests (36 unit + 9 e2e), host build 0 rustc warnings, Win7 cross-build 0 rustc warnings (exe 216 576 B, no `ProcessPrng`), CrossOver smoke: refusal from a scratch dir creates nothing; fake game dir → header + transcript + log
- [x] review; planning progress.md updated

## TDD cycles
1. Red: `tests/cli_e2e.rs` E7–E9 (3 FAILED against the Task 01 binary); `gamedir.rs`/`log.rs` authored with their unit tests (module absent = compile-red).
2. Green (first pass): wiring in `main.rs` → 4 log tests + E4/E5 failed:
   - `format_line` padding: `Display for Level` used `f.write_str`, which ignores `{:<5}` → switched to `f.pad`.
   - E4/E5 ran the bare binary from `target/debug`, which is (correctly) refused now → tests point `--game-dir` at a temp game folder.
3. Green: 36 + 9.
4. Review fixes: (a) std `canonicalize()` on Windows yields `\\?\Z:\…` verbatim paths (seen in the CrossOver smoke transcript) → `dunce::canonicalize` (strips the prefix only when equivalent); (b) targeted `#[allow(dead_code)]` with "consumed from Step 2" notes on `GameDir`'s work-dir accessors, `MANIFEST_NAME`, `Level::Error` — the root crate allows dead code crate-wide; the updater keeps it targeted so real dead code still warns.

## Deviations
- **`dunce` dependency added** (not in the task's list). Reason: verbatim `\\?\`
  paths in every message/log line and a known source of Win32 path trouble
  (later steps join relative paths onto `root`). Zero-dependency crate, identity
  on non-Windows hosts. Recorded in the manifest comment.
- Header vs transcript overlap: the log header (version / unix time / game
  folder / argv — per the task) and the §4.13 transcript lines (`DDR World Hook
  updater <v>` / `Game folder:`) both name the version and folder. Kept both as
  specified: the header is the field-log record, the transcript the operator
  view.

## Files
- `updater/src/gamedir.rs` (new), `updater/src/log.rs` (new),
  `updater/src/main.rs` (wiring), `updater/tests/cli_e2e.rs` (E4/E5 adjusted,
  E7–E9 added), `updater/Cargo.toml` + `Cargo.lock` (`dunce`)
