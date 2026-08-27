# Task: Wire On-Demand Generation Into the Live Transaction

## Description
Replace the Step 4 pre-generated diagnostic import with strict on-demand rate generation (any multiple of 5 in 25%..=175%; 100% = identity): the open-time redirect requests a build from the already-running Step 2 worker/cache, and the proven two-stage transaction consumes the generated bank unchanged. Retire the one-song diagnostic config. No player-facing UI or policy in this task; arming remains driven by injected requests (host tests) until Task 2 supplies the option row.

## Background
Step 4 proved the full transaction end to end on the cabinet with one pre-generated 75% bank. The audible dance BGM is NOT streamed from the XACT wave bank's file handle: for slot-5 banks the engine's read callback memcpys from the FileManager's in-memory copy of `sound/win/dance/<code>.xwb` loaded via `avs_fs_open`/`avs_fs_read`. The working redirect is therefore two-stage: (1) an open-time redirect of `avs_fs_open`/`avs_fs_lstat` serves the generated bank into the audible RAM copy; (2) the `wavebank_create` convert-seam exposes the token and is the sole commit authority for the Q31 clock scale and score taint, and it structurally refuses (`RefuseReason::OpenNotRedirected`) unless the open redirect already happened. This invariant is load-bearing and MUST survive this task unchanged.

Today the substitution point is `src/services/song_rate/conversion.rs` (`validate_and_import`, the block that reads `spec.xwb_path`, runs `validate_diagnostic_bank`, and imports via `publish_checked`). The `DiagnosticCoordinator` (worker thread + cache store) is already lazily constructed at runtime but nothing calls its `request()`/`request_superseding()` APIs outside host tests. The 75-only gate lives in `lifecycle::validate_diagnostic`; the song-specific gates are `is_diagnostic_dance_path` plus the `DIAGNOSTIC` OnceLock guards in `runtime.rs`.

Per maintainer decision (2026-08-07), Steps 5 and 6 of the plan are merged into one delivery and there is NO interim dev-forced rate config: the option row (Task 2) is the only production rate source. The arm-scope movie-suppression gap recorded in the feature's progress.md dissolves under this model — every armed song genuinely commits at the session's selected rate, and tentative suppression through a failed attempt is accepted design behavior.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-08-05-song-playback-speed/design/detailed-design.md` (Audio, Cache, LayeredFS Integration, Cache Manager, Failure and Release Gates 48-50, Data Models)
- Plan Steps 5-6: `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md`
- Progress + deviations: `.agents/planning/2026-08-05-song-playback-speed/progress.md` (2026-08-06/07 Deploy & test log entries — the audio-source discovery; Deviations)

**Additional References (if relevant to this task):**
- `docs/song_playback_speed.md` (clock math)
- `src/services/song_rate/conversion.rs`, `worker.rs`, `cache.rs`, `runtime.rs`, `lifecycle.rs`, `transaction.rs`, `model.rs`
- `src/services/avs_layeredfs/file_hooks.rs`
- `src/lib.rs` (init 5b, diagnostic config parsing)
- `src/mods/config.rs` (`song_playback_speed` block)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Generalize the arm specification: replace the 75-only, single-song `DiagnosticSpec` with a rate-generic arm model in which the requested percent is any multiple of 5 in `25..=175` (100 arms identity; maintainer-approved design change 2026-08-07 superseding the design's 75/100/125 enum), arming is song-agnostic, the percent is supplied by the `ArmRequest`, and no configured song code or pre-generated `xwb_path` exists. Widen `lifecycle::validate_diagnostic`'s hard-coded 75 check and the `rate::target_for_percent` percent predicate (`src/core/xact/rate.rs`, currently `75 | 100 | 125`) to the same multiple-of-5 `25..=175` domain. The clock stub needs NO change (64-bit `imul` factor slot; `scale_music_count_q31` uses an i128 product) — add boundary Q31 reference vectors at 25% and 175% to the clock tests.
2. Remove the `song_playback_speed.diagnostic` config block (`song_code`/`requested_percent`/`xwb_path`) from parsing, `lib.rs` wiring, and `runtime::init`; the `layeredfs.developer_mode` gate no longer gates arming (it continues to gate the `DDR_SONG_RATE_FAULT` env selector). Runtime gates that read `DIAGNOSTIC.get()` (`redirect_dance_bank_open`, `convert_streaming_xwb`, `diagnostic_configured`, maintenance-drain spawn) are re-keyed to "a rate source is registered and a non-identity generation can arm" so that identity boots keep zero footprint (no cache directory creation, no drain thread until first needed, exactly as today).
3. Replace the pre-generated-bank import inside `validate_and_import` with the worker request: resolve the effective source (static LayeredFS `.xwb` replacement wins over the stock native path resolved through the ORIGINAL AVS conversion — existing plumbing), read it, construct the `GenerationRequest`, check quarantine, then call `coordinator.request()` (warm hits return without queueing; cold builds block with the existing 30-second deadline, cancellation/epoch, memory-ceiling, and free-space admission semantics). `validate_diagnostic_bank` (validating a foreign pre-generated file against the live source) is retired; the worker's mandatory output reparse and the store's warm-hit validation are the correctness authority.
4. Waiting-site discipline: the FileManager open (`prepare_open_redirect` via `redirect_dance_bank_open` in `fs_open_body`) is the only place a build may be awaited, alongside the already-permitted `fs_convert_path` nested inside `wavebank_create` (which after this task only re-derives a warm hit and never triggers a cold build, because the seam structurally requires the open redirect to have completed). `fs_lstat_body` continues to be served exclusively from the generation-keyed `OPEN_REDIRECT` cache and never waits on or starts a build.
5. Preserve the two-stage invariant byte for byte: the seam still enters only from `RedirectReady` (or `Committed` for idempotent reload) and refuses `OpenNotRedirected` from `Armed`; commit ordering (score -> movie -> snapshot -> Q31-last), identity-first reset, deferred-reset repair, slot/lease/quarantine transitions, and the exactly-once wave-bank protocol are unchanged.
6. Every pre-exposure failure — source read, profile validation, generation failure, timeout, cancellation, quarantine hit, space admission, worker saturation — resolves to `EarlyFailed` and stock 100% with a bounded (per-generation) warning and no score taint; post-exposure failure behavior is unchanged from Step 4. Note that the existing 128 MiB memory-admission ceiling and the 28-bit XWB duration field become REACHABLE at extreme slow rates (25% quadruples output frames/bytes on long songs) — both must resolve through this same early-failure leg, covered by tests.
7. Parse `song_playback_speed.cache_limit_gib` through the existing `model::normalize_cache_limit` (default 10, clamp 1..=1024, one startup warning on normalization) and thread it into coordinator construction, replacing the hard-coded default.
8. Re-wire the `DDR_SONG_RATE_FAULT` selector's `source-read`/`validation`/`conversion` legs so they fire against the on-demand path (the transaction-side legs are untouched); the selector remains dev-mode/env gated.
9. Detour discipline is unchanged: audio detours stay allocation-free, lock-free, log-free, and panic-contained; the nested `fs_convert_path` and the FileManager open remain the only waiting/I-O sites; all logging happens in the maintenance drain or non-detour paths.
10. Host tests (added in the same task, TDD): 100%/unarmed zero footprint (no worker submission, no cache output, no `OPEN_REDIRECT` entry); cold-build success at 75 and 125 through the open redirect plus rate-math/Q31/exact-rate boundary coverage at 25 and 175 (and rejection of non-multiples-of-5 and out-of-range percents); warm-hit reuse without a second transform; static-LayeredFS custom source selected over stock; every requirement-6 failure leg (including a 25%-driven memory-ceiling/duration-overflow rejection); structural refusal when the seam runs without the open redirect; idempotent same-generation reload; worker-thread reuse across consecutive songs; unrelated/nested XWB path isolation; cache-limit parsing default/clamp/warning cases. New test FILES must be registered in BOTH `scripts/validate_song_playback_speed.sh`'s file-existence list AND its generated harness `main.rs` mods.
11. Run all five host gates (`./scripts/validate_song_playback_speed.sh`, `./scripts/validate_se_bank_synth.sh`, `cargo check --target x86_64-pc-windows-msvc`, whole-crate `cargo fmt`, `./build.sh`). Do NOT deploy — all cabinet validation is concentrated in Task 4.

## Dependencies
- Completed Step 2 worker/cache (`GenerationCoordinator::request`, `CacheStore`, leases/eviction/quarantine) — already live at runtime, host-proven.
- Completed Step 4 two-stage transaction (`prepare_open_redirect`, `prepare_streaming_redirect`, commit/reset/unload/late-fail) — cabinet-proven.

## Implementation Approach
1. Write the failing host tests for the new arm model, worker routing, and zero-footprint/failure legs first.
2. Generalize the lifecycle arm spec and validation; remove the diagnostic config plumbing end to end (`config.rs`, `lib.rs`, `runtime.rs`).
3. Swap the import block in `conversion.rs::validate_and_import` for source resolution + `GenerationRequest` + `coordinator.request()`; keep quarantine/lease/slot/expose logic intact.
4. Wire `cache_limit_gib` and the fault-selector legs; verify detour paths gained no allocation/lock/log.
5. Run the full gate set; update the canonical planning-dir `progress.md` (never `.agents/scratchpad/`).

## Acceptance Criteria

1. **On-Demand Generation Through the Open Redirect**
   - Given an armed non-identity generation for a song with a stock or static-LayeredFS source and a cold cache
   - When the FileManager open probes the dance XWB path
   - Then the worker builds the bank within the deadline/admission rules, the open redirect serves the generated path, the seam later re-derives a warm hit and exposes, and commit carries the exact per-request rate (proven at 75 and 125, with exact-rate/Q31 math additionally covered at the 25 and 175 boundaries)

2. **Zero Footprint at Identity**
   - Given a 100%/unarmed selection or an identity boot
   - When songs load and play
   - Then no worker request, cache directory, open-redirect entry, or generated artifact is produced and LayeredFS behavior is byte-identical to stock

3. **Two-Stage Invariant Preserved**
   - Given a seam invocation whose generation never completed the open redirect
   - When `prepare_streaming_redirect` runs
   - Then it structurally refuses (`OpenNotRedirected`), no token is exposed, no rate commits against stock audio, and the generation resolves `EarlyFailed`

4. **Every Early Failure Falls Back Stock**
   - Given each documented pre-exposure failure (source read, validation, generation, timeout, cancellation, quarantine, space, saturation) injected via tests or the fault selector
   - When the open redirect runs
   - Then playback proceeds with stock audio at identity clock, no score taint exists, exactly one bounded warning is recorded, and clock/token/lease state is clean for the next song

5. **Config and Gates**
   - Given `cache_limit_gib` absent, in range, zero, and out of range, plus the retired `diagnostic` block present in an old config
   - When the DLL initializes
   - Then the limit normalizes per design with one warning, the stale `diagnostic` block is ignored gracefully, and all five host gates pass with no deployment performed

## Metadata
- **Complexity**: High
- **Labels**: rust, layeredfs, xact, worker, cache, on-demand-generation, fault-injection, host-only, step-5
- **Required Skills**: code-assist, verification, self-documenting-code
- **Generated By**: code-task-generator 2026-08-07
- **Source Plan**: `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md`
- **Plan Step**: Steps 5+6 (merged delivery, maintainer-approved 2026-08-07) — Step 5: Connect on-demand generation and cache to XACT
