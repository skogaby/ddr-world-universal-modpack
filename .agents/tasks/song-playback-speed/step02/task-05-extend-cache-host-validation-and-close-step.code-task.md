# Task: Extend Cache Host Validation and Close Step

## Description
Extend the mandatory song-rate host validator to exercise the complete persistent cache and worker lifecycle, emit stable cache evidence, demonstrate concurrent cold/warm behavior plus crash recovery/eviction, and close implementation-plan Step 2 only after every host/build gate passes.

## Background
Tasks 1-4 implement the Step 2 cache, store, worker, leases, eviction, and quarantine behavior. Step 2 is complete only when one host command proves those components outside the DLL and produces the cache section later runtime work will rely on.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-08-05-song-playback-speed/design/detailed-design.md`

**Additional References (if relevant to this task):**
- `.agents/planning/2026-08-05-song-playback-speed/research/pitch-preservation.md`
- `.agents/planning/2026-08-05-song-playback-speed/research/runtime-integration.md`
- `scripts/validate_song_playback_speed.sh`
- `src/services/song_rate/` from Step 2 Tasks 1-4

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Extend `scripts/validate_song_playback_speed.sh` and schema `song-rate-validation/v1` with a stable cache section while preserving every Step 1 metric and failure behavior.
2. In a temporary cache, issue concurrent requests for one uncached synthetic/local XWB and prove exactly one build with shared waiter results.
3. Demonstrate a validated warm hit with no transform, then invalidate by source digest, requested rate, exact frame target, and each cache/algorithm/codec version.
4. Exercise queued and active cancellation, stale epoch refusal, 30-second injected timeout, worker panic, and unconditional waiter wakeup/continued worker operation.
5. Inject write, flush, rename, disk-full, permission, corrupt destination, delete, and interrupted-publication failures; prove manifest-last validity and startup recovery.
6. Exercise LRU ordering, configured limit, free-space safety margin, leased/active protection, `Evicting` retry, unload-style release, quarantine process pinning, stale tombstone cleanup, and stopped-game purge refusal/success.
7. Report cache keys/digests, cold/warm latency, peak estimated/measured memory, build/share/cancel/recovery counts, committed/temp bytes, lease/state counts, eviction order, quarantine outcome, and every check status without absolute paths.
8. Support optional external corpus benchmarking through the existing environment/CLI contract; release mode remains fail-closed without external stock/custom coverage and explicit platform.
9. Generate all cache/demo/report artifacts only under ignored target/temp locations and remove stale evidence before each run.
10. Keep all tests host-only; do not install signatures, hooks, LayeredFS handlers, XACT calls, UI, or score behavior.
11. Run the Step 1/2 host validator, Assist Tick validator, Windows target check, format, and release build gates.
12. After the cache demo passes, update canonical `progress.md` to Step 2 done with Step 3 as `NEXT ACTION`, and check only Step 2 in the approved implementation plan.

## Dependencies
- Completed Step 2 Tasks 1-4.
- Existing Step 1 validator/report and synthetic fixtures.
- External corpus/platform evidence only for release mode, not ordinary development completion.

## Implementation Approach
1. Add cache test orchestration and report models to the throwaway host harness.
2. Add deterministic concurrency/fault/capacity scenarios using injected seams and temporary directories.
3. Produce the required concurrent cold-build, warm-hit, eviction, crash-recovery, and quarantine demo evidence.
4. Exercise CLI/report determinism and stale-output cleanup.
5. Run optional external corpus performance validation when configured.
6. Run all regression/build gates and review report contents.
7. Update canonical progress and check only implementation-plan Step 2.

## Acceptance Criteria

1. **Concurrent Cold and Warm Demo**
   - Given multiple concurrent requests for one uncached supported bank
   - When the host validator runs
   - Then exactly one cold build occurs, all waiters share it, a later warm hit performs no transform, and immutable path/digest/lease metadata agree

2. **Crash and Failure Recovery**
   - Given every injected storage, timeout, panic, cancellation, and interrupted-publication failure
   - When recovery and a subsequent valid request run
   - Then no false hit/stuck waiter/stale publication remains and the later build succeeds

3. **Eviction, Lease, and Quarantine Evidence**
   - Given over-limit/free-space pressure, active leases, `Evicting`, and matching/stale tombstones
   - When maintenance runs
   - Then only inactive LRU entries are removed, active state is protected, retries wait safely, matching quarantine blocks generation, and stale identities recover

4. **Stable Cache Report**
   - Given a successful ordinary validation run
   - When `target/song-rate-validation/report.json` is inspected
   - Then schema-v1 retains Step 1 evidence and includes every required cache metric/check with no absolute paths or game-derived copied inputs

5. **Measured Resource Thresholds**
   - Given synthetic and optional external corpus builds
   - When cold/warm latency and estimated/measured memory are evaluated
   - Then synthetic checks pass, configured native/CrossOver thresholds are enforced in release mode, and violations exit nonzero

6. **Step 2 Closure**
   - Given passing host/cache demos, Assist Tick regression, target check, format, and release build
   - When canonical planning records are updated
   - Then only Step 2 is newly checked, progress records Step 2 done, and Step 3 is the exact next action

## Metadata
- **Complexity**: High
- **Labels**: shell, rust, host-validation, cache-demo, concurrency, crash-recovery, step-2
- **Required Skills**: code-assist, verification, self-documenting-code
- **Generated By**: code-task-generator 2026-08-05
- **Source Plan**: `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md`
- **Plan Step**: Step 2: Build the persistent cache and generation worker
