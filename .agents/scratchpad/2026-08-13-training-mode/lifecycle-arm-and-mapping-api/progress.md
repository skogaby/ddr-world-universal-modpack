# Progress — task-02 lifecycle arm + mapping API

## Checklist

- [x] Baseline: harness green post-task-01 (189/189)
- [x] Tests: training-arm classification (armable at 100, pin intact, gates unweakened) + identity-arm movie clear (lifecycle_tests, +3)
- [x] Tests: identity preflight (passthrough, no producer), `identity-bind-refused` leg, registry mapping API (binding_tests, +3)
- [x] Tests: identity-commit composition — no taint/ledger/movie, snapshot identity (wavebank_hook_tests, +1)
- [x] Impl: `EligibilityInputs.training_arm` + classifier gate + conditional movie suppression (lifecycle.rs)
- [x] Impl: `FaultSelector.identity_bind_refused` + commit-leg skip for identity tokens (transaction.rs)
- [x] Impl: `prepare_binding` identity split (plan_identity_bank → new_identity_passthrough, no spawn) + `BindingRegistry::set_active_content_mapping` (binding.rs)
- [x] Impl: `set_training_arm`/`training_arm_requested`/`set_content_mapping` + scene-26 session-read wiring (runtime.rs, windows glue)
- [x] Validate: harness **196 passed / 0 failed** (189 baseline + 7 new)
- [x] Validate: `cargo check --target x86_64-pc-windows-msvc` clean, `cargo fmt`, `./build.sh` release DLL built
- [x] Close (no commit — maintainer handles git)

## TDD cycles

1. Wrote the 7 tests first → confirmed compile-fail on the missing surface
   (`training_arm` field, `set_active_content_mapping`) plus the
   would-fail-at-runtime fault-leg parse.
2. Implemented lifecycle → transaction → binding → runtime; suite green on
   the first post-implementation run (196/196).

## Deviations

- None beyond context.md's recorded auto-mode decisions (identity arm emits
  an explicit `Movie(false)`; `identity-bind-refused` reuses the `Injected`
  wire code; identity path skips producer-only faults; bind-time mapping =
  the runtime API called post-publication, no `prepare_binding` signature
  change).

## Files changed

- `src/services/song_rate/lifecycle.rs` — `training_arm` input, classifier
  gate, conditional arm-time movie suppression
- `src/services/song_rate/transaction.rs` — fault leg + identity commit skip
  (no ledger/taint/movie for percent-100 tokens)
- `src/services/song_rate/binding.rs` — `prepare_binding` identity split,
  `BindingRegistry::set_active_content_mapping`
- `src/services/song_rate/runtime.rs` — TRAINING_ARM atomic + wrappers +
  scene-26 wiring (session reads when training is requested)
- Tests: lifecycle_tests (+3), binding_tests (+3), wavebank_hook_tests (+1)

Status: Complete (uncommitted — maintainer handles git)
