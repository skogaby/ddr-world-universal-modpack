# Plan: Step 1 — Resampler core

Status: Approved 2026-08-12

Implements plan Step 1 of
`.agents/planning/2026-08-12-preserve-pitch-toggle/implementation/plan.md`.
Design contract: detailed-design §Components 1 (interfaces quoted there are
authoritative; not restated here).

## Implementation shape

`src/core/xact/resample.rs`, structured as:

1. `ResampleError` enum (`InvalidChannelCount`, `InvalidFrameCounts`,
   `InvalidLoopContext { field }`, `ArithmeticOverflow`) with
   `Display`/`Error` impls in the stretch's style, plus a private
   `map_rate_error`.
2. Private core shared by both forms (this is what makes reference ≡
   streaming structural rather than tested-into-existence):
   - `PositionMap` — precomputed `step_global`, optional loop segment
     (`loop_out_start/end`, `source_start_q32`, `step_loop`); method
     `pos_q32(output_frame) -> u128` implementing the piecewise map by
     direct multiplication (no accumulator).
   - `interpolate(source, pos_q32, channel) -> i16` — i0/i1 clamp +
     `divide_half_away_i128` linear blend.
3. `resample_interleaved(...)` — validate, build `PositionMap`, loop over
   `0..output_frames` into a `Vec<i16>` (with `try_reserve`-style allocation
   guard matching the stretch's `AllocationFailed`? — the stretch reference
   uses plain Vec; follow the stretch reference exactly: plain
   `Vec::with_capacity`). Frozen once landed.
4. `ResampleState` — fields per design Data Models; `produce` fills the
   caller buffer from `next_output_frame` via the same `PositionMap`,
   returns `Produced { frames, done }`; minimum granularity one frame;
   `positioned_at(frame)` clamps to `output_frames` and sets the cursor;
   `position()` accessor.
5. Validation: channels nonzero; `source_frames`/`output_frames` nonzero;
   `output_frames ≤` Q32 overflow bound (checked arithmetic throughout);
   loop context range checks (source/output ranges nonempty and in-bounds —
   no window-length requirement, unlike the stretch).

Wiring: `pub mod resample;` in `src/core/xact/mod.rs`; add `resample.rs` to
the validation script's module-source precondition list.

## Test scenarios (all in `src/core/xact/tests.rs`; TDD — written first,
failing against the absent module)

Rate matrix throughout: percents [25, 50, 75, 125, 175] with output lengths
derived via `rate::target_for_percent` (block-quantized like production
plans), stereo unless noted.

- **T1 reference ratio tracking:** sine input via `tone_pcm`-style
  generator; zero-crossing period of the reference output ≈ source period ×
  `output_frames/source_frames` (inverse — output plays the source S/O
  faster) within a coarse tolerance; exact output length; also a mono case.
- **T2 endpoint behavior/clamps:** first output frame equals source frame 0;
  positions never panic the `SourcePcm` (bounded-access guard using the
  tests' existing `TrackingPcm`-style wrapper at 175 % where positions
  approach the source end).
- **T3 streaming ≡ reference:** `ResampleState` pulled to completion equals
  `resample_interleaved` byte-for-byte across the matrix (loop and no-loop).
- **T4 chunking independence:** pulls with capacities {1 frame, 7 frames,
  1024 frames, whole-buffer} produce identical streams; `done` flags and
  frame counts consistent; zero-capacity call returns `frames: 0` without
  state change.
- **T5 seek suffix identity:** for several targets (0, mid, block-aligned,
  last frame), `positioned_at(t)` then pull-to-end equals the uninterrupted
  stream's suffix.
- **T6 loop-seam mapping:** with a `mapped_loop`-derived context, the output
  frame at `output_start` interpolates at exactly `source_start`
  (frac = 0), and the loop segment's final frame's position is
  `< source_end` while `output_end`'s (post-loop, global) position resumes
  the global map — assert the mapped positions directly via a probe
  `SourcePcm` recording access indices, plus stream-level: a simulated
  engine loop (play to `output_end`, jump to `output_start`) is
  source-continuous (the jump's source positions are `source_end − ε` →
  `source_start`).
- **T7 error parity:** zero channels / zero frames / inverted or
  out-of-bounds loop ranges rejected with the specific `ResampleError`;
  identity (`output == source`) accepted and equal to a straight copy.

## Risks / notes

- Q32 `u128` direct multiplication `i × step` cannot overflow for any
  realistic bank (`output_frames < 2^28` enforced by the XWB duration
  ceiling upstream; `step < 2^35`) — checked arithmetic still used, mapped
  to `ArithmeticOverflow`.
- The reference is frozen after this step (oracle discipline); later steps
  may not modify it.

## Execution checklist

Tracked in `progress.md` beside this file.
