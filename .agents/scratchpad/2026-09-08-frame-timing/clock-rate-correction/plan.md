# Clock-Rate Correction Plan

Updated: 2026-09-09
Status: Unapproved - estimator approach questioned; deterministic audio-sample approach under investigation

User feedback: the four-minute learning heuristic feels awkward/unnecessary;
investigate direct audio-sample authority before asking to approve a correction.
The estimator plan below is retained as an unapproved alternative, not authority
to implement. No correction source/test code has been written.

## Implementation

1. Add default-off audio_clock_sync.enabled and a shared XACT cursor owner.
   Diagnostics demand the existing extended probes; correction-only demand
   installs only the passive cursor capability. Learning does not depend on
   diagnostic ACTIVE, file capacity, logging, or diagnostic playback hooks.
2. Resolve the original raw game-time reader and UserFootPanel age getter from
   content, cross-check their global against frame_tick_global, and attest
   current/older code shapes. Observe fresh raw time on the game thread with a
   QPC bracket. Validate raw audio continuity at each existing cursor call;
   queue bounded retained observations for worker fitting.
3. Implement a pure bounded estimator: five-second buckets, up to ten minutes,
   >=240s and >=80% common coverage; robust centered fits and 30s-block uncertainty,
   split-window stability, query-width/outlier/freshness checks. Reject implausible
   factors rather than clamp. Qualify against a known <=300s song duration and
   0.5ms rate-error budget; require evidence distinguishable from identity.
4. Freeze the decision for a verified normal solo 100% attempt before its first
   corrected playhead. Match a fresh audio-bank publication and source identity;
   skip unknown length, course, versus, loop/pre-shift and calibration cases.
5. Extend the owned clock stub with an optional ABI-preserving callback. Keep
   nominal song-rate Q31 independent. Map raw elapsed time plus the existing raw
   count, preserving additive offsets. Native press-age uses the same immutable
   frame/anchor/factor context; synthetic panels and unrelated calls are not
   transformed. Use signed fixed-point arithmetic and mapped endpoint differences.
6. Rebase zero restarts on accepted new anchors, retaining the factor. Refuse
   nonzero seeks before their audio/state writes while an experimental correction
   is active. Never change a running factor on learning failure or toggle-off.
7. Add side-specific experimental score containment, published before correction.
   Stage suppression lasts through the credit, logout retains profile settings
   via the existing sanitizer, and only matched card-in clears the new taint.
8. Integrate optional trace/status reporting and operator instructions; run all
   build, signature, input-domain and regression gates. No commit or deployment.

## Tests Before Implementation

- Identity exactly preserves every tested signed raw count/age, including lead-in
  and rounding boundaries. Opposite ppm signs produce the correct direction.
- Corrected_now - corrected_age equals mapped press time with the same offsets;
  independently rounded age is rejected by boundary fixtures. Native low-word
  wrap, zero/consumed timestamps, stale/mismatched contexts and synthetic input
  do not become phantom hits. No nominal-rate semantics change at non-100%.
- Callback stub preserves GPRs, XMM0..5, flags, stack alignment and original
  overwritten instructions; target/prefix failures do not enable a half-pair.
- Estimator recovers known small slopes under integer game-time and quantized
  audio, correlated noise, bursty sampling, variable query widths and long uptime.
  Short/noisy/sparse/stepped/stale evidence remains identity. Block uncertainty
  and song-duration qualification tested separately from slope point estimates.
- Audio wrap/format/reset/freeze/gap and observer-loss cases invalidate future
  qualification. Loss of a decimated observation is not a hidden raw cursor wrap.
- Qualified mid-song never applies; next-song commitment freezes the factor.
  Zero reset rebases; nonzero seek refuses before mutations. Unknown/unsupported
  modes, length/identity mismatch, or missing score readiness remain stock.
- Experimental taint precedes exposure, survives per-song and broad resets,
  rejects unknown-side saves, and clears only for a positively matched card-in.
- Diagnostics off still learns; CSV cap/failure cannot terminate observation or
  jump the applied clock. Single-detour ownership under either/both consumers.

## Deliberate Limits

This prototype does not solve audible-onset jitter, dropped inputs, arbitrary
clock resets, nonzero training seeks, non-100% timing, or native-Win7 behavior
without testing. No score is submitted from an experimentally corrected credit.
