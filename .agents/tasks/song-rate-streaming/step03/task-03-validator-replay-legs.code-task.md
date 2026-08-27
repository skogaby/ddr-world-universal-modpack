# Task: The Validator's Replay Legs (Step 3 Demo)

## Description

Extend the `streaming` report section in
`scripts/validate_song_playback_speed.sh` with the plan Step 3 demo: full
synthetic replay legs at **50% and 175%** — the virtual bank served
packet-by-packet in the release run, reassembled, reparsed, decoded, and
matched against the reference — visible as per-leg rows and PASS/FAIL console
lines. Same schema (`song-rate-validation/v1`, no discriminator), same
in-place evolution pattern as Step 2's section.

## Background

Task-02's `#[cfg(test)]` replay suite already PROVES the properties
exhaustively in the harness's cargo-test phase; this task makes the two
demo-rate replays VISIBLE in `target/song-rate-validation/report.json` — the
maintainer reads the report, not the test list. The harness `main.rs` already
holds the pieces to compose: the `transform_bank` whole-buffer oracle, the
synthetic bank fixtures (`build_source`, both entry orders), `drive_streaming`,
and the `streaming` section with its `checks` list already conjoined into
`overall_pass` and gated by the python leg (any failed check in
`streaming.checks` exits nonzero — new checks are gated for free).

Per the approved breakdown, the release-run pump is a compact re-derivation
in `main.rs` (Step 2 precedent: exhaustive in tests, representative and
independently re-derived in the report), not a shared crate helper.

**Editing hazard (bit Step 1 once):** the harness `main.rs` heredoc at
`cat >"$TMP/src/main.rs" <<EOF` is UNQUOTED — backticks execute as command
substitution and dollar signs expand. Keep both out of inserted Rust.

## Reference Documentation

**Required:**
- Design: `.agents/planning/2026-08-08-song-rate-streaming/design/detailed-design.md`
  (Testing Strategy — the streaming-section bullets and the synthetic engine
  replay; Appendix: the read-pattern facts)

**Additional References (if relevant to this task):**
- `.agents/planning/2026-08-08-song-rate-streaming/implementation/step02-task-03-validator-streaming-section/progress.md`
  — how the streaming section was built in place (structs, checks, python leg)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. `StreamingReport` gains replay results (shape free; recommended: a
   `replays: Vec<...>` with per-leg percent, entry order, packet count,
   reassembled length, and booleans for reparse / byte-equality-vs-oracle /
   decode equality, plus `passed`), each also emitted as a
   `check("streaming_replay_{percent}", …)` line so a failing leg is
   identifiable from the terminal alone.
2. The release-run pump mirrors task-02's read pattern compactly: 0x1000
   header read at offset 0, sequential block-align-rounded 64 KiB packets,
   EOF clamp, served through `plan_virtual_bank`/`resolve` with entry bytes
   from a `StretchState` + `encode_block` feed over `BlockCachePcm`.
3. Legs: 50% and 175% (the plan's demo rates) on the existing synthetic
   fixture; at least one leg must use the preview-first (reverse) entry-order
   fixture, which `main()` already builds.
4. Pass criteria per leg: reassembled bytes byte-equal `transform_bank`'s
   output for the same source and percent; `parse_song_bank` accepts them;
   the main entry decodes equal to the oracle's decoded main entry.
   `streaming.passed` (and hence `overall_pass`) conjoins the new legs via
   the existing checks conjunction.
5. No schema/version discriminator; no changes to the surviving sections or
   to Step 2's existing streaming legs; the throughput figure remains
   informational and untouched.
6. The python schema heredoc needs NO structural change (the streaming leg
   already gates all checks) — verify by the failing-check spot-check
   (temporary mutation of a report COPY during development, never committed).

## Dependencies

- `task-02-synthetic-engine-replay-harness` (the replay mechanics this leg
  re-derives; it cannot be demoed before it exists and is proven).

## Implementation Approach

1. Add the replay structs + a `replay_virtual_bank(...)` helper to the
   heredoc, composing the existing harness pieces; wire two legs into
   `validate_streaming` after the per-rate matrix.
2. Verify the console lines and report rows; spot-check the python gate with
   a mutated report copy.
3. Full standing gates; verify the report by inspection (replays present and
   passed, `overall_pass` true, schema unchanged, no cache/on_demand keys).
4. Record progress in
   `.agents/planning/2026-08-08-song-rate-streaming/implementation/` (repo
   convention: NEVER `.agents/scratchpad/`); tick plan Step 3's checklist
   item once this task lands green and both sibling tasks carry
   `Status: Complete`.

## Acceptance Criteria

1. **Replay legs present and green**
   - Given a validator run on the completed step
   - When inspecting `target/song-rate-validation/report.json`
   - Then the `streaming` section carries 50% and 175% replay results
     (packet-served → reassembled → reparsed → decoded → matched against the
     reference), both passed, with `overall_pass` true and `schema` still
     `song-rate-validation/v1`

2. **Step demo holds**
   - Given the completed step
   - When the maintainer reads the validator output
   - Then the console shows PASS lines for `streaming_replay_50` and
     `streaming_replay_175` (plan Step 3's demo statement)

3. **Failing legs gate the run**
   - Given a report copy with a failing replay check
   - When the script's python schema check runs against it
   - Then it exits nonzero (spot-check by temporary mutation, not committed)

4. **Tree is green**
   - Given the completed task
   - When running the five standing gates
   - Then all pass, with the Windows-target check at 0 warnings

## Metadata

- **Complexity**: Medium
- **Labels**: validation, reporting, song-rate, streaming, replay
- **Required Skills**: Rust, bash heredoc harness editing, repository
  host-validator harness
- **Generated By**: code-task-generator 2026-08-09
- **Source Plan**: `.agents/planning/2026-08-08-song-rate-streaming/implementation/plan.md`
- **Plan Step**: Step 3: Build the virtual bank and the synthetic engine replay
