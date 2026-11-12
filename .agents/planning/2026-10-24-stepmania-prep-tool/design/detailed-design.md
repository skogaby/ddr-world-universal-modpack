# Detailed Design — StepMania Prep Tool (`ddr_world_hook_prep.exe`)

Status: Approved 2026-10-24

## 1. Overview

The DDR World hook DLL loads StepMania packs dropped into `stepmania/Songs/<Pack>/<Song>/`
by converting each song into the game's own formats (SSQ chart, XSB sound bank, DDS
thumbnail), injecting an entry into `musicdb.xml` as the game opens `startup.arc`, and
serving the converted files through its LayeredFS layer. Today that conversion runs *inside
the game process* at boot on a worker pool, with a 30 s bounded wait at the `startup.arc`
open and a background auto-sync sweep during play. A tester's 14,000-song library showed
the cost: minutes of silent black screen on a cold boot, a partial-injection path never
exercised on a cabinet, hours of background measurement, and two writers (the index thread
and, in principle, anything else) on one cache directory.

This design moves **all** indexing, conversion and sync measurement out of the game into a
standalone Windows executable, `ddr_world_hook_prep.exe`, that the user runs before the
game (from `gamestart.bat` on the line before spice2x, like the updater, or by
double-click). The DLL becomes a *reader* of the cache the tool wrote: it loads
`index.bin` at boot, validates each record read-only against the disk, injects the songs
the ceiling admits, and serves them. The only way the game loads StepMania songs is from
a cache the tool produced beforehand.

Consequences:

- **No race conditions** — the cache has a strict ownership split (§5.3); the DLL never
  writes a file the tool reads and vice versa.
- **No background work during play** — no index thread, no sync sweep, no on-decide
  measurement thread, no stage-load hold.
- **A smaller DLL** — `index.rs` shrinks from ~850 to ~250 lines; `sync_sweep.rs` (341)
  and most of `sync_apply.rs` (539) go; `index_timeout_s` and its timeout path go.
- **One conversion pipeline** — the tool runs the DLL's real pure modules by `#[path]`
  mounting, and the separate dry-run tool `tools/sm_pack_check` (a second implementation
  of the pipeline that had already drifted) is folded into it and deleted.

## 2. Detailed Requirements

### 2.1 Functional

| ID | Requirement |
|---|---|
| F1 | The tool indexes `stepmania/Songs/` of the game folder and writes the DLL's warm-boot cache: `index.bin` plus `ssq/<b>[_N].ssq`, `xsb/<b>.xsb`, `tn/<b>.dds` per song, under `data_mods/_cache/stepmania/`. Conversion is byte-identical to what the DLL produced before this change (same mounted code). |
| F2 | A warm run (nothing changed) is a stat walk and a no-op: no file written except the log; ~1 s per 5,000 songs. A changed song is re-converted; a vanished song's artefacts are deleted; rejections are cached against a folder stamp and re-reported without re-parsing. |
| F3 | The tool measures auto-sync for every song that the injection ceiling admits and that has no `sync.bin` record for its current fingerprint (and is not `audio_failed`), on the worker pool, and writes `sync.bin`. `--no-sync` skips the pass; `stepmania_packs.auto_sync: false` in `mod-config.json` skips it too. The estimated duration is printed before the pass starts. |
| F4 | A song whose audio fails to decode during measurement is recorded `audio_failed` in `index.bin` (the DLL then rejects it with the existing message). |
| F5 | The tool prints a ceiling report: songs indexed; songs the game will inject (`MAX_INJECTED_SONGS`, first by `pack/folder`); the packs left out with counts; a note that the report assumes Song Limit Expansion is enabled. |
| F6 | The tool refuses to run while the game is running (`spice64.exe` or `spice.exe` process present): message, exit code 2, nothing written. |
| F7 | `index.bin` is checkpointed during a cold run (every 250 converted songs or 15 s, whichever first) so an interrupted run keeps its progress. |
| F8 | The tool honours the `stepmania-packs` mod toggle in `mod-config.json`: disabled ⇒ says so, exit 0, nothing written. |
| F9 | `--dry-run <song-or-pack dir> --out <dir>` converts into a scratch directory and prints a per-song verdict (the former `sm_pack_check` behaviour, including `--code`, `--lead-in`, `--max-audio-mb`, `--auto-sync`). |
| F10 | The DLL loads `index.bin` at `early_init` and publishes the lookup maps synchronously; it never discovers, converts, or measures. |
| F11 | The DLL validates each loaded record read-only on a thread — the three artefacts exist, and the simfile/audio/art stamps match the record's fingerprint — and the `startup.arc` fingerprint waits for validation (safety cap 15 s). Records that fail, and records not validated by the cap, are dropped with one summary WARN naming the tool. |
| F12 | With no (or an unreadable) `index.bin`, the DLL injects nothing and logs one WARN naming the tool. The mod's init log states "no index — run `ddr_world_hook_prep.exe`". |
| F13 | The DLL's runtime behaviour for injected songs — resolver, progressive full bank, preview/jacket prefetch scheduler, thumbnail arc, score taint, scroll gimmicks, visibility guards, fast-bootup quarantine — is unchanged. |
| F14 | The judgement-offset delta for a measured song is applied at play exactly as today (ledger → live map → freeze rule → provider). A song without a record plays as authored; one INFO per boot states how many injected songs have no record. |
| F15 | `index_timeout_s` is removed from the config schema (parsed and ignored as a retired key). |
| F16 | The tool ships in the release zip and as a bare exe, built for `x86_64-win7-windows-msvc`; README documents the `gamestart.bat` line and the manual workflow. |

### 2.2 Non-functional

| ID | Requirement |
|---|---|
| N1 | DLL boot with a valid cache: load ≤ 50 ms for 15,000 records; validation ≤ 200 ms per 1,000 songs on local disk; no step of `lib.rs` init before LayeredFS install grows by more than the synchronous load. |
| N2 | Tool cold run: ≈ 25 ms/song aggregate on 6 workers (unchanged pipeline); measurement ≈ 0.8 s/song CPU, parallel. |
| N3 | Every per-song step in the tool is panic-contained; one bad file costs one rejection. Every write is atomic (`.tmp` + rename). |
| N4 | The tool never blocks `gamestart.bat` from starting the game except for the work it was asked to do: unexpected failures are logged and exit 0. |
| N5 | No path specific to a machine is written into any artefact or log line that ships. |

### 2.3 Assumptions

- Testers' existing `index.bin` (FORMAT_VERSION 5, DLL-written) is reused by the tool as a warm cache; the format does not change.
- Files the DLL no longer uses but the tool mounts (`identity.rs`, the conversion half of `convert.rs`) stay declared in the DLL's module tree for `cargo check` coverage.
- Stock-song sync measurement is a separate feature.

## 3. Architecture Overview

```mermaid
flowchart LR
  subgraph User
    GS[gamestart.bat]
  end
  subgraph Tool["ddr_world_hook_prep.exe (prep/ crate)"]
    CLI[cli + gamedir + console + log]
    IDX[indexer: discover → identity → warm/cold → convert → checkpoint]
    MEAS[measure: eligible songs → decode → core::sync → sync.bin]
    REP[report: rejects, ceiling, summary]
    DRY[dry-run]
    MNT["mounted from src/: core::{sm,ssq,audio,sync,xact,art}, services::stepmania::{cache,ceiling,convert,identity,model,musicdb,sync_state}"]
  end
  subgraph Cache["data_mods/_cache/stepmania/"]
    IB[index.bin]
    ART[ssq/ xsb/ tn/]
    SB[sync.bin]
    RT[xwb/ jacket/ lru.bin thumbnail/]
  end
  subgraph DLL["ddr_world_hook.dll"]
    LD[index.rs: load → validate (thread) → publish]
    REG[ensure_registration + thumbnail arc]
    TR[startup.arc fingerprint + musicdb transform]
    PROV[providers / scheduler / progressive]
    SA[sync_apply: ledger → live map → provider]
  end
  GS -->|1. runs| CLI --> IDX --> MEAS --> REP
  IDX -->|writes| IB & ART
  MEAS -->|writes| SB
  IDX -. writes audio_failed .-> IB
  GS -->|2. starts spice| DLL
  IB -->|read| LD --> REG --> TR
  ART -->|read| PROV
  SB -->|read| SA
  PROV -->|writes| RT
  REG -->|writes| RT
```

Revised boot timeline inside the game (the parts that changed are marked ★):

```text
init thread                                   game thread (gamemdx onBoot)
0   config::init
0'  stepmania::early_init
    ├ root + README, settings, cache layout, LRU
    ├ ★ index::load (index.bin → lookup maps, ms)
    ├ ★ index::validate_async (read-only stat thread)
    ├ register resolver (mod_paths)
    └ register startup.arc musicdb transform + fingerprint
0b  avs_layeredfs::init (hooks live)
1-2 gamemdx wait, resolve_all …               avs_fs_open arc/startup.arc
                                                pre-repack: arena patch
                                                fingerprint cb: ★ wait_validated(≤ 15 s)
                                                                injectable() (ceiling)
                                                                ensure_registration + thumbnail arc
                                                transform cb:   musicdb::transform (inject)
7   mods register (StepmaniaPacksMod::init → ensure_registration; init_runtime)
8   mods enable (★ sync_apply::init only — no sweep thread)
```

## 4. Components and Interfaces

### 4.1 `prep/` crate

A native crate beside `updater/` with its own `[workspace]`, so it builds and tests on any
host without the DLL's Windows-only dependencies. Target for release:
`x86_64-win7-windows-msvc` with `trim-paths = "all"` (as the updater).

```text
prep/
├── Cargo.toml                 symphonia (mp3, vorbis, wav, pcm, ogg), rustfft, image (+texpresso),
│                              encoding_rs, serde + serde_json, windows-sys (process list)
├── src/
│   ├── main.rs                crate root: `log_info!/log_warn!/log_debug!` macro shims → console + log file;
│   │                          `pub mod core; pub mod services;` mounts; CLI dispatch
│   ├── cli.rs                 argument parsing; USAGE
│   ├── gamedir.rs             exe directory (or --game-dir), anchored on spice64.exe / ddr_world_hook.dll
│   ├── console.rs             pause-at-end when alone on the console (double-click)
│   ├── log.rs                 `ddr_world_hook_prep.log` in the game folder, overwritten per run
│   ├── running.rs             is spice64.exe / spice.exe running? (CreateToolhelp32Snapshot)
│   ├── config.rs              `mod-config.json` → { stepmania_packs_enabled, auto_sync }
│   ├── indexer.rs             MOVED from src/services/stepmania/index.rs: discover, identity::assign,
│   │                          warm reuse, convert_song, checkpointing, index.bin write, progress events
│   ├── measure.rs             MOVED from src/services/stepmania/sync_sweep.rs::measure_song (+ files_of),
│   │                          parallel pass, sync_state::decide_song → sync.bin, audio_failed marking
│   ├── report.rs              rejects, ceiling report, summary lines, duration estimate
│   ├── dry_run.rs             MOVED from tools/sm_pack_check/src/main.rs (minus its own convert_song: it
│   │                          now calls indexer::convert_song into --out)
│   ├── core/mod.rs            `#[path]` mounts of src/core/{audio,sm,ssq,sync,xact}; art mirror as sm_pack_check had
│   └── services/stepmania/mod.rs  `#[path]` mounts of cache, ceiling, convert, identity, model, musicdb, sync_state
└── tests/                     indexer_e2e.rs, measure_e2e.rs (synthetic corpus in temp dirs)
```

Mounted files keep their purity contract (imports only `crate::core::*`, siblings via
`super::`, and the crate-root log macros). `cache.rs` becomes mountable by that rule
already (its only non-`std` imports are `super::model::LruTable` and the log macros);
`indexer.rs` carries the two things `index.rs` had that were runtime-only — the
`on_index_complete` callback and the process-wide publish maps — as a `Sink` trait the
tool implements for progress output.

#### CLI

```text
ddr_world_hook_prep.exe [--game-dir <dir>] [--no-sync] [--workers <n>] [--verbose]
ddr_world_hook_prep.exe --dry-run <song-or-pack dir> --out <dir> [--code <code>] [--lead-in auto|0|<ms>]
                        [--max-audio-mb <n>] [--auto-sync]
ddr_world_hook_prep.exe --help
```

Default run, in order:

1. Resolve the game folder (§4.1 `gamedir`); not recognised ⇒ message, exit 3.
2. `running::game_is_running()` ⇒ message, exit 2.
3. Read `mod-config.json`. Mod disabled ⇒ "StepMania Packs is disabled in mod-config.json — nothing to do", exit 0.
4. `cache::ensure_layout`, `cache::sweep_tmp_files`.
5. `indexer::run(root, workers, sink)` — prints one line per pack (`[ 12/ 87] Stamina RPG 5 — 134 songs: 130 reused, 2 converted, 2 rejected`), rejects with reasons as they land, checkpoints, final `index.bin`.
6. `report::ceiling(entries)` — `ceiling::select` with the expanded arena assumed.
7. Unless `--no-sync` / `auto_sync: false`: `measure::run(eligible, workers, sink)` — eligible = ceiling-kept ∧ no valid `sync.bin` record ∧ ¬`audio_failed`; prints the estimate (`N songs × ~0.8 s / workers`), one line per song at `--verbose`, a summary (`measured / refused / failed`), writes `sync.bin` (atomic, once at the end plus every 250 songs).
8. Summary; pause-at-end if alone on the console; exit 0.

Exit codes: 0 success or nothing to do; 2 game running; 3 game folder not recognised. Any
other failure is logged and exits 0 (the game must still start from `gamestart.bat`).

### 4.2 `indexer.rs` (tool)

The cold/warm coordinator moved verbatim from the DLL's `index.rs` with these changes:

- `run(settings, sink) -> IndexOutcome` is synchronous (the caller owns the thread).
- Progress and logging go through `trait Sink { fn pack_done(..); fn song_rejected(..); fn song_converted(..); }` plus the crate-root log macros; the DLL's `publish_entry` maps and `STATE`/`READY` are gone.
- **Checkpoint**: after every 250 converted songs or 15 s since the last write, the coordinator serialises `IndexFile { songs: reused ∪ converted-so-far, rejects: so-far }` to `index.bin` atomically. The file format is unchanged; a checkpoint is simply a valid `index.bin` that lacks the songs not yet converted — the next run's warm path reuses every record in it. Work is distributed through the existing `VecDeque` queue; checkpointing happens on the coordinator by draining a results channel, so workers never touch `index.bin`.
- `convert_song` is `pub` so `dry_run.rs` calls it (writing into `--out` instead of the cache: `convert_song` takes a `Paths` argument — `cache::*` for the real run, the scratch dir for the dry run).

### 4.3 `measure.rs` (tool)

`measure_song(entry) -> Result<SongMeasurement, &'static str>` moved from `sync_sweep.rs`
(re-parse simfile with `core::sm`, `decode_whole`, one onset envelope, `core::sync::measure`
per materialised file). `run(entries, workers, sink)`:

- loads the existing `sync.bin` (`sync_state::SyncTable::parse`), drops stale records (the
  existing rule: song gone or fingerprint changed);
- queues eligible songs; workers measure under `catch_unwind` (panic ⇒ `Refused{panicked}`);
- the coordinator reduces each song's files with `sync_state::decide_song`, inserts the
  record, writes `sync.bin` every 250 songs and at the end;
- a `decode_whole` failure marks the song `audio_failed` in the in-memory `IndexFile` and
  re-writes `index.bin` at the end of the pass.

### 4.4 DLL `src/services/stepmania/index.rs` (slim)

```rust
pub struct LoadReport { pub songs: usize, pub rejects: usize, pub source: LoadSource } // Loaded | Missing | Corrupt
pub fn load(root: &Path) -> LoadReport;            // early_init: parse index.bin, publish BY_BASENAME / BY_MCODE, SONGS
pub fn validate_async(root: &Path);                 // early_init: spawn "stepmania-validate" (read-only)
pub fn wait_validated(cap: Duration) -> IndexSnapshot; // startup fingerprint / ensure_registration
pub fn snapshot() -> IndexSnapshot;                 // validated set (or the loaded set before validation finished — callers use wait_validated)
pub fn entry_by_basename / entry_by_mcode / has_basename / has_mcode  // unchanged signatures
pub fn mark_audio_failed_session(basename: &str);   // in-memory only (providers)
pub fn is_audio_failed_session(basename: &str) -> bool;
pub fn read_capped(..); pub fn compose_art_for_entry(..); pub const MAX_AUDIO_BYTES  // unchanged
```

Validation (per record, on the thread): `song_dir = root/pack/folder`; simfile =
`convert::find_simfile(song_dir)`; `artefacts_present(entry)`; `warm_fingerprint(simfile,
entry) == entry.fingerprint`. The decision function is pure (`validation.rs`, harness-
mounted): `fn verdict(record: &SongEntry, probe: &dyn Fn(&Path) -> Option<(u64,u64)>,
artefacts: [bool; 3]) -> Verdict { Keep | Dropped(reason) }`. Failed records are removed
from the maps when validation completes; `wait_validated` returns the validated snapshot.
On the cap: records not yet reached are dropped too (WARN counts them separately:
"N not validated in time"). `IndexSnapshot` loses `complete`, `converted`, `reused`,
`elapsed_ms`; keeps `songs`, `packs`, `rejected`, gains `dropped: usize`.

### 4.5 DLL `src/services/stepmania/mod.rs`

- `early_init`: after settings — `cache::ensure_layout()`, `cache::sweep_tmp_files()`,
  `cache::init_lru(audio_cache_mb)` (moved here from `index::run`), `index::load(&root)`,
  `index::validate_async(&root)`; the INFO line becomes
  `StepMania: index.bin -- N song(s), R cached rejection(s) (validating)` or the F12 WARN.
- `wait_index` → `index::wait_validated(VALIDATION_CAP)`; the WARN for the cap replaces the
  timeout WARN. `TIMEOUT_WARNED`, `timeout()`, `on_index_complete` are removed.
- `on_gameplay_load`: the `request_on_decide` call goes.
- `start_sync_sweep` → `init_sync()`: `sync_apply::init()` only.
- `status()` returns the validated snapshot.

### 4.6 DLL `src/services/stepmania/sync_apply.rs` (slim)

Keeps: `init` (load `sync.bin`, seed `LiveMap`, register the delta provider with
`per_song_judgement_offsets`), `delta_for_play`, `delta_provider`, the freeze rule, the
one-per-boot INFO "N injected song(s) without a sync record play as authored" (computed
at the first `injected_entries()` publication — the `StepmaniaPacksMod::enable` guards
path). Removes: on-decide queue/thread, `hold_for_measurement`, `request_on_decide`,
`publish_song`, `refuse_song`, `sync_index`, `begin/end_measurement`, `on_decide_pending`,
`save_table` (the DLL never writes `sync.bin`). `sync_sweep.rs` is deleted.
`providers::serve_full_bank` drops the hold; `mark_audio_failed` call sites become
`mark_audio_failed_session`.

### 4.7 Config, docs, release

- `StepmaniaPacksConfig`: remove `index_timeout_s` (serde ignores unknown keys; record it
  under retired keys in the data-model notes).
- `songs_readme.txt`: rewritten around the tool (drop packs → run the tool or let
  `gamestart.bat` run it → start the game; what the ceiling means; where the log is).
- README: StepMania section + the `gamestart.bat` snippet gains the prep line between the
  updater and spice; build commands table gains `prep/`.
- `scripts/build_release_archive.sh` / `build_release_distribution.bat`: build the prep
  exe for the Win7 target, `assert_win7_safe`, stage it in the zip and as a bare exe.
  The updater's release manifest lists it like any other release-owned file.
- Delete `tools/sm_pack_check/` and `scripts/validate_sm_pack_check.sh`; add
  `scripts/validate_prep.sh` (`cargo test` in `prep/` + a smoke `--dry-run` on a generated
  song).

## 5. Data Models

### 5.1 `index.bin` (unchanged format, FORMAT_VERSION 5)

`IndexFile { songs: Vec<SongEntry>, rejects: Vec<RejectEntry> }`. New semantics only:
written exclusively by the tool (checkpoints are partial but valid files); `audio_failed`
is set by the tool's measurement pass (and still by the DLL *in memory* for the session).
The DLL reads `songs`; `rejects` are counted for the init log and otherwise ignored.

### 5.2 `sync.bin` (unchanged format, v2)

Written exclusively by the tool. The DLL loads it into the `LiveMap` at `init`.

### 5.3 Cache ownership

| path | writer | reader |
|---|---|---|
| `index.bin`, `ssq/`, `xsb/`, `tn/`, `sync.bin` | tool | DLL |
| `xwb/`, `jacket/`, `lru.bin`, `thumbnail/<S>.arc` + `.digest` | DLL | DLL |
| `*.tmp` (either side's in-flight writes) | writer | swept by both at start (`sweep_tmp_files`), which is safe because the tool refuses to run while the game runs |

### 5.4 Validation verdict (DLL, in memory)

```rust
enum Verdict { Keep, Dropped(DropCause) }
enum DropCause { ArtefactMissing, SourceMissing, SourceChanged }
```

Aggregated into one WARN: `StepMania: N song(s) dropped -- changed or missing since the
last prep run (a artefacts, s sources, c changed)[, u not validated within 15 s] -- run
ddr_world_hook_prep.exe`.

### 5.5 Tool config view

```rust
struct ConfigView { stepmania_packs_enabled: bool, auto_sync: bool }
```
Derived from `mod-config.json` → `mods` (toggle list; same semantics as
`mod_enabled_in_config`) and `stepmania_packs.auto_sync` (default `true`). Missing file
or section ⇒ defaults (enabled, auto_sync on) — the DLL behaves the same.

## 6. Error Handling

| Situation | Tool | DLL |
|---|---|---|
| No `index.bin` | first run: cold index | WARN F12; nothing injected; mod stays active (`is_active` = root exists) |
| Corrupt `index.bin` | `parse_index` fails ⇒ treated as cold | WARN "index.bin unreadable — run the tool"; nothing injected |
| Song folder removed since last run | artefacts deleted, record dropped | validation drops it (SourceMissing) |
| Simfile/audio/art edited | re-converted | validation drops it (SourceChanged) until the tool runs |
| Artefact deleted from `_cache` | re-converted | validation drops it (ArtefactMissing) |
| Validation exceeds 15 s | — | remaining records dropped, counted in the WARN; the game boots |
| Game running | exit 2, nothing written | — |
| Game folder not recognised | exit 3 | — |
| Per-song panic (parse/convert/measure) | one rejection / `Refused{panicked}` | — |
| Ctrl+C mid-run | checkpointed records survive; `.tmp` swept next run | — |
| Bank decode fails at play | — | silence + WARN naming the song; `audio_failed` for the session only |
| Unexpected tool failure | logged, exit 0 | — |

## 7. Testing Strategy

- **`prep/` `cargo test`** (native, any host): the mounted modules' own suites; `indexer`
  end-to-end on a synthetic corpus in a temp game folder (cold run → every artefact + a
  record; warm run → no writes, same `index.bin` bytes; edit a simfile → that song only
  re-converted; delete a song → artefacts removed; interrupt simulation via a `Sink` that
  aborts after N songs → checkpoint file is valid and the next run reuses it); `measure`
  on the synthetic click-track fixture the stepmania harness already uses; `report` text
  for the ceiling; `config` parsing of the real `mod-config.json` shape; `dry_run`
  byte-identity of `--out` artefacts against a direct `convert_song` call.
- **`scripts/validate_stepmania.sh`**: gains the DLL's pure `validation.rs` (verdicts for
  each drop cause, keep when all match). Existing legs unchanged.
- **`scripts/validate_prep.sh`**: replaces `validate_sm_pack_check.sh`.
- **Cabinet** (the only validation for engine-facing code): (1) boot with packs and no
  `index.bin` → F12 WARN, no STEPMANIA folder; (2) run the tool, boot → songs load, init
  log shows the load/validate line, no index thread names in the log; (3) edit a simfile,
  boot → that song dropped with the WARN, run the tool → back; (4) play a measured song →
  the judge line shows the delta; (5) `--no-sync` library → "play as authored" INFO, no
  hold at stage load; (6) a 14k synthetic library → ceiling WARN, boot completes.

## Appendix A — Alternatives considered

- **Pre-warmer tool, DLL keeps indexing** (the first proposal). Rejected by the maintainer:
  two writers remain, the in-game timeout path remains, and the DLL keeps every line of
  the pipeline.
- **Trust `index.bin` without validation.** Zero threads at boot, but a deleted song lists
  and plays silence and an edited chart plays stale until the tool runs. Validation is
  the same stat walk the warm path already did, read-only, and `gamestart.bat` integration
  makes it a no-op in practice.
- **Keep on-decide measurement in the DLL.** Bounded, but it is a normal-priority thread
  during play and a stage-load hold; the tool measures by default, so the fallback buys
  little. Removed for simplicity; `auto_sync: false` semantics already cover "as authored".
- **Prep as an updater subcommand.** One exe fewer, but the updater is release-critical and
  self-updating; adding symphonia/rustfft/image to it is the wrong trade.
