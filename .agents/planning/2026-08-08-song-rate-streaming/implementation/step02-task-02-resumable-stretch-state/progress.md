# Progress — Step 2 task-02: The Resumable StretchState with Byte-Equality Proof

Updated: 2026-08-09
Status: Complete (uncommitted — maintainer commits personally)

## Checklist

- [x] T1 small-matrix equality test + stub (failed against the zero-emitting
      stub as planned — `logs/check-cycle1.log`, harness run)
- [x] Identity + FirstCopy + Main + Terminal ported (full matrix passed on the
      first complete port)
- [x] Full matrix (T1) incl. failure parity + boundary/error parity (T2)
- [x] Chunking independence (T3) incl. small/misaligned buffer handling
- [x] Checkpoint/restore (T4) incl. cyclic-previous validation legs
- [x] Bounded access instrumentation (T5) with per-event nominal tracking
- [x] Gate 1: validator green — 123/123 (118 + 5 new), validation passed
      (`logs/validator.log`)
- [x] Gate 2: se-bank ALL CHECKS PASSED
- [x] Gate 3: windows check 0 warnings
- [x] Gate 4: fmt clean
- [x] Gate 5: build.sh release DLL OK
- [x] NO commit (maintainer commits personally)

## TDD cycles

1. Harness + T1 (full rate×loop×channel matrix) + API stub (real `new`
   validation, zero-emitting `produce`) → FAILED as required.
   - Discovery during the failing run: the REFERENCE itself deterministically
     returns `NoCandidate` for 25%/50% cells with no-loop or interior-loop
     contexts (near the output end the nominal exceeds the last window start
     by more than the search radius; verified content- and size-independent,
     8 kHz and 48 kHz — probe log kept mentally, see Deviations). T1 was made
     a BEHAVIOR-parity matrix: equal bytes+counters where the reference
     succeeds, identical `StretchError` where it fails.
2. Real machine (events FirstCopy/Main/Terminal + Identity; retained
   provisional tail; drain layer) → full matrix passed first run (bytes,
   clipped_samples, cyclic_windows, failure parity).
3. T2–T5 added; two fixes: `Debug` derives for `expect_err` ergonomics
   (SourceWindow/StretchState — additive only), and the loop-start checkpoint
   capture moved one boundary deeper (resume ≥ loop start + 2·hop) so the
   captured snapshot provably carries a CYCLIC previous window (making the
   cyclic-without-loop-context rejection leg real).

## Acceptance criteria evidence

- AC1 byte equality: `streaming_stretch_matches_reference_across_matrix` —
  stereo × {25,50,75,100,125,175} × {none, interior, boundary} + channels
  {1,6} × {50,125} × {none, interior}; bytes + both counters equal; identical
  errors on the reference's own failure cells.
- AC2 chunking independence: constant {1, H−1, H, H+1, W+17, 997, whole} +
  LCG-random chunkings byte-equal; `out.len() < channels` →
  `OutputTooShort{0,1}` (documented minimum: ONE frame); misaligned buffers
  use only the whole-frame prefix; `done` fires exactly once.
- AC3 checkpoint/restore: zero checkpoint reproduces the whole run; the
  loop-region checkpoint reproduces the suffix byte-identically with matching
  end counters; `checkpoint()` is None once the terminal region begins;
  invalid restores (resume past terminal, cyclic previous without loop
  context) → `InvalidCheckpoint`.
- AC4 bounded access: per-event instrumentation at 75%/175% — first copy ⊆
  [0, W); main events ⊆ [nominal − R − H − 2, nominal + R + W); terminal ⊆
  [S − W, S); event count exact.
- AC5 reference untouched: `stretch_interleaved_with` body byte-for-byte
  unchanged; the streaming machine reuses only the pure position helpers
  (q32/nominal/window_wraps/candidate_precedes/validate_loop_context) and
  mirrors the source-reading ones; existing suites + validator synthetic/
  corpus sections pass unmodified.
- AC6: five gates green (validator cargo-test phase now ~5 s — the matrix
  runs in debug; within tolerance).

## Deviations

- **Reference NoCandidate envelope (matrix semantics)**: the task's matrix
  implied all cells succeed; empirically the reference fails 25%/50% without
  a full-entry loop. Byte equality is only defined where the oracle produces
  bytes, so those cells assert ERROR parity instead (streaming reproduces the
  identical failure through the identical event sequence). Production shape
  is unaffected: stock banks carry whole-entry loops (the boundary-shaped
  cells), which succeed at all six rates. Flagged for Step 3/4 awareness:
  the generator must treat `NoCandidate` as a preflight/failure leg, not an
  impossibility.
- **Allocation-bound failure cells excluded from parity**: the reference's
  `AllocationFailed`/`ArithmeticOverflow` legs for absurd outputs (e.g.
  usize::MAX frames) arise from its whole-buffer allocation, which the
  streaming machine exists to avoid; parity is scoped to the task's listed
  classes (channels, too-short inputs, loop contexts) plus mid-run
  NoCandidate.
- **Additive changes near the reference**: `StretchError::InvalidCheckpoint`
  variant (+ Display arm) and `Debug` derives on `SourceWindow`/
  `StretchState`. No observable reference behavior change (suite passes
  unmodified).
- **Bounded-access lower bound corrected**: the joint-SAD reference window
  (anchored at previous + hop) extends the design's [nominal − R, …) figure
  by up to one source-hop at fast rates; the test asserts
  [nominal − R − H − 2, nominal + R + W). Cache sizing guidance unaffected.
- Checkpoint carries 5 words (phase omitted — recomputed as
  phase_step·(resume/hop); the provisional tail is rebuilt from the previous
  window's direct-copy region, which is a pure source copy — the reason
  checkpoints stop at the terminal region, whose blend input has mixed
  provenance).

Status: Complete (uncommitted — maintainer commits personally)
