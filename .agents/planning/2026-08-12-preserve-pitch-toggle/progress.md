# Progress: Preserve Song Pitch sub-option

Updated: 2026-08-13
Status: **FEATURE COMPLETE** — all 6 plan steps done, cabinet-validated
2026-08-13. Everything uncommitted (maintainer commits, both repos).

**NEXT ACTION:** none — maintainer folds the working trees into commits
(this repo + ../bemani-buddy) at their convenience.

Resume protocol: read `implementation/plan.md` (checklist), the design
(§Components for any code question), and the per-step records under
`tasks/step0*/progress.md`.

## Done

- **PDD:** register accepted, design + plan approved 2026-08-12.
- **Step 1 — resampler core:** `src/core/xact/resample.rs` (frozen
  reference + streaming `ResampleState`, shared `PositionMap`/`interpolate`
  core). 7 host tests. Record: `tasks/step01-resampler-core/`.
- **Step 2 — generator seam:** `DspState::{Wsola, Resample}` in `Feed`;
  `Binding.preserve_pitch` + `prepare_binding` param;
  `transform_bank_oracle_mode` + 2 generator tests (resample-oracle byte
  identity through the serve dispatch; seek-based regen identity). Record:
  `tasks/step02-generator-seam/`.
- **Step 3 — flag carriage:** `runtime::DESIRED_PRESERVE_PITCH` atomics +
  accessors; `EligibilityInputs.desired_preserve`,
  `ArmRequest.preserve_pitch`, `LifecycleState` atomic (identity resets to
  true); `wavebank_hook` passes the lifecycle value; arm log line shows
  `preserve_pitch=`. New `preserve_pitch_latches_from_the_entered_side`
  test. Record: `tasks/step03-flag-carriage/`.
- **Step 4 — UI:** `ShowWhen::NotEquals` (api/registry/rows ×2);
  `preserve_pitch` bool row in `song_playback_speed.rs` (default ON,
  `NotEquals{song_speed,100}`, load clamp, on_change → atomics, Duplicate +
  re-seed + availability); textures generated & visually verified
  (`seop_item_preserve_pitch` + `seop_image_preserve_pitch_{off,on}`);
  script duplicate-entry cleanup. Record: `tasks/step04-option-row/`.
- **Step 5 — backend (../bemani-buddy, stacked on in-flight 012/013):**
  model JSON (both shapes) → codegen → migration 014 (applied to local
  MySQL) → DB model/DAO → handler (load/new/save) + 5 tests →
  `cargo sqlx prepare`. Build OK; 246/0 tests. Record:
  `tasks/step05-backend/`.
- **Step 6 (automatable half):** validation script `resample` report
  section (inverted pitch expectation via new fundamental-true
  `zero_crossing_frequency`; the autocorrelation estimator folds
  subharmonically — real bug caught by the first run) + tail verifier;
  README + AGENTS.md docs. **Full validation run GREEN** (overall_pass
  true; resample 50 %: err 0.0062 %, 175 %: err 0.0123 %). Record:
  `tasks/step06-validation-docs/`.

## Gates (all green, 2026-08-13)

- Host harness (xact + song_rate + siblings): **151 passed / 0 failed**
  (baseline was 148 before the feature; +7 resample, +2 generator, +1
  lifecycle, −7... net counts include suite growth).
- Full `scripts/validate_song_playback_speed.sh`: overall_pass = true.
- bemani-buddy: `cargo build` OK, `cargo test` 246/0.
- `cargo check --target x86_64-pc-windows-msvc` clean; `cargo fmt` applied;
  `./build.sh` release DLL built.

## Deploy & test log

- **2026-08-13 — cabinet end-to-end PASS (maintainer):** multiple songs at
  multiple rates, with and without pitch preservation — audio as expected
  in both modes, and the option round-tripped (persistence confirmed).
  Feature accepted.

### Cabinet checklist (from the design; maintainer-validated)

1. Visibility: row hidden at 100, appears live per-side when speed ≠ 100;
   label/ribbons/previews render.
2. ON at 75 % — identical to today (pitch-preserved).
3. OFF at 75 % — pitch audibly lower, sync + assist ticks correct; OFF at
   150 % — higher; loading no slower than ON.
4. Looping bank at OFF — no click at the loop seam.
5. Quick Restart mid-song at OFF — restart keeps resampled audio.
6. Persistence: card-out/in round trip (server), then JSON-cache path.
7. Containment: rate-played at OFF still suppresses the per-stage save.
8. 100 %: row hidden; identity zero-footprint log.

## Deviations & open questions

- Step 1: zero-capacity `produce` = typed `OutputTooShort` (stretch-contract
  mirror), not the sketch's 0-frame return.
- Step 6: script needed a new `zero_crossing_frequency` helper (design named
  the existing `estimate_frequency`, which is subharmonic-folding and only
  ratio-invariant — unfit for the inverted expectation).
- User directives: backend in scope (done); NO PUS/CSV changes (honored —
  `RateSnapshot` untouched); bemani-buddy fmt churn may stay (none was
  produced); commits are the maintainer's.

## Key facts for a cold resume

- OFF ⇒ resample (pitch follows rate); ON/default ⇒ WSOLA. The plan fixes
  output length/ratio, so clock/ticks/Real-Speed/score-guard are mode-blind.
- The seam: `GeneratorCore::new` reads `binding.preserve_pitch()` once →
  `Feed`'s `DspState`. Regen in resample mode = O(1) seek, no checkpoints.
- Flag latch: entered side at scene-26; identity resets the lifecycle flag
  to true; wire field `mod_preserve_pitch`; backend col
  `opt_mod_preserve_pitch` (migration 014).
- Fast host harness: temp package mounting `core/xact` + `song_rate` + the
  custom_options kernel (mirror of the validation script's scaffold).
