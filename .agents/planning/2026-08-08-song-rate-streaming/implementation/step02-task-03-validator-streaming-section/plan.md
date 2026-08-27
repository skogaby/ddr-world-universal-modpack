# Plan — Step 2 task-03: The Validator's Streaming Report Section

Status: Approved 2026-08-09 (via the maintainer-approved Step 2 task breakdown;
Source Plan and design `Status: Approved 2026-08-08` — verified in context.md.
Auto mode per handoff instruction.)

## Test-first shape

The "failing test" is the extended python schema gate: add the `streaming`
presence + passed + all-checks leg FIRST and prove it rejects the CURRENT
report (which lacks the section) by running the heredoc's python against
`target/song-rate-validation/report.json` directly (instant, no full validator
cycle). Then implement the section and run the full validator to green.
Acceptance criterion 3's failing-check spot-check is done the same way with a
temporarily mutated report copy (never committed).

## Implementation

1. Python leg (mirrors `identity_runtime`): missing/failed `streaming` or any
   failed `streaming.checks[]` → nonzero exit.
2. Harness structs: `Report.streaming: StreamingReport`;
   `StreamingReport { passed, synthetic_frames_per_second: f64, rates:
   Vec<StreamingRateResult>, checks: Vec<Check> }`;
   `StreamingRateResult { percent, loop_shape, source_frames, output_frames,
   byte_equality, counters_match, chunking_independent, checkpoint_restore,
   passed }`.
3. `validate_streaming(source: &[u8]) -> StreamingReport`:
   - Parse the synthetic bank; take the MAIN entry; whole-buffer oracle =
     `decode_interleaved` + `stretch_interleaved`.
   - Per rate in [25, 50, 75, 100, 125, 175]: `virtual_bank::plan_entry`
     (real planning path → output frames + full-entry mapped LoopContext);
     streaming runs over `adpcm::BlockCachePcm` (real on-demand decode view):
     (a) whole-run produce → byte equality + counters vs the reference;
     (b) 997-frame chunked run → chunking independence;
     (c) hop-chunked run capturing the first checkpoint at/past the midpoint
     → restore → suffix equality from `resume_frame()`.
     One `check("streaming_{percent}", …)` line each; a per-rate result row.
   - Throughput: `measure_operation` over 24 repeated whole 75% stretches;
     frames/sec = total frames / max(latency_ms, 1) · 1000. Recorded in the
     struct + an ALWAYS-PASSED `streaming_throughput` check (figure in the
     detail text) — provably outside every pass expression.
   - `passed` = all rate rows passed AND all checks passed (throughput check
     constant true).
4. `main()`: `let streaming = validate_streaming(&source);` before assembly;
   `overall_pass = checks && identity_runtime.passed && streaming.passed`;
   field added to the Report initializer. Schema string untouched.
5. Heredoc hygiene: no backticks, no dollar signs in inserted Rust.

## Gates

The standing five, in order; plus report inspection (`python3` over
report.json: streaming present, no cache/on_demand, overall_pass true).
