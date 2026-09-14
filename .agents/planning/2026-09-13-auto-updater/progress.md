# Progress: Auto-Updater

Updated: 2026-09-13
Status: All 6 plan steps done (uncommitted). Remaining: MAINTAINER-ONLY
hardware validation (below) and the first release that ships the updater.
NEXT ACTION (maintainer): (1) review + commit the uncommitted work; (2) run
the Windows validation runbook below on a Win10/11 machine and a Win7 cabinet;
(3) publish the next release with `scripts/build_release_archive.sh` (the zip
already carries `ddr_world_hook_updater.exe`); operators copy the exe once and
add the bat line — from then on it self-updates.

Resume protocol: read `summary.md` → `implementation/plan.md` (checklist) →
`design/detailed-design.md` (§ references) → `idea-honing.md` (why). Research
lives in `research/`. Per-task working records:
`.agents/scratchpad/2026-09-13-auto-updater/<task-name>/{context,plan,progress}.md`.

## Done
- 2026-09-13 PDD pass: register accepted, design approved, plan approved.
- Win7/TLS cross-build de-risked by `prototypes/win7-tls-probe/` (throwaway —
  never copy into `updater/`).
- **Step 1 (3 tasks, all `Status: Complete (uncommitted)`):**
  - `updater/` crate (`ddr-world-hook-updater` package, bin
    `ddr_world_hook_updater`), `cli.rs` (full §4.1 flag set, usage errors exit
    0, help/version precedence), `main.rs` (`Outcome` → single `exit_code`
    mapping incl. reserved 1/3, `catch_unwind` guard, quiet panic hook,
    placeholder lines), `gamedir.rs` (exe-relative resolution + `--game-dir`,
    `spice64.exe`/`ddr_world_hook.dll` gate, `dunce` canonicalisation, work-dir
    accessors), `log.rs` (console + per-run file, `[+S.SSs] LEVEL` lines,
    header, `log_info!/warn!/error!`). 36 unit + 9 e2e tests green; host and
    Win7 builds warning-free (apart from cargo's cosmetic kebab-case bin-name
    note). Exe 216 576 B, no `ProcessPrng`.
  - `scripts/build_release_archive.sh` builds the updater with the Win7 recipe,
    ships it at the zip root, guards both binaries against `ProcessPrng` (fails
    closed without `strings`), comments corrected. Full run → 408 entries
    (407 + exe). `/updater/target` gitignored.

- **Step 2 (3 tasks, all `Status: Complete (uncommitted)`):** `relpath.rs`,
  `archive.rs` (two-pass safe extraction, caps, `NotAModpack`), `manifest.rs`
  (schema 1, temp+rename write, equality `needs_update`, sha256 helpers),
  `plan.rs` (pure §4.10 rules; merged files + updater exe skipped), `fault.rs`
  (`DDR_UPDATER_FAULT`), `apply.rs` (journal, backup mirror, rename-based
  execute, reverse rollback, cleanup), `main.rs::install` pipeline reached via
  `--from-zip [--tag] [--force] [--check]`. 61 unit + 17 e2e tests green. Win7
  exe 533 504 B. Installed the 20260913 release zip onto the REAL install (see
  deploy log) and rolled back an injected failure on it byte-identically.

- **Step 3 (4 tasks, all `Status: Complete (uncommitted)`):** `merge/json.rs`,
  `merge/option_menu.rs` (§4.7 verbatim; 41-row round-trip proof on the
  committed list), `merge/csv_grammar.rs` (`#[path]` mount of the DLL's csv.rs)
  + `merge/csv.rs`, wired into `main.rs::merge_inputs` with §4.13 report lines
  and R15 handling. 95 unit + 21 e2e tests green. Win7 exe 602 112 B.
  Real-file check on a COPY of the install's config/CSV: exactly one addition
  (`mods.series-expansion: false`), CSV unchanged — real run deferred (see
  Deviations & open questions).

- **Step 4 (3 tasks, all `Status: Complete (uncommitted)`):** `github.rs`
  (rustls via `ureq`, latest/prerelease selection, asset + digest parsing),
  `download.rs` (streamed sha256, size/digest verify), `main.rs::run_default`.
  103 unit + 21 e2e green. Win7 exe 2 446 848 B. LIVE update from GitHub on
  the real install: v1.2 downloaded (6.1 MB, 0.5 s), digest-verified, installed
  (366 written, 3 pruned), re-run up to date; dev build then restored with
  `--from-zip`.

- **Step 5 (3 tasks, all `Status: Complete (uncommitted)`):** journal crash
  recovery, empty-dir cleanup, self-update rename-swap (`SelfUpdate` action,
  undo on rollback, `.old` cleanup), bounded console Enter-wait, changelog
  preview. 111 unit + 26 e2e green. Win7 exe 2 499 072 B. Self-update, bat
  no-wait and crash recovery all exercised under CrossOver on the real install.

- **Step 6:** README "Automatic updates" section (+ Installation bullet,
  Troubleshooting bullet, Building block), AGENTS.md Key Entry Points row +
  Build lines, final `scripts/build_release_archive.sh` run → 408-entry zip
  with the 2 552 832 B updater; the shipped exe re-installed that zip on the
  real install (`--from-zip`, up to date on rerun, live `--check` → v1.2
  available = the documented dev-cabinet caveat).

## Post-plan addition (maintainer request, 2026-09-13): tester installer
`scripts/build_release_archive.sh` now writes to `release/` (gitignored): the
zip, a bare `ddr_world_hook_updater.exe`, and `install-update-YYYYMMDD.bat`
(same `STAMP` as the zip) running `--from-zip "%~dp0<zip>" --tag
dev-YYYYMMDD-<git sha>[-dirty]`; guards: not-a-game-folder, missing exe/zip,
`spice64.exe` running (`tasklist | find`); `pause` at the end; NOTE about the
auto-update line downgrading a private build. CRLF. Tested under CrossOver on
the real install: install (369 written, tag `dev-20260913-dce1ccf-dirty`),
rerun up to date, all three guards incl. a real running game. README "Testing a
private build" paragraph + AGENTS row updated.

## Validation runbook — status
| Check | Status |
|-------|--------|
| CrossOver: fresh folder (no manifest) installs latest from GitHub | DONE 2026-09-13 (v1.2 live) |
| CrossOver: `--check` exit 0/3, offline/bad repo → skipped exit 0 | DONE |
| CrossOver: merges on the real config/CSV + game boots on merged config | DONE |
| CrossOver: rollback (injected), crash recovery, self-update, `.old` cleanup, bat no-wait | DONE |
| Windows 10/11: fresh install from GitHub; locked-DLL case (game running → rolled back, exit 0) | **OPEN — maintainer** |
| Windows 7 cabinet: exe launches, TLS to GitHub succeeds, install completes, self-update `.old` cleanup on next boot, bat ordering (game waits) | **OPEN — maintainer** |

## In flight
- Nothing. Uncommitted changes: `.gitignore`, `scripts/build_release_archive.sh`,
  `README.md`, `AGENTS.md`, `updater/**`; untracked build artifact
  `ddr-world-universal-modpack-20260913.zip` (maintainer: upload or delete —
  never `git add`).
- Real install state after the tester-installer test: the release-zip exe,
  manifest tag `dev-20260913-dce1ccf-dirty` (the maintainer's dev build content),
  `README.md`, `.ddr_world_hook_updater/backup/`. **A bare updater run on this
  install will DOWNGRADE to v1.2** (latest published) until the dev build is
  released — keep the bare run out of the dev cabinet's bat (Step 6 documents
  this).

## Deploy & test log
- 2026-09-13 **Step 5 on the real install (CrossOver):** self-update with the
  running image (zip carrying a padded copy of the new exe): `updater replaced`,
  running exe → `.exe.old`, next run removed `.old`; `cmd /c <bat>` path does
  not wait, bare `wine exe` launch shows the bounded 60 s Enter prompt (EOF →
  immediate exit); `DDR_UPDATER_FAULT=crash-after:50` → exit 70 + journal; next
  run `restored 50 file(s)` then completed the install. Install left with the
  Step 5 exe, manifest tag `dev-step5-b`.
- 2026-09-13 **Step 4 LIVE GitHub run on the real install** (CrossOver):
  `--check` → `update available: v1.2 (installed: local:…)` exit 3 (0.3 s);
  bare run → `ddr-world-universal-modpack-20260903_hotfix.zip` 6.1 MB in
  ~0.5 s, `Verified … against the published sha256`, 368 extracted, config/CSV
  no changes, 366 written + 3 pruned (S-MFC lamp textures newer than v1.2),
  manifest `v1.2`, 2.8 s total; re-run `up to date (v1.2)`; dev build restored
  with `--from-zip … --tag dev-20260913`.
- 2026-09-13 **Step 3 FULL END-TO-END on the real install** (maintainer-approved
  config override): `--from-zip <20260913 zip> --force` → `Merging
  mod-config.json ... 1 key added (mods.series-expansion)`, CSV `no changes`,
  369 files written, exit 0; diff = one appended key + trailing newline;
  pre-merge config byte-identical in `.ddr_world_hook_updater/backup/`. Then
  BOOTED THE GAME via the operator's `gamestart-bemanibuddy.bat` (windowed):
  `Config: loaded mod-config.json`, `Mod 'Series Expansion' early_apply skipped
  (disabled in config)` (the merged key took effect), `DDR World Hook DLL ready.
  37 mod(s) active.` at ~18 s, no new hook WARN/ERROR classes vs the previous
  boot (only the usual `_v1` alternate-signature misses). Game killed after the
  ready line; boot log kept as `log.updater-test-boot.txt` in the install.
- 2026-09-13 **Step 3 real-file check** (CrossOver, temp game folder holding
  COPIES of the real install's `mod-config.json` + `judgement_offsets.csv` + DLL,
  real 20260913 zip): `Merging mod-config.json ... 1 key added
  (mods.series-expansion)`, `Merging judgement_offsets.csv ... no changes`; diff
  = that one appended key + trailing newline, DLL-alphabetical order preserved.
- 2026-09-13 **Step 2 on the real install**: `--from-zip <20260913 zip> --check`
  → exit 3; install → 369 files written in ~4.5 s, DLL sha unchanged (zip DLL
  byte-identical to the deployed one), both user files untouched, data_mods file
  count unchanged (1213), backup 368 originals; rerun → "up to date";
  `DDR_UPDATER_FAULT=apply-after:3 --force` → rolled back, tree digest identical.
  Game not launched afterwards (DLL bytes unchanged, nothing to observe).
- 2026-09-13 **Real install (`$DDR_WORLD_INSTALL`, CrossOver `bemani` bottle, maps
  to `C:\ddr_world\contents`)**, Step 1 exe from the fresh release zip copied
  next to `spice64.exe`: (1) bare run with CWD elsewhere → exe-relative
  resolution correct, header + transcript, `ddr_world_hook_updater.log` written,
  no work dir, exit 0; (2) bat integration via a temp copy of the operator's
  `gamestart-bemanibuddy.bat` with the spice line replaced by an echo → the
  updater line BLOCKS the batch, `errorlevel 0`, next line runs afterwards;
  (3) `--check` and a usage error (`--tag oops`) both exit 0. Only
  `ddr_world_hook_updater.exe` + `.log` were added to the folder; the exe was
  left in place (its intended location).
- 2026-09-13 CrossOver (`bemani` bottle), Win7-target exe: `--version` /
  `--check` / `--tag x` (usage error) all exit 0 with the specified output;
  refusal from a scratch folder creates nothing; fake game folder (touch
  `spice64.exe`) → header + transcript + `ddr_world_hook_updater.log`, paths
  shown as `Z:\…` (no `\\?\` prefix after the dunce fix).

## Deviations & open questions
- RESOLVED 2026-09-13: the maintainer accepted the `mods.series-expansion:
  false` flip; the full end-to-end run was performed on the real install and
  the game booted cleanly on the merged config (see deploy log).
- Step 3: new SECTIONS (headers) are inserted after their release predecessor's
  section wherever the user placed it, not at the end (§4.7 as written) — say so
  in the README merge description (Step 6).
- Step 3: unparseable-file copies go to `.ddr_world_hook_updater/unparseable/`
  (not `backup/`, which every apply resets); CSV grammar shared with the DLL via
  `#[path]` (the DLL's `csv.rs` must stay dependency-free).
- Task 01: dependencies (`serde`/`serde_json`/`sha2`) deferred to the step that
  first uses them (nightly cargo `unused_dependencies` warnings); package name
  kebab-case, bin snake_case (one cosmetic cargo warning, documented).
- Task 02 (Step 1): `dunce` added (Windows verbatim-path stripping). The
  Step-2-consumed `#[allow(dead_code)]` markers were removed in Step 2; the two
  left (`apply::Journal::read`, `RelPath::parent`) are for Step 5 — remove then.
- Step 2: `--check` also works in `--from-zip` mode (exit 3/0); manifest write
  failure is non-fatal per design §7; `DDR_UPDATER_FAULT` dev env var added for
  rollback tests (documented in `fault.rs`).
- Step 2 note: adding `serde_json` `preserve_order` (IndexMap → `RandomState`)
  will make a default-target build import `ProcessPrng`; the Win7 recipe + the
  script guard cover it — never build the shipped exe with the default target.

## Key facts for a cold resume
- Exe name `ddr_world_hook_updater.exe`; crate `updater/` (own manifest, not a
  workspace member); build from `updater/` with
  `cargo xwin build --release --target x86_64-win7-windows-msvc -Z build-std=std,panic_abort`.
- Updater-owned files in the game folder: `ddr_world_hook_updater.manifest.json`,
  `.ddr_world_hook_updater/{download,stage,backup,journal.json}`,
  `ddr_world_hook_updater.log` (accessors on `gamedir::GameDir`).
- Merged (user-owned) files: `mod-config.json`, `judgement_offsets.csv`;
  everything else in the zip is release-owned; anything in neither zip nor
  previous manifest is never touched.
- Host tests: `cargo test --manifest-path updater/Cargo.toml`; e2e tests spawn
  the built binary (`CARGO_BIN_EXE_ddr_world_hook_updater`) against temp game
  folders. Agents never `git commit`.
