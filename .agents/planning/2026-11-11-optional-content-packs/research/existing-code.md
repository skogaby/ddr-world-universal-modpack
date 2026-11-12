# Research: existing code the design has to fit

Read 2026-11-11. Paths are repository-relative.

## Updater (`updater/src/`)

| Module | What it does today | What packs need from it |
|---|---|---|
| `main.rs` | `run_default`: fetch release → compare manifest → download → `install`. Returns early on "up to date", on every network failure, and on `--check`. | A pack step that runs after the base step in every outcome that leaves a usable base (updated, already current, offline), then the timed prompt. |
| `github.rs` | `/releases/latest`, or the newest non-draft of the first 10 with `--include-prerelease`; first asset matching `ddr-world-universal-modpack-*.zip`. | Pre-release selection must skip releases without a base asset. Nothing else: packs are not discovered through the API. |
| `download.rs` | `fetch_asset(agent, &Asset, dir, progress)`: streams to `download/<name>`, hashes while streaming, verifies size + digest, deletes the file on any failure. | The same call fed from a catalog entry (name, size, sha256, URL map onto `Asset`), plus resume from a kept partial file. |
| `archive.rs` | Two-pass extract into `stage/`; rejects unsafe names, symlinks, > 100 000 entries, > 2 GiB uncompressed; REQUIRES `ddr_world_hook.dll` at the root. | A pack mode: no DLL requirement; instead every entry must lie under the pack's declared root (its source folder) or be its marker file — a pack zip must not be able to write anywhere else. |
| `plan.rs` | Pure `build(stage_files, stage_hashes, previous_manifest, probe, merged)` → `Write` / `Prune` / `KeepModified` / … | Reusable as is for one pack: the marker plays the role of the previous manifest. Its prune rule (remove only if unmodified, else keep and report) is exactly register D9 rule 3. |
| `apply.rs` | Journaled, rollback-capable execution; ONE journal path; **wipes `backup/` at the start of every apply**. | Packs applied one by one would each wipe the backup of the base update that just ran. Backups need a per-transaction subfolder, with "last update only" retention applied to the run as a whole. |
| `manifest.rs` | Schema 1, one asset, `files` map. A different schema is ignored (treated as a first run). | Unchanged. Pack state never enters it. |
| `merge/option_menu.rs` | Section-aware insertion of missing rows (shipped since v1.3). | The prefix tie-break of register D11. Its tests pin the canonical `mod-config.json` round-trips; they stay. |
| `console.rs` | Alone-on-console detection; bounded Enter wait. | The timed prompt; the chooser's line input. |
| `cli.rs` | Hand-written flag parser; any malformed line prints usage and exits 0. | New flags `--packs`, `--packs-install`, `--packs-list`, `--packs-dir`. |
| `log.rs` | Every line as `[+0.00s] LEVEL msg` to console and `ddr_world_hook_updater.log`. | The checklist is drawn raw (not as log lines); what was chosen and done is logged. |
| `gamedir.rs` | Work dir `.ddr_world_hook_updater/` (`download/`, `stage/`, `backup/`, journal). | Paths for the selection file and partial downloads. |

Tests: `cargo test --manifest-path updater/Cargo.toml` runs natively (unit tests per
module, `updater/tests/apply_e2e.rs`, `updater/tests/cli_e2e.rs` with temp game
folders and zip fixtures built in-test). The same pattern covers packs; a local HTTP
fixture is only needed for resume.

Exit-code contract (auto-updater design R20): 0 whenever the game can start. Pack
failures stay inside it.

## DLL

- The custom-model scanner skips dot-entries (`src/mods/background_dancers/custom_scan.rs`,
  `name.starts_with('.')`), so `data_mods/custom_models/.packs/` is invisible to it.
- A source folder that is absent simply has no rows; the DLL needs no change for a
  cabinet with some packs missing.
- Model keys must be unique across stock and all sources; the first wins
  (`custom_content.rs` tests `duplicate_custom_key_across_folders_keeps_the_first`,
  `duplicate_key_across_sources_keeps_the_first_source`).
- Rows missing from the config: appended at the end, except `ShowWhen` families
  (`src/services/custom_options/ordering.rs`).

## Release tooling

- `scripts/build_release_archive.sh`: builds three Win7 binaries, stages
  `data_mods/` with rsync, pre-packs the staged `custom_models` in place, zips the
  stage, writes the tester `.bat` (which calls `--from-zip … --tag …`).
- `scripts/build_release_distribution.bat`: Windows twin; stages an unzipped image
  into `release/`, no archive.
- `scripts/pack_custom_models.py`: `SRC --out DST` writes a packed mirror; `--in-place`
  packs a staged copy. Deterministic (see `hosting-and-downloads.md` §4).
- `release/` is gitignored.

## v1.4 as installed

- Manifest lists loose model files under `data_mods/custom_models/dancers/<Friendly>/…`
  and `…/stages/<Friendly>/…` (pre-source layout, commit `80c8a70e`).
- Its updater binary is the one that will install v1.5; its merge and plan code are
  identical to HEAD's (`git diff 80c8a70e HEAD -- updater/src/merge` is empty).
