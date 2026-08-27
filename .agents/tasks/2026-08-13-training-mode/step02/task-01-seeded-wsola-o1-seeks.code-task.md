# Task: Seeded WSOLA — O(1) shift seeks in pitch-preserved mode

## Description
Make `shift_blocks > 0` mappings on a pitch-preserved (WSOLA) Stretch
binding O(1): instead of positioning the canonical whole-song stretch via
checkpoint-restore + produce-and-discard (live-measured ~25 s for a 60 s
pre-shift at 90 %), construct a FRESH stretch seeded at the shift-mapped
source position, targeting exactly `output_total − shift` frames. This is
the maintainer-decided design §4.5 amendment (2026-08-13): frame count and
duration exact by construction; byte-level alignment deliberately unpinned
across mapping epochs.

## Background
WSOLA is sequential — each output window's source position comes from a
similarity search anchored on the previous window's landing
(`StretchCheckpoint.previous_start`), so the canonical stream's bytes at
output P require the full alignment chain up to P. A seek always rides a
cue stop/replay discontinuity, so cross-epoch byte differences are
inaudible. WITHIN one mapping epoch the fresh-seeded run becomes the
deterministic byte authority: the engine's re-reads and any behind-window
regeneration must reproduce ITS bytes — which also means the generator's
cross-epoch checkpoints (currently retained across mapping changes as
"still valid for the same stretched stream") MUST be invalidated on every
epoch change. Mapping `{0, 0}` remains the canonical stream (Quick Restart
and all shipped behavior unchanged); resample mode keeps its exact
positional seeks; identity passthrough is untouched.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-13-training-mode/design/detailed-design.md (§4.5 incl. the 2026-08-13 amendment, §7)

**Additional References (if relevant to this task):**
- docs/training_mode_research.md §5.2 (correction + decision note)
- src/core/xact/stretch.rs (StretchState, StretchCheckpoint, the whole-buffer reference `stretch_interleaved`)
- src/services/song_rate/generator.rs (Feed, ensure_feed_at, the mapping-epoch pickup in `step()`)
- src/services/song_rate/generator_tests.rs (`stretch_mapping_change_reserves_the_remapped_stream` — its oracle changes with this task; `transform_bank_oracle_mode` pattern)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `stretch.rs`: a seeded construction path — given the whole entry's
   `(source_frames, output_frames)` plan and a block-aligned seek output
   frame `S`, produce output frames `[S, output_frames)` from the
   shift-mapped source position, emitting EXACTLY `output_frames − S`
   frames (the existing exact-count contract, applied to the tail).
   Source-position mapping uses the codebase's established half-up
   boundary conventions. Loop context: `None` for seeded runs (training
   seeks play linearly; the loop-restart anchor belongs to the canonical
   `{0,0}` stream only).
2. A frozen whole-buffer seeded reference (alongside
   `stretch_interleaved`) — the independent byte authority the streaming
   seeded run is validated against.
3. `generator.rs`: on a mapping-epoch change in WSOLA mode with
   `shift > 0`, construct the seeded feed (O(1) — no produce-and-discard);
   `shift == 0` keeps the canonical `Feed::new` path. Checkpoints are
   INVALIDATED on every epoch change. Within-epoch behind-window
   regeneration reproduces the CURRENT run's bytes (existing
   checkpoint/`positioned_at` machinery operating on the seeded run).
4. Resample mode: unchanged (its `positioned_at` is exact and O(1));
   identity passthrough: unchanged.
5. Frame-count/duration invariants: the virtual layout never changes; the
   seeded run's emission accounting must line up with the mapped serving's
   content window (`stream_len − shift_bytes` bytes, then the silent tail).
6. Update the step-01 stretch-mapping test's oracle from
   canonical-stream-slice to the seeded reference; keep the `{0,0}` and
   resample-mode byte-identity pins green and unchanged.

## Dependencies
- Step 1 complete (mapping storage, epoch handshake, mapped produce_chunk).

## Implementation Approach
1. Pure layer first: seeded constructor + seeded whole-buffer reference +
   host tests (frame-count exactness, determinism, chunked-streaming
   equivalence to the reference).
2. Generator integration: epoch pickup branches on `shift > 0` + WSOLA;
   checkpoint invalidation; within-epoch regen tests with a shrunken ring.
3. Re-anchor the existing mapping-change test's expected bytes.

## Acceptance Criteria

1. **Frame-exact seeded tail**
   - Given a planned entry `(source_frames, output_frames)` and a block-aligned seek frame `S`
   - When the seeded run produces to completion
   - Then it emits exactly `output_frames − S` frames (whole blocks), for several `S` values including block boundaries and the final block
2. **Streaming equals the seeded reference**
   - Given a Stretch binding at 50 % (and one other rate) with a mapping `{shift > 0, lead}`
   - When the full virtual file is served through the serve dispatch (deferrals pumped)
   - Then the main-entry content region is byte-identical to the seeded whole-buffer reference (lead silent blocks ++ seeded tail ++ silent fill), and pre-data/side entry are untouched
3. **O(1) seeding**
   - Given a mapping change to a deep shift (e.g. most of the fixture)
   - When the generator processes the epoch
   - Then the seeded feed starts WITHOUT producing/discarding the pre-shift alignment chain (assert via produced-frames metrics or an equivalent observable)
4. **Epoch checkpoint invalidation**
   - Given a canonical run that captured a checkpoint, then a mapping change, then a behind-window regeneration within the new epoch
   - When the regenerated bytes are re-served
   - Then they equal the CURRENT (seeded) run's earlier bytes — never the previous epoch's
5. **Shipped behavior unregressed**
   - Given mapping `{0, 0}`, resample mode, and identity bindings
   - When the existing suites run
   - Then every pre-existing test passes unchanged (except the one oracle re-anchor named above)

## Metadata
- **Complexity**: High
- **Labels**: song-rate, audio, wsola, dsp, host-tested
- **Required Skills**: Rust, the repo's stretch/generator architecture, WSOLA
- **Generated By**: code-task-generator 2026-08-13
- **Source Plan**: .agents/planning/2026-08-13-training-mode/implementation/plan.md
- **Plan Step**: Step 2: Seek-to-T in song_reset + A/B gestures + restart-from-A
