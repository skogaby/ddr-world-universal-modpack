# Implementation Plan — StepMania Prep Tool

Status: Approved 2026-10-24

Design: `design/detailed-design.md`. Register: `idea-honing.md`.

- [ ] Step 1: `prep/` crate skeleton with the dry-run port (sm_pack_check retired)
- [ ] Step 2: Indexer moves into the tool; the tool produces the DLL's warm cache
- [ ] Step 3: DLL becomes a reader — load, validate, publish; index thread and timeout removed
- [ ] Step 4: Sync measurement moves into the tool; DLL sweep and on-decide removed
- [ ] Step 5: Release plumbing and documentation

Sequencing rationale: the tool must exist and produce a trusted cache (Steps 1–2) before
the DLL stops producing its own (Step 3), so the maintainer's cabinet keeps booting songs
at every step. The mounting refactor (Step 2) is the step most likely to surface purity
problems and goes first among the risky ones.

---

## Step 1: `prep/` crate skeleton with the dry-run port

**Objective.** A native crate that builds on the host and on `x86_64-win7-windows-msvc`,
mounts the DLL's pure modules, resolves the game folder, logs, pauses when double-clicked,
and already does something useful: the `sm_pack_check` dry-run, so that tool and its
validation script can be deleted.

**Guidance.**
- `prep/Cargo.toml` with its own `[workspace]`, dependencies per design §4.1, `trim-paths`
  and release profile copied from `updater/Cargo.toml`.
- `src/main.rs`: crate-root `log_info!`/`log_warn!`/`log_debug!` macros (console + log
  file), `pub mod core; pub mod services;` mounts (`#[path]` lines as in
  `tools/sm_pack_check/src/{core,services}` plus `cache.rs`, `ceiling.rs`, `sync_state.rs`),
  CLI dispatch.
- `gamedir.rs`, `console.rs`, `log.rs`: adapt from `updater/src/` (anchors
  `spice64.exe`/`ddr_world_hook.dll`, `--game-dir`, pause-at-end, `ddr_world_hook_prep.log`).
- `running.rs`: `game_is_running()` via `CreateToolhelp32Snapshot` on Windows; `false` on
  other hosts.
- `config.rs`: `ConfigView` from `mod-config.json` (design §5.5).
- `dry_run.rs`: move `tools/sm_pack_check/src/main.rs` logic; keep its own `convert_song`
  for now (Step 2 replaces it with the shared one).
- Delete `tools/sm_pack_check/` and `scripts/validate_sm_pack_check.sh`; add
  `scripts/validate_prep.sh` (`cargo test` in `prep/` + a smoke `--dry-run`).
- Update AGENTS.md's directory map / build table lines that name `sm_pack_check`.

**Tests.** `cargo test` in `prep/` runs the mounted suites; unit tests for `gamedir`
(anchor detection, `--game-dir` override, rejection), `config` (real file shape, missing
file, disabled toggle), `cli` parsing; `validate_prep.sh` green.

**Integration.** None with the DLL yet.

**Demo.** `ddr_world_hook_prep.exe --dry-run <pack> --out /tmp/x` prints the per-song
verdicts the old tool did; `ddr_world_hook_prep.exe` from a non-game folder exits 3 with a
message; from the game folder with the mod disabled prints "nothing to do".

---

## Step 2: Indexer moves into the tool; the tool produces the DLL's warm cache

**Objective.** The tool runs the DLL's real index pipeline into
`data_mods/_cache/stepmania/` with progress, checkpoints and the ceiling report. The DLL
is untouched in this step; after a tool run its own index thread finds everything warm.

**Guidance.**
- `prep/src/indexer.rs` = `src/services/stepmania/index.rs` moved (not copied) and
  reshaped per design §4.2: synchronous `run`, `Sink` trait, no process-wide maps,
  checkpoint every 250 converted songs / 15 s on the coordinator, `pub convert_song` with a
  `Paths` argument. Leave the DLL's `index.rs` as it is for this step (the two files
  diverge for one step only; Step 3 deletes the DLL's conversion half).
- `report.rs`: pack lines, reject lines (same wording as the DLL's WARNs), ceiling report
  using `ceiling::select`. The expanded arena size the report assumes must be the one the
  DLL patches: move `EXPANDED_ARENA_BYTES` into the mountable `ceiling.rs` (next to
  `STOCK_ARENA_BYTES`) and have `src/mods/song_limit_expansion.rs` import it from there, so
  tool and DLL share one definition.
- `dry_run.rs` switches to `indexer::convert_song` into `--out`.
- Refuse-while-running check (`running.rs`) gates the default run.

**Tests.** `prep/tests/indexer_e2e.rs` on a synthetic corpus (generate `.ssc` + WAV + PNG
with the helpers the stepmania harness uses): cold → artefacts + `index.bin`; warm → no
writes (compare directory mtimes and `index.bin` bytes); simfile edit → one re-conversion;
song removed → artefacts gone; abort-after-N `Sink` → checkpoint parses and the next run
reports N reused. `report` unit tests for the ceiling text. `--dry-run` byte-identity with
a direct `convert_song`.

**Integration.** The tool writes exactly the files the DLL's `warm_fingerprint` +
`artefacts_present` accept; verified on the cabinet by the DLL's own log
(`index complete -- N songs … (warm; 0 converted, N reused)`).

**Demo.** Run the tool on the maintainer's `stepmania/Songs/` → progress per pack, ceiling
report, `index.bin`; boot the game → the DLL logs a warm index in well under the 30 s wait.

---

## Step 3: DLL becomes a reader — load, validate, publish; index thread and timeout removed

**Objective.** The DLL never converts again. `index.bin` is loaded at `early_init`,
validated read-only on a thread, and the `startup.arc` fingerprint waits for validation.

**Guidance.**
- `src/services/stepmania/index.rs` → design §4.4 API. Remove `spawn`, `run`, `discover`,
  `convert_song`, `wait_ready`, `mark_audio_failed`, the `Condvar`; add `load`,
  `validate_async`, `wait_validated`, `mark_audio_failed_session`. Keep `warm_fingerprint`,
  `artefacts_present`, `song_fingerprint`, `read_capped`, `compose_art_for_entry`.
- New pure `src/services/stepmania/validation.rs` (`verdict`, `DropCause`, aggregation
  text); mount it in `scripts/validate_stepmania.sh`.
- `mod.rs` per design §4.5 (`early_init` owns cache layout/LRU; `wait_validated` with
  `VALIDATION_CAP = 15 s`; remove `on_index_complete`, `TIMEOUT_WARNED`; F12 WARN).
- `providers.rs`: `mark_audio_failed` → `mark_audio_failed_session` (6 sites); the
  resolver's `serving()` gate is set at load.
- `fast_bootup/preflight.rs` doc comment: the "index has finished" assumption is now
  structural.
- `config.rs`: drop `index_timeout_s`; `data_models` retired-keys note (summary refresh at
  Step 5). `StepmaniaPacksMod::init` log text for the no-index case.
- Readiness gate: `cargo check`, `cargo fmt`, `./build.sh`, stepmania harness.

**Tests.** `validation.rs` suite (keep; each drop cause; aggregation wording). Harness green.
Cabinet: boot with packs and no `index.bin` → F12 WARN and no STEPMANIA folder; run the
tool, boot → songs; edit one simfile, boot → that song dropped with the aggregated WARN,
run the tool → back; log shows no `stepmania-index` thread activity.

**Integration.** Uses the cache Step 2's tool writes; the `startup.arc` path, ceiling and
transform are unchanged.

**Demo.** Two boots from the cabinet log: one without an index (WARN, nothing injected),
one with (load line, validated count, songs on the wheel) — no 30 s stall in either.

---

## Step 4: Sync measurement moves into the tool; DLL sweep and on-decide removed

**Objective.** The tool measures eligible songs in parallel and writes `sync.bin`; the DLL
only applies.

**Guidance.**
- `prep/src/measure.rs` = `measure_song` + `files_of` moved from `sync_sweep.rs`, plus the
  parallel pass and `sync.bin` writes per design §4.3; `audio_failed` marking into
  `index.bin` at the end of the pass. Eligibility uses `ceiling::select` (the same kept set
  the report showed) so the tool never measures songs the game will not inject.
- Delete `src/services/stepmania/sync_sweep.rs`; slim `sync_apply.rs` per design §4.6;
  `providers::serve_full_bank` drops the hold; `mod.rs::on_gameplay_load` drops
  `request_on_decide`; `start_sync_sweep` → `init_sync`; the "N without a record play as
  authored" INFO.
- Tool flags: `--no-sync`, `--workers`; the estimate line.

**Tests.** `prep/tests/measure_e2e.rs`: synthetic click-track fixture → `Measured{d}` with
d within ±0.5 ms of the planted offset; refusal cases from the harness reused; a corrupt
audio file → `audio_failed` in `index.bin`; stale records dropped; second run measures
nothing. DLL: the judgement_offsets harness stays green (compose/freeze unchanged).
Cabinet: play a measured song → `P1 override applied (d ms = stock s + d)`; a `--no-sync`
library → the INFO and no hold at stage load.

**Integration.** `sync.bin` v2 unchanged; the DLL's `sync_apply::init` loads it exactly as
before.

**Demo.** Tool run shows the measurement estimate, progress and summary; the in-game judge
line applies the delta on the first play with no stage-load hold.

---

## Step 5: Release plumbing and documentation

**Objective.** The exe ships, the docs describe the new workflow, the knowledge base
reflects the architecture.

**Guidance.**
- `scripts/build_release_archive.sh` and `scripts/build_release_distribution.bat`: build
  `prep/` for the Win7 target, `assert_win7_safe`, stage `ddr_world_hook_prep.exe` in the
  zip and as a bare exe; the tester `.bat` mentions it.
- README: StepMania section rewritten around the tool; `gamestart.bat` snippet
  (updater → prep → spice); build table; config table loses `index_timeout_s`.
- `src/services/stepmania/songs_readme.txt` rewritten.
- `.agents/learnings/learnings.md`: replace the "cold index ≈ 25 ms/song / timeout" trap
  with the ownership rule (tool writes index/ssq/xsb/tn/sync; DLL writes xwb/jacket/lru/
  thumbnail; never cross) and the "tool must be the DLL's code, mounted" rule.
- `.agents/summary/` refresh (codebase-summary workflow) for components, data models
  (retired key), workflows.
- Loader feature `progress.md`: note the hand-over to this feature.

**Tests.** `./scripts/build_release_archive.sh` produces a zip containing the exe; the exe
runs under the Win7 compatibility assertion; README snippet copy-pasted into a scratch
`gamestart.bat` runs updater → prep → spice in order on the cabinet.

**Integration.** Final; nothing orphaned (`tools/sm_pack_check` gone in Step 1, DLL
conversion gone in Step 3, sweep gone in Step 4).

**Demo.** A fresh tester folder: unzip the release, add the three lines to `gamestart.bat`,
drop a pack, launch — the console shows the prep tool index the pack, then the game boots
with the STEPMANIA folder present.
