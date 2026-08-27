# Plan — Step 4 Task 02: side selection + diagnostics

**Status: Approved** (auto mode — approval inherited from the verified approved plan/design chain
plus the maintainer-approved Step 4 breakdown, 2026-07-26)

## Verification scenarios

1. AC6 (solo regression, agent's): same chart as task 01 → same kept count (340), diagnostic
   line `siblings=1`, side 0 chosen, tick deltas unchanged.
2. AC4 (degraded mode): verified by inspection (all four bail-outs land in the same fallback
   path) — a forced-failure probe boot only if inspection leaves doubt.
3. AC1/AC2/AC3/AC5: maintainer's listening rows, log-corroborated by the diagnostic line.
4. AC7: walk runs only in the rebuild; per-dispatch adds one usize compare.
5. AC8: gates.

## Implementation shape

- `static GAMEPLAY_ACTOR_VTABLE: AtomicPtr<u8>`; stashed in `init` via `get_address` (missing →
  null → permanent degraded mode, warned once at first rebuild).
- `struct ActorInfo { actor: usize, side: i32, style: i32 }`.
- `fn enumerate_actors(dispatched: *mut u8) -> Option<Vec<ActorInfo>>` — None = degrade. Bounded
  walk (cap 64), containment check.
- `fn choose_actor(actors: &[ActorInfo]) -> ActorInfo` — doubles / solo / 2P-sort-by-side table.
- `rebuild_for`: enumerate (or degrade+warn once) → choose → `build_tick_list(chosen)` → latch
  `(actor usize, side)`; diagnostic folded into the build log line.
- `SongState`: `tick_side: i32` stays; add `tick_actor: usize` (0 = none/inert); `tick_clock`
  identity check becomes `actor as usize != song.tick_actor`.
