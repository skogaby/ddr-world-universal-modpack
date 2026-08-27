# Progress: Background Movie Sync

Updated: 2026-08-24
Status: **FEATURE COMPLETE — validated on BOTH platforms** (maintainer
confirmed clean passes on Windows and CrossOver, 2026-08-24). Position
sync (always-on park model) + rate sync (SYNC BACKGROUND VIDEO → scaled
reference-clock proxy, platform-uniform, D14 superseded). All work
uncommitted by design (maintainer commits manually); the bemani-buddy
sibling repo carries migration 018 + passthrough, also uncommitted.
Planning docs archived to `.agents/planning/_archive/`.
NEXT ACTION: none — feature closed. See `summary.md` for the
implementation outcome and residuals (stch UnsupportedProfile backlog;
data_mods texture deploy note). Maintainer commits both repos when ready.

Resume protocol: read `implementation/plan.md` (steps + checklist),
`design/detailed-design.md` (approved 2026-08-20 + dated amendments), and
`research/movie-sync-re.md` (player offsets, vtable facts) before touching
code.

## Done

- PDD complete (register D1–D14, research, design, plan — all approved
  2026-08-20; see `summary.md`).
- Step 1 (service skeleton + probe): `src/services/movie_sync.rs` created,
  `movie_policy` Passthrough handoff, `lib.rs` init at 5a. Probe v1
  (paused-state, 2026-08-20) → deploy #1 findings; probe v2
  (running-state, wall-clock) → deploy #2 findings. **Step 1 complete:
  decision point passed (running-state seeks proven; SetRate dead on Wine,
  deferred to a Windows test; probe retained for it).**
- Step 2 (always-on position sync) — COMPLETE, cabinet-validated
  2026-08-22 (deploy #14, rev K): the unified park model (see the deploy
  log below and movie_sync.rs module docs).
- Step 3 (rate directive carriage + SetRate + ladder) — implementation
  complete 2026-08-22, all gates green (check / fmt /
  validate_movie_sync 18/18 / validate_song_playback_speed full pass /
  build.sh):
  - `src/core/platform.rs` (NEW): shared cached `running_under_wine()`;
    `mfplat_vih_fix.rs` + `ntdll_state_shim.rs` refactored onto it (local
    copies deleted).
  - `song_rate/runtime.rs`: `DESIRED_SYNC_MOVIE` per-side atomics
    (default false) + `set_desired_sync_movie`/`desired_sync_movie`;
    `sync_movie_capable()` = `movie_sync::is_available() && !Wine` (D14),
    ANDed into the per-side flags at scene-26 input gathering (so the
    latched flag is always EFFECTIVE; one latched INFO when a raw desire
    is platform-blocked); arm log gains `sync_movie=` + "movie kept for
    rate sync" wording; `movie_rate_directive() -> Option<f64>` (committed
    non-identity snapshot + latched sync ⇒ exact source/output as the
    SetRate argument); `DDR_SYNC_MOVIE_FORCE=1` dev override in `init`
    (forces both raw desires ON; capability gate still applies; REMOVE in
    Step 4).
  - `song_rate/lifecycle.rs`: `EligibilityInputs.desired_sync: [bool;2]`
    → `ArmRequest.sync_movie` (latched from the entered side) →
    `LifecycleState.sync_movie` atomic + getter; arm's tentative
    suppression becomes `percent != 100 && !sync_movie` (sync ON ⇒
    explicit clear, stale suppression flushed); identity resolution
    resets the latch to false.
  - `song_rate/transaction.rs`: the commit's movie confirmation is
    skipped when `parts.lifecycle.sync_movie()` (score
    protection/snapshot/Q31-last untouched) — placed HERE (pure,
    host-tested) instead of the windows-only `confirm_movie` closure.
  - `movie_sync.rs`: `apply_rate_directive` at capture (graph open) —
    consumes the directive, `SetRate(rate)` + `GetRate` readback, pure
    `rate_sync_verdict` (readback-within-epsilon 0.01, NEVER hr-only —
    the Wine silent-no-op lesson); failure ⇒ stop command (new
    `PLAYER_VTBL_CMD_STOP` = vtbl +0x28) + one WARN + capture dropped
    (static background, song unaffected). Module docs updated (Step 3
    section; capture now performs COM for the directive only).
  - Host tests: `sync_movie_latches_from_the_entered_side`,
    `sync_on_arm_keeps_the_movie_unsuppressed_and_identity_resets_the_latch`
    (lifecycle), `commit_with_sync_movie_latched_skips_the_movie_confirm`
    (transaction), `rate_verdict_*` ×2 (movie_sync ladder). All
    `ArmRequest`/`EligibilityInputs` literal sites updated.
  - NOT yet live-proven: actual SetRate acceptance on real Windows (no
    box available — the ladder makes either outcome safe; the probe env
    remains the acceptance test). CrossOver demo option: turn the row ON
    at a non-100% speed — expect the arm INFO naming the Wine gate and
    stock suppression (the D14 path), NOT a rate-locked movie.
- Step 4 (SYNC BACKGROUND VIDEO row + persistence) — implementation
  complete 2026-08-22, all gates green (check / fmt / build.sh; row logic
  follows the proven preserve_pitch pattern, no new host-testable pure
  code):
  - `src/mods/song_playback_speed.rs`: `sync_movie` bool child row
    registered right after `preserve_pitch` (default OFF,
    `ShowWhen::NotEquals(song_speed, 100)`, `PersistMode::Full` via
    `bool_toggle`'s default, `load_clamp_bool`, `on_change` →
    `set_desired_sync_movie`); non-fatal registration; enable-time
    re-seed from persisted values + disable-time reset (row hidden, both
    desires false); module docs updated ("two boolean child rows").
  - Step-3 `DDR_SYNC_MOVIE_FORCE` env REMOVED from `runtime::init` (the
    row is now the only writer of the desired atomics).
  - Textures: `sync_movie` label + off/on preview strings added to
    `scripts/option_strings.py` (en/ja/ko); regenerated via
    `scripts/gen_option_labels.py` — exactly 3 new PNGs per language
    (`seop_item_sync_movie`, `seop_image_sync_movie_{off,on}`), zero
    changes to existing textures.
  - bemani-buddy (external repo, uncommitted like everything else):
    migration `018_ddr_world_sync_movie.sql` (nullable
    `opt_mod_sync_movie`, migration-014/017 shape), `mod_sync_movie`
    verbatim wire passthrough (playdata_3.rs both structs, db
    model/mysql row-map + UPDATE, playdata.rs load echo + save parse +
    all-none scaffolds), 5 new handler tests (present/absent/malformed/
    none-skip/echo — all pass). Migration applied to the local dev DB +
    `cargo sqlx prepare --workspace` regenerated the offline cache;
    `cargo build` + `cargo test` green. (Repo-wide `cargo fmt --check`
    drift and clippy warnings there are PRE-EXISTING toolchain skew —
    untouched, my edits mirror surrounding style.)
  - NOT yet live-proven: row appears/hides at non-100%, card-out/card-in
    round-trip of `mod_sync_movie` (needs the cabinet + server).
- Step 5 (polish + docs) — implementation complete 2026-08-22, all gates
  green (check / fmt / validate_movie_sync 18/18 / build.sh):
  - FR-7 (delayed restart): NO new code — the plan's original
    song_reset pause/run integration is SUPERSEDED by the §1b park model
    (design amendment 2026-08-22): a delayed reset's negative live count
    parks the movie at 0 (frozen first frame through the countdown) and
    runs at the 0-crossing. Cabinet confirm is in the checklist below.
  - Permanent `DDR_MOVIE_SYNC_FAULT` env (movie_sync.rs, gated on
    `layeredfs.developer_mode` — the DDR_SONG_RATE_FAULT policy):
    `set-rate` corrupts the readback so every rate directive takes the
    stop rung; `seek` marks every capture non-seekable (status-quo-desync
    rung). Unknown values / non-dev-mode warn and select nothing.
  - Probe decision: `DDR_MOVIE_SYNC_PROBE` KEPT (module docs updated) —
    it is the future Windows-cabinet SetRate acceptance test.
  - Docs: AGENTS.md Key Entry Points row (Background Movie Sync — full
    architecture digest + the vtbl +0x48/+0x58 correction note) + README
    feature-table row (built-in position sync + the SYNC BACKGROUND
    VIDEO sub-option). AGENTS.md carried no stale movie-vtable labels
    needing correction (they live only in the research docs, already
    corrected).

## Step 5 cabinet checklist (for the maintainer)

CrossOver, builtin quartz, fallback movie mode + `-audiohookdisable`,
NO env vars. Movie-backed songs:

1. **Regression sweep at 100%** (Step 2 revalidation after the Step 3–5
   diffs): plain song / scrubs+mash / loop / quick restart — all in sync,
   same as deploy #14. Log: the familiar `movie_sync: park ->` lines.
2. **FR-7 delayed restart**: set `quick_restart.restart_delay_ms` (e.g.
   3000), press 1 mid-song: the movie must freeze on its FIRST frame
   through the countdown and start exactly with the song. Log:
   `park -> 0 ms` → `parked at 0 ms (paused...)` → `crossing reached`.
3. **Wine rate gate (D14 path)**: set SONG SPEED ≠ 100% and turn SYNC
   BACKGROUND VIDEO ON: the row must appear/hide with the speed value,
   and the song must play with the movie SUPPRESSED exactly as today.
   Log: one `song_rate: SYNC BACKGROUND VIDEO desired but unavailable on
   this platform (Wine...)` INFO + the arm line showing
   `sync_movie=false`.
4. **Persistence round-trip**: toggle ON, card out, card in (bemani-buddy
   with migration 018 applied): the row must come back ON. Server side:
   `opt_mod_sync_movie` = 1 in ddr_world_profiles.
5. **Fault ladder** (dev mode: `layeredfs.developer_mode` true +
   `DDR_MOVIE_SYNC_FAULT=seek`): movies play but never sync (one
   `FAULT seek` WARN per song); scrubs desync exactly like stock. Then
   unset. (`set-rate` is only reachable on real Windows — skip on
   CrossOver.)
6. **Suppress-mode inertness**: `non_native_os_support.movie_mode` =
   `"suppress"` boot: zero `movie_sync:` capture lines (no graph ever
   builds).

Windows box (whenever available): `DDR_MOVIE_SYNC_PROBE=1` on a
movie-backed song → judge `SetRate(1.5)` by the readback line + visible
speedup; then SONG SPEED 50%/150% with SYNC ON (both pitch modes) → movie
rate-locked; `DDR_MOVIE_SYNC_FAULT=set-rate` → static background, song
unaffected, one WARN.
- Known Step 2 limitation (by design, Step 5 verifies): a DELAYED quick
  restart parks at 0 through the countdown via the park model (rev G+);
  confirm on cabinet in Step 5.

## In flight

- Nothing uncommitted beyond the Step 1 diff (maintainer commits manually).

## Deploy & test log

- 2026-08-24 — **CROSSOVER trial #2 (adaptive swap): WINE RATE SYNC
  WORKS.** All rates rate-locked (`rate sync engaged` at 0.5000 and
  1.7500 across four songs), the consumption diagnostic proves the graph
  drinks from the proxy (27–102 GetTime calls + matching advises per
  song), no wedges, and the maintainer reports the video stayed
  perfectly in sync **even through off-rate scrubs** (position sync
  composing with rate sync under Wine). The in-place SetSyncSource path
  is the one Wine takes — the stop/re-pause dance was the trial-#1
  wedge, exactly as diagnosed.
  Separately explained: the SYNC BACKGROUND VIDEO row was invisible
  in-game on this install — the row registers and injects fine
  (`injected Enum row for "sync_movie"`), but the label texture is
  missing (`get_bitmap_info[seop_item_sync_movie] can not find`): the
  9 generated PNGs under `data_mods/custom_options/
  select_music_option_lang_{eng,jpn,kor}_v3_ifs/tex/` were never copied
  (deploy.sh ships the DLL only). Fix = copy the data_mods files; the
  ON value that drove the trials came from the persisted profile value
  set on the Windows box.
- 2026-08-24 — **Rev built (Wine support folded in — D14 SUPERSEDED,
  trial env REMOVED):** `sync_movie_capable()` is now
  `movie_sync::is_available()` alone (platform-uniform);
  `DDR_MOVIE_SYNC_WINE_TRIAL` deleted from the code outright (it served
  one trial; the env name survives only in this log as history);
  all D14/Windows-only references updated across
  movie_sync/lifecycle/song_playback_speed docs; design FR-3 gained the
  2026-08-24 supersession amendment + the decision matrix now has
  Wine-fallback (rate-locked) and Wine-suppress (absent) rows;
  README/AGENTS platform wording updated (AGENTS also records the
  in-place-vs-escalated install rule and the data_mods deploy note).
  Gates: check / fmt / 20/20 harness / build.sh clean.

- 2026-08-24 — **CROSSOVER trial #1 (175% + 100%, trial env): install
  VERIFIED on Wine, then the graph WEDGED — never reached RUNNING.**
  Log: trial WARN at boot → 175% song: `rate sync engaged — graph clock
  scaled to 1.7500 (proxy verified by GetSyncSource readback)` but NO
  consumption INFO for the whole song (the diagnostic requires
  state==RUNNING) and the maintainer saw no movie; the 100% song was
  unaffected (position sync fine). Diagnosis: Wine's SetSyncSource
  plumbing works, but the STOP → RE-PAUSE dance (which exists only for
  Windows' VFW_E_NOT_STOPPED rule) wedged Wine's async cue/stop state
  machine — the same class of unexercised-paused-state fragility as
  probe v1. Song unaffected throughout (fail-open held).
- 2026-08-24 — **Rev built (adaptive swap + wedge watchdog):**
  (a) `rate_clock::install` now tries `SetSyncSource` IN PLACE first
  (Wine's FGM allows a paused-graph swap — the state machine is never
  touched) and escalates to stop → swap → re-pause ONLY on
  `VFW_E_NOT_STOPPED` (Windows — the escalated path is byte-identical to
  the sequence test #5 validated; the only Windows delta is one extra
  refused SetSyncSource call). The not-verified WARN now reports
  `escalated=`.
  (b) Wedge watchdog: a verified install whose player never reaches
  RUNNING within 15 s now logs one WARN naming the wedge (trial #1's
  failure mode was only inferable from a MISSING line).
  Gates: check / fmt / 20/20 harness / build.sh clean.
  **Awaiting CrossOver trial #2** (same procedure) — and afterwards ONE
  Windows re-verification song (the adaptive path must still escalate
  there; expect the same `rate sync engaged` + consumption INFO).

- 2026-08-24 — **Rev built (Wine rate-sync trial, D14 opt-in override +
  consumption diagnostic):**
  (a) `DDR_MOVIE_SYNC_WINE_TRIAL=1` (latched at movie_sync init, WARN
  when active under Wine): relaxes the Wine leg of
  `runtime::sync_movie_capable()` — the clock-proxy mechanism no longer
  depends on the SetRate call whose Wine silent-no-op motivated D14.
  Without the env, Wine behavior is UNCHANGED (D14 suppression; the
  blocked-desire INFO now names the trial env). Requires
  `non_native_os_support.movie_mode="fallback"` (suppress mode never
  builds movie graphs) — the boot WARN says so.
  (b) Clock-proxy CONSUMPTION diagnostic (all platforms, one INFO per
  engaged song): `rate_clock` counts GetTime/advise calls (reset per
  install); ~2 s after the game's Run the frame driver logs them.
  Near-zero counts with a verified install = the renderer paces by
  another clock — the discriminator for a "verified but movie ignores
  the rate" outcome without another RE round-trip.
  Gates: check / fmt / 20/20 harness / build.sh clean.
  **Awaiting CrossOver trial run** (instructions below).

### Wine trial run (for the maintainer, CrossOver bottle)

Fallback movie mode + `-audiohookdisable` (the run_ddr default), native
WM runtime bottle, `DDR_MOVIE_SYNC_WINE_TRIAL=1` exported into the game
environment. Boot: expect the `WINE RATE-SYNC TRIAL ACTIVE` WARN. Then a
movie-backed song at 175% and one at 50–80% with SYNC BACKGROUND VIDEO
ON:
1. Log check per song: `sync_movie=true — movie kept for rate sync` →
   `rate sync engaged — graph clock scaled to …` → the consumption INFO
   (`clock proxy consumption at Run+2s — N GetTime call(s), M advise(s)`).
2. Visual: movie rate-locked to the audio (the win); or degraded static
   background (install refused — WARN names the hr); or movie at 1.0× /
   stalled (the bad case — check the consumption counts and copy the log
   back).
3. Also one 100% song: position sync must still behave (regression).
Worst realistic failure = a desynced/stalled movie on trial songs only;
unset the env to restore stock D14 behavior instantly.

- 2026-08-24 — **WINDOWS cabinet test #5 (clock proxy v2): RATE SYNC
  WORKS — the feature's Windows rate leg is DONE.** Three rate-synced
  songs, all engaged and verified: 80% (`graph clock scaled to 0.8000`),
  175% (1.7500), 50% (0.5000) — maintainer confirms slow AND fast rates
  visibly rate-lock the background video to the audio. Zero movie_sync
  WARNs. **Bonus (unplanned coverage): position sync composes with rate
  sync live** — the 50% song's log shows training scrubs parking/crossing
  on a rate-locked movie (parks at 47s→94s, restart latencies 0–150 ms on
  Windows — the park model's "self-neutralizes on fast machines" claim
  measured for real). The stop→swap→re-pause install introduced no
  observed start-up stutter.
- 2026-08-24 — **WINDOWS cabinet test #4 (175% + 80%, clock proxy v1):
  install REFUSED — `SetSyncSource` returned VFW_E_NOT_STOPPED
  (0x80040224).** The FGM enforces the documented "change the clock only
  while STOPPED" rule as a hard error, and BuildGraph leaves the graph
  PAUSED. Positives: the pointer-readback verification caught it exactly
  as designed, and the pre-Run degradation showed the correct static
  no-movie background (not black, not frozen) at both rates.
- 2026-08-24 — **Rev built (clock proxy v2: stop → swap → re-pause):**
  `rate_clock::install` now briefly STOPS the freshly built graph
  (`IMediaControl::Stop`, direct — pre-Run, single COM thread, the game
  has not seen the build result yet), performs the `SetSyncSource` swap,
  readback-verifies, and re-PAUSES to hand the game back exactly what
  BuildGraph left (Stop does not reset the position — still cued at 0
  from BuildGraph's own seek; the re-Pause re-cues frame 0 like the
  stock epilogue). Failed stop ⇒ bail to the ladder untouched; failed
  re-pause ⇒ WARN + continue (the game's own Run also starts a stopped
  graph). Gates: check / fmt / 20/20 harness / build.sh clean.
  **Awaiting Windows test #5:** same checklist — expect
  `rate sync engaged — graph clock scaled to …` + the movie visibly
  rate-locked; watch for start-up stutter (the stop/pause cycle
  re-priming the WM ASF Reader) and A/V rate mismatch.

- 2026-08-23 — **WINDOWS cabinet test #3 (probe graph dump): refuser
  pinned, clock present, GO for the proxy.** Graph = the GAME'S OWN custom
  video renderer ('0001', clsid {8313E2A7-9F46-4AD0-9E60-7882BE265214} —
  the filter feeding the frame-deposit slot) + WMVideo Decoder DMO + WM
  ASF Reader ({187463A0-…}, no filter-level IMediaSeeking). The only
  seeking-capable filter answers caps=0x0037 (== the graph caps) and
  E_INVALIDARGs SetRate — the chain refusal is structural and
  game-invariant. Graph sync source present (hr=0, non-null). Probe's
  running-state SetRate(1.5) at +5s: E_INVALIDARG (closes the last
  SetRate loophole). Maintainer approved the clock-proxy replacement.
- 2026-08-23 — **Rev built (scaled reference-clock proxy — the rate
  mechanism, replacing SetRate outright):**
  - `movie_sync::rate_clock` (new module): single-instance
    `IReferenceClock` COM object (static vtable, cosmetic refcount).
    `GetTime = T0 + (inner − t0)·r` with a fetch_max monotonic floor and
    basis continuity `T0 = t0` at install; `AdviseTime`/`AdvisePeriodic`
    forward to the WRAPPED clock with inverse-mapped deadlines
    (`t0 + (X − T0)/r`) and scaled periods (floor 1 tick); `Unadvise`
    forwards the inner cookie verbatim — the inner clock does all real
    waiting/signaling, the proxy owns no threads. Installed at capture
    (graph still paused) via `IMediaFilter::SetSyncSource`, verified by
    `GetSyncSource` POINTER READBACK. The AddRef'd inner clock is held
    until the NEXT install (graph threads may call GetTime between scene
    change and teardown — releasing at capture drop would race).
  - Pure host-tested time maps: `scale_clock_time` /
    `map_advise_deadline` / `map_advise_period` (+4 tests: identity,
    fast/slow scaling, roundtrip within rounding, period floor,
    nonpositive-rate guards). 20/20 harness tests.
  - SetRate machinery DELETED (two-stage, pending-rate drain, boot
    latch, `rate_sync_verdict`/`RateVerdict` + their tests): dead on a
    chain that ships inside the game itself, i.e. every cabinet. The
    probe's per-filter SetRate diagnostics remain (dev-only evidence).
  - All rate failures now degrade PRE-RUN (install happens at capture):
    the frozen-first-frame window is gone by construction; failure shows
    the clean static background.
  - `DDR_MOVIE_SYNC_FAULT=set-rate` now forces the clock install to be
    treated as failed (env value name kept; exercises the same rung).
  - Docs: design FR-3 second amendment (2026-08-23, maintainer-approved
    mechanism replacement), module docs rewritten, AGENTS.md row updated.
  - Gates: check / fmt / 20/20 harness / build.sh clean. **Awaiting
    Windows test #4:** 175% + SYNC ON expect
    `movie_sync: rate sync engaged — graph clock scaled to 1.7500 (proxy
    verified by GetSyncSource readback)` + the movie visibly playing
    fast, in lockstep with the audio; also one slow song (e.g. 75%).
    Watch for: A/V rate mismatch (movie fast/slow but not exactly the
    song's rate), stutter at song start, or EC_COMPLETE oddities near
    the movie's end. If the proxy install verifies but the movie still
    plays at 1.0×, the game's renderer paces by something other than
    the graph clock — report, and plan B is the renderer RE.

- 2026-08-23 — **WINDOWS cabinet test #2 (0.75 / 1.2 / 1.75, two-stage
  build): SetRate REJECTED at every rate in BOTH states — the DirectShow
  rate-change mechanism is dead on this chain.** Log: each song shows
  `SetRate(x) not verified (hr_set=0x80070057 [E_INVALIDARG])` at graph
  open AND at the first running frame, then the degradation. Conclusions:
  (a) not a state problem, not a range problem — the filter chain (prime
  suspect: the WM ASF Reader source, which historically does not support
  `IMediaSeeking::SetRate != 1.0`) categorically refuses; (b) the frozen
  FIRST FRAME the maintainer saw is the predicted post-Run degradation
  cosmetic: by stage 2 one frame has presented, the movie actor latches
  movie mode, and `opened=0` freezes the plane on that frame instead of
  restoring the static background.
- 2026-08-23 — **Rev built (boot latch + probe graph dump):**
  (a) `RATE_UNSUPPORTED_THIS_BOOT`: a failed running-state retry latches
  "chain refuses rates" — subsequent songs degrade at STAGE 1 (pre-Run:
  the actor never latches movie mode ⇒ clean static background; only the
  boot's FIRST rate-synced song can show the frozen-frame cosmetic).
  (b) `DDR_MOVIE_SYNC_PROBE` now also dumps the live graph at capture:
  every filter's name + CLSID, the graph sync source, per-filter
  `GetCapabilities` and an isolated `SetRate(1.5)`→readback→restore probe
  — pinpoints the refusing filter from one log. Gates: check / fmt /
  18/18 harness / build.sh clean. **Awaiting Windows probe run**
  (`DDR_MOVIE_SYNC_PROBE=1`, one movie-backed song at 100%).
- **DECIDED (maintainer, 2026-08-23): rate mechanism replacement.** With
  SetRate dead on Windows WMV graphs, the replacement is the SCALED
  REFERENCE CLOCK proxy (approved and implemented — see the rev above;
  design FR-3 amended).

- 2026-08-23 — **WINDOWS cabinet test #1 (175% + SYNC ON): pipeline
  end-to-end CORRECT, SetRate acceptance FAILED (paused state), stop rung
  showed BLACK instead of the static background.** Log (`./log.txt`):
  arm `sync_movie=true — movie kept for rate sync` → commit 175% → real
  graph built (`captured ... seekable=true`) → `rate sync FAILED
  (hr_set=0x80070057 [E_INVALIDARG], hr_get=0, readback 1.0000 vs 1.7500)`
  → stop rung → user saw a BLACK plane all song. Two findings:
  (a) **Windows quartz rejects SetRate on the freshly built, still-PAUSED
  graph** — the one state this engine otherwise avoids for COM. The stock
  game never calls SetRate, so this was unexercised territory; whether the
  RUNNING state accepts it is still open (the probe applies SetRate while
  running and was not active this test).
  (b) **The stop rung's visual was wrong**: stopping a REALLY-built graph
  leaves `opened=1`, so the game keeps drawing a video plane that never
  receives a frame — black; the suppress path's static background comes
  from `opened=0` (get-frame early-return, plane never drawn).
- 2026-08-23 — **Rev built (two-stage SetRate + suppressed-equivalent
  degradation):** (a) stage 1 = paused attempt at graph open (zero-frame
  window if it works); refused ⇒ pending rate retried ONCE at the first
  RUNNING frame by the frame driver (the standard DirectShow rate-change
  state; sub-frame 1.0× window, invisible); both refused ⇒ degrade.
  (b) `degrade_to_suppressed`: direct `IMediaControl::Stop` (player+0x50,
  vtbl +0x48 — race-free: single COM thread, and once `opened` drops the
  game never dispatches the command byte again) + `opened=0` → the exact
  `fake_opened` shape (state 3, opened 0) the suppress/fallback paths
  prove out → static background; teardown releases the COM pointers
  unconditionally (RE §6), nothing leaks. Fault injection moved into
  `try_set_rate` (both stages exercise it). RE doc §2 updated with the
  E_INVALIDARG finding; module docs + AGENTS.md row updated. Gates:
  check / fmt / 18/18 harness / build.sh clean. **Awaiting Windows
  retest:** at 175% + SYNC ON expect either
  `rate sync applied at first running frame — SetRate(1.7500) verified`
  (movie plays fast, in sync — the win) or
  `rate sync FAILED in both states` + the game's normal no-movie static
  background (NOT black). Also worth one 50% song: if 0.5 verifies where
  1.75 fails, the chain has a rate-range limit.

- 2026-08-22 — **Step 2 deploy #14 (rev K, CrossOver): EVERYTHING PERFECT
  — STEP 2 COMPLETE.** Repeat plays of the same song, scrubs + mashes on
  both plays, loops, quick restarts, plain songs: all in sync. The final
  architecture that got there (14 cabinet deploys):
  - ONE mechanism (the park) for every timeline alteration: seek video to
    destination → pause when its frame arrives (frame-deposit slot
    `*(player+0x78)+0x250`) → run when the live music count crosses the
    destination. Approach events park at their trigger; scrubs park at
    `live + Δ` (Δ = 2× per-capture measured restart latency, 500 ms fresh
    default; Δ sizes only the brief post-scrub freeze, never sync).
  - Pure classification: `drain_action` (Hold/Jump/Park) + `is_advancing`
    stability gate + `map_position` + `jump_delta` + `lead_update` — all
    host-tested (16 tests, scripts/validate_movie_sync.sh).
  - NO cross-song mutable state in the sync path (first-playthrough
    semantics, maintainer-directed); no hardcoded sync offsets anywhere.
  - Seeks only while RUNNING via the game's own native seek (vtbl +0x58);
    pause/run via the game's command byte; COM all on the game thread.

- 2026-08-22 — **Step 2 deploy #13 (rev J, CrossOver).** Same song twice
  in a row: first playthrough perfect (scrubs + mashes); second
  playthrough mostly desynced. Maintainer call: fresh state demonstrably
  outperforms carried state — clear it between songs, treat every song as
  a first playthrough.
- 2026-08-22 — **Rev K built (first-playthrough semantics):**
  (a) run-startup pre-issue DELETED entirely (PARK_MEASURE_RUN stage,
  PARK_RUN_LEAD, startup_update): its measured benefit (~70–160 ms
  lateness, imperceptible in live testing) never justified its failure
  mode (early video from stall-polluted estimates — the dominant desync
  source in tests #12–#13); the Run now fires at the crossing, period.
  (b) The per-direction restart estimates (Δ sizing only) reset per
  capture — every song starts at the proven-perfect fresh defaults
  (Δ = 500 ms), within-song measurements only resize that song's
  freezes. NO cross-song mutable state remains in the sync path.
  (Cleanup note: a python edit duplicated park_cancel/park_tick
  mid-refactor — caught by cargo check, stale copies deleted.)
  16/16 host tests; check/fmt/build clean. **Awaiting cabinet test #14.**

- 2026-08-22 15:01 — **Step 2 deploy #12 (rev I, CrossOver).** First song:
  PERFECT through scrub mashing. Retest of the same song after two other
  songs: most scrubs slightly off. Log caught the poisoning in the act:
  the run-startup estimate — which PRE-ISSUES the Run and therefore
  shifts sync directly — absorbed stall artifacts (`park run startup
  measured 2567 ms (estimate 703 -> 1635)`) and the next park ran the
  video 1.6 s early; legitimate machine readings are 71–161 ms.
- 2026-08-22 — **Rev J built (startup outlier rejection):** pure
  `startup_update` — readings above the mechanism-derived plausibility
  bound (250 ms: ≤3 frame-dispatches + renderer resume; a physical bound
  of the mechanism, not a sync tune) are REJECTED as stall artifacts
  (estimate kept, rejection logged); sane readings adopt-then-average,
  clamped. 17/17 host tests. **Awaiting cabinet test #13.**

- 2026-08-22 — **Step 2 deploy #11 (rev H, CrossOver).** Loops + quick
  restarts: in sync (park + run-startup lead confirmed). Plain song: in
  sync. Scrubs/mash: in sync MOST of the time, non-deterministically off
  otherwise — per-seek restart variance (log: fwd R measured 526 vs
  1126 ms on consecutive scrubs) that no average lead can cover.
- 2026-08-22 — **Rev I built (unified park model):** jumps now EXECUTE as
  parks at `live + Δ` (`jump_delta` = 2× the per-direction measured
  restart estimate, one-time 500 ms default, clamp 2 s): the audio
  catches up to the parked frame instead of the video chasing a moving
  target. Δ sizes only the brief post-scrub freeze — sync is governed by
  the park crossing + run-startup lead — and Δ→0 degrades the park to a
  plain seek on instant-seek machines (real Windows unaffected). R
  measurement moved INTO the park deposit-wait (every park feeds the
  estimates); the entire jump/correction machinery DELETED (−161 lines:
  fire_seek, measure_tick, corrections). ONE mechanism for every
  scenario. 16/16 host tests; check/fmt/build clean. **Awaiting cabinet
  test #12.**

- 2026-08-22 01:31 — **Step 2 deploy #10 (rev G, CrossOver).** Scrubs/mash:
  basically in sync (park/jump split works; log confirms loops WERE
  poisoning the estimates — one loop measurement had bumped bwd 272→814
  before the split). Parks execute their full sequence (park → paused →
  crossing → running). Two residuals:
  (a) loops/restarts "pretty close, slightly noticeably off" — the park
  path is late-only: crossing detection (count updates once per game
  frame) + command dispatch (get-frame next frame) + renderer resume;
  (b) **stch (mcode 38172, 86 MB movie): scrubs unavailable — NOT a
  movie_sync issue**: `song_rate: bind refused (UnsupportedProfile)` →
  `TrainingMode: no live song-rate binding -- scrub unavailable this
  song`. The AUDIO bank fails the streaming preflight; training scrubs
  need the identity binding. Pre-existing limitation, out of scope here —
  follow-up: inspect stch's XWB header for the profile assumption it
  breaks. (Movie-side: capture fine/seekable; its restart parks hit the
  3 s deposit timeout → designed no-pause degradation.)
- 2026-08-22 — **Rev H built (park run-startup lead):** parks measure
  run→first-deposit (the startup latency) into a process-global
  runtime-learned `PARK_RUN_LEAD_MS` and pre-issue the Run command by it,
  so the actual resume lands ON the crossing. Same self-neutralizing
  principle as the jump leads; no constants. **Awaiting cabinet test #11.**

Follow-up backlog (out of feature scope):
- stch / UnsupportedProfile: song_rate bind preflight rejects stch's XWB
  (86 MB-movie song; audio bank format quirk) — scrubs/rate unavailable
  on such songs. Investigate the profile check against stch's header.

- 2026-08-22 — **Step 2 deploy #9 (rev F, CrossOver).** FF/RW mash issues
  mostly resolved; loops and quick restarts now land the video 200–300 ms
  AHEAD. Root cause: loops/restarts seek into cached just-played content
  (near-instant restart) but share the learned bwd estimate with RW
  scrubs (slow fresh-demux restarts) — the shared state oscillates
  between scenario classes; maintainer flagged the accumulating
  heuristics and asked for a conceptually simpler "audio altered → seek
  accordingly" model.
- 2026-08-22 — **Rev G built (two-path drain: Jump vs Park):**
  `drain_action(queued, live, advancing, elapsed)` classifies every event:
  **Jump** (scrubs — audio continues: compensated seek, rev F machinery
  unchanged) vs **Park** (approach events — loop wrap, quick/delayed
  restart, SONG START: live count ≥500 ms below trigger or negative):
  seek to the destination with NO lead, PAUSE via the game's command byte
  when the frame-deposit arrives, RUN at the live count's crossing —
  deterministic, zero prediction, never touches the jump estimates. A
  uniform `is_advancing` stability gate (two samples ≥20 ms apart at wall
  rate) replaces the transient special-cases. Parks deliver FR-7
  (delayed restart) for free. park_cancel un-freezes on any newer event
  (ordered BEFORE the RUNNING gate — a paused park would deadlock the
  drain). Design amended (§1b). 16/16 host tests rewritten around
  drain_action/is_advancing. check/fmt/build clean. **Awaiting cabinet
  test #10.**

- 2026-08-22 — **Step 2 deploy #8 (rev E, CrossOver).** "Significantly
  improved, almost there": after rapid FF the audio leads *ever so
  slightly* (video a touch behind); after rapid RW the video leads by
  ~150–200 ms. Diagnosis: restart latency is DIRECTION-ASYMMETRIC —
  backward targets sit in already-read/cached content and restart faster
  than forward targets (fresh demux+decode), so one mixed estimate
  over-leads RW and slightly under-leads FF; the RW residual sat at the
  old 150 ms correction boundary and went uncorrected.
- 2026-08-22 — **Rev F built (direction-split estimates):**
  (a) `LEAD_FORWARD_MS` / `LEAD_BACKWARD_MS`, classified per seek by
  `is_backward_seek(target, expected_movie)` where expected = last seek's
  content target + wall time since (per-capture expectation, reset on
  drop); measurements update the matching estimate; (b) estimates are now
  PROCESS-GLOBAL (machine pipeline property — new songs start
  pre-calibrated instead of landing their first seek uncompensated; still
  100 % runtime-measured); (c) correction threshold 150 → 100 ms (gates
  correction-worthiness only). Log lines now carry fwd/bwd. 16/16 host
  tests (direction + threshold cases added); check/fmt/build clean.
  **Awaiting cabinet test #9.**

- 2026-08-22 00:00 — **Step 2 deploy #7 (rev D, CrossOver).** ALL seek
  scenarios (single scrub, mash, loop, pre-set bounds, restart) now land
  audio 200–400 ms AHEAD of video; only a plain no-marker start is
  perfectly in sync. Log analysis: rev D's clock-readback verification
  measured "InSync" after every scrub (no correction lines) while the eye
  saw the lag — **Wine's GetCurrentPosition tracks the graph clock, not
  presentation; it is useless for sync measurement.** The real mechanism:
  a RUNNING seek restarts the pipeline (flush → reposition → decode to
  target) while the game keeps playing; frames resume exactly that
  restart-time behind. Maintainer directive: NO hardcoded offsets —
  algorithmic, machine-independent only.
- 2026-08-22 — **Rev E built (restart-latency compensation):** rev D's
  readback verification REMOVED. Every seek now (a) leads its target by
  the per-capture restart-latency estimate (starts 0), (b) measures the
  actual restart via the streaming thread's frame-deposit slot
  (`*(player+0x78)+0x250`, RE-verified: written per frame by the
  streaming thread, atomically taken by get-frame; deposits stop during
  the restart — seek→first-deposit IS the latency, presentation-accurate),
  (c) folds it into the estimate (adopt-then-average `lead_update`, clamp
  0..2000 ms), and (d) fires at most ONE corrective re-seek per event
  chain when the lead was off ≥150 ms (`needs_correction`; corrective
  seeks are measured but never correct again). Stale-deposit handling via
  a seek-time slot snapshot; 3 s measurement timeout keeps the last
  estimate. On real Windows the latency measures ~0 and the whole
  mechanism self-neutralizes — nothing machine-specific. Convergence:
  first seek of a capture lands behind once and corrects ~R later;
  every subsequent seek (loop wraps!) is pre-compensated. 16/16 host
  tests (lead_update/needs_correction replace the verify suite);
  check/fmt/build clean. **Awaiting cabinet test #8.**

- 2026-08-21 — **Step 2 deploy #6 (rev C, CrossOver).** Plain song +
  moderate scrubs: IN SYNC. Two residuals: (a) MASHED scrubs end
  desynced (one follow-up scrub re-syncs) — root cause: scrub steps are
  exactly 5000 ms and SETTLE_WINDOW_MS was ±5000 INCLUSIVE, so mid-mash
  the drain accepted the STALE pre-scrub count (one step from the new
  trigger) and seeked to the old position; (b) loop wraps land 0.2–0.5 s
  off (no longer restarting from 0) — Wine seek-execution bias (flush
  latency + keyframe snap), same magnitude invisible on a 5 s scrub jump
  but glaring on a repeated loop section.
- 2026-08-21 — **Rev D built:** (a) SETTLE_WINDOW_MS 5000 → 3000 (> the
  2.5 s approach lead, < one scrub step; direction-flips at 10 s also
  held); (b) one-shot POST-SEEK VERIFICATION: 300/400 ms after each fired
  seek, sample the movie position twice (the double-sample is the
  aliveness probe — Wine echo readbacks disable corrections for the rest
  of the capture), measure error vs the live count, and issue AT MOST ONE
  corrective seek to `live − error` (pre-compensating the measured bias;
  threshold 150 ms, sanity cap 10 s; never re-armed by its own
  correction; superseded by any new pending seek). NOT continual drift
  correction — one bounded correction per seek event, right after an
  already-visible jump; also auto-repairs the mash tail. Pure
  `verify_decision` + 5 new host tests (16 total). check/fmt/harness/
  build clean. **Awaiting cabinet test #7.**

- 2026-08-21 12:56 — **Step 2 deploy #5 (rev B, CrossOver).** Plain full
  song: IN SYNC (test-#3 regression FIXED). But every scrub and loop wrap
  restarted the video from its beginning. Log (new `live (queued ...)`
  format made it unambiguous): `seek -> 0 ms live (queued 44670 ms, ...)`
  on every event — the queued triggers were all correct, but the ~2 kHz
  drain fires sub-frame after the notification and
  `current_raw_music_count()` transiently reads **~0** during the reset
  transaction (the re-anchor passes through the song-start protocol's
  playhead-0 state before the target adjust lands). Rev B traded the lead
  bug for a transient bug.
- 2026-08-21 — **Rev C built (settle rule):** pure
  `settle_decision(queued, live, elapsed)` gate on the drain — Hold while
  live is negative (pre-anchor approach) or diverges from the queued
  trigger by > 5 s (SETTLE_WINDOW_MS, covers the 2.5 s approach lead)
  within a 500 ms timeout; Seek(live) once settled or timed out;
  Seek(queued) only when live is unreadable past the timeout. 5 new host
  tests (11 total). check/fmt/harness/build clean. **Awaiting cabinet
  test #6** (same checklist: plain song, SONG START, loop wraps, scrubs,
  quick restart).

- 2026-08-21 (post-experiment) — **D14 recorded (maintainer decision):
  NO drift correction** ("extremely visually stuttery"). Wine: SONG SPEED
  ≠ 100 % suppresses the movie regardless of the SYNC BACKGROUND VIDEO
  toggle; scrub/seek position sync stays 100 %-only there. Real Windows:
  actual SetRate (readback-verified). Register D14, design FR-3/matrix/
  Components §4 + Appendix B amended, plan Step 3 gated on a shared
  `running_under_wine()`. `ntdll_state_shim` de-armed (install call
  removed from non_native_os_support; module retained as the proven
  LdrRegisterDllNotification pre-DllMain IAT-patch pattern). Build clean.

- 2026-08-21 12:43 — **Native-quartz boot #2: shim v2 WORKED, native quartz
  FATAL — experiment ABANDONED, bottle reverted.** Log sequence: shim
  registered at boot → first movie open: `quartz.dll IAT patched
  (RtlGetPersistedStateLocation -> shim) pre-DllMain` (v2 mechanism
  proven) → BuildGraph NEVER RETURNED — no capture, no failure hr; last
  activity is three fresh WineD3D fake windows at the freeze → black
  screen, hard lock, force kill. Signature: native quartz's intelligent
  connect instantiating a default Video Renderer/VMR (d3d via wined3d),
  deadlocking against the game's live device on the update thread —
  inside quartz x wined3d, beyond safe in-process fixing. Bottle reverted
  same session (builtin files restored, overrides deleted, verified).
  Docs: native_wm_runtime_bottle_setup.md §2.9 rewritten as an ABANDONED
  EXPERIMENT record. The ntdll_state_shim STAYS (Wine-gated, quartz-only,
  fail-open, inert on stock bottles — and the LdrRegisterDllNotification
  IAT-patch pattern is now proven for future native-DLL needs).
  **Consequence: Wine rate sync = seek-based drift correction (to be
  designed into Step 3's ladder); real SetRate = Windows cabinet only,
  pending its probe.**

- 2026-08-21 12:30 — **Native-quartz boot #1: shim v1 FAILED (premise
  wrong).** Log: `ntdll_state_shim: RtlGetPersistedStateLocation export
  missing` at boot, then `movie_policy: graph build failed (hr=0xc0260001)
  -- faked opened, no movie this song` — no movies at all (fallback
  degraded as designed, no crash). Root cause: Wine declares the function
  `@ stub` — it has NO export (verified by parsing CrossOver ntdll's
  export table: ABSENT among 1463 named exports); the abort thunk is
  loader-SYNTHESIZED into quartz's IAT at import-snap, so there is nothing
  for GetProcAddress/retour to detour. **Shim v2 (built):**
  `LdrRegisterDllNotification` (EXPORTED — verified) LOADED callback fires
  post-snap/pre-DllMain → walks quartz's ntdll import descriptor → patches
  the one IAT slot to a local "no persisted state" implementation
  (STATUS_OBJECT_NAME_NOT_FOUND, out-length 0). quartz-only, Wine-gated,
  fail-open (registration/patch failure = today's no-movie degradation).
  Escalation if Wine's notification ordering ever proves post-DllMain:
  ntdll export-table augmentation (not built until needed).

- 2026-08-21 10:37–10:42 — **Step 2 deploy #3 (CrossOver, no probe).**
  Maintainer observations + log correlation found three bugs, one root
  cause each:
  - (1) Plain song: movie FROZEN on frame 0, scrubs can't revive. Log:
    `capture-time sync queued (454647 ms)` → `seek -> ... target 141300 ms`
    = the movie's END. Root cause: at capture the run is NOT yet anchored,
    so `current_raw_music_count()` returns session-wall-clock garbage
    (grows ~1:1 with wall time across songs: 454647/516667/603883) that
    passes the ±60s..60min sanity range. The end-seek fires EC_COMPLETE →
    stock non-loop STOP → unrecoverable (drain requires RUNNING). Songs
    with bounds escaped only because the start-adjust's rescue seek won
    the race the same second.
  - (2) SONG START=60s: movie led the audio by ~2.5 s until the first
    scrub. Log: `run adjusted in place -- t_q 59999 ms, lead 2499 ms` —
    the notification carries the DESTINATION; the run starts a silent
    approach 2.5 s below it. Seeking to the destination = movie early by
    exactly the lead. Scrubs were perfect because their transaction has
    no lead.
  - (3) Loop: same lead offset re-introduced at every wrap
    (`loop iteration ... reset to 114999` with the same approach shape).
  - **Rev B fixes (2026-08-21):** (a) drain seeks to the LIVE music count
    (queued value = trigger + fallback only) — locks through approach
    leads; negative live counts hold the drain until the 0-crossing;
    (b) capture-time sync gated on `song_reset::first_anchored_frame()`
    (late-build case only — kills the garbage queue); (c) `map_position`
    non-loop clamp lands `duration - 500 ms`, never the exact end (the
    EC_COMPLETE stop trap). Tests updated (6/6), check/fmt/build clean.
- 2026-08-21 00:51 — **Probe v2 deploy #2 (CrossOver, fallback,
  `-audiohookdisable`).** Two movie-backed songs. Results:
  - **RUNNING-STATE SEEK WORKS ON WINE** — visible content jump at the 60 s
    seek, playback continued normally; genuine clock positions read
    correctly on the untouched graph (133/332 ms). **Step 1 decision point:
    GO for position sync (Step 2).**
  - **SetRate conclusively dead on Wine** — hr S_OK, readback 1.000, AND
    maintainer recorded gameplay and compared against the raw video file:
    identical rate. Ladder handles it (readback check → stop rung);
    Windows cabinet remains the rate target (probe retained for that test,
    now wall-clock timed).
  - **`input_manager::on_frame` dispatch is ~2 kHz, NOT per rendered
    frame** — all four probe lines landed in the same second; "+300
    frames" corresponded to 133 ms of playback. Probe v1's frame counting
    was ~35× off (the "+12 s" seek fired ~0.2 s after Run — maintainer
    noticed it landing "as soon as gameplay was visible"). All timing is
    now wall-clock (`SystemTime`), and Step 2's drain logic is
    state-driven, not time-driven, so the dispatch rate is irrelevant to
    production behavior.
  - Video started IN SYNC this run (no pre-Run COM) — confirms probe v1's
    paused-seek diagnosis.
- 2026-08-21 00:08 — **Probe v1 deploy #1** (see below).

- 2026-08-21 00:08 — **Probe deploy #1 (CrossOver bottle, fallback movie
  mode, `-audiohookdisable` present, zero exceptions).** Movie-backed song,
  100 % speed. Results:
  - Capture: `player 0x19ec0030 (loop=0, state=3, seekable=true)` —
    gameplay movies do NOT set the loop flag ⇒ clamp mode per FR-5.
  - `caps hr=0 bits=0x0037` (CanSeekAbsolute|Forwards|Backwards|
    GetStopPos|GetDuration), `duration hr=0` 131766 ms, `pos hr=0` 0 ms.
  - **Native seek: PROVEN** — issued 30000 ms, position readback
    `hr=0, 30000 ms`. (Step 2 unblocked.)
  - **SetRate: SUSPECT on Wine** — `SetRate(1.25) hr=0x00000000` but
    `GetRate readback = 1.000`: accepted-but-not-stored, i.e. a silent
    no-op (classic Wine quartz semi-stub pattern). Awaiting visual
    confirmation (did the movie run fast / start 30 s in?) and a probe on
    the real Windows cabinet (D8: the required platform) before the Step-1
    decision point is judged.
  - Consequence either way: **the Step-3 ladder must verify SetRate by
    GetRate readback (epsilon match), not hr alone** — otherwise a Wine
    silent no-op would present as "synced" and desync instead of degrading
    to the stop rung. Recorded as a design amendment.
- 2026-08-21 (follow-up, maintainer visual reports over several songs):
  - Video did NOT start 30 s in — content played from ~0. The position
    readback (30000 ms) was a Wine FGM echo, not truth: **post-seek
    position readbacks cannot be trusted on Wine.**
  - No visible speedup: **SetRate confirmed a full no-op on Wine** (hr,
    readback, and visuals all agree).
  - The video began presenting at graph open (scroll start), a few seconds
    AHEAD of the audio — the **paused-state seek itself kicked Wine's
    graph into presenting early**, breaking the game's delayed-Run sync
    (stock playback without the probe is in sync). The stock game never
    seeks a paused graph (its open-time seek precedes the Pause).
  - Design amendments recorded in `design/detailed-design.md` (dated
    2026-08-21): NFR-1 readback verification; **seeks only while running**
    (`state == 2`), pre-Run targets held as pending seeks drained at first
    running frame.
- 2026-08-21 — **Probe v2 built** (awaiting deploy): no COM at capture;
  per-frame driver (`input_manager::on_frame`, game thread) waits for the
  game's own Run, logs the genuine clock position + duration at ~5 s of
  playback, applies `SetRate(1.5)` (readback-logged; 1.5 per maintainer —
  more discernible), and issues a RUNNING-state seek to 60 s at ~12 s —
  the exact production seek shape. Reusable COM helpers split out
  (`ms_get_i64` / `ms_set_rate` / `ms_get_rate` / `native_seek`).
  Validation: cargo check clean, harness test pass, fmt, build.sh clean.

## Step 2 test instructions — cabinet test #3 (for the maintainer)

NO env var this time (unset `DDR_MOVIE_SYNC_PROBE` — with it set the probe
will deliberately desync each movie at +5 s/+12 s). CrossOver fallback mode
with `-audiohookdisable`, movie-backed songs at 100 % speed:

1. **Quick restart (press 1)** mid-song: the movie should snap back to its
   beginning together with the song. Log: `movie_sync: seek -> 0 ms ...`.
2. **Training Mode scrubs (7 = RW / 9 = FF)**: the movie should jump with
   each scrub. Log: one `seek -> <t> ms` per scrub.
3. **Training SONG START > 0** (e.g. 30 s): the movie should start at the
   bound, in sync. Log: `capture-time sync queued (...)` OR a `seek ->`
   from the bound engagement, then the drain line.
4. **LOOP SONG**: each wrap should snap the movie back to the A bound.
5. One song WITHOUT any gesture: zero `seek ->` lines, stock behavior.

Report which passed/failed + the `movie_sync` log lines. Watch for: any
crackle/hitch at seek moments, the movie freezing after a seek, or a seek
landing visibly offset from the audio.

## Deviations & open questions

- **OPEN (Step 3 gate): SetRate on real Windows** — dead on Wine
  (three-way confirmed incl. a recording comparison); the Windows cabinet
  probe is still pending ("not easy to test on Windows right now" —
  maintainer). Step 3 proceeds regardless: its readback-verified ladder
  makes Wine degrade to the stop rung correctly, and Windows either works
  or degrades the same way. Probe kept in the build for the eventual test.
- Design amendments (both dated in `design/detailed-design.md`): NFR-1
  SetRate verified by GetRate readback; seeks only while RUNNING with the
  pending-seek drain.
- Probe env var read at init, not gated on layeredfs.developer_mode —
  deemed harmless (logs + intentional desync only).
- Plan Step 2 said "WARN latch for seek failure" — the native seek is a
  void game method with its own null guard; failure surfaces as a missing
  visual jump, not an hr. The seekable=false capture WARN covers the
  detectable case; nothing else to latch.

## Key facts for a cold resume

- Capture source: `movie_policy`'s BuildGraph detour; only REAL successful
  builds (`Passthrough`, hr==0). Suppress/fallback-faked paths never call
  `on_graph_opened`.
- Player: state +0x08, opened +0x14 (touch gate), loop +0x16,
  IMediaSeeking +0x58 (may be legitimately null), native seek = player
  vtbl +0x58 (100 ns absolute).
- IMediaSeeking vtbl: caps +0x18, duration +0x50, current pos +0x60,
  SetRate +0x88, GetRate +0x90.
- Threading: everything runs on the game's actor-update thread inside the
  detour; scene-drop only clears atomics.
- Host tests: temp-crate `#[path]` harness (movie_sync.rs mounts clean on
  non-windows — all engine code is cfg(windows)-gated).
