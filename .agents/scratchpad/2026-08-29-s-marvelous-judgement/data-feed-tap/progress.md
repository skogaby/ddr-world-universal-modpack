# Progress — task-02 data-feed-tap

Status: Complete (uncommitted — maintainer commits manually)

## Checklist
- [x] `ACTOR_COMBO_OFFSET = 0x1DC` constant beside the side offset
- [x] S-Marvelous block at the top of the `player_side <= 1` branch (before
  the has_ms_error split ⇒ all grades 0..=6 reach the state machine; O.K.
  passes `ms: None`)
- [x] Module doc updated (names the S-Marvelous tap as a hosted consumer)
- [x] cargo check --target x86_64-pc-windows-msvc clean (logs/check.log)

## Review against acceptance criteria
1. Disarmed cost: the block is a single `is_armed` relaxed load + branch. ✔
2. Armed classification: values plumbed verbatim into the task-01-tested
   `on_judge_event`. ✔
3. All grades reach the machine: block precedes the has_ms_error split;
   `ms` recomputed locally with the identical predicate
   (`judge_code != OPCODE_OK && !scratch.is_null()`). ✔
4. No regression: calibration tap, buffers, CSV, widget update untouched;
   type check clean. ✔ (Live confirmation lands with task-03's cabinet demo.)

## Deviations
- None. Commit step skipped per repo AGENTS.md git rules.
