# Task: Promote Movie Policy and Validate Identity Runtime

## Description
Move DirectShow movie-hook ownership into a shared policy service without changing current Non-Native OS behavior, then close Step 3 with host and cabinet evidence that every new runtime primitive is semantically identity. Step 3 must remain unavailable for non-100% playback.

## Background
Tasks 1-4 install the clock, transaction state, wave hooks, and inert LayeredFS seam. The remaining shared-hook ownership and end-to-end identity proof must be complete before Step 4 may expose one pre-generated 75% diagnostic bank.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-08-05-song-playback-speed/design/detailed-design.md`
- Runtime research: `.agents/planning/2026-08-05-song-playback-speed/research/runtime-integration.md`

**Additional References:**
- `src/mods/non_native_os_support.rs`
- `src/core/signatures.rs` (`movie_build_graph`)
- `src/lib.rs`
- `scripts/validate_song_playback_speed.sh`
- `AGENTS.md`

## Technical Requirements
1. Add `src/services/movie_policy.rs` as the sole `movie_build_graph` hook owner with independent atomic contributors for Non-Native OS and future song-rate suppression.
2. Preserve Non-Native OS behavior exactly: when enabled and installed, bypass graph construction, write player state `+0x8 = 3`, leave opened byte `+0x14 = 0`, and return success; when disabled/unavailable, execute original behavior.
3. Keep the song-rate contributor permanently false throughout Step 3 and expose only policy APIs required by later diagnostic work.
4. Ensure one detour is installed, contributor changes are allocation-free/lock-free, and every callback calls either the stub policy or original exactly once.
5. Integrate movie-policy readiness into song-rate identity readiness without making the mod/user option available or arming non-100% state.
6. Extend host validation with stable identity-transaction evidence: Q31 identity, coherent snapshot/reset, empty TLS/slots/maintenance, no dynamic redirect, exactly-once counters, LayeredFS readiness/rollback, and movie contributor combinations.
7. Run all repository validation gates and deploy an identity build to the current cabinet.
8. Collect bounded boot/hook/play evidence showing zero generated song-rate files, stock `music_count`, normal score submission, unchanged movie behavior for the configured Non-Native OS policy, and normal song unload.
9. If any host or cabinet identity invariant fails, leave Step 3 unchecked and record the blocker; do not proceed to the pre-generated 75% diagnostic.
10. After all evidence passes, check only Step 3 and update canonical progress to Step 4 task generation/approval.

## Dependencies
- Step 3 Tasks 1-4.
- Current cabinet access for the required identity deployment demo.
- Existing Non-Native OS movie behavior and Step 1/2 validators.
- No generated-XWB exposure, non-identity Q31, score taint, scene-26 arm, or player UI.

## Implementation Approach
1. Add policy-combination and exactly-once hook tests before migrating ownership.
2. Move hook installation/state from the mod to the shared service and preserve the mod-facing readiness contract.
3. Extend stable host evidence without removing Step 1/2 report fields.
4. Run build gates, deploy identity-only, record observations, and close Step 3 only on success.

## Acceptance Criteria

1. **Movie Behavior Preservation**
   - Given every combination of hook availability and Non-Native OS enablement
   - When movie graph construction runs
   - Then behavior and return/state writes match the pre-migration implementation exactly and only one detour owns the target

2. **Song-Rate Contributor Inertness**
   - Given the complete Step 3 service
   - When any runtime lifecycle or file/audio call occurs
   - Then the song-rate movie contributor stays false and cannot suppress a movie independently

3. **Stable Host Identity Evidence**
   - Given the mandatory host validator
   - When all transaction fault/concurrency scenarios run
   - Then Step 1/2 evidence remains intact and identity clock, snapshot, TLS/slots, exactly-once hooks, LayeredFS rollback, and movie policy all pass in the stable report

4. **Cabinet Identity Demo**
   - Given an identity-only build on the current cabinet
   - When the game boots, plays a normal 100% song, submits its normal score, handles its movie policy, and unloads the bank
   - Then no generated song-rate file/path appears, `music_count` remains stock, audio/chart/scoring are unchanged, and identity hook logs are complete

5. **Step 3 Closure Gate**
   - Given passing host validation, Assist Tick validation, Windows target check, format, release build, and cabinet identity evidence
   - When planning records are updated
   - Then only Step 3 is newly checked and Step 4 task generation/approval is the exact next action

## Metadata
- **Complexity**: High
- **Labels**: rust, movie-policy, identity-validation, cabinet-test, step-3
- **Required Skills**: code-assist, verification, self-documenting-code
- **Generated By**: code-task-generator 2026-08-06
- **Source Plan**: `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md`
- **Plan Step**: Step 3: Install identity-only runtime transaction infrastructure
