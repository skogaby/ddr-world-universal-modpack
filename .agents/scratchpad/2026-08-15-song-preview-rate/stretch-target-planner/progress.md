# Progress: task-01 StretchTarget parameterization

Updated: 2026-08-15
Status: Complete (uncommitted — maintainer-managed commits per AGENTS.md
workflow; all readiness gates green)

## Checklist

- [x] Cycle 1: T1 pin against current API (green baseline) → API change →
      call-site sweep (src + tests + validator script) → T1 green
- [x] Cycle 2: T2 Side-target inverse plan
- [x] Cycle 3: T3 side-loop mapping + T4 refusal identity
- [x] Cycle 4: T5 identity coherence + doc comments
- [x] Validate: validator script, windows check, fmt, release build
- [x] Close-out (commit deferred to the maintainer)

## Cycles

- Baseline: validator 214 tests green; `cargo check --target
  x86_64-pc-windows-msvc` clean. NOTE: repo-root `cargo test` is
  PRE-EXISTING-BROKEN on this macOS host (retour's arch module needs a
  windows target) — the project's actual host-test runner is the validator
  script's temp-dir harness; treated as the test command throughout.
- Cycle 1: added `stretch_target_main_reproduces_the_shipped_plan` (pin;
  independent `plan_entry_values` recomputation + serializer oracle,
  honest fixtures, both entry orders) — green against the OLD 2-arg API
  (215). Then: `StretchTarget` enum + `target_entry_index` field +
  3-arg `plan_virtual_bank`; compile-red sweep across `binding.rs:238`,
  `core/xact/tests.rs`, `binding_tests.rs`, `generator_tests.rs`, and the
  validator script's embedded harness (perl one-liner, verified
  exhaustive by grep). Pin green with identical values (215).
- Cycles 2–3: `stretch_target_side_inverts_the_plan` (incl. reparse of
  the completed virtual bytes + served-bytes oracle),
  `stretch_target_side_maps_the_side_loop` (known 75% vector 176/1056 on
  the side entry; main stock loop untouched),
  `stretch_target_side_refusals_name_the_side_entry` (ceiling on side
  refuses with side identity; ceiling on main passes through under Side).
  218 tests green.
- Cycle 4: `target_entry_index == main_entry_index` assertion added to
  `identity_plan_advertises_stock_values_for_both_entries`; module/fn doc
  comments updated to the target-aware language.

## Validation results

- Validator script: 218 passed / 0 failed (baseline 214; +4 new tests).
- `cargo check --target x86_64-pc-windows-msvc`: clean.
- `cargo fmt`: applied (whole crate).
- `./build.sh`: release DLL built clean.

## Deviations

- T2–T4's red edge was subsumed by Cycle 1's API change: the Side arm is
  a 3-line `match` inseparable from adding the parameter, so the Side
  tests were written against an already-present implementation. Honesty
  preserved by construction: every expectation is computed independently
  of the implementation (`plan_entry_values` recomputation, stock values,
  serializer oracle) rather than snapshotted from its output.
- No commit: AGENTS.md's solo-maintainer workflow ("Commit/push only when
  the maintainer asks") overrides the sop's commit step conservatively.
  All gates verified green as if committing.

## Files changed

- `src/core/xact/virtual_bank.rs` — `StretchTarget`, `target_entry_index`,
  3-arg `plan_virtual_bank`, identity-plan field population, docs.
- `src/core/xact/tests.rs` — pin + 3 Side tests + 2 helpers + identity
  assertion; call spellings.
- `src/services/song_rate/binding.rs` — call spelling only
  (`StretchTarget::Main`).
- `src/services/song_rate/binding_tests.rs`,
  `src/services/song_rate/generator_tests.rs` — call spellings.
- `scripts/validate_song_playback_speed.sh` — embedded-harness call
  spellings.
