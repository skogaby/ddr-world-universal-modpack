# Task: Implement Leases, LRU Eviction, and Quarantine

## Description
Complete the cache lifecycle with consuming leases, `Evicting` exclusion, LRU maintenance, free-space admission, bounded eviction, quarantine tombstones, stopped-game purge, and operator-facing recovery diagnostics. Active, building, exposed, or late-failed artifacts must never be deleted.

## Background
Tasks 1-3 provide identity, durable storage, and coordinated generation. Runtime steps will later expose generated paths to XACT and transfer cache ownership through lease IDs, so Step 2 must establish deletion safety and quarantine behavior before hooks exist.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-08-05-song-playback-speed/design/detailed-design.md`

**Additional References (if relevant to this task):**
- `.agents/planning/2026-08-05-song-playback-speed/research/runtime-integration.md`
- `src/services/song_rate/` from Step 2 Tasks 1-3
- `src/core/xact/transform.rs`

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Implement a fixed/preallocated lease table whose IDs can be transferred but whose `CacheLease` handle is consuming and non-cloneable.
2. Maintain lease counts/states under the same coordinator lock as build and eviction metadata; building, leased, active-XACT, late-failed, and otherwise pinned entries are never eviction candidates.
3. Implement `Ready -> Evicting` claim under lock, remove the immutable manifest commit marker before XWB outside the lock, and finalize/recover state under lock. Prepare requests encountering `Evicting` wait/retry rather than racing deletion.
4. Enforce the normalized committed-cache limit with true inactive-LRU order and account temporary output separately from committed size.
5. Before a build, reserve free space for exact estimated final XWB, temporary output, and a 64 MiB safety margin; if admission cannot be satisfied after safe eviction, fail before generation.
6. Update only the separate versioned LRU index for recency; persist it temp-plus-rename and rebuild from manifests when missing/corrupt.
7. Implement quarantine markers keyed by exact source/output/version/game/platform identity. Matching tombstones block retries; changed identities remove stale tombstones and permit a new build.
8. Model late-failed entries as process-pinned: their lease is intentionally retained and the bulky XWB cannot be removed during the current process. On later startup the XWB may be evicted while the small matching tombstone remains.
9. Implement an explicit stopped-game purge that refuses while any job/lease/eviction is active and removes commit markers before artifacts/tombstones in safe order.
10. Expose pure diagnostics for committed/temp bytes, limits, free-space reservation, active leases, state counts, recovery cleanup, eviction decisions/failures, quarantine, and purge refusal/success.
11. Inject disk-capacity, clock, and deletion behavior for deterministic tests; use temporary directories only.

## Dependencies
- Step 2 Tasks 1-2 model/storage APIs and Task 3 coordinator/build-table states.
- Approved consuming lease, `Evicting`, LRU, free-space, quarantine, and purge contracts.
- No dependency on runtime wavebank hooks; later steps transfer/release these lease IDs.

## Implementation Approach
1. Define lease IDs/slots/handles and pin reasons.
2. Implement safe lease acquire/transfer/release and eviction eligibility.
3. Implement LRU selection, free-space admission, `Evicting` transitions, and ordered deletion.
4. Implement tombstone lifecycle and process pinning.
5. Implement stopped-game purge and structured diagnostics.
6. Add race/failure tests for lease/eviction, waiting prepare, limits, free space, tombstones, delete failures, and purge refusal.
7. Run validators/build gates and update canonical progress with Task 5 as the next action.

## Acceptance Criteria

1. **Lease-Protected Artifacts**
   - Given leased, building, active, or late-failed entries
   - When eviction scans run
   - Then none are selected or deleted, and consuming lease transfer/release updates exactly one slot/count

2. **Race-Safe Eviction**
   - Given an inactive LRU entry and concurrent prepare request
   - When eviction claims `Evicting`
   - Then prepare waits/retries, manifest is removed before XWB, and no caller observes/deletes a partially evicted artifact

3. **Limit and Free-Space Admission**
   - Given cache contents at/over limit and injected free-space values
   - When a build reserves final output, temporary growth, and 64 MiB safety margin
   - Then inactive LRU entries are evicted in order or generation fails before allocation without touching protected entries

4. **Quarantine Safety**
   - Given a matching late-rejection tombstone
   - When the key is requested this boot or a later matching boot
   - Then generation is blocked and the current-process artifact stays pinned; changed source/version/game/platform identity removes the stale marker and permits retry

5. **Stopped-Game Purge and Diagnostics**
   - Given inactive versus active cache state
   - When purge is requested
   - Then inactive state is removed in safe commit-marker order, active state is refused, and diagnostics accurately report sizes/states/recovery outcomes

6. **Build Readiness**
   - Given the completed cache lifecycle
   - When focused tests, host validators, and repository build gates run
   - Then all pass and canonical progress identifies Step 2 Task 5 as the next action

## Metadata
- **Complexity**: High
- **Labels**: rust, cache-lease, lru, eviction, quarantine, disk-admission, step-2
- **Required Skills**: code-assist, verification, self-documenting-code
- **Generated By**: code-task-generator 2026-08-05
- **Source Plan**: `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md`
- **Plan Step**: Step 2: Build the persistent cache and generation worker
