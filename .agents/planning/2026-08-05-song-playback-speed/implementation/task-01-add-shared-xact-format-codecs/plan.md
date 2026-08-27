# Task 1 Plan: Shared XACT Format Codecs

Status: Approved 2026-08-05 (inherits the approved generated task and source design)

## Test-Driven Sequence

1. Add synthetic XWB fixture tests before parser code.
   - Valid inputs: main/preview and preview/main physical order, distinct sample rates, exact blocks, and the approved stock tail variants.
   - Expected: borrowed names/data, preserved order/identity, logical-duration decode trimming, and exact field exposure.
   - Invalid inputs: magic/version/header, all segment framing fields, bank flags/count/name/element sizes/alignment/compact format, entry names/formats/flags/duration/loops, ranges/overlap/alignment, partial-tail shape, and complete-block equation.
2. Implement the minimal checked reader, strict parser, and typed errors needed to pass those tests.
3. Add codec tests before codec implementation.
   - Mono/stereo deterministic byte identity, channel ordering, silence, malformed predictor, incomplete frame/block rejection, exact output size, and at least 30 dB sine SNR.
   - Add stock-tail decode tests that trim to logical duration and never consume the partial tail.
4. Port/adapt direct interleaved block encode and arbitrary-channel decode to pass the codec tests without padding or per-song channel duplication.
5. Add serializer tests before serializer implementation.
   - Rebuild both physical orders with replacement payloads and verify names/order, bank metadata, format/sample rates, duration/loops, aligned ranges, complete blocks, and deterministic bytes after reparsing.
6. Implement checked serializer layout and writes.
7. Prove shared mono output is byte-identical to the existing Assist Tick and sibling encoders, then migrate only Assist Tick's codec primitive calls.
8. Run host tests, Assist Tick validation, check, format, release build, and record evidence in the canonical feature `progress.md`.

## Design Choices

- Keep `src/core/xact/` standard-library-only so the module can be compiled directly by host validation.
- Parse into fixed two-entry borrowed views because any other entry count is outside the approved profile.
- Keep serializer identity-preserving by accepting only payload/duration/loop replacements, not replacement names or formats.
- Reject partial PCM input rather than padding; callers must choose exact block-aligned frame counts explicitly.
- Use checked `usize` arithmetic for every file range and output layout conversion.

## Risks

- Stock tail terminology is easy to invert: allowed remainders are `block_align - 1` and `block_align - 2`, not one or two bytes after a complete block.
- The nibble stream is frame-major across channels; changing packing order would preserve sizes while corrupting stereo.
- Assist Tick's standalone validator must include the shared module explicitly after migration.
