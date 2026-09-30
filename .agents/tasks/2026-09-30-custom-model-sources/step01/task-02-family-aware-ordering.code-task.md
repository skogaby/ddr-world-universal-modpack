# Task: Family-aware row ordering

## Description
Teach the custom-options display ordering to keep `ShowWhen` families together when the operator's
`option_menu_settings` lists only some of their members: an unlisted parent is placed immediately
before its first listed child, and unlisted children are placed immediately after the last placed
member of their family. Unconfigured ordering and every existing configuration without such families
are unchanged.

## Background
`ordering.rs::compute_order` places listed ids first and appends every unlisted non-header in
registration order. The shipped `mod-config.json` (and every existing install's) lists
`background_dancer` and `background_stage`. The new `background_dancer_source` /
`background_stage_source` rows (parents) and the per-source model rows (children) are unlisted, so
without this rule they would land at the bottom of the menu, separated from the rows they control.
Both callers — the in-game builder hook and the overlay snapshot — build parallel slices for the
ordering from a per-open snapshot of the registry.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-30-custom-model-sources/design/detailed-design.md` — §4.9
  "Family-aware ordering", §2.2 R12

**Additional References (if relevant to this task):**
- `.agents/planning/2026-09-30-custom-model-sources/research/framework-plumbing.md` §2

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `compute_order(registered, is_header, parent: &[Option<usize>], configured)` — `parent[i]` is the
   snapshot index of `registered[i]`'s `ShowWhen` parent, `None` when it has none or the parent is
   not in the snapshot. Rule for each unlisted non-header in registration order: (1) parent already
   placed ⇒ insert right after the last placed family member (parent or any placed sibling);
   (2) else one of its own children already placed ⇒ insert right before the first placed child;
   (3) else append. Listed placement, duplicate handling, unknown-id collection and the header rule
   (R10) are unchanged. The unconfigured fast path stays identity-minus-headers.
2. A pure helper `parent_positions(ids, parent_ids: &[Option<&str>]) -> Vec<Option<usize>>` maps
   parent ids to snapshot positions (exact match; ids are unique).
3. `display_order_for(ids, is_header, parent)` takes the new slice.
4. `builder_hook.rs` captures each snapshot entry's `ShowWhen` parent id under the registry lock and
   passes the computed positions; `registry.rs::overlay_snapshot_rows` does the same and its
   `order_for` callback type becomes `&dyn Fn(&[&str], &[bool], &[Option<usize>]) -> Vec<usize>`
   (test helpers updated).
5. `RegisteredOption` exposes `show_when_parent_id(&self) -> Option<&str>` (or equivalent) so both
   callers read the parent the same way.
6. The `ordering.rs` module doc lists the new rule beside the existing ones.

## Dependencies
- None (framework-internal). Independent of task-01.

## Implementation Approach
1. Write the `compute_order` tests first (scenarios below) in `ordering.rs`'s test module.
2. Implement the rule and the helper; thread the new parameter through `display_order_for`, the
   builder hook and the overlay snapshot; update the overlay snapshot tests' closures.

## Acceptance Criteria

1. **Shipped-config scenario**
   - Given registered ids in order `[hdr, premium_free, ddr_selection, background_dancer_source,
     background_dancer, background_dancer_a, background_dancer_b, background_stage_source,
     background_stage, background_stage_a, header_training, training_x]` with parents
     `dancer* → dancer_source`, `stage* → stage_source`, and a configured list
     `[hdr, premium_free, ddr_selection, background_dancer, background_stage, header_training, training_x]`
   - When `compute_order` runs
   - Then the order is `[hdr, premium_free, ddr_selection, background_dancer_source,
     background_dancer, background_dancer_a, background_dancer_b, background_stage_source,
     background_stage, background_stage_a, header_training, training_x]`

2. **Unconfigured identity**
   - Given any parents and no (or empty) configuration
   - When `compute_order` runs
   - Then the result is identity minus headers, byte-identical to today

3. **Listed parent, unlisted children**
   - Given a listed parent `p` followed in the list by `q`, and unlisted children `c1, c2` of `p`
   - When `compute_order` runs
   - Then the order is `[p, c1, c2, q]`

4. **Parent outside the snapshot**
   - Given an unlisted child whose `parent[i]` is `None`
   - When `compute_order` runs
   - Then it is appended as today

5. **Headers untouched**
   - Given an unlisted header and an unlisted family
   - When `compute_order` runs
   - Then the header is still excluded (R10) and the family rule applies to the rest

6. **Callers**
   - Given the overlay snapshot tests' `identity_order` / `reversed` / `listed` closures updated to
     the new signature
   - When `./scripts/validate_custom_options.sh` runs
   - Then every test passes, and `cargo check --target x86_64-pc-windows-msvc` is clean

## Metadata
- **Complexity**: Medium
- **Labels**: custom_options, framework, ordering
- **Required Skills**: Rust, custom_options service internals
- **Generated By**: code-task-generator 2026-09-30 (breakdown approved in conversation — maintainer authorised autonomous progression)
- **Source Plan**: `.agents/planning/2026-09-30-custom-model-sources/implementation/plan.md`
- **Plan Step**: Step 1: Framework — texture aliases + family-aware row ordering
