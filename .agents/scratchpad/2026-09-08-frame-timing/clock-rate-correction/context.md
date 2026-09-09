# Audio Clock-Rate Correction

Updated: 2026-09-09

Status note: the estimator requirements below are UNAPPROVED. The user asks for
a deterministic approach instead; deterministic-research.md records the current
direction. Do not implement the four-minute plan from this historical context.

## Request and Evidence

The user wants to proceed to correction rather than require another clean trace.
The approximately 1 ms sensitivity is meaningful. The two CrossOver captures
showed repeatable game/QPC slope differences and mixed-output cursor evidence
consistent with a small relative rate mismatch. Background compilation is a
confound for stalls, not a justification to hardcode an observed ppm value.
Per-song audible onset remains unmeasured; this task addresses RATE only.

## Instructions and Approval

AGENTS.md and README.md govern this work; CODEASSIST.md is absent. Direct task,
not generated from approved code-task-generator artifacts. Local approach must
be approved before test/implementation code. Mode: auto after that approval.
All existing uncommitted work and operator mod-config.json changes are preserved.
No commits, push, deployment or live debugger. Logs stay under this task's logs/.

## Verified Timing Contract

For raw game tick T, actor anchor A, SOUND S, actual option-vcall result J,
absolute native press timestamp P and note time N:

    raw_now = T - A - S + J
    native_press_age = T - P   (low-32-bit subtraction)
    event_count = raw_now - native_press_age

Both native press-age sites use the UserFootPanel virtual getter at +0x28.
AutoFootPanel is different: its synthetic age already uses incoming music count.
Never apply physical-input scaling again to that synthetic age.

With per-song k = output_rate / raw_game_rate and F(e)=round(k*e):

    D(e) = F(e) - e
    corrected_now = raw_now + D(T-A)
    corrected_age = native_age + D(T-A) - D(T-A-native_age)

This preserves the already-computed J, raw anchor, additive offsets and native
windows. Difference-of-mapped-endpoints matters at integer-ms rounding boundaries.
Identity is exact. Do not multiply absolute uptime or change the nominal song-rate
publication. Existing non-100% playhead/press-age scaling has a separate static
inconsistency; fixing it is outside this first correction scope.

## Source Acquisition

- The game clock source is the no-argument import called immediately before the
  input poll stores RAX into its cached +0x1268 field. Observe/call the original
  source on the same game thread with a QPC bracket, rather than fitting the age
  of a cached tick read after rendering.
- XACT mixed-output cursor is a u64 accumulated PLAY byte count. It is a possible
  rate reference after continuity/format/plausibility checks, not song onset.
- Learn through a shared cursor observer independent of diagnostic CSV lifetime.
  Retain factory-return/pre-Initialize installation and one hook per target.

## Acceptance Criteria

1. Default-off boot setting audio_clock_sync.enabled; no hardcoded ppm knob.
2. Bounded, robust two-clock estimator against QPC; minimum four minutes of
   common learning, ten-minute history, block-based uncertainty and stability
   checks. Qualification is for the upcoming verified song duration (<=300s),
   with a 0.5 ms rate-estimation budget, freshness and sane-factor gates.
3. Normal solo, 100% non-course playback only initially. Active loops/section
   starts, calibration and unknown identities/settings decline correction.
4. Freeze a qualified factor before the first corrected frame; no mid-song
   retuning. Missing estimates mean stock for the whole attempt. Estimator loss
   retains a frozen factor in holdover and invalidates future qualification.
5. Pair onUpdate playhead mapping with native UserFootPanel press-age mapping.
   Preserve synthetic autoplay ages, invalid/consumed timestamp behavior, native
   windows, offsets, and nominal song-rate semantics.
6. Zero-time restart can rebase around its new anchor with the same frozen
   factor. Nonzero seeks must be declined before any mutation while correction
   is active; broader seek/rate/versus integration is a later change.
7. Experimental nonidentity commitment taints the affected side before exposure:
   suppress stage saves for the rest of the credit and sanitize logout/profile
   writeback using the existing mechanism. Clear only at positively matched
   card-in. Learning and stock-fallback songs do not create that taint.
8. Existing clock_patch owns the sole playhead redirect. Optional correction
   callback preserves live registers/flags/stack ABI; default-off stub remains
   unchanged. Native press-age hook is capability-gated and passthrough outside
   its matching original-judge context. No independently corrected half-pair.
9. Diagnostics remain optional. Expose estimate/qualification/applied/holdover
   status through bounded logs and optional trace records; no hot-path logging,
   fitting, allocation or waits.
10. TDD pure mapping/estimator/lifecycle/score tests, actual signature and machine
    code validation, normal/Win7 builds. Runtime success remains to be tested.

## Build Commands

Repository root: cargo check --target x86_64-pc-windows-msvc; cargo fmt;
./build.sh; ./build_win7.sh; ./scripts/validate_signatures.sh with the local
four-build corpus, plus shape_diff.py for consumed bytes. New host harnesses
must mount actual pure modules; plain crate cargo test cannot build retour on
this ARM host. Run existing audio/frame/song-rate/score regression harnesses.
