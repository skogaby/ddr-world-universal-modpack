# Task: parts, mirrored forearm, shadow — session / director / lifecycle wiring

## Description
Complete the A3 character assembly in the per-song lifecycle: the pick's arc set gains, per dancer, every
part arc that RESOLVES (`pl_<key>_{head00,hips00,chest00,forearm00,face01}.arc`; the game's `%s_%s00.arc`
rule, faces 02/03 never shown) plus `pl_shadow00.arc` once; the parse thread reads the body's `.b2it`
(`data/chara/pl_<key>/pl_<key>.b2it`) for the attach bones (`head00`/`face01` → `Head`, `hips00` → `Hips`,
`chest00` → `Spine2`, `forearm00` → `LeftForeArmRoll` AND a second, MIRRORED instance on
`RightForeArmRoll` sharing the same model resource) and the ground set `{Hips, Spine2, Head, LeftToeBase,
RightToeBase}`, and each part's `.model` bone table (1 bone); the `Session` instance table grows
`InstanceKind::Part { dancer, part }` and `InstanceKind::Shadow(dancer)` (build order: stage parts,
dancers, parts, shadows — all pass mask 2); the director evaluates each dancer's bones ONCE and derives its
parts (`part_world`) and shadow (`shadow_target`/`shadow_step`/`shadow_world`, tint `(0,0,0,1)`) from
them, publishing every child instance with a single identity bone; the shadow low-pass resets on every
clock (re)latch. Missing part arcs / bone names are skipped silently (one summary line in the parse
warnings at most). Stage `:N` priorities are already in place since Step 7 (`PASS_MASK_LOWPRIO` + sort
key) — verify on a `boom00` row-0 pick, do not redo.

## Background
`docs/3d_model_format_research.md` §8: `FUN_18005e560(actor, part, bone_index, extra)`; a missing part arc
is harmless (the node is destroyed when the resource is null). World's install ships 26 `face01`, 5
`head00`, 3 `chest00`, 2 `hips00`, 2 `forearm00` arcs (`rinon00` has all seven, `afro00` head only,
`emi00` faces only) — the pick must not WARN per missing part. Every part model and `pl_shadow00` are
single-bone rigid models (bind identity; verified with `ktmdl_dump.py` on `pl_rinon00_*` /
`pl_shadow00`), so `render_item::build` takes the rigid path (mode 0xB) and the frame board carries
`[IDENTITY]` as their bone array; the placement lives entirely in the published world matrix. Shadow quad =
1×1 on y = 0. The forearm-R instance's model name equals forearm-L's — `model_registry::model_resource`
returns the same resource, `render_item::build` still creates a private item per instance. Frame board:
`MAX_INSTANCES = 32` ≥ 8 stage parts + 2 × (1 body + 6 parts + 1 shadow) = 24.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-16-enable-background-dancers/design/detailed-design.md` (§4.3.4, §4.3.5,
  §5.4, §6 error table)
- Plan: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md` Step 8
- Task 01 of this step (the math)

**Additional References (if relevant to this task):**
- `src/mods/background_dancers/session.rs` (`Pick::arcs`, `parse_pick`, `Session::new`, `initial_world`,
  `build_pending`), `director.rs` (`produce`), `lifecycle.rs` (`drive_live` clock events), `selection.rs`
  (`DancerCandidate`), `src/core/anm/b2it.rs` (`parse`, `index_of`), `src/core/anm/pose.rs` (`Skeleton`)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `selection.rs`: `pub const PART_NAMES: [&str; 5] = ["head00", "hips00", "chest00", "forearm00", "face01"]`;
   `pub fn part_attach_bone(part: &str) -> Option<&'static str>` (`head00`/`face01` → `Head`, `hips00` →
   `Hips`, `chest00` → `Spine2`, `forearm00` → `LeftForeArmRoll`); `pub const MIRROR_ATTACH_BONE: &str =
   "RightForeArmRoll"`; `pub const GROUND_BONES: [&str; 5]`; `DancerCandidate::part_arc_name(part)` /
   `part_model_name(part)`; `pub const SHADOW_ARC: &str = "pl_shadow00.arc"`, `SHADOW_MODEL: &str`.
2. `session.rs`: `Pick` gains `parts: Vec<Vec<String>>` (per dancer, the part names whose arc resolved at pick
   time — `assemble_pick` takes an `exists` closure like `stage_candidates`); `Pick::arcs()` appends the part
   arcs and `pl_shadow00.arc` (dedup); `summary()` shows `parts=[n,…]`. `ParsedPart { part, model_name,
   attach: u32 /*bone index*/, mirror: bool, skeleton: Skeleton }`; `ParsedDancer` gains `parts: Vec<ParsedPart>`,
   `ground: Vec<u32>`, `hips: Option<u32>`, `shadow_scale: f32`; `Parsed` gains `shadow: Option<Skeleton>`
   (`pl_shadow00.model` bone table). `parse_pick`: read `.b2it` (missing/unparseable ⇒ no parts, no shadow
   for that dancer, one warning); for each picked part: open the arc, parse the model bone table (must be
   ≥ 1 bone), look up the attach bone (missing name ⇒ skip + warning); `forearm00` produces TWO
   `ParsedPart`s (L, and R with `mirror = true` on `RightForeArmRoll` when that name exists).
   `InstanceKind::{StagePart(usize), Dancer(usize), Part { dancer: usize, part: usize }, Shadow(usize)}`;
   `Session::new` pushes parts (pass mask `PASS_MASK_DANCER`, sort 0) then one shadow per dancer with a
   ground set (needs `Parsed::shadow`); `Session` gains `shadow_size: Vec<f32>` (per dancer, seeded to
   `shadow_scale`) + `pub fn reset_shadow(&mut self)`; `initial_world`: Part = `part_world(mirror,
   &skeleton.bind_world[attach], &body_world)`, Shadow = `shadow_world(shadow_scale, body_world(centre of
   the bind ground points))`. The instance's `model_name` for the R forearm is the SAME model name (the
   build path already handles it); the log line still says which instance (`kind` in the built line).
3. `director.rs::produce`: stage parts as before; then per dancer: evaluate bones once → publish body →
   for each Part instance of that dancer: `world = part_world(mirror, &bones[attach], &body_world)`,
   publish `(world, WHITE, hidden, &[IDENTITY])` → Shadow: ground points = `bones[g][12..15]` with `y :=
   0.02`, `Δ = bind_world[hips][13] − bones[hips][13]`, `(centre, target) = shadow_target(…)`,
   `size = shadow_step(prev, target)` stored back, `world = shadow_world(size, transform_point(&body_world,
   centre))`, publish `(world, BLACK, hidden, &[IDENTITY])`. Precompute `children: Vec<Vec<usize>>` (instance
   indices per dancer) in `Session::new` so the loop never re-scans. `hide_all` unchanged (covers every Built
   instance).
4. `lifecycle.rs`: on `ClockEvent::Latched` call `sess.reset_shadow()` (A3: low-pass reset at song start).
5. Diagnostics: the per-song pick INFO shows the part counts; the `built` INFO counts parts/shadows
   (`N instance(s) attached hidden (S stage, D dancer, P part, W shadow)`); a `.b2it` miss or a bone-name miss
   is ONE parse warning per dancer (existing `parse:` WARN path).
6. Gates: `cargo check --target x86_64-pc-windows-msvc` → `cargo fmt` → `./build.sh` →
   `./scripts/validate_background_dancers.sh`.

## Dependencies
- Task 01 (math).

## Implementation Approach
1. `selection.rs` constants/helpers → `session.rs` (pick, parse, instances, worlds) → `director.rs`
   (restructured produce) → `lifecycle.rs` (reset hook) → gates.
2. Hand the maintainer the deploy checklist (progress.md "Step 8 deploy #1 expected lines").

## Acceptance Criteria
1. **Cabinet (maintainer deploy)**
   - Given `mods["background-dancers"]: true` and `DDR_DANCERS_PIN` (developer_mode) forcing `rinon00`,
     then `afro00`, `babylon00`, `emi00`, then random picks
   - When songs are played
   - Then `rinon00` wears head/hips/chest/both forearms/face (the right forearm mirrored correctly — thumb
     side matches the left), `afro00` its head only, `babylon00` (scale 0.4) its face at the right size,
     `emi00` its face; a soft dark shadow follows each dancer's feet, growing on crouches and shrinking on
     jumps; `boom00` row 0 (`bg:-2`, `stage:-1`) draws its `:N` parts behind everything else; no new WARNs

2. **Fail-open**
   - Given a dancer whose `.b2it` is missing
   - When the song plays
   - Then the body dances without parts or shadow and one `parse:` WARN names the file

3. **Gates**
   - Given the finished change
   - When the four gates run
   - Then all are clean/green

## Metadata
- **Complexity**: Medium
- **Labels**: background-dancers, step-8, session, director, shadow
- **Required Skills**: the repo's scene3d/frame_board model, the parse-thread shape
- **Generated By**: code-task-generator 2026-09-16
- **Source Plan**: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md`
- **Plan Step**: Step 8: Character assembly completeness — parts, mirrored forearm, shadow, stage priorities
