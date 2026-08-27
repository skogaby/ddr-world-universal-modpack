# Progress: Step 2 — Generator/binding mode seam

Covered by the feature plan Step 2 (approved 2026-08-12); executed
autonomously per user instruction ("proceed through all steps").

## Done

- [x] TDD red: `make_binding`/`transform_bank_oracle_mode` signatures +
  2 new tests (`resample_mode_replay_matches_the_resample_oracle`,
  `resample_mode_behind_window_regen_is_identical`) — failed on missing
  `Binding::preserve_pitch`.
- [x] `binding.rs`: `preserve_pitch` field + accessor; `new`/
  `with_ring_capacity`/`prepare_binding` params.
- [x] `wavebank_hook.rs`: passes `true` (hardwired; Step 3 replaces with
  the lifecycle read).
- [x] `binding_tests.rs`: 7 call sites pass `true`.
- [x] `generator.rs`: `DspState::{Wsola, Resample}` (produce/checkpoint
  delegation), mode-aware `Feed::new` / `Feed::positioned_at` (resample arm
  = O(1) seek, no checkpoints, `capture_target = None`), `GeneratorCore`
  reads the binding flag once.
- [x] Green: 150/150 (148 baseline + 2 new) in the extended host harness
  (xact + song_rate + score_guard + movie_policy + custom_options kernel).
- [x] `cargo check --target x86_64-pc-windows-msvc` clean.

## Notes

- The resample-oracle test also asserts the two modes' outputs actually
  differ (guards against a silently-ignored flag).
- The existing WSOLA-mode tests all still pass with `true` — behavior
  unchanged for pitch-preserved (and production callers hardwire `true`
  until Step 3).

## Deviations

(none)
