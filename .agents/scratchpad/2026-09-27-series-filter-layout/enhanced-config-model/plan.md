# Plan — enhanced-config-model

Status: Approved 2026-09-27 (upstream approval; auto mode)

Tests (model.rs `#[cfg(test)]`, run by the harness):
- valid minimal block → 1 cell, no warnings; `series_end` defaults to start.
- `filters: []` → 0 cells; not-an-object → Err.
- num_columns: missing / 0 / 6 / "3" / 2.5 → 2 with one warning each; 1..=5 accepted.
- invalid cells each skipped with one warning: start > end, 256, -1, non-integer, bad texture,
  empty / non-ASCII / NUL label, unknown group, non-object cell.
- cap: 1 column × 30 cells → 24 + one warning; 5 columns × 70 → 64 + one warning.
- 16→15: [16,16]→[15,15]; [14,16]→[14,15]; [16,20]→[15,20]; default group from raw start (16 → WHITE).
- groups: default rule for 1, 13, 14, 17, 18, 21, 0, 22; explicit override; "none".
- members(0/1/2/3) on the canonical plan → CLASSIC 13 / WHITE 3 / GOLD 4 / empty.
- table_rows: N=0 → 10 inert rows; N=1 → 10 rows; N=9 → 10; N=20 → 21; end_excl = end+1; inert = (i32::MAX, 0, "").
- custom_names: first exact single-value ≥ 22 wins; ranges and < 22 ignored.
- label_key / texture_name / label_stems (dedupe, order) / source_candidates / canvas_width.
- fixture: canonical block → 20 cells, 3 columns, zero warnings.

Implementation: serde_json::Value walk; warnings as `String`s; no panics on any input.
