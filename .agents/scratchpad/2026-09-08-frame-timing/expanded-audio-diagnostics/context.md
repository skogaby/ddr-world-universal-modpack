# Expanded Timing Diagnostics

Updated: 2026-09-09

## Request

The tester can notice approximately 1 ms differences; reported score variation
is 700-1000 points out of one million. This is a meaningful measurement target,
not a reason to dismiss the small clock-rate difference. Score variation cannot
be converted into milliseconds without chart/grade data.

The first CrossOver capture (recorded in the sibling frame-and-audio-diagnostics
task) showed five identity-rate part plays, start-request/anchor pairing within
0.21 ms, stable once-per-frame work, 11-12 ppm game/QPC rate difference, and
gameplay-update gaps up to 95.8 ms. It did NOT measure audio presentation and was
not the original Win7 cabinet. User requests additional diagnostics, not clock
correction or a changed scoring policy.

## Instructions and Approval

Use AGENTS.md and README.md; CODEASSIST.md is absent. This is a direct follow-up,
not a code-task-generator task with upstream approved artifacts. Mode is auto
after local approach approval. Preserve all existing uncommitted work, including
the user's mod-config.json changes. No commit, push, deployment or live debugger.
Build logs go under this task's logs/; only relative paths in working documents.

## Requirements

1. Enabled-only QPC spans distinguish mod-frame poll/jobs, original layer walk,
   topmost emission, judge pre/original/post, and diagnostic sampling. Explicitly
   label wall-time/inclusive measurements, not GPU execution or pristine game CPU.
2. Individual judgement records retain grade, signed integer-ms error, note time,
   incoming judge count where known, and pre-submit death state. Reuse the one
   shared submit detour; never change its arguments, scores, windows or ordering.
3. Safely correlate prepared game cues to XACT scheduling and streaming-voice
   submission across immediate and deferred/threaded paths. Use validated live
   reciprocal ownership plus generation tokens, not nearest timestamps or TLS-only
   matching. Invalidated/unknown identities stay unmatched.
4. Passively observe the engine's existing DirectSound cursor reads if the exact
   supported backend is verified. Mixed-output cursor is NOT a per-song DAC cursor.
5. Internal engine hooks install in the factory-return/pre-Initialize window,
   gated by module identity and content attestations; missed window/unknown engine
   fails open with explicit unavailable channels. No hot-patching active pumps.
6. Diagnostic off adds no QPC calls or new diagnostic hooks/writer. Enabled
   producers remain allocation/IO/blocking-free; bounded summaries and rate-limited
   slow examples preserve room for lifecycle/hit records, with loss accounting.
7. Explicit v2 schema/output filename preserves the existing v1 capture. Fixed
   in-memory storage and a capped output file; an offline report reads v1 and v2.
8. Host TDD, native/Win7 build checks, game and engine signature/shape checks;
   no runtime success claim before another actual capture.

## Research Authority

- docs/audio_sync_diagnostics.md and docs/frame_scheduling.md (existing behavior).
- src/services/audio_sync_diag/{mod.rs,model.rs}: current bounded recorder.
- src/core/frame_pump.rs; services/widget_renderer, overlay_draw, judge_hook,
  input_manager; mods/power_user_statistics/data_feed: existing hook owners.
- XACT static RE: game start manager has a handle-slot cue pointer; event
  scheduling and streaming-wave submission are DIFFERENT boundaries. Below
  event start there is an additional pending-wave deferral.
- Streaming submission (engine RVA 0x25ED0) calls the voice interface Start;
  read-only chain W->E->event-state->track->sound->cue has reciprocal checks.
- Existing output-cursor update (engine RVA 0x35A50) calls DirectSound's
  GetCurrentPosition. Capture its existing results only, with buffer identity,
  ring size and format. No new output calls or interpretation as audible onset.
- Engine under study: AMD64 PE32+, timestamp 0x471C7720, SizeOfImage 0x69000,
  file size 404120. Runtime resolution uses AOBs and shape/identity validation,
  never these research RVAs.

## Validation Commands

Run at repository root: cargo check --target x86_64-pc-windows-msvc; cargo fmt;
./build.sh; ./build_win7.sh; bash scripts/validate_audio_sync_diag.sh;
bash scripts/validate_frame_pump.sh; relevant existing regression harnesses;
./scripts/validate_signatures.sh with the local four-build corpus and shape_diff.
Add offline tests for the report and exact engine-layout/signature validation.
Plain cargo test is not usable for the Windows/retour crate on an ARM host.
