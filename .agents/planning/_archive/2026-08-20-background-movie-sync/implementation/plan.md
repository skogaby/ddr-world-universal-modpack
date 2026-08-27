# Implementation Plan: Background Movie Sync

Status: Approved 2026-08-20

Design: `.agents/planning/2026-08-20-background-movie-sync/design/detailed-design.md`
(Approved 2026-08-20). Per-step validation gates: `cargo check` →
`cargo fmt` (whole crate) → `cargo test` → `./build.sh` → cabinet deploy +
log observation. Maintain `progress.md` in this directory throughout.

## Checklist

- [x] Step 1: Service skeleton + live diagnostic probe
- [x] Step 2: Always-on position sync at 100 %
- [x] Step 3: Rate directive carriage + SetRate + failure ladder
- [x] Step 4: SYNC BACKGROUND VIDEO option row + persistence
- [x] Step 5: Delayed-restart pause/run, fault injection, docs
      (implementation complete 2026-08-22; the live validation matrix —
      incl. the FR-7 delayed-restart confirm, delivered by Step 2's park
      model rather than new code — is the maintainer's cabinet checklist
      in progress.md)

## Step 1: Service skeleton + live diagnostic probe

**Objective:** Prove the design's live-only unknowns (SetRate acceptance,
seek behavior, duration/caps readability) on the real cabinet graph before
building the feature on top of them — the design's front-loaded risk.

**Guidance:**
- Create `src/services/movie_sync.rs`: capture state (player `AtomicPtr` +
  generation), the COM validity gate, capture-drop on scene change
  (scene_manager callback), and `init()` wired in `src/lib.rs` after
  `movie_policy::init`.
- Extend `movie_policy`'s hook: `CallOutcome::Passthrough && hr == 0` →
  `movie_sync::on_graph_opened(this)`.
- Probe path, gated on env `DDR_MOVIE_SYNC_PROBE`: at graph open, log
  loop flag (+0x16), `GetCapabilities`, `GetDuration`, `GetRate`; attempt
  `SetRate(1.25)` → read back → `SetRate(1.0)` restore; native seek (vtbl
  +0x58) to 5 s then back to 0. Log every hr. Without the env var the
  service only captures and logs one INFO per open.

**Tests:** capture/validity-gate unit tests where pure (generation
invalidation, gate predicate on synthetic player bytes). Probe itself is
live-validated.

**Integration:** movie_policy is the only caller; no behavior change
without the env var.

**Demo:** deploy probe build; on a movie-backed song the log shows
caps/duration/SetRate/seek results — visually, the probe's 5 s seek blip
confirms seeking works. Decision point: if SetRate is rejected on the
cabinet, stop and re-plan (the fallback rung becomes the rate deliverable).

## Step 2: Always-on position sync at 100 %

**Objective:** The core end-to-end deliverable — a playing movie follows
every song-timeline jump at 100 % speed, independent of the
song-playback-speed mod.

**Guidance:**
- Pure position mapping (loop modulo / clamp / unknown duration / negative
  t → 0) as a host-testable function.
- Cache loop flag + duration at capture; subscribe
  `song_reset::on_song_reset(t_ms)` → mapped native seek (panic-contained,
  validity-gated).
- Add the `song_reset` accessor for current song content ms; implement the
  capture-time position sync (>500 ms threshold).
- One-shot per-capture WARN latch for seek failure (status-quo desync).

**Tests:** host tests for the mapping function and latch logic; existing
`song_reset` tests untouched.

**Integration:** consumes Step 1's capture; no song_rate involvement yet.

**Demo:** at 100 %: press-1 quick restart → movie restarts with the song
(including in-place training loops and FF/RW scrubs, and SONG START > 0
via the capture-time sync). Attract mode and versus unaffected.

## Step 3: Rate directive carriage + SetRate + failure ladder

**Objective:** Non-identity songs can keep their movie rate-locked instead
of suppressed, driven by a latched per-side flag (no UI yet).

**Guidance:**
- `song_rate`: `DESIRED_SYNC_MOVIE` atomics + setter (`runtime.rs`);
  `EligibilityInputs.desired_sync` → `ArmRequest.sync_movie` →
  `LifecycleState` (mirror `preserve_pitch` carriage); arm/commit skip the
  `MovieSuppressor::SongRate` set when latched ON **and**
  `movie_sync::is_available()` **and NOT under Wine** (D14: Wine
  suppresses at non-100 % regardless of the toggle — hoist a shared
  `running_under_wine()` helper out of `mfplat_vih_fix` for the gate);
  committed-directive accessor for movie_sync (effective rate + sync
  flag).
- `movie_sync::on_graph_opened`: consume the directive — `SetRate(rate)`
  when rate ≠ 1 (Windows-only by construction: under Wine the directive
  never arrives, the graph was suppressed); verify by `GetRate` readback
  (epsilon), NOT hr; on null IMediaSeeking, hr < 0, or readback mismatch,
  issue the stop command (+0x0C = 4 via vtbl +0x28) + one WARN (the
  failure-ladder rung).
- Dev-only override to exercise before the row exists: env
  `DDR_SYNC_MOVIE_FORCE=1` sets both sides' desired atomics at init
  (removed in Step 4).

**Tests:** lifecycle host tests — flag latches from the entered side;
suppressor NOT set at arm/commit when sync ON (via the existing `MovieSink`
double); identity/versus/failure paths still suppress or stay untouched
per the design's decision matrix. Ladder decision logic as a pure
function, table-tested.

**Integration:** builds on Step 2's engine; song_rate seam changes are
inert while the desired atomics stay false.

**Demo:** with the force env: a 50 % and a 150 % song play with the movie
rate-locked to the audio (pitch mode irrelevant); without it, non-100 %
suppresses exactly as today; fault-forcing SetRate (temporarily) lands on
a static background with the song unaffected.

## Step 4: SYNC BACKGROUND VIDEO option row + persistence

**Objective:** The user-facing toggle, per-side, profile-persisted.

**Guidance:**
- `src/mods/song_playback_speed.rs`: register `sync_movie` bool child row
  after `preserve_pitch` (default OFF, `ShowWhen::NotEquals(song_speed,
  100)`, `PersistMode::Full`, bool clamp, `on_change` →
  `set_desired_sync_movie`); enable-time re-seed + disable-time reset;
  non-fatal registration failure. Remove the Step 3 force env.
- Textures: en/ja/ko strings in `scripts/option_strings.py`, regenerate
  via `scripts/gen_option_labels.py` (`seop_item_sync_movie`,
  `seop_image_sync_movie_{off,on}`).
- bemani-buddy (external repo): migration 018 nullable
  `opt_mod_sync_movie` + verbatim wire passthrough for `mod_sync_movie`
  (shape of migration 014).

**Tests:** row registration/seed/reset covered by the mod's existing
test pattern where host-testable; persistence round-trip validated live
(card-in/card-out).

**Integration:** replaces the dev override as the only writer of the
desired atomics; completes the design's decision matrix end to end.

**Demo:** set SONG SPEED to 75 % → SYNC BACKGROUND VIDEO appears; ON →
next song keeps its movie in sync; OFF → suppressed; value follows the
profile across card-out/card-in; hidden again at 100 %.

## Step 5: Delayed-restart pause/run, fault injection, docs

**Objective:** FR-7 polish, permanent fault hooks, and documentation.

**Guidance:**
- `song_reset`: distinguish delayed resets to movie_sync (pause at reset)
  and add the anchor-landed notification (run + re-seek at the existing
  idempotent re-anchor). If invasive, fall back to seek-at-reset and
  document the countdown drift.
- Permanent `DDR_MOVIE_SYNC_FAULT` env (dev mode) forcing
  SetRate/seek failures.
- Full live validation matrix (design Testing Strategy §live): 100 %
  suite, rate suite both pitch modes, delayed restart, fault injection,
  CrossOver fallback best-effort pass, suppress-mode inertness.
- Docs: AGENTS.md entry-point row + README feature blurb; note the
  vtable-label corrections for the old movie RE where AGENTS.md references
  it.

**Tests:** host tests for the pause/run state decisions (pure part);
everything else live per matrix.

**Integration:** completes the feature; no orphaned code (probe env from
Step 1 either removed or folded into the fault/diag tooling — decide at
implementation).

**Demo:** quick restart with a 3 s RESTART DELAY: field resets, movie
freezes at frame 0, both start together when the countdown ends. Fault
injection shows the static-background degradation. Docs render the
feature for operators.
