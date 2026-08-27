# Implementation Plan: Song Playback Speed — Streaming Rate Engine

Status: Approved 2026-08-08

Date: 2026-08-08

Design: `.agents/planning/2026-08-08-song-rate-streaming/design/detailed-design.md`
(Approved 2026-08-08). Requirement numbers below refer to that document. Standing
gates for every step: `./scripts/validate_song_playback_speed.sh`,
`./scripts/validate_se_bank_synth.sh`, `cargo check --target x86_64-pc-windows-msvc`
(0 warnings), whole-crate `cargo fmt`, `./build.sh`. The maintainer commits personally
and runs all deployments; steps 1–4 and 6 are host-only (no deployment).

## Checklist

- [x] Step 1: Remove the retired cache model and land the identity-only base
- [x] Step 2: Build the streaming WSOLA core with byte-equality proof
- [x] Step 3: Build the virtual bank and the synthetic engine replay
- [x] Step 4: Wire the callback detours, binding, and generator into the transaction
- [x] Step 5: Live bring-up and throughput benchmark on the cabinet
- [x] Step 6: Integrate dependent features (Assist Tick, Real Speed, PUS)
- [x] Step 7: Hardening, documentation, and the release matrix

## Step 1: Remove the retired cache model and land the identity-only base

**Objective:** Delete the rejected machinery so every later diff builds on a clean
base that still compiles, validates, and plays literally stock (reqs 6, 25, 38).

**Implementation guidance:**

- Delete `src/services/song_rate/cache.rs`, `worker.rs`, and the cache data models in
  `model.rs`; delete `src/core/xact/transform.rs` after relocating its pure entry-plan
  and loop-mapping logic to a stub `src/core/xact/virtual_bank.rs` (filled in Step 3).
- Strip `conversion.rs` to the surviving pure pieces (`dance_bank_song_code`,
  song-digest helpers) — rename to `binding.rs` with preflight left `todo`-refusing
  (always EarlyFailed) for now.
- Remove the song-rate seams from `src/services/avs_layeredfs/file_hooks.rs`
  (`song_rate_open_redirect`, the generated-path conversion seam) and the
  coordinator/cache statics, `OPEN_REDIRECT` cache, and lease/tombstone drain from
  `runtime.rs`. Drop `cache_limit_gib` from the config structs.
- Rename lifecycle phase `Preparing` → `Binding`; delete `RedirectReady` and its
  transitions; trim `xact_runtime.rs` (`lease_id`, `ReleaseLease`) and
  `transaction.rs` (`TransactionParts` lease/cache fields).
- `integration_ready()` must now return false (binding preflight refuses), so the
  option row does not register — the safe intermediate state.
- Update `scripts/validate_song_playback_speed.sh` in place: remove the `cache` and
  `on_demand` report sections and the deleted modules' tests; keep every Step-1
  synthetic audio section and all surviving lifecycle/clock/score/transaction tests.

**Tests:** The trimmed validator runs green with the surviving suites; a readiness
test asserts `integration_ready()` is false and no row registers; existing
lifecycle/transaction tests updated for the renamed phases pass.

**Integration with previous steps:** First step; establishes the base every later
step diffs against.

**Demo:** A release build boots (host-side reasoning: validator + check gates); the
validator report carries no cache/on-demand sections; the codebase contains no
reference to the cache directory, leases, deadlines, or admission.

## Step 2: Build the streaming WSOLA core with byte-equality proof

**Objective:** The resumable `StretchState` with the properties reqs 19–20 demand,
proven against the untouched whole-buffer reference.

**Implementation guidance:**

- Implement `StretchState` in `src/core/xact/stretch.rs` per the design's component
  spec (produce-run API, `SourcePcm` random-access decode view, checkpoint/restore,
  terminal-anchor final region). The existing `stretch_interleaved_with` is not
  modified — it is the test oracle.
- Add public `decode_block`/`encode_block` to `src/core/xact/adpcm.rs` (thin wrappers
  over the existing per-block internals) and a small block-cache `SourcePcm`
  implementation over a borrowed source bank.

**Tests:** Byte-equality vs the reference across rates 25/50/75/100/125/175, loop
contexts (none/interior/clamped), 1/2/6 channels, short/boundary inputs;
chunking-independence (arbitrary produce-call sizes yield identical bytes);
checkpoint-at-loop-start restore reproduces the identical suffix; identity shortcut
equality; per-block codec wrappers byte-match the whole-buffer codec.

**Integration with previous steps:** Pure addition on Step 1's base; no runtime code
touches it yet.

**Demo:** Validator's new `streaming` section (first legs) shows equality and
chunking-independence results across the rate matrix.

## Step 3: Build the virtual bank and the synthetic engine replay

**Objective:** The pure layout/header/mapping layer (reqs 12–14) and the host replay
harness that proves a virtual bank survives the engine's exact read pattern.

**Implementation guidance:**

- Fill `src/core/xact/virtual_bank.rs`: `plan_virtual_bank` (per-entry
  `target_for_percent` + half-up loop mapping with the one-frame clamp rule, 28-bit
  refusal), pre-data block synthesis reusing `xwb.rs`'s canonical streaming header
  emission, `resolve(offset, len)` region mapping with the EOF clamp.
- Build the synthetic engine replay in the validator harness: 0x1000 header read at
  offset 0, sequential 64 KiB block-align-rounded packets, EOF clamp, loop-restart
  jump — driving a `Binding`-shaped serving surface fed by Step 2's generator core
  (still host-side, no threads required: a pull-driven test pump).

**Tests:** Header bytes identical to `write_song_bank_streaming`'s pre-data emission
for the same entry values; `resolve` property tests against the serializer's physical
layout (pre-data/data/gap/EOF, both physical entry orders); 28-bit and loop-mapping
refusals; the replay reassembles a byte-stream that `parse_song_bank` accepts and
whose decoded audio equals the whole-buffer reference transform of the same source;
loop-restart replay reproduces identical bytes via checkpoint restore.

**Integration with previous steps:** Consumes Step 2's `StretchState` and codec
wrappers; still zero runtime wiring.

**Demo:** Validator `streaming` section shows a full synthetic 50 % and 175 % replay:
virtual bank served packet-by-packet, reassembled, reparsed, decoded, and matched
against the reference.

## Step 4: Wire the callback detours, binding, and generator into the transaction

**Objective:** The complete runtime path (reqs 9–11, 15–18, 21, 23–28, 40–41) —
host-tested, release-built, not yet deployed.

**Implementation guidance:**

- Add the callback-pair AOB to `src/core/signatures.rs` (manager-ctor registration
  region: `0xFA` imm + three LEA/MOV pairs; wildcard LEA disp32s and frame disp8s;
  RIP-decode the readFile and getOverlappedResult targets). Cross-verify in Ghidra on
  2026-03-24 / 04-21 / 06-16 / 07-21 and record resolved addresses in
  `docs/xact_streaming_research.md`'s cross-version section.
- Implement `src/services/song_rate/io_callback_hook.rs` (both detours, handle→
  file_id walk, epoch guard, pending slots, silence-fill serving), `binding.rs`
  preflight (source validation from an injected `SourceView`, `plan_virtual_bank`,
  source copy, producer start, publish) and `generator.rs` (producer thread, ring,
  checkpoint, regeneration targets, pending completion, `catch_unwind` →
  SilenceFill).
- Rework `wavebank_hook.rs`/`transaction.rs`: pre-original `bind` closure replaces
  the nested-convert exposure; post-original commit order untouched; unregister
  retires the binding and enqueues `ReclaimBinding`; drain performs reclamation at
  reader quiescence and logs generator diagnostics.
- Restore `integration_ready()` (clock ∧ wave hooks ∧ IO-callback hooks ∧ score
  readiness) so the option row registers again.
- Retarget `DDR_SONG_RATE_FAULT` legs (req 41). Add the interim assist-tick
  scaffolding gate (req 32): tick synthesis refuses when a non-identity generation is
  committed.
- Throughput/deferral metrics recorded per generation, logged at completion via the
  drain (feeds Step 5's benchmark).

**Tests:** Binding preflight refusal legs → EarlyFailed with no binding; bind →
create-success → commit ordering (Q31 last); create-failure → retire + LateFailed;
deferral protocol (exactly-once completion, `Internal` accounting,
poll-before-complete incomplete); behind-window regeneration; unregister-with-pending
cancellation; epoch-guard reclamation only at quiescence; Quick-Restart re-create
regeneration; silence-fill after injected producer death mid-stream (stream still
parses/decodes); fault-selector legs; readiness conjunction combinations; tick
scaffold gate.

**Integration with previous steps:** The runtime consumes Steps 2–3's pure cores
through the same interfaces the host harness exercised; the transaction reuses the
surviving Step-1 machinery.

**Demo:** Full validator green including the runtime suites; a release DLL whose new
signature resolves on all four supported builds (Ghidra evidence recorded); the
SONG SPEED row registers again in a host-reasoned readiness test.

## Step 5: Live bring-up and throughput benchmark on the cabinet

**Objective:** Retire the two assumptions no host test can: the engine accepts the
virtual bank live, and cabinet production speed sustains ≥ 1× realtime at the
extremes (reqs 27, 29, 42). Front-loaded because failure here invalidates the design.

**Implementation guidance:**

- Single maintainer deployment of Step 4's build to the CrossOver cabinet. Run sheet:
  (a) 50 % song end-to-end — slowed pitch-correct audio, arrows in sync, stage save
  suppressed; (b) 25 % and 175 % songs — the drain's generator diagnostics log
  production throughput and max deferral latency; (c) Quick Restart mid-song at 50 %;
  (d) a 100 % song — no binding, literal stock, save allowed;
  (e) `DDR_SONG_RATE_FAULT=mid-song-failure` run — silence-fill audible, song
  completes, WARN logged, taint retained.
- Expected log vocabulary documented in the run sheet before deployment (bind,
  commit, generator stats, silence-fill WARN).
- If throughput margin is inadequate on cabinet hardware, STOP and return to design
  (candidate levers exist — window parameters, encode batching — but they are design
  changes, not tweaks).

**Tests:** This step's validation is the live run sheet; no new host tests. Any fix
that changes code re-runs the standing gates before redeployment.

**Integration with previous steps:** First execution of Steps 2–4 inside the game
process.

**Demo:** A cabinet video/log pair showing a 50 % song fully synced, throughput
numbers at 25 %/175 % with recorded margin, and the silence-fill failure mode
behaving as designed.

## Step 6: Integrate dependent features (Assist Tick, Real Speed, PUS)

**Objective:** The delivery-required cross-feature work (reqs 30–34): assist ticks
correct at every rate, Real Speed × rate, PUS CSV columns.

**Implementation guidance:**

- Assist Tick: read `clock_patch::snapshot()` at the gameplay-start synthesis; convert
  chart-derived content positions and restart skips via `content_to_wall_ms` with the
  committed `RateRatio`; cabinet `sound_offset` stays unscaled; the judgment-timing
  term follows the clock stub's domain (design req 30). Raise `TICK_CAPACITY_MS` to
  1200 s (req 31; lazy registration unchanged; truncation WARN). Remove Step 4's
  scaffolding gate.
- Real Speed: at non-identity commit the normalized multiplier derives from
  `Core BPM × effective_rate` independent of the fix toggle (req 33).
- PUS: requested/effective rate columns in the CSV export (req 34).

**Tests:** Tick placement vectors at exact ratios (25/50/75/125/175) including
restart skips and 1200 s truncation; a regression vector proving 100 % placement is
bit-identical to today's; Real Speed multiplier derivation vectors at both toggle
states; PUS CSV column presence/values; the scaffold-gate removal covered by a test
asserting synthesis proceeds at committed rate.

**Integration with previous steps:** Consumes the committed-rate snapshot Step 4
publishes; no changes to the streaming engine.

**Demo:** Host vectors green; validator carries the tick-conversion evidence. (Live
tick-alignment listening check lands in Step 7's matrix.)

## Step 7: Hardening, documentation, and the release matrix

**Objective:** Close the feature: docs current, final live matrix passed (req 42).

**Implementation guidance:**

- Documentation: rewrite the Song Playback Speed rows/sections in `AGENTS.md` and
  `README.md` (no cache, no `cache_limit_gib`, streaming description, tick
  integration); refresh `docs/xact_streaming_research.md` cross-version table with
  any implementation-time findings; note the Assist Tick capacity change in its
  AGENTS.md row.
- Final maintainer deployment and matrix: slow (≤ 50 %) and fast (> 100 %) songs;
  assist-tick alignment at 50 % and 100 %; Quick Restart; Premium Free interaction;
  score containment re-oracle (suppressed stage saves, sanitised card-out logout,
  backend absence of rate-played scores, presence of interleaved 100 % scores);
  100 % literal-stock verification; a long-session soak (multiple rate songs
  back-to-back) watching reclamation (no growth in the drain's diagnostics).
- Sweep for leftovers: no `cache_limit_gib` reference, no stale planning-dir
  pointers in shipped docs, `mod-config.json` example updated.

**Tests:** No new host suites; the full standing gates re-run on the final tree. The
matrix run sheet is the acceptance record, logged in the feature's `progress.md`.

**Integration with previous steps:** Everything.

**Demo:** The maintainer plays a 50 % song with assist tick on — claps land on
judgment moments, music is pitch-correct and slow, arrows in sync, and the backend
shows no competitive record of it. A following 100 % song saves normally.
