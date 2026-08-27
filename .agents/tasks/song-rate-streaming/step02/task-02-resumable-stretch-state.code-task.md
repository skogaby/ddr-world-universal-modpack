# Task: The Resumable StretchState with Byte-Equality Proof

## Description

Reformulate the WSOLA stretcher as a resumable streaming state machine:
`StretchState` in `src/core/xact/stretch.rs` with `new`/`produce`/
`checkpoint`/`restore`, proven **byte-identical** to the untouched
whole-buffer `stretch_interleaved_with` for every input. This is the heart of
plan Step 2 (design reqs 19–20) — it preserves all shipped DSP evidence
(pitch preservation, determinism, exact output length, seam behavior) while
making incremental generation possible.

## Background

The whole-buffer core is already a left-to-right sliding-window process
(orientation §"The streamability finding"): an output-driven main loop whose
per-step state is tiny — a Q32.32 phase accumulator, `output_start`, the
previous selected `SourceWindow`, and counters. Four things block
incrementality today, all reformulable without changing any computed sample:

1. The whole-buffer API (`&[i16]` in, `Vec<i16>` out) — becomes
   produce-into-caller-buffer over a `SourcePcm` view (previous task).
2. The identity shortcut (`output_frames == source_frames` memcpys the whole
   buffer) — becomes an incremental copy path with identical bytes.
3. The terminal end-anchor region: output `[output_frames − window,
   output_frames)` is written by a special non-hop terminal placement —
   becomes a distinct final-region code path emitted as one final run.
4. The diagnostics vectors (`selected_source_starts` etc.) assume whole runs —
   `StretchState` carries counters (`clipped_samples`, `cyclic_windows`), not
   vectors; the reference keeps its full `StretchResult`.

**The finalization subtlety (load-bearing):** each main-loop step writes a
full `window` of frames at `output_start`, but only the first
`synthesis_hop` of them are final — the tail is provisional until the NEXT
step blends over it (and the terminal placement blends over
`[terminal_output_start, terminal_output_start + hop)`). `produce` must emit
only finalized frames and hold the provisional tail back internally, or
chunked output will differ from the reference.

## Reference Documentation

**Required:**
- Design: `.agents/planning/2026-08-08-song-rate-streaming/design/detailed-design.md`
  (§`core::xact::stretch — streaming WSOLA state machine` incl. the API
  sketch; reqs 19–20; Testing Strategy §"Streaming stretcher")

**Additional References (if relevant to this task):**
- `.agents/planning/2026-08-08-song-rate-streaming/research/orientation.md` —
  §"The streamability finding" (the blocker list this task resolves)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. Implement `StretchState` per the design's component sketch:
   `new(source_frames, output_frames, channels, sample_rate, loop_context)`,
   `produce(&mut self, source: &impl SourcePcm, out: &mut [i16]) -> Produced`,
   `checkpoint(&self) -> StretchCheckpoint` (~5 words), and
   `restore(checkpoint, ...)`. Exact signatures may vary where the design
   sketch is elided (`Produced`'s shape, `restore`'s parameters) — behavior
   may not.
2. `stretch_interleaved_with` / `stretch_interleaved` are NOT modified in any
   observable way: they are the test oracle (and Step 3's replay reference).
   Refactoring shared helpers out for reuse is fine ONLY if the reference's
   outputs stay bit-identical and its suite passes unmodified.
3. Byte equality with the reference for every input, including the identity
   shortcut, loop remapping/cyclic windows, and the terminal non-hop end
   anchor. Same validation failures too (`StretchError` parity for invalid
   channel counts, too-short inputs, invalid loop contexts).
4. Chunking independence: any sequence of `produce` calls (varying `out`
   capacities, down to the documented minimum granularity — the design notes
   emission in whole synthesis hops, with the terminal region as one final
   run) yields identical bytes; document the minimum and reject/handle
   too-small buffers explicitly rather than silently stalling.
5. Bounded source access: any single `produce` call touches only
   `[nominal − radius, nominal + radius + window)` of the source (the
   property the block-cache view is sized for) — assert it in tests via an
   instrumented `SourcePcm` wrapper.
6. Checkpoint at the stretched loop start of a looped entry: restoring it and
   producing forward reproduces the identical output suffix (the loop-restart
   /Quick-Restart regeneration primitive, design req 20). Restoring the
   zero checkpoint reproduces the identical whole run.
7. Counters (`clipped_samples`, `cyclic_windows`) match the reference's
   values at end of run.
8. Tests live host-side and run through the validator harness's `cargo test`
   phase.

## Dependencies

- `task-01-block-codec-and-source-pcm` (the `SourcePcm` trait + block-cache
  view this task consumes).

## Implementation Approach

1. Write the equality test harness FIRST (reference vs a
   run-`StretchState`-to-completion helper) over a small matrix, watch it
   fail against a stub, then port the main loop stage by stage: first-window
   copy → main loop → terminal region → identity shortcut.
2. Expand the matrix: rates 25/50/75/100/125/175 × loop contexts
   (none/interior/boundary-clamped) × channels 1/2/6 × short/boundary
   inputs (window+hop minimum, one-hop-over, non-multiple lengths).
3. Add chunking-independence (randomized and adversarial chunk sizes),
   checkpoint/restore, and bounded-access instrumentation tests.
4. Full standing gates; record progress in the planning dir (never
   `.agents/scratchpad/`).

## Acceptance Criteria

1. **Byte equality across the matrix**
   - Given every (rate, loop-context, channel-count, input-shape) cell in the
     test matrix
   - When `StretchState` produces the full output over a `SourcePcm` view
   - Then the bytes equal `stretch_interleaved_with`'s output exactly, and
     end-of-run counters match

2. **Chunking independence**
   - Given the same input stretched with different `produce`-call chunkings
   - When the emitted runs are concatenated
   - Then all chunkings yield identical bytes, equal to the reference

3. **Checkpoint/restore reproduces suffixes**
   - Given a looped stretch checkpointed at the stretched loop start
   - When a fresh state is restored from the checkpoint and produced forward
   - Then the suffix is byte-identical to the uninterrupted run's suffix

4. **Bounded source access**
   - Given an instrumented `SourcePcm` recording the accessed frame range
   - When any single `produce` call runs
   - Then accesses stay within `[nominal − radius, nominal + radius + window)`

5. **Reference untouched**
   - Given the existing stretch test suite and the validator's synthetic/
     corpus DSP sections (which consume the whole-buffer reference)
   - When the validator runs
   - Then they pass without modification

6. **Tree is green**
   - Given the completed task
   - When running the five standing gates
   - Then all pass, with the Windows-target check at 0 warnings

## Metadata

- **Complexity**: High
- **Labels**: dsp, wsola, song-rate, streaming, host-validation
- **Required Skills**: Rust, WSOLA/overlap-add DSP, fixed-point arithmetic,
  repository host-validator harness
- **Generated By**: code-task-generator 2026-08-09
- **Source Plan**: `.agents/planning/2026-08-08-song-rate-streaming/implementation/plan.md`
- **Plan Step**: Step 2: Build the streaming WSOLA core with byte-equality proof
