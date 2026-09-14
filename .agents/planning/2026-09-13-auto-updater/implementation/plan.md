# Implementation Plan: DDR World Hook Auto-Updater

Status: Approved 2026-09-13

Design: `design/detailed-design.md` (Approved 2026-09-13). Register:
`idea-honing.md`. Research: `research/`. This plan decomposes the design into
steps that each leave a working, demonstrable updater; it does not restate
design detail — section references (§) point into the design.

Repo conventions that apply to every step: no `git commit` by agents (the
maintainer commits); keep `progress.md` in this planning directory current
after each step; `cargo fmt` the `updater/` crate; host tests run with plain
`cargo test --manifest-path updater/Cargo.toml` (the crate has no `retour`
dependency, so it compiles natively on macOS); the Win7 cross-build is
`cargo xwin build --release --target x86_64-win7-windows-msvc -Z build-std=std,panic_abort`
run from `updater/`. Never write machine-specific paths into tracked files.

## Checklist

- [x] Step 1: Crate skeleton, CLI shell, logging, game-folder gate, release-archive integration
- [x] Step 2: Local end-to-end install — archive extraction, manifest, plan, transactional apply, `--from-zip`
- [x] Step 3: Configuration merges — `mod-config.json`, `option_menu_settings`, `judgement_offsets.csv`
- [x] Step 4: GitHub release feed, download with digest verification, `--check`, `--include-prerelease`
- [x] Step 5: Hardening — journal crash recovery, self-update, cleanup, console-owner wait, changelog display
- [x] Step 6: Operator documentation, maintainer docs, and platform validation

## Steps

### Step 1: Crate skeleton, CLI shell, logging, game-folder gate, release-archive integration

**Objective.** Stand up `updater/` as a buildable, cross-compilable crate that
already behaves correctly at its edges (argument handling, refusal outside a
game folder, logging, exit codes) and is produced by the release script, so
every later step ships through the real pipeline.

**Implementation guidance.**
- `updater/Cargo.toml`: package/bin name `ddr_world_hook_updater`, edition
  2021, `cargo-features = ["trim-paths"]`, `[profile.release]` `opt-level = 2`,
  `lto = true`, `trim-paths = "all"` (mirror the root manifest's rationale
  comment). Dependencies for this step: `serde`, `serde_json` (`preserve_order`),
  `sha2`. Add `ureq`/`zip` when their steps arrive (keeps each step's build
  honest). Commit `updater/Cargo.lock`. Add `/updater/target` to the root
  `.gitignore`.
- `src/main.rs`: hand-written argument parser producing a `Cli` struct for the
  full flag set in §4.1 (flags not yet implemented return "not implemented
  yet" and exit 0 — they are wired in Steps 2–4); `--help`/`--version`;
  unknown option → usage, exit 0; top-level `catch_unwind` mapping a panic to a
  logged message and exit 0. Exit-code mapping lives in one function.
- `src/log.rs`: elapsed-time-prefixed `INFO/WARN/ERROR` to stdout and to
  `<game>/ddr_world_hook_updater.log` (truncate at start; header line with
  updater version, unix time, resolved game folder, argv). Until the game
  folder is resolved, log to the console only.
- `src/gamedir.rs`: §4.2 — `current_exe().parent()` / `--game-dir`,
  canonicalised; gate on `spice64.exe` or `ddr_world_hook.dll`; `GameDir`
  with the work-dir layout paths.
- `scripts/build_release_archive.sh`: add the updater build (same flags as the
  DLL block, run in `updater/`), copy `ddr_world_hook_updater.exe` into the
  stage, and add a guard that fails the script if `strings` finds `ProcessPrng`
  in either binary; correct the stale rsync comment (the `_cache`/`*_ifs`/
  `*.arc` exclusions it describes do not exist; `*_ifs/` under
  `custom_options`, `custom_folders`, `custom_series`,
  `music_wheel_song_length` are committed shipping content).

**Tests.**
- `cli` unit tests: every flag parses; `--tag` without `--from-zip` is a usage
  error; unknown flag → `Usage` outcome; `--game-dir` value captured.
- `gamedir` tests (temp dirs): folder with `spice64.exe` accepted; with only
  `ddr_world_hook.dll` accepted; empty folder refused with a message naming the
  path; `--game-dir` override wins over the exe location.
- `log` test: log file is created/truncated and contains the header line.

**Integration.** First step; establishes the crate, the script hook, and the
conventions the following steps extend.

**Demo.** `cargo test` passes on the host. `cargo xwin build` (Win7 target)
produces `ddr_world_hook_updater.exe`; run under CrossOver from a game folder it
prints the resolved folder and "nothing to do yet", from a scratch folder it
refuses with exit 0; `strings` shows no `ProcessPrng`. Running
`scripts/build_release_archive.sh` yields a zip whose root contains the exe.

### Step 2: Local end-to-end install — archive extraction, manifest, plan, transactional apply, `--from-zip`

**Objective.** Make the updater able to install a release archive from disk
into a game folder correctly and safely: release-owned files written, dropped
files pruned only when unmodified, everything else untouched, full rollback on
failure, manifest written last. This is the highest-risk logic; landing it
before any networking lets the whole install path be exercised with the
maintainer's own release zips.

**Implementation guidance.**
- Add `zip` (`default-features = false`, `deflate`).
- `src/archive.rs` (§4.5): extract into `work/stage/` with `enclosed_name()`,
  symlink rejection, entry/size caps, `NotAModpack` when the root lacks
  `ddr_world_hook.dll`; return the `RelPath` listing.
- `src/manifest.rs` (§4.9, §5.1): `Manifest` (serde), `read` (absent vs
  unparseable → `Ok(None)` + WARN), `write` (temp + rename), `needs_update`,
  `sha256_file`, hashing of the staged tree.
- `src/plan.rs` (§4.10): pure `build` over the stage listing + hashes, the
  previous manifest, and an injected disk probe. In this step `WriteMerged` is
  emitted only when the user file is ABSENT (copy the release file); the real
  merges arrive in Step 3.
- `src/apply.rs` (§6.1–6.3): journal write, `backup/` reset, action execution
  by rename, same-run rollback with collected restore errors, `RolledBack` vs
  `RollbackFailed`, deletion of `download/`/`stage/` on success. Write the
  journal now (crash RECOVERY is Step 5) so the on-disk protocol is final.
- `main.rs`: implement `--from-zip <path> [--tag]` (tag default
  `local:<first 12 hex of the zip's sha256>`, asset name = file name, digest
  computed locally) and `--force`; skip when `needs_update` is false.
- Console summary line per §4.13 ("N files written, M obsolete files removed,
  K locally modified files kept").

**Tests.**
- `archive`: synthetic zips — normal extraction; `../evil`, absolute and
  drive-letter names rejected; symlink entry rejected; entry-count and
  total-size caps; missing DLL → `NotAModpack`.
- `manifest`: JSON round trip; `needs_update` truth table (absent / tag
  differs / digest differs / equal / `force`); unknown `schema` and garbage →
  treated as absent.
- `plan`: first run → no `Prune`; second run → `Prune` for unchanged dropped
  files, `KeepModified` for changed, nothing for missing; paths outside both
  manifest and stage never appear; merged files and the updater exe are never
  pruned; `existed` flags reflect the probe.
- `apply` integration (temp dirs): synthetic game folder (stub `spice64.exe`,
  old DLL, `data_mods/` with shipped files, a user folder, `_cache/`, a
  generated `texturelist.merged.xml`) + synthetic zip → after `--from-zip`:
  shipped files replaced, user/`_cache`/generated files byte-identical,
  manifest correct, `backup/` holds the replaced originals, `download/` and
  `stage/` gone. Second run with a zip that drops one file and modifies
  another: prune/keep behaviour. Injected failure (a target path pre-created
  as a directory so the rename fails): folder byte-identical to before the
  run, exit 0, cause logged. Rollback failure path exercised by making a
  backup unreadable → exit 1 with the backup path in the message. `--force`
  re-applies; a second identical run is a no-op ("up to date").

**Integration.** Uses Step 1's `GameDir`, logger and exit mapping; `--from-zip`
replaces Step 1's "nothing to do yet" placeholder.

**Demo.** On the CrossOver install: `ddr_world_hook_updater.exe --from-zip
<release zip>` installs the archive, prints the summary, writes the manifest;
running it again reports "up to date"; `--force` reinstalls; deleting a shipped
texture from a copy of the zip and installing that copy prunes the file from
the game folder while a user-added folder under `data_mods/` survives.

### Step 3: Configuration merges — `mod-config.json`, `option_menu_settings`, `judgement_offsets.csv`

**Objective.** Deliver the user-facing merge semantics (§4.6–§4.8, R12–R18) as
pure, exhaustively tested modules and wire them into the install pipeline so
an update preserves every user value while adding what the release introduces.

**Implementation guidance.**
- `src/merge/json.rs`: `merge_config` with the dotted-path `MergeReport`;
  detect `custom_options.option_menu_settings` by path and delegate.
- `src/merge/option_menu.rs`: the §4.7 algorithm exactly as written
  (definitions, four-way `insert_at` match).
- `src/merge/csv.rs`: `parse`/`serialize`/`merge_csv` mirroring the DLL's
  grammar in `src/mods/per_song_judgement_offsets/csv.rs` (read it; copy its
  literal test fixtures so parity is pinned).
- Pipeline (`main.rs`/`plan.rs`): read the user files from the game folder,
  merge against the staged release copies, emit `WriteMerged` only when the
  bytes changed; unparseable user file → copy it to `backup/`, WARN, skip that
  merge (R15). Log the reports in the §4.13 wording.

**Tests.**
- `merge::json`: new top-level key; new nested key; user scalar wins; arrays
  atomic; type mismatch keeps user; user key order preserved with new keys
  appended; whole-subtree copy when `custom_options` is absent; report paths;
  idempotence; output ends with a newline and uses 2-space indent.
- `merge::option_menu`: the seven worked examples from §4.7 as table-driven
  tests; case-insensitive ids; duplicate user ids; rows without `id` on either
  side; release flags copied verbatim; user flags untouched; ids the release
  dropped kept; the repo's real `option_menu_settings` (loaded from
  `mod-config.json` via `CARGO_MANIFEST_DIR/..`) merged with itself is a no-op
  and, with any single row removed from the user copy, reproduces the original
  order exactly.
- `merge::csv`: grammar parity fixtures (header optional, CRLF, trimming, ±100
  clamp, >3 cells dropped, non-integer dropped, duplicate first-wins,
  `code,,5`); blank cells filled, non-blank kept, per-side independence;
  missing rows appended in release order; user order preserved; no-change
  report is zero; the repo's `judgement_offsets.csv` merged with itself is a
  no-op; LF + trailing newline.
- Pipeline integration (extend the Step 2 temp-dir harness): a customised
  `mod-config.json` and a CSV with user values survive an update that adds
  keys, a header, rows and offsets; unparseable user config is left untouched
  and copied to `backup/`.

**Integration.** Replaces Step 2's "copy when absent" placeholder for the two
merged files; everything else in the apply path is unchanged.

**Demo.** On the CrossOver install with a deliberately customised
`mod-config.json` (reordered menu, a removed row, a changed value) and a CSV
with hand-set offsets: `--from-zip` a release archive whose config carries an
extra key and an extra menu row; the console reports what was added, the
user's values and ordering are intact, the new row sits under its header, and
the blank CSV cells are filled while set ones are untouched.

### Step 4: GitHub release feed, download with digest verification, `--check`, `--include-prerelease`

**Objective.** Connect the proven install path to the real release feed so a
bare `ddr_world_hook_updater.exe` run does the whole job, and give operators
the diagnostic `--check`.

**Implementation guidance.**
- Add `ureq` (`default-features = false`, `tls`, `json`).
- `src/github.rs` (§4.3): `Release`/`Asset` models (unknown fields ignored),
  `fetch_latest` for `/releases/latest` and `/releases?per_page=10`, pure
  `select_release`, `select_asset`, `parse_digest`; agent with 10 s connect /
  30 s read timeouts and the mandatory `User-Agent`; `--repo` override.
- `src/download.rs` (§4.4): streamed download to `work/download/<asset>` with
  running SHA-256, size check, digest check (warn when the digest is absent),
  10 % progress ticks.
- `main.rs`: default mode = fetch → `needs_update` → download → the Step 2/3
  pipeline; `--check` prints the outcome and exits 0/3 without touching the
  folder; network/API failures print the one-line "skipped" message and exit 0
  (§7 table).

**Tests.**
- `github`: fixture JSON captured from the live API (the `v1.0`–`v1.2`
  responses, inlined): asset selection by name; a second non-matching asset
  ignored; two matching assets → first + warning; missing digest → `None`;
  `parse_digest` rejects wrong prefix/length/non-hex; `select_release` ignores
  drafts, orders by `published_at`, includes pre-releases only when asked.
- `download`: verification logic factored so the size/digest checks are unit
  tested on in-memory data (the HTTP transport itself is covered manually).
- `--check` exit codes via the temp-dir harness with a stub release object.

**Integration.** Slots in front of Step 2's pipeline; `--from-zip` remains the
offline path and shares every stage after download.

**Demo.** From the CrossOver install: `--check` reports "update available"
(exit 3) when the manifest names an older tag, "up to date" (exit 0) after an
install; a bare run downloads the live latest release (progress shown),
verifies the digest, installs it, and writes the manifest; with networking
disabled the run prints the skipped line within the timeout and exits 0.

### Step 5: Hardening — journal crash recovery, self-update, cleanup, console-owner wait, changelog display

**Objective.** Close the remaining robustness and UX items from the design:
recover an interrupted run, replace the updater's own executable, tidy up
after prunes, behave well when double-clicked, and show what an update
contains.

**Implementation guidance.**
- `apply::recover_if_interrupted` (§6.4): run at start-up before any network
  call; restore from `backup/` per journaled action; delete the journal; log
  what was recovered.
- `src/selfupdate.rs` (§4.12): `cleanup_stale` at start-up; `swap_in` as the
  `SelfUpdate` action (rename → `.exe.old`, move the staged exe in); `undo` for
  same-run rollback; plan emits `SelfUpdate` only when the staged exe's hash
  differs from the running one.
- Empty-directory cleanup under `data_mods/` for ancestors of pruned files
  (best-effort, after a successful apply).
- Console-owner detection (R19): `[target.'cfg(windows)'.dependencies]
  windows-sys` with `Win32_System_Console`; when `GetConsoleProcessList`
  reports the updater as the console's only process, print "Press Enter to
  close" and wait; never on other platforms and never when launched from a
  batch file.
- Changelog display (§4.13): release name plus the first non-empty lines of
  `body` (cap ~15 lines, strip Markdown emphasis markers) and the `html_url`.

**Tests.**
- Journal recovery (temp dirs): simulate a crash by running the apply harness
  with a hook that aborts after N actions leaving the journal in place; the
  next run restores every backed-up file, deletes newly written ones, removes
  the journal, and then completes the update normally.
- `selfupdate`: rename-swap and `undo` on dummy files; `cleanup_stale` removes
  `.old`; plan omits `SelfUpdate` when hashes match.
- Empty-dir cleanup: only directories emptied by pruning are removed; user
  directories and non-empty directories are untouched.
- Changelog formatter: line cap, emphasis stripping, empty body.
- Console detection is compiled only on Windows; the decision function that
  consumes the process count is unit tested with injected values.

**Integration.** Recovery runs before Step 4's fetch; `SelfUpdate` joins the
Step 2 plan ordering (after merges, before the manifest); the changelog is
printed between Step 4's "update available" and the download.

**Demo.** Under CrossOver: kill the updater mid-install (or plant a journal +
backup by hand), relaunch — the folder is restored and the update completes;
a release zip carrying a modified updater exe replaces the running exe, the
`.old` sibling disappears on the following run; double-clicking the exe in
the Wine desktop waits for Enter, while launching from `gamestart.bat` does
not; an update prints the release notes before downloading.

### Step 6: Operator documentation, maintainer docs, and platform validation

**Objective.** Make the feature usable and maintainable: tell operators how to
install it, record the maintainer-facing facts in the repo's agent docs, and
validate on the real platforms.

**Implementation guidance.**
- `README.md`: "Automatic updates" section per Appendix C (bat snippet,
  behaviour summary, `--check`/`--force`/`--include-prerelease`, backup
  location, how to opt out); mention the updater in the Installation steps.
- `AGENTS.md`: a Key Entry Points row for the updater (crate location, build
  recipe, merge rules in one paragraph, manifest/journal file names, "never
  edit the Win7 build flags independently of `build_release_archive.sh`");
  add the updater's `cargo test` to the Build section.
- Final cross-build via `scripts/build_release_archive.sh`; confirm the zip
  root lists `ddr_world_hook_updater.exe` and the import-table guard passes.
- Platform validation runbook (record results in `progress.md`): CrossOver —
  fresh folder without manifest installs latest; Windows 10/11 — same, plus the
  locked-DLL case (game running → rolled back, exit 0); Windows 7 cabinet —
  exe launches, TLS to GitHub succeeds, install completes, self-update
  `.old` cleanup works on the next boot; `gamestart.bat` ordering confirmed
  (game does not start until the updater exits).

**Tests.** No new code paths; the full `cargo test` suite and the Step 1
import-table guard run as the gate. Manual validation checklist above.

**Integration.** Documentation and validation over the completed Steps 1–5.

**Demo.** A stock cabinet with the one-line `gamestart.bat` addition boots,
the updater installs the newest release from GitHub, the game starts with the
new DLL, the operator's configuration and offsets are intact, and the README
explains all of it.
