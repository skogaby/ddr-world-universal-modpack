# Plan — Step 2 task-02: The Resumable StretchState with Byte-Equality Proof

Status: Approved 2026-08-09 (via the maintainer-approved Step 2 task breakdown;
Source Plan and design `Status: Approved 2026-08-08` — verified in context.md.
Auto mode per handoff instruction.)

## Design (implementation shape)

All in `src/core/xact/stretch.rs`, BELOW the untouched reference.

```text
StretchState {
  parameters, source_frames, output_frames, channels, loop_context,
  phase_step, phase, output_start,          // main-event bookkeeping (reference-identical)
  previous: SourceWindow,
  cyclic_windows, clipped_samples,
  stage: Identity | FirstCopy | Main | Terminal | Done,
  emit_frame,        // absolute frame index of buffer[0]; below = handed out
  finalized_frame,   // absolute frame index up to which output is final
  buffer: Vec<i16>,  // frames [emit_frame, ..); bounded ≈ 2W frames
}
```

- `new`: reference-order validation (channels → parameters → SourceTooShort →
  OutputTooShort → loop context); stage = Identity iff O == S; else FirstCopy
  with phase_step computed eagerly (reference parity: identity never computes
  it).
- `produce(source, out)`: assert source totals; reject `out.len() < channels`;
  loop { drain finalized frames into out; return when out full or all
  emitted; else run ONE event via `advance` }. Every event finalizes ≥ 1
  frame, so no stall is possible.
- `advance` events (exact ports of the reference arithmetic, reading through
  SourcePcm and writing into the buffer at `abs_frame − emit_frame`):
  - Identity: copy the next run of source frames (O == S ⇒ 1:1).
  - FirstCopy: window {0} copied to [0, W); finalized = H; stage = Main if
    H < T else Terminal.
  - Main at p = output_start: nominal via reused pure helpers
    (`q32_to_frame`, `nominal_for_output`); candidate search + joint SAD via
    new SourcePcm mirrors; cyclic_windows bump via reused `window_wraps`;
    overlap-write (blend [p, p+H) against buffer content, direct-copy
    [p+H, p+W), i128 blend + `divide_half_away_i128` + clip count — exact
    mirror); phase += phase_step, output_start += H; finalized =
    min(output_start, T); stage = Main/Terminal accordingly.
  - Terminal: window {S−W} overlap-write at T (blend reads retained buffer —
    always present: last write end ≥ T + H); finalized = O; stage = Done.
- `checkpoint() -> Option<StretchCheckpoint>`: Identity ⇒ {resume =
  finalized_frame}; FirstCopy ⇒ zero checkpoint; Main ⇒ {resume =
  output_start (== finalized_frame), previous, counters}; Terminal/Done ⇒
  None. 5 fields; phase omitted (recomputed as phase_step·(resume/H)).
- `restore(chk, …, source)`: validate like `new`; resume == 0 ⇒ fresh state;
  Identity ⇒ cursor = resume (≤ O); Main ⇒ validate resume hop-aligned in
  [H, T), previous window in range (cyclic ⇒ loop context present and start
  within the loop region), zero-trust shape checks →
  `InvalidCheckpoint { field }`; rebuild the provisional tail [resume,
  resume+H) from `previous` frames [H, W); emission resumes at resume_frame.
- New public surface: `Produced { frames, done }`, `StretchCheckpoint` +
  `resume_frame()`, accessors `clipped_samples()`/`cyclic_windows()`,
  `StretchError::InvalidCheckpoint { field }` (+ Display arm).

## Test scenarios (written first; initial failure = equality asserts fail
against a stub whose produce emits zeros)

All in `src/core/xact/tests.rs`. Shared helper `run_stretch_state(source,
channels, sample_rate, output_frames, loop, chunk_iter) -> Result<(samples,
clipped, cyclic), StretchError>` driving produce-to-completion through
`SlicePcm`, honoring a chunk-size pattern.

T1. `streaming_stretch_matches_reference_across_matrix` (AC1, R3, R7)
    Stereo: rates {25,50,75,100,125,175} × loops {none, interior [1000,7000),
    boundary [0,S)→[0,O)} (18 cells). Channels 1 and 6: rates {50,125} ×
    loops {none, interior} (8 cells). Each cell: reference vs streaming run
    (one whole-buffer chunk AND hop-sized chunks) — bytes, clipped, cyclic all
    equal. Includes the identity shortcut (100%) and cyclic-window cells.

T2. `streaming_stretch_boundary_shapes_and_error_parity` (R3)
    Direct output shapes (stereo, no loop, 2000-frame source): O ∈ {W+H,
    W+H+1, 2W−1, 2777}, impulse source at 2731 — bytes+counters equal.
    Error parity vs the reference for: channels 0, source < W+R, output <
    W+H, sample_rate 0, invalid loop contexts (source range, output range,
    too-short ranges), NoCandidate (337-frame source, O 512). Same variants.

T3. `streaming_stretch_chunking_is_independent` (AC2, R4)
    Cells 75%-stereo-interior and 175%-mono-none: chunk patterns {1, H−1, H,
    H+1, W+17, 997, LCG-random in 1..=W+H, single huge} all byte-equal to the
    reference; a misaligned out (len = k·C + C−1) uses the whole-frame prefix;
    `out.len() < C` → Err(OutputTooShort); done fires exactly once, total
    frames == O.

T4. `streaming_stretch_checkpoint_restore_reproduces_suffix` (AC3, R6)
    50%-stereo-interior-loop: (a) zero checkpoint → restore → full-run bytes +
    counters equal; (b) first checkpoint with resume ≥ stretched loop start →
    restore into a fresh state → produced bytes equal
    reference.samples[resume·C..], end counters equal; (c) checkpoint() is
    None after the run completes; (d) tampered resume (misaligned / ≥ T) →
    Err(InvalidCheckpoint).

T5. `streaming_stretch_source_access_is_bounded` (AC4, R5)
    Instrumented SourcePcm wrapper (Cell min/max per produce call) around the
    slice view; 75% and 175% stereo no-loop, hop-sized chunks (one event per
    call); parallel phase tracker recomputes nominal per event; assert every
    call's access range ⊆ [nominal − R − H, nominal + R + W) ∩ [0, S)
    (corrected bound per context.md; − H covers the joint-SAD reference-window
    read at fast rates).

AC5 (reference untouched) = existing suites + validator sections pass with
zero edits to the reference region. AC6 = the five standing gates.

## Steps

1. Write T1 (small matrix first: stereo 75/100/125 × none/interior) + the
   stub (`new` validates; `produce` emits zeros and completes; trivial
   checkpoint/restore) — watch equality FAIL.
2. Port stage by stage against T1: Identity → FirstCopy → Main → Terminal.
3. Expand T1 to the full matrix; add T2.
4. Add T3 (chunking), T4 (checkpoint/restore), T5 (bounded access) and the
   remaining API surface they exercise.
5. Full gate set; record progress.

## Risks

- Blend-input provenance error would show as off-by-a-hop divergence — caught
  by T1 immediately.
- Debug-profile matrix runtime: trimmed channel dimension keeps the validator
  cargo-test phase within seconds (watch and trim further if it exceeds ~30 s).
- Buffer index arithmetic: all writes go through one abs→buffer mapping
  helper; drain-from-front keeps indices stable within an event.
