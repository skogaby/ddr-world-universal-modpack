# Implementation Plan: Song Playback Speed

Status: Approved 2026-08-05

Date: 2026-08-05

## Checklist

- [x] Step 1: Build the deterministic host audio pipeline
- [x] Step 2: Build the persistent cache and generation worker
- [x] Step 3: Install identity-only runtime transaction infrastructure
- [x] Step 4: Prove one pre-generated 75% song end to end
- [ ] Step 5: Connect on-demand generation and cache to XACT
- [ ] Step 6: Add player-facing policy, persistence, and backend support
- [ ] Step 7: Complete dependent-feature and lifecycle integration
- [ ] Step 8: Complete operational hardening and release validation

## Step 1: Build the deterministic host audio pipeline

**Objective:** Produce and validate a pitch-preserved XWB entirely outside the
game, establishing the format, codec, exact-rate, and DSP foundation before any
hook code depends on it.

**Implementation guidance:**

- Create the feature's canonical `progress.md` before code work and record the
  exact `ddr-chart-tools` source revision being ported.
- Add shared pure XACT format code under `src/core/xact/`: strict borrowed XWB
  v43 parsing, serialization, arbitrary-channel MS-ADPCM decode/encode, and the
  accepted two-entry DDR song profile validation.
- Migrate Assist Tick's compatible ADPCM primitives to the shared code only
  after byte-for-byte parity tests; keep its fixed XSB/container policy local.
- Implement the deterministic joint-stereo WSOLA-like stretcher and exact
  block-aligned `RateRatio` arithmetic from the approved design.
- Implement sequential entry processing, checked memory admission, exact
  duration/loop mapping, cyclic preview seam handling, and streaming block
  encoding without whole-song channel duplication.
- Add `scripts/validate_song_playback_speed.sh` with the documented environment,
  stable JSON report, cross-repository comparison, synthetic fixtures, and local
  corpus support. The script must never copy or commit game audio.

**Tests:**

- Cover every accepted/rejected XWB field and stock partial-tail rule.
- Cover ADPCM deterministic output, exact block count, malformed lengths, and
  minimum 30 dB synthetic sine SNR.
- Cover WSOLA exact output length, frequency error, anti-phase/asymmetric stereo,
  deterministic tie-breaking, short/boundary inputs, clipping, and preview seam.
- Cover exact rate, Q31, signed content-to-wall conversion, overflow, saturation,
  and 75%/125% targets.
- Run the validator with synthetic fixtures and the non-committed local stock/
  custom corpus, then run the repository check/format/release build gates.

**Integration with previous steps:** This is the first step. Its APIs remain
game-independent and become the only audio-format/DSP implementation consumed by
later runtime steps.

**Demo:** Run `./scripts/validate_song_playback_speed.sh` to generate validated
75% and 125% variants of a local test bank. Show exact frame ratios, unchanged
pitch within tolerance, preserved bank/entry identity, and a passing JSON report.

## Step 2: Build the persistent cache and generation worker

**Objective:** Turn the pure transform into a bounded, crash-safe, concurrent
cache service without attaching it to AVS or XACT yet.

**Implementation guidance:**

- Add `src/services/song_rate/cache.rs` and the CPU generation worker around the
  Step 1 pipeline.
- Implement canonical full-digest keys, immutable output/manifest publication,
  separate versioned LRU index, same-directory temporary files, free-space
  reservation, output-digest validation, and startup recovery.
- Implement the build table, epochs, one-heavy-job queue, duplicate waiter
  sharing, cooperative cancellation at every specified boundary, 30-second
  deadline, worker panic guard, and unconditional waiter notification.
- Implement preallocated lease ids/states, `Evicting` exclusion, active-entry
  protection, tombstone quarantine, stopped-game purge behavior, and the 10 GiB
  default/config normalization.
- Add operator-facing cache size/recovery diagnostics as pure service outputs;
  do not install game hooks in this step.

**Tests:**

- Exercise cold build, warm hit, digest/version invalidation, duplicate callers,
  queued and active cancellation, timeout, worker panic, and stale epoch
  publication prevention.
- Inject write, flush, rename, disk-full, permission, corrupt-destination, delete,
  and interrupted-publication failures in temporary directories.
- Verify startup orphan cleanup, LRU index recovery, lease/eviction races,
  quarantine tombstone behavior, limit accounting, and active-entry refusal.
- Benchmark memory/latency against the design thresholds using the local corpus.
- Run the host validator and normal repository build gates.

**Integration with previous steps:** Uses only Step 1's pure transform APIs.
Later runtime code receives immutable generated paths and consuming lease ids; it
does not know codec/DSP internals.

**Demo:** In a temporary cache, issue concurrent requests for one local XWB,
show one build and shared waiters, demonstrate a warm hit, force eviction and
crash recovery, and produce the cache section of the JSON validation report.

## Step 3: Install identity-only runtime transaction infrastructure

**Objective:** Install every low-level hook and state primitive at semantic
identity, proving that ordinary 100% gameplay remains stock before transformed
audio is ever exposed.

**Implementation guidance:**

- Add and cross-verify the central-clock, `wavebank_create`, and wave-bank-unload
  signatures/derivations on all four supported binaries.
- Add checked code-patch helpers for protection, near allocation, jump range,
  instruction-cache flush, readback, and rollback.
- Implement the permanent identity Q31 clock stub and its Rust reference model.
- Create `src/services/song_rate/` coordinator/publication state, fixed XACT slot
  table, call-nonced TLS frames, seqlock writer protocol, deferred reset, and
  exactly-once wavebank/unload detours. Keep non-100% arming unavailable.
- Make LayeredFS hook installation transactional and expose strict conversion/
  source-read readiness. Add the generated-XWB conversion seam, but return no
  dynamic rate replacement while the service is identity-only.
- Promote `movie_build_graph` ownership into `movie_policy` while preserving
  existing Non-Native OS behavior exactly.
- Add the preallocated maintenance event queue used by unload/quarantine paths;
  audio detours may only CAS slots and enqueue.

**Tests:**

- Verify match counts, derived sites, expected bytes, emitted stub disassembly,
  register preservation, Q31 reference vectors, and failure rollback.
- Stress seqlock commit/reset overlap and fixed-slot/TLS nesting/thread reuse.
- Prove every detour calls its original exactly once under normal and injected
  pre/post failures.
- Prove LayeredFS partial installation rolls back and readiness remains false.
- Run all build gates, deploy an identity build, and collect boot/hook evidence
  on the current cabinet build.

**Integration with previous steps:** The runtime service can hold Step 2 cache
lease ids but cannot request a non-100% build yet. Every externally visible path
is identity or original-call pass-through.

**Demo:** Boot and play a normal 100% song with zero generated song-rate files,
stock `music_count`, normal score submission, correct movie behavior, and logs
showing the clock/wave/unload hooks armed at identity.

## Step 4: Prove one pre-generated 75% song end to end

**Objective:** Pass the load-bearing diagnostic gate using one locally generated
75% bank before adding generalized runtime generation or player UI.

**Implementation guidance:**

- Implement final scene-26 eligibility primitives needed for the diagnostic:
  stage index, course field, entered side, nonblocking arm, and permanent
  lifecycle callback.
- Implement full score-sanitization readiness, per-side pending stage-save rings,
  exact side/stage claim/consume behavior, per-side successful-card reset, and
  unknown-decode fail-closed handling.
- Implement tentative song-rate movie suppression at arm and its definitive
  lifecycle clearing.
- Add a developer-only diagnostic arm for one configured/local test song and
  expose its pre-generated Step 1 XWB through the call-nonced
  `fs_convert_path -> XactInFlight -> wavebank_create` transaction.
- Implement allocation-free commit/late-fail ordering: score protection,
  coherent snapshot, Q31-last activation, identity-first reset, and quarantine
  maintenance enqueue.
- Add generation-correlated logs and the diagnostic fault selector needed for
  this gate. Do not add release UI or generalized source transformation yet.

**Tests:**

- Host-test pending-save identity, duplicates/reordering, Quick Restart
  deduplication, unknown side/stage, session reset, transaction order, and
  XactInFlight supersession refusal.
- Inject token mismatch, pre/post-original faults, XACT rejection, scene reset
  overlap, and movie policy combinations.
- Run repository build gates before deployment.
- Execute the approved cabinet diagnostic: pitch error, first/late/final
  landmarks, <=2 ms drift, natural song end, judgment alignment, backend score
  absence, logout sanitation, and literal 100% restoration.

**Integration with previous steps:** Uses a Step 1 artifact, Step 2 lease/cache
metadata, and Step 3 identity transaction. Passing this step is a hard gate; if
it fails, stop and revise the design/evidence before Steps 5-8.

**Demo:** On the cabinet, play the selected 75% diagnostic song from beginning
to natural exit, show synchronized pitch-preserved audio and chart logs, capture
the suppressed stage save/sanitized logout, then play 100% with stock behavior.

## Step 5: Connect on-demand generation and cache to XACT

**Objective:** Replace the one-song diagnostic artifact with strict, on-demand
75%/125% generation from effective stock or static LayeredFS XWB sources.

**Implementation guidance:**

- Connect the Step 2 cache/generation API to only the streaming
  `fs_convert_path` branch; lstat/open remain nonblocking against the effective
  source.
- Resolve stock native sources through the original AVS conversion and direct
  LayeredFS sources through their host paths.
- Implement exact virtual/generated path and cache/output digest tokens,
  fixed-slot lease transfer, idempotent same-generation re-exposure/recommit,
  unload release, and late-failure process pinning/tombstones.
- Enforce strict XWB profile fallback, timeout/cancellation/build epoch, free-
  space admission, warm-hit validation, and cache maintenance outside hooks.
- Remove the one-song diagnostic force from normal behavior while retaining
  developer fault injection.

**Tests:**

- Host-test path/source selection, digest token mismatch, unrelated/nested XWBs,
  worker-thread reuse, reload idempotence, unload release, and quarantine.
- Exercise stock and static custom-song banks at 75% and 125%, cold and warm.
- Verify 100% incurs no rate cache output and minimal conversion overhead.
- Inject every documented early/late cache, conversion, and XACT failure and
  verify clock/token/lease/taint recovery.
- Run build gates and deploy cold/warm cache tests under native Windows and
  CrossOver.

**Integration with previous steps:** Generalizes the exact transaction proven in
Step 4; it does not change player policy or expose a release option yet.

**Demo:** Select several developer-forced songs at 75% and 125%; show first-load
generation within memory/latency limits, warm reuse, custom-source composition,
cache eviction/recovery, and safe 100%/failure fallback.

## Step 6: Add player-facing policy, persistence, and backend support

**Objective:** Turn the proven runtime mechanism into the accepted per-player
feature without weakening mode or readiness gates.

**Implementation guidance:**

- Add `SongPlaybackSpeedMod`, register `song_speed` in `init`, add
  `set_option_available`/strict row-injection readiness, normalize with
  `load_transform`, and keep one permanent nonblocking scene callback.
- Add 75/100/125 label assets and generator definitions before the one-time atlas
  flush; add built-in row-order/config examples.
- Resolve ordinary e-amusement solo/doubles from scene 26, participant mask,
  course and stage state. Force identity for zero/two sides and all alternate
  scene chains.
- Implement next-song-only changes, runtime disable/re-enable behavior, JSON
  cache persistence, and capability/readiness visibility.
- Add `song_playback_speed.cache_limit_gib` parsing/default/clamping.
- Update the sibling `bemani-buddy` schema, profile model/storage migration,
  save/load handlers, and tests for `mod_song_speed`.

**Tests:**

- Test registration before atlas flush, boot-disabled enable, availability on
  form rebuild, unknown persisted values, P1/P2 isolation, JSON cache, and
  backend round-trip.
- Test solo P1, solo P2, P1/P2-started doubles, local versus, course, alternate
  scenes, missing pointers/services, mid-song edits, and disable-mid-song.
- Capture request/response/database evidence for the new backend field.
- Run the host validator, backend tests, repository build gates, and cabinet
  option/policy demo.

**Integration with previous steps:** The mod supplies only desired policy and
arm requests to Step 5. The shared runtime remains the sole authority for what
actually commits.

**Demo:** Swipe P1/P2 profiles with different saved values, select 75%/125% in
eligible solo/doubles, prove changes apply on the next song, and prove versus/
course/special flows remain 100%. Restart and confirm server/JSON round-trip.

## Step 7: Complete dependent-feature and lifecycle integration

**Objective:** Make every accepted cross-feature behavior consume the same exact
committed rate and close all score/profile/lifecycle gaps.

**Implementation guidance:**

- Make Assist Tick rate-aware, increase capacity to 400 seconds, and retain the
  identity fast path/silent-song fallback.
- Re-derive and live-validate the Core-BPM cache source, then perform the native
  Real Speed normalized-field formula as a raw gameplay-entry write without
  competing with the existing Real Speed Fix patches.
- Relabel Power User Statistics/pacemaker threshold values as chart
  milliseconds and add requested/effective rate CSV fields.
- Complete `docs/song_playback_speed_score_audit.md`, implement every required
  competitive-field sanitizer, and make full readiness depend on its closure,
  scene manager, and checked league-node removal.
- Instrument/prove Quick Restart's redirected scene sequence and support both
  retained-bank and idempotent reload behavior.
- Complete movie ordering, natural exit, session reset, delayed save, Premium
  Free, Quick Fail, Autoplay, and late-failure behavior.

**Tests:**

- Host-test Assist Tick conversions, capacity, rewinds, Real Speed formula and
  field validation, PUS/CSV schema, sanitizer outcomes, and delayed/duplicate
  save rings.
- Cabinet-test Assist Tick first/middle/final rows with timing offsets; Real
  Speed target preservation; `75 -> 100 -> 75`; Quick Restart/reload; Quick
  Fail; Premium Free; Autoplay; natural logout; and movie-backed songs.
- Capture field-specific backend payloads and database sentinels proving all
  competitive data is absent while permitted profile/options/calories persist.
- Run all host/backend/build gates.

**Integration with previous steps:** Extends Step 6's committed-rate snapshot;
no dependent feature derives rate independently or changes audio/clock
ownership.

**Demo:** Run one integrated training session using Assist Tick, Real Speed,
statistics, Quick Restart, Premium Free, and a movie-backed song. Show exact-rate
behavior, score safety, persisted permitted fields, and clean 100% restoration.

## Step 8: Complete operational hardening and release validation

**Objective:** Produce a releasable, diagnosable feature with completed evidence,
operator documentation, compatibility records, and recovery procedures.

**Implementation guidance:**

- Finalize generation-correlated diagnostics, stable host report, fault
  injection, cache size/recovery/purge reporting, and bounded warning behavior.
- Create `docs/song_playback_speed_validation.md` with one row per requirement,
  the full platform/build matrix, oracle, artifact, owner, and status.
- Complete `docs/song_playback_speed_listening_checklist.md`, durable RE notes,
  README configuration/feature/cache/operator guidance, AGENTS.md navigation,
  and all module/config examples.
- Record static AOB/stub evidence for all four builds and full current-build live
  evidence for native Windows and CrossOver/spice2x. Include Win7 build/smoke if
  that artifact remains supported.
- Run every fault-injection postcondition, cache crash-recovery case, field-
  specific backend check, unsupported-mode case, and full integration matrix.
- Keep generated game-derived audio/log artifacts out of source control while
  retaining hashes and pass/fail summaries.

**Tests:**

- Run `./scripts/validate_song_playback_speed.sh` with the release corpus and
  require schema-v1 success.
- Run backend migration/model/handler tests.
- Run `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`, and
  `./build.sh`; run `./build_win7.sh` when applicable.
- Execute and sign off every validation-matrix row on both maintained runtime
  environments, including exact `75 -> 100 -> 75`, 125%, cold/warm cache,
  score/profile evidence, movies, restart, failure injection, and recovery.

**Integration with previous steps:** Hardens and documents the complete Steps
1-7 system. Any failed release row sends work back to the owning implementation
step; no acceptance criterion is waived in this final step.

**Demo:** Present the release candidate with green host/backend/build gates,
completed requirement matrix and score audit, native/CrossOver evidence bundle,
operator cache recovery demonstration, and no unresolved high-risk finding.
