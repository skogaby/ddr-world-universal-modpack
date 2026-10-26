# Progress: StepMania Prep Tool

Updated: 2026-10-26
Status: ALL 5 STEPS DONE (2026-10-25); cabinet matrix PASSED 2026-10-26 (deploys #1–#4) — uncommitted

NEXT ACTION: Maintainer: review + commit. Optional: the 14k synthetic-library ceiling run
(design §7 item 6) and the tester release note ("run the tool once after updating").

Resume protocol: `implementation/plan.md` (checklist) → `design/detailed-design.md` →
`idea-honing.md`. Checkpoint commit before this work: `d5e019bc`.

## Done

- 2026-10-24 — PDD complete (register, design, plan approved); checkpoint commit `d5e019bc`.
- 2026-10-24 — **Step 1**: `prep/` crate (own `[workspace]`, Win7-ready manifest like
  `updater/`): `main.rs` (log-macro shims at the crate root, mounts, dispatch, exit codes
  0/2/3), `cli.rs`, `gamedir.rs` (anchors `spice64.exe`/`ddr_world_hook.dll`), `console.rs`
  (from updater), `log.rs` (+ DEBUG level, console only with `--verbose`), `running.rs`
  (Toolhelp snapshot for `spice64.exe`/`spice.exe`), `config.rs` (`mods` toggles +
  `stepmania_packs.auto_sync`), `dry_run.rs` = `tools/sm_pack_check/src/main.rs` moved
  (`git mv`; own `convert_song` kept until Step 2). Mounts: `core::{audio,sm,ssq,sync,xact}`,
  `core::art::source`, `services::stepmania::{ceiling,convert,identity,model,musicdb,
  sync_state}`. Deleted `tools/sm_pack_check/`, `scripts/validate_sm_pack_check.sh`; added
  `scripts/validate_prep.sh` (cargo test 273 incl. mounted suites, smoke `--dry-run`, arena
  constant parity leg — SKIP until Step 2). `cargo check --target x86_64-pc-windows-msvc`
  clean for the crate; `cargo fmt` clean. AGENTS.md directory map + build table updated;
  `src/` doc comments and `songs_readme.txt` no longer point at `sm_pack_check`.

- 2026-10-24 — **Step 2**: `prep/src/indexer.rs` = the DLL's `index.rs` coordinator moved and
  reshaped: synchronous `run(&Settings, &dyn Sink) -> Outcome`, `Sink` trait (discovered /
  pack_done / rejected / note / converted / checkpoint / should_abort), mpsc results channel,
  **checkpoint `index.bin` every 250 conversions / 15 s** (plain valid file, no new format),
  identical-bytes short-circuit (a warm run rewrites nothing), `pub convert_song(root, &Job,
  &ConvertOptions, &dyn ArtefactPaths) -> Converted` (no logging; the report-shaped
  intermediates come back), `CachePaths` / `FlatPaths`, `Job::for_dir`. Shared pure helpers
  moved to `src/services/stepmania/records.rs` (`song_fingerprint`, `warm_fingerprint`,
  `artefacts_present`, `folder_stamp`, `rel_to_root`) — the DLL's `index.rs` now imports them;
  `src/core/art/placeholder.rs` split out so the full art stack mounts. `EXPANDED_ARENA_BYTES`
  (+ `STOCK_`) live in `ceiling.rs`; `song_limit_expansion.rs` imports them. `prep/src/report.rs`
  (ConsoleSink, `index_summary`, `ceiling_report`); `dry_run.rs` now calls the shared
  `convert_song` and adds only the full bank + report. Crate split into lib + thin `main.rs`;
  `prep/tests/indexer_e2e.rs` (5 scenarios: cold/warm/edit/remove, cached rejections,
  abort → checkpoint → reuse, disabled mod / exit 3, Song Limit Expansion off). Test hooks
  `--abort-after` / `--checkpoint-every` (undocumented). Gates: prep `cargo test` 287 + 5 e2e,
  `validate_prep.sh` ALL PASS (leg 3 now checks the single arena definition),
  `validate_stepmania.sh` 287 tests / 102 legs (cache.rs + records.rs mounted; harness got
  log-macro shims), DLL `cargo check` + `./build.sh` clean, prep `cargo check` for
  x86_64-pc-windows-msvc clean. **Real install**: warm run over the DLL-written cache (138
  songs) = 0.03 s, nothing rewritten, `index.bin` byte-identical; cold run over ITG1 (72
  songs) 0.46 s on 6 workers — SSQ/XSB byte-identical to the DLL's cache (144/144), 29/72
  `tn/*.dds` differ by ONE BC1 block endpoint ±1 LSB (texpresso on the ARM host vs the x86
  DLL — float rounding, not pipeline drift; fingerprints are over sources so the DLL reuses
  them).

- 2026-10-24 — **Step 3**: the DLL is a reader. `src/services/stepmania/index.rs` 850 → 440
  lines: `load()` (parse `index.bin`, publish maps, `audio_failed` records stay out),
  `validate_async(root)` (thread `stepmania-validate`, pops a shared `pending` queue so an
  expired wait drops only the records not reached), `wait_validated(cap)`, `snapshot`
  (`validated`/`packs`/`rejected`/`dropped`), lookups, `mark_audio_failed_session` /
  `is_audio_failed_session` (in memory only; `serve_full_bank` and scheduler jobs
  short-circuit to silence). New pure `validation.rs` (`Probe` → `Verdict`, `DropTally`
  Display = the one WARN naming `ddr_world_hook_prep.exe`), harness-mounted. `mod.rs`:
  `early_init` owns cache layout / tmp sweep / LRU, F12 WARNs for missing / corrupt
  `index.bin`, `PREP_TOOL`, `VALIDATION_CAP = 15 s`; `wait_index` → `wait_validated`;
  `on_index_complete`, `TIMEOUT_WARNED`, `timeout()` gone; module doc rewritten (boot
  timeline, cache ownership). `config.rs`: `index_timeout_s` removed (serde ignores the
  key). `StepmaniaPacksMod::init` log: "no index -- run … " / "index validated -- …".
  `providers.rs` / `scheduler.rs`: `mark_audio_failed` → `_session`. `sync_sweep.rs` still
  compiles against `wait_validated` (deleted in Step 4). README config row updated;
  `lib.rs` step-0' comment updated. Gates: `cargo check` + `./build.sh` clean, stepmania
  harness 289 tests / 102 legs.

- 2026-10-25 — **Step 4**: `prep/src/measure.rs` = `measure_song` + `files_of` moved from the
  DLL's `sync_sweep.rs`, plus a parallel `run(root, &mut IndexFile, kept, &Settings, &dyn Sink)`
  over the ceiling-kept songs without a current record (stale records dropped at load as the
  DLL did; `sync.bin` checkpointed every 250 songs; `decode_failed` → `audio_failed` in
  `index.bin` via `indexer::write_index_file`). `report.rs`: `MeasureConsole` (estimate line
  at ~0.8 s/song CPU ÷ workers, progress every 100), `measure_summary`, `human_duration`.
  `lib.rs::prep` wires `--no-sync` / `auto_sync: false` / interrupted-index skips.
  **DLL**: `sync_sweep.rs` deleted; `sync_apply.rs` 539 → 136 lines (ledger load, seed from
  the validated index, provider, freeze rule; the init INFO counts injected songs without a
  record and names the tool); `providers::serve_full_bank` drops the stage-load hold;
  `mod.rs::on_gameplay_load` drops `request_on_decide`; `start_sync_sweep` → `init_sync`;
  scheduler's 25→26 on-decide trigger removed; doc comments in `taint.rs`, `core/sync`,
  `config.rs` updated. `prep/tests/measure_e2e.rs`: a planted 20 ms click offset comes back as
  a 20 ms delta difference between two synthetic songs; second run measures nothing;
  `--no-sync` leaves `sync.bin` alone; `auto_sync: false` skips. Gates: prep 290 unit + 7 e2e,
  `validate_prep.sh` ALL PASS, stepmania harness 289/102, judgement_offsets harness OK, DLL
  `cargo check` + `./build.sh` clean, prep `cargo check` for x86_64-pc-windows-msvc clean.
  **Real data**: ITG1 (72 songs) measured in 7 s on 6 workers (51 measured / 21 refused —
  the in-game sweep's ratio) → a 4,000-song library ≈ 7 min.

- 2026-10-25 — **Step 5**: `scripts/build_release_archive.sh` + `build_release_distribution.bat`
  build `prep/` for `x86_64-win7-windows-msvc`, `assert_win7_safe` it, stage
  `ddr_world_hook_prep.exe` in the zip and as a bare exe (`PREP` var, summary lines). Verified:
  `cargo xwin build --release --target x86_64-win7-windows-msvc -Z build-std` in `prep/`
  succeeds (57 s), 4.7 MB exe, no `ProcessPrng` import, no home path in the binary. README:
  file list, `gamestart.bat` snippet (updater → prep → spice), StepMania section rewritten
  around the tool (ceiling, timings, `--help`/`--no-sync`/`--dry-run`, Song Limit Expansion
  must stay on), mod table row, build table + crate paragraph. `songs_readme.txt` rewritten.
  `.agents/learnings/learnings.md`: the cold-index/timeout trap replaced by three rules (DLL
  never indexes + cache ownership; tool = the DLL's pipeline mounted, BC1 asterisk; ceiling is
  a constant). Loader `progress.md` hand-over note. `.agents/summary/` refresh delegated to a
  background subagent (codebase-summary workflow; targeted corrections of every
  `sm_pack_check` / `sync_sweep` / `index_timeout_s` / 8 MiB mention).

- 2026-10-25 — `.agents/summary/` refreshed (subagent, all nine files; its report is in the
  session). Its three stale-`//!` findings fixed (`providers.rs`, `scheduler.rs`,
  `stepmania_packs/mod.rs`) plus the feasibility doc's module map.

## In flight

- nothing. Feature complete pending maintainer commit + cabinet matrix.

## Deploy & test log

| # | date | build | result |
|---|---|---|---|
| 1 | 2026-10-26 | prep exe (Win7), maintainer's CrossOver install | **Tool PASS.** Warm run over the DLL-written cache: 138 reused / 0 written in 1.4 s (Wine stat walk). With `auto_sync: true`: 138 measured in 28 s on 6 workers → **102 measured / 36 refused — identical to the in-game sweep's result on the same packs** (loader run #15). Up-front duration estimate was 36 % off → replaced by elapsed + extrapolated remaining on the progress line (every 100 songs / ≥ 10 s). In-game test pending. |
| 2 | 2026-10-26 | Step 3+4 DLL (Win7) + prep exe, `_cache/stepmania` deleted first | **PASS.** Tool: cold 138 songs in 4.4 s, 102/36 sync in 27 s, 0 WARN. Game: `SongLimitExpansion: … 1 MiB → 64 MiB (6 patches)` applied (first cabinet confirmation), `registered series 22 / bit 31 for 138`, thumbnail arc 138 members 93 ms, `StepmaniaPacks: index validated -- 138 … 0 dropped by validation`, pre-flight 138/0 failing, `sync.bin holds 138 record(s); 138 of 138 live`, no StepMania WARN, no index/sweep/on-decide lines. Wheel: previews + jackets 150–260 ms each. Play (Anubis `z3agij`): bank served complete with no hold, `P1 override applied (-4 ms = stock 0 -4)` = the tool's `delta -4 ms`. Early-init lines predate the debughook (known). |
| 3 | 2026-10-26 | same DLL; one character changed in Anubis's `.sm`, no tool run | **PASS.** `StepMania: 1 song(s) dropped -- changed or missing since the last prep run (0 artefact(s) missing, 0 source(s) missing, 1 changed) -- run ddr_world_hook_prep.exe`; `index validated -- 137 kept, 1 dropped, 1473 ms` (validation lines reached log.txt this boot); registration / thumbnail arc / musicdb / sanitiser all 137; `sync.bin holds 138 record(s); 137 of 137 live` (stale record inert until the tool runs); `z3agij` absent from the rest of the log. |
| 4 | 2026-10-26 | same DLL; `index.bin` renamed away | **PASS.** `StepmaniaPacks: no index -- run ddr_world_hook_prep.exe before the game to load packs from ./stepmania/Songs`; no series/folder/thumbnail/injection lines; `0 song(s) in the music DB`; STEPMANIA folder absent in-game (maintainer); no WARN/ERROR, stock boot otherwise. Early-init WARN predates the debughook (known). **Design §7 cabinet matrix complete** except the 14k synthetic-library ceiling run. | Add to the matrix: (4) play a tool-measured song → `P1 override applied (d ms = stock s + d)` judge line, no stage-load hold; (5) `--no-sync` library → `N without a record play as authored` INFO at enable. Matrix (design §7): (1) packs + no `index.bin` → F12 WARN, no STEPMANIA folder; (2) after a tool run → songs load, `index validated -- N kept` line, no `stepmania-index` thread; (3) edit a simfile → that song dropped with the tally WARN; tool run → back; also verifies the 64 MiB arena. |

## Deviations & open questions

- Step 2: BC1 thumbnail bytes are not bit-reproducible across host architectures (ARM tool vs
  x86 DLL) — one block endpoint ±1 LSB in ~40% of files. Irrelevant to the warm path (source
  fingerprints) and invisible; on a Windows cabinet both sides are x86. Note for anyone
  expecting "byte-for-byte" of `tn/`.

- `--dry-run --help` (Step 1): the dry-run USAGE prints via `dry_run::parse_args` returning
  `Err(String::new())` as the help sentinel — small but worth knowing when extending the parser.
- The `.agents/summary/*` references to `tools/sm_pack_check` are left for the Step 5 refresh.

## Key facts for a cold resume

- Tool mounts DLL source by `#[path]` exactly like `tools/sm_pack_check` did (`core/mod.rs`,
  `services/stepmania/mod.rs` mirrors). Purity rule: mounted files import only
  `crate::core::*`, siblings via `super::`, and the crate-root `log_*` macros.
- Ownership split (design §5.3): tool writes `index.bin`/`ssq`/`xsb`/`tn`/`sync.bin`; DLL
  writes `xwb`/`jacket`/`lru.bin`/`thumbnail`.
- DLL stays fully functional (own index thread) until Step 3.
