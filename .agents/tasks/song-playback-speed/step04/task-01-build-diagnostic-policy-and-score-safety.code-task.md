# Task: Build Diagnostic Policy and Score Safety

## Description
Implement the host-testable lifecycle, eligibility, and score-safety foundation for one developer-only pre-generated 75% diagnostic. This task must remain non-deployable by itself: no generated path is exposed and the runtime clock remains identity.

## Background
Step 3 installed and live-validated identity-only clock, movie, LayeredFS, and wave-bank transaction infrastructure. Step 4 may expose one pre-generated bank only after eligibility, pending-save, sanitation, and tentative movie policy are fail-closed and fully tested.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-08-05-song-playback-speed/design/detailed-design.md`
- Plan Step 4: `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md`
- Runtime research: `.agents/planning/2026-08-05-song-playback-speed/research/runtime-integration.md`

**Additional References:**
- `src/services/song_rate/`
- `src/services/score_guard.rs`
- `src/services/stage_records.rs`
- `src/services/custom_options_persistence.rs`
- `src/services/scene_manager.rs`
- `src/services/movie_policy.rs`

## Technical Requirements
1. Implement the nonblocking generation lifecycle needed for Step 4: identity, armed diagnostic, preparing, redirect-ready, XACT-in-flight, committed, completed, early-failed, and late-failed.
2. Add scene-26 eligibility primitives for entered side, active participant mask, stage index, course exclusion, solo/doubles ownership, and rejection of local-versus, matching/BPL, demo, event, or unknown modes.
3. Register one permanent scene callback whose hot path reads only atomics/raw validated state and never waits, performs I/O, calls game functions, or re-enters scene manager.
4. Add a developer-only diagnostic configuration selecting one exact song code, 75%, and a pre-generated local XWB identity; unsupported/missing configuration remains identity.
5. Implement per-side fixed eight-entry pending rate-save rings with generation/stage identity, duplicate suppression, claim/consume states, Quick Restart deduplication, and successful card-in reset ownership.
6. Extend score-guard readiness to require save detour, decoded stage/course records, scene manager, EAM_EXIT sanitizer callback, and league-node removal semantics.
7. Make unknown side/stage decoding fail closed while pending rate state exists; never default to P1 or consume an unmatched pending entry.
8. Implement tentative `MovieSuppressor::SongRate` policy at accepted diagnostic arm and deterministic clearing at the definitive lifecycle boundary.
9. Keep Q31 identity, generated-path exposure disabled, and score state untainted until Task 2's exact transaction commits.
10. Add pure host tests for every eligibility/state/save/policy transition and run all local validation/build gates; do not deploy in this task.
11. Update canonical progress with Task 2 as the next action only after all gates pass.

## Dependencies
- Completed and CrossOver-validated Step 3 identity transaction.
- Existing score save/sanitizer and stage-record infrastructure.
- No player-facing option, generalized generation, or backend schema work.

## Implementation Approach
1. Define pure classification/lifecycle/ring models and table-driven tests.
2. Integrate fail-closed readiness and scene/card lifecycle boundaries.
3. Add diagnostic config parsing and tentative movie contributor without path exposure.
4. Run host validators and repository gates only.

## Acceptance Criteria

1. **Eligibility Is Exact and Nonblocking**
   - Given normal solo/P2-started doubles and every excluded/ambiguous mode
   - When scene 26 evaluates the diagnostic arm
   - Then only the configured eligible participant/song arms and every other case remains identity without blocking or I/O

2. **Pending Save Identity Is Fail-Closed**
   - Given reordered, duplicate, delayed, Quick-Restart, unknown-side, and card-reset save sequences
   - When pending entries are claimed and consumed
   - Then only the exact side/stage/generation is consumed once and ambiguity suppresses without erasing state

3. **Full Sanitization Readiness**
   - Given each score/sanitizer prerequisite present or individually absent
   - When song-rate readiness is evaluated
   - Then diagnostic arming is available only for the complete prerequisite set

4. **Tentative Movie Lifecycle**
   - Given accepted, rejected, early-failed, restarted, completed, and reset diagnostic attempts
   - When lifecycle transitions occur
   - Then the song-rate movie contributor activates before stage construction and clears only at the approved definitive boundary

5. **Host-Only Safety**
   - Given Task 1 complete
   - When all host/build gates run
   - Then tests pass, Q31 remains identity, no generated path is returned, no deployment occurs, and progress advances to Task 2

## Metadata
- **Complexity**: High
- **Labels**: rust, lifecycle, eligibility, score-guard, movie-policy, host-only, step-4
- **Required Skills**: code-assist, verification, self-documenting-code
- **Generated By**: code-task-generator 2026-08-06
- **Source Plan**: `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md`
- **Plan Step**: Step 4: Prove one pre-generated 75% song end to end
