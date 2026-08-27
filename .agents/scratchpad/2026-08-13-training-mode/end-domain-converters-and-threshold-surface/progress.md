# Progress: end-domain converters + chart-end threshold surface

Task: step04/task-01. Mode: auto (verified upstream approval).

## Checklist

- [x] Converter tests written (seek_tests.rs) and failing
- [x] `display_for_raw` / `raw_for_display` implemented, tests green
- [x] End-policy tests written (section_math) and failing
- [x] `EndPolicy` + `end_policy` implemented, tests green
- [x] song_reset surface (`chart_end_thresholds`,
      `set_chart_end_thresholds`, `decoded_notes`, 0x94 offset)
- [x] Gates: harness → cargo check → fmt
- [x] Zero behavior change confirmed (all 242 pre-existing tests green)

## Log

- Setup complete; baseline harness 242/242.
- Cycle 1 (converters): 4 tests appended to
  `src/services/song_reset/seek_tests.rs` — failed with the expected
  E0425 (functions absent); implemented `interpolate_notes` (shared
  private bracketing/extrapolation core, i64 round-half-away) +
  `display_for_raw`/`raw_for_display` in
  `src/services/song_reset/seek.rs`. 246/246.
- Cycle 2 (end policy): exclusivity-table test added to
  `src/mods/training_mode/section_math.rs` — failed E0425; implemented
  `EndPolicy` {WriteThresholds{b_ms}, ArmLoop, Natural} + `end_policy`.
  247/247.
- Cycle 3 (service surface, windows-side): added
  `CMA_CHART_END_DISPLAY_OFFSET = 0x94` beside the raw offset and three
  functions to `src/services/song_reset/mod.rs` —
  `chart_end_thresholds(side)` (both fields, both range-checked, quiet
  Option per the `chart_end_raw` precedent),
  `set_chart_end_thresholds(display, raw)` (validate inputs → resolve
  EVERY live CMA → only then write both fields on each;
  refuse-before-write), `decoded_notes(side)` (side-matched actor →
  mirrored `plan_side_rebuilds` bounds/stride validation →
  `decode_notes`). `cargo check --target x86_64-pc-windows-msvc` clean.
- Validate: harness 247/247 (242 pre-existing + 5 new), check clean,
  `cargo fmt` (whole crate) — no post-fmt drift. No callers added
  (task req 6): the new surface is consumed by tasks 02/03.

## Deviations

- None from the task text. Interpretation calls recorded in context.md:
  full-vector converter domain (control notes included), linear
  extrapolation past the edges from the nearest distinct-key pair
  (clamping would fabricate boundary equality), `plan_side_rebuilds`
  validation MIRRORED not factored (that path is cabinet-validated
  shipped code), quiet Option surface (task-02 owns the WARN-once
  ladder).

## Review notes

- New code follows the file conventions: doc comments cite design §s and
  research §s, `#[must_use]` on pure fns, narrow unsafe blocks, sanity
  ranges reuse `CHART_END_SANE_MAX_MS` (display-domain scale analysis in
  context.md).

Status: Complete (uncommitted — the maintainer commits)
