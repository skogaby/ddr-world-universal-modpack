# Progress

Updated: 2026-09-08
Status: Complete (uncommitted - maintainer commits manually); first CrossOver capture analyzed; native-Win7 validation pending
NEXT ACTION: Read ../expanded-audio-diagnostics/progress.md: v2 diagnostics are built and ready for another off/on capture; no automatic anchor correction.

Resume: Read context.md and plan.md in this directory; user approved the full approach.

## Checklist

- [x] Inspect current source, user reproduction, and worktree state.
- [x] Audit frame callback and self-requeue consumers.
- [x] Verify existing once-per-frame dispatcher in Ghidra on 20260825.
- [x] Identify audio observation seams and their limitations.
- [x] Write requirements, approach, and test scenarios.
- [x] Obtain approach approval (Proceed, including diagnostics; no clock correction).
- [x] Run baseline checks and write failing scheduling tests (missing FramePump).
- [x] Implement once-per-frame queue dispatch and consumer migrations (6 scheduler and 9 consumer tests pass).
- [x] Write failing diagnostic model tests and implement optional capture (15 tests pass; integrated).
- [x] Research actual cue/wave start and document deterministic-sync feasibility (see docs/audio_sync_diagnostics.md).
- [x] Run focused tests, checks, formatting, normal/Win7 builds, signature sweep.
- [x] Document runtime validation and final uncommitted status.

## Decisions

- Preserve poll-before-queue and same-frame handling of poll-enqueued work;
  only batch-requeued continuations must wait for the next frame.
- Reuse the existing dispatcher hook; retain per-wrapper/per-list emission.
- Audio stage is observational, default off; no judgement/clock modifications.
- Existing operator-config change and unrelated battle research remain untouched.

## Validation

- Baseline and integrated Windows-target cargo check pass.
- logs/frame-pump-red.log: expected missing-type failures; frame-pump-green.log:
  6 tests pass, including nested poll/job/original dispatch and panic isolation.
- audio-progress.md and consumer-progress.md record the independent TDD tracks
  and their host tests; audio signature sweep and consumed-byte review pass.
- Ghidra confirms dispatcher RVA 0x2AF60 has one caller in the 20260825 main
  frame function RVA 0x3000 after update/draw preparation before consumer kick.
- No live cabinet operations performed. Runtime timing effectiveness pending.
- Final host runs: frame pump 6, consumer guards 9, audio model 15, mod menu 37,
  custom options 40 plus display-string lint, training 26, overlay 21,
  calibration 14, song-rate 281 plus synthetic pipeline report: all passed.
- Normal and Win7 release builds pass; Win7 imports checked for QPC and legacy
  RNG, with no ProcessPrng/bcryptprimitives/WaitOnAddress/precise-time dependency.
- Final signature sweep ALL GREEN (22 misses covered by version alternates).
  Shapes: dispatcher/start/prepare/ready/stop/broadcast identical through 0x100;
  quartet/raw-store divergences lie beyond consumed bytes. v1 checked separately.
- Additional Ghidra caller checks: 20250805 dispatcher 0x2AF50 -> caller 0x3070;
  20260224 dispatcher 0x2AAB0 -> caller 0x3050; both unconditional before kick.
- git diff --check and final cargo fmt --check pass. Original operator-config
  edit and unrelated untracked battle research remain untouched.
- Existing song-rate harness emits shell command warnings from backticks in an
  unquoted heredoc, plus pre-existing compile warnings. Tests and synthetic
  report pass; no unrelated harness changes made.
- Public instructions: docs/frame_scheduling.md and docs/audio_sync_diagnostics.md.
  No commits, staging, deployment, live debugging, or timing corrections.
- User supplied first runtime log/CSV: 36.7-minute CrossOver run at 1080p/120FPS
  with internal 4x MSAA, not the original Win7 cabinet. See capture-2026-09-08.md.
  All channels/fields valid, zero record drops. Five part plays: frame cadence
  118.67-119.99Hz, observed queue stable at one continuation. Start-to-anchor
  entry 0.055-0.203ms. Small game/QPC rate delta 11.1-12.0ppm, plus update gaps
  up to 95.8ms. Actual audible onset/drift remains unmeasured; no correction made.

## Deviations

- The countdown's existing 250 ms prepare lead, deadline, and synchronous
  prepared-to-anchor block remain unchanged. A newly queued readiness check
  now runs next frame (bounded frame-cadence sampling rather than same-frame
  busy polling); this can add readiness-observation latency, not a fixed clock
  correction. Include delayed reset testing in cabinet validation.
- Missing layer_dispatcher now explicitly disables frame callbacks/queued work
  with a WARN; no legacy per-wrapper fallback is used. A missing layer-table
  derivation only disables the dependent emitters, not the scheduler.
- No test or implementation code was written before approach approval.
