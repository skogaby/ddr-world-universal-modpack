# Plan — Task 02: Add logging and the game-folder safety gate

Status: Approved 2026-09-13 (auto mode — approved plan/design chain, see context.md)

## Test scenarios

### `gamedir` (unit, temp dirs)
| # | Scenario | Expected |
|---|----------|----------|
| G1 | dir with `spice64.exe`, `resolve_with(Some(dir), fs probe)` | `root == canonical(dir)`, `work == root/.ddr_world_hook_updater` |
| G2 | dir with only `ddr_world_hook.dll` | accepted |
| G3 | empty dir | `Refusal` whose message contains the dir path, `spice64.exe`, `ddr_world_hook.dll` |
| G4 | override given while injected exe location is another (valid) dir | root == override |
| G5 | no override, injected exe location = valid dir | root == that dir (exe-relative path) |
| G6 | no override, exe location unknown (probe returns None) | `Refusal::UnknownExeLocation` |
| G7 | accessors | exact design names |
| G8 | nonexistent override path | Refusal (canonicalise fails) mentioning the path |

### `log` (unit)
| # | Scenario | Expected |
|---|----------|----------|
| L1 | `format_line(elapsed=12.345s, Warn, "x")` | `"[+12.35s] WARN  x"` (matches the pinned regex) |
| L2 | `attach_file` on a path with old content, log two lines | file truncated; contains exactly header-less two lines in order |
| L3 | `header(...)` | four header lines: version, unix time, game folder, argv |
| L4 | attach to a path that is a directory | attach returns Err; subsequent logging still returns normally (no panic) and the run continues |
| L5 | before attach | lines go to console sink only (test via an injectable console sink capturing strings) |

### e2e (`tests/cli_e2e.rs`, temp dirs)
| # | Args | Expected |
|---|------|----------|
| E7 | `--game-dir <empty temp>` | stdout has one line with the path + "spice64.exe"; exit 0; no `ddr_world_hook_updater.log` and no `.ddr_world_hook_updater/` created |
| E8 | `--game-dir <temp with spice64.exe>` (log pre-filled with "OLD") | exit 0; stdout contains `DDR World Hook updater 0.1.0`, `Game folder: `, `nothing to do yet`; log file exists, does not contain "OLD", starts with header, every non-header line matches the prefix regex |
| E9 | `--game-dir <valid> --check` | stdout + log contain `--check: not implemented yet` |

## Implementation shape
- `gamedir.rs`: `pub enum Refusal { NotAGameFolder(PathBuf), UnknownExeLocation, Unreadable(PathBuf, io::Error) }` with `Display`; `pub fn resolve(cli_override) -> Result<GameDir, Refusal>` = `resolve_with(cli_override, current_exe_dir(), &|p| p.is_file())`; `pub fn resolve_with(override, exe_dir: Option<PathBuf>, exists: &dyn Fn(&Path) -> bool)`.
- `log.rs`: `Level`, `struct Logger { start: Instant, file: Option<File>, file_failed: bool, console: Box<dyn Fn(Level, &str) + Send> }` in `static LOGGER: OnceLock<Mutex<Logger>>`; `pub fn attach_file(&Path) -> io::Result<()>`; `pub fn header(...)`; `pub fn emit(Level, &str)`; macros; `pub fn format_line(elapsed: Duration, Level, &str) -> String` (pure). Console sink defaults to stdout/stderr; `#[cfg(test)] set_console_sink` for L5.
- `main.rs`: `run(Parsed)` resolves via `gamedir::resolve(cli.game_dir.as_deref())` for `Parsed::Run` only; refusal → `println!` + `Outcome::Skipped`; else attach log (Err → WARN console) → header → `log_info!("DDR World Hook updater {}")`, `log_info!("Game folder: {}")`, `log_info!(placeholder)`.

## Notes
- `Instant`-based elapsed avoids any date crate; unix time in the header via `SystemTime`.
- Logger is process-global because macros are called from every module; tests that touch the global run serially (`#[serial]`-free: use a single test that exercises L2–L5 in sequence, or per-test unique files and don't assert on global ordering).
