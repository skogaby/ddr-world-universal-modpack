# Task: Multi-node spike — `pl_emi00` in bind pose + `pl_shadow00` on the boom00 footpanel

## Description
Extend the Step 3 harness from one node to a small scene: load `data/arc/pl_emi00.arc` and
`data/arc/pl_shadow00.arc` alongside `mapset_boom00.arc`; when a model is resident AND its material
textures are registered (`render_item::texture_readiness(res).still_default == 0`, or a 10 s timeout),
build its item and node: the footpanel (pass mask 4, identity), Emi's body (`pl_emi00`, skinned, pass
mask 2, world `diag(0.9, 0.9, 0.9, 1)` — the `emi00` rlist model scale — at the origin), and the shadow quad
(`pl_shadow00`, rigid, pass mask 2, world `scale_translation(0.75, 0, 0.02, 0)` — the rlist shadow scale at
the design's y = 0.02, tint `(0, 0, 0, 1)`). All nodes attach under the root; teardown handles N nodes
(all items unlisted → queue every destroy → all dtors → free node blocks → free the three arcs) and logs
the texture balance. This is the cabinet proof of the skinned pipeline on Windows AND CrossOver.

## Background
`pl_emi00.model`: 33 bones, 2 materials (`mdl_ch_lambert`), 2 draw records (32-slot palettes), one
texture `mdxemi01` (`mdx_emi01.dds`, 1.3 MB — registers AFTER the model converts, the Step 3 cold-load
finding), sphere centre `(0, 0.84, 0)` r 1.12. `pl_shadow00.model`: rigid 1×1 quad on the XZ plane at
y = 0, `mdl_bg_constant`, TRANS pass with alpha blend, texture `plshadow00`. rlist `emi00 = F, class A,
model_scale 0.9, shadow_scale 0.75`. With bones = bind the upload writes identity chains into the bone
texture and the skinning VS reproduces the bind pose. The footpanel from Step 3 stays as the floor
reference. Multi-node teardown reuses the Step 3 guard (item unlisted for 2 frames before the destroy
is queued) for EVERY item — the destroy vector buffer installed by `queue_destroy` on first use holds
256 entries.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-16-enable-background-dancers/design/detailed-design.md` (§5.2, §5.4 shadow rule — v1 spike uses a fixed size, §7.3 protocol item 3)
- RE record: `docs/background_dancers_research.md` §2.6–§2.7 (Step 3 findings: texture registration lag, destroy vector, teardown guard)

**Additional References (if relevant to this task):**
- `src/mods/background_dancers/spike.rs` (the harness to generalise), `src/services/scene3d/render_item.rs::texture_readiness`
- `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md` Step 4

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `spike.rs`: a `SPEC` table of the models to render — `{arc, stem, pass_mask, world: [f32;16], tint: [f32;4], sort_key}` for `gm_boom00_footpanel` (mapset_boom00, 4, identity, white, 0), `pl_emi00` (pl_emi00, 2, `scale_translation(0.9, 0,0,0)`, white, 0), `pl_shadow00` (pl_shadow00, 2, `scale_translation(0.75, 0, 0.02, 0)`, `(0,0,0,1)`, 0). `arc_set::load` takes all three arcs (one `ArcSet`). Residency poll covers the six boom00 models (Step 2 logging kept) + the two character models.
2. Per SPEC entry, build when `model_resource(stem)` is `Some` AND `texture_readiness(res).still_default == 0` OR 10 s have elapsed since the arc load (then build anyway with the per-frame retry from Step 3); build INFO per node (`… item built … bone_tex=… textures …`), `set_world_raw`/`set_tint_raw` before attach, `new_node(item, pass_mask, sort_key)`, `attach_under_root`. Camera written once (first node). Hide armed once.
3. `SceneState` holds `Vec<NodeSlot>` (`node, item, material_count, textures_pending, attached_at`); the per-frame driver retries textures per slot; the first-frame / collected diagnostics fire on the graph as a whole (`items == slots.len()` = "all collected"). Teardown: disable+hide every node, `Detaching` until EVERY item is unlisted for 2 frames, queue every destroy (retry the ones that refuse), `Destroying` until every `is_destroyed`, free every node block, then free the arcs and log `scene3d textures: <balance>`; 5 s cap per phase with the Step 3 leak rules.
4. `mod.rs`/docs: module docs updated to "Step 4"; no config, no rows.
5. Rust Quality Rules as Step 3 (panic-free callbacks, probed pointers, no lock across `run_on_render_thread`).

## Dependencies
- Task 01 (balance counters)
- Step 3 code (all of `scene3d`, `background_hide`)

## Implementation Approach
1. Generalise `SceneState` to slots; SPEC table; multi-arc load.
2. Readiness gate (`texture_readiness`) + per-slot build.
3. N-node teardown + balance line.
4. `cargo check` → `cargo fmt` → `./build.sh`; hand the DLL over for Windows + CrossOver.

## Acceptance Criteria

1. **Emi in bind pose on the footpanel, both platforms**
   - Given the mod ON + `DDR_DANCERS_SPIKE=1`
   - When a song plays (CrossOver AND a Windows cabinet)
   - Then Emi stands upright in bind (T/A-pose) on the boom00 footpanel with her texture, a dark shadow quad under her, all three collected (`items=3`), torn down at exit with `created=2k released=2k stale=0`, no WARN across 3 songs

2. **Cold-load textures**
   - Given the first song after boot
   - When the log is read
   - Then either the readiness gate delayed the build until the DDS registered (`textures … default=0` at build) or the retry line reports when it landed — never a magenta Emi for a whole song

3. **Teardown completes**
   - Given song exit / quick-fail
   - When the log is read
   - Then `destroy queued` (×3 or one line listing 3) → `destroyed by the engine flush` → arcs freed, `installed a 256-entry destroy-vector buffer` once per boot

4. **Gates**
   - Given the finished change
   - When `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`, `./build.sh`, `./scripts/validate_background_dancers.sh` run
   - Then all are clean/green

## Metadata
- **Complexity**: Medium
- **Labels**: background-dancers, step-4, spike, skinned, shadow
- **Required Skills**: the Step 3 harness state machine, `scene3d` service API
- **Generated By**: code-task-generator 2026-09-16
- **Source Plan**: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md`
- **Plan Step**: Step 4: Skinned path — bone textures, `pl_emi00` in bind pose on Windows and CrossOver
