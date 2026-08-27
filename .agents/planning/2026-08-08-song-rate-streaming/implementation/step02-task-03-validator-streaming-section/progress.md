# Progress — Step 2 task-03: The Validator's Streaming Report Section

Updated: 2026-08-09
Status: Complete (uncommitted — maintainer commits personally)

## Checklist

- [x] Python streaming leg added FIRST; proven to reject the then-current
      section-less report (exit 1: "report streaming section is missing or
      failed")
- [x] Structs (`StreamingReport`, `StreamingRateResult`) + `drive_streaming`
      + `validate_streaming()` + `main()` wiring in the harness heredoc
      (no backticks / dollar signs in inserted Rust — Step 1's gotcha honored)
- [x] Full validator green: all six `streaming_{25,50,75,100,125,175}` checks
      PASS (bytes/counters/chunking/checkpoint all true), `overall_pass`
      conjoins `streaming.passed`, schema still `song-rate-validation/v1`
      (`logs/validator.log`)
- [x] Report inspected by script: rates exactly [25,50,75,100,125,175], all
      four legs true per rate, no cache/on_demand, fps finite
- [x] Failing-check spot-check: the gate rejects a missing section, a false
      `passed`, and a failing check — via mutated report COPIES in /tmp,
      nothing committed
- [x] Gates: validator green (123/123 + report checks), se-bank ALL CHECKS
      PASSED, windows check 0 warnings, fmt clean, build.sh OK
- [x] NO commit (maintainer commits personally)

## Implementation notes

- The release matrix exercises the PRODUCTION cell shape per rate: output
  frames + full-entry LoopContext derived through the real planning path
  (`virtual_bank::plan_entry`), source served through the real on-demand view
  (`adpcm::BlockCachePcm`), reference = `decode_interleaved` +
  `stretch_interleaved`. Per task-02's discovery, 25%/50% REQUIRE the
  full-entry loop (NoCandidate otherwise), so `loop_shape: "full-entry"` is
  recorded per rate.
- Throughput: 24 repeated whole 75% stretches through `measure_operation`;
  ms clamped to ≥ 1 keeps the f64 finite for serde_json. Recorded in
  `synthetic_frames_per_second` + an always-passed `streaming_throughput`
  check (figure in the detail); it appears in NO pass expression — verified
  by inspection (AC2). Observed ≈ 1.65M frames/sec synthetic at 8 kHz stereo
  on this host (≈ 206× realtime against a 8 kHz source; informational only —
  the real gate is plan Step 5's live cabinet benchmark).

## Deviations

- Guidance-vs-requirement conflict resolved toward the requirement:
  checkpoint-restore is exercised PER RATE (Technical Requirement 1) rather
  than as the single leg the Implementation Approach sketched — cheap in the
  release build and it makes every rate row carry all four booleans.
- The task sketched "one interior-loop and one no-loop context, stereo" for
  the release matrix; per task-02's reference-envelope discovery those shapes
  FAIL at 25%/50% (`NoCandidate`). The section uses the production full-entry
  loop shape for all six rates instead; interior/no-loop coverage lives in the
  exhaustive `cargo test` matrix where it asserts behavior parity.

Status: Complete (uncommitted — maintainer commits personally)
