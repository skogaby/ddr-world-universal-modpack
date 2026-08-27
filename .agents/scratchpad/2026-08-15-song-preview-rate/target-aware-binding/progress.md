# Progress: task-02 Target-aware Binding runtime

Updated: 2026-08-15
Status: Complete (uncommitted — maintainer-managed commits per AGENTS.md
workflow; all readiness gates green)

## Checklist

- [x] Cycle 1: binding.rs + generator.rs vocabulary refactor (Main
      semantics) — suites green unchanged (218)
- [x] Cycle 2: prepare_binding target parameter + 12-call-site sweep
- [x] Cycle 3: Side-target tests T2–T5 (generalized oracle/make_binding)
- [x] Cycle 4: validate (validator, windows check, fmt, build.sh)
- [x] Close-out (commit deferred to maintainer, per task-01 precedent)

## Cycles

- Cycle 1: `Binding` fields → `verbatim_entry` / `verbatim_source_offset` /
  `target_source_offset` / `target_source_len`; `build` derives from
  `layout.target_entry_index` (ring base, verbatim entry, identity guard);
  serve dispatch / `check_spans` / `copy_spans[_silent]` key on
  target-vs-verbatim; `copy_mapped_main` → `copy_mapped_target`;
  `main_data_start/end` → `target_data_start/end`; `ms_to_blocks` +
  `active_content_grid` + generator entry selection/regen guard → target;
  doc comments rewritten to the target/verbatim vocabulary. Validator 218
  green unchanged; windows check clean.
- Cycle 2: `prepare_binding(..., target: StretchTarget)` — plan through
  target, rate from `layout.entries[target_entry_index].rate`,
  `debug_assert!(target == Main)` on the identity (100%) arm; sweep:
  `wavebank_hook.rs` (Main) + 11 binding_tests call sites. 218 green.
- Cycle 3: generalized `transform_bank_oracle_target` (pub(super)) +
  `make_binding_target`; new tests:
  `side_target_replay_matches_the_oracle` (2 orders × 50/175 % × both DSP
  modes, full engine replay vs Side oracle + ring-range containment),
  `main_entry_prepare_read_completes_without_side_production` (verbatim
  main serves synchronously, zero production),
  `side_target_retire_cancels_pending` (AC5),
  `prepare_binding_side_target_streams_the_side_entry` (rate wiring +
  spawned-producer packet vs oracle). Validator **222** green.
- Cycle 4: fmt; windows check clean; validator 222; `./build.sh` release
  DLL clean.

## Validation results

- Validator script: 222 passed / 0 failed (218 post-task-01 baseline + 4).
- `cargo check --target x86_64-pc-windows-msvc`: clean.
- `cargo fmt`: applied.
- `./build.sh`: release DLL built clean.

## Deviations

- Cycle 3's tests were green on first run: the Cycle-1/2 refactor IS the
  Side capability (anticipated in plan.md — "the tests are the proof
  against independent oracles"). No implementation was shaped by the
  tests' failures; honesty comes from the independent whole-buffer
  oracles and stock-byte comparisons.
- No commit (maintainer-managed; task-01 precedent).

## Files changed (this task)

- `src/services/song_rate/binding.rs` — field/method renames, target-aware
  build + dispatch, `prepare_binding` target parameter.
- `src/services/song_rate/generator.rs` — target-relative accessors + docs.
- `src/services/song_rate/generator_tests.rs` — generalized oracle +
  builder, 3 new tests.
- `src/services/song_rate/binding_tests.rs` — call-site sweep + 1 new test.
- `src/services/song_rate/wavebank_hook.rs` — `StretchTarget::Main` at the
  production bind.

Status: Complete (uncommitted — see above)
