# Implementation Plan — Enable Background Dancers

Status: Approved 2026-09-16 (maintainer)

Design: `.agents/planning/2026-09-16-enable-background-dancers/design/detailed-design.md` (Approved 2026-09-16).
Research: `research/orientation.md`, `research/a3-runtime-rules.md`, `research/world-background-and-movie.md`,
`research/formats-and-data.md`. Register: `idea-honing.md`.

Every step ends with a working, demoable system and runs its own validation. Steps 1–4 are the **spike** the
design is gated on: Step 3 is the go/no-go for the whole architecture (render-item ABI acceptance on a real
cabinet). Cabinet deploys are the only validation for engine-facing code; keep `progress.md` in the planning
directory current after each step (resume point).

## Checklist

- [x] Step 1: Signatures and derivations for the `scene3d` group + cross-build sweep
- [x] Step 2: `services/scene3d` arc loading and model-registry readiness (spike part 1)
- [x] Step 3: Render item, scene node, root attach/destroy, camera slot 0, background hide — static footpanel (spike GO/NO-GO) — **GO 2026-09-16**
- [x] Step 4: Skinned path — bone textures, `pl_emi00` in bind pose on Windows and CrossOver — **PASS on CrossOver 2026-09-16 (Windows pending; no platform-specific code in the path)**
- [x] Step 5: Pure format layer `core/anm` with Python-generated fixtures — **host-green 2026-09-16 (50 tests; 30 dance clips + 69 stage loops + 93 camanms + 4 rlists + pl_emi00 vs the Python reference)**
- [x] Step 6: Pure selection and schedule — **host-green 2026-09-16 (17 tests: χ² uniformity, permutations, shared cuts, A3 camera timeline, purity vs 1/60 s stepping)**
- [x] Step 7: Director, session and lifecycle — first fully animated random song — **cabinet PASS 2026-09-16 (deploy #3 with Steps 8–10; RE doc §3.4 for the deploy-#2 findings)**
- [x] Step 8: Character assembly completeness — parts, mirrored forearm, shadow, stage priorities — **cabinet PASS 2026-09-16 (rinon01 head+chest+both forearms+face, shadows on every dancer)**
- [x] Step 9: Camera director — A3 stage-mode sequencing with the re-projection — **cabinet PASS 2026-09-16**
- [x] Step 10: Movie-size override, diagnostics, registration, documentation, final validation — **cabinet PASS 2026-09-16 (movie songs in the thumbnail, random stage + dancer every song); one follow-up built the same day: training REWIND no longer restarts the timeline (clock `t0` never re-latches) — awaiting its confirmation run**

---

## Step 1: Signatures and derivations for the `scene3d` group + cross-build sweep

**Objective.** Resolve every engine site the design needs (design §4.2.7) on all four supported builds, all-or-nothing, before any engine-facing code exists.

**Implementation guidance.**
- Add to `src/core/signatures.rs`: AOBs for World `DancePlaySequence::onUpdate` (`FUN_180057e10` — anchor on the step-5 `OR dword [RAX+8],1` neighbourhood), the `SceneGraphManager` per-frame fn (`FUN_180023fb0`), the manager/graph ctors (`FUN_1800238a0`, `FUN_180213dc0`, camera sizing `FUN_180214ce0`), the ResourceManager hash lookup (`FUN_180202d50` family), the texture create/release pair, `FUN_1800320a0` (BgMovieActor readiness) and `FUN_18003e5b0` (`bg_root` create). Derivations in one `derive_scene3d` fn following the `derive_bottom_text` (identity-gated, all-or-nothing, un-resolve everything on any miss) and `derive_smarvelous_burst` (all sites must agree) shapes; non-address results via `publish_value` (`scene_graph_root_child_off`, `scene_graph_camera_off`, `camera_stride`, `mgr_destroy_vec_off`, `mgr_lock_count_off`, `bgframe_off`, `bg_clip_slot_off`, `cmovieclip_pool_base`, `cmovieclip_pool_stride`).
- Implementation-time RE (Ghidra `DDRWorld_Ghidra`, 20260825 open): (a) the texture RELEASE function (start from the A3 item dtor `FUN_180175ea0` family, find the World twin); (b) what the pass-4 `visit` context is and how A3's `FUN_18015b300` pushes the node onto the visible vector; (c) the manager lock: `Ordinal_16/17` called when `mgr+0x28 > 0` in `FUN_180024250` — which libavs export, and how the DLL calls it (IAT slot). Record findings in `docs/background_dancers_research.md` (new RE doc; the feasibility doc stays as-is).
- Add a `pub fn scene3d_sites(&self) -> Option<Scene3dSites>` getter bundling every value.

**Tests.** `./scripts/validate_signatures.sh ~/Desktop/ddr_modules` green for the new names on 20250805 / 20260224 / 20260721 / 20260825; `scripts/sig_harness/shape_diff.py` over `FUN_180263430`, `FUN_180261780`, `FUN_180262670`, `FUN_180214570`, `FUN_180023fb0`, `FUN_18003e5b0` twins — every offset in design §5.2/§5.3 identical across builds (record the report in the RE doc). `cargo check` clean.

**Integration.** None yet at runtime beyond the boot log.

**Demo.** Cabinet boot log shows `[+] <name> (derived) = 0x…` for every site of the group (or the single WARN naming the first miss on a build where it fails).

## Step 2: `services/scene3d` arc loading and model-registry readiness (spike part 1)

**Objective.** Prove the stock loader turns an A3 arc into GPU model resources when we register it, and measure the latency.

**Implementation guidance.**
- New `src/services/scene3d/{mod.rs, arc_set.rs, model_registry.rs}` per design §4.2.1–§4.2.2 (`resolve_path` via `avs_layeredfs::mod_paths::find_first_modfile("arc/<file>")` else the stock path under the game dir; `load`/`free` through the existing `file_manager_*` signatures; `read_bytes` for our own parsing; `model_resource(name)` + `ResourceView`).
- A developer-only harness inside the mod skeleton (`src/mods/background_dancers/mod.rs`, id/name per design, registered in `src/lib.rs`, in `DEFAULT_OFF_MODS`): when `DDR_DANCERS_SPIKE=1` (developer_mode-gated) load `mapset_boom00.arc` at GAMEPLAY entry via `run_on_render_thread`, poll `model_resource("gm_boom00_<part>")` per frame in `input_manager::on_frame`, log each model's residency time and `ResourceView` fields (bone count, draw records, skinned bit), free at window exit.

**Tests.** Host: `resolve_path` precedence (mod folder over stock, missing → None) with a temp dir. Cabinet: the log.

**Integration.** Mod skeleton + scene/frame callbacks exist; nothing rendered yet.

**Demo.** Cabinet log: `scene3d: gm_boom00_footpanel resident after 180 ms (bones=1 records=1 skinned=0)` for every part; free at exit without WARN; three consecutive songs stable.

## Step 3: Render item, scene node, root attach/destroy, camera slot 0, background hide — static footpanel (spike GO/NO-GO)

**Objective.** A hand-built render item + node drawn by the engine's own passes behind the lane. This is the load-bearing unknown of the whole feature.

**Implementation guidance.**
- `services/scene3d/{texture.rs (create/release only), render_item.rs, node.rs, scene_graph.rs}` per design §4.2.3–§4.2.6 and the §5.2/§5.3 layouts. Rigid model first (footpanel): item mode `0x9`, bones seeded from `ResourceView::bind()`, tint `(1,1,1,1)`, pass mask 4. Node vtable: 8 slots, `[-1]` null COL, `[0]` dtor, `[1]` visit; `visit(2)` copies a static world/bones, `visit(4)` refreshes flags + pushes visible (per the Step 1 RE), returns 0 otherwise.
- `attach_under_root` / `queue_destroy` under the manager lock exactly as `FUN_180024250` does it; nodes flat.
- `write_camera0` with a fixed camera (eye `(0, 1.2, 4)`, target `(0, 0.8, 0)`, up `+Y`, `t' = 0.5`, near 0.1, far 100).
- `mods/background_dancers/background_hide.rs` per design §4.3.6 (live handle from `BgMovieActor→BackgroundFrame+0x140`, pool validation, per-frame `layer_set_color_raw(…, 0)`, restore on window exit; shared-capture fallback only if the derivations are absent).
- Spike harness: at GAMEPLAY entry build + attach the footpanel item/node once its model is resident; queue destroy and restore the background at window exit; log every stage.

**Tests.** Host: `offset_of!` tests for `SceneNode`, the item header and the camera slot; `render_item::build` against a synthetic `ResourceView` over a `Vec<u8>` (every pointer/size in §5.2, draw-record flag copy, palette/material copy counts, rigid vs skinned mode flags). Cabinet: the demo below, three songs in a row, quick-restart and quick-fail mid-song (teardown paths), zero WARNs.

**Integration.** Uses Step 2's loader; the background hide is the design's final mechanism (not spike-only).

**Demo.** The boom00 footpanel is visible behind the lane with the 2D background hidden, disappears cleanly at song exit. **Go/no-go:** if the collector/draw rejects the item after reasonable ABI fixes, stop — Option B is a separate PDD.

## Step 4: Skinned path — bone textures, `pl_emi00` in bind pose on Windows and CrossOver

**Objective.** The skinned pipeline (two dynamic `A32B32G32R32F` bone textures, engine upload, skinning VS) works on both platforms.

**Implementation guidance.**
- `texture::create_dynamic(bone_count, 4, 0x74, 0x2001)` ×2 per skinned item; item mode `0xF`; ModelParameters `{bones, 1, bones, 0}`; bones = bind (identity chain). Release in the dtor.
- Spike harness extension: load `pl_emi00.arc` + `pl_shadow00.arc`; add the body item (pass mask 2) at the origin with `diag(0.9,0.9,0.9,1)` (rlist scale of emi00), and the shadow quad item under it.

**Tests.** Host: builder test for the skinned shape (texture handles wired, mode flags). Cabinet: Windows AND CrossOver — Emi standing on the footpanel, correctly textured, no texture-registry growth across songs (log the create/release balance).

**Integration.** Step 3's item builder gains the skinned branch; nothing else changes.

**Demo.** Emi in bind pose on boom00 under the lane on both platforms; `texture created=2 released=2` per song.

## Step 5: Pure format layer `core/anm` with Python-generated fixtures

**Objective.** A host-tested Rust port of the ANM/CAMANM/B2IT/MRL0/KTMDL-bone-table codecs matching the existing Python reference.

**Implementation guidance.**
- `src/core/anm/{mod.rs, anm.rs, sample.rs, pose.rs, camera.rs, b2it.rs, rlist.rs, ktmdl.rs}` per design §4.1. Dependency-free (mountable by a `#[path]` harness).
- `scripts/gen_anm_fixtures.py` (uses `scripts/anm_dump.py` / `scripts/ktmdl_dump.py`, reads `$DDR_WORLD_INSTALL` via `scripts/unpack_arc.py`): for every dance clip of `mc_male.arc`/`mc_female.arc`, every `_play_loop.anm` of every `mapset_*.arc`, and every `.camanm` of `stage_camera.arc`, emit header fields + `evaluate_pose`/camera-slot samples at 8 fractional frames (JSON under `tests/fixtures/anm/`, small: values only). Also dump the four `startup.arc` rlists, `pl_emi00.b2it`, and the `pl_emi00.model` bone table.
- `scripts/validate_background_dancers.sh`: temp-crate harness (the `validate_judgement_offsets.sh` pattern — plain `cargo test` cannot compile `retour` on ARM hosts) mounting `core/anm/*` + the fixtures.

**Tests.** Fixture equality (1e-5; quaternions up to sign), q48 round trip, loop flag, bind seeding on a synthetic partial clip, camera recipe vs `game_camera_half_tangent`, rlist/b2it/bone-table equality, `clip_time` clamp/wrap.

**Integration.** Not yet wired to the engine.

**Demo.** `./scripts/validate_background_dancers.sh` green over all stock clips and camanms.

## Step 6: Pure selection and schedule

**Objective.** The random picks and the A3 sequencing rules as pure, deterministic, host-tested functions.

**Implementation guidance.** `src/mods/background_dancers/{selection.rs, schedule.rs}` per design §4.3.2–§4.3.3 (xorshift64* RNG; stage candidates minus `dummy00`, distinct-key then row; dancer candidates over all rows with a body arc; fixed pools; Fisher–Yates playlists; camera main/`_non` split; `DanceSchedule::at` with `CUT_LEAD = 1.5 s` shared cuts; `CameraSchedule::advance/at` with the freeze at 2.0 s, `_non` hold `1 + U[0,1)`, rotation, NO beat gate). Add the files to the Step 5 harness.

**Tests.** Per design §7.1 items 5–6: exclusions, uniformity (χ²), permutation property, `_non` split, PIN override, segment lengths, shared cut instants, `at(t)` == incremental stepping, rewind re-simulation, camera transition timing.

**Integration.** Consumed by Step 7.

**Demo.** Harness green; a `--print` example in the test output shows a seeded song's segment table and camera timeline.

## Step 7: Director, session and lifecycle — first fully animated random song

**Objective.** Replace the spike harness with the real per-song loop: random stage + dancer(s), animated body and stage parts, clock-driven, torn down cleanly.

**Implementation guidance.**
- `mods/background_dancers/{lifecycle.rs, session.rs, director.rs}` per design §4.3.1, §4.3.4, §4.3.5: window state machine on scene callbacks (Idle → Requested → Built → Playing → Teardown), one parse thread per song (`arc_set::read_bytes` → `core/anm`), residency poll, build order (stage parts, dancers; parts/shadow come in Step 8), attach hidden, `Playing` at `live_dps()` step ≥ 5 ∧ `first_anchored_frame()`, `t0` latch + rewind re-latch, `FrameState` double buffer + seqlock read in `visit(2)`; teardown = disable nodes, queue destroy, restore hide, free arcs after every dtor (5 s cap → leak + WARN).
- Eligibility per design §4.3.1 (entered sides from `stage_records::side_entered`, n dancers, x pitch 1.6).
- Diagnostics: per-song INFO (pick), `Built` INFO (ms), `Playing` INFO, residency-timeout WARN.

**Tests.** Host: director math (world matrices, hidden-before-edge) in the harness. Cabinet: 10 consecutive random songs; 1P and 2P; SONG SPEED 150 % (dance follows); quick restart (in-place and `finish` path); training scrub forward/backward and loop; quick-fail with skip-results; course mode; log shows every lifecycle line, zero WARNs.

**Integration.** Spike harness removed; the mod is now the real thing minus parts/shadow/camera polish (camera still the Step 3 fixed camera).

**Demo.** Every song shows a random stage with random dancer(s) dancing to the music from the song-start edge, torn down at exit.

## Step 8: Character assembly completeness — parts, mirrored forearm, shadow, stage priorities

**Objective.** A3-complete dancers and stages.

**Implementation guidance.** Session/director additions per design §4.3.4–§4.3.5 and §5.4: part arcs (`head00/hips00/chest00/forearm00/face01`, those that resolve), attach bones via the body `.b2it`, `E = diag(s)` / `diag(−s,−s,−s,1)` right-forearm copy sharing the same model resource, part bones = bind; shadow quad with the A3 size/centroid rule, tint `(0,0,0,1)`, low-pass reset at song start; stage `:N` parts → pass mask `0x10`; rlist `model_scale`/`shadow_scale`. Missing part arcs are skipped silently (A3 behaviour).

**Tests.** Host: part world identity `v_part · E · Bind[attach]` against the add-on's verified numbers (`tools/blender_ddr_addon/tests/character_test.py` values), shadow clamps. Cabinet: `rinon00` (all parts), `afro00` (head only), `babylon00` (class B scale 0.4), `emi00` (faces only); shadow follows feet; `:N` stage parts (boom00 `bg:-2`, `stage:-1`) draw in priority order.

**Integration.** Same lifecycle; more instances per song.

**Demo.** Dancers wear their parts (mirrored right forearm correct), stand on a shadow that tracks them; stages render with correct layering.

## Step 9: Camera director — A3 stage-mode sequencing with the re-projection

**Objective.** Replace the fixed camera with the stage's `.camanm` sets sequenced by the A3 rules.

**Implementation guidance.** Load `camera/stage_camera.arc` bytes, parse the stage row's camanms (`data/camera/long/<name[:5]>/<name>.camanm`), drive `CameraSchedule` from the dance schedule's cut times, `core/anm::camera::sample_camera(…, 1.0, 1.0)` each frame, `scene_graph::write_camera0` from the game thread. Log the camera timeline at song start (dev mode).

**Tests.** Host: schedule tests already cover transitions; add a camera-sample → slot-field mapping test. Cabinet: the footpanel/floor sits at the frame bottom with the stage upright (projection sanity); cut-aways (`_non`) appear at dance cuts and return; Custom Resolution 1080p/4K and SD 640×480 unchanged aspect; CrossOver parity.

**Integration.** Director gains the camera; Step 3's fixed camera removed.

**Demo.** Camera cycles through the stage's shots, cutting to `_non` angles right before each dance cut, exactly as A3.

## Step 10: Movie-size override, diagnostics, registration, documentation, final validation

**Objective.** Finish the user-facing contract and hand over a validated build.

**Implementation guidance.**
- `movie_size.rs` per design §4.3.7 (apply at window entry for every entered side reading 0/1 → 2; restore at exit); confirm on a movie song that the movie renders in the thumbnail marker with the 3D behind it.
- Finalise diagnostics (design §4.3.8), the `DDR_DANCERS_PIN` dev env, `is_active()` semantics ("CAN work"), `DEFAULT_OFF_MODS` entry, `disable()` tearing down a live session and restoring hide/movie state.
- Docs: `docs/background_dancers_research.md` (Step 1 RE + spike findings + any ABI corrections), AGENTS.md Key-Entry-Points row for the mod (id, module map, signatures, fail-open rules, the two write-and-restore mechanisms, thread model), README operator note (default OFF, what it does, movie thumbnail behaviour), `.agents/summary` touch-ups if the module tree changed.
- Final gates: `cargo check` → `cargo fmt` → `./build.sh` → `validate_signatures.sh` all green → `shape_diff.py` reviewed → `validate_background_dancers.sh` green → cabinet run on Windows and CrossOver (movie song, non-movie song, 2P, rate, restart, scrub, course, attract untouched, mod toggled OFF live).

**Tests.** As listed; plus a host test for `movie_size::apply/restore` value mapping.

**Integration.** Complete: mod registered, default OFF, all docs updated.

**Demo.** With the mod ON: every song has a random 3D stage and dancers behind the lane; movie songs show their movie as a thumbnail over the scene; toggling OFF mid-session restores stock behaviour at the next song; boot with the mod OFF is byte-for-byte stock behaviour.
