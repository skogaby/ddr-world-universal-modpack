# Progress — Enable Background Dancers

Updated: 2026-09-16 (Steps 1–10 cabinet-PASSED; post-PASS follow-ups: rewind fix + BPM sync / STOP slow-mo)
Status: ALL 10 STEPS CABINET-PASSED 2026-09-16. Two post-PASS follow-ups are BUILT and await one confirmation
run (`release/ddr_world_hook.step10-deploy3.dll` = `target/x86_64-pc-windows-msvc/release/ddr_world_hook.dll`):
(a) training REWIND no longer restarts the timeline (nothing is latched — scene time is a pure function of
the music count); (b) maintainer-requested **BPM sync + STOP slow-motion**, both ON by default
(`background_dancers.{bpm_sync,stop_slow}`): the dancers/stage/camera run at chart BPM/120 (half a second
of the 120-BPM clips per beat), phase-pinned to the measure grid, cuts on beats, 1/12 speed through STOPs —
the A3 rate rule (`FUN_18003a1b0`, RE doc §3.5) integrated over the chart's SSQ tempo chunk. Gates: `cargo
check` (+ `--tests`) / `cargo fmt` / `./build.sh` / harness 102 green / `validate_signatures.sh` ALL GREEN ×5.
NEXT ACTION: maintainer plays (1) a steady-tempo song and checks the dancers hit the beat (the clips' steps
land on the chart's beats; at 140 BPM the dance runs ~17 % faster than before), (2) a STOP song (`anan`,
`aeth`, any chart with a stop) and sees the whole scene drop into slow motion through the stop, (3) one
training song with REWIND (7) / LOOP — the dancer jumps back, log `music count jumped back … timeline
follows`, (4) a quick restart (1) — dance restarts from the top. Expected log per song: `tempo map for
'<basename>' -- N node(s), X BPM at music 0, dance time 0 at Y ms (bpm_sync=true stop_slow=true)` BEFORE
`visible`, then `playing -- first count -276 ms (dance time …, tempo map: X BPM here, tau=0 at Y ms)`. Then
the maintainer decides on `DEFAULT_OFF_MODS` and commits.

Resume protocol: read `implementation/plan.md` (the 10 steps + checklist) → `design/detailed-design.md`
(§4.2 service API, §5.2/§5.3 layouts) → `docs/background_dancers_research.md` §1 (the Step 1 RE record:
lock protocol §1.2, pass protocol §1.3, texture API §1.4, model registry walk §1.5, sweep §1.9) → this
file. Task files for the current step live under
`.agents/tasks/2026-09-16-enable-background-dancers/stepNN/`.

## Done

- Planning phase complete and approved (rough-idea, idea-honing 23 decisions, 4 research files, design,
  plan, summary) — 2026-09-16.
- **Step 1 (2026-09-16):** `src/core/signatures.rs` — 12 new AOBs (`sg_*`, `model_registry_release`,
  `texture_create_site`, `texture_release`, `bgmovie_*`, `bg_root_create_site`), one all-or-nothing
  `derive_scene3d` (identity-gated at 10 points, plausibility-checked), 46 published `scene3d_*` names,
  `Scene3dSites` bundle + `scene3d_sites()` getter. Sweep ALL GREEN on 20250805 / 20260224 / 20260721 /
  20260825 / 20260915; every derived value identical across builds; every engine consumer (collector,
  upload, draw, update, tick, flush, camera rebuilds, bg_root creator, item push) byte-shape-identical
  on all five builds with identical struct-field reads. RE + results: `docs/background_dancers_research.md`
  §1. Task records: `.agents/tasks/…/step01/task-0{1,2}-*.code-task.md`.
  Status: Complete (uncommitted — maintainer commits manually).

- **Step 2 (2026-09-16, code):** `src/services/scene3d/{mod.rs (init/is_available/sites/file_manager +
  the shared `with_avs_mutex` engine-lock helper reading the IAT slots at call time), pure.rs
  (dependency-free: `data_relative`, `resolve_with`/`Resolved`, `fnv1_name_hash` — host-tested),
  arc_set.rs (`resolve_path`/`resolve`/`read_bytes` + FileManager `load`/`free`, mod-override-aware),
  model_registry.rs (`model_resource` = the read-only red-black-tree walk under the map's mutex, every
  node probed; `ResourceView` over the GPU resource header)}`; `src/mods/background_dancers/{mod.rs
  (skeleton: id `background-dancers`, DEFAULT OFF, scene + frame callbacks, `is_active` = service
  available), spike.rs (dev harness: `DDR_DANCERS_SPIKE` env ONLY — the developer_mode gate the plan
  named was dropped by the maintainer so the test runs the production LayeredFS path ⇒ load
  `mapset_boom00.arc` at GAMEPLAY entry, poll 6 models per frame, free at window exit, 20 s WARN)}`;
  wired in `lib.rs` (`scene3d::init` after `asset_loader`, mod registered after two_player_bpl),
  `services/mod.rs`, `mods/mod.rs`, `DEFAULT_OFF_MODS`; `SignatureStore::module_base()` added.
  `scripts/validate_background_dancers.sh` (temp-crate harness; 6 tests green). `cargo check` /
  `cargo fmt` / `./build.sh` clean. Task record: `.agents/tasks/…/step02/task-01-*.code-task.md`.
  Cabinet PASS 2026-09-16. Status: Complete (uncommitted — maintainer commits manually).

- **Step 3 (2026-09-16, code — awaiting cabinet):** RE first (`docs/background_dancers_research.md`
  §2): World's model converter resolves material textures at CONVERSION time through the gs texture
  registry (`FUN_180273e20` table fill → `FUN_18026f9e0` lookup, default `DAT_1806f3298` on miss;
  `FUN_180274070` writes `mat+0xA8+slot*0x18`) and **A3's setModel-time re-resolve has NO World twin**
  (the lookup's only caller is the converter) — a `.dds` registered after its `.model` (the arc order)
  leaves the material on the default texture; the palette (200 B) is the vertex-stream binding block
  and, like the material, is read-only for the engine ⇒ plain `memcpy` copies (amendment 7 moot); the
  frame stamp `+0xB4` MUST be non-zero (`0xFFFFFFFF`; a 0 seed hangs the pass driver — upload protocol
  §2.3); `ModelParameters.z = 0` for rigid AND skinned; the ONLY engine read of `+0xA8` is bit `0x10`.
  New OPTIONAL signature `texture_lookup_site` (`scene3d_texture_lookup/_default/_spin`, sweep ALL GREEN
  ×5, identical addresses to Ghidra on 20260825). Code: `memory::free_alloc`;
  `services/scene3d/{texture.rs, render_item_layout.rs (pure), render_item.rs, node_layout.rs (pure),
  node.rs, scene_graph.rs}`; `mods/background_dancers/background_hide.rs`; `spike.rs` extended into the
  footpanel GO/NO-GO harness (build → attach → camera → hide → first-frame diagnostic → guarded
  teardown). Host harness 19 tests green (`validate_background_dancers.sh`), `cargo check` / `cargo fmt`
  / `./build.sh` clean. Task records: `.agents/tasks/…/step03/task-0{1,2,3}-*.code-task.md`.
  **Cabinet GO 2026-09-16** (deploy #1 below) + two follow-up fixes (texture retry, destroy buffer) built
  the same day. Status: Complete (uncommitted — maintainer commits manually).

- **Step 4 (2026-09-16, code — awaiting cabinet):** `texture.rs` create/release/stale counters +
  `balance()` (`Display` = `created=N released=M stale=K`); skinned build asserts two distinct non-zero
  bone-texture handles; `render_item::texture_readiness(res)` (table walk + registry lookups, no item)
  and `retry_texture_resolve(item)` (per-frame re-resolve on an ATTACHED item — aligned pointer stores
  into our copies); `spike.rs` generalised to a `SPEC` table + `Vec<Slot>`: `mapset_boom00` /
  `pl_emi00` / `pl_shadow00` loaded together, each model built when resident AND `still_default == 0`
  (10 s timeout ⇒ build + retry), footpanel (mask 4, identity) + Emi body (skinned, mask 2,
  `scale 0.9` = rlist `emi00` model scale) + shadow quad (mask 2, `scale 0.75` at y 0.02, tint black),
  N-node teardown (all items unlisted → all destroys queued → all dtors → node blocks → arcs + the
  texture balance line). Task records: `.agents/tasks/…/step04/task-0{1,2}-*.code-task.md`. Host
  harness 20 tests green; `cargo check` / `cargo fmt` / `./build.sh` clean.
  Status: Complete (uncommitted — maintainer commits manually); plan checkbox ticked on cabinet PASS.

  **Cabinet PASS 2026-09-16 (CrossOver)** after one fix (bone-texture orientation, RE §2.8). Status:
  Complete (uncommitted — maintainer commits manually).

- **Step 5 (2026-09-16, host-only):** `src/core/anm/{mod.rs (Le reader, Mat4/Quat/Vec3, mat_mul/
  mat_inverse/quat_to_rowmat/mat_to_quat), anm.rs (container: magic, frame_count, loop bit @6 bit0, fps
  only with a type-4 chunk, type-0 bone tracks, type-4 camera slots, everything else ignored; every read
  bounds-checked), sample.rs (q48 decode/encode, half floats, kinds 1/4/8/10/0x1B/0x1C/0x1D/0x1E/0x1F,
  game-equivalent `sample` incl. explicit-time dup skip + last-key clamp, slerp, `clip_time`), pose.rs
  (Skeleton/Trs, A3 bind seed `seed_local_trs`, `evaluate`/`evaluate_into` (no-alloc)/`sampled_trs`),
  camera.rs (six slots → `CamSample` — field-identical to `scene_graph::CamSample`; `half_tangent` = the
  add-on's formula), b2it.rs, rlist.rs, ktmdl.rs (bone table)}`; `pub mod anm` in `core/mod.rs`.
  `scripts/gen_anm_fixtures.py` → `tests/fixtures/anm/{dance_clips,stage_loops,stage_cameras,rlists,
  pl_emi00}.json` (2.1 MB, values only; regenerate, never hand-edit). Tests: `src/core/anm/tests.rs`
  (44 synthetic: builder-made ANM images, q48/half/clip_time, parser, sampling rules, mat↔quat, seed +
  chain incl. the scaled-parent case, camera worked values + monotonicity, rlist/b2it/ktmdl images) +
  `tests/fixtures.rs` (6 fixture tests with a test-only arc reader + Konami LZ77; SKIP with a note when
  `DDR_WORLD_INSTALL`/fixtures are absent). Harness mounts `core/anm/mod.rs` as a directory module +
  `serde_json` dev-dep + `ANM_FIXTURE_DIR`; **50 tests green**, every stock clip/camanm/rlist matches the
  Python reference. `cargo check` / `cargo fmt` / `./build.sh` clean. RE record + the facts that matter
  downstream (partial stage loops need the bind seed; `read_bytes` returns the whole arc): RE doc §3.
  Task records: `.agents/tasks/…/step05/task-0{1,2}-*.code-task.md`.
  Status: Complete (uncommitted — maintainer commits manually).

- **Step 6 (2026-09-16, host-only):** `src/mods/background_dancers/selection.rs` (xorshift64* `Rng` with
  Lemire `below` + Fisher–Yates `shuffle`, `seed_from(qpc, scene)`, `Sex`, `StageCandidate`/`DancerCandidate`
  from the two rlists — `dummy00` dropped, arc existence injected, unlock ids ignored —, `pick_stage` =
  uniform over DISTINCT keys then rows, `pick_dancers` independent picks, `POOL_MALE`(14)/`POOL_FEMALE`(13)
  + `playlist`, `camera_lists` (`_non` split + shuffle), `clip_member_path`/`camanm_member_path`, `dancer_x`
  = `(i − (n−1)·0.5)·1.6`, `parse_pin`/`apply_pin` for `DDR_DANCERS_PIN`) and `schedule.rs` (`ClipRef`,
  `DanceSchedule` — segment k = every dancer's `playlist[k mod n]`, length `max(0.05, min dur − 1.5)`,
  `at`/`segment_at`/`cuts_until`/`cut_at_or_after`; `CameraSchedule` — main cycles on finish, FROZEN while
  a cut is within 2.0 s (a finish inside the window is deferred to the cut), `_non` shot AT the cut held
  `1 + U_k` with `U_k = Rng::new(seed ^ (k+1)).next_f32()`, resume = next main; empty `_non` ⇒ the deferred
  finish fires at the cut; `advance(st, from, to)` = event loop over `(from, to]`, `at(t)` = re-simulate).
  17 tests (χ² over 10⁵ stage picks on the real 34-row table incl. the boom00 row split, permutation +
  `tu01` absence, shared cut instants with the 3-vs-4 playlist wrap, the A3 timeline, `at == 1/60 s
  stepping` for 3 seeds over 200 s + coarse steps + rewind, short-segment re-cuts, `print_example_timeline`
  demo). Both mounted in the harness (67 tests total). `cargo check` / `cargo fmt` / `./build.sh` clean.
  Task records: `.agents/tasks/…/step06/task-0{1,2}-*.code-task.md`.
  Status: Complete (uncommitted — maintainer commits manually).

- **Step 7 (2026-09-16, code — awaiting cabinet):** `services/scene3d/frame_board.rs` (32 seqlocked slots ×
  64 bones, every payload word an `AtomicU32`; `publish` game thread / `read_slot_into` job thread, bounded
  retries; harness-tested) + `node.rs` (`new_node(…, instance)`, `visit(2)` copies the slot into the item —
  the ONLY writer of an attached item's pose; pass 4 can only FORCE hidden on a board node) —
  `mods/background_dancers/{session.rs (Pick + arcs + summary, make_pick/assemble_pick, the parse thread's
  Parsed bundle via arc_set::read_bytes → core::arc::{parse,extract} → core::anm — stage parts seeded from
  the .model bind table, dancers' playlist clips —, dance_schedule, Session/Instance table + build_pending
  (spike's build_slot generalised; bone-count cross-check vs the resident model)), director.rs (`produce`:
  dancers = schedule.at → clip_time → evaluate_into → publish body_world; stage parts = loop wrapped or the
  bind pose), director_math.rs (pure: body_world/clip_frame — harness), clock.rs (pure 3-rule song clock:
  graph off ⇒ hidden/t=0; on+unanchored ⇒ t=0 pose or hold; anchored ⇒ (count−t0)/1000, edge + rewind
  re-latch — harness), lifecycle.rs (tables from startup.arc at enable — 27 stage rows/25 stages, 26
  dancers, camera rows; window entry = entered sides → seed → pin/random pick → INFO → render-thread
  arc load + parse thread; per frame Requested→Built/Abandoned(20 s), fixed camera once, clock, hide arm,
  produce, texture retry, the Step 3 diagnostics; window exit = the spike's Detaching→Destroying teardown
  + orphan parking; disable = neutralise)}`; `mod.rs` rewired; **`spike.rs` deleted**. Harness 76 tests
  green; `cargo check` / `cargo fmt` / `./build.sh` clean. `core/anm::sample` explicit-time lookup switched
  to `partition_point` (the game's binary search; fixtures still equal). Task records:
  `.agents/tasks/…/step07/task-0{1,2,3}-*.code-task.md`.
  Status: Code complete (uncommitted — maintainer commits manually); plan checkbox ticks on cabinet PASS.

  **Deploy-#2 fixes (2026-09-16, built, awaiting deploy #3; RE doc §3.4):** (A) `Instance::node_shown` —
  the node-level force-hidden flag is dropped per instance right after its FIRST `director::produce`
  publish (was: once, on the single frame `visible` first turned true). (B) NEW optional RTTI signature
  `dance_play_sequence_vtable` (`signatures.rs::find_dance_play_sequence_vtable`, resolves on all five
  sweep builds) → `song_reset::{dps_step, dps_identity_available, DPS_STEP_GRAPH_ENABLE}`; the visibility
  gate is now `graph_stats().enabled ∧ dps_step() >= 5` (vtable-verified DPS). `scene_manager`/`scene`
  imports dropped from `lifecycle.rs`. (C) The close crash is the SceneGraphManager SHUTDOWN freeing our
  destroy-vector buffer through the engine allocation header at `begin−0x20` (`gamemdx.dll+0x24178` decoded
  against the cabinet's 20260915 build — the earlier "camera copy in the tick" attribution used the 20260825
  program and was wrong; stock World has camera slot 0 ACTIVE anyway); fix = `scene_graph::
  destroy_reserve_begin` allocates the buffer behind a valid header pointing at a mod-owned no-op allocator.
  No camera changes.

- **Step 8 (2026-09-16, code — awaiting cabinet):** `director_math.rs` grew the pure part/shadow math
  (`mat_mul` copy pinned against `core::anm::mat_mul`, `transform_point`, `MIRROR = diag(−1,−1,−1,1)`,
  `part_world(mirror, bone, body) = E · bone · body`, the A3 shadow rule `shadow_target`/`shadow_step`/
  `shadow_world` with `SHADOW_FLOOR_Y 0.02`, gain 1.5, max 2, low-pass 0.1 — 11 new harness tests);
  `selection.rs`: `PART_NAMES`, `part_attach_bone` (`head00`/`face01` → Head, `hips00` → Hips, `chest00` →
  Spine2, `forearm00` → LeftForeArmRoll), `MIRROR_PART`/`MIRROR_ATTACH_BONE` (RightForeArmRoll),
  `GROUND_BONES`, `HIPS_BONE`, `SHADOW_ARC`/`SHADOW_MODEL`, `DancerCandidate::{part_arc_name,
  part_model_name, parts_present}`; `session.rs`: `Pick.parts` (per dancer, the part arcs that RESOLVE at pick
  time — `make_pick`/`assemble_pick` take an `arc_exists` closure), `Pick::arcs()` appends the part arcs +
  `pl_shadow00.arc`, summary shows `wear=[head+hips+…]`; the parse thread reads the body `.b2it`
  (`core::anm::b2it`) for attach/ground/hips indices, each part's `.model` bone table (1 bone), the forearm
  TWICE (L + `mirror: true` on RightForeArmRoll, same model name), and `pl_shadow00.model` once; `InstanceKind::
  {Part{dancer,part}, Shadow(dancer)}` (build order stage → dancers → parts → shadows, all pass mask 2, shadow
  tint BLACK), `Session.children` (per-dancer instance indices) + `shadow_size` low-pass state +
  `reset_shadow()` + `built_counts()`; `initial_world` gives parts their bind placement and the shadow its
  rest size; `director.rs::produce` evaluates each body ONCE and derives its parts (`part_world` over
  `bones[attach]`) and shadow (ground-bone translations, Hips bind-vs-anim Δ, low-pass) from the same bone
  buffer, publishing children with `[IDENTITY]`; `lifecycle.rs` resets the shadow low-pass on every clock
  (re)latch, logs part counts in `parsed`/`built`. **Design amendment:** `E = diag(±1)`, NOT `diag(±s)` — the
  body scale already sits in `body_world`; `diag(s)` on the part side would scale vertices twice (the format
  doc §8 vertex path `s·(v·R) + s·t + x` and the add-on's `v·E·Bind` identity both have ONE `s`). Stage `:N`
  priorities were already shipped in Step 7 (`PASS_MASK_LOWPRIO` + sort key) — deploy #2 happened to pick the
  priority-free `boom00` row 32; row 0 (`bg:-2`, `stage:-1`) is the cabinet check. Task records:
  `.agents/tasks/…/step08/task-0{1,2}-*.code-task.md`. Gates: `cargo check` (+ `--tests`) / `cargo fmt` /
  `./build.sh` / harness 88 green.
  Status: Code complete (uncommitted — maintainer commits manually); plan checkbox ticks on cabinet PASS.

- **Step 9 (2026-09-16, code — awaiting cabinet):** camera director. `session.rs`: `Pick.camera_main/
  camera_non` (`selection::camera_lists` — the row's names split on `_non`, both Fisher–Yates shuffled from
  the song seed; summary `cameras=main:N non:M`), `STAGE_CAMERA_ARC = data/arc/camera/stage_camera.arc`
  (read by OUR `ArcReader` on the parse thread — never `FileManager::Load`ed, the engine needs nothing from
  it), `ParsedCameras { main, non }` (each `camanm_member_path(name)` → `parse_clip`; a missing member = one
  warning; no main clip ⇒ `None` ⇒ the fixed fallback camera), `camera_schedule(parsed, seed)` →
  `Session.camera: Option<CameraSchedule>` + `camera_state: Option<(CameraState, f32)>` + `reset_camera()`
  / `has_camera()`. `director.rs::camera_frame(sess, t)`: `advance(prev, prev_t, t, dance)` when `t ≥
  prev_t`, else (song (re)start / backwards jump) `at(t)`; picks the `Clip` for `ClipSel::{Main,Non}`,
  `frame = clip_frame(t − clip_start, dur, fps, loops)`, `core::anm::camera::sample_camera(anm, bytes,
  frame, 1.0, 1.0)` → `scene_graph::CamSample` (field copy; engine-side test `cam_sample_shapes_agree`);
  `camera_timeline(sess, until)` = one `k@cut: before -> at` entry per dance cut for the dev log.
  `lifecycle.rs`: with a camera set slot 0 is written EVERY frame (hidden frames too, so the first visible
  frame already renders through the right camera — the tick copies the slot a frame after the dirty bytes);
  first write logs `camera director -- main:[…] non:[…]` (+ `camera timeline -- …` under
  `layeredfs.developer_mode`); the Step 7 fixed camera is now the FALLBACK written once when
  `!has_camera()` (`fixed fallback camera … no camera set for this stage row`); `ClockEvent::Latched` ⇒
  `reset_camera()` beside `reset_shadow()`. Task record: `.agents/tasks/…/step09/task-01-*.code-task.md`.
  Gates: `cargo check` (+ `--tests`) / `cargo fmt` / `./build.sh` / harness 88 green. DLL staged as
  `release/ddr_world_hook.step9-deploy1.dll` (= Step 7 fixes + Step 8 + Step 9).
  Status: Code complete (uncommitted — maintainer commits manually); plan checkbox ticks on cabinet PASS.

- **Step 10 (2026-09-16, code — awaiting cabinet):** `mods/background_dancers/movie_size.rs` (design §4.3.7:
  `init(signatures)` stashes the shared `customize_offset` derivation — optional, one INFO when missing;
  `apply([entered])` writes `Customize+0x30` 0/1 → 2 for every entered side (field pointer =
  `stage_records::player_work(side) + customize_offset + 0x30`, `memory::is_readable`-probed) and returns the
  originals; `restore(saved)` writes them back only where the field still reads 2; pure `override_for` +
  test); `lifecycle.rs` applies it at window entry right after the pick INFO (`movie size overridden for the
  song -- P1 Some(1) P2 None -> 2`) and restores SYNCHRONOUSLY in the scene callback's exit branch (before
  the teardown is scheduled — SONG_SELECT's VIDEO SIZE re-seed and the EAM_EXIT customize write-back both
  come later) and in `teardown_on_disable`; `mod.rs::init` calls `movie_size::init`, module doc at its final
  shape. Docs: AGENTS.md Key-Entry-Points row "Enable Background Dancers" (architecture, thread model,
  visibility gate, allocation-header convention, A3 rules, overrides, teardown, tests, RE pointers), README
  feature-list row + "Enable Background Dancers" operator section, RE doc §3.4. `.agents/summary` untouched
  (it predates several mods; AGENTS.md is the authoritative table). Task record: `.agents/tasks/…/step10/
  task-01-*.code-task.md`. Gates: `cargo check` (+ `--tests`) / `cargo fmt` / `./build.sh` / harness 88 green /
  `validate_signatures.sh` ALL GREEN ×5 (run after the `dance_play_sequence_vtable` addition; nothing in
  `signatures.rs` changed since). DLL staged as `release/ddr_world_hook.step10-deploy1.dll` (= everything).
  Status: Code complete (uncommitted — maintainer commits manually); plan checkbox ticks on cabinet PASS.

- **Cabinet PASS — Steps 7–10 together (2026-09-16, CrossOver, `step10-deploy1`):** see the deploy log entry
  below. All four plan checkboxes ticked. Status: Complete (uncommitted — maintainer commits manually).

- **Post-PASS follow-up #2 (2026-09-16, built, `step10-deploy3`): BPM sync + STOP slow-motion (maintainer
  request).** RE: A3's `FUN_18003a1b0` (RE doc §3.5) — `mgr+0x38 = minBpm < 10 ∧ MOTION_STOP_SLOW ? 1/12 :
  MOTION_BPM_DEPENDENCY ? maxBpm/120 : 1` scaling the WHOLE scene's dt; retail `false`/`true`. Port: pure
  `tempo.rs` (`TempoMap` over the SSQ tempo nodes: `τ(mc)` piecewise-linear, `Δτ = Δtick/2048` per bpm-sync
  segment = 0.5 s per beat exactly, `Δms/12000` per stop segment, warps instant; `τ = 0` pinned to the measure
  boundary nearest music 0; `nodes_from_ssq_pairs` = the game's `round(td·1000/TPS+0.5)`; `snap_to_beats`;
  13 tests incl. 120/180/60 BPM, stop, A3-retail, warp, phase pin), `tempo_source.rs` (live DPS basename →
  SSQ mod-first unsplit/`_1..5` → `TempoConverter::entries` → map, on a std thread),
  `song_reset::live_dps_basename()` (vtable-gated, `+0xA0` MSVC string), `DanceSchedule::with_quantum(0.5)`
  (segment lengths snapped to beats in bpm-sync mode; 1 test), `config.rs::BackgroundDancersConfig
  {bpm_sync, stop_slow}` both default `true` (`ConfigFile.background_dancers`), `clock.rs` REWRITTEN to
  report the raw music count with NO latch (`ClockEvent::Latched{mc}` = first anchored frame only, `Rewound`
  informational; pre-song `None` ⇒ `PRE_SONG_MC_MS` −300; tail holds the last count), `lifecycle.rs`
  `tempo_tick` (per-frame DPS-pointer/basename watch, resolver poll, one INFO per map / one WARN per miss ⇒
  real time), `t = map.tau(mc)` (or `mc/1000`), `Session::new(…, tempo_opts)`. Units cross-checked against
  real charts (`dind2` 140, `goli` 150, `anan` 175 + 60 stops, `aeth` 384/192 + 2 stops). Harness 102 green.
  Docs: RE doc §3.5, AGENTS.md row + config entry, README config row + operator paragraph.

- **Post-PASS follow-up #1 (2026-09-16, built, `step10-deploy2`, superseded by #2's clock rewrite): rewind semantics.** Maintainer report: training
  fast-forward skipped the dancer/stage timeline correctly, REWIND restarted it from the beginning. Cause: the
  Step 7 clock re-latched `t0` on every count jump back (> 50 ms) — designed for the in-place restart but a
  training rewind/loop is the same signal. Fix (`clock.rs`): `t0` latches ONCE per run (anchor edge) and is
  never moved; `t = (count − t0)/1000` is a pure function of the music count, so a rewind moves the whole
  timeline back (dance/camera/stage loops re-simulate — `CameraSchedule::at(t)` on `t < prev_t`) and an
  in-place restart, which resets the count to the SAME song-start value the run latched at (`t0 = −276` on
  every song this boot), lands at `t ≈ 0` = clip 0 for free; a delayed restart runs `t` negative and the
  one-shot clips clamp to frame 0 (dancers hold their start pose through the countdown). `ClockEvent::
  Rewound { from, to }` replaces the re-latch variant (log `timeline follows (t = …, t0 kept)`); shadow /
  camera resets stay on `Latched` only. Test `rewind_moves_the_timeline_back_without_relatching` replaces
  the re-latch test (89 harness tests). AGENTS.md row updated. Awaiting one confirmation song.

## In flight

- Step 7 cabinet run (deploy #3). Owed: a Windows run (rides along).

## Deploy & test log

- **Steps 7–10 deploy — PASS (2026-09-16, CrossOver, gamemdx 20260915, `step10-deploy1`):** boot: `[+]
  dance_play_sequence_vtable (RTTI) @ +0x360AF8` (= the sweep value), tables ready, 0 WARNs from
  BackgroundDancers/bg-hide/scene3d across the whole boot (130 lines). 5 songs — `replicant00` + `rinon01`
  (`wear=[head+chest+forearm+face]`, `[5] part(s)` = the forearm twice, 15 instances: 8 stage + 1 dancer + 5
  part + 1 shadow), `boom01` + `jenny01`, `monitor01` + `jenny01`, `boom03` + `rage00`, `replicant04` +
  `alice00` (each `wear=[face]`, `shadow=true`). **Visibility gate:** `visible -- graph enabled 6383 / 5563 /
  5042 / 5045 / 5053 ms after request (t = 0.00, dps step Some(6))` — every window, always AFTER `built` (181–
  389 ms); song 1's early `items collected 46 ms` (stale enable bit, all items hidden) is harmless — the
  scene appeared at 6.4 s. **Camera director** line every song with the shuffled main/`_non` lists; `playing
  -- t0 = -276 ms` each song. **Movie size:** songs 1–2 (VIDEO SIZE fullscreen) `P1 Some(0) -> 2` at entry,
  `restored (1 side(s))` at exit — the maintainer saw the movie in the thumbnail with the 3D behind it; songs
  3–5 (VIDEO SIZE changed to ON in the options menu at 23:05) no override line, as designed. **Teardown:**
  `installed a 256-entry destroy-vector buffer (engine allocation header + no-op allocator)` once, every song
  `N destroy(s) queued 11–20 ms` → `all N destroyed 16–31 ms`, `created == released` (2/16/34/48/50),
  `stale=0`. **Close:** clean shutdown (`CNetworkManager::onTerminate`, rawinput/xinput disposed), NO `HARD
  FAULT` in `ddr_hook_crash.log` — the deploy-#2 crash is gone. Maintainer: "everything looks to be working
  fully … random dancer and stage every song … movies forced the thumbnail mode as expected". One behaviour
  note → the rewind follow-up above.

- **Step 10 deploy #1 expected lines (pending; `release/ddr_world_hook.step10-deploy1.dll` = Steps 7–10 —
  the ONE build to deploy):** on a MOVIE song with VIDEO SIZE = FULLSCREEN (or unset): after the pick INFO
  `movie size overridden for the song -- P1 Some(1) P2 None -> 2 (sized thumbnail; restored at window exit)`;
  on screen the movie plays in its small "ON" thumbnail rectangle with the 3D stage + dancers visible around
  it (no more black movie songs); at window exit `movie size restored (1 side(s)) at song-window exit`; back
  at song select the VIDEO SIZE row still shows FULLSCREEN (the restore beat the re-seed). With VIDEO SIZE =
  ON/OFF: no override lines, movie as configured. Toggling the mod OFF mid-session (0-0-0 menu): `mod disabled
  …`/`arcs freed at mod disable`, the next song is stock (2D background back, movie fullscreen). Boot with the
  mod OFF: only the 46 `[+] scene3d_*` derivation lines + `[+] dance_play_sequence_vtable (RTTI)`, no
  BackgroundDancers lines at all.

- **Step 9 deploy #1 expected lines (pending; `release/ddr_world_hook.step9-deploy1.dll` carries Steps 7–9):**
  per window the pick INFO reads `cameras=main:6 non:4` (boom00) / `main:7 non:4` (cyber00) etc.; `parsed …`
  unchanged shape; INSTEAD of `camera slot 0 written (interim …)`: `camera director -- main:["st001_st04",
  …] non:["st001_non02", …] (slot 0 written every frame)` right after the first build frame, plus (dev mode)
  `camera timeline -- t0: st001_stXX | 0@18.5s: st001_stXX -> st001_nonYY | 1@…`. **On screen:** the picked
  stage framed by its own shots — floor at the frame bottom, stage upright, dancers centred; each main shot
  plays out (they pan/dolly), then the next; ~1.5 s BEFORE every dance cut the view jumps to a `_non` angle
  for 1–2 s and returns to the next main shot; quick restart / scrub restarts the camera with the dance;
  Custom Resolution 1080p / SD 640×480 keep the same framing (render aspect is the 16:9 canvas). **FAIL
  signatures:** everything upside down / mirrored ⇒ the `up`/target rows (report the stage); stage far too
  small/large ⇒ the `half_tangent` recipe (report `fovV` from the camanm name); camera inside geometry on
  ONE stage only ⇒ that stage's camanm set (A3 had the same shot); `camera director produced no sample` WARN
  ⇒ schedule/clip mismatch (report the pick line); `fixed fallback camera` on a stage that has a row ⇒
  `parse:` WARNs name the missing `.camanm` members.

- **Step 8 deploy #1 expected lines (pending; same build as Step 7 deploy #3):** with
  `layeredfs.developer_mode: true`, run once per pin — `DDR_DANCERS_PIN=boom00,rinon00` (all 7 parts + the
  `:N` stage row 0), `DDR_DANCERS_PIN=,afro00` (head only), `DDR_DANCERS_PIN=,babylon00` (scale 0.4 face),
  `DDR_DANCERS_PIN=,emi00` (face only) — then unpinned random songs incl. one 2P. Per window: the pick INFO now
  carries `wear=[head+hips+chest+forearm+face]` (rinon00) / `wear=[head]` / `wear=[face]` and `arcs=` grows by
  the part count + 1 (`pl_shadow00.arc`); `parsed … {n} clip(s), [7] part(s), shadow=true` (rinon00: 5 parts →
  7 ParsedParts, the forearm twice); one `… [part] item built … (mode=0xB bones=1 … pass=0x2)` per part with the
  right forearm tagged `[part mirrored]` and BOTH forearm lines naming `pl_rinon00_forearm00`; one
  `pl_shadow00 [shadow] item built … mode=0xB bones=1` per dancer; `built … (S stage, D dancer, P part, W
  shadow)`; `boom00` row 0: `gm_boom00_bg … slot=0 pass=0x10 sort=-2` and `gm_boom00_stage … pass=0x10
  sort=-1`. **On screen:** rinon00 wears her head/hair, hips skirt, chest, BOTH forearms (the right one a
  mirror image of the left — thumb sides match) and face; afro00 his hair; babylon00's face sits on the
  0.4-scale body; emi00 her face; a soft dark square under each dancer's feet that follows them, grows when
  they crouch and shrinks on jumps, resetting on every restart; boom00 row 0's `bg`/`stage` render behind the
  other parts (LOWPRIO pass). **FAIL signatures:** a part floating away from the body ⇒ `part_world` order or
  the attach index (`.b2it` lookup) — report the part name; a part at the origin ⇒ the bone index read
  `bones[attach]` (check `parsed … part(s)` count); the right forearm inside-out/misplaced ⇒ `MIRROR`; the
  shadow missing ⇒ `shadow=false` in `parsed` (b2it/ground bones) or `pl_shadow00` not resident; shadow huge
  (2 m) ⇒ the Hips Δ sign; any `parse:` WARN names the member. Movie songs still black (Step 10).

- **Step 7 deploy #3 expected lines (pending):** cabinet steps as deploy #1 (`mods["background-dancers"]:
  true`, ≥ 3 songs 1P incl. songs where the READY panel is skipped fast, ≥ 1 song 2P, one quick restart
  (pinpad 1) and one quick-fail (pinpad 3), a training scrub/loop song, one movie song) PLUS: **close the
  game (X) after at least one song was played** — with a song running AND once from song select. Per
  window (in order): `stage=… seed=…` → `FileManager::Load accepted 3 of 3 arcs` → `parsed in N ms` →
  `… item built … slot=S` ×N → `built N ms after request -- N instance(s) attached hidden, 0 skipped` →
  `camera slot 0 written (interim fixed camera …)` → `first frame after attach -- graph enabled=false …`
  → `items not collected 180 frames …` INFO (normal) → **`visible -- graph enabled ~5000 ms after request
  (t = 0.00, dps step Some(5|6|7))`** — on EVERY window, never 33–43 ms, and ALWAYS after `built` →
  `bg-hide: bg_root layer 0x… alpha 0` (no `no live bg_root clip` WARN) → `items collected … items=N`
  (N = every built instance) → `playing -- t0 = …`. ONCE per boot (first teardown): `scene3d: installed a
  256-entry destroy-vector buffer (engine allocation header + no-op allocator) …`. **On screen:** every
  stage part AND the dancer(s) visible and dancing on EVERY song — including the 3rd+ song of the boot,
  after a quick restart (finish path: the scene vanishes during the new READY, `playing` again ~5 s later;
  in-place: `music count jumped back … re-latched`) and after a quick-fail. **Closing the game: NO `HARD
  FAULT` line in `ddr_hook_crash.log`** (the previous boots faulted at `gamemdx.dll+0x24178` on every close
  after a song). Movie song still mostly black = EXPECTED until Step 10. Any `destroyed by the ENGINE
  outside our teardown` WARN = report it.

- **Step 7 deploy #2 — PROGRESS + 3 bugs (2026-09-16, CrossOver):** 9 windows; songs 1–2 (`monitor00` 2P
  with zero00+alice01, `replicant03`) built BEFORE `visible` (177/69 ms vs 882/824 ms) and rendered with
  dancers dancing; from song 3 on `visible` fired at 33–43 ms, BEFORE `built` (90–134 ms) ⇒ every node
  built after that frame kept `node.hidden = true` (set at build, cleared only in the one `arm_hide` frame;
  pass 4 forces hidden) ⇒ "stage partially there / black, no dancers ever again" (the dancer is always the
  LAST instance built). Root of the early `visible`: `scene_manager::current_scene()==GAMEPLAY &&
  song_reset::live_dps().is_some()` passes for the pre-`createNextSequence` child (the stage-indicator
  sequence is still the active child for the first frames of scene 28; scenes 26/27 took < 1 frame each on
  those windows). Movie song: mostly black = the 2D hide (alpha 0 on `bg_root`) also hides the movie
  plane (Step 10's `Customize+0x30` thumbnail override is the intended answer; until then movies vanish) +
  the interim camera. **Crash on close reproduced and — after decoding against the cabinet's REAL build
  (20260915) — attributed to the SceneGraphManager SHUTDOWN freeing our destroy-vector buffer through the
  engine allocation header at `begin−0x20` (RE doc §3.4).** The session's first attribution (the tick's
  camera copy block, `+0x24178` read against the 20260825 program) was wrong. `created == released` every
  song (10..42). Teardowns 7–33 ms. All three fixed in code the same day (see Step 7's "Deploy-#2 fixes").

- **Step 7 deploy #1 — PARTIAL (2026-09-16, CrossOver):** 3 songs (`boom04`/`boom02`/`replicant04`), every
  instance built + collected (`default=0`, 7–9 items, `records` 14–15), clean teardowns (`created ==
  released`), `playing -- t0 = -277 ms` each song. Maintainer saw the boom stages rendered but the fixed
  Step 3 camera (4 m) sat inside the stage (upper geometry, no floor/dancer visible); `replicant04` = black
  background; ACCESS_VIOLATION when closing the game mid-song (`0x00006FFFFA724178`, unnamed thread, system
  module — the SAME site the Step 4 deploy-#2 boot faulted at when it was closed). Root causes + fixes (RE
  doc §3.3): (1) `visible` fired at scene 26 because the graph enable bit is still set during the loader —
  gate = enabled ∧ scene 28 ∧ live DPS; (2) interim camera = the add-on's verified `(0,1.6,5)→(0,0.9,0)`
  hFOV 76.8°; (3) shutdown: the dying sequence leaves our skinned items listed while resources/graphics die
  and no game-thread frame runs — `visit(4)` now refuses to push once `live_dps_probed()` is false
  (detour-free), `visit(2)` copies straight into the item with the bone count clamped to
  `ModelParameters.x`, engine-destroyed nodes are parked, the crash handler names the module.
- **Step 7 deploy #2 expected lines (pending):** same cabinet steps as deploy #1 PLUS: close the game (X)
  mid-song once, and (dev mode) `DDR_DANCERS_STATIC=1` is available as a bisect knob (poses published once).
  Per song: `stage=… seed=…` → `FileManager::Load accepted 3 of 3` → `parsed in N ms` → `item built …`
  ×N → `built … attached hidden` → `camera slot 0 written (interim fixed camera: eye [0,1.6,5] …)` →
  `first frame after attach -- graph enabled=false …` (NEW: must be false — the gate now waits for the
  DPS) → `items not collected 180 frames …` INFO (normal) → **`visible -- graph enabled ~5000 ms after
  request`** (the DPS step-5 edge, NOT 39 ms) → `bg-hide: … alpha 0` (no `no live bg_root` WARN) → `items
  collected …` → `playing -- t0 = …`. On screen: the stage floor + the dancer at the origin dancing, camera
  ~5 m back at 1.6 m height (large stages may still clip — Step 9). Closing mid-song: NO `HARD FAULT` in
  `ddr_hook_crash.log`; if one appears it now reads `in <module>.dll+0x…`. Any `node … destroyed by the
  ENGINE outside our teardown` WARN = report it (it means the engine reaches our nodes on a path we do not
  know).

- **Step 7 deploy #1 expected lines (pending):** cabinet steps: `mods["background-dancers"]: true` in
  `mod-config.json` (no env var needed; `DDR_DANCERS_PIN=<stage>[,<chara>]` only with
  `layeredfs.developer_mode` to force a pick), play ≥ 3 songs 1P, ≥ 1 song 2P, one song at SONG SPEED 150 %,
  one quick restart (pinpad 1) and one quick-fail (pinpad 3); if stable, a training scrub/loop song and a
  course. **Boot:** the 46 `[+] scene3d_*` lines, `scene3d: available …`, `BackgroundDancers: tables ready --
  27 stage rows (25 distinct stages), 26 dancers, 34 camera rows`, `BackgroundDancers: enabled -- random A3
  stage + dancers every song`. **Per song (in order):** `BackgroundDancers: stage=<key>[<row>] parts=N
  dancers=[<key>(M|F) x=+0.0] clips=[a>b>c] cameras=K arcs=3 seed=0x…` → `FileManager::Load accepted 3 of 3
  arcs -- parsing + polling residency` → `parsed in N ms -- P stage part(s), 1 dancer(s) with [14|13]
  clip(s)` (N is disk read + LZ77 + 33-bone × 14 clips parse; expect 50–300 ms; any `parse: …` WARN names a
  missing member) → one `… item built at 0x… N ms after request (mode=0xF|0xB bones=… slot=S pass=0x4|0x10|0x2
  sort=…)` per stage part + one for the dancer body (`pl_<key>` `mode=0xF bones=33 … pass=0x2`), each
  `default=0` (readiness gate) → `built N ms after request -- P+1 instance(s) attached hidden, 0 skipped` →
  `camera slot 0 written (fixed Step 3 camera)` → `first frame after attach -- graph enabled=false …` (READY
  dwell) → `visible -- graph enabled N ms after request (t = 0.00)` + `bg-hide: … alpha 0` (DPS step 5, ~5 s)
  → `items collected by SceneGraph::update … items=P+1 records=…` → `playing -- t0 = <count> ms` (the anchor
  frame; count may be negative). **On screen:** the picked stage's parts animating their loops behind the
  lane (the fixed camera frames the origin at ~4 m — parts of large stages will be off-frame until Step 9)
  with the dancer (faceless, body only) dancing from the song-start edge, hard cuts every ~20 s, 2P = two
  dancers at x ∓0.8. **Quick restart (in-place):** `music count jumped back (A -> B ms) -- schedule
  re-latched` + `playing -- t0 = … (re-latched)`; the dance restarts from its first clip. **Quick restart
  (finish path, fresh DPS):** the scene disappears during the new READY (graph disabled), then `playing --
  t0 = …` again. **At exit:** `song-window exit -- N node(s) disabled …` → `scene -- N destroy(s) queued …`
  → `scene -- all N node(s) destroyed …` → `N arc handle(s) freed after the scene teardown; scene3d
  textures: created=K released=K stale=0` (K grows by 2 × skinned instances per song — every stage part
  except `footpanel` is skinned). **FAIL signatures:** `resident with N bones but the .model file has M`
  (bone table mismatch — the file/resident disagreement would corrupt the upload; that model is skipped) ·
  dancer visible but frozen in T-pose ⇒ `visit(2)` not copying (board `seq`/`instance`) — check `playing`
  appeared · dancer exploding/garbage ⇒ bone order/seed (report the clip name from the pick line) · stage
  parts at wrong places ⇒ the partial-loop seed (`boom00 stage`, `boom01 stage`, `crystaldium00 bg`) ·
  nothing after `built` ⇒ same NO-GO ladder as Step 3 · `residency timeout … still missing […]` ⇒ loader
  (which model) · any teardown WARN as in Steps 3/4. Frame-time: watch for stutter with 2 dancers + 8 parts
  (NFR-3 budget 0.3 ms — `produce` evaluates ≤ 10 skeletons per frame).

- **Step 3 deploy #1 — GO (2026-09-16, build 20260915 / spice2x, CrossOver):** the boom00 foot panel
  rendered behind the lane where expected on 3 songs (screenshots: 1st STAGE untextured magenta panel,
  2nd STAGE fully textured), quick-restart and quick-fail had no ill effect, 2D background hidden and
  restored each song, no crash. Log: all six models resident 22–102 ms; `item built (mode=0xB … textures
  total=1 load=0 re=0 default=1)` on song 1 (cold arc — the DDS registered AFTER the item was built ⇒
  default/magenta texture), `load=0 re=1 default=0` on songs 2–3 (DDS still resident from the previous
  load ⇒ the build-time re-resolve fixed it); `node attached`, `camera slot 0 written`, `bg-hide … alpha
  0`, `first frame after attach -- graph enabled=false` (READY banner), `item collected by
  SceneGraph::update ~4.9 s after attach -- enabled=true visible-nodes=1 items=1 records=1` every song.
  **Two WARN classes, both understood and fixed in code the same day (not yet cabinet-observed):**
  (1) `item NOT collected 180 frames after attach -- graph enabled=false` — a false alarm (3 s < the
  READY dwell; the diagnostic threshold is now informational — see below); (2) `queue_destroy refused
  60 frames in a row` every song + `new window while the previous node is still Detaching -- parking
  it` — World's manager destroy vector has zero capacity forever (RE §2.7); `queue_destroy` now installs
  a 256-entry mod-owned buffer on first use and the Detaching phase times out into leak-node/free-arc.
  Per-frame texture re-resolve retry added for the cold-load case (`material textures re-resolved N ms
  after attach`). RE §2.7 has the full record.
- **Step 4 deploy #1 — FAIL (2026-09-16, CrossOver):** magenta flashing across the screen, no Emi,
  ACCESS_VIOLATION on quick-exit. Root cause: bone textures created `(bone_count, 4)` per the design;
  the upload writes ONE BONE PER ROW so the right shape is `(4, bone_count)` — the 33×4 texture's
  2 KB staging buffer took a 17 KB write every frame (heap corruption ⇒ flashes + crash at free). Fixed
  (`BONE_TEX_WIDTH = 4`; RE §2.8; design §4.2.3/§5.2 are wrong on this). Everything else in the log was
  right: readiness gate held each build until its DDS registered (`load=0 re=N default=0`), `items=3
  records=4` collected, and the Step 3 teardown fixes worked end to end (`installed a 256-entry
  destroy-vector buffer` → `3 destroy(s) queued 18 ms` → `all 3 node(s) destroyed … 30 ms` → `3 arc
  handle(s) freed … created=2 released=2 stale=0`). Shadow quad moved from y 0.02 (inside the opaque pad)
  to the pad surface, Emi stood on the pad.
- **Step 4 deploy #2 — PASS (2026-09-16, CrossOver):** Emi T-posing (bind pose, textured, faceless —
  faces are Step 8 part arcs) on the pad with the shadow under her for 3 songs, no artifacts, clean
  exits; `pl_emi00 item built … mode=0xF bones=33 … bone_tex=0x…/0x… default=0`, `items=3 records=4`,
  teardown 7–20 ms, `created=2/4/6 released=2/4/6 stale=0`, zero WARNs. Windows run still owed (nothing
  platform-specific in the path). RE §2.9.
- **Step 4 deploy #2 expected lines (kept for the Windows rerun):** Same cabinet steps as Step 3
  (`mods["background-dancers"]: true`, `DDR_DANCERS_SPIKE=1`, 3 songs, quick-restart, quick-fail).
  **Expected per song:** `FileManager::Load accepted 3 of 3 arcs`; the six `gm_boom00_*` residency lines;
  three `… item built at 0x… N ms after load (…)` lines — `gm_boom00_footpanel` (`mode=0xB bones=1 …
  bone_tex=0x0/0x0 … default=0`), `pl_emi00` (`mode=0xF bones=33 records=2 mats=2 pals=2 skinned=true
  bone_tex=0x…/0x… (two distinct non-zero) textures total=1 … default=0` — the readiness gate should hold
  the build until `mdx_emi01.dds` registers, so `N ms after load` for Emi is the DDS registration
  latency on a cold arc), `pl_shadow00` (`mode=0xB bones=1`); three `… node 0x… attached` lines; one
  `camera slot 0 written`; `bg-hide … alpha 0`; `first frame after attach -- graph enabled=false …
  (nodes attached so far: 1..3)`; `items collected by SceneGraph::update N ms after the first attach --
  … items=3 records=4` (footpanel 1 + Emi 2 + shadow 1). On screen: Emi (0.9× scale, ~1.5 m) standing
  upright in bind pose ON the boom00 footpanel with her body texture, a dark soft square under her feet.
  At exit: `song-window exit -- 3 node(s) disabled …`; ONCE per boot `scene3d: installed a 256-entry
  destroy-vector buffer into the SceneGraphManager`; `scene -- 3 destroy(s) queued N ms after window exit
  (items unlisted for 2 frames)`; `scene -- all 3 node(s) destroyed by the engine flush N ms after
  window exit -- node blocks freed`; `3 arc handle(s) freed after the scene teardown; scene3d textures:
  created=2k released=2k stale=0` (k = songs so far). **FAIL signatures:** Emi missing but footpanel
  present ⇒ skinned path (bone textures / upload / VS) — check the Emi build line and `records=4`; Emi
  present but untextured (magenta) for the whole song ⇒ readiness gate/retry regression; Emi distorted ⇒
  bone-texture layout or `ModelParameters`; `stale>0` or `created != released` after a teardown ⇒ handle
  lifecycle; any of the Step 3 teardown WARNs.
- Cabinet steps used (kept for reruns): `mods["background-dancers"]: true` in
  `mod-config.json`, env `DDR_DANCERS_SPIKE=1`, play 3 songs, also quick-restart (pinpad 1) and
  quick-fail (pinpad 3) mid-song. **Expected lines (in order, per song):** boot: the 46 `[+] scene3d_*`
  lines PLUS `[+] scene3d_texture_lookup / _default / _spin (derived)` (a `[-] scene3d_texture_lookup
  (optional)` WARN would only disable the re-resolve), `scene3d: available …`,
  `BackgroundDancers: enabled (spike harness ON)`, `BackgroundDancers[spike]: armed -- … render
  gm_boom00_footpanel …`. Per song: `FileManager::Load(…) accepted`; the six `scene3d: gm_boom00_<part>
  resident after N ms …`; `BackgroundDancers[spike]: footpanel item built at 0x… (mode=0xB bones=1
  records=1 mats=1 pals=1 skinned=false textures total=1 load=L re=R default=D)` — **L+R+D == 1; D=1
  means the DDS was not registered when the item was built ⇒ untextured panel (a WARN names it)**;
  `node 0x… attached under the graph root (item 0x…, pass mask 0x4, sort key 0)`; `camera slot 0 written
  (eye [0.0, 1.2, 4.0] …)`; `bg-hide: bg_root layer 0x… alpha 0`; `first frame after attach -- graph
  enabled=E visible-nodes=V items=I records=K (item listed: …)` (enabled=false is NORMAL before the DPS
  reaches step 5 — the READY banner); `item collected by SceneGraph::update N ms after attach -- graph
  enabled=true visible-nodes=1 items=1 records=1`. At song exit: `song-window exit -- node disabled,
  waiting …`; `node destroy queued N ms after window exit (item unlisted for 2 frames)`; `node destroyed
  by the engine flush N ms after window exit -- node block freed`; `mapset_boom00 freed (1 handle(s))
  after the node teardown`; `bg-hide: disarmed (layer 0x… alpha restored)`. **GO** = the boom00 footpanel
  (a ~1 m grey/white arcade foot panel, textured) visible behind the lane with the 2D background hidden,
  gone cleanly at song exit, zero WARNs. **NO-GO ladder:** `item NOT collected 180 frames after attach`
  WARN with `enabled=true visible-nodes=0` ⇒ the pass-4 gate/visit/push; `visible-nodes≥1 items=0` ⇒ the
  item push; `items≥1` but nothing drawn ⇒ collector filter / clip-space cull (camera) / material bind
  (texture stats) / draw — report with the log; a crash ⇒ report the faulting module+offset from the
  spice2x log. Teardown WARNs to watch: `still referenced by the engine's item list … leaking`
  (graph disabled with a stale list — expected NOT to happen on a natural song end), `dtor not
  observed`, `queue_destroy refused 60 frames`.

- **Step 2 deploy #1 — PASS (2026-09-16, build 20260915 / spice2x, CrossOver):** `scene3d: available`,
  all 46 derivations; per song `FileManager::Load … accepted` → all six `gm_boom00_*` resident at
  55–63 ms (bg/ripple/sp) and 99–111 ms (spot/stage/footpanel), `fully resident after 105–111 ms`, freed
  at window exit — identical on 3 songs, resource pointers identical across songs (slots freed and
  re-acquired ⇒ refcount balanced), zero feature WARNs. **Finding:** stage parts are SKINNED
  (`bg` 8 bones, `ripple` 19, `sp` 32, `spot` 11, `stage` 10 — `res+0x1C bit0 = 1`); only `footpanel`
  is rigid (1 bone). The design's "rigid stage parts" assumption is wrong — Step 4's skinned path covers
  every stage part except the footpanel. Expected lines were: — `scene3d: available (mgr=+0x… rm=+0x… tex
  create/release=+0x…/+0x… bgmovie=+0x… pool=+0x…)`, the 46 `[+] scene3d_* (derived)` lines,
  `BackgroundDancers: enabled (spike harness ON)`, `BackgroundDancers[spike]: armed …` +
  `… data/arc/mapset_boom00.arc resolves to …`. Per song: `BackgroundDancers[spike]: FileManager::Load(…)
  accepted -- polling residency`, six `scene3d: gm_boom00_<part> resident after N ms (bones=… records=…
  materials=… palettes=… skinned=…)`, `scene3d: mapset_boom00 fully resident after N ms (6 models)`, and at
  song exit `BackgroundDancers[spike]: mapset_boom00 freed (1 handle(s)) at song-window exit`.
  Expected field sanity: `footpanel bones=1 skinned=false records≥1`; `sp` the largest (materials ≥ 1);
  `bg` has 5 DDS textures. Failure signatures: `scene3d: … NOT fully resident after 20000 ms -- missing
  […]` (loader accepted the arc but the ModelFileCallback did not register — or the hash/name convention
  is wrong: the WARN prints the FNV of the first missing name for cross-checking), `FileManager::Load(…)
  returned -1` (path/engine refusal), `scene3d: … not found (mod folders or stock)` (resolution).

## Deviations & open questions

- **Step 5 — A3 bind seed wording (2026-09-16):** the handoff described `FUN_180190a90` as "matrix→quat of
  the row matrix with rows divided by scale"; the decompile shows A3 passes the UNNORMALISED matrix (raw
  `bindWorld` for roots, `diag(parentScale)·M` for children). The port is faithful to A3, not to the
  wording — identical on every unit-scale bone, and the only inexact case (a scaled bone's own rotation
  seed) is always overridden by a rotation track in stock data (RE doc §3.1). Also: 4 stock stage loops
  are PARTIAL (`boom00`/`boom00_g`/`boom01` `stage`, `crystaldium00` `bg`) — Step 7 must seed stage parts
  from the bind table (RE doc §3.2).
- **Step 5 — fixture tests need the install:** the JSON carries values only (no Konami bytes); the Rust
  fixture leg re-reads the arcs from `$DDR_WORLD_INSTALL` and skips cleanly without it. Tolerances: world
  matrices 2e-5 (f32 chain vs the f64 Python), samples 1e-5, camera positions relative (~2 500 cm).

- **Spike harness gate (2026-09-16, maintainer):** plan Step 2 said `DDR_DANCERS_SPIKE=1
  (developer_mode-gated)`; shipped as env-var ONLY. Rationale: the env var is an explicit opt-in already,
  and `layeredfs.developer_mode` disables the mod-folder index cache, so requiring it would test a
  non-production file-resolution path. The Step 7+ `DDR_DANCERS_PIN` dev knob keeps the design's
  developer_mode gate unless the maintainer says otherwise.

- **Step 3 RE amendments** (RE doc §2): (a) material textures are converter-resolved — the builder
  re-resolves into its copies via the OPTIONAL `texture_lookup_site` trio (not part of the all-or-nothing
  group: a miss only disables the re-resolve); (b) amendment 7 below is MOOT — palette/material copies
  are plain `memcpy`s (the engine reads them; our dtor never releases their handles); (c) frame stamp
  seed `0xFFFFFFFF` (never 0); (d) item mode `0xB` rigid / `0xF` skinned (private bones — A3's `9`
  shared the resource's bind array and wrote poses INTO it); (e) `ModelParameters.z = 0` always;
  (f) the node dtor frees the item but NOT the node block (the lifecycle frees it after polling
  `destroyed`); (g) teardown queues the destroy only after the engine's item list dropped the item for
  2 frames (the DPS disables the graph at `onInitialize` → stale list until step 5).

- **Design amendments from the Step 1 RE** (all recorded in the RE doc, none change the architecture):
  1. The mod-owned node MUST carry a valid `i32` sort key at `+0xE8` — `SceneGraph::update` `std::sort`s
     the visible vector by it (design §4.2.5 did not list the field). Use 0 for dancers, the `:N`
     priority for stage parts (A3 semantics).
  2. `visit(4)`'s ctx is `&p` where `p = graph+0x58` (pointer-to-pointer to the visible vector
     `{begin,end,cap}`); the push is `**ctx`. Never call the engine grow helper from the job thread —
     skip the push when `end == cap` (invisible one frame).
  3. The manager lock is libavs-win64 ordinal 16/17 (`avs_mutex_lock/unlock(i32 id)`) reached through
     IAT SLOTS the derivation publishes (`scene3d_mutex_lock_iat` / `_unlock_iat`) — the service reads the
     loader-patched pointer out of the slot at call time; never hardcode the ordinal.
  4. A3's model lookup-by-hash has no World twin: `model_registry::model_resource` walks the
     ResourceManager's model `std::map` itself, read-only, under the map's own mutex (offsets published).
  5. `+0x2A0/+0x2A4` (r−l, t−b) in design §5.3 are read by no World function — dropped from the camera
     write. Write protocol = fields + `+0x2B0 = 1` + `+0x2B1 = 1`.
  6. The node vtable needs only 2 slots (dtor, visit); the design's 8 is harmless over-allocation.
  7. The render item's palette copies must be ZEROED (mask/handle fields) before the engine-style copy so
     our dtor's release path only touches handles the copy registered (A3 ctor shape, §1.4).
- The design's `texture_create` signature was described as a "World twin of FUN_1802488e0 (20260721)";
  on 20260825 it is `FUN_180249c20`, located via the ArrowPalette factory's `(0x100,0x20,1,0x15,0x2002)`
  call site instead.

## Key facts for a cold resume

- Architecture: Option A hybrid — DLL builds render items + ONE flat node type + writes camera slot 0;
  the engine's own `MODEL:*` passes draw. Zero rendering detours. Nodes FLAT under the root (engine's
  destroy flush never calls a child's dtor).
- Maintainer overrides: never suppress movies (write `Customize+0x30` 0/1 → 2 for the song, restore at
  window exit); hide the REAL 2D bg clip via per-frame alpha-0 on the live `bg_root` AFP layer
  (`BgMovieActor(DAT_1806f2d38)+0x58 → BackgroundFrame+0x140`), no placeholder arc.
- Time base = content-domain music count (`GamePlayActor+0x178`), gated on
  `song_reset::first_anchored_frame()`; `t0` latched at DPS step 5 + anchored.
- A3 rules: fixed pool per sex (no `tu01`), Fisher–Yates playlist, 1.5 s hard cut both dancers,
  camera freeze < 2.0 s, `_non` hold `1 + U[0,1)`, x pitch 1.6 m, no idle, no BPM scaling.
- Step 3 is GO/NO-GO: if the engine rejects the hand-built render item, STOP (Option B = separate PDD).
- Spike harness state machine (`spike.rs`): `Phase::{None, Attached, Detaching, Destroying}` per window
  generation + an `orphan` slot for a previous window still tearing down; the engine's item list is the
  arbiter of when an item may be freed (`scene_graph::item_listed`).
- The 2D background hide is the FINAL mechanism (`background_hide.rs`), not spike-only.
- Ghidra: project `DDRWorld_Ghidra`, `gamemdx_20260825.dll` (World) + `gamemdx_20240402_A3_Final.dll`
  (A3). Always pass `program=`; never rename/comment/save. **The maintainer's cabinet runs gamemdx
  20260915** (`build timestamp of dll: 2026-09-09` in log.txt; md5 == `~/Desktop/ddr_modules/
  gamemdx_20260915.dll`), which is NOT in Ghidra — decode a `gamemdx.dll+0x…` crash offset against the
  20260915 file (capstone over the PE image, `scripts/sig_harness/shape_diff.py::Image` shape) or the
  sweep table in RE doc §1.9, never against the 20260825 program (deploy-#2 lesson).
- Engine allocation convention (RE §3.4): every engine-allocated buffer carries `{allocator* @-0x20, raw
  @-0x18, size @-0x10}` in front of it and frees walk that header — anything of ours the ENGINE may free
  needs the fake header (`scene_graph::destroy_reserve_begin`). Camera slot 0 is ACTIVE in stock World;
  never deactivate it.
- Builds for the sweep: `~/Desktop/ddr_modules` (20250805 / 20260224 / 20260721 / 20260825 / 20260915).
- Working tree also carries the maintainer's own concurrent edits (premium_free `ghost_cache.rs` id==0
  fix, AGENTS.md row, `docs/premium_free_stale_record_bug.md`, `lib.rs` v1.4 splash) — not part of this
  feature; leave them alone.
- Step 7 shape: ONE `frame_board` (static, 32 slots) is the only path from game-thread poses to attached
  items — never write an attached item's world/bones from the game thread; `Session.instances[i].slot == i`;
  stage `:N` parts get pass mask `0x10` + sort key = priority, plain parts `0x4`, dancers `0x2`; the
  visibility/clock decision is `clock::Clock::step(graph_enabled, anchored, count)` every frame (no phase
  ladder); the teardown is the spike's, verbatim over `Session::built()`.
- Step 6 rules as shipped: cut lead 1.5 s, camera freeze 2.0 s, `_non` hold `1 + U[0,1)`, no beat gate;
  `CameraSchedule::advance` is an event loop — never hand-roll per-frame camera state in the director.
- `core/anm` is the ONLY parser for `.anm`/`.camanm`/`.b2it`/`.rlist`/bone tables — std-only, mounted by
  the harness; `core::arc::{parse, extract}` extracts (and LZ77-decompresses) arc members for it.
- Never `git commit`/push; never write absolute local paths/usernames into tracked files.
