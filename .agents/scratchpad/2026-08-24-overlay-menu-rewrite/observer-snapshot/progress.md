# Progress — observer-snapshot (Step 5 task-03)

## Checklist

- [x] observers.rs — token multicast, snapshot-clone dispatch, per-subscriber
      catch_unwind + latched WARN; 4 in-file tests (tokens/unsubscribe,
      subscription-order fire, panic containment, subscribe-during-dispatch
      no-deadlock via a test-serializing lock)
- [x] registry.rs — `FrameworkState::show_when_satisfied` (moved from
      rows.rs; shared evaluator for both menus); pub `OverlayRowKind`/
      `OverlayRowInfo`; pure `overlay_snapshot_rows(state, side,
      overlay_override, order_for)` composition (filter available ∧ resolved
      overlay, then order — builder_hook's shape) + per-row builder (bool
      detection = exact bool_toggle signature; display/label fallbacks;
      live bounds; formatted via format_scalar_value_utf8); 10 in-file tests
      incl. the mutation-primitive tuple assertions
- [x] mod.rs — `mod observers;` + pub use (subscribe/unsubscribe + snapshot
      types); observer dispatch at set_value / set_value_silent (observer
      only, no on_change) / resolve_from_load / reset_session_values /
      set_scalar_bounds; `overlay_snapshot(side)` facade (one STATE lock,
      ordering adapters injected)
- [x] rows.rs — press-path observer dispatch after the cb call;
      show_when call sites moved to the registry method
- [x] Harness MODULES=(api.rs observers.rs ordering.rs registry.rs) — 40/40
- [x] Gates: check 0 warnings → fmt → build clean; sibling harnesses 23+12
- [x] Boot regression (inert — no subscribers exist): 6 pre-existing WARNs
      only, 0 panics, 41 registrations

## TDD note

The one red-phase failure was LOAD-BEARING: my test assumed the `set_value`
primitive was changed-only — it is UNCONDITIONAL (facades dedupe). Blindly
wiring observers at set_value_silent/resolve_from_load would have spammed
the overlay mirror on every unchanged per-frame re-seed. Fix: observer
gated on actual change at those two facades (set_value_silent now
early-returns on no-change; resolve_from_load keeps its always-fire
on_change contract but observers fire changed-only); test amended to pin the
primitive's real contract.

## Deviations

- None from the task spec (the observer changed-only gating is the task's
  own "changed only semantics inherited" requirement, landed at the facade
  rather than the primitive to preserve resolve_from_load's existing
  on_change behavior).

Status: Complete (uncommitted — maintainer commits manually)
