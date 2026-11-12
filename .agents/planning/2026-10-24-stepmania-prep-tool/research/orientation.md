# Orientation — StepMania prep tool

What the repository already has that shapes this tool, and the constraints it
inherits. Everything below was read from the source on 2026-10-24.

## The in-game loader's cache is the tool's output

`src/services/stepmania/cache.rs` — `./data_mods/_cache/stepmania/`, every path
relative to the game directory (the DLL runs with the game folder as cwd):

| artefact | written by | needed at boot? |
|---|---|---|
| `index.bin` | `index.rs::run` — ONLY at the end of a complete run | yes — the warm path (`warm_fingerprint` + `artefacts_present`) reuses a song only when its record is here |
| `ssq/<b>[_N].ssq`, `xsb/<b>.xsb`, `tn/<b>.dds` | `index.rs::convert_song` | yes — `artefacts_present` |
| `sync.bin` | `sync_apply` / `sync_state` from `sync_sweep::measure_song` | no — missing records are measured after boot (sweep) or at song decide (bounded hold) |
| `thumbnail/<S>.arc` | `mod.rs::build_thumbnail_arc` | built at the `startup.arc` open from `tn/`; depends on the runtime series value → not pre-buildable |
| `xwb/`, `jacket/` (LRU) | scheduler / providers on demand | no |
| `data_mods/_cache/step_data/v1.bin` | `mods/fast_bootup` from the game's own analysis | no — cannot be produced outside the game |

So "warm" = `index.bin` + the three per-song artefacts; "sync-ready" = `sync.bin`
records for every song that will be injected.

## What a warm boot costs today

- Warm index: stat walk, ~tens–200 ms per 1,000 songs (`progress.md` N1).
- Cold index: ~25 ms/song aggregate on 6 workers (627 songs = 15.4 s; 14k ≈ 6 min).
- `index_timeout_s` (30 s): the `startup.arc` open injects what finished; the
  index thread keeps converting and writes `index.bin` at completion. **Progress is
  lost if the process dies before completion** (no checkpoint).
- Sync sweep (`sync_sweep.rs`): below-normal thread after boot, ~0.8 s/song
  analysis / ~1.3 s/song wall, one song at a time, paused during gameplay. 4,000
  songs ≈ 1.5 h of background work per fresh library.
- Ceiling (`ceiling.rs`, 2026-10-24): the game receives at most
  `MAX_INJECTED_SONGS` (4,000) songs, first by `pack/folder` name; nothing under a
  stock-sized XML arena.

## Code that can be reused as-is vs. what is impure

The repository already mounts DLL source into native crates by `#[path]`:
`tools/sm_pack_check` (mounts `core::{sm,ssq,audio,sync,xact,art}` and
`services::stepmania::{convert,identity,model,musicdb}`), `tools/bot_sim`,
`scripts/validate_*.sh`. The purity contract: a mounted file imports only
`crate::core::*` and siblings, and nothing from `crate::services::*`/`crate::mods`
other than through `super::`.

Status of the files the tool needs:

| file | mountable today? | blockers |
|---|---|---|
| `services/stepmania/{identity,model,musicdb,convert,ceiling,sync_state}.rs` | yes (already in the harness) | — |
| `core/{sm,ssq,audio,sync,xact,art}` | yes (sm_pack_check mounts them) | `core/art/mod.rs` has a DLL-only part; sm_pack_check mirrors it with its own `core/art/mod.rs` |
| `services/stepmania/cache.rs` | nearly | `use crate::{log_info, log_warn}` — the tool can define the macros at its crate root |
| `services/stepmania/index.rs` | nearly | `crate::{log_info, log_warn}`; `finish()` calls `super::on_index_complete()` (a runtime hook — thumbnail-arc rebuild); the process-wide `STATE`/`BY_BASENAME` statics are plain `std::sync` and fine in a tool |
| `services/stepmania/sync_sweep.rs::measure_song` | no | the file imports `scene_manager`, `per_song_judgement_offsets`, `sync_apply`; the measurement body itself uses only `core::*` + `convert` + `index::read_capped` + `model` |

`tools/sm_pack_check/src/main.rs` carries its OWN `convert_song` (a re-implementation of
the pipeline for a report) — a drift risk this tool must not repeat: the DLL
reuses a record only when `fingerprint`, identity and artefacts match, so a tool
that converts even slightly differently produces a cache the game re-converts
anyway.

## Patterns to inherit from `updater/`

- `updater/src/gamedir.rs`: the game folder is the directory containing the exe
  (or `--game-dir`), accepted only if it holds `spice64.exe` or
  `ddr_world_hook.dll`. Never trusts the working directory.
- `updater/src/console.rs`: "alone on the console" wait so a double-clicked exe
  does not vanish; `log.rs`: per-run log file in the game folder.
- Built for `x86_64-win7-windows-msvc` with `trim-paths`
  (`scripts/build_release_archive.sh` line 129); shipped in the release zip and as
  a bare exe; `gamestart.bat` runs it on the line before spice2x.
- Separate crate with `[workspace]` of its own (not a member of the DLL crate) so
  it builds natively for tests.

## Concurrency with the game

Nothing today prevents the tool and the DLL's index thread from writing the same
cache at once. All artefact writes are `write_atomic` (tmp + rename) and
`index.bin` is rewritten whole, so the worst case is wasted work or a lost
`index.bin` update, not corruption — but the tool should refuse to run while the
game is up.

## Config the tool must honour

`mod-config.json` → `mods` (is `stepmania-packs` enabled?) and `stepmania_packs`
(`auto_sync`; `audio_cache_mb` is irrelevant offline). Reading the JSON needs
`serde_json` (the DLL's `config.rs` is not mountable — it pulls the whole mod
registry); the tool reads the two keys it needs with a tiny struct.
