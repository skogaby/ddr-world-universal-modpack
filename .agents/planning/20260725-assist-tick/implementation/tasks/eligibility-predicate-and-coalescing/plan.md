# Plan — Step 4 Task 01: predicate + coalescing

**Status: Approved** (auto mode — approval inherited from the verified approved plan/design chain
plus the maintainer-approved Step 4 breakdown, 2026-07-26)

## Verification scenarios

1. AC6 (reconciliation): play the Step 3 reference chart; assert from `log.txt` that
   `results == kept + rej_kind + rej_shock + rej_panel + rej_negative + coalesced` and that kept
   < 437 (Step 3's over-permissive figure on the same chart).
2. AC1–AC5 (note-type listening rows): maintainer's, log-assisted by the rejection counts.
3. AC7: diff review — only `build_tick_list` + the log line + new predicate/constants changed.
4. AC8: cargo check / fmt / build.sh.

## Implementation shape

- `const COALESCE_MS: i32 = 4;` with the provisional comment.
- `unsafe fn should_tick(note: *const GameNote) -> bool` — transcription of the research
  reference implementation, with the four ordered tests and the `length[]`/`FREEZE ARROW: OFF`
  doc block.
- `struct RejectCounts { kind: usize, shock: usize, no_panel: usize, negative: usize }` (or four
  locals) threaded through `build_tick_list`; classification mirrors the predicate's order so
  each entry is counted exactly once.
- Coalescing pass: retain-based single walk keeping the earlier timestamp; count merged entries.
- Extend the `song build` log line: `results= kept= rej_kind= rej_shock= rej_panel= rej_neg=
  coalesced= first=[…]`.
