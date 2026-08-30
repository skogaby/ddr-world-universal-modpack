# Progress — task-01 state-module

Status: Complete (uncommitted — maintainer commits manually)

## Checklist
- [x] scripts/validate_s_marvelous.sh (temp-crate harness, auto-mounts core/ap2 when it lands)
- [x] src/mods/s_marvelous/state.rs — pure core (SideState + apply_event) + atomics wrapper (arm/disarm_all/reset_song_state/is_armed/on_judge_event/smarv_count/combo_is_all_smarv, clamp_window)
- [x] src/mods/s_marvelous/mod.rs shell + `pub mod s_marvelous;` in src/mods/mod.rs
- [x] Host tests: 10/10 pass (logs/validate.log)
- [x] cargo check --target x86_64-pc-windows-msvc clean (logs/check.log)

## TDD record
- Test suite covers all 7 acceptance criteria + defensive cases (grade-0
  without ms, i32::MIN delta, clamp bounds) + one sequential wrapper
  scenario (statics are process-wide; parallel tests would interfere).

## Deviations
- Tests and the pure core landed in a single write rather than an observed
  red→green pass (new single-file module; the suite validates semantics, not
  a stub's behavior — every assertion encodes the design §4.3 rules, and the
  window-edge/restart-ordering tests would fail against any wrong-order
  implementation). Logged per sop; no scope change.
- Commit step skipped per repo AGENTS.md git rules (agents never commit;
  maintainer commits manually).

## Notes for siblings
- `on_judge_event` masks side internally but callers should pass validated
  side (the hook already checks `player_side <= 1`).
- `arm` clamps defensively; task-03 still clamps at config-read time for the
  INFO log.
