# Task: `instance_plan.rs` (pure) + `Session::new(.., slot_base, item_pass_mask)` + `Session::with_schedule`

## Description
Give `Session` the two knobs the preview driver needs (design §4.8 / §5.3) — a frame-board slot base
(P1 previews 0, P2 previews 16; gameplay 0) and an item pass-mask override (the side's private node-mask
bit; gameplay `None` keeps the stock `2 / 4 / 0x10` masks) — plus a synthetic dance schedule fallback for
stage-only scenes (the camera event loop only needs cut times), and make the instance-table planning a
pure, host-tested function. NO gameplay behaviour change: `Session::new(.., 0, None)` builds exactly the
instance table it builds today (same order, same slots, same masks, same hull twins, same budget rule).

## Background
`Session::new` (`src/mods/background_dancers/session.rs`) builds the instance table in a fixed order —
stage parts (`:N` priority ⇒ `PASS_MASK_LOWPRIO` + sort key = priority, else `PASS_MASK_STAGE`), dancers,
each dancer's parts, each dancer's shadow (only when the dancer has ground bones AND the shadow skeleton
parsed), then — when the hull plan is non-empty — one `Hull { of, layer }` twin per restyle-eligible
non-skipped instance per plan layer, sharing the body's slot. Slots are the owner's index; owners past
`frame_board::MAX_INSTANCES` (32) are `Skipped` with `NO_SLOT` (one WARN). `restyle_allowed` excludes
the shadow and `_bg` skydome parts. `Instance.pass_mask` feeds BOTH `render_item::build` and
`node::new_node` (`visit(4)` re-stamps the item mask every frame from the node), so an override at plan
time is complete. The `DanceSchedule` (`schedule.rs`) is `None` without dancers, but
`director::camera_frame` needs one for the cut events; a stage-only preview supplies a synthetic
single-segment schedule (`STAGE_CUT_PERIOD_S = 9.0` ⇒ a cut every `9.0 − CUT_LEAD = 7.5 s`).

`session.rs` cannot be mounted by the host harness (`crate::` imports); the plan asks for host tests on
slot bases, the mask override and the synthetic schedule, so the table planning moves to a pure file.
`PASS_MASK_*` live in `services/scene3d/render_item_layout.rs` (a different crate path in the harness) and
`NO_SLOT` in `frame_board.rs` — the pure file takes the masks as a parameter and defines its own
`NO_SLOT` pinned equal by a `const` assertion in `session.rs`.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-21-background-dancers-selection-options/design/detailed-design.md`
  (§4.6 "preview/scene.rs", §4.8, §5.3 constants, §6 "> 16 instances")

**Additional References (if relevant to this task):**
- `.agents/planning/2026-09-21-background-dancers-selection-options/implementation/plan.md` (Step 4)
- `src/mods/background_dancers/session.rs` (`Session::new`, `InstanceKind`, `restyle_allowed`, `build_one`)
- `src/mods/background_dancers/schedule.rs` (`DanceSchedule::new`, `CUT_LEAD`, `segment_len`)
- `src/mods/background_dancers/outline.rs` (`HullPlan::layers`, `HullPlan::none`)
- `src/services/scene3d/frame_board.rs` (`MAX_INSTANCES`, `NO_SLOT`), `src/services/scene3d/node.rs`
  (`new_node` — the node stamps `item_pass_mask` every `visit(4)`)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. New pure file `src/mods/background_dancers/instance_plan.rs` (std + `super::selection::SHADOW_MODEL`
   only): move `InstanceKind` (+ `tag`, `owns_slot`), `InstanceStatus`, `Instance` and `restyle_allowed`
   here verbatim; add
   - `pub const NO_SLOT: u32 = u32::MAX;`
   - `#[derive(Clone, Copy)] pub struct PassMasks { pub stage: u32, pub lowprio: u32, pub dancer: u32 }`
   - `pub struct StagePartSpec { pub model_name: String, pub priority: Option<i32>, pub bone_count: usize }`,
     `pub struct PartSpec { pub model_name: String, pub mirror: bool, pub bone_count: usize }`,
     `pub struct DancerSpec { pub model_name: String, pub bone_count: usize, pub parts: Vec<PartSpec>,
     pub has_ground: bool }`, `pub struct PlanInput { pub stage_parts: Vec<StagePartSpec>, pub dancers:
     Vec<DancerSpec>, pub shadow_bone_count: Option<usize>, pub shadow_model: String }`
   - `pub struct Plan { pub instances: Vec<Instance>, pub children: Vec<Vec<usize>>, pub max_bones: usize,
     pub truncated: usize }`
   - `pub fn plan_instances(input: &PlanInput, masks: PassMasks, slot_base: u32, slot_budget: usize,
     hull_layers: usize, item_pass_mask: Option<u32>) -> Plan` reproducing today's loop exactly: owner
     slot = `slot_base + owner_index`; owners beyond `slot_budget` ⇒ `Skipped` + `NO_SLOT`
     (`truncated` = their count; the WARN stays in `session.rs`); `item_pass_mask = Some(m)` replaces
     EVERY instance's `pass_mask` (hull twins copy the body's, so they get `m` too); sort keys unchanged;
     hull twins only when `hull_layers > 0`, for each restyle-eligible non-skipped instance, `layer` in
     `0..hull_layers`, grouped per body in body order.
2. `session.rs`: `pub use super::instance_plan::{restyle_allowed, Instance, InstanceKind,
   InstanceStatus};` (every existing `super::session::InstanceKind` import keeps compiling);
   `const _: () = assert!(instance_plan::NO_SLOT == frame_board::NO_SLOT);`.
   `Session::new(pick, parsed, requested_at, tempo_opts, style, hulls, slot_base: u32, item_pass_mask:
   Option<u32>)` builds the `PlanInput` from `parsed`, calls `plan_instances(.., PassMasks { stage:
   PASS_MASK_STAGE, lowprio: PASS_MASK_LOWPRIO, dancer: PASS_MASK_DANCER }, slot_base,
   frame_board::MAX_INSTANCES - slot_base as usize, hulls.layers.len(), item_pass_mask)` and logs the
   existing "instances exceed the frame board" WARN when `truncated > 0` (same text). `scratch`/`bones`
   sized from `plan.max_bones` as today. New `pub fn with_schedule(mut self, fallback: DanceSchedule)
   -> Session` — installs `fallback` only when `self.schedule.is_none()`.
3. `schedule.rs`: `pub fn synthetic_schedule(period_s: f32) -> Option<DanceSchedule>` — one
   pseudo-dancer with one non-looping `ClipRef` of `period_s` (name `"synthetic"`), so
   `segment_len(k) == max(MIN_SEGMENT, period_s − CUT_LEAD)` for every `k`. Document that only the cut
   times are consumed (the camera event loop) — no dancer reads it.
4. `lifecycle.rs`: the one `Session::new(..)` call site passes `0, None`.
5. `scripts/validate_background_dancers.sh`: mount `instance_plan` (`src/mods/background_dancers/instance_plan.rs`).
6. Host tests: `instance_plan.rs` — (a) a fixture input (2 stage parts one with priority, 2 dancers with
   2 and 1 parts, both with ground bones, shadow present) with `slot_base 0`, budget 32, `hull_layers 0`,
   `None` ⇒ the exact order `[Stage(0) mask stage sort 0, Stage(1) mask lowprio sort -1, Dancer(0),
   Dancer(1), Part{0,0}, Part{0,1}, Part{1,0}, Shadow(0), Shadow(1)]`, slots `0..=8`, `children ==
   [[4,5,7],[6,8]]`; (b) `slot_base 16` ⇒ slots `16..`; (c) `item_pass_mask = Some(0x20)` ⇒ every
   `pass_mask == 0x20` incl. hull twins (`hull_layers 2`), hull count = 2 × (2 dancers + 3 parts + 1
   non-`_bg` stage part) and NO hull for a `_bg` stage part or a shadow, each hull's `slot == its body's`;
   (d) budget 4 ⇒ owners 4.. `Skipped` with `NO_SLOT`, `truncated == 5`, and no hull for a skipped body;
   (e) `restyle_allowed` cases. `schedule.rs` — `synthetic_schedule(9.0)` cuts at `7.5, 15.0, 22.5`
   (`cut_times(23.0)`), `dancer_count() == 1`; `synthetic_schedule(0.0)` still yields segments of
   `MIN_SEGMENT`.

## Dependencies
- task-01 of this step (`Pick.stage: Option<…>` — `Session::new` reads `pick.seed` only, so the tasks
  are independent in code but sequential in the working tree).

## Implementation Approach
1. Tests first in `instance_plan.rs` (build the fixture `PlanInput` by hand — no `Parsed` needed).
2. Move the types + `restyle_allowed`; write `plan_instances` as a line-for-line port of the current
   loops (keep the comments about deploy #4's `cargo fmt` reflow near the hull loop).
3. Rewrite `Session::new`'s body to build `PlanInput` and consume `Plan`; add `with_schedule`; add
   `synthetic_schedule` + its tests; update the lifecycle call site.
4. `cargo check --target x86_64-pc-windows-msvc`, `./scripts/validate_background_dancers.sh`, `cargo fmt`.

## Acceptance Criteria

1. **Gameplay table identical**
   - Given the fixture input, `slot_base 0`, budget 32, `hull_layers 0`, `item_pass_mask None`
   - When `plan_instances` runs
   - Then instances, slots, masks, sort keys and `children` equal the documented order (criterion 6a).

2. **Slot base**
   - Given the same input with `slot_base 16`
   - When `plan_instances` runs
   - Then owner slots are `16, 17, …` and hull twins carry their body's slot.

3. **Mask override**
   - Given `item_pass_mask = Some(0x20)` and `hull_layers 2`
   - When `plan_instances` runs
   - Then every instance's `pass_mask == 0x20` and the hull set is exactly the restyle-eligible bodies × 2.

4. **Budget**
   - Given `slot_budget 4`
   - When `plan_instances` runs
   - Then the first four owners have slots `base..base+4`, the rest are `Skipped` with `NO_SLOT`,
     `truncated == owners − 4`, and skipped bodies get no hull.

5. **Synthetic schedule**
   - Given `synthetic_schedule(9.0)`
   - When `cut_times(23.0)` is read
   - Then it is `[7.5, 15.0, 22.5]` (within 1e-4).

6. **Build + harness**
   - Given the crate
   - When `cargo check --target x86_64-pc-windows-msvc` and `./scripts/validate_background_dancers.sh` run
   - Then both are clean (`instance_plan` mounted, all tests pass) and `lifecycle.rs` calls
     `Session::new(.., 0, None)`.

## Metadata
- **Complexity**: Medium
- **Labels**: background-dancers, refactor, pure-layer, host-tested
- **Required Skills**: Rust, the repo's pure-module harness convention
- **Generated By**: code-task-generator 2026-09-21
- **Source Plan**: `.agents/planning/2026-09-21-background-dancers-selection-options/implementation/plan.md`
- **Plan Step**: Step 4: `Pick`/`Session` generalisation + `scene_window.rs` extraction
