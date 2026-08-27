# Plan: driver-and-silent-skip-first-start

Status: Approved 2026-08-13 (verified upstream approval, auto mode).

## Test scenarios (host-viable pieces first)

1. `seek::blocks_to_wall_ms` (seek_tests.rs — mounted):
   - round-trip property: for a set of targets, `quantize_seek(t).blocks`
     fed back through `blocks_to_wall_ms` == `quantize_seek(t).t_q_ms`.
   - explicit values on the production-like grid (128 samples @ 44 100 Hz);
     degenerate grid (rate 0 / spb 0) → None; block 0 → 0.
2. `section_math::pre_shift_wall_ms` (mounted):
   - identity percent → equal; 50 % → ×2; 175 % → ×100/175 (Step-2 demo
     numeric: 60 000 content @ 175 → 34 285); 0 content → 0; percent
     floor guards (0/negative treated as identity — defensive).
3. The refactor's behavior-identity is pinned by the FULL existing suite
   (`perform_seek` callers unchanged) + gates re-run before building on it
   (task instruction).
4. Driver/adjust/fallback: cabinet-validated (the Step-3 demo).

## Implementation order (gates re-run between 1 and 2 per the task)

1. **Refactor first**: extract `perform_adjust(dps, actors, plan)` from
   `perform_seek` (everything except `reset_side_state`); `perform_seek`
   = adjust + reset_side_state. Run FULL gates (harness + check) before
   proceeding.
2. `seek::blocks_to_wall_ms` + tests; `section_math::pre_shift_wall_ms`
   + tests.
3. `song_reset::adjust_run_to(t_q_ms, lead_wall_ms)`: seek-identical gate
   set (available/scene/DPS step 7/course/actors step 4/end-cascade clamp
   incl. `t_q < min_end − SEEK_END_MARGIN_MS`), snapshot-free (no
   accumulator block), `perform_adjust`, INFO + `notify_subscribers`.
   Plus `first_anchored_frame()` (DPS 7, actors 4, anchors +0x160 ≠ 0)
   and `GPA_ANCHOR_OFFSET`.
4. Pre-shift arming in training_mode (mod.rs `refresh_pre_shift`, called
   from the skip row's on_change + scene 25/26 entries via
   bounds::on_scene_change; entered-side choice per context.md; effective
   audio clamp composed; clear on skip 0 — disable already clears).
5. `driver.rs`: generation-tokened render-thread loop; arms at GAMEPLAY
   entry iff resolution pending OR mapping shift > 0; per frame: retry
   `try_resolve_row_bounds`, then (when a shift was requested and not yet
   adjusted) wait for `first_anchored_frame` → one-shot adjust with the
   mapping read-back derivation + fallback ladder; 60 s soft timeout WARN;
   scene exit/generation bump kills the loop.
6. Full gates + fmt + build.sh (the Step-3 sequence close).
