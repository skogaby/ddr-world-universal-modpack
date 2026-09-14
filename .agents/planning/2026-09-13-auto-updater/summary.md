# Summary: DDR World Hook Auto-Updater

PDD pass completed 2026-09-13. Register accepted, design approved, plan approved.

## Artifacts

| Path | Content |
|------|---------|
| `rough-idea.md` | The maintainer's original request, verbatim in substance |
| `idea-honing.md` | Decision register D1–D18 (D1 overridden → `ddr_world_hook_updater.exe`; D2–D13 accepted; D14–D18 assumed) with per-decision rationale; `Readiness Confirmed 2026-09-13` |
| `research/orientation.md` | Repository + live release-feed facts: flat archive layout, DLL config/CSV semantics, `option_menu_settings` header rules, runtime-generated files under `data_mods/`, no on-disk version marker, launch-integration facts |
| `research/technologies.md` | Win7 cross-build probe result (rustls/ring/ureq/zip/serde_json link for the tier-3 target with no `ProcessPrng` import; live TLS request succeeded under CrossOver) and the GitHub Releases API facts used |
| `prototypes/win7-tls-probe/` | Throwaway build probe; NOT product code, never to be carried into `updater/` |
| `design/detailed-design.md` | Approved design: requirements R1–R27, architecture, module interfaces, merge algorithms, transactional apply protocol, data models, error table, testing strategy, appendices |
| `implementation/plan.md` | Approved 6-step plan with checklist |
| `summary.md` | This file |

## Design in brief

A standalone Rust console exe, `ddr_world_hook_updater.exe`, built from a
separate crate `updater/` with the DLL's Windows 7 cross-build recipe and
shipped at the root of every release zip. Invoked as one bare line in
`gamestart.bat` before `spice64.exe`, it queries the GitHub Releases API,
compares the latest release's tag and asset SHA-256 with a local manifest
(`ddr_world_hook_updater.manifest.json`; equality, not ordering), downloads and
verifies the archive, extracts it to a staging directory, computes merged
`mod-config.json` (recursive additive merge, user values win, header-scoped
insertion into `option_menu_settings`) and `judgement_offsets.csv` (cell-level
fill, never overwrite a set value), and applies everything transactionally:
release-owned files overwritten, files the previous release shipped but the new
one dropped pruned only when unmodified, user mods and runtime-generated files
never touched, every replaced file backed up, a journal enabling rollback of an
interrupted run, self-replacement by rename-swap, manifest written last. It is
non-interactive and exits 0 in every case except a failed rollback, so an
offline or failing update never prevents the game from starting.

## Plan in brief

1. Crate skeleton, CLI, logging, game-folder gate, release-script integration.
2. Local end-to-end install (`--from-zip`): extraction, manifest, plan,
   transactional apply with rollback.
3. The three merges with exhaustive host tests.
4. GitHub feed, verified download, `--check`, `--include-prerelease`.
5. Hardening: journal recovery, self-update, cleanup, double-click wait,
   changelog display.
6. README/AGENTS documentation and Win7 / Win10 / CrossOver validation.

## Status (2026-09-13)

All six plan steps are implemented (`updater/`, 111 unit + 26 e2e host tests,
warning-free Win7 cross-build, docs in README/AGENTS) and validated under
CrossOver on the maintainer's real install, including a live update from
GitHub (v1.2), merges on the real config, a game boot on the merged config,
injected rollback, crash recovery and self-update. Task records:
`.agents/tasks/2026-09-13-auto-updater/step0{1..5}/`,
`.agents/scratchpad/2026-09-13-auto-updater/*/progress.md`; live status:
`progress.md` in this directory.

## Next steps (maintainer)

1. Review and commit the uncommitted work (agents never commit here).
2. Windows validation runbook (see `progress.md`): Win10/11 fresh install +
   locked-DLL rollback; Win7 cabinet TLS/install/self-update/bat ordering.
3. Publish the next release via `scripts/build_release_archive.sh` — the zip
   already carries `ddr_world_hook_updater.exe`; operators copy it once and add
   the bat line, after which it self-updates.
4. Review the committed `mod-config.json` as the release default (see below).

## Assumptions and areas to watch during implementation

- **Windows 7 execution is inferred, not observed.** The probe proves the
  import table is Win7-safe (same evidence the DLL relies on) and that the
  binary runs under Wine; the first physical Win7 run happens in Step 6's
  validation. If it fails, the fallback candidates are `+crt-static` (drop the
  UCRT dependency) or a different TLS provider — both are contained in
  `updater/`.
- **GitHub asset `digest`** has been present on every release so far; the
  design degrades to a size check with a warning if it ever disappears.
- **Committed `mod-config.json` = release defaults.** New `mods.<id>` keys and
  new sections reach users with whatever values are committed (today the
  maintainer's own config, incl. a cabinet-specific `timing_offsets.sound_offset`
  and several mods set `false`). Worth a deliberate review before the first
  auto-updated release; not part of this feature.
- **First run in a folder never prunes** (no previous manifest). Moot today —
  no file has ever been removed from `data_mods/` in a published release — but
  a `retired_paths.txt` mechanism is the noted follow-up if that changes.
- **CSV corrections do not propagate**: a value the user inherited from an
  older release is indistinguishable from one they set; only additions and
  blank-cell fills reach users. Accepted limitation.
- **The DLL's hardcoded splash version** (`src/lib.rs`, "v1.2") could read the
  new manifest — the most likely follow-up feature.
- **`-Z build-std=std,panic_abort`** is reused from the DLL script for
  consistency; the probe showed std still pulls `panic_unwind`, so the
  updater's `Result`-based rollback plus top-level `catch_unwind` both work.
  Step 1 re-verifies with the real crate.
