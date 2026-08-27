# Task: Implement Deterministic Rate Stretch

## Description
Implement exact song-rate arithmetic and the deterministic joint-stereo
WSOLA-like time stretcher defined by the approved design. The implementation
must preserve pitch, use one source offset for both channels, emit an exact frame
count, and remain host-testable without loading game code.

## Background
Pitch preservation is the only missing DSP stage between the shared XWB/ADPCM
foundation and a transformed song bank. Requested percentages cannot directly
drive the game clock because output duration is quantized to whole ADPCM blocks.
This task therefore owns both the reduced exact `RateRatio` model and the fixed,
fully deterministic stretch algorithm that produces the target frame count.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-08-05-song-playback-speed/design/detailed-design.md`

**Additional References (if relevant to this task):**
- `.agents/planning/2026-08-05-song-playback-speed/research/pitch-preservation.md`
- Sibling repository `itgmania`: `src/RageSoundReader_SpeedChange.cpp` as algorithmic prior art only
- `src/core/xact/` from Task 1

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Implement `RateRatio` and checked half-up block-target arithmetic for 75%, 100% test identity, and 125%, including reduced fractions, Q31 conversion, content-to-wall conversion, signed half-away rounding, and final i32 saturation.
2. Implement the exact approved window, synthesis-hop, match-length, search-radius, Q32 phase, candidate-bound, joint-channel SAD, tie-break, and fixed-point overlap formulas.
3. Use one chosen offset for every channel; anti-phase and asymmetric stereo must not collapse to a mono-only correlation signal.
4. Anchor source frame zero and the final logical source frame, emit exactly the caller-provided output length, and never index outside source bounds.
5. Support cyclic context at an explicit loop boundary and expose enough data for the transformer to validate seam continuity.
6. Reject inputs too short for the approved window/search profile rather than inventing a different algorithm.
7. Use checked arithmetic and `try_reserve_exact` for large allocations; report typed failures rather than depending on panic recovery.
8. Keep output deterministic across repeated runs and independent of platform floating-point behavior by using the specified integer/fixed-point operations.
9. Add focused tests in the same modules; do not defer DSP/rate tests to the host-validator task.
10. Update canonical `progress.md` after implementation and validation.

## Dependencies
- Task 1 shared XACT format/codec types and test conventions.
- Approved DSP/rate formulas in the detailed design.
- No runtime cache, LayeredFS, XACT, or game-hook dependency.

## Implementation Approach
1. Implement and test `RateRatio`, target block/frame selection, Q31, and signed conversions.
2. Add fixed integer helpers for half-up and half-away rounding with overflow checks.
3. Implement the WSOLA state machine over interleaved frames using exact approved parameters.
4. Add terminal anchoring and cyclic-loop context without changing normal-window behavior.
5. Build deterministic synthetic signal fixtures for pitch, stereo, transients, silence, and boundaries.
6. Measure and report frequency, clipping, seam, and exact-length results; run repository build gates and update progress.

## Acceptance Criteria

1. **Exact Rate Authority**
   - Given a logical source frame count, ADPCM block size, and a supported percentage
   - When target blocks and `RateRatio` are computed
   - Then the target uses checked half-up arithmetic, is block-aligned/nonzero, and every Q31/content-time result matches the reference vectors

2. **Pitch-Preserved Duration Change**
   - Given stereo sine fixtures at 75% and 125%
   - When the stretcher emits the exact requested output frames
   - Then measured pitch error is at most 0.25%, output length is exact, and no new clipping occurs for the -6 dBFS corpus

3. **Stereo Coherence**
   - Given identical, anti-phase, and asymmetric stereo fixtures
   - When candidate matching runs
   - Then one deterministic source offset is applied to both channels and no inter-channel sample lag is introduced

4. **Deterministic Boundaries**
   - Given silence, impulses, short-invalid inputs, terminal windows, and equal-score candidates
   - When stretching runs repeatedly
   - Then valid inputs are byte-identical across runs, tie-breaking follows the design, endpoints remain anchored, and invalid inputs return typed errors

5. **Loop Context**
   - Given a looped synthetic preview with an explicit half-open loop range
   - When cyclic stretch context is used at the mapped loop boundary
   - Then the mapped loop remains valid and the generated seam stays within the approved discontinuity threshold

6. **Build Readiness**
   - Given the completed rate and stretch modules
   - When their tests and the repository check/format/release build gates run
   - Then all pass and canonical `progress.md` identifies Task 3 as the next action

## Metadata
- **Complexity**: High
- **Labels**: rust, audio-dsp, wsola, fixed-point, stereo, step-1
- **Required Skills**: code-assist, verification, self-documenting-code
- **Generated By**: code-task-generator 2026-08-05
- **Source Plan**: `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md`
- **Plan Step**: Step 1: Build the deterministic host audio pipeline
