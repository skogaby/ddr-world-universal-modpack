# Task: Implement Atomic Cache Storage and Recovery

## Description
Implement the filesystem-backed artifact store for immutable transformed XWBs, manifest commit markers, mutable LRU metadata, and startup recovery. Publication and validation must remain crash-safe under injected write, flush, rename, corruption, permission, and deletion failures.

## Background
Task 1 defines cache identities and schemas. This task turns those models into durable files without adding concurrency policy or game integration. A manifest is the sole commit marker: the XWB publishes first, the immutable manifest publishes last, and readers never accept partial or digest-mismatched output.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-08-05-song-playback-speed/design/detailed-design.md`

**Additional References (if relevant to this task):**
- `.agents/planning/2026-08-05-song-playback-speed/research/orientation.md`
- `src/services/avs_layeredfs/cache_hasher.rs`
- `src/core/xact/transform.rs`

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Add a cache-store layer under `src/services/song_rate/` that consumes Task 1 keys/manifests and Step 1 transformed output without knowing runtime hooks.
2. Use same-directory temporary names `.<key>.tmp.<process>.<nonce>` for XWB, immutable manifest, LRU index, and tombstone publication.
3. Publish transformed output by writing/flush-validating a temporary XWB, atomically renaming immutable XWB first, then atomically renaming the validated immutable manifest last as the commit marker.
4. Validate hits using key inputs, manifest schema/versions, source identity, output length, and full output digest; the mutable LRU index must not participate in hit validity.
5. When a valid destination already exists, discard the temporary output and reuse the winner. When a destination is corrupt, remove its manifest first, return an eviction/retry outcome, and never overwrite it in place.
6. Publish the versioned LRU index temp-plus-rename; rebuild it from immutable manifest creation times when missing/corrupt and retain newest duplicate timestamps.
7. Implement startup recovery that removes orphan temporary files, XWBs without manifests, manifests with missing/corrupt outputs, and stale cache/version artifacts in manifest-before-XWB deletion order.
8. Implement atomic quarantine marker read/write/removal using the Task 1 tombstone identity; stale identities are removed while matching identities remain.
9. Introduce deterministic filesystem/fault seams for tests covering short write, flush, rename, disk-full, permission, interrupted publication, corrupt destination, delete failure, and partial recovery.
10. Return typed store/recovery outcomes and bounded operator diagnostics; no panic, process exit, or silent corruption is allowed.
11. Keep all tests in temporary directories and prove no operation escapes its configured cache root.

## Dependencies
- Step 2 Task 1 cache identity, manifest, LRU, tombstone, limit, and clock models.
- Step 1 streaming transformer output and digest report.
- No dependency on the later build coordinator or lease/eviction state machine.

## Implementation Approach
1. Define cache-root path validation, temporary naming, publication, lookup, recovery, and diagnostic result types.
2. Implement immutable XWB/manifest writing with explicit flush and readback validation.
3. Implement valid-destination-wins and corrupt-destination eviction/retry outcomes.
4. Implement independent LRU and tombstone atomic stores.
5. Implement startup scans and ordered cleanup.
6. Build an injected faulting filesystem wrapper and table-driven crash/failure tests.
7. Run the Step 1 validator and repository build gates; update canonical progress with Task 3 as the next action.

## Acceptance Criteria

1. **Crash-Safe Publication**
   - Given a transformed temporary XWB and matching immutable manifest
   - When every publication boundary succeeds
   - Then the XWB is renamed first, the manifest appears last, and a fresh lookup validates the exact output digest/length

2. **Interrupted Publication Recovery**
   - Given failure before XWB rename, between XWB and manifest rename, or during manifest publication
   - When startup recovery runs
   - Then no false cache hit exists, orphan temp/XWB files are removed safely, and a later build may retry

3. **Destination Races and Corruption**
   - Given a concurrent valid destination or a corrupt existing destination
   - When publication reaches the destination
   - Then the valid destination wins unchanged, while corruption loses its manifest first and yields an explicit eviction/retry path

4. **Mutable Metadata Isolation**
   - Given a missing/corrupt LRU index or stale/matching tombstone
   - When recovery/validation runs
   - Then LRU is rebuilt independently, stale tombstones are removed, matching tombstones remain, and immutable artifact validity is unchanged

5. **Fault Containment**
   - Given injected write, flush, rename, disk-full, permission, delete, and readback failures
   - When storage/recovery operations run
   - Then each returns a typed failure, preserves the commit-marker invariant, and never modifies files outside the cache root

6. **Build Readiness**
   - Given the completed storage layer
   - When focused host tests, `./scripts/validate_song_playback_speed.sh`, and repository check/format/release gates run
   - Then all pass and canonical progress identifies Step 2 Task 3 as the next action

## Metadata
- **Complexity**: High
- **Labels**: rust, filesystem, atomic-publication, crash-recovery, fault-injection, step-2
- **Required Skills**: code-assist, verification, self-documenting-code
- **Generated By**: code-task-generator 2026-08-05
- **Source Plan**: `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md`
- **Plan Step**: Step 2: Build the persistent cache and generation worker
