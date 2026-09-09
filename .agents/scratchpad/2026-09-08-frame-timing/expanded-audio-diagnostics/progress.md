# Progress

Updated: 2026-09-09
Status: Complete (uncommitted - maintainer commits manually); first v2 CrossOver capture analyzed; clean/Win7 validation pending
NEXT ACTION: Read capture-v2-2026-09-09.md. Repeat without background compilation and with calibration OFF before attributing stalls or correcting clock rates.
Resume: Read context.md, plan.md and the sibling first-capture report.

- [x] Review first capture and preserve 1 ms sensitivity as meaningful.
- [x] Inspect existing recorder, frame/submit owners and worktree state.
- [x] Research authoritative cue/wave ownership and a deeper streaming submission boundary.
- [x] Identify safe factory-return installation and passive output cursor observation.
- [x] Obtain local approach approval (Proceed, full scope).
- [x] Extend bounded recorder and add frame/judge/hit spans.
- [x] Implement binary-attested engine observations and lifecycle-safe correlation.
- [x] Add offline report/tests and documentation.
- [x] Run host/build/signature/Win7 validation.

## Completed Validation

- Recorder/span harness: 39 passed (includes 8 actual frame-pump tests).
- Frame-pump harness independently: 8 passed.
- XACT model/sites/actual-binary harness: 21 passed; includes all four game
  factories/managers, engine ASLR/IAT changes, and consumed-byte mutation rejection.
- Python report: 12 passed; real v1 capture reproduces its five frame rates,
  start-anchor intervals and 11-12 ppm clock fits.
- Windows-target check, cargo fmt and fmt --check, normal release and Win7
  release passed. Win7 imports use QPC and legacy RNG; no ProcessPrng,
  bcryptprimitives, WaitOnAddress or precise-system-time dependency.
- Game signature sweep ALL GREEN (22 covered version-alternate gaps).
  judge_submit diverges at +0x70, after the consumed dead-byte instruction at
  +41; other prior quartet/raw-store divergences remain outside consumed bytes.
- Calibration 14, overlay 21, training 26 and song-rate 281 regression tests
  passed, plus song-rate synthetic validation. Its old heredoc warnings persist.
- Git diff whitespace check passed. No staging, commit, deployment, live debug,
  operator config change, or gameplay-clock/scoring correction performed.

## Integration Decisions

- Factory bootstrap runs immediately after gamemdx discovery, BEFORE the full
  signature scan, using a locally attested factory/manager shape and GameModule.
  LayeredFS still installs first. An already-loaded engine is never late-patched.
- Cue/bank identity binds atomically at prepare return using audio_manager_global;
  start revalidates it. This prevents unrelated cue cleanup in the READY dwell
  from discarding the ordinary prepared song's unbound token. Unknown identities
  still degrade to unmatched; no mutable sound/track walk occurs on game-side bind.
- Schedule/voice QPC pairs now enclose original execution, not the preceding
  ownership probe. Its own pre-call latency is reported separately in counter0
  (valid iff counter3 bit0). Extra RED/GREEN tests cover this and prepare binding.
- V2 filename preserves v1 evidence; first 32 columns stable, 15 appended fields,
  32 MiB output cap. Summaries are process-cumulative, not per-attempt distributions.
- Finished flag remains unavailable rather than read from an unattested layout.
- See docs/audio_sync_diagnostics_v2.md and track records for schemas/limitations.

## Remaining Runtime Questions

First v2 capture confirms all engine probes installed and all three part starts
matched cue identities, including an asynchronous engine-thread start. See
capture-v2-2026-09-09.md: 0.20-0.35 ms request-to-voice submission, approximately
18-24 ppm game/mixed-cursor relative rate estimates (quantization caveat), and
third-play update gaps up to 135 ms outside measured slices. First play calibrated
SOUND 450->449 and hid feedback; user reports possible concurrent compilation by
another agent. These are confounds, not evidence of a modpack regression.

The remaining questions below now concern clean/native-Win7 validation and
presentation semantics rather than whether the first v2 capture installed at all.

Does the early factory window fire on the actual setup? Do live wave owners
match the prepared cue tokens? What do original execution and pre-probe costs
measure on Win7? Does mixed-output progression differ from game/QPC, and can it
be mapped to the song's actual presented samples? No runtime claims yet.
