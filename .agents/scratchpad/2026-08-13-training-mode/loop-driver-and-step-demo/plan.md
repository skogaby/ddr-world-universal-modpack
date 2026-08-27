# Plan: LOOP ON driver + Step-4 cabinet demo

Status: Approved (verified upstream approval — see task-01's context.md;
auto mode per the sop's Step-1 rule.)

## Test scenarios (host — section_math)

1. `loop_fire_bound_composes_min_and_margin`:
   - All terms present: `min` picked (each term exercised as the min),
     margin subtracted.
   - b_live None (loop-whole-song): `min(t94_raw, t98) − margin`.
   - stock_display_raw None (converter-failure degrade): `min(b, t98) −
     margin`.
   - Both None: `t98 − margin` (the always-guarded step-5 edge).
   - Degenerate: results ≤ 0 ⇒ `None` (t98 ≤ margin, or a tiny b).

The driver leg itself is engine-facing — cabinet-validated by the step
demo (task text). Zero regression: full suite stays green.

## Implementation shape

1. **section_math**: `loop_fire_bound(b_live, stock_display_raw,
   stock_raw, margin) -> Option<i32>` + tests.
2. **driver.rs** (extend, not restructure):
   - Consts: `LOOP_FIRE_MARGIN_MS = 1_000`.
   - Statics (reset in `on_gameplay_entry`): `LOOP_DISARMED`,
     `LOOP_BOUND` (0 = uncomputed), `LOOP_BOUND_FROM_B`, `LOOP_COOLING`,
     `LOOP_RETRY_USED`, `LOOP_T94_WARNED`.
   - `loop_step() -> bool` ("the loop leg is live"): gated on
     `bounds::loop_latched()` and not disarmed; (re)computes the bound
     when uncomputed or `section_end()` moved (thresholds via
     `chart_end_thresholds`, t94→raw via `decoded_notes` +
     `raw_for_display` with the WARN-once dropped-term degrade);
     degeneracy checks (bound None / current count ≥ bound at compute /
     `active_section_start() ≥ bound`) ⇒ disarm + one WARN; cooling
     drain (count < bound clears); fire at count ≥ bound via
     `request_reset(a_live.unwrap_or(0), TRAINING_LEAD_MS as i32, Zero,
     None)` — Started ⇒ cooling; Refused/Unsupported ⇒ one retry next
     frame then disarm + one WARN.
   - `step()`: after the Step-3 legs, `let loop_running = resolved &&
     loop_step();` — exit when `resolved && !adjust_outstanding &&
     !loop_running`; the 60 s timeout check moves below the exit and is
     skipped while `loop_running` (pre-anchor scope).
3. Readiness gates: harness → check → fmt → `./build.sh`.
4. Demo handoff: Step-4 demo script + PNG-deploy reminder into the
   feature `progress.md`; tick plan Step 4 only when the cabinet demo
   passes (left unticked at handoff).

## Risks

- The one-in-flight rule: never fire while cooling; the count's climb
  through the prepare window is absorbed by the cooling latch.
- Timeout re-scope must not lose the pre-anchor protection — covered by
  gating the skip on `loop_running` (false until resolution settles).
