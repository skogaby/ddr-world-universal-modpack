# Task: End-domain converters + chart-end threshold service surface

## Description
The pure and service-layer groundwork for Step 4 (design §4.2/§4.3, no
behavior change yet): (1) pure display⇄raw time-domain converters over the
decoded note vector in `song_reset::seek` — the game's own interpolation,
replicated; (2) the pure end-policy state machine in
`training_mode::section_math` (LOOP ON/OFF × section-end-exists →
write-thresholds / arm-loop-with-clamped-bound / natural); (3) a
`song_reset` service surface to read/stash and write the
ControlMessageActor end thresholds (`+0x94` display / `+0x98` raw, all
actors) and to expose a side's decoded note vector.

## Background
The natural song end is entirely two one-way ControlMessageActor
thresholds (research §4.1): `+0x94` = last-note-end in the DISPLAY domain
(fires `0x104A`, cascade step 4) and `+0x98` = outro in raw ms (fires
`0x104B` → GamePlayActor step 6 → banner/results). LOOP OFF's early
natural end is just writing both smaller; the `+0x94` value needs
raw→display conversion — the game's converter maps `+0x08` (raw) →
`+0x04` (display) by bracketing between consecutive notes and linearly
interpolating (design §4.2; `seek::NoteView` already decodes both fields
as `raw_time`/`display_time`).

The INVERSE converter (display→raw) is equally load-bearing, per the
approved breakdown decision: the seek gate refuses once the cascade
reaches step 4 (`request_seek`'s `CMA step < CMA_STEP_CONTENT_OVER`), and
the cascade never rewinds (research §4.3) — so LOOP ON's fire bound must
stay strictly below the `+0x94`-equivalent RAW time as well as `+0x98`,
or one late iteration permanently breaks seeking mid-grind. The clamp
margin is the existing 1000 ms end-margin class (also covers the
~150–300 ms stop/replay prepare window during which the old anchor keeps
counting).

`song_reset` already owns the actor walk (`live_dps`, `gameplay_actors`,
`control_message_child`) and the CMA offsets (`CMA_CHART_END_RAW_OFFSET`
= 0x98; add the 0x94 display sibling). Threshold writes must stash the
stock values on first write so a cleared section end can restore them
(task-02's restore-on-clear).

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-13-training-mode/design/detailed-design.md (§4.2 LOOP OFF wiring + display-domain converter, §4.3 loop bullet, §6 ladder rows "CMA vtable unresolved / thresholds unwritable")

**Additional References (if relevant to this task):**
- docs/training_mode_research.md §4 (the full end chain: threshold ctor sources, cascade one-way-ness, §4.3 loop-mode guard, §4.4 early-natural-end variant)
- src/services/song_reset/seek.rs (`NoteView` — `display_time`/`raw_time`; `decode_notes`)
- src/services/song_reset/mod.rs (`control_message_child`, `gameplay_actors`, CMA offsets, `plan_side_rebuilds`' note-vector read pattern)
- src/mods/training_mode/section_math.rs (the pure-leaf home; harness-mounted)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `seek::display_for_raw(notes: &[NoteView], raw_ms: i32) -> Option<i32>`
   — the game's converter: bracket `raw_ms` between consecutive notes'
   `raw_time`, linearly interpolate their `display_time` (design §4.2).
   Define and document the edge behavior: before the first note / past the
   last note (extrapolate from the nearest pair or clamp — match what the
   threshold write needs: a `b_ms` below the last note's end must yield a
   display value below the stock `+0x94`), empty/single-note vectors ⇒
   `None`. Pure, no allocation.
2. `seek::raw_for_display(notes, display_ms) -> Option<i32>` — the exact
   inverse (same bracketing on `display_time`, interpolate `raw_time`).
   Round-trip within interpolation slop on monotone vectors.
3. `section_math` end-policy: a pure function/enum deciding, from
   `{loop_on: bool, b_ms: i32 /* 0 = none */}`, among
   `WriteThresholds { b_ms }` (LOOP OFF + section end),
   `ArmLoop` (LOOP ON — thresholds NEVER written; the fire bound is
   computed by the caller from live threshold reads), and `Natural`
   (LOOP OFF + no section end). Mutually exclusive by construction — the
   plan's state-machine host test.
4. `song_reset` surface (windows-side, fail-closed, range-validated):
   - `chart_end_thresholds(side) -> Option<(i32 /*display*/, i32 /*raw*/)>`
     — read `+0x94`/`+0x98` off the side's CMA (sane-range checks like
     `chart_end_raw`).
   - `set_chart_end_thresholds(display_ms, raw_ms) -> bool` — write both
     fields on EVERY live actor's CMA (design §4.2 "both sides"); refuse
     (false, nothing written) when any actor's CMA is unresolvable or a
     value is out of sane range.
   - `decoded_notes(side) -> Option<Vec<seek::NoteView>>` — the side's
     note vector through the existing validated read path
     (`plan_side_rebuilds`' bounds/stride checks factored or mirrored).
5. Host tests (harness): converters against synthetic monotone note
   vectors (interpolation exactness at note points, midpoints, edge
   behavior, round-trip, degenerate vectors ⇒ None — the plan's
   "display-domain interpolation" test); the end-policy state machine's
   exclusivity table (the plan's "threshold/loop-bound mutual-exclusion"
   test).
6. No caller changes: nothing consumes the new surface yet (tasks 02/03).
   Zero behavior change — the full existing suite stays green.

## Dependencies
- Steps 1–3 shipped (seek layer, actor walk, resolution). None new.

## Implementation Approach
1. TDD the converters + end-policy in the harness (synthetic vectors).
2. Add the CMA display-threshold offset + the three service functions to
   `song_reset` (windows-side; compile-clean via cargo check).
3. Full gates (harness → check → fmt).

## Acceptance Criteria

1. **Converter exactness**
   - Given a synthetic monotone note vector with known (raw, display) pairs
   - When `display_for_raw` is evaluated at note points and midpoints
   - Then note points return the exact display values, midpoints the
     linear interpolation, and `raw_for_display` round-trips within slop
2. **Degenerate inputs refuse**
   - Given an empty or single-note vector
   - When either converter runs
   - Then it returns `None` (the caller's WARN-once natural-end ladder)
3. **End-policy exclusivity**
   - Given every `{loop_on, b_ms}` combination
   - When the end policy is computed
   - Then exactly one of WriteThresholds/ArmLoop/Natural results, LOOP ON
     never yields WriteThresholds, and LOOP OFF without a section end
     yields Natural
4. **Service surface is fail-closed**
   - Given a boot where the CMA is unresolvable (or out-of-range inputs)
   - When `set_chart_end_thresholds` is called
   - Then it returns false with NOTHING written (verified by code
     inspection + the cabinet demo's ladder leg in task-02)
5. **Zero behavior change**
   - Given the existing suites
   - When the harness runs
   - Then every pre-existing test passes unchanged

## Metadata
- **Complexity**: Medium
- **Labels**: training-mode, song-reset, pure-math, host-tested
- **Required Skills**: Rust, the seek/notes domain model, the song_reset actor walk
- **Generated By**: code-task-generator 2026-08-14
- **Source Plan**: .agents/planning/2026-08-13-training-mode/implementation/plan.md
- **Plan Step**: Step 4: LOOP SONG — loop driver + early natural end
