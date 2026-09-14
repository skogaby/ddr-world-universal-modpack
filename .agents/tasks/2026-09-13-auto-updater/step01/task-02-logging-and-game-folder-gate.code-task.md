# Task: Add logging and the game-folder safety gate

## Description
Give the updater its two operational edges: a logger that writes every line to
the console and to `ddr_world_hook_updater.log` in the game folder, and a
game-folder resolver that refuses to do anything outside a folder that
recognisably holds the game. Wire both into `main` so a bare run now resolves
the folder, either refuses (exit 0) or logs a header and reports "nothing to do
yet".

## Background
The updater is meant to sit next to `spice64.exe` and be invoked from
`gamestart.bat`. Batch files in the field usually `cd /d %~dp0` first, but the
updater must not rely on the working directory: a double-clicked exe or a
mis-written bat still has to target the right folder, and an exe copied into
`Downloads` must not scatter files there. The design therefore resolves the
game folder from the executable's own location (`--game-dir` overrides) and
requires `spice64.exe` or `ddr_world_hook.dll` to be present. All the
updater's own state lives under `<game>/.ddr_world_hook_updater/`
(`download/`, `stage/`, `backup/`, `journal.json`) — this task only defines the
paths; later steps create and use them.

Field bug reports arrive as `log.txt` attachments today; the updater's own log
must be equally attachable, so it is overwritten per run (the last run is the
one that matters) and begins with a header that identifies the build and
invocation. Timestamps are elapsed seconds since start — deliberately avoiding
a date/time crate.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-13-auto-updater/design/detailed-design.md`
  (§2.6 R22, R26; §4.2 `gamedir`; §4.13 console/log output; §5.3/§5.5 work-dir layout and file ownership)

**Additional References (if relevant to this task):**
- `.agents/planning/2026-09-13-auto-updater/research/orientation.md` §5
  (launch-integration facts: why exe-relative resolution, unattended boots)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `updater/src/gamedir.rs`:
   - `pub struct GameDir { pub root: PathBuf, pub work: PathBuf }` plus
     accessor methods for `download_dir()`, `stage_dir()`, `backup_dir()`,
     `journal_path()`, `manifest_path()` (`root/ddr_world_hook_updater.manifest.json`)
     and `log_path()` (`root/ddr_world_hook_updater.log`), all derived from
     `root`; `work` = `root/.ddr_world_hook_updater`.
   - `pub fn resolve(cli_override: Option<&Path>) -> Result<GameDir, Refusal>`:
     candidate = the override if given, else the parent directory of
     `std::env::current_exe()`; canonicalise it; accept iff `spice64.exe` or
     `ddr_world_hook.dll` exists directly inside it. `Refusal` carries a
     human-readable one-line message that names the path examined and what was
     expected there (also a variant for "cannot determine the executable's
     location").
   - The probe of the two anchor files is injectable (e.g. `resolve_with(candidate,
     exists: &dyn Fn(&Path) -> bool)` used by `resolve`) so tests do not depend
     on the test binary's location.
2. `updater/src/log.rs`:
   - Levels `INFO`, `WARN`, `ERROR`; line format `[+12.34s] LEVEL  message`
     (elapsed seconds since process start, two decimals; level padded to 5).
   - Before a log file is attached, lines go to the console only (stdout for
     INFO, stderr for WARN/ERROR); `attach_file(path)` truncates/creates the
     file and from then on every line is also appended to it. Console and file
     lines are byte-identical, prefix included (operators paste console text
     into bug reports; one format keeps that unambiguous).
   - `header(version, argv, game_root)` writes the first lines: updater
     version, unix time (`SystemTime::now()` seconds), resolved game folder,
     and the argument list.
   - A single process-wide logger (e.g. `OnceLock<Mutex<Logger>>`) behind
     `log_info!`/`log_warn!`/`log_error!` macros; file write errors are
     swallowed after one console `WARN` (logging must never abort the run).
3. `updater/src/main.rs` wiring, in order: parse args → `gamedir::resolve`
   → on `Refusal` print the message (console only, no log file — the folder is
   untrusted) and exit 0 → attach the log file at `game.log_path()` → write the
   header → placeholder behaviour from Task 01 (now also logged).
4. Console transcript for the happy path matches §4.13's first two lines
   (`DDR World Hook updater <version>`, `Game folder: <path>`); the refusal
   prints one explanatory line, e.g. `This folder does not look like a DDR World
   game folder (no spice64.exe or ddr_world_hook.dll in <path>); nothing done.`
5. `cargo test` and `cargo fmt` clean inside `updater/`; host build and Win7
   cross-build still succeed.
6. Do not write machine-specific absolute paths into any tracked file (test
   fixtures use temp directories). Do not commit.

## Dependencies
- Task 01 (crate skeleton, `cli.rs`, `Outcome`/exit mapping, `catch_unwind` wrapper).

## Implementation Approach
1. Write `gamedir` tests first against temp directories (`std::env::temp_dir()`
   + unique subdir; clean up): accepted with `spice64.exe` only, with
   `ddr_world_hook.dll` only, refused when empty, override beats the exe
   location, refusal message contains the examined path, derived paths are
   exactly the design's names.
2. Implement `gamedir.rs`.
3. Write `log` tests: `attach_file` truncates an existing file; header line
   content; a WARN after attach lands in both the file and the captured
   console sink (make the console sink injectable or test via the file only);
   the elapsed prefix matches exactly `^\[\+\d+\.\d{2}s\] (INFO |WARN |ERROR) ` —
   pin that format in a test.
4. Implement `log.rs`, then the `main.rs` wiring; run the exe from a temp game
   folder (host build is fine for this) to eyeball the transcript and the log
   file.
5. Re-run the Win7 cross-build.

## Acceptance Criteria

1. **Exe-relative resolution accepts a game folder**
   - Given a directory containing `spice64.exe` (or only `ddr_world_hook.dll`) and the updater's location resolving to it
   - When `gamedir::resolve(None)` runs
   - Then it returns `GameDir` whose `root` is the canonical path of that directory and whose `work` is `root/.ddr_world_hook_updater`

2. **Override wins**
   - Given `--game-dir <dir>` pointing at a valid game folder while the exe lives elsewhere
   - When `resolve(Some(dir))` runs
   - Then `root` is the canonical override path

3. **Refusal outside a game folder**
   - Given a directory containing neither anchor file
   - When the exe runs with `--game-dir` pointing at it
   - Then it prints one line naming that path and the missing anchors, creates no files there (no log, no work dir), and exits 0

4. **Log file per run**
   - Given a valid game folder whose `ddr_world_hook_updater.log` already holds old content
   - When the exe runs
   - Then the log is truncated and starts with the header (version, unix time, game folder, argv) followed by the run's lines, each prefixed `[+S.SS] LEVEL`

5. **Console mirrors the log**
   - Given a valid game folder
   - When the exe runs
   - Then stdout shows `DDR World Hook updater <version>` and `Game folder: <path>` and every subsequent INFO line also present in the file; WARN/ERROR lines go to stderr

6. **Derived paths**
   - Given any `GameDir`
   - When its accessors are called
   - Then they return `root/.ddr_world_hook_updater/{download,stage,backup}`, `root/.ddr_world_hook_updater/journal.json`, `root/ddr_world_hook_updater.manifest.json`, `root/ddr_world_hook_updater.log`

7. **Logging never aborts the run**
   - Given the log file cannot be written (e.g. the path is a directory in a test)
   - When lines are logged
   - Then the run continues, one console WARN reports the log problem, and the exit code is unchanged

## Metadata
- **Complexity**: Low
- **Labels**: updater, rust, logging, filesystem, step-1
- **Required Skills**: Rust std filesystem/path handling, process-wide state with `OnceLock`/`Mutex`, macro-based logging façade, temp-dir unit testing
- **Generated By**: code-task-generator 2026-09-13
- **Source Plan**: `.agents/planning/2026-09-13-auto-updater/implementation/plan.md`
- **Plan Step**: Step 1: Crate skeleton, CLI shell, logging, game-folder gate, release-archive integration
