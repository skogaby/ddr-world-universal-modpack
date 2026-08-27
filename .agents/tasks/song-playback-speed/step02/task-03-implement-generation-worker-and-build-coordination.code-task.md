# Task: Implement Generation Worker and Build Coordination

## Description
Implement the single-heavy-job generation worker and process-local build table around the Step 1 transformer and Task 2 atomic store. Concurrent callers for one key must share one build, obsolete work must cancel cooperatively, and timeout/panic/stale epochs must always wake waiters without publishing stale output.

## Background
Tasks 1-2 provide deterministic identities and crash-safe storage, but no concurrency or lifecycle owner. The approved design requires one CPU-heavy worker, monotonically increasing epochs, duplicate waiter sharing, cooperative cancellation at every Step 1 checkpoint, a 30-second deadline, and an RAII panic guard that prevents permanent `Building` states.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-08-05-song-playback-speed/design/detailed-design.md`

**Additional References (if relevant to this task):**
- `.agents/planning/2026-08-05-song-playback-speed/research/runtime-integration.md`
- `src/core/xact/transform.rs`
- `src/services/song_rate/` from Step 2 Tasks 1-2

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Define build-table states `Queued`, `Building`, `Ready`, `Failed`, `Evicting`, and `Quarantined` with monotonically increasing build epochs and waiter notification state.
2. Implement one background generation worker that serializes CPU-heavy transforms while allowing concurrent lookup/state operations.
3. Deduplicate concurrent requests for an identical key so one job builds and every waiter observes the same success/failure result.
4. Drop obsolete queued jobs and cancel active obsolete jobs cooperatively through the Step 1 source-hash, decode-block, stretch-window, encode-block, output-write, output-digest, and validation checkpoints.
5. Enforce a 30-second deadline from the waiting request, propagate timeout cancellation, and ensure every waiter wakes on success, failure, cancellation, timeout, or panic.
6. Wrap each job in `catch_unwind` plus an RAII completion guard that transitions unfinished `Building` state to `Failed`, removes temporary files through the store, and notifies all waiters.
7. Prevent stale epochs from renaming/publishing output after timeout, supersession, or a newer build; epoch must be checked immediately before every irreversible store transition.
8. Keep coordinator/build-table locks limited to metadata transitions. Source reads, hashing, DSP, file I/O, condition waits, and callbacks must run with no coordinator or LayeredFS/game lock held.
9. Expose host-only request/result APIs returning immutable generated paths/metadata; do not install hooks, resolve AVS paths, or expose non-100% runtime behavior.
10. Use bounded diagnostics for queue depth, shared waiter count, build phase/duration, cancellation reason, panic, timeout, and stale-result discard.
11. Add deterministic concurrency tests using barriers/injected clocks rather than timing sleeps wherever possible.

## Dependencies
- Step 2 Task 1 cache keys/manifests and Task 2 atomic store/recovery APIs.
- Step 1 `transform_song_xwb` cancellation phases and manifest-ready reports.
- No dependency on later runtime XACT/LayeredFS transaction code.

## Implementation Approach
1. Define request, result, phase, epoch, waiter, build-entry, queue, and diagnostic models.
2. Implement duplicate request joining and one-worker queue scheduling.
3. Thread epoch/deadline cancellation into every Step 1 checkpoint and store publication boundary.
4. Add RAII completion and panic containment with unconditional notification.
5. Implement stale-epoch publication refusal and cleanup.
6. Add barrier-driven tests for cold build, warm hit, duplicate callers, queued/active cancellation, timeout, panic, and supersession.
7. Run validators/build gates and update canonical progress with Task 4 as the next action.

## Acceptance Criteria

1. **Duplicate Build Sharing**
   - Given multiple concurrent callers requesting one uncached key
   - When generation starts
   - Then exactly one transform runs, every caller waits on the same entry, and all receive the same immutable result

2. **Cooperative Cancellation and Epoch Safety**
   - Given queued or active work superseded by a newer epoch
   - When checkpoints and publication boundaries run
   - Then obsolete work exits, wakes waiters, removes temporary output, and cannot publish over the newer state

3. **Timeout and Panic Completion**
   - Given a job crossing the 30-second injected deadline or panicking at any phase
   - When the completion guard unwinds
   - Then `Building` cannot remain stuck, every waiter wakes with a typed failure, and the worker continues processing later jobs

4. **Single Heavy Worker**
   - Given different uncached keys requested concurrently
   - When work is scheduled
   - Then at most one transform executes at a time while lookups, waiter registration, and completed-result reads remain available

5. **Lock Discipline**
   - Given injected probes around source reads, DSP, file operations, and waits
   - When a build runs
   - Then no coordinator lock is held across any heavy operation or callback

6. **Build Readiness**
   - Given the complete worker/coordinator
   - When focused concurrency tests, host validators, and repository build gates run
   - Then all pass and canonical progress identifies Step 2 Task 4 as the next action

## Metadata
- **Complexity**: High
- **Labels**: rust, concurrency, worker, cancellation, timeout, panic-safety, step-2
- **Required Skills**: code-assist, verification, self-documenting-code
- **Generated By**: code-task-generator 2026-08-05
- **Source Plan**: `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md`
- **Plan Step**: Step 2: Build the persistent cache and generation worker
