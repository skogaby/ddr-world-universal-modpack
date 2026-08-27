# Task: Assist Tick Rate Conversion and the 1200 s Capacity

## Description

Make Assist Tick correct at every supported song rate — the feature's
headline use case (D6, maintainer-overridden to REQUIRED for delivery:
slow the song down, enable assist tick, study the chart). Convert every
chart-derived content position and restart skip to wall time with the
committed generation's exact `RateRatio` (`content_to_wall_ms`); the
cabinet wall-domain `sound_offset` applies unscaled; the judgment-timing
term follows the clock stub's domain. Raise `TICK_CAPACITY_MS` from 300 s
to 1200 s wall (D15) so 300 s of chart content stays covered at the
slowest rate. REMOVE Step 4's interim scaffold gate (the
`Action::RateGated` refusal in `tick_clock`'s `AwaitAnchor` arm and its
`RateSnapshot::is_non_identity_commit()` consumer) — synthesis proceeds at
committed rates once the conversion is in. Design reqs 30–32.

## Background

The tick track is ONE pre-mixed whole-song mono waveform played as a
single cue on the music's own mixer clock (`docs/xact_audio_research.md`);
each tick's content position is
`content_ms = t_i + JUDGMENT_TIMING − SOUND_OFFSET − m0` today — all
content-domain terms. Under a committed rate the music count (m0, restart
skips) and the chart times remain CONTENT-domain, but the audible track
plays in WALL time scaled by the Q31 factor: the mix positions and skip
arithmetic must convert through the committed `RateRatio`
(`core::xact::rate::RateRatio::content_to_wall_ms` — exact integer math,
half-up). The committed rate is read from
`services::song_rate::clock_patch::snapshot()` at the gameplay-start
synthesis hand-off (the `AwaitAnchor` arm — strictly after any
loader-thread commit lands; the same site the scaffold gate occupies
today). The domain algebra (which terms scale and which do not) is settled
in the design (req 30): chart positions and restart skips convert;
`sound_offset` (wall) does not; the judgment-timing term follows the clock
stub's domain. 100% and uncommitted boots must stay BIT-IDENTICAL (the
identity ratio's conversion is exact 1:1).

`TICK_CAPACITY_MS` (`services/se_bank_synth/containers.rs`) is the
declared immortal-bank entry size; raising it to 1_200_000 keeps lazy
registration and the FR-8 graceful-truncation WARN contract unchanged
(~28.8 MB, allocated only when Assist Tick is used — D15 accepted).

## Reference Documentation

**Required:**
- Design: `.agents/planning/2026-08-08-song-rate-streaming/design/detailed-design.md`
  (reqs 30–32; §Dependent features; the D6/D15 register decisions)

**Additional References (if relevant to this task):**
- `docs/xact_audio_research.md` — the tick bank/mixer-clock model the
  conversion rides on (why sample-exact spacing survives a rate: the track
  itself is content mixed at wall positions)
- `.agents/planning/2026-08-08-song-rate-streaming/implementation/step04-task-04-io-callback-detours-and-readiness/progress.md`
  — the scaffold gate this task removes (site + predicate)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. `src/mods/assist_tick.rs`: at the `AwaitAnchor` hand-off read
   `clock_patch::snapshot()`; a committed non-identity generation supplies
   the `RateRatio` used to convert (a) every tick's mix position and
   (b) the restart-skip arithmetic (`skip = mc − m0` and the rewind
   re-anchor path) from content to wall milliseconds via
   `content_to_wall_ms`. The cabinet `sound_offset` term stays unscaled
   (wall domain); the judgment-timing term follows the clock stub's domain
   per the design. Identity/uncommitted snapshots take the literally
   unchanged existing arithmetic (bit-identical output).
2. Remove the scaffold gate: `Action::RateGated` + its log line + the
   `is_non_identity_commit()` call site in `assist_tick.rs`. KEEP
   `RateSnapshot::is_non_identity_commit()` itself (host-tested predicate;
   other consumers may arrive) unless nothing else references it — then
   removal is fine with its test.
3. `TICK_CAPACITY_MS` 300_000 → 1_200_000 in
   `src/services/se_bank_synth/containers.rs`; truncation WARN text and
   lazy-registration behavior unchanged; se-bank validator
   (`scripts/validate_se_bank_synth.sh`) updated only if it pins the
   constant's value.
4. Host tests (the tick domain algebra is pure): conversion vectors at
   exact ratios (25/50/75/125/175) for tick positions AND restart skips;
   a 100% regression vector proving placement bit-identical to today's;
   1200 s truncation boundary; synthesis-proceeds-at-committed-rate (the
   inverted scaffold expectation). Extract the position/skip math into a
   pure host-testable function if it is not already one (the mod is not
   host-mounted; the pure function is — mirror the
   `is_non_identity_commit` pattern from Step 4 task-04).

## Dependencies

- Step 4's committed-rate snapshot (`clock_patch::snapshot()`, live) and
  the exact-rate `content_to_wall_ms` (proven core — do not modify).

## Implementation Approach

1. Extract/locate the pure tick-timing math; write the conversion vectors
   RED against it (including the 100% bit-identity pin).
2. Wire the committed-ratio conversion at the AwaitAnchor hand-off and the
   restart/rewind paths; remove the scaffold gate.
3. Raise the capacity constant; run the truncation-boundary vector.
4. Full gate set; record per the repo's planning-directory convention
   (NEVER `.agents/scratchpad/`).

## Acceptance Criteria

1. **Ticks convert exactly at every rate**
   - Given a committed generation at each of 25/50/75/125/175 % and a
     chart-derived tick list
   - When the track positions and restart skips are computed
   - Then each equals the exact `content_to_wall_ms` conversion of its
     content position (vector-pinned), with `sound_offset` unscaled

2. **100% is bit-identical**
   - Given an identity commit or no commit
   - When synthesis runs
   - Then positions and skips are byte-for-byte today's values

3. **The scaffold gate is gone**
   - Given a committed non-identity generation
   - When the first judge dispatch reaches AwaitAnchor
   - Then synthesis PROCEEDS (no RateGated refusal, no gate log line)

4. **Capacity holds 1200 s**
   - Given content up to 1200 s wall (and beyond)
   - When the track is mixed
   - Then in-capacity content mixes and beyond-capacity truncates
     gracefully with the one WARN (unchanged contract)

5. **Tree is green**
   - Given the completed task
   - When running the five standing gates
   - Then all pass with the windows check at 0 warnings

## Metadata

- **Complexity**: High
- **Labels**: song-rate, assist-tick, rate-conversion, capacity,
  host-validation
- **Required Skills**: Rust, the assist-tick synthesis pipeline, exact-rate
  arithmetic, repository host-validator harness
- **Generated By**: code-task-generator 2026-08-11
- **Source Plan**: `.agents/planning/2026-08-08-song-rate-streaming/implementation/plan.md`
- **Plan Step**: Step 6: Integrate dependent features (Assist Tick, Real Speed, PUS)
