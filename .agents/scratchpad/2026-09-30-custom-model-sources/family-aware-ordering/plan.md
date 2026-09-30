# Plan — family-aware-ordering

Status: Approved 2026-09-30 (auto mode — verified upstream approval chain, see context.md)

## Test scenarios (`ordering.rs` tests module)
Existing tests get a `NO_PARENTS` slice (all `None`) — their expectations are unchanged.
New:
1. `family_shipped_config_scenario` (AC1): 12 registered ids, parents for the 6 model rows, configured
   list of 7 ⇒ the exact 12-index order from the task.
2. `family_unconfigured_is_identity` (AC2): parents set, `None` config ⇒ identity minus headers.
3. `family_listed_parent_gathers_unlisted_children` (AC3): `[p, c1, c2, q]` registered `[p, q, c1, c2]`,
   configured `[p, q]` ⇒ `[0, 2, 3, 1]`.
4. `family_parent_outside_snapshot_appends` (AC4): child with `None` parent ⇒ appended.
5. `family_rule_leaves_headers_excluded` (AC5): unlisted header + unlisted family with a listed child
   ⇒ header absent, parent before child.
6. `parent_positions_maps_ids`: `["a","b","c"]`, parents `[None, Some("a"), Some("zz")]` ⇒
   `[None, Some(0), None]`.
7. `family_unlisted_parent_and_children_all_unlisted_append_in_order`: nothing of the family listed,
   config lists something else ⇒ family appended in registration order (branch 3 then branch 1).
Overlay snapshot tests: closures gain the third parameter; one new test
`overlay_family_order_injected` is not needed (the order callback is injected; the real rule is
covered above) — only signature updates.

## Implementation
- `ordering.rs`: `parent_positions`; `compute_order(.., parent, ..)` step 2 replaced by the
  3-branch loop with a `position_of(order, idx)` helper; `display_order_for(ids, is_header, parent)`;
  doc bullets.
- `registry.rs`: `RegisteredOption::show_when_parent_id(&self) -> Option<&str>`;
  `overlay_snapshot_rows` builds `parent_ids` → `parent_positions` → passes to `order_for`.
- `builder_hook.rs`: snapshot tuple gains `Option<String>` parent id; compute positions; pass through.
- `mod.rs::overlay_snapshot` passes `&ordering::display_order_for` — signature follows.
