# Task: Observer + overlay snapshot — value-changed multicast, `overlay_snapshot(side)`

## Description
Land the two APIs the PLAYER SETTINGS tab (Step 6) consumes: a token-based
value-changed observer (`subscribe_value_changed`/`unsubscribe_value_changed`,
`Arc<dyn Fn(&str, u8, i32)>`, panic-contained, never dispatched under lock)
wired into EVERY value-mutation path including the in-game press path, and
`overlay_snapshot(side) -> Vec<OverlayRowInfo>` — a plain-data snapshot
applying availability ⊕ placement (registration ⊕ config override, config
wins) ⊕ configured order ⊕ live scalar bounds ⊕ per-side ShowWhen, with
formatted scalar text identical to the in-game menu.

## Background
Step 5 of the overlay-menu rewrite (design §4.3.3–.4). No consumer exists yet
— the overlay doesn't call any of this until Step 6.

Mutation-path census (verified 2026-08-24; the tuples each primitive already
returns are the natural observer feed — "changed only" semantics inherited):

| Path | Registry primitive | Facade dispatch site |
|---|---|---|
| set_value | registry.rs:230-243 | mod.rs:290-292 |
| set_value_silent | same primitive | mod.rs:347 (currently DISCARDS the tuple — observer must fire here; on_change stays suppressed) |
| resolve_from_load | same primitive | mod.rs:324-326 |
| card-in reset | reset_session_values registry.rs:255-271 | mod.rs:395-397 |
| set_scalar_bounds clamp-writes | registry.rs:284-315 | mod.rs:423-425 |
| **in-game press** (approved 6th path) | none — rows.rs::press_body writes values[side] at rows.rs:2466, calls cb directly at :2476 | dispatch added after the existing cb call |

APPROVED DECISIONS: press path IS instrumented; registration-time prime
(mod.rs:252-253) does NOT fire the observer.

Dispatch-pattern precedents: `dispatch_callback` (mod.rs:636-648,
catch_unwind + on-panic callback replacement) and the lifecycle subscribers
(mod.rs:555-583, snapshot-clone-then-dispatch). The existing subscriber lists
are plain `fn` vectors — the observer needs `Arc<dyn Fn>` + usize tokens (no
precedent; new mountable module).

Snapshot inputs: `FrameworkState.options` (registry.rs:127-131; values
`[i32;2]` per side, availability flag :51, task-02's menus/display strings);
`is_show_when_satisfied` currently private in rows.rs:311-331 (pure vs
`&FrameworkState` — MOVE to a `FrameworkState` method in registry.rs, rows.rs
re-imports); ordering's `display_order_for` + `placement_override_for`
(task-01); `format_scalar_value_utf8` (task-02).

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-24-overlay-menu-rewrite/design/detailed-design.md (§4.3.3 observer, §4.3.4 snapshot, §5 OverlayRowInfo sketch)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. **New `src/services/custom_options/observers.rs`** (mountable: std +
   `crate::log_*` via harness stubs only):
   - `subscribe_value_changed(Arc<dyn Fn(&str, u8, i32) + Send + Sync>) -> usize`
     (monotonic token), `unsubscribe_value_changed(usize)`.
   - `pub(crate) fn dispatch(id: &str, side: u8, value: i32)`: snapshot-clone
     the subscriber list under its own lock, drop, then call each wrapped in
     `catch_unwind` (latched WARN per panicking subscriber; subsequent
     subscribers still run). NEVER called while any framework lock is held.
   - In-file tests: token uniqueness; unsubscribe stops delivery; multiple
     subscribers all fire in order; panicking subscriber contained + others
     still fire; a subscriber that calls subscribe during dispatch does not
     deadlock (proves no-lock-held dispatch).
2. **Wiring** — after each facade's existing lock-release dispatch:
   set_value, set_value_silent (observer only, on_change still suppressed),
   resolve_from_load, reset_session_values, set_scalar_bounds; plus
   rows.rs::press_body after its direct cb call (:2476). NOT register_option's
   prime. Every site passes the option id (the reset/bounds primitives already
   return ids; `set_value`'s primitive tuple may need the id added — keep the
   deferred-dispatch shape).
3. **Snapshot types** (api.rs or registry.rs, pub, plain data):
   `OverlayRowKind { Bool { value: bool }, Enum { index: usize, values:
   Vec<i32>, labels: Vec<String> }, Scalar { value, min, max, step_fine,
   step_coarse, formatted: String }, Header }`;
   `OverlayRowInfo { id: String, display_name: String, description: String,
   kind: OverlayRowKind, visible: bool }`.
4. **Snapshot builder** (pure, in registry.rs as a `FrameworkState` method so
   the harness tests it): inputs = `side`, the display-order permutation, and
   a per-id placement-override lookup (passed in — the OnceCell stays in
   ordering). Semantics:
   - Rows in configured display order (identical permutation source to
     builder_hook).
   - Availability filter (like in-game); placement filter on RESOLVED
     `overlay` (config override wins over `menus.overlay`).
   - `visible` = per-side ShowWhen evaluation (REPORTED, not filtered).
   - `display_name` = spec display_name else `prettify_id`; `description` =
     spec description else empty; enum labels = display_label else
     `prettify_texture_suffix(label_texture_name)`; bool detection = the
     `bool_toggle` shape (2 enum values 0/1 with OFF/ON labels) → `Bool`,
     other enums → `Enum`.
   - Scalar carries LIVE bounds (post-`set_scalar_bounds`) + `formatted` from
     `format_scalar_value_utf8` — identical text in both menus.
5. **Facade**: `pub fn overlay_snapshot(side: u8) -> Vec<OverlayRowInfo>` in
   mod.rs — one STATE lock, gathers ordering inputs, calls the builder,
   returns owned data. `pub use` the new types.
6. **Harness**: add `registry.rs` + `observers.rs` to
   `scripts/validate_custom_options.sh` MODULES (once_cell already a dep;
   registry mounts because `super::api` resolves against the mounted api.rs).
   Registry in-file tests: snapshot construction over a synthetic
   FrameworkState — availability drop, overlay-placement drop (override beats
   registration both directions), order permutation honored, ShowWhen
   visible-flag per side (Equals/NotEquals/unknown-parent fail-open), live
   bounds reflected after a bounds rewrite, formatted parity across all 7
   ScalarFormat variants, display-string fallbacks, bool-vs-enum detection;
   mutation primitives return observer-feed tuples for every path (the
   facade glue is cabinet-validated).
7. Panic-freedom: dispatch sites are hook-reachable (press path) —
   catch_unwind at the observers boundary per requirement 1; no unwrap in any
   new hook-reachable code.

## Dependencies
- task-01 (ordering placement/order queries) and task-02 (menus/display
  fields, format_scalar_value_utf8) — must land first.

## Implementation Approach
1. observers.rs + tests (red→green via harness).
2. Registry: show_when move + snapshot builder + types + tests.
3. Facade + wiring on all six paths; `pub use` exports.
4. Gates; autonomous boot: regression (menu opens, toggles work, no new
   WARNs) + a temporary dev assertion is NOT needed — observer has no
   subscribers yet, so the boot proves inertness.

## Acceptance Criteria

1. **Observer contract**
   - Given subscribers A (panics) and B
   - When any mutation path fires
   - Then B still receives the event, A's panic is contained with one WARN,
     and a subscriber subscribing during dispatch does not deadlock.

2. **All paths fire**
   - Given the six mutation paths
   - When each changes a value
   - Then the observer-feed tuples are produced for exactly the changed
     (id, side, value) triples (registry-level host tests) — and registration
     prime produces none.

3. **Snapshot correctness**
   - Given a synthetic registry with unavailable, overlay-hidden,
     ShowWhen-gated, bounds-clamped, and every-ScalarFormat rows plus a
     configured order
   - When `overlay_snapshot`'s builder runs for each side
   - Then rows appear in configured order, drops/visibility/bounds/formatted
     text match the in-game rules per the matrix above.

4. **Inert integration**
   - Given the DLL booted on the cabinet with no overlay consumer
   - When the in-game options menu is exercised
   - Then behavior and logs are unchanged (no new WARNs, no perf impact).

## Metadata
- **Complexity**: High
- **Labels**: custom-options, observer, snapshot, pure-layer
- **Required Skills**: Rust, repo custom_options conventions, host-test harness
- **Generated By**: code-task-generator 2026-08-24
- **Source Plan**: .agents/planning/2026-08-24-overlay-menu-rewrite/implementation/plan.md
- **Plan Step**: Step 5: custom_options extensions — placement, display strings, observer, snapshot, `option_menu_settings`
