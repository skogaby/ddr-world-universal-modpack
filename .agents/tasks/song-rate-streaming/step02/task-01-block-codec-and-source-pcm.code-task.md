# Task: Block-Level Codec Wrappers and the SourcePcm Decode View

## Description

Give the streaming stretcher its audio-access primitives: public per-block
MS-ADPCM `decode_block`/`encode_block` wrappers in `src/core/xact/adpcm.rs`, the
`SourcePcm` random-access decode-view trait, and a small block-cache
implementation of that trait over a borrowed source-bank entry. This is the
foundation half of plan Step 2; the resumable `StretchState` (next task)
consumes `SourcePcm` instead of a whole decoded buffer.

## Background

The retired model decoded entire entries up front. The streaming design's
producer decodes source ADPCM blocks on demand: blocks are fully
self-contained (stock stereo profile: 140 bytes / 128 frames; the block
geometry comes from `WaveFormat::block_align()` / `samples_per_block()`), and
any single `StretchState::produce` call touches only a bounded source window
(≈ 2160 frames at 48 kHz), so a tiny block cache suffices. `adpcm.rs` already
has private per-block `encode_block`/`decode_block` internals — this task
exposes them behind thin public wrappers rather than duplicating codec logic.
The final block of a stock entry may be logically partial (the entry's
`duration` ends inside the block); the view must clamp to the logical
duration exactly as `decode_interleaved` does.

## Reference Documentation

**Required:**
- Design: `.agents/planning/2026-08-08-song-rate-streaming/design/detailed-design.md`
  (§`core::xact::stretch — streaming WSOLA state machine`, the `SourcePcm`
  bullet; reqs 16–17)

**Additional References (if relevant to this task):**
- `.agents/planning/2026-08-08-song-rate-streaming/research/orientation.md` —
  "The streamability finding" (bounded-access numbers) and the `adpcm.rs`
  verdict row

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. Add public `decode_block` and `encode_block` to `src/core/xact/adpcm.rs` as
   thin wrappers over the existing per-block internals (no codec logic is
   duplicated or altered; the whole-buffer APIs are untouched).
2. Define the `SourcePcm` trait — a random-access view of decoded interleaved
   source PCM (frame-indexed reads plus the totals `StretchState` needs:
   frame count, channel count). Recommended home: `src/core/xact/stretch.rs`
   (consumer-owned), stated as guidance rather than mandate.
3. Implement a block-cache `SourcePcm` over a borrowed source-bank entry
   (`data: &[u8]`, `WaveFormat`, logical `duration`): decode blocks on demand
   into a small fixed-size cache (a handful of blocks — sized generously above
   the stretcher's bounded window), clamping the final partial block to the
   logical duration.
4. Provide a trivial in-memory implementation over `&[i16]` interleaved PCM
   (test fixtures and the next task's oracle comparisons feed raw PCM).
5. Rejection behavior mirrors the existing codec: malformed block sizes,
   zero-channel formats, and out-of-range frame indices must fail loudly in
   tests (panic or typed error per the surrounding module's conventions),
   never return silent garbage.
6. Tests live with the code (`src/core/xact/tests.rs` or inline
   `#[cfg(test)]`), running through the validator harness's `cargo test`
   phase (the crate has no local test runner).

## Dependencies

- None (first task of plan Step 2). Blocks `task-02-resumable-stretch-state`.

## Implementation Approach

1. Expose the wrappers; test them against the whole-buffer codec first
   (byte-for-byte per block) so the equivalence oracle exists before the view.
2. Build the block-cache view; property-test it against `decode_interleaved`
   over whole entries (every frame index equal), including a stock-shaped
   partial final block and 1/2/6-channel formats.
3. Run the full gate set: `./scripts/validate_song_playback_speed.sh`,
   `./scripts/validate_se_bank_synth.sh`,
   `cargo check --target x86_64-pc-windows-msvc` (0 warnings), whole-crate
   `cargo fmt`, `./build.sh`.
4. Record progress in
   `.agents/planning/2026-08-08-song-rate-streaming/progress.md` and the
   task's working directory under
   `.agents/planning/2026-08-08-song-rate-streaming/implementation/` (repo
   convention: NEVER `.agents/scratchpad/`).

## Acceptance Criteria

1. **Per-block codec equivalence**
   - Given an entry encoded by the whole-buffer `encode_interleaved`
   - When each block is decoded via `decode_block` and each whole-block PCM
     window is re-encoded via `encode_block`
   - Then the per-block outputs byte-match the corresponding slices of the
     whole-buffer codec's output, for mono, stereo, and 6-channel formats

2. **Block-cache view equals the whole-buffer decode**
   - Given a source entry (including one whose final block is logically
     partial)
   - When every frame index is read through the block-cache `SourcePcm`
     (in-order, reverse, and random orders)
   - Then every sample equals the corresponding `decode_interleaved` output,
     and reads past the logical duration are rejected

3. **Whole-buffer surfaces untouched**
   - Given the existing codec and stretch test suites
   - When the validator harness runs
   - Then they pass unmodified (wrappers add surface, never change behavior)

4. **Tree is green**
   - Given the completed task
   - When running the five standing gates
   - Then all pass, with the Windows-target check at 0 warnings

## Metadata

- **Complexity**: Medium
- **Labels**: audio, codec, song-rate, streaming, host-validation
- **Required Skills**: Rust, MS-ADPCM block format, repository host-validator
  harness
- **Generated By**: code-task-generator 2026-08-09
- **Source Plan**: `.agents/planning/2026-08-08-song-rate-streaming/implementation/plan.md`
- **Plan Step**: Step 2: Build the streaming WSOLA core with byte-equality proof
