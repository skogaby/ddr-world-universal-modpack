# Plan — Step 2 Task 03: transactional apply + --from-zip
Status: Approved 2026-09-13 (auto; approved chain)

## e2e scenarios (`tests/apply_e2e.rs`, synthetic game folder + zip)
A1 first run: shipped replaced (DLL, data_mods/shipped.png), user pack + _cache + texturelist.merged.xml untouched, README written, manifest {tag local:…, files = 3}, backup/ has old DLL + old shipped.png, download/ + stage/ + journal gone, stdout summary "2 files written", exit 0.
A2 rerun → "up to date (local:…)" exit 0, mtime unchanged; `--force` → reinstalls (exit 0, summary again).
A3 v2 zip drops shipped.png + adds new.png; user modified README locally → prune shipped.png (moved to backup), keep README (KeepModified reported), new.png written; manifest files updated.
A4 `--from-zip` with `--tag v9` records tag v9.
A5 injected `DDR_UPDATER_FAULT=apply-after:1` → every file byte-identical to before (hash the tree), no manifest, exit 0, output "rolled back".
A6 `DDR_UPDATER_FAULT=rollback` → exit 1, output names `.ddr_world_hook_updater/backup`.
A7 missing user mod-config.json → copied from release; present → byte-identical after run.
A8 not-a-modpack zip → "skipped" exit 0, nothing written; missing zip path → skipped exit 0.
A9 parent-is-a-file conflict (`data_mods/dir_as_file` is a file but release has `data_mods/dir_as_file/x.png`) → rolled back, exit 0.

## Unit tests (`apply.rs`)
journal round trip; `fault::parse` table; summary formatting.

## Shape
- `apply.rs`: `Journal`, `Summary`, `ApplyError::{RolledBack(String), RollbackFailed{cause, restore_errors: Vec<String>}}`, `execute(game, plan, stage_root, merged, manifest) -> Result<Summary, ApplyError>`; internal `Executor { done: Vec<Done> }` where `Done` records how to undo each performed action; `rollback(&done)`.
- `fault.rs`: `Fault::{None, ApplyAfter(usize), Rollback}` from env; checked in the executor loop.
- `main.rs`: `run_from_zip(cli, game)` pipeline; summary/skip/rollback messages; `Outcome` mapping.
