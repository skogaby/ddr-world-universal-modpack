# Audio/Game Clock Diagnostics

**Current builds write V2.** See [V2 capture/report instructions](audio_sync_diagnostics_v2.md).
The remainder of this page is the historical V1 schema and research reference,
retained to interpret the first capture. V2 uses a separate filename and keeps
V1 files readable through scripts/analyze_audio_sync.py.

Status: integrated; host/build/static validation passed; live validation pending.
Static research and host tests: 2026-09-08.

## Enable and Capture

Set `diagnostics.audio_sync` to `true` for the next launch. It defaults to false
independently of `diagnostics.profiling`. The false path creates no diagnostic
buffer, worker, hooks, or subscriptions. No operator configuration was changed
while implementing this feature.

An enabled launch writes `audio-sync-diagnostics.csv` in the process working
directory. The next enabled launch replaces that file. Copy/retain a useful
capture before another enabled launch. The file is capped at **8 MiB**, including
metadata. Reaching the cap or encountering a file/thread failure stops recording
and emits one summary/WARN; installed observers remain passthrough. Abrupt process
termination can lose the final buffered batch. There is no shutdown-time IO in
the DLL entry point.

Reproduce the reported case with a stock 100% song: play its first third,
quick-fail **with results**, then choose the same song again. Keep the CSV and
the normal game log together. Compare separate `attempt` / `scene_epoch` values,
not song names or recycled cue handles. Test clean start, repeated fresh-song
loads, in-place restart, training scrub/loop, both sides, and non-100% separately.
This feature does not fix timing and should be switched off outside diagnosis.

## Collection Contract

- The ring holds 512 fixed-size, Copy/POD events. The producer attempts its mutex
  exactly once. A full or contended buffer drops the event and increments a
  separate atomic counter. Producers never format, allocate, perform file IO,
  wait for a mutex, spin, pump audio, or walk the actor tree.
- The writer drains at most 256 records per 250 ms iteration, releases the lock
  before formatting/IO, uses a 32 KiB buffered file, and reports bytes/full drops/
  contention drops every 10 seconds. `# loss` CSV lines carry the same totals.
  Counts describe dropped candidate records, not intentionally decimated samples.
- Gameplay samples come from the existing shared judge dispatcher at post/Late.
  Each side and the frame channel record at most every 5 ms for their first
  3 seconds, then every 250 ms, plus observations following gaps of at least
  50 ms. `observations` counts all observations in that sampling segment;
  `max_gap_qpc` is its maximum inter-observation gap, including decimated calls.
- Every accepted scene event advances `scene_epoch`. A fresh confirm (26), a
  fresh stage loader (27 not preceded by 26), direct gameplay entry (28 not
  preceded by 26/27), or attract-demo entry (16) advances `attempt`. The ordinary
  26 -> 27 -> 28 chain is one attempt. Preview events retain their scene context;
  `attempt` is a play-attempt key, not an audio-engine handle generation.
- Scene identity is updated even when the ring is full. If a scene callback
  cannot acquire the lock, subsequent records have `context_valid=false` until
  a fresh scene-26 boundary; do not correlate those records across attempts.
- A matching observed anchor readback is required before progression is emitted.
  Scene changes clear that evidence. Anchor deliveries, actor/offset/rate/validity
  changes and backwards counts break progression into new per-side segments.
  In-place restarts remain in the same play attempt but have explicit anchor
  events and new segments. Missing events remain missing; they are not inferred
  as zero-latency milestones. Always inspect the loss counters.

All times use raw 64-bit `QueryPerformanceCounter` plus the captured frequency.
`qpc=-1` indicates a QPC call failure. No log wall clock, masked 31-bit engine
timestamp, `GetTickCount64`, or post-Win7 clock API is used for measurement.

## Events and Fields

| Event | Meaning / event-specific fields |
|---|---|
| `scene` | Scene callback; `id` is the prior scene, `scene` the new scene. IDs are zero-based. |
| `bank_create` | Existing wave-bank hook completion, even before rate ever arms; `id=file_id`, `result` is original success byte, `counter0` is existing BankCreatePath (0 none, 1 stock, 2 committed, 3 late failed, 4 recovery failed, 5 TLS overflow). |
| `bank_unregister` | Existing wave-bank unregister completion; `id=file_id`. |
| `prepare_request` | Entry QPC plus return QPC; `id` is returned cue handle (-1 failure), `result` is bank slot, `name` is bounded cue name. Slot 5 is the song bank. |
| `ready_observed` | Return QPC and original byte result of a readiness call the game/mod already made. Consecutive equal `(handle,result)` observations are coalesced. A first false is not readiness. No additional poll is made. |
| `start_request` | Entry and return QPC at the start-prepared manager function; `id` is cue handle. No status/actual-start claim: the original returns void and can merely queue a pending start. |
| `stop_request` | Entry and return QPC at the stop-by-handle wrapper. |
| `anchor_delivered` | GPA-only 0x1044 broadcast entry/return; `counter0` is requested payload, `result=1` means the unsuppressed actor's post-original anchor equals it. `judge_mc` is blank. |
| `gameplay_sample` | Post/Late judge observation. `judge_mc` is the actual argument, `stored_mc` the separate GPA field; side is 0/1. |
| `frame` | Once-per-frame callout after layer dispatch; counters are frame sequence, poll count (one), jobs executed and remaining queue depth. These are counts and cadence, not measured per-callback CPU durations. |

`end_qpc=0` means the event is not a timed call pair. The producer captures call
entry time and enqueues after the original returns; CSV order is enqueue order,
not necessarily QPC order when calls nest or run concurrently. A boundary that
re-enters a scene callback is attributed at enqueue time; inspect its call pair
and intervening scene events rather than inferring a precise cross-scene span.
Cue names are at most 32 bytes and CSV-sanitized. No card/profile IDs are read.

`valid` is a bitmask. A populated raw anchor is not by itself proof of anchoring:

| Bit | Evidence |
|---|---|
| 1 | GPA identity and timing layout validated; actor/anchor/stored count read. |
| 2 | Nonzero anchor with matching GPA 0x1044 readback observed in this scene. |
| 4 | Frame clock read via `*frame_tick_global`, then object +0x1268. |
| 8 | SOUND/INPUT/RENDER/BOMB field layout and key strings attested. |
| 16 | Derived per-side Option base and live vtable +0x248 getter verified to be `MOV EAX,[RCX+0x24]; RET`. |
| 32 | Actual effective Q31 clock factor available (or unchanged stock instructions verified). |

Unavailable scalar fields are empty CSV cells. SOUND, INPUT, RENDER and Option
JUDGEMENT are milliseconds; BOMB is **frames**, not milliseconds. `rate_q31 / 2^31`
is the multiplier used by the authoritative clock stub. It is not a request
percentage or an asynchronously cached rate estimate. The producer does not use
the rate publication's spinning seqlock reader: init verifies the installed
redirect and compares the stub to the actual `build_clock_stub` generator, then
reads its aligned atomic factor directly. Unknown/modified code disables this
channel instead of guessing identity.

`progress_wall_ms` and `progress_mc` compare consecutive retained judge samples
only within the same known attempt, scene, segment, actor, anchor, offsets and
rate. They measure game-clock advancement versus QPC, **not audio drift**. In the
current binary, GPA onUpdate writes +0x178 at its tail AFTER judgeNotes, so that
field can lag the actual argument by one update. It is deliberately not used as
the current judge time. No residual is computed from it.

The metadata `channels` bits 0..6 indicate installed prepare, readiness, stop,
start-request, broadcast, judge subscriber and scene subscriber, respectively.
`actor_layout`, `offsets`, and `rate` are separate availability gates. Frame and
bank channels depend on the frame dispatcher and the already-existing bank hooks;
absence of their records is **not** evidence of zero activity or latency.

## Static Evidence

All following game addresses are RVAs relative to image base `0x180000000`.
No address in this table is compiled into an observer.

| Site | 20250805 | 20260224 | 20260721 | 20260825 |
|---|---:|---:|---:|---:|
| `audio_start_prepared` manager | 0x1959A0 | 0x198030 | 0x1AB720 | 0x1AB1C0 |
| Timing offset subscription quartet | 0x583BF (R12) | 0x57497 (RDI) | 0x5B7C6 (RDI) | 0x5B756 (RDI) |
| Raw-count tail store | 0x5A4B6 | 0x594F6 | 0x5D928 | 0x5D8B8 |

On 20260825, `song_play_by_bank` is RVA 0x1AA060 -> 0x1AB120:
sound-bank vtable +0x18 is Prepare. The game obtains the cue handle and polls
RVA 0x1AA0D0. DPS onMessage at 0x59090 handles 0x1044 by calling the one-argument
start wrapper 0x1AA120 -> manager 0x1AB1C0 `(manager*, i32 handle)`.

The manager checks the handle slot's prepared byte. If false, it writes a
pending-start byte and calls engine DoWork. Otherwise it calls the cue object's
Play (vtable +0), or a wave object's Play (+8), then DoWork. The diagnostic
detours this manager entry only while enabled. All original arguments/results
are forwarded exactly once; failure of the observer cannot repeat the original.
None of these boundaries is named `audible_start`.

The GPA constructor's four configuration subscriptions pin +0x16C/+0x170/
+0x184/+0x188. Init decodes each key-string LEA and checks the exact string;
20250805 has 20-byte R12 cells, later builds 19-byte RDI cells. The preceding
clock-load block pins +0x1268/+0x16C/+0x160/+0x84 and must decode to the derived
`frame_tick_global`. The raw-count store must lie in the same bounded onUpdate
window as the clock site, identified through the RTTI-derived GPA vtable.
Every live pointer is range-checked before dereferencing. Option base is the
existing derivation (0xF0 old / 0xE0 new), never a fixed build assumption.

The cross-build sweep resolves every new observation/attestation or its version
alternate. Shape comparison finds start/readiness/prepare/stop/broadcast identical
through 0x100 bytes. Offset-quartet divergence at +0x4C is AFTER its final
subscription; raw-store divergence at +0x48 is after the onUpdate return. The
diagnostic reads neither divergent region. Runtime field validation still fails
closed if any required shape/string changes.

## XACT Feasibility

Static inspection of the already-analyzed `xactengine2_10.dll` confirms
`x86:LE:64:default`: image base **0x400000 does not mean x86-32**. No debugger was
attached. It was temporarily opened without analysis, then closed again.
Engine addresses below are RVAs relative to 0x400000.

- `Sound_Play` 0x22AB0 activates tracks through 0x27A80 -> 0x9D30. The track
  stamps a masked QPC-derived millisecond start, activates wave events and calls
  `Wave_ComputeScheduledStartMs` 0x136C0.
- The scheduler computes event deadlines and calls
  `Sound_InsertWaveSortedOrStartNow` 0x21B10. A due event can invoke wave vtable
  +8 immediately; otherwise it is inserted into a time-sorted queue.
- `Sound_PumpUpdate_DrainScheduledWaves` 0x22DA0 drains pending waves according
  to the engine's pump window. Prior notify/render-thread RE is in
  `docs/xact_audio_research.md`; this task performed static checks, not a new
  measurement of packet cadence. The immediate-due branch means assuming every
  start is deferred to a future notification would be too strong.
- `Wave_StartNow_NoSampleOffset` 0x14180 obtains a voice through wave vtable
  +0x80, calls voice +8 with `(1,0,0,0)`, then writes wave +0x68 with
  `(QPC * 1000 / frequency) & 0x7FFFFFFF` (or a GetTickCount fallback). This stamp
  is after voice submission and has neither sample precision nor an output
  presentation guarantee.
- Current gamemdx's callback-registration first LEA (0x1AA7DB) points to
  notification handler 0x1A9B90. It consumes a packed type byte/context pointer
  and handles types 1, 4, 12, 16, 17 for manager bookkeeping. Type 12 sets a
  slot byte at +0x10; types 4/16 clear cue/wave slots. This does not establish a
  usable output-start notification. No notification hook was added.

The remaining deterministic-sync research is **open**: map this specific game
cue handle to the right internal track/wave/voice, capture an unwrapped timestamp
at the correct internal boundary, establish source-frame position and the
output device's queued/presented sample relation, and prove a safe one-time
pre-judgement handoff. Pointer/handle reuse and bank teardown need explicit
lifetime proofs. A voice-start callback arrival time is not a substitute.
Output loopback or physical capture is still needed to validate audible onset
and drift. There is no automatic anchor correction, extra DoWork pumping,
mid-song 0x1044, offset edit, or player-error calibration in this implementation.

## Integration and Validation

The service is registered in src/services/mod.rs and initialized in src/lib.rs
after the judge, song-rate and overlay dispatcher services. The once-per-frame
queue driver calls record_frame after original rendering. The scheduling fix is
documented in docs/frame_scheduling.md.

The bank callout is already added before `rate_recording_active()` in
`wavebank_hook::record_bank_event`; stock 100% requires no rate arm.

Run `bash scripts/validate_audio_sync_diag.sh` for the actual pure model's tests.
The task's logs retain expected RED cycles, final GREEN (15 tests), signature
sweep, layout checks and shape review. The integrated crate passes cargo check,
whole-crate formatting, normal and Win7 release builds. The Win7 import audit
confirms QPC/frequency imports and no ProcessPrng dependency. Compilation/static
RE do not prove live timing behavior or the diagnostic's overhead on the low-spec
cabinet. Use the cabinet checklist in docs/frame_scheduling.md before judging
runtime success.
