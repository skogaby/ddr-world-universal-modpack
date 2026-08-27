# Progress — Step 3 task-03: The Validator's Replay Legs (Step 3 Demo)

Updated: 2026-08-09
Status: Complete (uncommitted — maintainer commits personally)

## Checklist

- [x] `StreamingReplayResult` (percent, entry_order, packet_count,
      reassembled_len, read_pattern, reparsed, matches_reference,
      decode_equality, passed) + `replays` on `StreamingReport` +
      `replay_virtual_bank` compact re-derivation + two legs wired into
      `validate_streaming` (new `reverse_order` parameter; `main()` call
      site updated). No backticks / dollar signs in inserted Rust.
- [x] Validator run: console PASS `streaming_replay_50`
      (order=main-preview, packets=2, reassembled=38400B) and
      `streaming_replay_175` (order=preview-main, packets=2,
      reassembled=13372B) — the plan Step 3 demo statement. Report
      inspected by script: both replays present with all five booleans
      true, `overall_pass` true, schema `song-rate-validation/v1`, no
      cache/on_demand keys, Step-2 rates/throughput untouched, fps finite.
- [x] Python-gate spot-check via mutated report COPIES in /tmp (extracted
      gate run standalone): failing `streaming_replay_50` check → exit
      nonzero; false `streaming.passed` → exit nonzero; pristine report
      accepted. Nothing committed.
- [x] Gate 1: validator green — 130/130 host tests + all report checks,
      validation passed (`logs/validator.log`)
- [x] Gate 2: se-bank ALL CHECKS PASSED (`logs/se-bank.log`)
- [x] Gate 3: windows check 0 warnings (`logs/check-windows.log`)
- [x] Gate 4: fmt clean (whole crate)
- [x] Gate 5: build.sh release DLL OK (`logs/build.log`)
- [x] Plan Step 3 ticked (all three sibling task records carry
      `Status: Complete`)
- [x] NO commit (maintainer commits personally)

## Implementation notes

- The compact feed satisfies the required pipeline naming with the
  EXISTING harness pieces: `drive_streaming` (= `StretchState::produce`
  over `adpcm::BlockCachePcm`, chunk 997) followed by per-whole-block
  `adpcm::encode_block`, length asserted == plan `data_len`. The
  pull-driven on-demand variant is task-02's exhaustive cargo-test proof;
  this is the representative, independently re-derived release-run form
  (Step 2 precedent).
- Leg assignment: 50% on the main-first fixture, 175% on the preview-first
  fixture (requirement: at least one reverse-order leg).
- `read_pattern` folds the pump-fidelity facts into the leg (full 0x1000
  header serve, every packet served in full, `offset + served ≤
  virtual_size`, defensive past-the-end read serves 0) so a pattern break
  fails the visible check, not just an internal assert.
- `StreamingReport::passed` conjoins `replays.iter().all(passed)`
  explicitly alongside the existing rates/checks conjunctions
  (belt-and-braces; the python gate already fails on any failing
  `streaming.checks[]` entry — verified structurally unchanged, R6).

## Deviations

- `decode_equality` verifies BOTH entries (superset of the acceptance
  criterion's "main entry decodes equal") — same cost class, stronger
  statement, matching task-02's exhaustive legs.
- `packet_count` in the report counts data-packet reads only; the header
  and defensive EOF reads are pattern-checked (inside `read_pattern`)
  rather than counted, keeping the field meaningful per entry stream.

Status: Complete (uncommitted — maintainer commits personally)
