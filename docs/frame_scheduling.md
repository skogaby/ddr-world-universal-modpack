# Once-Per-Frame Mod Work

Implemented: 2026-09-08. Host/build/static validation passed; live cabinet
validation is still required. This corrects scheduling, not audio synchronization.

## Boundary and Ownership

The old widget-wrapper hook called input_manager::poll and drained deferred
closures for every dirty wrapper. A self-requeued pump could run again in the
same frame, and a frame with no dirty wrappers did no mod-frame work. Prior
CrossOver observations reached approximately 2 kHz; that was not a measurement
of the reported Win7 cabinet.

The existing sole layer-dispatcher detour now owns the frame entry. It runs even
when no wrapper renders. No second detour, timer thread, millisecond gate, or
fixed-FPS throttle was added.

Game RVAs below are relative to gamemdx image base 0x180000000:

| Build | Dispatcher | Main-frame caller |
|---|---:|---:|
| 20250805 | 0x2AF50 | 0x3070 |
| 20260224 | 0x2AAB0 | 0x3050 |
| 20260721 | 0x2AF10 | 0x3020 |
| 20260825 | 0x2AF60 | 0x3000 |

The dispatcher has a single static caller in the inspected binaries. The
20260721 call was established in docs/overlay_draw_research.md; the older and
20260825 caller chains were checked again during this change. Invocation is
after actor update/draw preparation and before the render consumer kick. The
dispatcher AOB's normalized first 0x100 bytes agree across all four builds.

## Execution Contract

1. Enter the FramePump guard, which stays held through original rendering.
2. Poll registered frame callbacks, then arcade input, in the existing order.
3. Detach exactly one FIFO batch of queued work.
4. Execute that batch outside the queue lock, containing each job's panic.
5. Run the original layer dispatcher, including native wrapper-local emission.
6. Append the existing SMX topmost overlay and publish optional diagnostic counts.

Work enqueued during input polling joins the current batch. Work enqueued by a
queued job or original rendering waits for the next dispatch. Enqueue never
executes inline. Reentrant dispatch forwards the original but does not poll or
drain again. Two reusable vectors avoid allocating a fresh batch vector every
frame after capacities settle; creating a boxed continuation still allocates.

The menu's background remains at its own wrapper anchor, with dirty rearming
after original rendering. It was NOT moved to the start of the layer, which
would put it behind loading-screen art. The SMX overlay remains post-dispatcher.

Scheduling readiness is widget_renderer::frame_dispatch_available, independent
of font capture. Nonvisual reset/training drivers use it. Widget creation still
uses the existing font/readiness checks. Missing layer_dispatcher installs no
frame driver and emits an explicit WARN; there is no silent per-wrapper fallback.
A missing layer-table derivation disables dependent emission, not scheduling.

## Migrated Consumers

| Consumer | Change |
|---|---|
| Texture/background previews | 12-second elapsed load budget instead of 3600 wrapper polls; readiness wins at the boundary |
| SMX window discovery | Immediate attempt, then two-second elapsed pacing |
| SMX atlas warning | Ten seconds unresolved; continue accepting late readiness |
| Menu chrome loader | Coalesce concurrent completions/self-requeues into one pending pump |
| Series-filter scroll | Generation-bound activation and cursor pump; close/rebuild invalidates old work |
| PUS widget raise | One pending raise; recheck scene/menu and current wrappers at execution |
| Input callbacks | Contain individual callback failures; a failing exclusive consumer still consumes that event |

Existing time-based audio leads and synchronous reset subscribers are unchanged.
Prepare readiness is now observed at real frame cadence, not multiple times per
frame. In particular, delayed replay's newly queued readiness check may run one
frame later than the old busy-poll path; its prepared-to-anchor operation remains
synchronous. Validate instant/delayed resets and movie synchronization on cabinet.

## Verification

- bash scripts/validate_frame_pump.sh: 8 tests (ordering, no time/widget gate,
  callback/original reentrancy, panic containment, cross-thread enqueue).
- bash scripts/validate_frame_consumers.sh: 9 tests (elapsed deadlines,
  readiness at expiry, coalescing, cancellation, concurrent reservations).
- Audio diagnostic v2: see docs/audio_sync_diagnostics_v2.md for expanded tests,
  duration observations and capture instructions. The scheduling policy is unchanged.
- Existing mod-menu, custom-options, training, overlay, calibration and
  song-rate harnesses passed. The song-rate run was synthetic-only, not a
  release-corpus or live-audio validation. Its existing unquoted heredoc emits
  backtick command warnings; all 281 unit tests and synthetic checks still pass.
- Windows-target cargo check, cargo fmt, normal release and Win7 release pass.
- Four-build signature sweep all green; consumed-byte shape review passed.
- Win7 PE imports include QPC/frequency and SystemFunction036, not ProcessPrng,
  bcryptprimitives, WaitOnAddress, or GetSystemTimePreciseAsFileTime.

## Cabinet Checklist

First run with audio diagnostics OFF, then repeat with them ON to assess the
observer effect. Keep resolution, refresh, audio backend and player settings
fixed between captures.

- Boot, attract, login, loading screens and results: menu input and animation
  remain alive even when stock text is static.
- Open/close both option menus and the VERSION filter repeatedly; navigate fast;
  late preview/chrome loads still appear, and stale pumps never touch old objects.
- PUS statistics stay below an open mod menu. SMX remains topmost and touch/keypad
  input works, if that hardware is present.
- Instant/delayed restart, training markers/loop/scrub, preview rate edits and
  synchronized movies retain their behavior. Exercise multiple display FPS values.
- Reproduce first-third play -> quick-fail WITH results -> same-song reselect.
  Compare a fixed chart prefix and separate attempts. Preserve the diagnostic CSV
  and game log before the next enabled launch replaces the CSV.

No claim of improved timing, lower measured frame cost, or fixed dropped panel
inputs is made until those checks run on the affected hardware.
