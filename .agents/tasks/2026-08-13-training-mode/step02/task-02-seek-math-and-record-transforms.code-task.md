# Task: Seek math + record transforms (pure layer)

## Description
The host-testable half of seek-to-T: block quantization of the seek target,
the back-dated clock-anchor value, and the judge-record transforms — the
rebuild-at-playhead-T expectations and the spanning-freeze neutralization
pass — implemented as pure functions over synthetic note/record layouts.
Restructure `src/services/song_reset.rs` into a `song_reset/` directory so
the pure module is mountable in the host-test harness.

## Background
The seek playhead lives in the raw-ms domain (research §3.1). The clock is
one anchor subtraction: seek-to-T = broadcast `0x1044 {now_tick − wall(T_q)}`
where `T_q` is T quantized to the source ADPCM block grid FIRST (keeps
chart clock, claps, and audio mutually exact) and `wall()` is the existing
`song_rate::tick_domain` content→wall conversion (identity ⇒ 1:1). The
engine's own rebuild worker consumes a playhead: pre-T taps/heads become
consumed (grade 0/6), armed markers are playhead-independent, and kind-2
freeze-end markers behind T back-patch their head's hold progress
(research §3.2). A freeze SPANNING T (head < T < end) is rebuilt live; R14
requires it neutralized instead — copy the per-panel durations into the
head record's hold progress and mark the end record consumed, mirroring
the engine's own pre-T treatment (research §3.3). The note stride is 0x60
(kind byte, `+0x04` display / `+0x08` raw-ms, per-panel durations `+0x3C..`),
the record stride 0x40 (`judgedAt`, `grade`, hold progress `+0x14..`,
wobble sentinel).

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-13-training-mode/design/detailed-design.md (§4.4, §7)

**Additional References (if relevant to this task):**
- docs/training_mode_research.md §3 (note domains, rebuild semantics, freeze pass), §6 (anchor math)
- src/services/song_rate/tick_domain.rs (the content→wall conversion the anchor math rides)
- src/services/song_reset.rs (the module being restructured; its offset tables)
- src/services/stage_records.rs (the repo's fail-closed raw-layout decode pattern)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Restructure `src/services/song_reset.rs` → `src/services/song_reset/`
   (`mod.rs` verbatim-in-behavior) with a new pure submodule (e.g.
   `seek.rs`) carrying: T→T_q block quantization (given the block grid
   parameters), the anchor value computation (via a `RateSnapshot`-driven
   conversion — identity and rate cases), and the record-transform
   planners below. No logging, no game reads — host-mountable.
2. Rebuild-at-T expectation model: given synthetic 0x60-stride note bytes
   and a playhead, compute the expected per-record consumed/pending/armed
   state (the oracle task-03 verifies the engine's rebuild against, and
   the basis for the neutralization walker).
3. Spanning-freeze neutralization planner: given the note vector and the
   rebuilt 0x40-stride record vector, identify every freeze whose
   `head +0x08 < T_q < end +0x08` and emit the writes — per-panel durations
   into the head record's hold progress, end record marked consumed —
   as pure (offset, value) outputs the engine-facing caller applies.
4. Layout constants shared with the existing song_reset offset tables —
   one definition, no duplicated magic numbers.
5. Host tests alongside (harness-mounted): quantization edges (block
   boundaries, 0, past-end), anchor values at identity and at rate
   (exactness against `tick_domain`), neutralization against synthetic
   vectors covering: no freezes, freeze fully before T, freeze spanning T
   (single and multi-panel), freeze after T, back-to-back freezes.

## Dependencies
- None new (tick_domain ships; independent of task-01).

## Implementation Approach
1. Directory conversion first (pure mechanical, existing tests/build green).
2. Pure seek module + tests (TDD).
3. Keep the public `request_reset` surface untouched — task-03 consumes.

## Acceptance Criteria

1. **Block quantization**
   - Given block grid parameters and seek targets at/near block boundaries
   - When T_q is computed
   - Then it lands exactly on the grid (floor), with 0 and past-source-end handled per the design's clamp inputs
2. **Anchor value exactness**
   - Given a now-tick, a T_q, and identity / non-identity `RateSnapshot`s
   - When the anchor is computed
   - Then it equals `now − content_to_wall_ms(T_q)` with the identity case bit-identical to the legacy arithmetic
3. **Spanning-freeze neutralization**
   - Given synthetic note/record vectors with a freeze spanning T_q
   - When the planner runs
   - Then it emits exactly the engine-mirroring writes (durations → head hold progress, end record consumed) and nothing for non-spanning freezes
4. **Restructure is behavior-neutral**
   - Given the existing song_reset consumers and tests
   - When the crate builds and the suite runs
   - Then everything passes with no call-site changes beyond the module path

## Metadata
- **Complexity**: Medium
- **Labels**: song-reset, seek, pure-layer, host-tested
- **Required Skills**: Rust, the repo's raw-layout decode conventions, tick_domain
- **Generated By**: code-task-generator 2026-08-13
- **Source Plan**: .agents/planning/2026-08-13-training-mode/implementation/plan.md
- **Plan Step**: Step 2: Seek-to-T in song_reset + A/B gestures + restart-from-A
