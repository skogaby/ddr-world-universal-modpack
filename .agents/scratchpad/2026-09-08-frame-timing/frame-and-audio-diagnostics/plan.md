# Implementation Plan

Updated: 2026-09-08
Status: Approved 2026-09-08

## 1. Once-Per-Frame Scheduling

Use the existing sole layer-dispatcher detour, before its original layer walk,
to poll input/frame callbacks and dispatch one queued batch. Remove those jobs
from the widget-wrapper hook; retain wrapper-specific anchor emission and
post-original dirty rearming, and retain dispatcher-post SMX topmost emission.
Install the scheduling boundary independently of optional shader/layer-table
availability. No new timer thread, Present hook, or fixed 60 Hz throttle.

Keep the public run_on_render_thread API. Extract its queue into a pure,
host-testable owner with a reentrancy guard. Preserve input-first semantics:
poll once, then snapshot the queue, then execute that one FIFO batch. Jobs
enqueued during polling may execute that frame; jobs enqueued by the batch
wait until next frame. Never execute inline and never hold the queue lock
across callbacks. Keep original engine forwarding and panic containment.

If the dispatcher cannot resolve/install, log an explicit scheduling failure;
do not silently claim fixed scheduling or run both scheduling paths. Determine
the narrow fail-open behavior during implementation from existing consumers.

## 2. Consumer Migration

Convert callback-count timeouts to explicit elapsed durations (WebUI loading:
12 seconds, SMX window discovery: 2 seconds, atlas warning: 10 seconds).
Coalesce chrome-loader and series-filter pumps and queued PUS raise requests
where needed. Keep game/reset subscribers synchronous and keep existing
time-based audio leads. Audit prepared-after-Play continuation so removing
same-frame requeues cannot add an unnecessary reset-frame delay.

Keep this scoped: no new general task-priority framework, unrelated rendering
optimization, memory-residency change, or input event-retention redesign.

## 3. Optional Audio/Game Diagnostic

Add diagnostics.audio_sync, default false and boot-only. Use a small preallocated
bounded event buffer with explicit loss accounting and a fallible deferred
writer. Cap output; separate attempts even when song names/handles are reused.
Use QPC plus captured frequency, not log wall time or wrapped packed counters.

Observe existing prepare/readiness/stop and actor anchor boundaries through
diagnostic-only hooks; reuse existing judge and scene subscriptions plus
bank-hook callouts. Verify/derive the actual prepared-cue start boundary if
available; unavailable channels are reported explicitly. Record effective
offsets/rate, actor anchor/count, and frame/job cadence. Capture dense startup
samples and bounded ongoing progression/gap measurements. Do not perform
extra state-consuming input or audio calls merely to sample them.

Separate labels: prepare_request, ready_observed, start_request,
anchor_delivered, gameplay_sample. None claims audible_start. No audio/clock
behavior changes in this stage.

## 4. Synchronization Research

Trace prepared-cue start to actual XACT wave/mixer start and assess whether a
source timestamp can be mapped to the game's anchor safely before judging.
Document what the diagnostic proves and what requires output capture or
additional engine RE. No callback-arrival-based automatic anchor correction,
mid-song 0x1044, additional DoWork pumping, or player-error-based calibration.

## Test Scenarios

- Zero, one, and many wrapper invocations do not affect dispatch count.
- Frames closer than one millisecond are not suppressed by time bucketing.
- Reentrant polling/jobs cannot recurse into another batch; original engine
  forwarding remains outside diagnostic/user-callback failure paths.
- Initial queue A/B, poll enqueues P, A enqueues C: frame one A/B/P; frame two C.
- Enqueue/register/unregister from callbacks cannot deadlock. One panicking
  callback does not discard unrelated callbacks; next frame still works.
- Timeouts at varying invocation rates expire at the same elapsed duration;
  readiness at the boundary wins; cancelled generations cannot requeue.
- Duplicate pump requests coalesce; menu-close/reopen and queued raises cannot
  resurrect stale work or overtake a newly opened menu.
- Event buffer empty/full/wrap/recovery, concurrent producers, bounded drops,
  and reuse after drain; timestamp conversion and output-size caps.
- Same song and reused cue handle across attempts remain separate; missing
  readiness/anchor/start are unavailable, never fabricated zero measurements.
- Pre-anchor raw counts cannot be interpreted as song time. Offset changes,
  rate changes, resets, and scene transitions are explicit discontinuities.
- Diagnostics off has no worker/hooks/record allocations; producer failure
  leaves original arguments, return values, and call counts unchanged.

## Verification and Handoff

Run baseline and new pure tests, cargo check, whole-crate format, normal release
and Win7 release, signature sweep, relevant shape review. Inspect diffs and
leave changes uncommitted. Supply diagnostic configuration/output instructions
and cabinet checks for all-scene menu/input, preview loads, training/reset/movie
timing, and repeated partial-song exits. Runtime effectiveness remains pending
cabinet validation.
