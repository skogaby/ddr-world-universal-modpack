# Consumer Migration

Updated: 2026-09-08
Status: Complete (uncommitted; integrated, host validation and builds passed)
NEXT ACTION: Cabinet validation per docs/frame_scheduling.md; main progress.md records final evidence.

## Context and Approved Plan

Direct scoped implementation request under the approved `plan.md` (2026-09-08),
not a generated PDD task. Parent `context.md` documents the repository/build
constraints; no CODEASSIST.md exists. Only the seven assigned consumer files,
a minimal pure helper/test harness, and this progress record are owned here.
No commits, staging, deploys, operator configuration, scheduler, or audio edits.

## Acceptance and Test Plan

- [x] WebUI load timeouts use 12 seconds elapsed, resolving before expiring.
- [x] SMX discovery runs immediately then every 2 seconds; atlas warns after
  10 seconds unresolved, but keeps polling for late readiness.
- [x] Chrome pump requests coalesce across concurrent synthesis completions;
  clear the pending latch before draining so concurrent completion cannot be lost.
- [x] Series-filter activation and cursor updates are generation-bound;
  close/rebuild cancels old work, including activation queued before first frame.
- [x] PUS raises coalesce, cancel on scene/disable, and check current scene,
  visibility, and menu state when executing; resolve wrappers only then.
- [x] Preserve renderer/font creation gates, resource ownership and render affinity.

Tests precede implementation in `src/core/deferred_work.rs`: readiness before,
at and after deadline; 2/10/12-second deadlines independent of invocation count;
immediate/rearmed periodic cadence; duplicate requests; request during drain;
cancel/reopen with stale queued work; current work surviving a stale callback;
concurrent producers under the same mutex discipline as the chrome consumer.
Use `scripts/validate_frame_consumers.sh` to compile the actual dependency-free
module with rustc --test. No existing SMX unit-test suite was found. Existing relevant
host harnesses: `bash scripts/validate_mod_menu.sh` and
`bash scripts/validate_custom_options.sh`, both from repository root.

Engine pointer accesses, menu z-order, load/release operations, and actual
dispatcher behavior require parent integration and cabinet validation; pure
helper tests cannot validate them. Full cargo check/build/format belong to the
parent; adding `pub mod deferred_work;` in `src/core/mod.rs` is the only planned
module integration request (that file is outside ownership).

## TDD and Validation

- Baseline mod-menu: 37 tests passed (`logs/consumer-baseline-mod-menu.log`).
- Baseline custom-options: 40 tests and display-string lint passed; two existing
  unused-import warnings (`logs/consumer-baseline-custom-options.log`).
- RED: new module contained tests only; rustc failed on the missing `Deadline`,
  `Wait`, and `PendingPump` types, not an environment failure
  (`logs/consumer-red.log`). No stub implementation was introduced.
- GREEN: implemented actual pure deadline/pending-generation primitives;
  all 9 tests passed (`logs/consumer-green.log`). Consumers now use those types.
- Code inspection: PUS reserves under its state lock, schedules after dropping
  it, clears pending before checking execution guards, and resolves current
  wrappers only after those guards. Close/reopen invalidates series activation
  as well as cursor continuations; partial builds clear on close too.
- Final runs: 9 helper tests, 37 mod-menu tests, and 40 custom-options tests
  passed; display-string lint passed. Logs: `logs/consumer-final-pure.log`,
  `logs/consumer-final-mod-menu.log`, `logs/consumer-final-custom-options.log`.
- Scoped `rustfmt --edition 2021 --check` passed for all eight Rust files after
  apply_patch formatting fixes (`logs/consumer-format-check.log`). Shell syntax
  (`bash -n scripts/validate_frame_consumers.sh`) and scoped `git diff --check`
  passed. No whole-crate formatter was run on concurrent parent work.
- Full consumer type-check, release/Win7 builds, and live behavior remain
  unverified here by ownership agreement. rustup emitted the existing unavailable
  Win7 rust-std component warning while running host tests; tests still passed.

## Exact Owned Files

- `src/mods/webui_options/preview_overlay.rs`
- `src/mods/webui_options/bg_preview_overlay.rs`
- `src/mods/smx_hardware/touch.rs`
- `src/mods/smx_hardware/overlay.rs`
- `src/mods/mod_menu/chrome_loader.rs`
- `src/services/series_filter_scroll.rs`
- `src/mods/power_user_statistics/timing_stats_widget.rs`
- `src/core/deferred_work.rs` (new; actual shared helper and 9 unit tests)
- `scripts/validate_frame_consumers.sh` (new; run with bash)
- This `consumer-progress.md` and `logs/consumer-*.log`.

## Parent Integration and Runtime Checks

Integration and coordinated builds below are now complete; the remainder of
this section records the original integration request and outstanding live checks.

Only module registration is required from the parent: `pub mod deferred_work;`
in `src/core/mod.rs`. Consumer imports will not resolve until registered. No new
dependencies, signatures, offsets, or scheduler API changes. The existing
renderer/font readiness checks for widget creation remain intact; this track
does not substitute frame-dispatch readiness for them.

Parent must preserve poll-before-one-FIFO-snapshot and next-frame continuations.
Cabinet validation should cover cold/hot WebUI previews at different FPS,
SMX discovery and atlas late readiness, rapid chrome synthesis changes,
filter close/reopen before queued activation and during pumping, and opening
the mod menu between PUS raise reservation and execution. Existing missing-dtor
degradation in series-filter remains: scene change is still its partial backstop.
The host tests cover the real shared primitives, not engine pointer safety,
actual layer ordering, Windows window enumeration, or texture loading.

## Deviations

Use this consumer-scoped record instead of modifying parent context/plan/progress.
User explicitly delegates full builds to parent and forbids commits/staging.
