# Recorder / Frame / Judge / Hit Track

Updated: 2026-09-09
Status: Complete (uncommitted; recorder/frame/judge/hit track only)
NEXT ACTION: Runtime off/on capture; report/docs/integration and full build gates are complete (see progress.md).
Resume: Read context.md and plan.md in this directory. Parent owns engine integration, analyzer, docs and builds.

## Scope and Acceptance

- Retain the first 32 CSV columns, append the agreed fixed trace metadata; 512 events, <=512 bytes/event, 32 MiB v2-only output.
- Enabled-only spans, single-try-lock recorder, bounded slow examples and summaries covering all accepted invocations with explicit loss counts.
- Preserve frame ordering/reentry and exactly-once forwarding; separate poll/jobs/layer/topmost and judge pre/original/post/sampling.
- Reuse the existing submit tap with pre-mod snapshots; nested per-thread actor-matched incoming count, grade/MS/note/dead validity, unknown finished blank.
- Do not install the submit hook early: normal PUS/calibration installation remains its owner. No extra legacy work for diagnostics.
- Existing gameplay decimation stays unchanged. Spans are wall-time, originals inclusive of synchronous descendants, not GPU time.

## Done

- Read approved plan and context; direct user approval supplies implementation authority (not generated tasks).
- Baseline host audio model and frame-pump tests pass; logs/spans-baseline-{audio,frame}.log.
- Read hook owners and judge_submit byte authority. CODEASSIST.md absent; AGENTS.md and README apply.
- RED/GREEN model + span tests and frame observer tests implemented in actual production pure modules. Subsequent RED cases cover adapter absence, judge dispatcher absence, nested summary ordering, context loss accounting, failed-clock nesting, and session-summary attribution.
- Optional FramePump observer preserves existing dispatch API, ordering, reentry and panic behavior. Frame callback/input callback IDs survive snapshotting; jobs use one-based batch positions, NOT stable closure identities.
- Shared judge owner uses the host-tested dispatch skeleton; sampling runs after all ordinary posts and after closing the inclusive judge span. Shared submit hook snapshots before legacy PUS/calibration/S-Marv work; original runs once between pre/post work. No new submit installation.
- V2 file, fixed trace extension, scope summaries, headroom, bounded slow examples and loss counters integrated. Windows target check passed once the separately-owned xact.rs appeared.

## In Flight

- No remaining edits in this track. Owned files formatted only (rustfmt skip_children avoids modifying engine-owned files).
- Host commands at repository root: bash scripts/validate_audio_sync_diag.sh; bash scripts/validate_frame_pump.sh. Parent owns full-crate formatting/builds/signature sweep.

## Deploy & Test Log

- No runtime actions authorized or performed.
- Final `bash scripts/validate_audio_sync_diag.sh`: 39 passed, 0 failed (includes the 8 actual frame-pump tests); logs/spans-green-audio.log.
- Final standalone `bash scripts/validate_frame_pump.sh`: 8 passed, 0 failed; logs/spans-green-frame.log.
- Final `cargo check --target x86_64-pc-windows-msvc`: PASS with the actual separately-owned xact.rs present; logs/spans-check.log.
- Owned-file rustfmt PASS; both shell harnesses pass `bash -n`; logs/spans-format.log and logs/spans-shell-check.log.
- New regression RED evidence: logs/spans-red-audio.log, spans-red-frame.log, spans-red-adapter.log, spans-red-judge.log, spans-red-summary-order.log, spans-red-context.log, spans-red-clock-nesting.log, spans-red-summary-context.log.

## Deviations & Open Questions

- Finished flag stays unavailable without byte attestation. +0x1E8 is explicitly a suppression byte; not a timestamp of physical death.
- Working record is this track-specific file, avoiding concurrent edits to the parent's context/progress/plan.

## Parent Integration Contract

- `model::Event` retains every existing field and first 32 CSV columns. New columns (zero-based 32..46): `trace_id,parent_id,thread_id,origin_attempt,origin_scene,origin_known,detail_valid,detail0..detail7`. `detail_valid` bit N governs detailN; absent details and unknown origin attempt/scene format BLANK. Total: 47 columns, centralized in `model::CSV_COLUMNS`.
- `model::Context { attempt:u64, scene:i32, epoch:u64, valid:bool }` is Copy. `Recorder::context()` uses one try_lock, failure is invalid and counted in context_contention.
- Private parent functions available to engine children: `active`, `clock` (-1 inactive/failed), `push(Event)`, `capture_context`. `publish_channel(bit:u8)` ORs late availability; init/header/loss snapshots also OR `xact::channel_bits()`.
- FINAL channel allocation: existing bits 0..6 unchanged; **7 = frame dispatcher spans, 8 = shared judge_submit tap**. Engine owns **16..21**. Earlier proposed 16/17 for this track were changed to avoid the actual engine allocation. Submit bit can appear after the CSV header; periodic loss rows carry updated channels.
- Already wired: `xact::on_prepare(slot,handle,name:[u8;32],context)` AFTER prepare original; `xact::on_start(manager,handle)` BEFORE start original; `xact::on_stop(handle)` BEFORE stop original. All engine observer calls are panic-contained separately from originals.
- Parent still supplies early engine initialization and wavebank entry `xact::on_bank_unregister()`; neither lib.rs nor wavebank_hook.rs was edited here.
- New kinds supplied for engine: Scheduled/VoiceStart/SoundStop/CueDestroyed/OutputCursor/EngineStatus with the agreed snake_case labels. Engine detail payloads remain engine-owned.

## Frame / Judge CSV Semantics

- `span`: `id` = Scope numeric ID below, `qpc/end_qpc` are raw entry/exit wall stamps; `trace_id` identifies this invocation, `parent_id` its synchronous enclosing span, `thread_id` the OS thread. `origin_*` is the entry snapshot; existing attempt/scene fields are recorder receipt context. `counter0` = callback registration ID or job batch position, otherwise zero. Numeric IDs only, no producer strings/allocations.
- Scope IDs: 1 Frame (poll + jobs + original layer + topmost + bookkeeping, inclusive); 2 Poll; 3 Jobs (queue extraction/drain); 4 Job; 5 LayerOriginal; 6 Topmost; 7 Judge (pre + original + posts, no tail sampling); 8 JudgePre (includes list snapshot); 9 JudgeOriginal; 10 JudgePost; 11 JudgePreCallback; 12 JudgePostCallback; 13 FrameCallback; 14 InputCallback; 15 InputExclusive; 16 SubmitPre; 17 SubmitOriginal; 18 SubmitPost; 19 JudgeSample; 20 HitSample; 21 FrameSample; 22 Submit (whole hook, inclusive).
- Judge span (scope 7) additionally has `actor` pointer and detail0 = incoming judge MC (bit0 valid). This is not the stale stored actor MC. Other ordinary spans have no detail cells.
- `span_summary`: cumulative PROCESS-wide accepted invocations of `id` (scope), emitted at most once per second per dirty scope, not per-attempt and not per callback. Existing context_valid=false/attempt=0/scene=-1/epoch=0 prevent attributing aggregate totals to one attempt. `qpc/end_qpc`, trace/parent/thread, actor, origin fields and counter0 identify the maximum-duration invocation. `observations` = count; counter1 = sum duration QPC ticks; counter2 = >=250us count; counter3 = suppressed slow-example count. detail0 = minimum duration ticks, detail1 = earliest start QPC, detail2 = latest start QPC (mask 7). Totals are cumulative: take latest observations, never sum repeated summary rows.
- Slow example policy: >=250us, at most 4 per scope per one-second window and 64 total per one-second window. A 512-slot ring reserves its last 128 slots for lifecycle/hit events; spans and frame/gameplay/cursor samples cannot consume that reserve. Summary state is separate from the ring and updates BEFORE example suppression.
- `judgement`: one pre-mod snapshot for opcodes 0x1028..0x102e; id = raw opcode, side=0/1 or -1, actor = incoming pointer (not a blanket layout-valid claim), trace/parent metadata copied from the submit invocation; qpc is that hook's raw entry stamp and end_qpc=0 (instantaneous snapshot, NOT execution duration). detail0=grade 0..6; detail1=signed integer-ms error (blank for OK); detail2=expected note MC; detail3=actual incoming judge MC iff same actor in innermost current-thread judge; detail4=pre-submit +0x1E8 suppression/dead flag; detail5=finished flag (currently ALWAYS unavailable); detail6=timing class (1 normal grade, 2 automatic Miss/window-edge value, 3 freeze OK/no error); detail7 reserved blank. Each detail has its own validity bit. Shock/NG/cancel are not interpreted as normal hit timing.
- The +0x1E8 read is gated by the exact `judge_submit+41` MOVZX bytes (already in its AOB) and actor identity/readability. Note pointer, expected time and scratch reads are independently readability-checked. Capture precedes the original so a death-causing hit retains dead_before=false.
- `gameplay_sample`: existing decimation unchanged (first 3s >=5ms, then >=250ms, >=50ms gaps force emission). Sampling moved to the explicit owner tail, after all ordinary post callbacks; sample trace points to its sampling scope, parent points to the completed judge span. This corrects the old Late-subscriber placement ambiguity.
- `frame`: same sampled cadence, but qpc now denotes raw dispatcher ENTRY, counters still describe the completed frame. FrameSample cost is outside the Frame span. Poll/jobs/original/topmost spans summarize every invocation regardless of sampled frame rows.
- Frame scope observations include nested dispatcher entries; Frame rows and FramePump sequence/poll counts are outer dispatches only. Do not equate cumulative scope-1 observations to the count of engine frames under reentry.
- Loss rows: full = critical ring-full drops; contention = record try-lock failures; suppressed_examples = slow budget/headroom suppressions; span_invocations = every span record attempt; span_contention = span subset of contention; invalid_spans = invalid clocks/scope IDs; sample_capacity = sample headroom drops; context_contention = failed entry-context snapshots. Sum of latest scope observations + span_contention + invalid_spans equals span_invocations (subject to the final unflushed interval/concurrent snapshot). Writer stop logs include these counters too.

## Limitations / Validation Boundary

- All spans are WALL time. Original judge/layer spans INCLUDE synchronous descendant mods/diagnostic costs; child sampling costs are separately visible, not subtracted to claim pristine game CPU. No GPU or physical audio/pad presentation claim.
- Span invocation IDs and engine cue-generation trace IDs are kind-specific namespaces; do not join engine and span rows solely by numeric trace_id.
- Slow examples are deliberately incomplete trees. Summary maxima retain exact timestamps and IDs even if their individual example was suppressed. Summary totals are session-wide; per-attempt distributions cannot be reconstructed from those totals alone.
- Unreadable/unattested fields stay unavailable. Finished is deliberately not inferred from +0x1E9 or another unproven field. Host tests exercise pure classification/context/forwarding/recorder behavior, not live memory lifetime or cabinet performance.
- No signatures, engine-owned files, lib.rs, wavebank_hook.rs, docs, analyzer or configJSON were edited. No staging, commits, builds/deploys or live actions. A type check is not a cabinet validation; parent retains release/Win7/signature gates.

## Changed Files

- src/services/audio_sync_diag/mod.rs, model.rs, new spans.rs, new spans_tests.rs
- src/core/frame_pump.rs
- src/services/widget_renderer.rs, overlay_draw/mod.rs, judge_hook.rs, input_manager.rs
- src/mods/power_user_statistics/data_feed.rs
- scripts/validate_audio_sync_diag.sh (mounts actual span and frame modules); validate_frame_pump.sh itself required no edit.
- This spans-progress.md only; parent planning/docs unchanged.
