# Progress: LOOP ON driver + Step-4 cabinet demo

Task: step04/task-03. Mode: auto (verified upstream approval).

## Checklist

- [x] `loop_fire_bound` tests written (section_math), failing
- [x] `loop_fire_bound` implemented, tests green
- [x] driver.rs loop leg (compute/recompute, cooling, retry/disarm)
- [x] Driver lifetime + timeout re-scope (pre-anchor only)
- [x] Gates: harness → check → fmt → ./build.sh
- [x] Demo handoff written (feature progress.md; PNG reminder)
- [x] Cabinet demo (maintainer-run) — **PASSED 2026-08-14 after four
      cabinet-found fixes (anchor wait, scene-callback deadlock, cascade
      parking, +0x19C baseline restore) + the triple-5 clear semantics
      change; plan Step 4 TICKED.** Full history in the feature
      progress.md deploy log.

## Log

- Setup complete; baseline 248/248.
- Cycle 1 (pure fire bound): `loop_fire_bound_composes_min_and_margin`
  added to `src/mods/training_mode/section_math.rs` — failed E0425;
  implemented `loop_fire_bound(b_live, stock_display_raw, stock_raw,
  margin) -> Option<i32>` (min of the present terms − margin; ≤ 0 ⇒
  `None`). 249/249.
- Cycle 2 (driver leg, engine-facing — cabinet-validated by the step
  demo): `src/mods/training_mode/driver.rs`:
  - New consts/statics: `LOOP_FIRE_MARGIN_MS = 1_000`; per-song
    `LOOP_DISARMED`/`LOOP_BOUND_MS`/`LOOP_BOUND_FROM_B`/`LOOP_COOLING`/
    `LOOP_RETRY_USED`/`LOOP_T94_WARNED`, all reset at
    `on_gameplay_entry` (exit bumps the generation as before).
  - `loop_step()` (returns "loop leg live"): gated on
    `bounds::loop_latched()` + not disarmed; (re)computes the bound when
    none exists or `section_end()` moved; cooling drain (count < bound
    clears — absorbs the prepare window's climbing count); fire at
    count ≥ bound via the SHIPPED
    `request_reset(active_section_start().unwrap_or(0),
    TRAINING_LEAD_MS as i32, Zero, None)`; Started ⇒ cooling +
    per-iteration INFO; Refused/Unsupported ⇒ one retry next frame then
    disarm + one WARN.
  - `compute_fire_bound(b_live, initial)`: stock pair via
    `chart_end_thresholds` (unreadable ⇒ disarm + WARN); `+0x94` →
    raw via `decoded_notes` + `raw_for_display` (failure ⇒ WARN once,
    term dropped — the `+0x98 − margin` clamp still guards step 5;
    degraded one-shot loop per §6); `section_math::loop_fire_bound`;
    INITIAL-compute degeneracy (bound `None`, count ≥ bound, or
    a_live ≥ bound) ⇒ disarm + one WARN. One INFO logs the composed
    bound and its terms.
  - `step()`: loop leg runs after `resolved` (the latch + actors exist);
    exit now `resolved && !adjust_outstanding && !loop_running`; the
    60 s soft timeout moved BELOW the exit check and skipped while
    `loop_running` — pre-anchor stalls still time out, grinds run
    indefinitely. Step-3 legs byte-identical in behavior (resolution
    retry, stamp coherence, one-shot adjust untouched).
- Validate: harness 249/249 (243 pre-existing at task start + 1 new;
  suite total includes task-01/02's +6), `cargo check --target
  x86_64-pc-windows-msvc` clean, `cargo fmt`, `./build.sh` → release DLL
  at `target/x86_64-pc-windows-msvc/release/ddr_world_hook.dll`
  (build log: `logs/build.log`).
- Demo handoff + Step-4 demo script written into the feature
  `progress.md` (`.agents/planning/2026-08-13-training-mode/`).
  **Plan Step 4 left UNTICKED — it ticks when the cabinet demo passes.**

## Deploy & test log

- **2026-08-14 03:07 cabinet run (maintainer, LOOP ON + whole song +
  175 %): FAIL — no loop; the song played through once to results.**
  Log root cause (one line): `TrainingMode: degenerate section (count
  304644 ms / start 0 ms vs fire bound 123911 ms) -- loop disarmed`,
  0.85 s after gameplay entry. The initial fire-bound compute ran the
  instant the resolution completed — BEFORE the run's first `0x1044`
  anchor. `GamePlayActor+0x178` derives from the anchor at `+0x160`;
  unanchored it reads as the raw frame tick (~304 s since boot, still
  inside `current_raw_music_count`'s 1-hour sanity range), tripping the
  initial-compute `count >= bound` degeneracy disarm at song start.
  Rate-independent (the 175 % was incidental); the bound itself
  (123911 ms) was correct.
  **Fix (same session):** `loop_step` gates the INITIAL compute on
  `song_reset::first_anchored_frame()` (the silent-start adjust's own
  gate) plus a count-credibility check (`count < chart_end_raw` — the
  `+0x178` cache can hold the stale pre-anchor tick for one more frame
  after the anchor lands; a live pre-cascade run can never legitimately
  read at/past the +0x98 threshold). `loop_step` now returns a
  three-state `LoopState` {Idle, AwaitingAnchor, Grinding} so the
  anchor wait stays UNDER the 60 s pre-anchor timeout while a live
  grind remains exempt. Gates re-run: harness 249/249, check clean,
  fmt, build.sh → fresh DLL. Awaiting re-test.

## Deviations

- Recompute semantics (recorded in context.md): the degenerate-section
  disarm checks (count/section-start vs bound) apply on the INITIAL
  compute only, per the task's "once per song" framing; a mid-grind B
  gesture that lands behind the cursor recomputes the bound and simply
  fires on the next frame — the loop-ON mirror of LOOP OFF's accepted
  "end here" semantics. `bound ≤ 0` disarms on every compute.
- A gesture-A moved at/above the bound mid-grind leaves the cooling
  latch parked (no thrash, no WARN spam): the grind stops and the song
  ends via the stock cascade. Documented, not gated — the initial
  compute disarms the resolvable version of this shape.

- **2026-08-14 demo attempt 3 (legs 2 + 5): both failed, both fixed
  same session** — full analysis in the feature progress.md deploy log.
  (a) Leg-2 freeze: scene_manager fired callbacks UNDER its mutex and
  task-02's `clear_session_state` called `current_scene()` from the
  gameplay-exit callback → frame-thread self-deadlock, first reachable
  on the first run with written thresholds. Fixed in scene_manager
  (Arc-snapshot callbacks, fired outside the lock) AND in
  `clear_session_state` (no scene read — the restore attempt rides
  `set_chart_end_thresholds`' own fail-closed gates). learnings.md
  entry added. (b) Leg-5 early loop: the `+0x94` clamp now applies only
  to seek-path fires (section start > 0); t=0 whole-song fires clamp on
  `min(b?, +0x98) − margin` — the finale plays to 1MM
  (`compute_fire_bound(b_live, seek_path, initial)` +
  `LOOP_BOUND_FROM_A_PRESENT` recompute key; bound log names the path).
  Documented edge: gesture-A set mid whole-song grind after the cascade
  passed step 4 ⇒ refused seek → retry → disarm WARN → natural end.
  Gates: 249/249, check, fmt, build.sh → fresh DLL.

Status: Complete (uncommitted — the maintainer commits; cabinet demo
pending)
