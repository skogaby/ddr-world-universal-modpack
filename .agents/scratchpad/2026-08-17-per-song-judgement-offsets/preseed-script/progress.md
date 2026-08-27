# Progress: preseed-script

Updated: 2026-08-17
Status: Complete (uncommitted — maintainer commits manually)

## Checklist
- [x] Setup / Explore (validate_musicdb.py extraction pattern reused)
- [x] Plan (inline; approval inherited from approved plan/design chain)
- [x] Script implemented (`scripts/gen_judgement_offsets_csv.py`)
- [x] Run against real data — matched every prediction:
      1461 rows / 1441 seeded (P1=P2) / 20 blank / 0 unmapped;
      exactly one warning: line 123 `449 2 -6` → first value (2) taken
      (aaaa,2,2 in output). Spot check: puty,11,11 ✓ (friend line `10 11`).
- [x] Runtime compatibility proof: `committed_preseed_csv_parses_clean` test
      added to csv.rs; harness exports JUDGEMENT_OFFSETS_CSV when the file
      exists at repo root. Harness: 19 passed / 0 failed.
- [x] Validate (cargo check windows target clean; cargo fmt no churn)
- [x] Work complete; left staged (maintainer commits manually per AGENTS.md)

## Deviations
- Task said "place the output at the repository root … for the maintainer to
  commit" — done; `judgement_offsets.csv` is staged, not committed.

## Results
- `judgement_offsets.csv` (repo root): 1462 lines incl. header.
- Warnings log: logs/warnings.log (single 3-field warning).
- Harness log: logs/harness.log (19 tests green).

Status: Complete (uncommitted — maintainer commits manually)
