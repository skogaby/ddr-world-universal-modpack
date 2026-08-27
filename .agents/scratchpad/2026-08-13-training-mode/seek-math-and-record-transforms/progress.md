# Progress: seek-math-and-record-transforms

## Checklist
- [x] Directory conversion song_reset.rs → song_reset/mod.rs (git mv; behavior-neutral; check green before the pure module landed)
- [x] seek_tests.rs written + harness mount (failed first: module absent)
- [x] seek.rs implemented (layout constants, quantize_seek, anchor_tick, decode_notes, rebuild_expectations, neutralization_writes)
- [x] Full harness suite green (209 passed / 0 failed; 7 new)
- [x] cargo check clean; cargo fmt (suite re-verified after)
- [x] Close record (uncommitted — maintainer handles git)

## Record
- 2026-08-13: Setup + Explore complete (baseline 202/202 from task-01).
- 2026-08-13: Directory conversion via `git mv` — no call-site changes
  (module path unchanged); `pub mod seek;` + `#[cfg(test)] mod seek_tests;`
  added to `song_reset/mod.rs`.
- 2026-08-13: TDD cycle — 7 tests written first (compile-failed on the
  absent module), then `seek.rs`: note (0x60) + record (0x40) layout
  constants defined ONCE here (task-03's engine caller consumes them);
  `quantize_seek` (frame-domain floor to the block grid, `[0, max_blocks]`
  clamp, half-up ms of the grid point); `anchor_tick` (`now + delay −
  wall(T_q)`, tick_domain-selector identity pin, conversion-failure
  fallback); `decode_notes`; `rebuild_expectations` (per-kind semantics
  incl. shock shape + the kind-2 pre-T back-patch); `neutralization_writes`
  (strict spanning test, head hold-progress + end grade 6/judgedAt as
  byte-offset i32 writes). Harness gained the `services::song_reset::seek`
  mount (pure submodule only — mod.rs stays engine-facing, like
  song_rate's cfg(windows) io hooks).
- Gates: harness 209/209 → check clean → fmt (re-verified).

## Deviations
- `anchor_tick` takes a `delay_ms` parameter beyond the AC's
  `now − content_to_wall_ms(T_q)` shape: the shipped `perform_reset`
  future-dates the tick with `+ anchor_delay_ms`, and the pure fn composes
  both terms (`now + delay − wall(T_q)`) so task-03 has one anchor site.
  AC-2 is asserted at delay 0 (the exact AC shape) plus a delay-composition
  case. Additive, not a behavior change.

Status: Complete (uncommitted — maintainer handles git)
