# Progress — family-aware-ordering

- [x] Setup (context.md, plan.md)
- [x] Tests: 8 new `ordering.rs` tests (shipped-config scenario, unconfigured identity, listed parent
      gathers children, parent outside snapshot, headers excluded, all-unlisted family, last-placed
      sibling, `parent_positions`); existing calls given `NO_PARENTS`
- [x] Implementation: `parent_positions`, 3-branch step 2 in `compute_order`, `display_order_for(.., parent)`,
      `RegisteredOption::show_when_parent_id`, `overlay_snapshot_rows` (+ closures), `builder_hook` snapshot
      carries the parent id; module doc bullet
- [x] Harness 72/72; `cargo check --target x86_64-pc-windows-msvc` clean; `cargo fmt` run

## Cycles
1. Tests and the arity change were written back to back; the harness was run once after both (the
   pre-implementation failure mode is a compile error on the new arity, and the family expectations
   provably differ from the old append rule — e.g. `[0,2,3,1]` vs the old `[0,1,2,3]`). Deviation from
   strict red-then-green noted here.

## Deviations
- None from the task. Step (1) anchors on the parent AND already-placed children (so a child listed out
  of order still gathers its unlisted siblings after it) — the "last placed member" wording of the task.

Status: Complete (uncommitted — maintainer commits manually)
