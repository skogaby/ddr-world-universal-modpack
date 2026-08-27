# Summary: Background Movie Sync

PDD run completed 2026-08-20. All gates passed same day: register approved
(13 decisions), readiness confirmed, design approved, plan approved.

## Artifacts

- `rough-idea.md` — the refined idea (toggle semantics maintainer-specified)
- `idea-honing.md` — decision register D1–D13 (D9 research-resolved)
- `research/orientation.md` — codebase survey, prior art, unknowns
- `research/movie-sync-re.md` — Ghidra RE: IMediaSeeking slot pinned
  (+0x58), native seek discovered (vtbl +0x58; old RE doc's +0x48/+0x58
  labels corrected), loop-via-seek EC_COMPLETE semantics, cross-build
  verification (20260616 + 20260721), ffprobe corpus (386 movies, all
  video-only)
- `design/detailed-design.md` — Approved 2026-08-20
- `implementation/plan.md` — Approved 2026-08-20, 5 steps
- `progress.md` — live implementation resume point (maintained during
  implementation)

## Design in one paragraph

A new `src/services/movie_sync.rs` keeps the DirectShow background movie
aligned with the song: position sync (always on, any rate) drives the
game's own native seek from `song_reset` timeline-jump notifications, and
rate sync (opt-in via a new SYNC BACKGROUND VIDEO child row of SONG SPEED,
default OFF) applies one `IMediaSeeking::SetRate` per song instead of
suppressing the movie at non-100 % speeds. Zero new AOBs (player captured
from the existing BuildGraph detour), fail-open everywhere (rate failure ⇒
stop the movie = today's static background; 100 % seek failure ⇒ today's
desync), suppress-mode Wine boxes unaffected by construction.

## Plan shape

1. Service skeleton + live diagnostic probe (decision point: SetRate
   acceptance on the cabinet)
2. Always-on position sync at 100 %
3. Rate directive carriage through song_rate + SetRate + failure ladder
4. Option row + persistence (bemani-buddy migration 018 — external repo)
5. Delayed-restart pause/run, `DDR_MOVIE_SYNC_FAULT`, docs

## Implementation outcome (feature COMPLETE 2026-08-24)

Both halves shipped and are cabinet-validated on BOTH platforms
(maintainer-confirmed clean passes on the Windows box and the CrossOver
bottle). The full test trail — 14 CrossOver deploys for position sync,
5 Windows tests + 2 CrossOver trials for rate sync — lives in
`progress.md`'s deploy log.

**Position sync** shipped essentially as designed, after the cabinet
iterations replaced the planned "seek on notification" with the unified
PARK model (design §1b amendment): every timeline alteration classifies
via the pure `drain_action` and executes as seek → pause-on-frame-deposit
→ run-at-the-live-count-crossing, with per-capture measured restart
latencies (no cross-song state, no hardcoded offsets — maintainer
directives). FR-7 (delayed restart) fell out of the park model for free.

**Rate sync** shipped with a DIFFERENT mechanism than designed. The
design's `IMediaSeeking::SetRate` is impossible on the game's WMV chain —
the game's own custom renderer ('0001'), WMVideo Decoder DMO, and WM ASF
Reader refuse it categorically (E_INVALIDARG at every rate, paused AND
running, per-filter probes; Windows tests #1–#3) — so FR-3 was amended
(maintainer-approved) to the **scaled reference-clock proxy**:
`movie_sync::rate_clock`, a single-instance `IReferenceClock` wrapping
the graph's own sync source (`GetTime = T0 + (inner − t0)·(source/output)`,
advises inverse-mapped and forwarded), installed via
`IMediaFilter::SetSyncSource` at graph open. The install is ADAPTIVE:
in-place where the FGM permits a paused-graph swap (Wine), escalated
stop → swap → re-pause on `VFW_E_NOT_STOPPED` (Windows; Wine's cue/stop
state machine wedges on the unnecessary dance — CrossOver trial #1).
Verification is by `GetSyncSource` POINTER READBACK, never the HRESULT;
every failure degrades PRE-RUN to the suppress path's exact observable
state (direct `IMediaControl::Stop` + `opened=0` — post-Run degradations
strand a black plane or a frozen first frame, both live-observed).

**D14 SUPERSEDED (2026-08-24):** its Windows-only rationale was
SetRate-specific; the clock proxy is platform-uniform and the SYNC
BACKGROUND VIDEO toggle now works under CrossOver/Wine too (fallback
movie mode required for movies to exist at all). The interim Wine-trial
env was removed once trial #2 passed. D14's dead ends remain dead:
builtin quartz SetRate no-op, native quartz hard-lock, drift correction
rejected.

Diagnostics that ship: per-song clock-consumption INFO + 15 s
never-reached-RUNNING wedge WARN (both self-diagnose "verified but
ignored/wedged" outcomes), `DDR_MOVIE_SYNC_PROBE` (graph dump +
per-filter SetRate evidence + rate/seek pokes), `DDR_MOVIE_SYNC_FAULT=
set-rate|seek` (dev-mode failure-ladder injection).

Deliverables beyond `src/services/movie_sync.rs`: the `sync_movie`
option row in `song_playback_speed.rs` (+ en/ja/ko textures via
`option_strings.py`/`gen_option_labels.py` — NOTE: PNGs under
`data_mods/custom_options/` must be deployed alongside the DLL or the
row renders with a blank label), the `desired_sync` carriage through
`song_rate` (runtime → lifecycle → transaction, host-tested), the shared
`core::platform::running_under_wine()` helper, and bemani-buddy
migration 018 + `mod_sync_movie` verbatim passthrough (external repo,
tests + regenerated `.sqlx` cache).

Residuals / follow-ups (out of feature scope):
- stch (mcode 38172): training scrubs unavailable — pre-existing
  song_rate `UnsupportedProfile` bind refusal (audio bank format quirk),
  logged in progress.md.
- `ntdll_state_shim.rs` retained UNCALLED as the proven
  LdrRegisterDllNotification pre-DllMain IAT-patch pattern (abandoned
  native-quartz experiment).

## Next steps

Implementation proceeds in-session (maintainer-directed): Step 1 through
first cabinet test, no commits (maintainer commits manually). Alternately,
run code-task-generator against `implementation/plan.md` per step, then
code-assist per task.

## Assumptions / watch items

- SetRate acceptance on the cabinet's quartz + WM ASF Reader graph is the
  single design-invalidating risk — retired first by the Step 1 probe.
  *(Outcome: the risk materialized in full — SetRate is dead on the
  chain — and was resolved by the clock-proxy mechanism amendment.)*
- All training/restart repositioning is assumed to flow through
  `song_reset` notifications; the capture-time position sync covers
  pre-open seeks either way. *(Held.)*
- FR-7 (delayed-restart pause/run) has a documented seek-at-reset fallback
  if the anchor notification wiring proves invasive. *(Neither was needed —
  the park model delivers FR-7 structurally.)*
