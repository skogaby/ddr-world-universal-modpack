# Task 2 Plan: Deterministic Rate Stretch

Status: Approved 2026-08-05 (inherits the approved generated task and source design)

## Test-Driven Sequence

1. Add rate tests before the rate module.
   - Exact 75/100/125 block targets with a divisible source.
   - Half-up target rounding, minimum-one-block clamp, 28-bit overflow, fraction reduction, Q31 vectors, positive/negative half-away conversions, and i32 saturation.
2. Implement checked integer rate helpers and `RateRatio`.
3. Add stretcher tests before the DSP module.
   - Exact output length and determinism for 75/100/125 stereo sine fixtures.
   - Pitch error <=0.25%, no clipping for -6 dBFS input, and exact first/final source anchors.
   - Identical, anti-phase, and asymmetric stereo coherence with one reported match path.
   - Silence, impulse, equal-score search, malformed interleaving, too-short input/output, and terminal non-hop-aligned output.
   - Explicit cyclic loop context with a valid mapped range, observed cyclic windows, and seam discontinuity no worse than source seam plus 2048 sample units.
4. Implement the integer WSOLA-like state machine with checked allocations and typed errors.
5. Run all host/regression/build gates and update canonical progress with Task 3 as next action.

## Implementation Shape

- `rate.rs` owns exact target and clock/time math.
- `stretch.rs` owns parameter derivation, source-window access, candidate scoring, overlap, endpoint anchoring, loop context, and diagnostics.
- `StretchResult` returns samples plus selected source starts and cyclic-window count so Task 3 can validate behavior without duplicating DSP internals.
- Linear and looped paths share one state machine; loop context only changes source-window addressing and nominal mapping inside the explicit mapped loop.

## Risks

- Terminal output positions are not necessarily hop-aligned; the forced final window must overwrite at `output_frames - window` while retaining the fixed hop-sized crossfade.
- Silent/equal-score candidates require deterministic nearest-nominal then lower-index selection.
- Signed overlap and time conversion must round away from zero without platform floating-point behavior.
