# Progress: StepMania Prep Tool

Updated: 2026-10-26 (night)
Status: ALL 5 STEPS DONE (2026-10-25); cabinet matrix PASSED 2026-10-26 (deploys #1–#4);
**14,138-song synthetic library: boots, plays (maintainer play-tested the wheel + a song), and a
warm boot replays 78,109/78,110 items in 134 ms (deploy #8l, 2026-10-26 19:37)** — uncommitted.

NEXT ACTION: maintainer: review + commit (confirmation boot of the final DLL done 2026-10-26
20:02 — same patch lines, `replay plan -- 78109/78110`, title at +22 s); `.agents/summary/*`
refresh in a dedicated session (codebase-summary workflow); optionally remove the synthetic
packs (`scripts/gen_stepmania_synthetic_library.py --out … --remove`, then run the tool).

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

- nothing. Deploy #8i DLL is what `$DDR_WORLD_INSTALL/ddr_world_hook.dll` holds; all of it
  uncommitted (maintainer commits).

## Deploy & test log

| # | date | build | result |
|---|---|---|---|
| 1 | 2026-10-26 | prep exe (Win7), maintainer's CrossOver install | **Tool PASS.** Warm run over the DLL-written cache: 138 reused / 0 written in 1.4 s (Wine stat walk). With `auto_sync: true`: 138 measured in 28 s on 6 workers → **102 measured / 36 refused — identical to the in-game sweep's result on the same packs** (loader run #15). Up-front duration estimate was 36 % off → replaced by elapsed + extrapolated remaining on the progress line (every 100 songs / ≥ 10 s). In-game test pending. |
| 2 | 2026-10-26 | Step 3+4 DLL (Win7) + prep exe, `_cache/stepmania` deleted first | **PASS.** Tool: cold 138 songs in 4.4 s, 102/36 sync in 27 s, 0 WARN. Game: `SongLimitExpansion: … 1 MiB → 64 MiB (6 patches)` applied (first cabinet confirmation), `registered series 22 / bit 31 for 138`, thumbnail arc 138 members 93 ms, `StepmaniaPacks: index validated -- 138 … 0 dropped by validation`, pre-flight 138/0 failing, `sync.bin holds 138 record(s); 138 of 138 live`, no StepMania WARN, no index/sweep/on-decide lines. Wheel: previews + jackets 150–260 ms each. Play (Anubis `z3agij`): bank served complete with no hold, `P1 override applied (-4 ms = stock 0 -4)` = the tool's `delta -4 ms`. Early-init lines predate the debughook (known). |
| 3 | 2026-10-26 | same DLL; one character changed in Anubis's `.sm`, no tool run | **PASS.** `StepMania: 1 song(s) dropped -- changed or missing since the last prep run (0 artefact(s) missing, 0 source(s) missing, 1 changed) -- run ddr_world_hook_prep.exe`; `index validated -- 137 kept, 1 dropped, 1473 ms` (validation lines reached log.txt this boot); registration / thumbnail arc / musicdb / sanitiser all 137; `sync.bin holds 138 record(s); 137 of 137 live` (stale record inert until the tool runs); `z3agij` absent from the rest of the log. |
| 4 | 2026-10-26 | same DLL; `index.bin` renamed away | **PASS.** `StepmaniaPacks: no index -- run ddr_world_hook_prep.exe before the game to load packs from ./stepmania/Songs`; no series/folder/thumbnail/injection lines; `0 song(s) in the music DB`; STEPMANIA folder absent in-game (maintainer); no WARN/ERROR, stock boot otherwise. Early-init WARN predates the debughook (known). **Design §7 cabinet matrix complete** except the 14k synthetic-library ceiling run. | Add to the matrix: (4) play a tool-measured song → `P1 override applied (d ms = stock s + d)` judge line, no stage-load hold; (5) `--no-sync` library → `N without a record play as authored` INFO at enable. Matrix (design §7): (1) packs + no `index.bin` → F12 WARN, no STEPMANIA folder; (2) after a tool run → songs load, `index validated -- N kept` line, no `stepmania-index` thread; (3) edit a simfile → that song dropped with the tally WARN; tool run → back; also verifies the 64 MiB arena. |
| 5 | 2026-10-26 | Step 3+4 DLL + 14,000 synthetic songs (`gen_stepmania_synthetic_library.py`), tool run `--no-sync` | **FAIL (stall).** fast-bootup's inline pre-flight re-read 13.7k SSQs on the game thread; 58 s of validation + 13.6 s thumbnail-arc build inside the `startup.arc` open before any window. Fixes: plan build trusts files the background pre-flight did not reach; non-blocking boot (inject the loaded set, validate on below-normal workers, hide changed songs at completion); tool-built `thumbnail/all.arc` + digest. |
| 6 | 2026-10-26 | non-blocking-boot DLL + 14k | **FAIL ("NOW LOADING 0%" forever).** Window appeared; the step-data record pool (4,096, no overflow guard — `FUN_1801febf0` writes record −1 into the heap header when the free list is empty) overflowed at ~14,138 + 1,500 files; `CheckStepDataActor::onInit`'s work vector came back NULL. Almost certainly the original tester's 14k stall. Fixes: pool patch 0x1000 → 0x20000 via the new `ident_override::on_ea3_boot` registry; `ceiling` caps by Σ files ≤ `record_pool − 2048`; fast-bootup lets an empty work list fall through to the game's own onUpdate. |
| 8a–8i | 2026-10-26 17:22–18:58 | deploy-8 DLLs (Win7), 14k, run from this machine (`scripts/game_nav/launch.sh`, then the `run_ddr` alias shape — see Deviations) | **Nine boots, one new hard limit per boot, ending in PASS.** (8a, `-k`) LayeredFS missed `startup.arc` (the launcher reaches `ea3_boot` moments after a `-k` load) → stock DB; also found the `MineTextureLoader` fix works (loads issued after the first frame). (8b, `-z`) **`W:prop: read error by mem[…:8021201](80092143)`** = `AVS_ERROR_PROP_NODE_OVER_65535`: the music-DB property tree is capped at 65,535 nodes without `PROPERTY_MODE_EXTEND`; the DB came back EMPTY, onInit built no work list, the stock onUpdate read it through NULL (`gamemdx+0x32D05`). Fix: mode `0x11 → 0x1011` at the three `property_mem_read` sites (derived from the READ sites); `ceiling` node budget. (8c) LayeredFS lost the race AGAIN with `-z`: its `_cache` index walk (43k files, 42.5k of them the StepMania cache) took ~1 s under Wine. Fix: `ifs_textures::CACHE_INDEX_SKIP` += `stepmania`. (8d) Full DB injected, **boot pass 78,110 items in 49 s**, then AV at `gamemdx+0x343D` from the arc loader: `ArchiveFileCallback` parses the cue + name tables out of the FIRST 512 KiB chunk, no bounds check; the 14,138-member arc's header is 750 KB. Fix: chunk `0x80000 → 0x400000` + pool `0x200000 → 0x1000000` at `ea3_boot` (`ARC_CHUNK_PATTERN`); `ceiling` header budget. (8e–8h) AV on the step-data I/O worker (an AVS thread; libavs's `thread-task.c:48` handler re-raises it as a fatal C++ throw, so only `kernelbase+0xD8B7` was visible) — found by adding a first-chance vectored handler + `RtlCaptureStackBackTrace` to `crash_handler.rs`: `gamemdx+0x22F2EA` = `FUN_18022f220` (ResourceManager texture register) → `FUN_180237630(handle 0)` NULL: the **gs texture table (4,096 slots, `Renderer::initGs` config `0x1000`) was full** — every thumbnail is a resident texture. Fix: `0x1000 → 0x8000` (the handle's 15-bit slot field is the format limit) at `ea3_boot` (`TEXTURE_SLOTS_PATTERN`); `ceiling` texture budget (reserve 2,560 for stock). (8i) **PASS**: all six patches early (`ea3_boot`: pool, chunk, textures; `early_apply`: arena ×6, mode ×3), `injecting 14138 song(s)`, `boot pass complete -- processed 78110 items in 124952 ms` (Wine; 49 s in 8d before the thumbnail arc streamed concurrently), scenes NOW_LOADING → HARDWARE_CHECK → PRE_SPLASH ×3 → TITLE_SCREEN → ATTRACT_DEMO, `CustomResolution: OUTPUT set applied … 1920x1080`, `index validated -- 14138 kept … 82772 ms`, no WARN/ERROR from StepMania, empty crash log. Boot ≈ 2 min 50 s wall under Wine. |
| 8j–8l | 2026-10-26 19:07–19:37 | warm-boot work, 14k, maintainer's `run_ddr` | (8j) Second boot of 8i still ~2.5 min: `replay plan -- 0/78110 … verifier not ready` — the step-data cache verifier stat'd all 15,665 cached files single-threaded (two stats each, ~3–10 ms under Wine) and published 100 s AFTER the pass began, so the fully-captured cache was never used; also 15,626 `judgement_offsets: song identity … (ssq open)` INFO lines (one per SSQ the pass opened). Fixes: `Identity::Indexed { basename, fingerprint }` — injected SSQs are identified by their index record's source fingerprint, no stat (`cache::indexed_basename` handles split `_N` files); verifier on 4 workers, stock stat skipped when the override resolved; `fast_bootup::late_replan` amends the plan once if the verifier publishes mid-pass (`plan::late_replay_candidates`, host-tested); the `(ssq open)` line → DEBUG. (8k) First boot with it: `verifier ready … 1527 verified` at +7 s (before the pass), `replay plan -- 7420/78110` (all stock replayed), the 14,138 injected captured once: `boot pass complete -- processed 70690 items in 48278 ms`, `wrote step-data cache -- 15665 entries (14138 fresh)`, title at +70 s, validation 34 s. (8l) **PASS — warm boot at 14k is a stock fast-bootup boot**: `verifier ready -- 15665 cached file(s), 15665 verified` at +7 s, **`replay plan -- 78109/78110 items from cache, 15664 record(s) flipped`, `boot pass complete -- processed 1 items in 134 ms`**, title screen at +24 s (spice/ea3/scene boot — song-count independent). Maintainer: "more or less instant". The tool-side analysis reimplementation (option 1) is therefore NOT needed; the one-time capture pass after indexing new packs (~48 s per 14k under Wine) is the only per-song boot cost. |
| 7 | 2026-10-26 16:52 | pool-patch DLL (Win7) + 14k | **FAIL (silent hang, no window content; process alive at 236 % CPU).** Two findings. (a) Bookkeeping: `early_apply` ran BEFORE `ea3_boot` (scan < 1 s) and recorded `AppliedLate` → ceiling fell back to the stock pool → 2048 injected, arc rebuilt at boot (4.1 s), WARN — while the live manager actually had 131,072 records (read back from the process: `cap=0x20000`). (b) **Root cause of the hang, found with winedbg `bt all` + memory reads on the live process:** the game thread sat in `Application::onBoot`'s FIRST blocking drain (`FUN_1801fe040` ← `onBoot+0x3ba`) → pump status-4 branch → `PngFileCallback` (vtable `0x1802ddf50`, load `FUN_1801b0e70`) → `FUN_1801b0c40` → `FUN_1802392e0` spinning on `WaitForSingleObject(DAT_1806f30f8 = NULL, 10)` — the renderer's device mutex does not exist yet. Records 1–3 of the manager were `./data_mods/note_types_expansion/tex/note_types_mine00_{s,m,l}.png`: `NoteTypesExpansion::enable()` (init thread, 16:52:07) issued `FileManager::Load` while the game thread was inside the `startup.arc` open that our 4 s arc build + transform held until 16:52:10; the drain pumps every pending record, so our PNGs hit the PNG callback before the D3D device existed. Unrelated to the pool size and to 14k per se — a race the slow open exposed. Fixes: `texture_loader.rs` queues the loads to `widget_renderer::run_on_render_thread` (first frame dispatch ⇒ device exists); `apply_record_pool_patch` derives `early` from `ident_override::{is_installed, boot_observed}` under its lock and upgrades `AppliedLate → AppliedEarly` when the detour's callback confirms the boot had not run. |

## Deviations & open questions

- 2026-10-26 (night) — **Remaining large-library work: DONE** except the summary refresh.
  (2) → `src/core/song_limit_sites.rs` (specs + `find`/`find_mode_site`, shared by the mod and
  `SignatureStore::derive_song_limit_sites`; the harness mounts it) — the sweep publishes 12
  `song_limit_*` sites on all 5 builds, ALL GREEN. (4) → `MAX_INJECTED_SONGS` 30,000 final (the
  texture table's 30,208 is the hard cliff; comments/learnings updated). (5) → README (StepMania
  section + Song Limit Expansion row), `songs_readme.txt`, feasibility doc §7.6 (the six limits
  table), learnings. `.agents/summary/*` left for a dedicated session per the maintainer.
  Original list kept below for the record.
- 2026-10-26 — **Remaining large-library work** (after deploys #8i/#8l): (1) ~~maintainer
  play-test at 14k~~ done 2026-10-26 (wheel shows all 14,138 + the 138 ITG songs, a song played;
  the thumbnail arc is 251 MB / 14k textures ≈ 260 MB VRAM); (2) the five boot-patch AOBs (`RECORD_POOL_PATTERN`, `ARC_CHUNK_PATTERN`,
  `TEXTURE_SLOTS_PATTERN`, the `MOV R8D,0x11` derivation, the arena pair) into `signatures.rs` +
  `validate_signatures.sh` — today only `scripts/aob_check.py` over `~/Desktop/ddr_modules`
  proved them unique in all five builds; (3) **boot pass at 14k = 125 s under Wine** (78,110
  items; 49 s when the thumbnail arc was not streaming alongside) — that is the number for the
  precomputed-analysis decision (tool-owned `step_data.bin` of fast-bootup `SlotPayload`s per
  injected SSQ; a native cabinet will be faster, measure there first); (4) set
  `MAX_INJECTED_SONGS` — the hard ceilings are now: texture table 32,768 − 2,560 reserve =
  30,208 songs (one thumbnail each; the handle's 15-bit slot field cannot go higher), arc header
  4 MiB ≈ 80k songs, record pool 131,072 files, property nodes unlimited, arena 64 MiB ≈ 79k —
  so 30,000 stands unless VRAM argues lower (boot time no longer does: warm boots replay from
  the step-data cache regardless of count); (5) docs with old numbers (README StepMania
  section, `songs_readme.txt`, learnings' "30,000 provisional", `.agents/summary/*` via the
  codebase-summary workflow; `docs/stepmania_runtime_packs_feasibility.md` should gain the six
  limits). Also: `scripts/game_nav/launch.sh` now passes `-z` (was `-k`); for plain boots the
  maintainer's `run_ddr` alias shape (no SpiceAPI polling) is the better launcher — a copy lives
  only in this session's temp dir, not in the repo.

- 2026-10-26 — **Step-data record pool** (see deploy #6): the fourth hard limit, found only by
  the synthetic 14k run. `MAX_INJECTED_SONGS` stays 30,000 provisional; with the pool at 0x20000
  it holds the ceiling at 3 files/song. Sweep of the pool pattern: scripted byte search over
  `~/Desktop/ddr_modules/gamemdx_*.dll` (1 hit each, same `[RBP+0x38/0x3C/0x30]`); not yet in
  `signatures.rs` / `validate_signatures.sh` (the arena patterns live in the mod too).

- 2026-10-26 — **Non-blocking boot + tool-built thumbnail arc** (after deploy #5 showed 58 s of
  validation + 13.6 s of arc build before the window at 14k). The `startup.arc` open now injects
  the LOADED set with no wait (`validation::wait_cap` removed); validation runs on below-normal
  workers and, at completion, the failed records leave the maps and `mod.rs::on_validation_
  complete` hides them via the corruption flag (`hide::hide_unservable`), parked in
  `VALIDATION_DROPS` until the visibility guards have run when validation finishes first.
  `DropTally` says "hidden", no `unvalidated`. The thumbnail arc is `thumbnail/all.arc` +
  `all.digest` (series-independent; `thumbnail_arc.rs`, pure, shared): the tool builds it for
  `ceiling::select_loadable` (= the DLL's loaded set, `audio_failed` excluded) after the
  measurement pass; the DLL's `build_thumbnail_arc` finds the digest and logs "built by the
  prep tool", building only as a fallback (WARN). `core/art/arcpack.rs` is the pure arc writer
  (`ArcArchive::to_bytes` delegates). Ownership table: `thumbnail/` is now TOOL-owned (DLL
  fallback-writes only when the digest is missing). Tool on the 14k install: arc 14138 members
  / 251 MB in 2.2 s native. Also (deploy #5 fix): fast_bootup's plan build TRUSTS injected
  files the background pre-flight did not reach.

- 2026-10-26 — **Ceiling is PROVISIONALLY 30,000** (was 4,000, my tested-headroom placeholder;
  the maintainer's target is tens of thousands). To be set from the synthetic 14k cabinet run.
  Validation now runs on 2..=6 worker threads and the `startup.arc` wait is
  `validation::wait_cap(records)` = 15 s + 5 ms/record (138 under Wine took 1.47 s on ONE
  thread — ~10 ms/stat — so a flat 15 s would have dropped everything past ~1,400 songs).
  Synthetic library generator: `scripts/gen_stepmania_synthetic_library.py` (`--remove` to
  clean up). 14k synthetic songs are IN the maintainer's install right now (140 `Synthetic
  Pack NNN` folders, 1.2 GB; cache `tn/` 276 MB, `ssq/` 57 MB, `xsb/` 55 MB, `index.bin` 2.7 MB);
  tool run: 14,000 converted in 37.8 s native, 0 rejected, `--no-sync`.

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
