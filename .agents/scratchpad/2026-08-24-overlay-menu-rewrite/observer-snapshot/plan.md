# Plan — observer-snapshot (Step 5 task-03)

Status: Approved 2026-08-24 (auto mode — verified chain + in-session
breakdown approval)

## Approach (in order)
1. observers.rs (new): subscribe/unsubscribe/dispatch + in-file tests
   (tokens, unsubscribe, multi-fire order, panic containment, subscribe-
   during-dispatch no-deadlock) — red via missing impl, green.
2. registry.rs: `show_when_satisfied` method (rows.rs free fn removed, call
   sites updated); OverlayRowKind/OverlayRowInfo pub types; pure
   `overlay_snapshot_rows(state, side, overlay_override, order_for)`
   composition + per-row builder; in-file tests (snapshot matrix + mutation-
   primitive tuple assertions).
3. mod.rs: `mod observers;` + pub use (subscribe/unsubscribe, snapshot
   types); observer dispatch wired at the 5 facade sites (set_value_silent
   dispatches observers ONLY); `overlay_snapshot(side)` facade.
4. rows.rs: press_body observer dispatch after the cb call.
5. Harness MODULES=(api.rs observers.rs ordering.rs registry.rs).

## Test scenarios
- Observers: unique tokens; unsubscribed stops; A(panic)+B ⇒ B fires + one
  latched WARN; subscriber subscribing during dispatch completes.
- Snapshot matrix (synthetic FrameworkState via try_register): availability
  drop; overlay-placement drop (override false beats registration true, and
  override true beats registration false); order permutation honored
  (injected order_for reversing); per-side ShowWhen visible flag
  (Equals/NotEquals, both sides, unknown parent fail-open ⇒ visible);
  live bounds after set_scalar_bounds reflected in Scalar kind; formatted
  parity across all 7 ScalarFormat variants (== format_scalar_value_utf8);
  display fallbacks (name=prettify_id, desc="", enum label=prettify suffix,
  explicit strings pass through); bool-vs-enum detection (bool_toggle ⇒
  Bool; custom 0/1 enum with other textures ⇒ Enum); Header kind.
- Mutation tuples: set_value Some-on-change/None-on-no-change;
  reset_session_values only non-default Session rows; set_scalar_bounds
  clamp tuples only for out-of-range sides.

## Risks
- Shared static subscriber list across parallel tests — serialized via a
  tests-module lock + per-test unsubscribe.
- pub(crate) FrameworkState visibility in the harness — same-crate tests, ok.
