# Task: Run Single Cabinet Diagnostic and Close Step

## Description
Perform the only deployment/manual validation pass for Step 4. Extend stable host evidence, deploy the complete diagnostic candidate once, execute the approved 75%/100% matrix, and close Step 4 only if every audio, clock, score, movie, unload, and restoration oracle passes.

## Background
Tasks 1–2 deliberately perform no deployments so manual testing is consolidated here. The 75% diagnostic is the hard release gate before generalized on-demand generation or player UI may be implemented.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-08-05-song-playback-speed/design/detailed-design.md`
- Plan Step 4: `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md`
- Canonical progress: `.agents/planning/2026-08-05-song-playback-speed/progress.md`

**Additional References:**
- `scripts/validate_song_playback_speed.sh`
- `scripts/deploy.sh`
- `docs/song_playback_speed.md`
- `src/services/song_rate/`

## Technical Requirements
1. Extend stable schema `song-rate-validation/v1` with diagnostic-transaction checks while preserving all Step 1–3 evidence and failure behavior.
2. Record the configured song/source/generated/module/platform digests and exact requested/effective rate without copying game-derived banks or absolute local paths into committed artifacts.
3. Run the song-rate validator, Assist Tick validator, Windows target check, format, and release build immediately before the single deployment.
4. Deploy only after every host check passes; do not perform intermediate Task 1/2 deployments.
5. On the target, verify boot readiness and absence of song-rate warnings before starting the diagnostic.
6. Play the configured pre-generated 75% song from first audible/visible landmark through natural song end, measuring first/late/final alignment and requiring no accumulated drift beyond 2 ms.
7. Verify musical pitch remains unchanged, chart/judgment alignment is coherent, native judgment windows behave as designed, and loading/result transitions complete normally.
8. Verify the non-100% stage save is suppressed, logout is sanitized/forwarded, and no diagnostic score/grade/league/ranking data reaches the backend while permitted profile changes remain eligible.
9. Verify configured movie suppression, cache lease transfer, unregister release, and no stuck/pinned resource on the successful path.
10. Immediately play the same/another song at literal 100% and verify stock audio/clock, normal trusted score submission, normal movie policy, and clean next-selection recovery.
11. Exercise the approved late-failure/fault selector only where safe and confirm identity/quarantine behavior without attempting same-call stock retry.
12. Record exact observations in canonical progress. Any failed/unknown oracle leaves Step 4 unchecked and blocks Steps 5–8.
13. After all oracles pass, check only Step 4 and set Step 5 task generation/approval as `NEXT ACTION`.

## Dependencies
- Step 4 Tasks 1–2 complete with all local gates passing.
- Maintainer-approved target access and one pre-generated 75% diagnostic XWB kept outside source control.
- Backend/log access for score and logout verification.
- Manual testing time is intentionally concentrated in this one task.

## Implementation Approach
1. Finalize stable host evidence and a single deployable artifact.
2. Provide the maintainer one ordered test script/checklist with bounded expected logs.
3. Deploy once and collect runtime/backend observations.
4. Close or block Step 4 based strictly on the evidence matrix.

## Acceptance Criteria

1. **One Deployment Only**
   - Given completed Tasks 1–2 and green local gates
   - When Step 4 manual validation begins
   - Then exactly one consolidated deployment is needed for the planned success-path matrix

2. **75% End-to-End Synchronization**
   - Given the configured diagnostic song and generated bank
   - When it plays to natural completion
   - Then pitch is preserved, first/late/final landmarks align within 2 ms without drift, and chart/judgment/song-end behavior remains coherent

3. **Competitive Data Containment**
   - Given the committed 75% attempt and subsequent logout
   - When stage/logout requests reach the backend
   - Then stage score is absent, logout competitive fields are sanitized, permitted profile data persists, and no trusted ranking result is created

4. **Literal 100% Restoration**
   - Given the completed diagnostic attempt
   - When the next 100% song loads and saves
   - Then stock audio/clock/movie behavior and trusted score submission are fully restored with no stale token, lease, movie, or taint state

5. **Step 4 Closure Gate**
   - Given all host gates and runtime/backend oracles pass
   - When planning records are updated
   - Then only Step 4 is newly checked and Step 5 task generation/approval is the exact next action; otherwise the step remains blocked with the failing oracle recorded

## Metadata
- **Complexity**: High
- **Labels**: rust, cabinet-test, diagnostic, audio-sync, score-integrity, single-deployment, step-4
- **Required Skills**: code-assist, verification, self-documenting-code
- **Generated By**: code-task-generator 2026-08-06
- **Source Plan**: `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md`
- **Plan Step**: Step 4: Prove one pre-generated 75% song end to end
