# Task: Build Song XWB Transformer

## Description
Combine the shared XWB/ADPCM codecs and deterministic stretcher into a pure,
sequential song-bank transformation pipeline. The transformer must accept only
the approved two-entry DDR World profile, preserve XSB-visible identity, produce
exact duration/loop metadata, and enforce the approved memory limits.

## Background
Tasks 1 and 2 provide correct primitives but do not yet produce a complete
pitch-preserved song XWB. This task supplies the reusable operation later cache
and LayeredFS steps will call: validate an effective source bank, transform main
and preview entries in their existing order, stream encoded output, and return a
manifest-ready report containing exact rates and digests.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-08-05-song-playback-speed/design/detailed-design.md`

**Additional References (if relevant to this task):**
- `.agents/planning/2026-08-05-song-playback-speed/research/pitch-preservation.md`
- `.agents/planning/20260725-assist-tick/research/xact-bank-format.md`
- `src/core/xact/` from Tasks 1-2
- Sibling repository `ddr-chart-tools`: `src/job/mod.rs` for comparison only

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Add a pure song transformation API under the shared XACT boundary; it must take source bytes/song code/percentage/output writer and return typed output metadata without cache or game-service dependencies.
2. Enforce the exact two-entry profile and resolve the main entry by exact `<code>` name, with `<code>_s` as preview in either physical order.
3. Trim source PCM to validated logical duration, transform both entries independently, and preserve bank name, entry order/names, packed format/sample rate, flags, alignment, and wave indices.
4. Compute each target via the exact rate helper; the main `RateRatio` is the authoritative report value.
5. Map half-open loop boundaries with approved integer formulas, reject invalid correction, and validate the generated preview seam.
6. Process entries sequentially, release intermediate buffers promptly, direct-encode interleaved blocks, and stream the rebuilt XWB without duplicate whole-bank compressed buffers.
7. Use checked preflight memory estimation and `try_reserve_exact`; reject estimates above 128 MiB before allocation.
8. Check cancellation/build epoch during hashing, source processing, WSOLA windows, ADPCM blocks, output writes, and output digest validation through an injected pure callback/token.
9. Return source/output full digests, lengths, per-entry source/output frames, main index/rate, algorithm/codec versions, and measured clipping/seam data for the later cache manifest.
10. Add tests for both entry orders, 75%/125%, stock partial tails, invalid profiles, cancellation, memory admission, deterministic output, reparsing, and metadata identity.
11. Keep all game-derived test banks external; update canonical `progress.md` after validation.

## Dependencies
- Task 1 strict XWB/MS-ADPCM implementation.
- Task 2 `RateRatio` and deterministic stretch implementation.
- No persistent cache or runtime hook dependency.

## Implementation Approach
1. Define the transform request/result/error and cancellation interfaces.
2. Validate and classify the source bank before allocating PCM.
3. Estimate memory and output sizes with checked arithmetic.
4. Transform and encode each entry sequentially into serializer-owned output.
5. Rebuild duration/loop/range metadata and finalize output digest/report.
6. Reparse and verify generated output as a mandatory postcondition.
7. Add malformed, cancellation, determinism, order, and resource-limit tests; run all build gates and update progress.

## Acceptance Criteria

1. **Both Entry Orders Supported**
   - Given valid banks ordered main/preview and preview/main
   - When each is transformed at 75% and 125%
   - Then the original order and names remain unchanged, both entries are transformed, and the exact main rate is reported

2. **Metadata and XSB Identity Preserved**
   - Given a valid supported source bank
   - When the generated output is reparsed
   - Then bank identity, packed formats, sample rates, streaming flags, alignment, indices, whole-block data, durations, and half-open loops satisfy the approved profile

3. **Invalid Source Fails Before Publication**
   - Given malformed layout, names, entry count, codec/channel fields, durations, loops, partial tails, or output overflow
   - When transformation is requested
   - Then a specific typed error is returned and no complete output is reported

4. **Bounded Sequential Processing**
   - Given a large supported synthetic bank near the approved limit
   - When memory preflight and transformation run
   - Then the estimate stays within 128 MiB, entries are processed sequentially without whole-song channel duplication, and over-limit input is rejected before allocation

5. **Cancellation and Determinism**
   - Given cancellation at source hash, decode, stretch, encode, write, and output-digest phases
   - When transformation runs
   - Then it exits with cancellation and never reports a publishable result; without cancellation, repeated runs are byte-identical

6. **Build Readiness**
   - Given the complete transformer and tests
   - When all task tests and repository check/format/release gates run
   - Then all pass and canonical `progress.md` identifies Task 4 as the next action

## Metadata
- **Complexity**: High
- **Labels**: rust, xwb, audio-transform, pitch-preservation, resource-safety, step-1
- **Required Skills**: code-assist, verification, self-documenting-code
- **Generated By**: code-task-generator 2026-08-05
- **Source Plan**: `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md`
- **Plan Step**: Step 1: Build the deterministic host audio pipeline
