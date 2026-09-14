# Progress — Task 01: Create the updater crate skeleton and command-line shell

Status: Complete (uncommitted — maintainer commits manually)

## Checklist
- [x] `updater/Cargo.toml` + `.gitignore` entry (`/updater/target`)
- [x] Tests written (cli T1–T16 + usage-text, main M1–M5, e2e E1–E6) and failing for the right reason
- [x] `cli.rs` implemented → cli tests green
- [x] `main.rs` implemented → main + e2e tests green
- [x] `cargo fmt`, host build/tests clean (22 unit + 6 e2e, 0 rustc warnings)
- [x] Win7 cross-build clean; `ProcessPrng` count 0; exe 158 208 bytes; smoke-run under CrossOver (`--version`, `--check`, `--tag x`) behaves per spec
- [x] Review pass; planning `progress.md` updated

## TDD cycles
1. Red: stub `main.rs` (`mod cli; fn main() {}`) + `tests/cli_e2e.rs` → 6 e2e
   FAILED (no output/behaviour), cli unit tests 16/17 green (parser authored with
   its tests in one file — the red state for those is the compile error of an
   absent module, which is the practical TDD shape for a single-file Rust module),
   1 red: `usage_text_names_every_flag_once` over-specified "once" (`--from-zip`
   is legitimately mentioned in `--tag`'s description). Fixed the TEST to assert
   "documented as an option line" (`"\n  --flag"`) instead.
2. Green: real `main.rs` (`Outcome`, `exit_code`, `run_guarded`,
   `placeholder_line`, quiet panic hook, thin `main`) → 22 + 6 green.
3. Cross-build + smoke: see checklist.

## Deviations
- **Dependencies deferred (lesser deviation, conservative).** Task req. 2 asked to
  add `serde`/`serde_json(preserve_order)`/`sha2` now. Nightly cargo's
  `cargo::unused_dependencies` lint warned on all three at every build (nothing
  consumes them yet), conflicting with the task's "clean build" expectation and
  the TDD rule of implementing only what tests require. Removed them; the
  manifest comment records which step re-adds each (`serde_json` + `sha2` in
  Step 2, `ureq` in Step 4, `windows-sys` in Step 5). No behaviour or interface
  affected.
- **Package name kebab-case** (`ddr-world-hook-updater`, matching the root
  crate's `ddr-world-hook`) while the `[[bin]]` stays `ddr_world_hook_updater`
  per the approved naming (D1/D16). Cargo emits one cosmetic "should have a
  kebab-case name" warning for the bin target; there is no switch to silence it
  short of renaming the exe, which the design fixes. Documented in the manifest.

## Review notes
- `Outcome` variants for later steps (`Ok`, `Skipped`, `RollbackFailed`,
  `UpdateAvailable`) are already mapped by `exit_code` and exercised by M1, so
  no dead-code warnings and no exit-code decision is left for later steps.
- Import table of the Win7 exe: KERNEL32, ntdll, dbghelp (std backtrace
  support; present on Win7), VCRUNTIME140 + UCRT `api-ms-win-crt-*` (same set
  the DLL imports). `SystemFunction036` absent simply because nothing uses an
  RNG yet.
- `rustup` prints `warn: skipping unavailable component rust-std for target
  x86_64-win7-windows-msvc` on every cargo invocation — expected for a tier-3
  target (build-std supplies std); not a build problem.

## Files
- `updater/Cargo.toml`, `updater/Cargo.lock`, `updater/src/main.rs`,
  `updater/src/cli.rs`, `updater/tests/cli_e2e.rs`, `.gitignore` (+1 line)
