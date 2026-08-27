# Context: LOOP ON driver + Step-4 cabinet demo

Task file: `.agents/tasks/2026-08-13-training-mode/step04/task-03-loop-driver-and-step-demo.code-task.md`
Approval chain verified in task-01's context.md. Auto mode. Baseline
entering this task: **248/248** harness.

## Requirements

1. Fire-bound compute (once per song after resolution settles):
   `min(b_live?, raw_for_display(notes, t94)?, t98) − 1000 ms`; terms drop
   when unavailable (`raw_for_display` failure ⇒ one WARN, term dropped —
   the t98 clamp still guards the fatal step-5 edge; degraded to a
   one-shot loop). Bound ≤ 0, below the current count at compute time, or
   a section start at/above it ⇒ disarm + one WARN (degenerate section).
2. Per-frame loop leg (LOOP latch only): count ≥ bound ⇒
   `request_reset(active_section_start().unwrap_or(0), TRAINING_LEAD_MS
   as i32, AccumulatorPolicy::Zero, None)`. Started ⇒ cooling until the
   count reads below the bound; Refused ⇒ one retry next frame, then
   disarm + one WARN (natural continue).
3. Gestures: B changes recompute the fire bound (next iteration); A is
   read live at fire time; bounds cleared under LOOP ON ⇒ whole-song
   semantics continue (bound from thresholds only).
4. Driver lifetime: keep requeueing while the loop is armed; the 60 s
   soft timeout scoped to the PRE-ANCHOR phase only.
5. Zero footprint for LOOP OFF sessions (leg gated on the per-song latch).
6. Host tests: the fire-bound min/margin composition as a pure
   `section_math` helper with dropped-term cases.
7. Readiness gates + the cabinet demo handoff (incl. task-02 legs +
   the PNG-deploy reminder).

## Key facts

- Driver shape (`driver.rs::step`): render-thread self-requeueing,
  generation-tokened; current exit = `resolved && !adjust_outstanding`;
  timeout checked at the top. Step-3 legs (resolution retry, one-shot
  adjust) are cabinet-validated — EXTEND, don't restructure.
- `request_reset` / `perform_seek` / the transaction: shipped, cabinet-
  validated — task-03 only decides WHEN to call. One in-flight reset at a
  time: the COOLING latch (wait for the observed count to rewind below
  the bound) is the re-arm condition; never poll `Started` again during
  prepare (the pre-completion anchor keeps the count climbing, which the
  cooling latch naturally absorbs).
- `ResetOutcome::Unsupported` is retained-for-API-stability (no longer
  returned) — treated like Refused.
- LOOP ON never wrote the thresholds (task-02's policy exclusivity), so
  `chart_end_thresholds` reads the STOCK pair.
- Sides: threshold/notes reads via `(0..2).find_map(...)` (the task-02
  write path's shape — all live actors share one chart).
- Margin: the existing 1000 ms end-margin class (`SEEK_END_MARGIN_MS` /
  `MARKER_END_MARGIN_MS` precedents); also covers the ~150–300 ms
  stop/replay prepare window.

## Interpretation decisions (auto mode)

- Pure helper: `section_math::loop_fire_bound(b_live: Option<i32>,
  stock_display_raw: Option<i32>, stock_raw: i32, margin: i32) ->
  Option<i32>` — `None` = degenerate (≤ 0). The current-count/section-
  start degeneracy checks need live reads and stay in the driver.
- Recompute detection: the driver caches the `b_live` the bound was
  computed from and recomputes when the live value differs (B gestures
  and triple-5 both surface as a changed `section_end()`).
- Timeout scoping: the timeout check moves BELOW the work-done exit and
  is skipped while the loop leg is live (`loop_running`), so pre-anchor
  stalls still time out (resolution pending / adjust outstanding ⇒ the
  loop leg is not yet running) and grinds run indefinitely.
- Loop statics reset at `on_gameplay_entry` (per-song; exit bumps the
  generation so a stale queued step self-cancels).
- A gesture-A moved at/above the bound mid-grind leaves cooling
  permanently latched (fires stop; the song ends via the STOCK cascade —
  stock thresholds sit above the bound) — graceful, documented, no
  disarm WARN spam.
