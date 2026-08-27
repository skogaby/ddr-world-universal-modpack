# Progress — task-01 retire-cache-model

## Checklist

- [x] 1. core/xact: virtual_bank.rs relocation + delete transform.rs + trim tests.rs
- [x] 2. song_rate: delete cache/worker/tests/model; conversion.rs → binding.rs (+ binding_tests.rs); mod.rs
- [x] 3. xact_runtime.rs trim + test fallout
- [x] 4. transaction.rs trim + test fallout
- [x] 5. wavebank_hook.rs readiness reshape + test fallout
- [x] 6. runtime.rs trim
- [x] 7. file_hooks.rs seam removal
- [x] 8. lib.rs / config.rs / mod-config.json
- [x] 9. validator script in-place rework
- [x] 10. all five gates green

## Record

- 2026-08-09: setup + explore + plan complete (auto mode; upstream approvals verified).
- 2026-08-09: crate-side removal complete (checklist 1–8). First windows-target
  `cargo check` after the sweep: clean, 0 warnings. Residual grep (AC1): only the
  deliberately retained `MaintenanceKind::Quarantine` / `XactSlotPhase::Quarantined`
  vocabulary (task-02 renames the former) plus unrelated `Ordering::Release` noise.
  New tests: `virtual_bank.rs` inline suite (6 tests, ported transformer vectors);
  `binding_tests.rs` (3 tests: path gating port, digest properties, refusing
  preflight → EarlyFailed).

## Gate results (2026-08-09)

1. `./scripts/validate_song_playback_speed.sh` — PASS (harness `cargo test`: 111/111;
   all report checks green; report keys: checks/corpus/identity_runtime/mode/
   overall_pass/platform/schema/sibling_revision/synthetic/thresholds — no `cache`,
   no `on_demand`, schema still `song-rate-validation/v1`).
2. `./scripts/validate_se_bank_synth.sh` — PASS (ALL CHECKS PASSED).
3. `cargo check --target x86_64-pc-windows-msvc` — clean, 0 warnings.
4. `cargo fmt` (whole crate) — clean (`--check` empty).
5. `./build.sh` — release DLL built.

Test-count reconciliation: 156 at ee0368f − 23 (song_rate/tests.rs) − 25
(conversion_tests.rs) − 6 (transformer suite in xact/tests.rs) + 6 (virtual_bank
inline suite) + 3 (binding_tests.rs) = 111. ✓

## Deviations

- **No commit** (SOP Step 6): the handoff instruction is explicit — the maintainer
  commits personally; committing/pushing is forbidden in this repo. The tree is left
  green and staged-ready; recorded here instead of a commit hash.
- Validator: the synthetic/corpus DSP sections kept their checks by gaining a
  harness-local `transform_bank` oracle composed from the surviving crate
  primitives (parse → virtual_bank plan → decode → stretch → encode →
  stream-write); the admission-tied fields (`peak_memory_bytes`, the two
  memory-ceiling thresholds) died with the admission. The `output_length`
  postcondition is now planned-length (serialized_song_bank_len) vs written bytes.
- Gotcha for future heredoc edits: the validator's main.rs heredoc is UNQUOTED —
  backticks in inserted doc comments execute as command substitution (hit once,
  fixed by dropping the backticks).
- `MaintenanceKind::Quarantine` retained (sole surviving kind; the late-fail
  protocol + saturation fault tests need the push) — task-02 renames it to
  `ReclaimBinding` per its req 2.

Status: Complete (uncommitted — maintainer commits personally per handoff instruction)
