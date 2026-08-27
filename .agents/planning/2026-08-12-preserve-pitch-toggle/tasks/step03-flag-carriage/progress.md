# Progress: Step 3 — Flag carriage runtime → lifecycle → bind

Covered by the feature plan Step 3 (approved 2026-08-12); autonomous run.

## Done

- [x] TDD red: lifecycle_tests baseline gains distinct per-side
  `desired_preserve: [false, true]` + new
  `preserve_pitch_latches_from_the_entered_side` test (both sides, both
  values, identity ignores) — compile-red on missing fields.
- [x] `lifecycle.rs`: `EligibilityInputs.desired_preserve: [bool; 2]`,
  `ArmRequest.preserve_pitch`, `classify_scene26` emits the entered side's
  flag, `LifecycleState.preserve_pitch: AtomicBool` (default true; reset to
  true on identity, stored on arm) + getter.
- [x] `runtime.rs`: `DESIRED_PRESERVE_PITCH: [AtomicBool; 2]` (default
  true) + `set_desired_preserve_pitch`/`desired_preserve_pitch`;
  `EligibilityInputs` construction reads them; arm INFO log line now shows
  `preserve_pitch=<bool>` (cabinet observability per plan demo).
- [x] `wavebank_hook.rs`: Step 2's hardwired `true` replaced with
  `ctx.lifecycle.preserve_pitch()` — Quick Restart re-binds inherit the
  latched value automatically.
- [x] ArmRequest literals in binding/transaction/wavebank_hook tests
  updated (`preserve_pitch: true`).
- [x] Green: 151/151 in the extended host harness (incl.
  `preserve_pitch_latches_from_the_entered_side`);
  `cargo check --target x86_64-pc-windows-msvc` clean.

## Notes

- Existing per-side-selection tests now double as flag-latch coverage: the
  baseline carries distinct per-side flags ([false, true]) mirroring the
  distinct desired percents.
- Nothing writes the atomics yet (default true = WSOLA) — cabinet behavior
  unchanged until Step 4's option row.

## Deviations

(none)
