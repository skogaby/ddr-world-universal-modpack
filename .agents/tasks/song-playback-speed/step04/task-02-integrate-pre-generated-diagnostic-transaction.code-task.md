# Task: Integrate Pre-Generated Diagnostic Transaction

## Description
Connect one validated local 75% XWB to the existing identity transaction and implement allocation-free commit, reset, unload, and late-failure behavior. All fault and ordering validation remains host-side so this task still requires no deployment.

## Background
Task 1 supplies fail-closed policy and score safety. Step 3 supplies the checked clock stub, call-nonced TLS, fixed XACT slots, maintenance queue, LayeredFS seam, cache leases, and shared movie policy; this task activates those pieces only for the configured diagnostic song.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-08-05-song-playback-speed/design/detailed-design.md`
- Plan Step 4: `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md`
- `docs/song_playback_speed.md`

**Additional References:**
- `src/services/song_rate/clock_patch.rs`
- `src/services/song_rate/xact_runtime.rs`
- `src/services/song_rate/wavebank_hook.rs`
- `src/services/avs_layeredfs/file_hooks.rs`
- `src/services/score_guard.rs`
- `src/services/movie_policy.rs`

## Technical Requirements
1. Validate the configured pre-generated XWB through the Step 1 parser/manifest rules and bind it to exact source/output/path/cache digests, requested 75%, generation, participant mask, and effective rate.
2. Expose the generated native path only from the qualifying `fs_convert_path` nested inside the matching call-nonced `wavebank_create` frame; lstat/open probes and unrelated/static replacements remain unchanged.
3. Transfer the complete token and consuming cache lease into one authoritative preallocated XACT slot before returning the generated path.
4. Implement allocation-free, lock-free, no-panic commit ordering: score protection first, movie confirmation second, coherent snapshot third, non-identity Q31 last.
5. Implement identity-first reset and deferred-reset repair so no stale non-identity write can follow gameplay exit/session reset.
6. Implement idempotent re-exposure/recommit for the same generation while rejecting supersession during XACT-in-flight.
7. On pre-exposure failure, fall back to stock 100% with bounded diagnostics and no score taint.
8. On XACT rejection or exact-token recovery failure after exposure, force loading failure, retain identity clock, preserve conservative score/movie policy, quarantine/process-pin the lease, enqueue maintenance, and never recall the original.
9. Release transferred leases only after original unregister completes; queue saturation retains resources pinned.
10. Add developer-only bounded fault injection for source/read/validation, token mismatch, pre/post-original, reset overlap, conversion, XACT reject, maintenance saturation, and quarantine paths.
11. Add deterministic host tests for transaction ordering, exactly-once originals, mismatch isolation, Quick Restart, unload, and all faults; run full local gates but do not deploy.
12. Update canonical progress with Task 3 as the next action after gates pass.

## Dependencies
- Step 4 Task 1 policy/score foundation.
- A locally generated, uncommitted Step 1 75% artifact for the configured diagnostic song.
- Completed Step 2 cache/lease and Step 3 identity transaction infrastructure.

## Implementation Approach
1. Extend the existing LayeredFS/XACT seams behind the developer diagnostic gate.
2. Write ordering/fault tests before permitting non-identity publication.
3. Connect lease transfer, commit/reset, unload, and quarantine maintenance.
4. Produce a release-build candidate and host evidence without deploying it.

## Acceptance Criteria

1. **Exact Redirect Isolation**
   - Given matching versus unrelated paths, calls, depths, generations, and digests
   - When conversion exposes the diagnostic XWB
   - Then only the exact nested wave-bank call receives a token/path and every mismatch remains stock identity

2. **Commit and Reset Ordering**
   - Given XACT success and overlapping gameplay exit/reset
   - When commit/reset writers execute
   - Then score/movie safety precedes snapshot/Q31 activation, reset writes identity first, and no late non-identity write survives

3. **Exactly-Once Failure Safety**
   - Given every pre/post-original and token-recovery fault
   - When wavebank creation runs
   - Then the original executes exactly once, pre-exposure faults fall back stock, and post-exposure uncertainty aborts/quarantines without retry

4. **Lease and Reload Lifecycle**
   - Given normal unload, Quick Restart, repeated same-generation exposure, late failure, and full maintenance queue
   - When slot/lease transitions run
   - Then normal unload releases once, reload is idempotent, and uncertain resources remain process-pinned

5. **Deployment Candidate Readiness**
   - Given all host fault/concurrency tests and repository gates
   - When Task 2 completes
   - Then one identity-restorable 75% diagnostic build is ready, no manual deployment has occurred, and progress advances to the single validation task

## Metadata
- **Complexity**: High
- **Labels**: rust, layeredfs, xact, transaction, score-integrity, fault-injection, host-only, step-4
- **Required Skills**: code-assist, verification, self-documenting-code
- **Generated By**: code-task-generator 2026-08-06
- **Source Plan**: `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md`
- **Plan Step**: Step 4: Prove one pre-generated 75% song end to end
