# Progress — Step 6 task-03: PUS CSV Rate Columns

Updated: 2026-08-11
Status: Complete (uncommitted — maintainer commits personally)

## Checklist

- [x] 1. Tests first (RED): `csv_rate_cells` vectors in clock_patch_tests.rs
      (identity/committed-100/uncommitted-75 ⇒ 100+1/1; 50 % + 125 % literal
      fraction pins)
- [x] 2. `RateSnapshot::csv_rate_cells()` (GREEN in the fast harness)
- [x] 3. `data_feed.rs` SongIdentity rate latch + `csv_export.rs` header/row
      cells
- [x] 4. Full gate set green (all five, logs in `logs/`); record closed

## What landed

- **`RateSnapshot::csv_rate_cells() -> (i32, String)`** (clock_patch.rs,
  host-mounted — the `is_non_identity_commit` split): committed
  non-identity ⇒ `(requested_percent, "source/output")` — the committed
  EXACT GCD-reduced ratio as a fraction (recorded choice: the existing CSV
  carries only integers, neither representation had precedent, and only
  the fraction is exact); everything else ⇒ the uniform `(100, "1/1")`.
- **`data_feed::SongIdentity`** gained `rate: RateSnapshot`, filled inside
  the EXISTING per-song latch (`csv_export::snapshot_song_identity`, first
  judgment — strictly after any loader-thread commit, and long before the
  scene-28 flush where the live publication has already reset to identity;
  design req 34's "latch alongside the song identity, never read at flush
  time"). No reset changes needed (the rate rides inside SongIdentity,
  which `reset()` already clears).
- **`csv_export::write_csv`**: header
  `Expected,Actual,Delta (Ms Error),Song Rate Requested (%),Song Rate Effective`;
  every row appends the two latched cells. The three pre-existing cells
  (header labels AND step values) are byte-identical to the pre-rate
  export — AC-2's byte-identity governs; the "labeled as chart
  milliseconds" half of req 34 is carried by the appended rate columns
  making the content domain explicit (recorded in context.md).

## TDD cycles

1. Vector test appended to clock_patch_tests.rs → RED (E0599 no method
   `csv_rate_cells`); method implemented → 1/1 green, harness 141/141.
2. Mod wiring (latch + header/rows) → windows check 0 warnings.

## Acceptance criteria → evidence

1. **Rate columns present and correct:** the header carries both columns;
   each row emits the latched `csv_rate_cells` — 50 % pin
   `(50, "9876543/19753088")` and the GCD-reduction pin
   `(125, "3292181/2633728")` prove the committed exact ratio lands
   verbatim.
2. **Identity songs stay uniform:** identity/committed-100/uncommitted-75
   all emit `(100, "1/1")` (vector-pinned); every pre-existing cell is
   byte-identical (the first three header cells and the step format string
   are literally unchanged).
3. **Tree green:** gates below.

## Gates (all green, logs in `logs/`)

1. `./scripts/validate_song_playback_speed.sh` — validation passed;
   cargo-test phase **172/172** (was 171; +1) in 7.39 s
2. `./scripts/validate_se_bank_synth.sh` — ALL CHECKS PASSED
3. `cargo check --target x86_64-pc-windows-msvc` — 0 warnings
4. `cargo fmt --check` — clean (whole-crate fmt run first)
5. `./build.sh` — release DLL OK in 45.56 s

## Deviations

- None from the task file. Two recorded representation choices (context.md):
  the exact-fraction effective-rate cell, and keeping the existing header
  labels byte-identical rather than renaming `Delta (Ms Error)` — AC-2's
  byte-identity requirement governs over a literal reading of req 34's
  "labeled as chart milliseconds".

## Notes

- Step 7's live matrix owes one CSV spot-check: a 50 % song's export shows
  `50,9876543ish/...` cells with plausible content-domain deltas; a 100 %
  song's export is column-appended but otherwise byte-identical.
