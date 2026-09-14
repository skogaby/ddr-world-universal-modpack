# Task: Create the updater crate skeleton and command-line shell

## Description
Create the standalone `updater/` Rust binary crate that will become
`ddr_world_hook_updater.exe`, with a hand-written command-line parser covering
the complete flag set from the design, a thin `main` that maps outcomes to exit
codes, and a top-level panic guard. No update logic yet: every flag that later
steps implement prints "not implemented yet" and exits 0. This is the
foundation every later step of the auto-updater extends.

## Background
The modpack ships a hook DLL loaded by spice2x. The auto-updater is a separate
console exe that runs from `gamestart.bat` before spice2x starts, downloads the
newest GitHub release, and installs it while preserving user configuration.
The design puts it in its own crate (`updater/`, own `Cargo.toml`/`Cargo.lock`,
NOT a Cargo workspace member — the root `Cargo.toml` is a plain package using
unstable `cargo-features`) so that the DLL build is untouched and the pure
merge logic can be host-tested with plain `cargo test` on macOS. The repo's
`rust-toolchain.toml` (nightly, `rust-src`, both msvc targets) applies to the
subdirectory automatically. The crate must cross-compile with the DLL's
Windows 7 recipe: `cargo xwin build --release --target x86_64-win7-windows-msvc
-Z build-std=std,panic_abort` (run from `updater/`). A probe with the same
dependency stack already built and ran, so the risk here is only wiring.

The exit-code contract is deliberate: a cabinet boots unattended into
`gamestart.bat`, so nothing the updater does may prevent the game from
starting except a failed rollback (exit 1, later step). Usage errors and
unknown flags therefore exit 0 after printing usage.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-13-auto-updater/design/detailed-design.md`
  (§2.1 R1–R3, §2.6 R19–R20, §3 module table, §4.1 command line, Appendix A)

**Additional References (if relevant to this task):**
- `.agents/planning/2026-09-13-auto-updater/research/technologies.md`
  (build probe result: dependency versions/features that link for the Win7 target)
- Root `Cargo.toml` (the `trim-paths` rationale comment and release profile to mirror)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `updater/Cargo.toml`: package and `[[bin]]` name `ddr_world_hook_updater`,
   `version = "0.1.0"`, `edition = "2021"`, `cargo-features = ["trim-paths"]`,
   `[profile.release]` with `opt-level = 2`, `lto = true`, `trim-paths = "all"`
   and a short comment mirroring the root manifest's rationale (no builder
   paths in the shipped binary).
2. Dependencies for this step only: `serde` (derive), `serde_json` with the
   `preserve_order` feature, `sha2`. Do not add `ureq`, `zip` or `windows-sys`
   yet — later steps add them when they are used.
3. Commit `updater/Cargo.lock` (binary crate). Add `/updater/target` to the
   root `.gitignore`.
4. `updater/src/cli.rs`: a hand-written parser (no CLI crate) producing a
   `Cli` struct for the full §4.1 set: `--game-dir <DIR>`, `--check`, `--force`,
   `--include-prerelease`, `--from-zip <PATH>`, `--tag <NAME>`,
   `--repo <OWNER/NAME>` (default `skogaby/ddr-world-universal-modpack`),
   `--help`, `--version`. Both `--flag value` and `--flag=value` forms are
   accepted for valued flags. Parsing returns an enum such as
   `Run(Cli) | Help | Version | Usage(String)`; `--tag` without `--from-zip`,
   a valued flag missing its value, an unknown flag, or a stray positional
   argument yield `Usage(message)`.
5. `updater/src/main.rs`: prints `ddr_world_hook_updater <version>` for
   `--version` (from `CARGO_PKG_VERSION`), the usage text for `--help` and for
   `Usage(_)` (message first), and for `Run(_)` prints a one-line placeholder
   ("<flag>: not implemented yet" naming the requested mode, or "nothing to do
   yet" for a bare run). All of these exit 0.
6. Exit codes are produced by exactly one function mapping an `Outcome` enum to
   an `i32` (`0` for everything defined so far; the enum leaves room for the
   design's `1` = rollback failed and `3` = `--check` found an update).
7. `main` wraps the run in `std::panic::catch_unwind`; a panic prints one line
   to stderr ("updater crashed internally: <payload>") and exits 0. Install a
   panic hook that suppresses the default backtrace noise (inferred: keep the
   console readable for operators).
8. `cargo test --manifest-path updater/Cargo.toml` and `cargo fmt` (run inside
   `updater/`) are clean; `cargo build --manifest-path updater/Cargo.toml`
   works natively on the host; the Win7 cross-build from `updater/` succeeds
   and the resulting exe contains no `ProcessPrng` import (`strings <exe> |
   grep -c ProcessPrng` prints 0).
9. Do not write machine-specific absolute paths into any tracked file. Do not
   commit (the maintainer commits manually).

## Dependencies
- None (first task of Step 1).

## Implementation Approach
1. Create `updater/Cargo.toml`, `updater/src/main.rs`, `updater/src/cli.rs`;
   run `cargo generate-lockfile` / first build to produce `Cargo.lock`.
2. Write the parser tests first (table-driven: each flag alone, combinations,
   both value syntaxes, every usage-error case, `--help`/`--version`
   precedence), then the parser.
3. Implement `Outcome` + `exit_code(Outcome) -> i32` and the `catch_unwind`
   wrapper in `main.rs`; keep `main` free of logic beyond parse → run → exit.
4. Verify host build/tests, `cargo fmt`, then the Win7 cross-build and the
   `ProcessPrng` check; record the exe size in the planning `progress.md`.

## Acceptance Criteria

1. **Crate builds on the host and cross-builds for Windows 7**
   - Given a fresh checkout with the repo's pinned nightly toolchain and cargo-xwin
   - When `cargo build --manifest-path updater/Cargo.toml` runs, and then
     `cargo xwin build --release --target x86_64-win7-windows-msvc -Z build-std=std,panic_abort` runs from `updater/`
   - Then both succeed, `updater/target/x86_64-win7-windows-msvc/release/ddr_world_hook_updater.exe` exists, and `strings` on it finds no `ProcessPrng`

2. **Every documented flag parses**
   - Given each of `--game-dir X`, `--game-dir=X`, `--check`, `--force`,
     `--include-prerelease`, `--from-zip p.zip`, `--from-zip p.zip --tag v9`,
     `--repo a/b`, alone and in combination
   - When `cli::parse` is called on the argument list
   - Then it returns `Run(cli)` with exactly those fields set and defaults
     (`repo` = `skogaby/ddr-world-universal-modpack`, `tag` = `None`) otherwise

3. **Usage errors never block the game**
   - Given `--tag v9` without `--from-zip`, `--game-dir` with no value, an
     unknown `--bogus` flag, or a stray positional argument
   - When the exe runs with those arguments
   - Then it prints the specific message followed by the usage text and exits 0

4. **Help and version**
   - Given `--help` or `--version` (with or without other flags)
   - When the exe runs
   - Then it prints the usage text, or `ddr_world_hook_updater 0.1.0`, respectively, and exits 0

5. **Placeholder run**
   - Given no arguments, or any valid combination of the not-yet-implemented flags
   - When the exe runs
   - Then it prints a single placeholder line naming the requested mode (or "nothing to do yet") and exits 0

6. **Panic containment**
   - Given a run whose body panics (exercise via a unit test of the wrapper with an injected panicking closure)
   - When the wrapper executes
   - Then it returns the outcome that maps to exit 0 and the panic message is reported on stderr without a backtrace

7. **Repository hygiene**
   - Given the finished change
   - When `git status` is inspected
   - Then `updater/Cargo.lock` is present, `updater/target/` is ignored, `cargo fmt --check` inside `updater/` passes, and no tracked file contains a machine-specific absolute path

## Metadata
- **Complexity**: Low
- **Labels**: updater, rust, cli, build, step-1
- **Required Skills**: Rust binary crates, Cargo manifests/profiles, cargo-xwin cross-compilation, hand-written argument parsing
- **Generated By**: code-task-generator 2026-09-13
- **Source Plan**: `.agents/planning/2026-09-13-auto-updater/implementation/plan.md`
- **Plan Step**: Step 1: Crate skeleton, CLI shell, logging, game-folder gate, release-archive integration
