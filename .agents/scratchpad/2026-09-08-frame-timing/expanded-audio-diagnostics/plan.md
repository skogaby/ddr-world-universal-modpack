# Expanded Diagnostic Plan

Updated: 2026-09-09
Status: Approved 2026-09-09

## Implementation

1. Extend the existing recorder to an explicit audio-sync/v2 CSV, separate from
   the original file, retaining its first 32 columns. Append fixed scalar trace
   metadata. Preserve bounded nonblocking recording and add per-scope summaries
   with slow-event budgets, so each frame need not generate a full event stream.
   Keep a fixed ring (at most 512 bytes per event) and cap v2 output at 32 MiB.
2. Instrument the existing frame and judge dispatch owners, with QPC entry stamps
   before their work. Separate poll/jobs, original layer rendering and topmost
   overlay. Move the diagnostic judge snapshot to an explicit owner tail so it
   cannot be mistaken for original or post-mod work. Maintain nested-call IDs,
   panic containment and original-forwarding invariants. Preserve the scheduler's
   already-correct ordering and cadence; no further scheduling policy changes.
3. Add per-hit records through the shared judge_submit hook, with pre-submit
   snapshots and correlation to the actual outer judge argument. Include grades
   and death/finished flags so Miss/failure-tail events are distinguishable.
   Do not pretend integer-ms engine errors acquire fractional precision from QPC.
4. Add a binary-attested XACT observation module. Install only in the engine
   factory-return window before Initialize. Observe event scheduling, streaming
   voice submission, stop/destruction, and existing mixed-output cursor updates.
   Correlate using bounded cue-generation state plus validated live ownership;
   poison attribution on critical update loss. Engine stays pinned while hooks
   exist. Unsupported/missed channels degrade independently to the current trace.
5. Add an offline v1/v2 report with clear sample/cadence/clock limitations, useful
   attempt summaries, slow-scope attribution and cue/request/submission timelines.
   Update capture documentation and leave clocks/offsets/judgement behavior alone.

## Tests Before Code

- Fake QPC proves sub-ms spans and exact phase boundaries; diagnostic off does
  not invoke its clock/observer. Reentrant original forwarding and job ordering
  remain unchanged; diagnostic failures cannot repeat or skip originals.
- Judge/submit nesting restores context; actor/thread mismatch is unavailable.
  Null/invalid note or scratch, OK/no-error, Miss, and pre-dead versus
  death-causing hits remain correctly classified and never change game inputs.
- Summary counters/max timestamps survive decimation; slow-event budgets have
  explicit suppression counts and do not consume reserved lifecycle capacity.
- Cue generation/handle reuse, asynchronous start, stop/dtor/unregister,
  context loss, unknown ownership and map contention never misattribute starts.
- Cursor failure, ring wrap, buffer/format changes and resets break continuity;
  mixed cursor measurements never become fabricated per-song presentation time.
- v1 reader fixture remains supported; v2 schema/formatter columns agree;
  missing fields/channels stay unavailable. Output cap and writer failure remain
  fail-open. Supported engine patterns match uniquely and all consumed layout
  bytes are checked, not merely the prologues.

## Validation and Delivery

Run host tests, Windows check, full-crate formatting, normal/Win7 release builds,
game signature sweep and shape review, separate engine attestation test, and
Win7 import inspection. No commit or deployment. Next runtime capture must
compare diagnostics off/on and include matching full/partial song attempts.
Actual hardware presentation remains an explicitly unmeasured endpoint.
