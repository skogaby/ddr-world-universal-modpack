# Progress: seek-to-t-transaction

## Checklist
- [x] Pure conversion helpers (wall_ms/content_ms) + test (TDD — failed first on absent symbols)
- [x] Signatures: judge_rebuild_anchor AOB + derive_judge_rebuild_trio + control_message_actor_vtable (RTTI table row)
- [x] runtime/binding: active_content_grid + active_content_mapping (+ ContentGrid)
- [x] song_reset: AccumulatorPolicy + seek gates + SeekPlan + request_seek + seek_driver_step + plan_side_rebuilds + perform_seek + reset_side_state extraction + revalidated_world + notify_subscribers(t) + chart_end_raw/seek_available accessors + clear_content_mapping_if_shifted (t=0 leftover-shift guard)
- [x] quick_restart_or_fail call-site update (AccumulatorPolicy::Zero)
- [x] Full harness suite green (210/210); cargo check clean; cargo fmt
- [x] Close record (uncommitted — cabinet validation rides task-04's demo)

## Record
- 2026-08-13: Setup + Explore complete. Live Ghidra verification on
  20260721 + 20260616: rewind-worker anchor pattern
  (`48 89 91 60 01 00 00 C7 81 90 01 00 00 FF FF FF FF`) unique and the
  whole trio region byte-identical across builds (one rip-disp32 differs);
  trio shapes confirmed (clear = end-reset, reserve = game-heap realloc
  0x40-stride, rebuild(out², begin, end, &{actor, playhead_i32}) writes 2
  qwords to out); CMA cascade offsets confirmed (+0x58/+0x82 step machine,
  +0x94 display fires step 3→4, +0x98 raw fires step 4→5 — refuse at
  step ≥ 4). Rebuild-worker decompile confirms task-02's pure model.
- 2026-08-13: Implementation. Derivation pins call 1 via the
  `LEA RCX,[R12+0xB0]`+E8 pair (records-vector layout pin), then the next
  two E8s; validates 3 distinct in-module targets; fail-open (t=0
  unaffected). Transaction: preflight (trio/CMA/binding grid/cascade
  step/end clamp MARGIN 1000 ms) → stop → set_content_mapping(shift, lead)
  → replay → seek driver (prepare poll → revalidate → perform_seek:
  pre-validated side plans, 0x1043 + back-dated 0x1044
  (`seek::anchor_tick(now, delay_wall_q, t_q, rate)`), trio at t_q,
  bounds-checked R14 neutralization writes, shared Zero block,
  notify(t_q)).

## Deviations
- **Wall-domain quantization at rate** (recorded in context.md §Domain-math):
  the mapping's blocks live on the SERVED-stream grid, so the seek
  quantizes `wall(t)` on that grid and derives the content playhead
  `t_q = content_ms(W_q)` (new pure inverse helper; ≤1 ms round-trip slop
  host-pinned). Identity collapses to the design's B(T) letter exactly.
- **Delayed seeks skip the countdown driver**: the mapping's silent lead
  IS the approach (replay immediately; single prepared→anchor adjacency,
  anchor = now + lead_q − wall(t_q)). The shipped t=0 countdown is
  unchanged. This is task req 6's composition, not the t=0 protocol.
- **t=0 leftover-shift guard**: a t=0 restart after a prior seek on the
  same song clears a non-{0,0} mapping between stop and replay
  (`clear_content_mapping_if_shifted`). Strictly a no-op when no seek
  ever happened (no binding read → no seqlock churn) — the shipped
  path stays bit-identical.
- **`ResetOutcome::Unsupported` retained** (API stability) but no longer
  returned; negative `t_ms` refuses.

## Cabinet validation (pending — task-04's Step-2 demo)
AC-1 (t=0 unregressed), AC-3 (seek lands exactly at 100/75/125 %),
AC-2's live Refused legs, AC-4 (Zero policy) — all validated via the
Step-2 demo deploy. Host-covered: conversion helpers, quantize/anchor
math, neutralization planner (task-02), trio derivation shape (static
cross-build evidence).

Status: Complete (uncommitted — maintainer handles git)
