# Task: The Validator's Streaming Report Section (First Legs)

## Description

Extend `scripts/validate_song_playback_speed.sh` IN PLACE with the new
`streaming` report section — the durable, machine-checkable record of the
streaming core's equality proof. First legs (this step): the byte-equality
matrix and chunking-independence results, plus an informational synthetic
frames/sec throughput metric. This is plan Step 2's demo artifact; later
steps extend the same section (virtual bank + replay in Step 3, deferral/
silence-fill in Step 4).

## Background

Task-02's `#[cfg(test)]` suites already PROVE the properties in the harness's
`cargo test` phase; this task makes representative results VISIBLE in
`target/song-rate-validation/report.json` the way the retired sections once
were — the maintainer reads the report, not the test list. Per the accepted
register decision D7 there is NO schema/version discriminator: the section is
added in place, `schema` stays `song-rate-validation/v1`, and `overall_pass`
gains the `streaming.passed` conjunct. The throughput number is informational
only (the real gate is plan Step 5's live cabinet benchmark) — it must never
fail the report.

**Editing hazard (bit us in Step 1):** the harness `main.rs` heredoc at
`cat >"$TMP/src/main.rs" <<EOF` is UNQUOTED — backticks in inserted Rust
comments execute as bash command substitution, and `$` expands. Keep both out
of generated-code comments.

## Reference Documentation

**Required:**
- Design: `.agents/planning/2026-08-08-song-rate-streaming/design/detailed-design.md`
  (Testing Strategy — the in-place validator update, the streaming-section
  bullet list, and the informational throughput metric)

**Additional References (if relevant to this task):**
- `.agents/planning/2026-08-08-song-rate-streaming/implementation/step01-task-01-retire-cache-model/progress.md`
  — Deviations section (the heredoc gotcha and how Step 1 evolved the report
  in place)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. Add a `streaming` field to the harness `Report` struct with a
   `StreamingReport` carrying: `passed`, per-rate results across
   25/50/75/100/125/175 (each recording byte-equality vs the whole-buffer
   reference, chunking-independence, checkpoint-restore equality, and the
   loop-context shape exercised), an informational
   `synthetic_frames_per_second` (or equivalent) throughput figure, and a
   `checks: Vec<Check>` list consistent with the existing sections.
2. Drive the section from the release-run `main()` using the crate's real
   mounted sources (`StretchState` + `SourcePcm` + the whole-buffer
   reference) over the existing synthetic bank fixtures — no new fixture
   machinery unless a matrix cell genuinely needs one.
3. `overall_pass` conjoins `streaming.passed`; the throughput figure is
   excluded from every pass criterion.
4. The python schema-check heredoc gains a `streaming` presence + `passed` +
   all-checks-passed leg (same shape as the `identity_runtime` leg).
5. No schema/version discriminator; no changes to the surviving sections;
   the report keys stay otherwise identical to Step 1's shape.
6. Console output follows the existing `PASS/FAIL name: detail` line
   convention so a failing leg is identifiable from the terminal alone.

## Dependencies

- `task-02-resumable-stretch-state` (the section reports that task's
  machinery; it cannot exist before `StretchState` does).

## Implementation Approach

1. Mirror the structure of an existing section (`identity_runtime` is the
   closest surviving template): struct → `validate_streaming()` → `main()`
   call + assembly → python leg.
2. Keep the release-run matrix representative rather than exhaustive (the
   exhaustive matrix already runs in `cargo test`): all six rates, one
   interior-loop and one no-loop context, stereo; one chunking-independence
   comparison per rate; one checkpoint-restore leg.
3. Measure throughput with the harness's existing `measure_operation`
   helper over a fixed-size stretch; record, never gate.
4. Full standing gates; verify the report by inspection
   (`python3 -c ...` over `report.json`: `streaming` present, no
   `cache`/`on_demand`, `overall_pass` true).
5. Record progress in the planning dir (never `.agents/scratchpad/`); tick
   plan Step 2's checklist item once this task lands green and both sibling
   tasks carry `Status: Complete`.

## Acceptance Criteria

1. **Streaming section present and green**
   - Given a validator run on the completed step
   - When inspecting `target/song-rate-validation/report.json`
   - Then a `streaming` section exists with per-rate equality +
     chunking-independence + checkpoint-restore results across all six rates,
     `streaming.passed` is true, `overall_pass` conjoins it, and `schema` is
     still `song-rate-validation/v1`

2. **Throughput is informational**
   - Given the streaming section's throughput figure
   - When any pass criterion is evaluated
   - Then the figure influences none of them (verified by inspection of the
     `passed` expressions)

3. **Python gate extended**
   - Given a report missing the `streaming` section or carrying a failing
     streaming check
   - When the script's python schema check runs
   - Then it exits nonzero (spot-check by temporary mutation during
     development, not committed)

4. **Step demo holds**
   - Given the completed step
   - When the maintainer reads the validator output
   - Then the new section shows equality and chunking-independence results
     across the rate matrix (plan Step 2's demo statement)

5. **Tree is green**
   - Given the completed task
   - When running the five standing gates
   - Then all pass, with the Windows-target check at 0 warnings

## Metadata

- **Complexity**: Medium
- **Labels**: validation, reporting, song-rate, streaming
- **Required Skills**: Rust, bash heredoc harness editing, repository
  host-validator harness
- **Generated By**: code-task-generator 2026-08-09
- **Source Plan**: `.agents/planning/2026-08-08-song-rate-streaming/implementation/plan.md`
- **Plan Step**: Step 2: Build the streaming WSOLA core with byte-equality proof
