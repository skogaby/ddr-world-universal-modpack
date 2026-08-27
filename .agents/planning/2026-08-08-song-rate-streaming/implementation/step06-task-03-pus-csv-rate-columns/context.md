# Context — Step 6 task-03: PUS CSV Rate Columns

Task file: `.agents/tasks/song-rate-streaming/step06/task-03-pus-csv-rate-columns.code-task.md`
Mode: auto (code-assist). Approval chain verified at setup (same as tasks
01/02). Design req 34 (KEEPER): PUS keeps content-domain error values
labeled as chart milliseconds and adds requested/effective rate columns to
CSV export.

## Territory (matches the task file — no premise issues)

- `src/mods/power_user_statistics/csv_export.rs`: `write_csv` emits header
  `Expected,Actual,Delta (Ms Error)\r\n` + one `{},{},{}\r\n` row per step.
  `snapshot_song_identity(actor, side)` latches `SongIdentity
  {songcode, difficulty}` from the DancePlaySequence on the FIRST judgment
  of a song (judge_submit detour — strictly after any loader-thread commit,
  the same timing class as assist_tick's anchor). `flush()` on scene
  28 → non-28 takes `per_step` + `song_identity` and writes the file.
- `src/mods/power_user_statistics/data_feed.rs`: `SongIdentity` struct,
  `MsErrorAccum` buffers, `reset()` clears `song_identity` (the new rate
  ride-along needs no reset changes — it lives inside SongIdentity).
- The mod is not host-mounted; the host-testable extraction follows the
  `is_non_identity_commit` precedent: the rate-cell composition lives on
  `RateSnapshot` in `clock_patch.rs` (harness-mounted), tested in
  `clock_patch_tests.rs`.

## Requirements

1. Two new columns appended after the existing three: requested rate
   (percent int) + effective rate; header updated; every row carries them
   (AC-1 "each row carries requested=50 and the committed exact effective
   rate"). Identity/uncommitted songs emit the uniform identity values
   (100 and 1/1).
2. Rate latched per song at the EXISTING song-identity snapshot boundary
   (`snapshot_song_identity`), never read at flush time (the publication
   resets to identity at gameplay exit).
3. ms-error columns keep values AND labels unchanged (AC-2's byte-identity
   of every pre-existing column governs — the first three header cells and
   all step cells stay literally today's bytes; only the appended cells are
   new).
4. Host tests: identity + non-identity vectors for the cell composition.

## Decisions recorded (auto mode)

- **Effective-rate representation: the exact fraction
  `source_frames/output_frames`** (e.g. `9876543/19753088`), not a decimal.
  The design says "the committed exact ratio — emit as a fraction or
  sufficiently precise decimal; pick what the existing CSV conventions
  favor and record the choice": the existing CSV has no float precedent
  (all ints), and only the fraction is EXACT — a decimal silently rounds
  the committed ratio. Identity = `1/1`.
- **Header cells:** `Song Rate Requested (%)` and `Song Rate Effective`,
  appended — matching the existing parenthesized-unit style
  (`Delta (Ms Error)`).
- **"Labeled as chart milliseconds"** (req 34's first half) is satisfied
  WITHOUT renaming the existing `Delta (Ms Error)` header cell: AC-2 pins
  every pre-existing column byte-identical, so the label stays; the
  appended rate columns are what make the content-domain interpretation
  explicit. Recorded as the deliberate reading of "unchanged" in the task
  text.
- **Latch shape:** `SongIdentity` gains `rate: RateSnapshot`, filled from
  `clock_patch::snapshot()` inside `snapshot_song_identity` (first
  judgment — post-commit guaranteed). The uniform identity emission falls
  out of `csv_rate_cells()`'s `!is_non_identity_commit()` branch, so an
  uncommitted/armed-failed song emits 100 + 1/1 exactly like a plain song.

## Build / test commands

Same five gates as tasks 01/02; the pure cells method runs in the fast
harness (clock_patch_tests.rs).
