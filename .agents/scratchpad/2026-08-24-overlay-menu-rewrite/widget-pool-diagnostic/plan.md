# Plan — widget-pool-diagnostic

Status: Approved 2026-08-24 (auto mode; verified approval chain per context.md)

## Test scenarios

No pure layer exists to host-test (the walk is raw game-memory reads; the crate's
engine-facing code has no host harness). Validation:

1. Compile gates: `cargo check` (0 warnings) → `cargo fmt` (no churn) → `./build.sh`.
2. Autonomous CrossOver boot run (maintainer-authorized): launch the game, let it reach
   attract, kill it, and grep the log for exactly one
   `WidgetRenderer: render list free pool:` INFO line with a plausible count (AC-1).
3. AC-2 (unavailable path) is exercised by code review of the guard ladder — it cannot
   be forced on a healthy install without breaking derivation.

## Implementation approach

- `static POOL_DIAG_LOGGED: AtomicBool`; one-shot check at the top of
  `wrapper_render_hook` (cost after first frame: one relaxed atomic load).
- `fn log_free_pool_count_once()`: poison-recover lock → copy `scene_manager_global` →
  unsafe walk (null-guard every hop, cap 4096) → single INFO (count, cap-exceeded, or
  unavailable reason).
