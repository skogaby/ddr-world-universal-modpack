# Task: One-time pre-seed script and repo-committed judgement_offsets.csv

## Description
Create `scripts/gen_judgement_offsets_csv.py`, a one-time generator that
cross-references a community-maintained, mcode-keyed sync-offset list against
the game's musicdb to produce a pre-seeded `judgement_offsets.csv` (committed
at the repository root, next to `mod-config.json`), with P1 and P2 seeded
identically. Run it against the real data and commit both the script and its
output.

## Background
A friend of the maintainer maintains a list of known-correct judgement offsets
keyed by each song's numeric `mcode`. The runtime feature keys everything by
the alphabetical `basename` instead (the code the song wheel exposes). The
game's musicdb (`data/arc/startup.arc` entry `data/gamedata/musicdb.xml`,
~1461 `<music>` entries) carries both fields, so a one-time scrape maps
mcode → basename.

Known properties of the real inputs (verified during planning):
- The friend's file is ASCII, CRLF, one `mcode<whitespace>offset` pair per
  line, 1441 lines; offsets span −23..+34.
- Exactly one line has three fields (`449 2 -6`); policy: take the FIRST
  value, print a warning.
- All 1441 mcodes resolve against the musicdb; 20 musicdb songs have no
  friend value (their rows get blank offsets).

The install location and the friend's file live OUTSIDE the repository — both
are CLI arguments (the maintainer's environment provides them; the script
must not hardcode any local path). `scripts/validate_musicdb.py` shows the
established pattern for extracting/parsing musicdb via the repo's
`scripts/arc_tool.py` (`KonamiLz77`, `_read_cstring`, cue-table parsing).

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-17-per-song-judgement-offsets/design/detailed-design.md
  (sections: Components → Tooling; Data Models → CSV; Detailed Requirements 10)

**Note:** Read the design document before beginning implementation.

## Technical Requirements
1. `scripts/gen_judgement_offsets_csv.py`, python3, stdlib only, reusing
   `arc_tool.py` for ARC extraction (same import pattern as
   `validate_musicdb.py`).
2. CLI: `gen_judgement_offsets_csv.py <ddr-install-dir> <offsets-file>
   [--out judgement_offsets.csv]` — default output is `judgement_offsets.csv`
   in the current directory.
3. Behavior:
   - Extract + parse musicdb → ordered list of (mcode, basename) preserving
     musicdb order; duplicate basenames (shouldn't exist) keep first, warn.
   - Parse the offsets file: tolerate CRLF/whitespace; lines with ≥3 fields
     use the FIRST value and are printed as warnings with line numbers;
     blank/comment-ish unparseable lines warned and skipped; values clamped
     to −100..+100 with a warning if any clamp occurs.
   - Emit CSV: header `code,p1_offset,p2_offset`; one row per musicdb entry
     in musicdb order; mapped songs get `code,v,v`; unmapped get `code,,`.
   - Friend mcodes absent from musicdb: printed and skipped (count in
     summary).
   - Summary line: total rows, seeded rows, blank rows, skipped/warned
     counts.
4. Exit non-zero on hard failures (missing arc, no music entries); warnings
   do not fail the run.
5. Run the script against the real install (`DDR_WORLD_INSTALL` env) and the
   friend's offsets file; place the output at the repository root as
   `judgement_offsets.csv` for the maintainer to commit. Expected: 1461 rows,
   1441 seeded, 20 blank, one 3-field warning (mcode 449).
6. The generated CSV must parse cleanly with the runtime layer
   (`src/mods/per_song_judgement_offsets/csv.rs::parse` → `is_clean()`);
   prove it with a host-harness test that parses the committed file when
   present.

## Dependencies
- Step 1's `csv.rs` (for the compatibility proof in requirement 6).
- `scripts/arc_tool.py` (existing).

## Implementation Approach
1. Model the script on `validate_musicdb.py`'s extraction helpers; parse
   `<music>` blocks with stdlib `re` or `xml.etree` (musicdb is plain XML
   once extracted).
2. Keep parsing/emit logic in small functions with a `main()` — no tests
   required for a one-time script beyond its own summary/verification output,
   but the CSV compatibility check (req 6) lives in the Rust harness suite.
3. Run against real data; eyeball the warnings; stage the CSV at repo root.

## Acceptance Criteria

1. **Correct mapping on real data**
   - Given the real install and the friend's file
   - When the script runs
   - Then it reports 1461 rows / 1441 seeded / 20 blank / 0 unmapped-mcodes,
     and warns exactly once about the 3-field line (taking value 2 for
     mcode 449 → basename `aaaa`)

2. **P1 = P2 seeding**
   - Given any mapped song
   - When the CSV is inspected
   - Then both offset cells carry the same value

3. **Runtime compatibility**
   - Given the committed `judgement_offsets.csv`
   - When parsed by `csv::parse` in the host harness
   - Then the parse is clean (no clamps, skips, or duplicates) and row count
     matches

4. **No environment leakage**
   - Given the script and CSV as committed
   - When inspected
   - Then no absolute paths, usernames, or machine-specific strings appear

## Metadata
- **Complexity**: Low
- **Labels**: python, tooling, one-time, csv
- **Required Skills**: Python, Rust (harness test)
- **Generated By**: code-task-generator 2026-08-17
- **Source Plan**: .agents/planning/2026-08-17-per-song-judgement-offsets/implementation/plan.md
- **Plan Step**: Step 2
