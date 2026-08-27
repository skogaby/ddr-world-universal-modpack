# Task: FF/RW scrobbling (pinpad 7/9)

## Description

Single-press pinpad **7 = rewind** / **9 = fast-forward** by
`training_mode.rw_increment_ms` / `training_mode.ff_increment_ms`
(default 5000 ms each) during eligible gameplay, dispatched through the
shipped seek transaction (`song_reset::request_reset` with `t_ms > 0`).
The Step-6 timeline cursor is the visual feedback (no toast — the
cursor jump IS the confirmation); the feature must work with the
timeline placement OFF too (the seek itself is the feature). Score
containment comes for free: Step 5's `on_song_reset(t > 0)` subscriber
taints the side automatically.

This is the design's R12 amendment (2026-08-14) pulled from v2 into v1
as plan Step 7 — the seek machinery shipped in Step 2, so this task is
a thin gesture + clamp + config layer over existing transactions.

## Background

- **Gesture surface**: `src/mods/training_mode/bounds.rs` already
  handles triple-4/5/6 in `on_input_event` (frame thread, panic-free,
  `GESTURES_ACTIVE` + GAMEPLAY-gated, per-side via `event.player`).
  7/9 are SINGLE-press (no GestureBuffer triple-detection) — one press
  = one increment. No button conflict: quick_logout's triple-9 is
  song-select-scoped, and this surface is GAMEPLAY-only.
- **Seek transaction**: `song_reset::request_reset(t_ms, delay_ms,
  AccumulatorPolicy::Zero, None)` (`src/services/song_reset/mod.rs`) —
  `t_ms > 0` is seek-to-T: block-quantized target, pre-T notes
  consumed-neutral, spanning freezes neutralized, timing anchor
  back-dated. `AccumulatorPolicy::Keep` is REFUSED in v1 (reserved) —
  this task MUST use `Zero`, per the approved plan text.
- **Clamp/quantize precedent**: the marker-set path in `bounds.rs`
  (`set_marker` → `quantize_marker` + `marker_clamp_bound` =
  `chart_end − MARKER_END_MARGIN_MS`). Note the round-2 amendment
  precedent in `section_math::loop_fire_bound`: safety margins guard
  STOCK thresholds, not user intent — the forward clamp bound should
  keep the seek target strictly below the end-cascade region
  (`min(b_live?, chart_end) − margin`, per the plan) while a rewind
  clamps at 0.
- **Cooling latch precedent**: the loop driver's `LOOP_COOLING`
  (`src/mods/training_mode/driver.rs`) — one reset in flight at a
  time. The scrub needs its own latch AND must yield while a loop
  reset is in flight (both mechanisms share the underlying
  `request_reset` machinery; concurrent dispatch is refusal-prone and
  pointless).
- **Config precedent**: `QuickRestartConfig` in `src/mods/config.rs`
  (`quick_restart.restart_delay_ms` — optional block, optional keys,
  normalize-with-INFO at consumption). The design reserved the
  `training_mode` config block for exactly these keys (R12).
- **Taint**: Step 5 subscribes to `song_reset`'s reset notifications;
  any `t > 0` reset taints the side's per-stage save. No new taint
  code in this task — verify, don't reimplement.

## Reference Documentation

**Required:**
- Design: .agents/planning/2026-08-13-training-mode/design/detailed-design.md
  (R12 + its 2026-08-14 amendment; R14 seek semantics; §4.4 seek-to-T)

**Additional References (if relevant to this task):**
- .agents/planning/2026-08-13-training-mode/implementation/plan.md — Step 7
  (objective/guidance/tests/demo) + the Step-6 "As landed" note (the
  amended integration surface: OFF/LEFT/RIGHT placement, loop fire
  margin shape)
- docs/training_mode_research.md — §3 seek-to-T record rebuild
  semantics, §4 natural song-end chain (why the forward clamp exists)
- .agents/scratchpad/2026-08-13-training-mode/markers-readout-placement-row-and-demo/progress.md
  — the round-2 loop-fire-margin amendment (margin guards stock
  thresholds only — the same principle applies to the FF clamp)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. Config: `training_mode` block in `src/mods/config.rs` with optional
   `ff_increment_ms` / `rw_increment_ms` (i32). Default 5000 each when
   absent; normalize out-of-range values to 250..=60000 with one INFO
   (the `restart_delay_ms` pattern). Read at mod enable (or first use);
   no live-edit requirement.
2. Pure target math in `src/mods/training_mode/section_math.rs`
   (harness-mounted — host tests run): given `current_ms`, signed
   `delta_ms`, optional `b_live`, `chart_end_ms`, and `margin_ms`,
   produce the clamped seek target: `clamp(current + delta, 0,
   min(b_live?, chart_end) − margin)`; degenerate inputs (no chart,
   bound ≤ 0) refuse with `None`. Block quantization stays in the live
   layer (`bounds::quantize_marker` — it needs the live binding's
   grid), matching the marker-set split.
3. Gesture surface: extend `bounds::on_input_event` for `NUM_7` /
   `NUM_9`, single-press, `InputEventType::Pressed` only, GAMEPLAY +
   `GESTURES_ACTIVE` gated (the existing gates), per-side. NO
   GestureBuffer for these two.
4. Dispatch: compute the target (pure math + live quantize) and fire
   `song_reset::request_reset(t_q, TRAINING_LEAD_MS, Zero, None)`.
   A target that quantizes to ≤ 0 (rewind past the start) MUST seek to
   0 via the restart-from-A path precedent rather than passing 0
   (which `request_reset` treats as the plain restart — decide and
   document; the restart semantics ARE acceptable for "rewind to
   start" if the timing anchor behavior matches).
5. Cooling: one scrub in flight (own `AtomicBool`, cleared on
   completion/refusal like `LOOP_COOLING`); additionally refuse while
   the loop driver's reset is in flight. A refused press is dropped
   silently (one debug log) — never queued.
6. Fail-open: every gate failure (no live binding, seek unavailable,
   refused transaction) drops the press with at most one WARN per
   song; the song must never be disturbed.
7. Host tests for the pure clamp math (normal, clamp-at-end with and
   without `b_live`, clamp-at-zero, degenerate refusal) and the config
   normalize (defaults, clamping, absent block). Engine-facing wiring
   is cabinet-validated (no host harness).

## Dependencies

- `song_reset::request_reset` seek-to-T (Step 2, shipped + cabinet-proven)
- `bounds.rs` gesture/scene infrastructure (Step 3, shipped)
- Loop driver cooling interplay (`driver.rs`, Step 4, shipped)
- Step-5 taint subscriber (shipped — verify it fires, do not extend)
- `src/mods/config.rs` (`QuickRestartConfig` pattern)

## Implementation Approach

1. Pure clamp function + tests in `section_math.rs` (TDD — tests first).
2. `TrainingModeConfig` block + parse/normalize + tests.
3. Gesture arm in `on_input_event` (NUM_7/NUM_9 → a `scrub(side,
   direction)` entry point in `bounds.rs` or a small new module beside
   `driver.rs` if the dispatch state warrants it).
4. Dispatch: current position (`song_reset::current_raw_music_count`),
   pure clamp, live quantize, cooling check, `request_reset`.
5. Gates in order: harness `cargo test` → `cargo check --target
   x86_64-pc-windows-msvc` → `cargo fmt` (whole crate) → `./build.sh`;
   then the cabinet demo.

## Acceptance Criteria

1. **Fast-forward skips forward by the configured increment**
   - Given an eligible song playing at position T with default config
   - When pinpad 9 is pressed once
   - Then playback resumes at ~T+5000 ms (block-quantized), the
     timeline cursor jumps accordingly, and claps/judging stay aligned

2. **Rewind skips backward**
   - Given an eligible song playing at position T > 5000 ms
   - When pinpad 7 is pressed once
   - Then playback resumes at ~T−5000 ms with records rebuilt
     (pre-target notes consumed-neutral — replayed notes judge again)

3. **Forward clamp near the chart end**
   - Given the current position within one increment of the end bound
   - When pinpad 9 is pressed
   - Then the target clamps below `min(b_live?, chart_end) − margin`
     and the end cascade never fires early

4. **Rewind clamp at the start**
   - Given the current position within one increment of 0
   - When pinpad 7 is pressed
   - Then playback resumes from the song start without a refused or
     wedged transaction

5. **Rate interaction**
   - Given a song playing at a non-100% rate (song-playback-speed)
   - When 7/9 are pressed
   - Then the skip lands correctly in content time and audio/claps stay
     aligned (the transaction's wall/content mapping does the work)

6. **One in flight**
   - Given a scrub transaction in flight (or a loop reset in flight)
   - When 7/9 is pressed again
   - Then the press is dropped (no queueing, no double-seek, no crash)

7. **Score containment**
   - Given a song where any scrub fired
   - When the per-stage save runs
   - Then the save is suppressed (Step-5 taint); an untouched song in
     the same session still submits normally

8. **Fail-open**
   - Given the seek machinery unavailable (no binding / gates failed)
   - When 7/9 is pressed
   - Then the song plays on undisturbed with at most one WARN per song

9. **Host tests green**
   - Given the temp-dir harness
   - When `cargo test` runs
   - Then the new clamp + config tests pass and the suite stays green

## Metadata

- **Complexity**: Medium
- **Labels**: training-mode, input, seek, config, gameplay
- **Required Skills**: Rust, in-process hooking discipline (AGENTS.md
  rules), the song_reset transaction model, cabinet deploy validation
- **Generated By**: code-task-generator 2026-08-15
- **Source Plan**: .agents/planning/2026-08-13-training-mode/implementation/plan.md
- **Plan Step**: Step 7: FF/RW scrobbling (pinpad 7/9)
