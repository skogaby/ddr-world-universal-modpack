# Task: `session.rs` + `director.rs` — per-song assets (parse thread), instance table and the FrameState producer

## Description
The per-song asset layer and the pure pose producer of the Background Dancers mod (design §4.3.4–§4.3.5,
Step 7 scope = stage parts + dancer BODIES; parts/shadow are Step 8, the camanm camera Step 9):
- `session.rs`: the `Pick` (seed, stage row, dancer rows, playlists, camera lists), the arc list of a pick,
  a `Parsed` bundle produced by ONE std thread per song (`arc_set::read_bytes` → `core::arc::{parse,
  extract}` (LZ77) → `core::anm::{parse, ktmdl::bone_table}`), the `DanceSchedule` built from the parsed
  clip durations, the bind seeds (`pose::seed_local_trs`) per skeleton, and the instance table (`Instance
  { kind, model_name, pass_mask, sort_key, slot, node, item, bone_count, material_count, textures_pending
  }`) with `build_instances` (game thread: `model_registry` → `render_item::build` → `node::new_node(…,
  slot)` → `attach_under_root`, attached HIDDEN).
- `director.rs`: `produce(session, t_anim, visible)` — per dancer `DanceSchedule::at` → `clip_time` →
  `pose::evaluate_into` (scratch buffers, no per-frame allocation) → `frame_board::publish(slot, world =
  scale_translation(model_scale, dancer_x(i, n), 0, 0), tint white, hidden = !visible, bones)`; per stage
  part `clip_time(t, dur, loops)` on its `_play_loop` (or the bind pose when the part has none) →
  publish with identity world. Pure math (`instance worlds`, `hidden-before-edge`) host-tested.

## Background
Facts that shape this task: `arc_set::read_bytes` returns the WHOLE `.arc` file — members must be
extracted with `core::arc` (RE doc §3.2); 4 stock stage loops are PARTIAL, so stage parts MUST be seeded
from the bind table (`ktmdl::bone_table` on the `.model` member — the same bytes the engine converted);
the item's bone array holds MODEL-space bone world matrices; A3 dancer placement `x = (i − (n−1)·0.5)·1.6`,
body world = `diag(s,s,s,1)·T(x,0,0)` with `s` = rlist `model_scale`; stage parts at the origin with pass
mask 4 (`:N` parts `0x10`, sort key = priority); dancers pass mask 2, sort key 0. The residency gate from
Step 4 (`render_item::texture_readiness(res).still_default == 0`, 10 s timeout ⇒ build + per-frame
`retry_texture_resolve`) stays. Model names: `gm_<stage>_<part>`, `pl_<key>`. Clip member paths:
`data/chara/mc_<sex>/<clip>.anm`, stage loops `data/map/gm_<stage>_<part>/gm_<stage>_<part>_play_loop.anm`,
models `data/map/gm_<stage>_<part>/gm_<stage>_<part>.model` / `data/chara/pl_<key>/pl_<key>.model`.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-16-enable-background-dancers/design/detailed-design.md` (§4.3.2 arc set
  of a pick, §4.3.4, §4.3.5, §4.4, §6 error rows)
- Plan: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md` Step 7

**Additional References (if relevant to this task):**
- `docs/background_dancers_research.md` §3.2 (partial loops, `read_bytes` semantics), §2.6/§2.8/§2.9
- `src/core/anm/*` (Step 5 API), `src/mods/background_dancers/{selection,schedule}.rs` (Step 6),
  `src/mods/background_dancers/spike.rs` (the build path being generalised — `build_slot`)
- `.agents/planning/2026-09-16-enable-background-dancers/research/a3-runtime-rules.md` §5 (placement)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `session.rs`: `Pick { seed, stage: StageCandidate, camera_row: Vec<String>, dancers: Vec<DancerCandidate>,
   playlists: Vec<Vec<String>> }` + `Pick::arcs(&self) -> Vec<String>` (game-relative: `data/arc/mapset_
   <stage>.arc`, per dancer `data/arc/pl_<key>.arc`, per sex used `data/arc/mc_<sex>.arc`; deduplicated,
   order stable); `pub fn make_pick(rng, stages, dancers, n) -> Option<Pick>` (uses selection).
2. `Parsed { stage_parts: Vec<ParsedStagePart { part, model_name, skeleton, seed: Vec<Trs>, loop_clip:
   Option<Clip> }>, dancers: Vec<ParsedDancer { key, model_name, model_scale, skeleton, seed, clips:
   Vec<Clip> }>, warnings: Vec<String> }`, `Clip { name, bytes: Arc<Vec<u8>>, anm: Anm }`;
   `pub fn parse_pick(pick: &Pick) -> Parsed` (blocking; run on a `std::thread::spawn` by the lifecycle,
   result handed over through `Arc<Mutex<Option<Parsed>>>`; never touches the engine; a missing member is
   a warning and the instance is skipped — a missing body `.model` or an EMPTY playlist drops that dancer;
   every failure is a string in `warnings`, logged ONCE by the lifecycle).
3. `pub fn dance_schedule(parsed) -> Option<DanceSchedule>` from the dancers' clip durations/loop flags
   (only dancers with ≥ 1 clip).
4. `Instance { kind: InstanceKind::{StagePart(usize), Dancer(usize)}, model_name: String, pass_mask: u32,
   sort_key: i32, slot: u32, node: usize, item: usize, bone_count: usize, material_count: usize,
   textures_pending: usize, attached_at: Instant }`; `pub struct Session { pick, parsed, schedule,
   instances: Vec<Instance>, scratch: Vec<Trs>, bones: Vec<Mat4>, … }`; `build_pending(&mut Session,
   since_request_ms) -> BuildProgress { built, pending, skipped }` (game thread) — for every instance not
   yet built: `model_registry::model_resource(name)` → `ResourceView` → readiness gate → `render_item::
   build(view, pass_mask)` → set world/tint/hidden(true) → `node::new_node(item, pass_mask, sort_key,
   slot)` → `attach_under_root` (refusal ⇒ `destroy_unattached`, WARN, skipped); bone_count from the view
   must equal the parsed skeleton's or the instance is skipped with a WARN (a mismatched bone array would
   corrupt the upload).
5. `director.rs`: `pub fn produce(sess: &mut Session, t: f32, visible: bool)`: dancers — `pos =
   schedule.at(i, t)`, clip = `clips[pos.clip]`, `(ct, _) = clip_time(pos.local_t, dur, loops)`,
   `evaluate_into(anm, bytes, ct·fps, sk, seed, scratch, bones)`, `publish(slot, &body_world, WHITE,
   !visible, &bones[..n])`; stage parts — `loop_clip`: `(ct, _) = clip_time(t, dur, true)` → evaluate,
   else bones = bind (`seed` evaluated once at build, cached), `publish(slot, &IDENTITY, WHITE, !visible,
   …)`. Pure helpers in a `#[cfg(test)]`-mountable shape: `pub fn body_world(model_scale, i, n) -> Mat4`
   (= `scale_translation(s, dancer_x(i,n), 0, 0)`), `pub fn stage_world() -> Mat4`. NO allocation per
   frame (scratch sized at session build).
6. Tests (host, `director.rs`/`session.rs` pure parts — keep them free of `crate::` by putting the
   engine-facing build in `session.rs` and the pure pick/parse-shape helpers in a `session_pure.rs` or
   behind functions that take slices): `body_world` for n=1/2 (x = 0 / ∓0.8, scale on the diagonal),
   `Pick::arcs` dedupe (two female dancers ⇒ one `mc_female.arc`), `dance_schedule` refuses an empty clip
   list, a stage part without a loop keeps its bind bones (evaluate with the seed reproduces bind — reuse
   the Step 5 synthetic rig).
7. Rust Quality Rules: game-thread-only engine calls; `produce` is called from `on_frame` — keep it
   ≤ ~0.3 ms (2 dancers × 33 bones + ≤ 8 parts).

## Dependencies
- Task 01 (`frame_board`, `new_node(…, slot)`), Steps 5–6.

## Implementation Approach
1. `session.rs` types + `parse_pick` (pure over bytes) + `dance_schedule`.
2. `build_pending` (port `spike::build_slot`).
3. `director.rs` `produce` + pure helpers + tests; harness mount of the pure files.
4. `cargo check --target x86_64-pc-windows-msvc`; `cargo fmt`.

## Acceptance Criteria

1. **Parse bundle**
   - Given a pick of `boom00` row 0 + one female dancer and the stock install
   - When `parse_pick` runs (host, via the arcs read from `$DDR_WORLD_INSTALL` in a `#[ignore]`d or
     env-gated test)
   - Then 5 stage parts with skeletons (4 with loops, `stage` partial), one dancer with 13 clips and a
     33-bone skeleton, no warnings

2. **Producer**
   - Given a session with 2 dancers and t = 25.0 s
   - When `produce` runs
   - Then both dancers' slots hold 33 bone matrices for segment 1's clips at `local_t = 25 − 19.4`, worlds
     at x = ∓0.8 with the rlist scale, hidden = false, and the stage parts hold their loop poses at
     `25 mod dur`

3. **Gates**
   - Given the finished change
   - When `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`, `./scripts/validate_background_dancers.sh` run
   - Then all are clean/green

## Metadata
- **Complexity**: High
- **Labels**: background-dancers, step-7, session, director, parse-thread
- **Required Skills**: Rust threads/Arc/Mutex, the scene3d build path, `core/anm`
- **Generated By**: code-task-generator 2026-09-16
- **Source Plan**: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md`
- **Plan Step**: Step 7: Director, session and lifecycle — first fully animated random song
