# Progress — FF/RW scrobbling (pinpad 7/9)

Status: Complete (uncommitted — the maintainer handles all git)

Updated: 2026-08-15
Cabinet validation: 4 demo rounds (2026-08-15), round 4 **ALL LEGS
CONFIRMED** (maintainer: "all legs confirmed working as expected") —
plan Step 7 TICKED with an "As landed" note. Round history: R1 = FF/RW
correct + 2 amendments (no lead-in, indicator icons); R2 = all task
legs correct + clap-resume root cause (pre-existing full-resynthesis →
re-shift demotion); R3 = FF/RW + clap resume perfect + 2 loop fixes
(reset clap floor, LOOP death bypass); R4 = everything green.

## Demo rounds

- **Round 4 (2026-08-15, FINAL)**: ALL LEGS CONFIRMED — loop lead-in
  clap-silent with the first post-A clap exact, LOOP ON death = instant
  restart at A (gauge restored), LOOP OFF death still fails out,
  quick-fail exits the grind. Task CLOSED.
- **Round 3 (2026-08-15)**: FF/RW perfect, claps resume immediately ✅.
  Two loop-interaction findings, both fixed same-day (v4):
  (a) **claps audible during the loop's silent lead-in** — the re-shift
  fix serves the whole track, so claps for pre-A CONSUMED notes (rebuilt
  consumed-neutral, never judged) played during the 2.5 s approach.
  Fix: reset clap floor — the subscriber stores the reset target; every
  commit mutes the served region before the target's clap position
  (computed with `tick_track_positions` on the one-element target — the
  synthesis formula itself, so a note exactly AT the target keeps its
  clap while strictly-before notes lose theirs, matching the rebuild's
  strictly-before consumption). Plumbed as a block-aligned
  `mute_head_bytes` on `game_audio::rewrite_tick_wave` (silence-block
  fill before the copied region). Also fixes the same leak on the R15
  silent skip-first start for free.
  (b) **LOOP ON now bypasses natural death** (maintainer expectation:
  death = instant restart at A, not a fail-out). See Deviations #4.
- **Round 2 (2026-08-15, ALL LEGS RUN)**: everything correct except the
  2–3 s clap resume — root-caused to the pre-existing full-resynthesis
  `on_song_reset` subscriber (also the checkpoint-4 loop gap), replaced
  with the Playing→Ready re-shift demotion (Deviations #3).
- **Round 1 (2026-08-15, partial legs)**: FF and RW both correct ✅;
  lead-in removed (delay-0 music-player scrub) + RW/FF indicator icons
  added (Deviations 1–2).

## Checklist

- [x] Setup: create working dir, approval chain verified (auto mode)
- [x] Explore: docs + code surfaces read, interpretations recorded (context.md)
- [x] Plan: test scenarios + implementation approach (plan.md)
- [x] TDD cycle 1: `scrub_target` + `normalize_scrub_increment_ms` in section_math.rs
- [x] Config block: `TrainingModeConfig` in config.rs
- [x] Accessors: `song_reset::reset_in_flight`, `driver::loop_reset_in_flight`
- [x] Wiring: bounds.rs scrub layer + gesture arm + mod.rs enable latch
- [x] Gates: harness test (306/306) → cargo check → cargo fmt → ./build.sh
- [x] Consistency review + record update
- [x] Cabinet demo (maintainer) — 4 rounds, round 4 ALL LEGS CONFIRMED
- [x] Plan Step 7 ticked + "As landed" note appended; record closed

## TDD cycles

1. **Pure math (section_math.rs)** — RED: 5 new tests written first
   (`scrub_target_moves_by_the_delta`, `…_clamps_at_the_end_bound`,
   `…_clamps_at_zero`, `…_refuses_degenerate_inputs`,
   `scrub_increment_normalizes_absent_and_out_of_range`); harness failed
   with 28 resolution errors (functions absent — the expected Rust-TDD red).
   GREEN: `scrub_target` + `normalize_scrub_increment_ms` + the three
   `SCRUB_INCREMENT_*` constants implemented; harness **306 passed / 0
   failed** (baseline 301 + 5).
2. **Config + wiring** — engine-facing (no host harness; per the task,
   cabinet-validated): `TrainingModeConfig` in `src/mods/config.rs`
   (+ `ConfigFile.training_mode` + both fallback literals);
   `song_reset::reset_in_flight()` (public one-liner over the existing
   `RESET_IN_FLIGHT`); `driver::loop_reset_in_flight()` (`pub(super)` over
   `LOOP_COOLING`); bounds.rs scrub layer (statics, `load_scrub_increments`,
   `warn_scrub_once`, `scrub`, 7/9 arm in `on_input_event`, session-state
   clears); `mod.rs` enable latch + doc/log updates.

## Validation (2026-08-15)

- Harness `cargo test`: **306 passed / 0 failed** (logs/harness-test.log)
- `cargo check --target x86_64-pc-windows-msvc`: clean (logs/cargo-check.log)
- `cargo fmt` (bare): no churn beyond the task's files
- `./build.sh`: clean → `target/x86_64-pc-windows-msvc/release/ddr_world_hook.dll`
- Consistency review of the full diff: gesture path for triple-4/5/6
  untouched; scrub layer matches the module's atomics/WARN-once/logging
  conventions; fixed a pre-existing stale comment on `GESTURE`
  (`[0=7, 1=9, 2=5]` → `[0=4, 1=6, 2=5]` — the code always mapped 4/6/5).
- Config seeded (maintainer request): the `training_mode` block
  (`ff_increment_ms`/`rw_increment_ms`, both 5000 = the defaults) added
  to the repo `mod-config.json` AND the install copy at
  `$DDR_WORLD_INSTALL/mod-config.json` (all other keys preserved,
  install re-parse verified). Keys are optional — absent behaves
  identically — but present they document the knobs and give the demo's
  optional out-of-range leg something to edit. `training-mode` stays
  absent from the `mods` enable map: absent = enabled
  (`unwrap_or(true)`, src/lib.rs:146 — how Steps 1–6 already ran).

## Requirement / AC coverage map

- Req 1 config → `TrainingModeConfig` + `load_scrub_increments` (INFO on
  out-of-range; enable-time read). Host-side semantics in test 5.
- Req 2 pure math → `section_math::scrub_target` (tests 1–4); quantization
  stays in `bounds::quantize_marker` (the marker-set split).
- Req 3 gesture → single-press 7/9 arm ahead of the triple-slot match,
  `Pressed` + `GESTURES_ACTIVE` + GAMEPLAY gated, per-side, no GestureBuffer.
- Req 4 dispatch → `request_reset(t_q, TRAINING_LEAD_MS as i32, Zero, None)`;
  t_q ≤ 0 documented as the plain t=0 restart (loop-driver precedent,
  anchor-equivalent to a seek-to-0 — see context.md interp. 1).
- Req 5 cooling → `SCRUB_COOLING` (set at Started, lazily cleared via
  `song_reset::reset_in_flight()`, cleared with session state) + yield to
  `driver::loop_reset_in_flight()`; refusals dropped with `log_debug!`.
- Req 6 fail-open → `warn_scrub_once` ladder (no binding / chart end
  unreadable / degenerate bound / seek unavailable / transaction refused);
  transient no-count stays debug-level.
- Req 7 host tests → 5 new tests, suite green.
- AC 7 score containment → `SESSION_ACTIVE` + `set_training_taint(side)` on
  `Started` (the set_marker pattern; no new taint machinery). NOTE: for the
  t=0 restart, `notify_subscribers(0)` fires synchronously INSIDE
  `request_reset` — before our `SESSION_ACTIVE` store — so the Step-5
  subscriber alone would miss it; the direct taint call is load-bearing there.
- ACs 1–6, 8 → cabinet demo legs (below).

## Deviations

- **No commit** (code-assist Step 6): repo rule — the maintainer handles
  ALL git; Steps 1–6 were committed by the maintainer as `4413cc8`. This
  record closes with `Status: Complete (uncommitted…)` on demo PASS, the
  Step-6 precedent.
- Minor additive service surface: `song_reset::reset_in_flight()` — chosen
  over count-observation heuristics as the scrub latch's completion signal
  (robust across completion, every recovery path, and scene changes).
  Recorded in context.md interp. 2.
- **Maintainer amendments (2026-08-15, mid-demo — round 1 feedback,
  supersede the task text's call shape):**
  1. **No approach lead**: the scrub dispatches
     `request_reset(t_q, 0, Zero, None)` — delay 0 instead of
     `TRAINING_LEAD_MS`. The task's spelled shape produced ~2.5 s of
     silent lead-in per skip; the maintainer wants a pure timeline
     adjuster (music-player FF/RW). `TRAINING_LEAD_MS` remains the
     section-practice lead (restart-from-A, loop passes) — unchanged.
     The t_q = 0 rewind-to-start now takes the INSTANT restart path
     (same music-player semantics).
  2. **Scrub indicator**: new `scrub_indicator.rs` — RW icon left
     (x=180) / FF icon right (x=1100), mid-height, 96 px, the toast's
     fade envelope (100/250/300 ms), flashed on every dispatched scrub
     (`Started` only — a dropped press flashes nothing). Two repo-shipped
     128×128 PNGs (`data_mods/training_mode/tex/training_scrub_{rw,ff}.png`,
     generator `scripts/gen_training_scrub_icons.py`), asset_loader
     chrome model (loaded once at enable via `prime()`, never released),
     one ImageWidget per side created lazily on resolve, generation-
     tokened animation (toast pattern verbatim), `dismiss()` on disable.
     Positions clear the chart strip at either placement (strip ≤ 128 px
     from the edge). Fail-open: unresolved texture / refused widget =
     no flash, the scrub already happened. Icon restyle (maintainer,
     same day): triangle overlap raised from tip-only (~19%) to 40%
     occlusion of the back triangle — the layered ⏩ look; PNGs
     regenerated + re-copied to the install (asset-only change, no
     rebuild; textures load at enable, so a running game needs a
     relaunch to pick them up).
  3. **Assist-tick clap-resume fix** (round-2 finding; a PRE-EXISTING
     shipped-mod bug this task surfaced — also the checkpoint-4 loop
     gap): `src/mods/assist_tick.rs`'s `on_song_reset` subscriber no
     longer does `clear()` + full rebuild (which re-synthesized the
     whole clap track = 2–3 s of silence after EVERY reset — scrubs,
     loop iterations, quick restarts). It now demotes a committed track
     `Playing → Ready` (retaining `encoded`/`m0`/`rate` — the track is
     content-authored; a reset moves only the wall anchor), and the next
     judge dispatch re-commits it shifted to the LIVE count via the
     shipped rewind-guard mechanism — claps resume within a frame of
     the transaction completing. The `Ready` commit arm additionally
     WAITS while `restart_skip_ms < 0` (a future-dated delayed restart:
     quick restart's countdown, the training lead — the track's first
     byte IS count `m0`, so the commit fires exactly when the count
     reaches it; the fresh-song path always has skip ≥ 0 and is
     unchanged). Earlier phases are left alone on reset: an in-flight
     synthesis stays authored against the unchanged `m0` and its
     eventual commit reads the live count anyway. The Playing-phase
     rewind guard stays as the safety net for non-notifying rewind
     paths. Expected log on scrub/loop: `AssistTick: song reset -- tick
     track re-armed (re-shift)` then `AssistTick: rewind re-anchor`-
     class commit lines — and NO `spawn_synthesis` mid-song.
  4. **Reset clap floor** (round-3 finding a): the subscriber stores the
     reset target (`reset_floor_ms`, latest-reset-governs, any phase);
     every commit computes the target's clap position via
     `tick_track_positions` on the one-element target (the synthesis
     formula itself — strictly-before notes lose their claps, a note AT
     the target keeps its, matching the rebuild's strictly-before
     consumption) and passes it as a block-aligned `mute_head_bytes` to
     `game_audio::rewrite_tick_wave` (silence-block fill ahead of the
     copied region; new 4th param, single caller). The loop's 2.5 s
     approach lead is now clap-silent; also fixes the same leak on the
     R15 silent skip-first start. Expected log: commit lines now show
     `mute head N bytes` (> 0 on loop iterations, 0 on scrubs).
  5. **LOOP ON bypasses natural death** (round-3 finding b; an amendment
     to the Step-4 loop semantics implemented here): mechanism = the
     engine's OWN instant-death gate byte (`GamePlayActor+0x2B7` —
     20260721 decompile: BOTH the `0x103C` STEP_GAME_OVER advance and
     the DPS finish-poll `FUN_18005bde0`'s death arm are conditioned on
     it being 0). At the loop latch, `bounds::arm_death_bypass` stashes
     the stock byte and sets it (fail-open: unreadable/refused ⇒ stock
     death, one WARN); a gauge death then latches `m_isDead` WITHOUT
     ending the run — race-free, no frame-timing dependence. The loop
     driver reads `song_reset::any_actor_dead()` per grinding frame and
     fires the normal loop reset immediately (`death revive` log tag);
     the transaction's shipped completion block already clears
     +0x1E8/+0x1E9/+0x2B8 + miss-streak and restores the gauge from the
     snapshot (`reset_side_state`) — the reset IS the revive, zero new
     revive code. A refused fire walks the existing retry/disarm
     ladder; disarm (and `clear_session_state`) restores the stashed
     gate, so a still-latched death fails out naturally — stock
     behavior is the fallback on every degraded path.
     Quick-fail/quick-restart remain the deliberate grind exits (they
     force step ≥ 5, which the gate does not block). New song_reset
     accessors: `death_gate()`, `set_death_gate(on)`,
     `any_actor_dead()`; the driver also defers loop fires while
     `reset_in_flight()` (one transaction total, scrub included).

## Demo checklist (plan Step 7 demo paragraph)

1. 7/9 skip backward/forward by the configured increment at 100%.
2. Same at a non-100% rate (song-playback-speed) — skips land in content
   time, claps/judging aligned after every skip.
3. Claps aligned after every skip (assist tick re-syncs via the
   transaction's subscriber notification).
4. Skips near the chart end clamp (no early end-cascade fire); with a live
   B, the clamp lands margin below B.
5. Rewind within one increment of 0 restarts from the song start (no
   refused/wedged transaction).
6. Rapid presses / press during a loop reset: dropped, no double-seek.
7. Scrubbed song's score suppressed; untouched song in the same session
   submits.
8. (Optional) `training_mode.ff_increment_ms` set out of range → one INFO,
   normalized value in effect.

Demo hygiene: launch with a CLEAN environment (no stale `DDR_*_FAULT` vars
— the Step-6 lesson).

## Key facts for a cold resume

- Task contract: `.agents/tasks/2026-08-13-training-mode/step07/task-01-ff-rw-scrobbling.code-task.md`
- Touched: `src/mods/training_mode/{bounds,driver,mod,section_math}.rs`,
  `src/mods/config.rs`, `src/services/song_reset/mod.rs` (all uncommitted)
- Harness: `/var/folders/31/yq10yrk557l1q0wyb1nx4vg40000gp/T/opencode/ddr-host-harness`
  (`cargo test --quiet`; recreate recipe in the feature progress.md)
- Gates order: harness test → `cargo check --target x86_64-pc-windows-msvc`
  → `cargo fmt` (bare) → `./build.sh`
