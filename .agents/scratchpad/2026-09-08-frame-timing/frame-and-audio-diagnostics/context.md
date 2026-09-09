# Frame Scheduling and Audio Diagnostics

Updated: 2026-09-08

## Request and Scope

Fix mod work dispatched once per widget rather than once per engine frame. Add
Win7-safe, default-off diagnostics for fresh-song audio/gameplay alignment and
research a deterministic synchronization mechanism. Do not alter judgement,
clock scaling, timing offsets, or audio scheduling without supporting evidence.

The reported reproduction is a stock low-spec Win7 cabinet, possibly 2 GB RAM
with integrated Radeon graphics: play the first third of one song, quick-fail
WITH results, reselect the same song. This is not an in-place reset. Exact
deployed configuration and traces are unavailable. Full cabinet reboot is the
normal restart procedure, not evidence that process restart cannot help.

## Instructions and Approval

- AGENTS.md and README.md are authoritative project instructions; no
  CODEASSIST.md exists.
- Read the architecture/interface summaries, reverse-engineering steering,
  overlay dispatcher research, audio research, and actual source.
- This is a direct implementation request, not a generated code task backed by
  an approved PDD plan. No upstream Status: Approved artifacts exist.
- Mode: auto after the required approach approval. No commits, pushes, or live
  cabinet deployments are authorized.
- Existing changes: mod-config.json and an unrelated untracked battle-mode
  research document. Preserve both; do not edit the operator configuration.

## Requirements and Acceptance Criteria

1. Frame callbacks/input polling and queued work execute once per actual frame,
   independent of wrapper count, without a millisecond/FPS throttle.
2. Keep native render-thread affinity, input-before-queued-work ordering,
   callback reentrancy safety, and per-wrapper/per-list visual emission order.
3. Self-requeued jobs wait for the next dispatch. A callback failure must not
   prevent remaining work or original engine rendering. No lock across user
   callbacks. Enqueuing never executes inline.
4. Replace wrapper-count-based timeout assumptions with elapsed time; prevent
   duplicate logical pump chains where the migration exposes them.
5. Add optional bounded diagnostics that distinguish repeated attempts of the
   same song and capture preparation/readiness/start requests, anchor delivery,
   effective timing state, and game-clock/frame progression against QPC.
6. Diagnostics default off; no background writer or diagnostic-only hooks when
   off. When on, producer operations have bounded nonblocking storage, no file
   IO/formatting, and explicit lost-record accounting. Bound output growth.
7. Never label request/callback timestamps as DAC presentation. Document which
   further measurement is needed to prove audio drift or deterministic start.
8. Validate pure scheduling/timing models, normal and Win7 builds, cross-build
   signature/consumer shapes, and document the live cabinet validation still
   required. Do not claim runtime success from compilation.

## Source Findings

- src/services/widget_renderer.rs: wrapper_render_hook currently polls input
  and drains pending_updates on every wrapper. Render-list context must remain
  available for overlay anchor emission.
- src/services/overlay_draw/mod.rs: the sole layer_dispatcher detour is an
  existing once-per-frame boundary. Ghidra confirms gamemdx 20260825 dispatcher
  at RVA 0x2AF60 has one caller, the main frame function at RVA 0x3000; its
  invocation is unconditional after actor draw preparation and before the
  consumer kick. Earlier 20260721/20260616 evidence is in
  docs/overlay_draw_research.md.
- src/services/input_manager.rs: frame callbacks are snapshotted and called
  before the ark gate and button reads. Keep this ordering.
- Frame-count assumptions exist in webui preview load timeouts, SMX window
  discovery, and SMX atlas warning. Other audio/training pumps generally use
  elapsed time. Audit countdown preparation at genuine frame cadence.
- Chrome-loader and series-filter pumps need coalescing/generation checks;
  PUS's queued raise can be requested more than once before execution.
- Quick-fail WITH results follows fail_song(None) and the natural commit/tail;
  PUS clears on next gameplay entry; per-song timing restores on exit.
- The song_play_by_bank name is misleading: prior RE identifies it as PREPARE.
  Actual start-prepared and internal XACT wave start are separate boundaries.
- Existing signatures include song_play_by_bank, song_is_prepared,
  song_stop_by_handle, update_broadcast, dps_timing_anchor_site, derived
  frame_tick_global and gameplay_actor_vtable. Reuse the owned judge dispatcher
  and bank hooks. The clock global holds a pointer; tick is at object +0x1268.
- Existing raw-music-count notes disagree about the option virtual getter;
  record raw quantities and verify before implementing a residual equation.
- Current stock-100% bank timeline gating is insufficient for this report.
  Training-enabled 100% still uses a passthrough binding (no DSP producer).

## Build and Test Commands

Run from repository root, logging output under this task's logs directory:

- cargo check --target x86_64-pc-windows-msvc
- cargo fmt (whole crate; inspect and preserve unrelated work)
- ./build.sh
- ./build_win7.sh
- ./scripts/validate_signatures.sh <local-game-build-corpus>
- scripts/sig_harness/shape_diff.py for changed fixed-offset consumers.
- New focused host validation script(s) mounting actual pure modules, following
  scripts/validate_custom_resolution.sh. Plain cargo test cannot build retour
  on this ARM host.

## Implementation Areas

core scheduling primitive and host tests; widget_renderer; overlay_draw;
input_manager; affected preview/SMX/pump consumers; diagnostics config;
services/audio_sync_diag; existing song-rate bank-hook callouts; lib.rs/service
registration; optional new content-derived start boundary; research/runbook.
