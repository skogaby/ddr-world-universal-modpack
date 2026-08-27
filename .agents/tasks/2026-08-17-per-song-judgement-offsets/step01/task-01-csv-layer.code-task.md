# Task: CSV layer and module scaffolding for per-song judgement offsets

## Description
Create the new mod module `src/mods/per_song_judgement_offsets/` with its
module gate and the pure CSV layer (`csv.rs`) that parses, serializes, and
append-merges the `judgement_offsets.csv` file. No game APIs, no Mod-trait
registration yet — this is the host-testable foundation the rest of the
feature builds on.

## Background
The feature stores per-song judgement offsets in a CSV living next to
`mod-config.json` (CWD-relative at runtime). Schema: header
`code,p1_offset,p2_offset`; `code` is the musicdb basename (opaque ASCII
string); offset cells are optional integers in −100..+100 (blank = unset for
that side). The file is machine-managed: a boot-time crawl appends missing
song codes with blank cells (never modifying existing rows), and options-menu
edits upsert individual cells. A repo-committed pre-seeded CSV will be
generated later by a script and must parse with this layer.

The crate is a Windows-target hook DLL, but pure modules are host-tested with
`cargo test` (see `src/services/chart_length.rs` and `src/core/ssq/` for the
pattern). There is no CSV crate dependency — parsing is hand-rolled (the only
existing CSV code, `src/mods/power_user_statistics/csv_export.rs`, is
write-only).

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-17-per-song-judgement-offsets/design/detailed-design.md
  (sections: Data Models → CSV; Detailed Requirements 4, 5; Error Handling table)

**Note:** Read the design document before beginning implementation.

## Technical Requirements
1. New module `src/mods/per_song_judgement_offsets/mod.rs` declaring
   `pub mod csv;` (and nothing else functional yet), wired into
   `src/mods/mod.rs` so the crate compiles with the new module present.
2. `csv.rs` exposes a document model and pure functions:
   - `CsvDoc` (or similar): ordered rows of
     `{ code: String, offsets: [Option<i8>; 2] }`, preserving file order.
   - `parse(text: &str) -> (CsvDoc, ParseStats)` — header row tolerated if
     absent; CRLF- and LF-tolerant; whitespace-trimmed cells; out-of-range
     values clamped to −100..+100; unparseable lines skipped; `ParseStats`
     reports clamped/skipped counts (and up to a few offending line numbers)
     so the caller can emit one aggregated WARN.
   - `serialize(&CsvDoc) -> String` — always writes the header, preserves row
     order, blank cells for `None`.
   - `append_missing(&mut CsvDoc, codes: impl Iterator<Item = &str>) -> usize`
     — appends codes not already present (dedupe against existing, case
     preserved), blank offsets, returns count appended; never touches existing
     rows.
   - `upsert(&mut CsvDoc, code: &str, side: usize, value: Option<i8>)` —
     updates one cell, appending the row if the code is absent.
3. Duplicate codes in an input file: first occurrence wins; later duplicates
   are reported in `ParseStats` and dropped (documented behavior — the file is
   machine-managed).
4. No `unsafe`, no game/OS APIs, no logging inside the pure layer (callers
   log); `i8` value type per the design.
5. `cargo fmt` clean; crate-wide `cargo check --target x86_64-pc-windows-msvc`
   passes; `cargo test` passes on host.

## Dependencies
- None (first task of the feature). `src/mods/mod.rs` is the only existing
  file touched.

## Implementation Approach
1. Add the module gate and empty `mod.rs`.
2. Implement `csv.rs` types and functions, TDD-style: write the parse/serialize
   round-trip tests first, then the merge/upsert behaviors.
3. Keep parsing strictly line-oriented (`split(',')` after line trim) — codes
   are known to never contain commas or quotes; document that assumption.

## Acceptance Criteria

1. **Round-trip fidelity**
   - Given a well-formed CSV with a header, values, negatives, and blank cells
   - When parsed and re-serialized
   - Then the output is byte-identical (modulo normalized line endings)

2. **Tolerant parse**
   - Given a file with a missing header, CRLF endings, an out-of-range value
     (e.g. 250), a garbage line, and a duplicate code
   - When parsed
   - Then good rows load, the value is clamped to 100, the garbage line and
     duplicate are skipped, and `ParseStats` reports each condition

3. **Append-only merge**
   - Given a parsed doc and a code list containing both existing and new codes
   - When `append_missing` runs
   - Then existing rows are untouched (order and values), only new codes are
     appended with blank cells, and the count returned matches

4. **Cell upsert**
   - Given a doc with a row for code X and none for code Y
   - When upserting X side 0 = Some(-7) and Y side 1 = Some(3)
   - Then X's p1 cell becomes -7 (p2 untouched) and Y is appended with only p2
     set

5. **Build hygiene**
   - Given the completed change
   - When `cargo check --target x86_64-pc-windows-msvc`, `cargo test`, and
     `cargo fmt` run
   - Then all pass with no unrelated churn

## Metadata
- **Complexity**: Low
- **Labels**: rust, pure-layer, csv, foundation
- **Required Skills**: Rust
- **Generated By**: code-task-generator 2026-08-17
- **Source Plan**: .agents/planning/2026-08-17-per-song-judgement-offsets/implementation/plan.md
- **Plan Step**: Step 1
