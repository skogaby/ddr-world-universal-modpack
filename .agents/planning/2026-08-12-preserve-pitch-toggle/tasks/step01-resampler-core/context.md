# Context: Step 1 — Resampler core

Task source: `.agents/planning/2026-08-12-preserve-pitch-toggle/implementation/plan.md`
Step 1 (plan `Status: Approved 2026-08-12`; design
`.agents/planning/2026-08-12-preserve-pitch-toggle/design/detailed-design.md`
`Status: Approved 2026-08-12`). Task files were skipped per user instruction
(code-task-generator bypassed) — therefore this task-level plan requires
explicit user approval before code, per the code-assist sop.

## Requirements (from design §Components 1, §Testing Strategy)

Functional:
1. New pure module `src/core/xact/resample.rs`:
   - Reference oracle `resample_interleaved(source, channels, output_frames,
     loop_context) -> Result<Vec<i16>, ResampleError>` — whole-buffer; frozen
     once landed.
   - Streaming `ResampleState` — `new(source_frames, output_frames, channels,
     loop_context)`, `produce(&mut self, source: &impl SourcePcm, out) ->
     Result<Produced, ResampleError>`, `positioned_at(output_frame)` (O(1)
     seek), `position()`.
2. Position map: piecewise Q32 fixed-point — global segments
   `pos(i) = i × step_global` (`step_global = round_half_up(S × 2^32, O)`);
   loop segment `[output_start, output_end)`:
   `pos = source_start × 2^32 + rel × step_loop`
   (`step_loop = round_half_up(loop_src_len × 2^32, loop_out_len)`).
3. Interpolation: linear, per channel, one shared phase; integer-only
   (`divide_half_away_i128` rounding); source index clamps at both ends.
4. Exact output length `output_frames`; deterministic: any `produce`
   chunking and any `positioned_at` seek reproduce the identical byte
   stream.
5. No game dependencies (host-compilable, same discipline as `stretch.rs`).

Acceptance criteria = the design's host-test list (§Testing Strategy 1–5):
reference pitch/ratio tracking + exact length + edge clamps; streaming ≡
reference across the rate matrix; chunk-size independence; seek suffix
identity; loop-seam continuity. Plus error-path validation parity.

## Existing patterns to reuse (verified in code)

- `SourcePcm` trait + `SlicePcm` (src/core/xact/stretch.rs:116-165) — panics
  on OOB by contract; reuse both (tests use `SlicePcm`).
- `Produced { frames, done }` (stretch.rs:678) — reuse the type.
- `LoopContext` (stretch.rs:37-42) — reuse.
- Rounding: `round_half_up_u128`, `divide_half_away_i128`
  (src/core/xact/rate.rs:143,153, `pub(crate)`) — reuse; map `RateError` →
  `ResampleError::ArithmeticOverflow` like stretch's `map_rate_error`.
- Error enum + `Display`/`Error` impl style: stretch.rs:54-102.
- Loop validation: stretch.rs `validate_loop_context` (range checks; the
  window-length checks don't apply — the resampler has no window).
- Test helpers in src/core/xact/tests.rs: `tone_pcm`, `mapped_loop`,
  `run_stretch_state`-style pull loops, chunking matrices
  (`streaming_stretch_chunking_is_independent` at tests.rs:822),
  suffix-identity shape (tests.rs:901).
- `rate::target_for_percent` (rate.rs:105) — use in tests to derive
  block-quantized output lengths the way production plans do.

## Build & test commands

- Type check (from repo root): `cargo check --target x86_64-pc-windows-msvc`
- Host test run: the validation harness mounts `src/core/xact/mod.rs` via
  `#[path]` into a temp package and runs `cargo test`
  (`scripts/validate_song_playback_speed.sh`, tail harness at ~line 1885;
  requires sibling `../ddr-chart-tools` — present). For fast iteration this
  task uses a minimal temp-dir harness that mounts only `core/xact` and runs
  `cargo test`, mirroring the script's tail harness.
- Logs: `.agents/planning/2026-08-12-preserve-pitch-toggle/tasks/step01-resampler-core/logs/`
- Readiness gates before handoff: `cargo check` → `cargo fmt` (whole crate)
  → (full `./build.sh` deferred to later steps; this step is pure host code
  but check must be clean for the msvc target too).

## Files to touch

- `src/core/xact/resample.rs` (new)
- `src/core/xact/mod.rs` (`pub mod resample;`)
- `src/core/xact/tests.rs` (new test block)
- `scripts/validate_song_playback_speed.sh` (add `resample.rs` to the
  module-source precondition list, ~line 93 — one word; the harness mounts
  `mod.rs` so the module and its tests compile/run automatically)

## Notes / interpretations

- The crate is `#![allow(dead_code)]` crate-wide — production consumers of
  `ResampleState` arrive in plan Step 2; no dead-code suppression needed.
- Reference-vs-streaming byte identity is guaranteed structurally: both
  compute each output frame's position with the same per-frame direct
  multiplication (no incremental accumulator drift), sharing one private
  position/interpolation helper pair.
- Linear interpolation between two i16 samples cannot leave i16 range — no
  saturation needed (documented in code).
