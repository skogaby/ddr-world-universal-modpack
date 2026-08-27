# Task: Define Cache Identity and Manifests

## Description
Define the pure, versioned data models that identify generated song-rate artifacts and describe their immutable and mutable metadata. This task establishes deterministic cache keys, strict manifests, LRU records, quarantine identities, and cache-limit normalization before any filesystem or worker behavior is implemented.

## Background
Step 1 produces deterministic transformed XWBs and a manifest-ready `TransformReport`, but no persistent identity or storage contract. Later cache, worker, eviction, and runtime code must agree on one canonical full-digest key and schema without string-concatenation ambiguity or mutable recency affecting artifact validity.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-08-05-song-playback-speed/design/detailed-design.md`

**Additional References (if relevant to this task):**
- `.agents/planning/2026-08-05-song-playback-speed/research/orientation.md`
- `src/core/xact/digest.rs`
- `src/core/xact/transform.rs`

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Create the host-only song-rate service boundary under `src/services/song_rate/` without installing hooks, registering runtime callbacks, or touching LayeredFS/XACT.
2. Define fixed cache, algorithm, codec, manifest, LRU-index, and tombstone version constants; use the Step 1 algorithm/codec versions as authoritative inputs rather than duplicating magic values.
3. Define `CacheKeyInput` from source full MD5 digest and length, requested percentage, exact per-entry output frame counts, and all implementation/cache versions.
4. Encode cache-key input as one canonical binary sequence with fixed tags and little-endian length-prefixed fields; never use string concatenation. Hash the complete sequence with full 128-bit MD5 and expose lowercase hex only at JSON/filesystem boundaries.
5. Define an immutable cache manifest containing source/output full digests and lengths, requested percentage, both entry transforms, main index/rate, versions, and injected creation time.
6. Define a separate versioned mutable LRU index whose records contain key digest and last-used time; duplicate records resolve to the newest timestamp and never affect artifact validity.
7. Define a quarantine/tombstone identity containing source/output digest, cache/algorithm/codec versions, game-module digest, and platform identity.
8. Define strict typed validation/deserialization errors for malformed versions, digest encodings, lengths, entry counts, rate denominators, main indices, and key/manifest mismatches.
9. Normalize `cache_limit_gib` to default 10 GiB and clamp to `1..=1024`; zero is the minimum, not unlimited, and normalization returns a bounded diagnostic reason.
10. Inject clock/time values into model construction so tests and serialized output are deterministic.
11. Use repository-relative paths in tests/docs and synthetic values only; no game-derived cache artifacts or local corpus paths may be committed.

## Dependencies
- Completed Step 1 pure digest, rate, and transform report APIs.
- Approved cache manifest, key, LRU, tombstone, and configuration contracts.
- No dependency on later Step 2 storage, worker, or eviction tasks.

## Implementation Approach
1. Add `src/services/song_rate/mod.rs` and focused model/key modules that remain callable from host tests.
2. Define strongly typed full-digest, cache-key, entry-transform, manifest, LRU, tombstone, and limit models.
3. Implement canonical binary key encoding and full-MD5 derivation.
4. Implement strict JSON serialization/deserialization and cross-field validation.
5. Add table-driven key separation, malformed schema, duplicate LRU, deterministic clock, and limit-normalization tests.
6. Run the Step 1 host validator and normal repository build gates; update canonical progress with Task 2 as the next action.

## Acceptance Criteria

1. **Canonical Cache Keys**
   - Given otherwise-identical inputs differing by source digest/length, percentage, per-entry frame target, or any version
   - When cache keys are derived
   - Then each difference produces a distinct full 128-bit key and repeated identical inputs produce identical canonical bytes and hex

2. **Strict Immutable Manifest**
   - Given a valid Step 1 transform report and injected creation time
   - When an immutable manifest is serialized and reparsed
   - Then all digests, lengths, entry transforms, main rate/index, versions, and time round-trip exactly and any inconsistent field returns a typed error

3. **Separate Mutable Recency**
   - Given duplicate LRU records for one key
   - When the versioned index is normalized
   - Then only the newest timestamp remains and changing recency never changes the immutable key/manifest identity

4. **Quarantine Identity**
   - Given source/output/version/game/platform identities
   - When a tombstone is validated
   - Then an exact match remains active while any source, implementation, game, or platform change makes it stale

5. **Configuration Normalization**
   - Given absent, zero, in-range, and oversized cache limits
   - When normalization runs
   - Then values resolve to 10, 1, unchanged, and 1024 GiB respectively with bounded diagnostics

6. **Build Readiness**
   - Given the completed identity/model layer
   - When focused host tests, `./scripts/validate_song_playback_speed.sh`, `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`, and `./build.sh` run
   - Then all pass and canonical progress identifies Step 2 Task 2 as the next action

## Metadata
- **Complexity**: High
- **Labels**: rust, cache-key, manifest, md5, serialization, step-2
- **Required Skills**: code-assist, verification, self-documenting-code
- **Generated By**: code-task-generator 2026-08-05
- **Source Plan**: `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md`
- **Plan Step**: Step 2: Build the persistent cache and generation worker
