# Task: camera director — A3 stage-mode camanm sequencing driving camera slot 0

## Description
Replace the interim fixed camera with the stage's `.camanm` sets sequenced by the A3 rules (plan Step 9,
design §4.3.3/§4.3.4/§5.3): the pick splits the stage row's `stage_camera_resources.rlist` fields into the
shuffled main / `_non` lists (`selection::camera_lists`, already pure-tested); the parse thread reads
`data/arc/camera/stage_camera.arc` (our own `ArcReader`, NOT a `FileManager::Load` — the engine needs
nothing from it) and parses every `data/camera/long/<name[..5]>/<name>.camanm` of both lists
(`core::anm::anm::parse` handles the type-4 camera chunk); the `Session` owns a `schedule::CameraSchedule`
(main/non `ClipRef`s + the pick seed) and a `CameraState` advanced by the event loop every frame
(`advance(st, prev_t, t, &dance)`; `at(t)` after every clock (re)latch); the director samples the current
clip at `clip_frame(t − clip_start, …)` with `core::anm::camera::sample_camera(anm, bytes, frame, 1.0,
1.0)` and the lifecycle writes the sample into camera slot 0 EVERY visible frame
(`scene_graph::write_camera0`). Without a usable camera set (no row, no main clip parsed) the Step 7 fixed
camera stays as the fallback (written once). Dev mode logs the camera timeline at song start.

## Background
Design §4.3.3: main cycles on finish; frozen while a dance cut is within 2.0 s; at a cut (`t` crosses cut −
1.5 s) the `_non` rotation takes over with `hold_until = t + 1 + U_k`, then the NEXT main clip resumes; no
beat gate in v1 — all already implemented and host-tested in `schedule.rs` (17 tests incl. the A3 timeline);
this task only WIRES it. `core/anm/camera.rs` is the verified recipe (position cm → m, target = eye −
10·row2, up = row1, `t' = tan(½·atan2(2, 2·tan(fovV/2)·aspect))`, b/t = ∓t'/(16/9), near/far from the
slots — e.g. `st001_st02`: near 0.1, far 32768, fovV 6.6° ⇒ hFOV ≈ 86°, eye (−3.5, 2.7, 5.0) m). The
render is always the internal 16:9 1280×720 canvas (Custom Resolution changes the output, not the render
aspect), so `OUTPUT_ASPECT` stays 16/9. `scene_graph::CamSample` is field-identical to
`core::anm::camera::CamSample`. `CameraSchedule::advance` with `to ≤ from` only re-evaluates `frozen`, so
a rewind MUST go through `at(t)` (the lifecycle's `ClockEvent::Latched`). Camera clips are one-shots
(`loops = false`); the schedule's `ClipRef::duration_s` = `frame_count / fps`.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-16-enable-background-dancers/design/detailed-design.md` (§4.3.3, §4.3.4,
  §5.3)
- Plan: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md` Step 9
- `docs/3d_model_format_research.md` §6 (camanm semantics + the game's projection)
- `.agents/planning/2026-09-16-enable-background-dancers/research/a3-runtime-rules.md` §3/§4 (camera rules)

**Additional References (if relevant to this task):**
- `src/mods/background_dancers/schedule.rs` (`CameraSchedule`, `CameraState`, `ClipSel`), `selection.rs`
  (`camera_lists`, `camanm_member_path`), `src/core/anm/camera.rs` (`sample_camera`),
  `src/services/scene3d/scene_graph.rs` (`write_camera0`, `CamSample`), `lifecycle.rs` (the interim camera
  block + the clock events)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `session.rs`: `Pick` gains `camera_main: Vec<String>` / `camera_non: Vec<String>` (from
   `camera_lists(rng, &camera_row)` in `assemble_pick`; summary `cameras=main:N non:M`); `pub const
   STAGE_CAMERA_ARC: &str = "data/arc/camera/stage_camera.arc"`; `ParsedCameras { main: Vec<Clip>, non:
   Vec<Clip> }`, `Parsed.cameras: Option<ParsedCameras>` (None when the arc is unreadable or no main clip
   parsed; each missing member = one warning); `Session` gains `camera: Option<CameraSchedule>`
   (`CameraSchedule::new(main refs, non refs, pick.seed)`), `camera_state: Option<CameraState>`,
   `camera_prev_t: f32`, `pub fn reset_camera(&mut self)` (state = None), `pub fn has_camera(&self) -> bool`.
2. `director.rs`: `pub fn camera_frame(sess: &mut Session, t: f32) -> Option<scene_graph::CamSample>`:
   `dance = sess.schedule?`, `sched = sess.camera?`; `st = match camera_state { None => sched.at(t), Some(s) =>
   sched.advance(&s, prev_t, t, dance) }` (a `t < prev_t` also re-simulates via `at`); store `st`/`prev_t`;
   clip = the `Clip` for `sched.clip_of(&st)` (main[i % n] / non[i % m]); `frame = clip_frame(t −
   st.clip_start, dur, fps, loops)`; `sample_camera(anm, bytes, frame, 1.0, 1.0)` → `scene_graph::CamSample`
   (field copy). `pub fn camera_timeline(sess: &Session, until_s: f32) -> String` — one line per dance cut
   `k@c: <main clip> -> <non clip or "-">` for the dev log.
3. `lifecycle.rs`: the interim fixed camera is written once ONLY when `!sess.has_camera()`; otherwise every
   frame with `visible`: `if let Some(cam) = director::camera_frame(sess, t) { scene_graph::write_camera0(&cam) }`
   with one INFO per window at the first write (`camera director -- main:[…] non:[…]`, plus the timeline
   under `layeredfs.developer_mode`) and one WARN per window if the write is refused; `ClockEvent::Latched`
   ⇒ `sess.reset_camera()` next to `reset_shadow()`.
4. Host test (harness, pure): none new beyond `schedule.rs`'s — add ONE engine-side test in `director.rs`
   mapping `core::anm::camera::CamSample` → `scene_graph::CamSample` field-for-field.
5. Gates: `cargo check --target x86_64-pc-windows-msvc` (+ `--tests`) → `cargo fmt` → `./build.sh` →
   `./scripts/validate_background_dancers.sh`.

## Dependencies
- Steps 7/8 code.

## Implementation Approach
1. `session.rs` (pick lists, parse, schedule + state) → `director.rs` (`camera_frame`, timeline) →
   `lifecycle.rs` (per-frame write, fallback, logs) → gates.
2. Deploy checklist in `progress.md` ("Step 9 deploy #1 expected lines").

## Acceptance Criteria
1. **Cabinet (maintainer deploy)**
   - Given `mods["background-dancers"]: true`
   - When songs are played
   - Then the camera cycles through the stage's shots (each `st0NN_stXX` runs to its end, then the next),
     cuts to a `_non` angle ~1.5 s before every dance cut and returns to the next main shot after ~1–2 s;
     the floor sits at the frame bottom with the stage upright; the log carries `camera director -- main:[…]
     non:[…]`; quick restart / scrub re-simulates (camera restarts with the dance); Custom Resolution 1080p
     and SD 640×480 keep the same framing

2. **Fallback**
   - Given a stage row without camanms (or `stage_camera.arc` missing)
   - When the song plays
   - Then the interim fixed camera is written once and one WARN names the missing set

3. **Gates**
   - Given the finished change
   - When the gates run
   - Then all are clean/green

## Metadata
- **Complexity**: Medium
- **Labels**: background-dancers, step-9, camera
- **Required Skills**: the schedule API, the camera recipe
- **Generated By**: code-task-generator 2026-09-16
- **Source Plan**: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md`
- **Plan Step**: Step 9: Camera director — A3 stage-mode sequencing with the re-projection
