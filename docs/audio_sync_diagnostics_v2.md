# Audio/Game Diagnostics V2

Implemented 2026-09-09. Static/host/build validation passed; runtime captures
on CrossOver (2026-09-09, five launches) confirmed every channel. Approximately
1 ms is a meaningful target for this work. The diagnostic itself performs no
clock correction, offset adjustment, judgement-window change, or scoring change.
The correction that the diagnostic motivated ships separately as the
`gameplay-timing-fixes` mod (`docs/audio_clock_research.md`); when that mod is
enabled the recorder additionally captures one `onset` row per clock arm (see
Engine Events) — the recorder and the correction are independent switches.

## Capture

Use the same boot-only switch:

```json
{
  "diagnostics": {
    "audio_sync": true
  }
}
```

V2 writes **audio-sync-diagnostics-v2.csv** in the game working directory. It
does not overwrite the original audio-sync-diagnostics.csv v1 evidence. Another
enabled v2 launch replaces the v2 file. The output cap is **32 MiB**; a full file
or writer failure stops recording while observers remain harmless passthrough.
The DLL entry point performs no shutdown IO, so abrupt exit can lose the final
buffered records/summaries.

First compare diagnostics OFF and ON with the same settings to assess observer
overhead. For the actual timing investigation, repeat a fixed chart prefix and
keep song rate, offsets, resolution, MSAA, refresh and audio backend unchanged.
Record which attempts used natural completion, quick exit with results, skipped
results, or an in-place reset. A CrossOver capture does not qualify Win7 behavior.

The engine observers need the early boot window. They intercept the game's XACT
factory before the full signature scan, then install at factory return before
Initialize starts audio threads. If the engine is already loaded, appears during
the bootstrap scan, or the factory is missed, **no late engine patching occurs**.
The normal frame/hit/game-clock channels can still work. Check EngineStatus and
channel metadata instead of interpreting absent engine events as zero latency.

## Offline Report

From this repository:

```bash
python3 scripts/analyze_audio_sync.py "$DDR_WORLD_INSTALL/audio-sync-diagnostics-v2.csv"
python3 scripts/analyze_audio_sync.py "$DDR_WORLD_INSTALL/audio-sync-diagnostics-v2.csv" --json
python3 scripts/analyze_audio_sync.py "$DDR_WORLD_INSTALL/audio-sync-diagnostics-v2.csv" --chart-start-ms 3000 --chart-end-ms 35000
```

The chart limits restrict **judgement rows**, using note timestamps, to compare
the same prefix. The report also accepts v1 files; missing v2 channels stay
unavailable. It reports cumulative scope statistics, the slowest retained
examples, per-attempt grade/error populations, known cue submission intervals,
and clock fits within valid continuous domains. It rejects malformed/truncated
CSV rows rather than treating missing fields as zero.

Scope summaries are PROCESS-CUMULATIVE. The report uses the latest totals,
never sums successive summaries. Original scopes include synchronous children;
adding parent and child durations double-counts time. QPC spans measure elapsed
wall time, including waits/preemption, not CPU utilization or GPU execution.

## What Was Added

| Channel | What it measures |
|---|---|
| Frame scopes | Poll/frame callbacks, queued jobs, original layer dispatch and SMX topmost emission separately |
| Judge scopes | Pre callbacks, original judge call, post callbacks, and diagnostic sampling separately |
| Submit scopes | Shared per-hit hook's pre/original/post work and its own hit sampling |
| Judgement records | Grade, signed integer-ms error, expected note timestamp, matching outer judge count, pre-submit suppression/dead flag |
| XACT scheduling | A verified wave event's scheduled deadline and ownership, for both immediate and queued paths |
| Streaming submission | Interval containing the underlying voice Start call, after both event and streaming-wave deferrals |
| Mixed-output cursor | Existing DirectSound GetCurrentPosition results, ring size, accumulated played bytes and output format |
| Audio-clock onset (`gameplay-timing-fixes` on) | The song voice's exact first output frame `F0`, the cursor sample it was latched against, the mean-preserving latency constant `C`, the fit's state, and `delta_vs_stock` — THIS play's stock onset error, i.e. how far the stock anchor would have been from the true DAC onset |

The v1 measurement stops at a game start request. V2 goes deeper: with the
`gameplay-timing-fixes` mod enabled, the `onset` row DOES establish when the
first sample of a particular song entered the mixed output (`F0`, exact to the
frame — see `docs/audio_clock_research.md` §3.5), and the report's
`audio_clock_summary` gives the per-play stock onset error distribution. Without
that mod the earlier caveat stands: the output cursor belongs to the mixed
stream, clock fits from short or quantized cursor segments can mislead, and no
per-song sample-to-output map exists. Neither mode captures the analog output;
the constant device latency past the cursor stays inside SOUND OFFSET.

## Schema

Header: audio-sync/v2, QPC frequency, availability flags and collection budgets.
The first 32 CSV columns retain the v1 names/order. Added columns are:

```text
trace_id,parent_id,thread_id,origin_attempt,origin_scene,origin_known,
detail_valid,detail0,detail1,detail2,detail3,detail4,detail5,detail6,detail7
```

detail_valid bit N controls detailN; absent cells are blank. Origin records
entry/cue provenance; the old attempt/scene columns describe recorder receipt
context. They can differ across a synchronous scene transition. Span invocation
IDs and engine cue-generation IDs are separate namespaces: never join different
kinds solely because their numeric trace_id matches.

### Frame and Judge Scopes

`span`: id is the scope below; qpc/end_qpc bracket that invocation. counter0 is a
callback registration ID or one-based job position where applicable. Jobs are
not identified by a stable closure address. Original layer scope includes its
wrapper hooks; original judge scope includes nested submit hooks/mods.

| ID | Scope | ID | Scope |
|---:|---|---:|---|
| 1 | Frame | 12 | JudgePostCallback |
| 2 | Poll | 13 | FrameCallback |
| 3 | Jobs | 14 | InputCallback |
| 4 | Job | 15 | InputExclusive |
| 5 | LayerOriginal | 16 | SubmitPre |
| 6 | Topmost | 17 | SubmitOriginal |
| 7 | Judge | 18 | SubmitPost |
| 8 | JudgePre | 19 | JudgeSample |
| 9 | JudgeOriginal | 20 | HitSample |
| 10 | JudgePost | 21 | FrameSample |
| 11 | JudgePreCallback | 22 | Submit |

`span_summary` observations = cumulative accepted calls. counter1 = cumulative
duration ticks, counter2 = calls >=250 us, counter3 = suppressed slow examples.
qpc/end_qpc and trace/origin/counter0 identify the MAXIMUM invocation, not the
summary's emission time. detail0 is minimum ticks; detail1/detail2 are earliest/
latest invocation starts. These rows intentionally have unknown ordinary attempt
context because their totals span the process, not one song.

Judge detail0 carries the actual incoming music count. Sampling is now at the
owner's tail, after all ordinary post callbacks, not an earlier same-priority
Late subscriber. Sample costs are outside the Judge scope. A nested judge
restores the previous thread-local context; an actor mismatch is unknown.

Frame rows now use dispatcher ENTRY QPC and describe the completed outer frame.
The Frame scope includes nested entries if reentry occurs, while frame rows and
the actual frame-pump sequence count outer dispatches. No whole-engine update,
present-call, or GPU timestamp query was added; those unmeasured regions must
not be assigned to the layer-dispatch scope.

### Individual Judgements

`judgement` is a pre-mod snapshot, not a duration span. detail0..6 are:

```text
grade, signed_error_ms, expected_note_ms, incoming_judge_mc,
dead_before, finished, timing_class
```

Grades 0..6 are M/P/G/Gd/Boo/Miss/OK. Timing class 1 is a normal grade, 2 an
automatic Miss/window-edge value, 3 freeze OK. OK has no error, and shock/cancel
payloads are not decoded as normal hit timing. Finished remains unavailable
without an attested field; it is not guessed. The pre-submit +0x1E8 suppression
flag is byte-attested. Capturing it before original preserves a legitimate hit
that itself causes death while distinguishing already-dead/failure-tail events.

The report's timing-eligible population requires grade 0..4, known error and
dead_before=false. This is NOT proof of human input: autoplay/input-source
provenance is not independently measured. QPC improves observation timing; it
does not turn the game's integer-ms judgement error into fractional-ms data.

### Engine Events

Engine events carry cue generation in trace_id and the game handle in id only
when the cue is known. Cue/bank identity is captured on prepare through the
derived audio-manager global, before unrelated sound cleanup during the READY
delay. Start revalidates the handle. Submission reconstructs live reciprocal
wave/event/track/sound/cue ownership, so asynchronous starts do not rely on TLS
or nearest-time matching. Stops, destruction, bank unregister and critical map
update loss invalidate attribution. Unknown cases stay unmatched.

| Kind | detail0..7 |
|---|---|
| scheduled | sound, event, wave, cue, bank, scheduled_ms, pump_ms, event_start_target |
| voice_start | wave, event, voice, Start_target, cue, sound, flags, branch |
| sound_stop | sound, cue, bank, immediate (remaining cells unavailable) |
| cue_destroyed | cue, sound, bank (remaining cells unavailable) |
| output_cursor | backend, DirectSound buffer, play bytes, write bytes, accumulated played bytes, ring bytes, sample Hz, block alignment |
| onset | F0 (song's first output frame), W, P, Wc (the cursor sample the arm used, frames), t_k (that sample's QPC, also in end_qpc), W−Wc (lead, frames), Wc−P (margin, frames), delta_vs_stock in µs — id = output Hz, result = content origin ms (0 normal start, t_q for a seek), counter0..3 = fit n, fit residual SD µs, C µs, voice generation |
| engine_status | channels, correlation epoch, critical-update losses |

voice_start branch: -1 unknown, 0 skipped, 1 Start succeeded, 2 failed. The QPC
pair brackets the ORIGINAL enclosing function, not the precise internal call
instruction. For scheduled/voice_start, counter0 contains pre-call ownership
probe ticks iff counter3 bit0 is set. That overhead really delayed entry and
must be considered in an off/on comparison; excluding it from the original
interval is not the same as removing its effect on scheduling.

Output cursor records are capped at four per second. detail_valid bit8 marks
continuity since the previous emitted sample. Buffer/format change, failure,
reset, excessive observation gap or sampler contention invalidates continuity.
The report fits all retained samples in a continuous segment against QPC call
midpoints, and reports endpoint difference separately. This is mixed-output
cursor progression, not a per-song playback position.

EngineStatus result: 0 unavailable, 1 factory armed, 2 installed, 3 missed
window, 4 unsupported, 5 install failed, 6 factory unavailable, 7 manager layout
unavailable. The report warns for 3..7. Supported engine code is gated on AMD64
PE identity plus exact consumed-code fingerprints; other engines fail open.

`onset` rows are emitted from the game thread when the deterministic audio
clock ARMS for a voice (normal start, quick restart, or a training seek — each
replay is a new voice and a new row), never from the render thread. They are
the recorder's only per-song "sample 0 reached the mixer" evidence. The report
prints one line per arm plus `audio_clock_summary` (arm count, `delta_vs_stock`
distribution, fit residual SD distribution — the cursor's reporting granularity
on this platform — and lead+margin). A wide `delta_vs_stock` spread with a tight
fit SD is the stock defect being measured, not a fault in the capture.

### Channels and Bounds

Bits 0..6 retain prepare/ready/stop/start/broadcast/judge/scene meanings. Bit7 is
frame timing; bit8 is the shared submit tap. Engine bits 16..21 mean factory,
schedule, streaming submission, sound stop, cue destruction and mixed cursor.
Later `# loss` rows update the mask when a channel installs after the header.

The ring holds 512 fixed events, each <=512 bytes. Its last 128 slots are
reserved for lifecycle/hit records. Producers try the lock once and never wait,
format strings, write files or allocate recording payloads. Every enabled span
contributes to an aggregate unless its record operation itself is lost; slow
examples are limited to four per scope and 64 total per second. Summaries are
written at most once per second; existing sample decimation is retained.

Inspect full, contention, span_contention, invalid_spans, sample_capacity and
context_contention loss counters. suppressed_examples is deliberate budget
suppression, not lost gameplay input. The latest cumulative scope counts plus
span_contention and invalid_spans should account for span_invocations, apart
from snapshots in flight/final unflushed summaries. A return-only span cannot
describe a call that never returned; absence does not mean zero duration.

## Validation

- `bash scripts/validate_audio_sync_diag.sh`: 39 tests, including 8 frame tests.
- `bash scripts/validate_frame_pump.sh`: the same 8 frame tests independently.
- `bash scripts/validate_xact_diagnostics.sh`: 21 tests including actual engine
  fingerprint checks and factory/manager validation on four game builds.
- `python3 -m unittest discover -s scripts -p test_analyze_audio_sync.py`: 13 tests
  (incl. the `onset` row summary).
- The real v1 capture still parses and reproduces its frame cadence/start gaps.
- Windows check, whole-crate formatting, normal and Win7 release builds pass.
- Game signature sweep all green; engine tests separately validate every consumed
  code span, changed layouts, ASLR relocation and IAT changes.
- Calibration, overlay, training and song-rate regression harnesses pass; the
  song-rate script retains its pre-existing harmless heredoc shell warnings.

Runtime captures (CrossOver, 2026-09-09): five launches with the recorder ON
alongside the `gameplay-timing-fixes` mod — every channel populated, no lost
summaries, ~2.5–4 MB per 10-minute session (an OFF/ON observer-overhead
comparison has not been run). The `onset` channel measured the stock per-play
onset error at −11.1 … +3.5 ms over ~19 plays (uniform-looking, as the 10 ms
mix-pass model predicts) and the CrossOver DirectSound cursor's residual SD at
4.2–4.6 ms (the 512-frame staircase). Win7 capture still pending.
