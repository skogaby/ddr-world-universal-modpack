# Task: Retire the Cache Model (Atomic Removal to a Green Identity Base)

## Description

Delete the rejected whole-file-generation/on-disk-cache machinery from the Song
Playback Speed feature and land a tree that still compiles, passes the (trimmed) host
validator, and behaves literally stock at runtime. This is the removal half of plan
Step 1; a follow-up task performs the binding-era renames on the resulting green tree.

## Background

The feature's audio internals are being redesigned streaming-only (design approved
2026-08-08). The retired model generated the entire stretched XWB at the FileManager
open and cached it on disk; everything specific to that model is removed. The
policy/clock/score surfaces (lifecycle eligibility, Q31 clock patch, score_guard
ledger, the `song_speed` option row, backend persistence) are KEEPERS and must not
change behavior. After this task the binding preflight always refuses, so
`integration_ready()` reports false and the option row does not register — the safe
intermediate state until the streaming engine lands in plan Step 4.

## Reference Documentation

**Required:**
- Design: `.agents/planning/2026-08-08-song-rate-streaming/design/detailed-design.md`
  (esp. Scope, reqs 6/25/38, and the "Components and Interfaces" trims)

**Additional References (if relevant to this task):**
- `.agents/planning/2026-08-08-song-rate-streaming/research/orientation.md` — the
  module-by-module keep/remove/rework verdict table this task executes

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. Delete `src/services/song_rate/cache.rs` and `src/services/song_rate/worker.rs`
   entirely, including their test suites wherever mounted (the validator harness).
2. Reduce `src/services/song_rate/model.rs` to what still compiles against consumers
   (cache keys, manifests, LRU, quarantine identities, and cache-limit normalization
   all go); delete the module outright if nothing survives — let the compiler decide.
3. Delete `src/core/xact/transform.rs` after relocating its PURE planning logic
   (per-entry rate targeting and half-up loop-boundary mapping with the one-frame
   clamp rule) into a new stub `src/core/xact/virtual_bank.rs` with its tests. The
   whole-song transform pipeline, the 128 MiB admission, and `TransformReport` die.
4. Rework `src/services/song_rate/conversion.rs` into
   `src/services/song_rate/binding.rs`: keep the pure helpers that survive
   (`dance_bank_song_code`, `song_code_digest`) and their tests; add a preflight
   entry point that ALWAYS refuses (typed refusal → EarlyFailed); delete the
   open-redirect, exposure-seam, store/lease/quarantine, and generated-path logic.
5. Remove the song-rate seams from `src/services/avs_layeredfs/file_hooks.rs`
   (`song_rate_open_redirect` wiring in `fs_open`/`fs_lstat`, the generated-path
   convert seam) restoring pure LayeredFS behavior.
6. Trim `src/services/song_rate/runtime.rs`: delete the coordinator/cache statics
   (`CACHE_ROOT`, `GAME_VISIBLE_ROOT`, `CACHE_LIMIT_BYTES`, lazy coordinator), the
   generation-keyed open-redirect cache, `redirect_dance_bank_open`,
   `convert_streaming_xwb`, and the lease-release/tombstone drain legs. Keep the
   scene callback, desired-percent atomics, commit-visibility log poll, bank-timeline
   drain, and `integration_ready()` (which must now evaluate false because the
   binding integration is absent).
7. Trim `src/services/song_rate/transaction.rs` and
   `src/services/song_rate/xact_runtime.rs`: remove the lease-id and cache-digest
   pass-through fields and the lease-era maintenance kinds; keep the exactly-once
   frame/slot/token protocol and its tests intact (inject-tested, must stay green).
8. Remove `DDR_SONG_RATE_FAULT` legs whose injection sites are deleted with the
   conversion pipeline; keep the selector and the surviving transaction legs (new
   streaming legs arrive in plan Step 4).
9. Drop the `cache_limit_gib` field from the `song_playback_speed` config struct (no
   parse-but-ignore shim, per the register) and remove the key from the repository's
   `mod-config.json` example.
10. Update `scripts/validate_song_playback_speed.sh` IN PLACE (no schema versioning):
    remove the `cache` and `on_demand` report sections, their checks, and the deleted
    modules' test mounts; every surviving section (Step-1 synthetic audio, lifecycle,
    clock, score, transaction, wavebank-hook identity) must run green.
11. No behavioral change to keepers: `src/mods/song_playback_speed.rs`,
    `src/services/song_rate/lifecycle.rs` (phase renames belong to the next task),
    `src/services/song_rate/clock_patch.rs`, `src/services/score_guard.rs`.

## Dependencies

- None (first task of plan Step 1). Blocks `task-02-binding-era-renames-and-identity-assertions`.

## Implementation Approach

1. Work outside-in so the tree converges: validator harness mounts first (drop the
   deleted suites), then services (`cache`/`worker`/`model` → `conversion`→`binding`
   → `runtime`/`file_hooks` → `transaction`/`xact_runtime`), then `core/xact`
   (`transform.rs` → `virtual_bank.rs` stub), letting `cargo check` drive out every
   dangling reference.
2. Relocate surviving tests with their code (song-code helpers into binding tests;
   entry-plan/loop-map tests into `virtual_bank.rs`).
3. Run the full gate set: `./scripts/validate_song_playback_speed.sh`,
   `./scripts/validate_se_bank_synth.sh`,
   `cargo check --target x86_64-pc-windows-msvc` (0 warnings), whole-crate
   `cargo fmt`, `./build.sh`.
4. Record progress in
   `.agents/planning/2026-08-08-song-rate-streaming/progress.md` (repo convention:
   NEVER `.agents/scratchpad/`).

## Acceptance Criteria

1. **Cache machinery is gone**
   - Given the completed task's tree
   - When searching the crate for cache leases, manifests, quarantine tombstones,
     eviction, the 30 s deadline, the 128 MiB admission, or
     `data_mods/_cache/song_playback_speed`
   - Then no source reference remains (docs/planning records excepted)

2. **Tree is green**
   - Given the completed removal
   - When running the five standing gates
   - Then all pass, with the Windows-target check at 0 warnings

3. **Validator trimmed in place**
   - Given a validator run
   - When inspecting the stable report
   - Then no `cache` or `on_demand` section exists, no schema/version discriminator
     was added, and every surviving section passes

4. **Identity-only runtime**
   - Given the trimmed `runtime.rs` and refusing preflight
   - When host tests evaluate `integration_ready()`
   - Then it returns false (binding integration absent) while the clock patch,
     scene callback, and score-guard readiness remain individually intact

5. **Keepers untouched**
   - Given the keeper modules
   - When their existing test suites run
   - Then they pass without modification (except mechanical import/type fallout
     from deleted modules)

6. **Pure plan logic preserved**
   - Given `src/core/xact/virtual_bank.rs`
   - When its relocated entry-plan/loop-mapping tests run
   - Then they pass with behavior identical to the deleted `transform.rs` logic
     (same targets, same half-up mapping, same one-frame clamp rule, same 28-bit
     refusal)

## Metadata

- **Complexity**: High
- **Labels**: refactor, removal, song-rate, host-validation
- **Required Skills**: Rust, repository host-validator harness, careful subtractive
  refactoring against a compiler
- **Generated By**: code-task-generator 2026-08-08
- **Source Plan**: `.agents/planning/2026-08-08-song-rate-streaming/implementation/plan.md`
- **Plan Step**: Step 1: Remove the retired cache model and land the identity-only base
