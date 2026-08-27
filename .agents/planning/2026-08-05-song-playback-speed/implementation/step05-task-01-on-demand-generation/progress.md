# Progress: Step 5 Task 1 — Wire On-Demand Generation Into the Live Transaction

## Checklist

- [x] Baseline: host validator green before any change (147)
- [x] Slice A: rate-domain widening (target_for_percent 25..=175 step 5) + boundary
      Q31 clock vectors — red (2 failures on the 75|100|125 predicate) then green
- [x] Slice B: rate-generic arm model (lifecycle) — DiagnosticSpec/validate_diagnostic
      retired; `is_supported_rate_percent` + domain consts; `EligibilityInputs.desired`;
      IdentityReason {NoRateSource, IdentityRate, UnsupportedRate}
- [x] Slice C: on-demand conversion pipeline — `resolve_generated_bank` replaces the
      pre-generated import with `coordinator.request()`; `dance_bank_song_code` +
      `effective_source_path` (pure, host-testable); `RefuseReason::Generation(kind)`
- [x] Slice D: `DiagnosticCoordinator::new_with_limit`; normalize coverage pre-existing
- [x] Slice E: windows glue — runtime desired-percent atomics + `set_desired_percent`,
      phase-first fast gates, first-arm drain/timeline latch (`rate_recording_active`),
      config `cache_limit_gib` + retired `diagnostic` parse-but-ignore, lib.rs 5b rewrite
- [x] Review fix: SONG BINDING (see Deviations — a real defect the song-agnostic model
      introduced, caught in the consistency review)
- [x] Validate: all five gates green (153 host tests); zero cargo warnings
- [x] Canonical planning-dir progress.md updated
- [x] NO commit / NO deployment (maintainer owns both)

## Record

- Baseline validator: 147 passed.
- Slice A red: 2 new tests failed on the old `75 | 100 | 125` predicate (plus one
  wrong expectation of mine — half-away rounding at i32::MAX/4 — fixed in the test).
- Slice A green: 149 passed after widening `rate::target_for_percent` to multiples of
  5 in 25..=175. Boundary vectors prove the 175% factor exceeds i32::MAX and scales
  exactly through the 64-bit imul slot; DurationOutOfRange proven reachable at 25%
  near the 28-bit ceiling (the documented early-failure leg).
- Slices B+C red: harness compile failures on removed `DiagnosticSpec` APIs, then two
  logic failures — 80 IS a valid multiple of 5 (my bad test case; replaced with 77),
  and `completed_builds` counts failed jobs too (admission test now asserts an empty
  store inventory instead).
- Slices B+C green: 152 passed, including on-demand cold builds at 75 AND 125 from one
  source (distinct cache keys, generated banks reparse with exact durations), warm
  reuse with zero re-transforms, cross-song worker reuse, admission-failure fallback
  (`Generation(Admission)`), corrupt/mislabeled-source refusal, and every retained
  Step-4 invariant test (structural refusal, convert ordering, quarantine, reload,
  end-to-end commit/unload/late-fail).
- Song-binding fix + regression test: 153 passed.
- Final gates: validator 153 green; se_bank_synth green; `cargo check` windows target
  0 errors 0 warnings; whole-crate `cargo fmt`; `./build.sh` release DLL produced.

## Deviations

- **Song binding added (design-strengthening, not design-contradicting).** The
  approved design predates the merged-delivery song-agnostic arm model; Step 4's
  song specificity came free from the configured diagnostic code. Making the code
  path-derived opened two holes: the generation-keyed `OPEN_REDIRECT` cache could
  serve the generated bank to a DIFFERENT song's open, and the seam could expose a
  dance bank whose RAM copy was never redirected — recreating the rate-against-
  stock-audio class the two-stage invariant exists to prevent. Fix: the generation
  binds to the song digest at the FIRST successful open redirect
  (`LifecycleState::bind_song`, cleared on every arm); the seam and the runtime
  cached-path branch refuse any other song (`OpenNotRedirected` class). This
  PRESERVES the invariant the task made load-bearing; recorded as a strengthening
  under the same structural-refusal umbrella.
- `completed_builds` diagnostics counter counts finished jobs (success AND failure);
  test expectations were adjusted, production behavior untouched.
- The quarantine tombstone check now necessarily runs AFTER worker resolution (the
  identity's output digest comes from the built/warm manifest) — anticipated in the
  task context; the artifact may sit in cache but never reaches XACT.
- The seam re-derivation MAY cold-build after an eviction race between open and
  create; that is the design's original sanctioned waiting site (req 24), no guard
  added (normal case is a warm hit by construction — asserted in tests).

## Status

All host work complete and gated. NO commit was made (maintainer owns git history)
and NO deployment occurred (prohibited for this task; cabinet validation concentrates
in step05 task 04). Next task: `task-02-add-player-facing-song-speed-option` —
its arm source (`runtime::set_desired_percent`) is already in place.
