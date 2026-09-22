# progress — instance-plan-and-session-knobs (Step 4 task-02)
- [x] `instance_plan.rs` (pure): `InstanceKind`/`InstanceStatus`/`Instance`/`restyle_allowed` moved verbatim; `NO_SLOT`,
      `PassMasks`, `StagePartSpec`/`PartSpec`/`DancerSpec`/`PlanInput`, `Plan`, `plan_instances` (line-for-line port of
      `Session::new`'s loops: slot = `slot_base + owner_index`, owner budget, mask override at construction so hull twins
      copy it). 7 tests (order/slots/masks/children, slot base 16, mask override + hull set, budget truncation, restyle
      cases, missing shadow skeleton, stage-only / dancer-only shapes).
- [x] `session.rs`: re-export + `const _` NO_SLOT pin; `Session::new(.., slot_base, item_pass_mask)` builds `PlanInput`
      from `parsed`, budget = `MAX_INSTANCES − slot_base`, same WARN text on truncation; `with_schedule(fallback)`.
- [x] `schedule.rs`: `synthetic_schedule(period_s)` + test (9.0 ⇒ cuts 7.5/15/22.5; 0.0 ⇒ MIN_SEGMENT).
- [x] `lifecycle.rs`: `Session::new(.., 0, None)`; `mod.rs`: `pub mod instance_plan;`; harness mounts it → 141 ✓
      (`logs/harness.log`). `cargo check` clean, 0 warnings (`logs/cargo-check.log`); `cargo fmt` no unrelated churn.
## Deviations
- None. (The truncation WARN prints the owner budget, which equals `MAX_INSTANCES` for gameplay — identical text.)
Status: Complete (uncommitted — maintainer commits manually)
