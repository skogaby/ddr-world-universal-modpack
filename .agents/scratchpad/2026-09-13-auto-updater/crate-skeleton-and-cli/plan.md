# Plan — Task 01: Create the updater crate skeleton and command-line shell

Status: Approved 2026-09-13 (auto mode — approval carried by the approved
plan/design chain recorded in context.md)

## Test scenarios (written first; must fail before implementation)

### `cli::parse` (unit tests in `updater/src/cli.rs`)
| # | Input argv | Expected |
|---|-----------|----------|
| T1 | `[]` | `Run(Cli::default())`: no flags, `repo == "skogaby/ddr-world-universal-modpack"`, `tag == None` |
| T2 | `--check` / `--force` / `--include-prerelease` each alone | `Run` with exactly that bool set |
| T3 | `--game-dir X` and `--game-dir=X` | `Run { game_dir: Some("X") }` |
| T4 | `--from-zip p.zip` and `--from-zip=p.zip` | `Run { from_zip: Some("p.zip") }` |
| T5 | `--from-zip p.zip --tag v9` | `Run { from_zip: Some, tag: Some("v9") }` |
| T6 | `--repo a/b` | `Run { repo: "a/b" }` |
| T7 | all flags combined | every field set |
| T8 | `--tag v9` alone | `Usage(msg)` mentioning `--tag` and `--from-zip` |
| T9 | `--game-dir` (no value), `--from-zip` (no value), `--tag` (no value), `--repo` (no value) | `Usage(msg)` naming the flag |
| T10 | `--bogus` | `Usage(msg)` containing `--bogus` |
| T11 | `stray` positional | `Usage(msg)` containing `stray` |
| T12 | `--repo ab` / `--repo /b` / `--repo a/` / `--repo a/b/c` | `Usage` |
| T13 | `--help` with other flags (even invalid ones) | `Help` |
| T14 | `--version` with other flags (no `--help`) | `Version` |
| T15 | `--game-dir A --game-dir B` | last wins (`Some("B")`) |
| T16 | `--game-dir=` (empty value) | `Usage` |

### `main` helpers (unit tests in `updater/src/main.rs`)
| # | Scenario | Expected |
|---|----------|----------|
| M1 | `exit_code` over every `Outcome` variant | `Ok/NothingToDo/Skipped/Usage/Help/Version/Crashed → 0`, `RollbackFailed → 1`, `UpdateAvailable → 3` |
| M2 | `run_guarded(|| panic!("boom"))` | returns `Outcome::Crashed`, captured message contains `boom` |
| M3 | `run_guarded(|| Outcome::NothingToDo)` | passes the outcome through |
| M4 | `placeholder_line(&cli)` | bare → `nothing to do yet`; `--check` → starts with `--check: not implemented yet`; `--from-zip` → `--from-zip: …`; precedence check ≥ from-zip ≥ force ≥ include-prerelease |
| M5 | `usage()` text | contains every flag name once |

### End-to-end (host binary, `updater/tests/cli_e2e.rs` using `env!("CARGO_BIN_EXE_ddr_world_hook_updater")`)
| # | Args | Expected |
|---|------|----------|
| E1 | `--version` | stdout `ddr_world_hook_updater 0.1.0\n`, exit 0 |
| E2 | `--help` | stdout contains `Usage:` and `--game-dir`, exit 0 |
| E3 | `--bogus` | stdout contains the message and usage, exit 0 |
| E4 | none | stdout `nothing to do yet`, exit 0 |
| E5 | `--check` | stdout contains `--check: not implemented yet`, exit 0 |
| E6 | `--tag v9` | usage error, exit 0 |

## Implementation shape
- `updater/Cargo.toml`: as specified. `[[bin]] name = "ddr_world_hook_updater"`,
  `path = "src/main.rs"` (explicit so the exe name is unambiguous).
- `updater/src/cli.rs`:
  - `pub struct Cli { game_dir: Option<PathBuf>, check, force, include_prerelease: bool, from_zip: Option<PathBuf>, tag: Option<String>, repo: String }` + `Default`.
  - `pub enum Parsed { Run(Cli), Help, Version, Usage(String) }`.
  - `pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Parsed` — first pass: any `--help`/`-h` → Help; any `--version`/`-V` → Version. Second pass: iterate with a small `take_value(flag, inline, iter) -> Result<String, String>` helper handling `--flag=value` / `--flag value`.
  - `pub fn usage() -> String`, `pub const REPO_DEFAULT`.
  - Post-validation: `tag` without `from_zip` → Usage; `repo` shape check.
- `updater/src/main.rs`:
  - `mod cli;`
  - `enum Outcome { Ok, NothingToDo, UpdateAvailable, Skipped, RollbackFailed, Usage, Help, Version, Crashed }`, `fn exit_code(Outcome) -> i32`.
  - `fn run_guarded<F: FnOnce() -> Outcome + UnwindSafe>(f) -> (Outcome, Option<String>)` — `catch_unwind`, extracts `&str`/`String` payloads.
  - `fn run(parsed) -> Outcome` prints per mode; `fn placeholder_line(&Cli) -> String`.
  - `main`: install silent panic hook, `run_guarded(|| run(cli::parse(env::args().skip(1))))`, on Crashed print `updater crashed internally: <msg>` to stderr, `process::exit(exit_code(outcome))`.
- `.gitignore`: add `/updater/target`.

## Rationale
- Two-pass parsing keeps help/version precedence trivial and testable.
- E2E via `CARGO_BIN_EXE_*` proves the stdout/exit contract on the actual binary
  without a process-spawning helper crate.
- Placeholder wording lives in one function so later steps replace it in one
  place.

## Risks
- `trim-paths` in a nested crate: works on the same nightly as the root (already
  used there). If cargo-xwin's build-std trips on it, drop to a comment and note
  the deviation.
- `-Z build-std=std,panic_abort` with the default unwind profile — probe proved
  std still pulls `panic_unwind`; `catch_unwind` test M2 runs on the host anyway.
