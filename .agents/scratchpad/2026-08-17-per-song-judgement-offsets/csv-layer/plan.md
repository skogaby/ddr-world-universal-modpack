# Plan: csv-layer

Status: Approved 2026-08-17 (inherited from approved plan/design chain, auto mode)

## Test scenarios (written first, must fail before implementation)

1. `round_trip_preserves_content` — parse header+rows (`puty,11,11`,
   `neg,-100,`, `blank,,`) → serialize → identical text (LF-normalized).
2. `parse_without_header` — same rows, no header line → same doc; serialize
   re-adds header.
3. `parse_crlf_and_whitespace` — CRLF file with padded cells → values load.
4. `parse_clamps_out_of_range` — `a,250,-999` → (100, -100), stats.clamped=2.
5. `parse_skips_garbage_lines` — non-integer cell, 4-cell line → both skipped,
   stats.skipped=2 with line numbers recorded.
6. `parse_first_duplicate_wins` — `dup,1,` then `dup,9,9` → value 1, stats
   reports the duplicate.
7. `append_missing_appends_only_new` — existing {a,b}; append [a,c,d] → rows
   a,b,c,d; a/b values untouched; returns 2; c/d blank.
8. `upsert_updates_and_appends` — upsert existing X side0 Some(-7) leaves p2;
   upsert absent Y side1 Some(3) appends row with only p2; upsert to None
   blanks a cell.
9. `empty_and_header_only_inputs` — "" and "code,p1_offset,p2_offset\n" both
   parse to empty docs without stats noise.

## Implementation shape

- `csv.rs`:
  - `pub struct CsvRow { pub code: String, pub offsets: [Option<i8>; 2] }`
  - `pub struct CsvDoc { rows: Vec<CsvRow> }` + `index: HashMap<String, usize>`
    maintained internally for O(1) dedupe/upsert (rebuilt on parse).
  - `pub struct ParseStats { clamped: u32, skipped: u32, duplicates: u32,
    bad_lines: Vec<u32> /* ≤8, 1-based */ }` with an `is_clean()` helper.
  - `parse`, `serialize`, `append_missing`, `upsert`, plus small accessors
    (`rows()`, `get(code)`).
  - Private `parse_cell(&str) -> CellParse` enum (Blank / Value(i8, clamped) /
    Bad) keeps the line loop readable.
- `mod.rs`: module doc + `pub mod csv;`.
- `src/mods/mod.rs`: alphabetical `pub mod per_song_judgement_offsets;` +
  doc-list line.

## Risks
- None notable; pure code. The only contract risk (header heuristic) is
  documented in context.md and covered by scenario 2.
