# Task: Synthetic Engine Replay Harness with Byte-Equality Proof

## Description

Build the host-side synthetic engine replay: a pull-driven test pump that
reproduces the RE-pinned XACT streaming read pattern against a
`Binding`-shaped serving surface (virtual-bank `resolve` regions + a
per-entry encoded feed built from Step 2's `StretchState` and codec
wrappers), proving that a virtual bank survives the engine's exact reads —
the reassembled byte stream parses, decodes, and byte-matches the whole-buffer
reference transform, and a loop-restart replay reproduces identical bytes via
checkpoint restore. Still zero runtime wiring; no threads (the pump is a
synchronous pull loop).

## Background

The RE-pinned read pattern (design appendix, `docs/xact_streaming_research.md`):
one 0x1000-byte header read at offset 0 issued synchronously at create
(spanning the 2048-byte pre-data AND the start of entry-0 data); data reads
sequential, block-align-rounded 64 KiB packets (packetSize 0x20 sectors), one
outstanding per stream; the loop start is the ONLY backward jump; every read
clamps to `min(len, size − offset)`. The pump replays this shape; the async
completion protocol (OVERLAPPED/pending) is Step 4's concern, not this
task's — serves complete synchronously here.

The encoded feed composes proven pieces: `StretchState::produce` (byte-equal
to the reference, Step 2 task-02) → accumulate whole `samples_per_block`
frames → `adpcm::encode_block` (byte-equal per block to the whole-buffer
encoder, Step 2 task-01). Byte equality of the reassembled bank against the
whole-buffer oracle therefore holds exactly, not approximately.

Loop restart (design req 20): the pump captures a `StretchCheckpoint` when
generation passes the stretched loop start; the backward jump regenerates by
`restore` + producing forward, DISCARDING frames before the block-aligned
target offset (checkpoint resumes are hop-aligned, encoded offsets are
block-aligned — the discard bridges the two). Re-served packets must be
byte-identical to their first serving.

Rate-envelope caveat (Step 2 discovery, recorded in the feature progress
Deviations): 25%/50% require a full-entry loop context or the stretcher fails
`NoCandidate`. The synthetic fixtures carry full-entry loops (the production
shape), so all six rates are replayable; do not build interior-loop replay
cells at 25/50.

## Reference Documentation

**Required:**
- Design: `.agents/planning/2026-08-08-song-rate-streaming/design/detailed-design.md`
  (reqs 12–14, 16, 19–20; Testing Strategy §"Synthetic engine replay";
  Appendix: the read-pattern facts)

**Additional References (if relevant to this task):**
- `docs/xact_streaming_research.md` — packet sizes, header read, sequential
  block-align-rounded reads, loop-seek behavior
- `.agents/planning/2026-08-08-song-rate-streaming/research/streaming-mechanism.md`
  — design implications distilled from the RE (loop restart, serving shape)
- `.agents/planning/2026-08-08-song-rate-streaming/implementation/step02-task-02-resumable-stretch-state/progress.md`
  — checkpoint semantics + the NoCandidate envelope

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. A test-side replay pump in `src/core/xact/tests.rs` (approved at
   breakdown: NOT crate code — the production producer is Step 4's) that
   issues the engine's read sequence against a virtual bank: 0x1000 header
   read at 0, then per streamed entry sequential 64 KiB packets rounded to
   block-align boundaries, EOF-clamped, reassembling every returned byte at
   its virtual offset.
2. A per-entry encoded feed: `adpcm::BlockCachePcm` over the source entry →
   `StretchState::produce` → whole-block accumulation → `adpcm::encode_block`,
   serving arbitrary in-order encoded-byte offsets; feed length must equal the
   plan's `data_len` exactly (whole output blocks).
3. Serving goes through `VirtualBankLayout::resolve` region by region
   (pre-data bytes from the synthesized block, entry bytes from the feeds,
   zeros for gaps) — the same surface Step 4's detour will use.
4. Byte-equality proof: the reassembled virtual file is byte-identical to the
   whole-buffer oracle (parse → plan → decode_interleaved →
   stretch_interleaved → encode_interleaved → write_song_bank_streaming — the
   validator's `transform_bank` composition, rebuilt as a test helper), AND it
   re-parses via `parse_song_bank` with both entries decoding to the
   reference's stretched PCM.
5. Loop-restart replay: after serving past the stretched loop end, jump back
   to the block containing the stretched loop start and re-serve forward;
   bytes are identical to the first serving, produced via
   `StretchCheckpoint`/`restore` + discard-to-block-boundary (never by
   retaining the whole first-pass output).
6. Matrix: rates {50, 175} at minimum (the step-demo rates) plus one slow and
   the identity rate ({25, 100} recommended), each × both physical entry
   orders, on synthetic full-entry-loop fixtures.
7. Tests run through the validator harness's cargo-test phase; keep debug
   runtime reasonable (trim the matrix before letting the phase exceed ~30 s).

## Dependencies

- `task-01-virtual-bank-layout-and-resolve` (the layout + resolve surface).
- Step 2's `StretchState`, `StretchCheckpoint`, `adpcm::encode_block`,
  `adpcm::BlockCachePcm` (complete).

## Implementation Approach

1. Build the oracle helper first (whole-buffer transform to bytes) and pin it
   against `write_song_bank_streaming` on one cell.
2. Build the encoded feed; unit-pin it: concatenated feed bytes ==
   the oracle's encoded entry payload for the same plan.
3. Build the pump (header read + packet loop + EOF clamp) over `resolve`;
   assert reassembly equality, reparse, and decode equality across the
   matrix.
4. Add the loop-restart leg (checkpoint capture during first pass, restore +
   discard on the jump, byte-compare the re-served window).
5. Record progress in
   `.agents/planning/2026-08-08-song-rate-streaming/implementation/` (repo
   convention: NEVER `.agents/scratchpad/`); run the full gate set.

## Acceptance Criteria

1. **Replayed bank equals the oracle**
   - Given a synthetic full-entry-loop bank (each physical entry order) and
     each matrix rate
   - When the pump replays the engine read pattern against the virtual bank
   - Then the reassembled bytes are byte-identical to the whole-buffer oracle
     bank, `parse_song_bank` accepts them, and both entries decode to the
     reference's stretched PCM

2. **Read-pattern fidelity**
   - Given the pump's issued reads
   - When inspected by the test
   - Then the first read is exactly 0x1000 at offset 0, data reads are
     sequential block-align-rounded 64 KiB packets with EOF clamping, and no
     read exceeds `virtual_size`

3. **Loop-restart reproduces identical bytes**
   - Given a first pass that captured a checkpoint at the stretched loop start
   - When the pump jumps back and re-serves from the loop-start block via
     restore + discard
   - Then the re-served bytes byte-match the first serving of the same range

4. **Tree is green**
   - Given the completed task
   - When running the five standing gates
   - Then all pass, with the Windows-target check at 0 warnings

## Metadata

- **Complexity**: High
- **Labels**: xact, replay, song-rate, streaming, host-validation
- **Required Skills**: Rust, XACT streaming read protocol, WSOLA streaming
  core, repository host-validator harness
- **Generated By**: code-task-generator 2026-08-09
- **Source Plan**: `.agents/planning/2026-08-08-song-rate-streaming/implementation/plan.md`
- **Plan Step**: Step 3: Build the virtual bank and the synthetic engine replay
