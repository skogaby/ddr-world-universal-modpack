# Task: `scene_window.rs` — extract the asset/scene phases of the gameplay `Window` into a reusable `SceneWindow`

## Description
Move the cabinet-proven load / parse / residency-gated build / engine-destroyed-node parking / texture
retry / attached diagnostics / three-phase teardown machinery out of `lifecycle::Window` into
`src/mods/background_dancers/scene_window.rs` as `SceneWindow`, so the Step 5 preview driver can run
the identical lifecycle on its own scenes. Gameplay-only concerns — the music-count clock, the tempo map,
the 2D `background_hide`, the movie-size override, camera slot 0, the one-shot playing/visible/rewind logs
and the `DDR_DANCERS_STATIC` knob — stay in a thin `lifecycle::Window` wrapper. Refactor ONLY: same
constants, same log lines (byte-identical for gameplay), same orphan handling, same leak/timeout rules.

## Background
`lifecycle.rs` (`src/mods/background_dancers/lifecycle.rs`) is ~1400 lines. Its `Window` struct mixes:

- asset phase (`AssetPhase` Requested/Built/Abandoned, `arcs: Option<ArcSet>`, `parse_rx`, `session`,
  `requested_at`, `warnings_logged`, `built_logged`, `frames_since_attach`, `first_frame_logged`,
  `collected_logged`) — driven by `request_load` (arc load + parse thread) and the first half of
  `drive_live` (parse → `Session::new`, `build_pending`, residency timeout), plus `retry_textures`,
  `attached_diagnostics` and the "destroyed by the ENGINE outside our teardown" parking loop;
- scene phase (`ScenePhase` Live/Detaching/Destroying/Done, `teardown_started`, `unlisted_frames`,
  `queue_retries`) — `begin_teardown`, `drive_teardown`, `finish_window`, and the disable-time
  `neutralise_on_disable` loop;
- gameplay-only state (`clock`, `tempo*`, `visible_logged`, `playing_logged`, `rewind_logged`,
  `camera_written`, `hide_armed`, `static_published`).

`State { generation, live: Option<Window>, orphan: Option<Window> }`: a previous window still tearing down
when a new one opens is parked as the orphan and only ever driven to its end; a second orphan is dropped
with its arcs leaked (WARN). Every engine call is game-thread only; the parse thread never touches the
engine. Log lines are all prefixed `BackgroundDancers:`; field logs are read against those exact
strings (the deploy log in `progress.md` references `built … instance(s) attached hidden`, `song-window
exit -- … node(s) disabled`, `destroy(s) queued`, `all … node(s) destroyed by the engine flush`,
`arc handle(s) freed after the scene teardown`).

The design's `PreviewWindow` (§4.6, §5.3) = `{ identity, scene_window: SceneWindow, built_at, seed }`;
its driver will call the same phases with a per-side tag (`BackgroundDancers: preview P1`) and a
`Session` built with `slot_base` / `item_pass_mask` / `HullPlan::none()` / a synthetic schedule.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-21-background-dancers-selection-options/design/detailed-design.md`
  (§4.6 "preview/window.rs", §5.3, §6 residency / teardown rows)

**Additional References (if relevant to this task):**
- `.agents/planning/2026-09-21-background-dancers-selection-options/implementation/plan.md` (Step 4)
- `src/mods/background_dancers/lifecycle.rs` (the code being split), `src/mods/background_dancers/session.rs`
  (`Session::build_pending`, `built()`, `built_mut()`, `all_settled()`), `src/services/scene3d/scene_graph.rs`
  (`item_listed`, `queue_destroy`, `graph_stats`), `src/services/scene3d/node.rs` (`set_enabled`,
  `set_hidden`, `is_destroyed`, `free_node_block`), `src/services/scene3d/arc_set.rs`
- `docs/background_dancers_research.md` §3.3/§3.4 (why the teardown is shaped this way — port, don't redesign)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. New engine-facing file `src/mods/background_dancers/scene_window.rs`:
   - `pub(super) const RESIDENCY_TIMEOUT_MS / TEARDOWN_TIMEOUT_MS / UNLISTED_FRAMES_REQUIRED /
     NOT_COLLECTED_DIAG_FRAMES / NOT_COLLECTED_WARN_FRAMES / TEXTURE_RETRY_FRAMES` moved from
     `lifecycle.rs` (same values).
   - `pub enum AssetPhase`, `pub enum ScenePhase` moved (same variants, same doc comments).
   - `pub struct LoadedArcs` (the `ArcSet` + accepted count) returned by
     `pub fn load_arcs(pick: &Pick, opts: &ParseOptions) -> LoadedArcs` (= today's `arc_set::load` over
     `pick.arcs_for(opts)`), and `LoadedArcs::free(self)` (the "window closed before we ran" path).
   - `pub struct SceneWindow { tag: String, scope: &'static str, pick, arcs, parse_rx, session,
     requested_at, assets, scene, warnings_logged, built_logged, frames_since_attach, first_frame_logged,
     collected_logged, teardown_started, unlisted_frames, queue_retries }` with:
     - `pub fn start(tag: impl Into<String>, scope: &'static str, pick: Pick, loaded: LoadedArcs,
       opts: ParseOptions) -> SceneWindow` — spawns the `bg-dancers-parse` thread (or the "parse thread
       could not be spawned" WARN), logs the "FileManager::Load accepted {} of {} arcs" INFO / "no arc
       loaded" WARN exactly as today (prefix = `tag`), `assets = Requested | Abandoned`;
     - `pub fn drive_assets(&mut self, make_session: impl FnOnce(&Pick, Parsed, Instant) -> Session)
       -> bool` — the parse→session (once) block incl. its warnings/INFO, the "nothing parsed" WARN
       (wording via `scope`: gameplay "this song"), `build_pending` + the "built … attached hidden" INFO,
       the residency-timeout WARN; returns `has_built` and, when true, advances `frames_since_attach`
       (the `> 0 ⇒ += 1` rule);
     - `pub fn park_engine_destroyed(&mut self)` (the "destroyed by the ENGINE outside our teardown" loop);
     - `pub fn publish(&mut self, t: f32, visible: bool)` — `director::produce` + the `node_shown` drop
       loop (the deploy #2 comment travels with it);
     - `pub fn retry_textures(&mut self)`, `pub fn attached_diagnostics(&mut self)` (moved verbatim);
     - `pub fn begin_teardown(&mut self, exit_label: &str) -> bool` — the body of today's
       `begin_teardown` after its generation/`background_hide` prologue: returns `false` (nothing to do)
       when `scene != Live`; frees the arcs + logs "arc handle(s) freed at {exit_label} (no nodes)" when
       nothing was built; otherwise `hide_all`, disable+hide every built node, `Detaching`, the
       "{exit_label} -- {} node(s) disabled, waiting…" INFO. Gameplay passes `"song-window exit"` so the
       lines stay identical;
     - `pub fn drive_teardown(&mut self, label: &str) -> bool` and `pub fn finish(self, label: &str)`
       (moved verbatim; the "{} ms after window exit" wording unchanged);
     - `pub fn neutralise(self) -> bool` — the per-window body of `neutralise_on_disable` (disable+hide
       live nodes, leak the arcs when nodes exist and the scene is not Done, else free them); returns
       `leaked`;
     - accessors: `pick()`, `session()`, `session_mut()`, `assets()`, `scene()`, `has_built()`,
       `requested_at()`, `since_request_ms()`, `frames_since_attach()`.
   - Doc comment: game thread only; the parse thread never touches the engine; every path panic-free.
2. `lifecycle.rs`:
   - `struct Window { generation: u64, scene: SceneWindow, clock, tempo, tempo_dps, tempo_basename,
     tempo_rx, tempo_failed_logged, visible_logged, playing_logged, rewind_logged, camera_written,
     hide_armed, static_published }`; `struct State { generation, live: Option<Window>, orphan:
     Option<SceneWindow> }`.
   - `request_load`: `let loaded = scene_window::load_arcs(&pick, &ParseOptions::GAMEPLAY);` then the
     existing generation / `IN_WINDOW` check (`loaded.free()` on mismatch), the existing orphan parking
     (`prev.scene` → `prev.scene.scene() != Done`; an orphan is the `SceneWindow`), then `st.live =
     Some(Window { generation, scene: SceneWindow::start("BackgroundDancers", "this song", pick, loaded,
     ParseOptions::GAMEPLAY), … })`.
   - `drive_live`: `let has_built = w.scene.drive_assets(|pick, parsed, requested_at| { let eff =
     style::effective(); let hulls = style::hull_plan(&eff); Session::new(pick.clone(), parsed,
     requested_at, tempo_options(), eff.style, hulls, 0, None) });` then the unchanged gameplay body
     (fixed camera, `w.scene.park_engine_destroyed()`, `tempo_tick`, clock, hide arm, the publish gate ⇒
     `w.scene.publish(t, visible)`, camera director, `w.scene.retry_textures()`,
     `w.scene.attached_diagnostics()`).
   - `begin_teardown(window_gen)`: generation check + `background_hide::disarm()` stay; then
     `w.scene.begin_teardown("song-window exit")` and `ACTIVE.store(true)` when it returned `true`.
   - `on_frame`: `drive_teardown` / `finish_window` become `w.scene.drive_teardown("scene")` /
     `w.scene.finish("")` and `o.drive_teardown("orphan scene")` / `o.finish("orphan window's ")`.
   - `neutralise_on_disable`: `leaked |= w.scene.neutralise()` per live/orphan; `frame_board::clear_all()`
     + the two summary lines stay here.
3. `mod.rs`: `pub mod scene_window;`.
4. No new log strings for the gameplay path; no constant changes; `cargo check` clean; `cargo fmt`.

## Dependencies
- task-01 (`ParseOptions`, `Pick::arcs_for`) and task-02 (`Session::new(.., 0, None)`) of this step.

## Implementation Approach
1. Create `scene_window.rs` by MOVING code (cut/paste, then adapt `w.` → `self.`); keep every string
   literal; diff the moved bodies against the originals before deleting them from `lifecycle.rs`.
2. Rewrite `lifecycle::Window` / `State` around `SceneWindow`; adapt `request_load`, `drive_live`,
   `begin_teardown`, `on_frame`, `neutralise_on_disable`.
3. `cargo check --target x86_64-pc-windows-msvc`; grep both files for every `log_info!`/`log_warn!` string
   and confirm the gameplay set is unchanged (a before/after `rg -o '"BackgroundDancers: [^"]*'` list must
   match); `./scripts/validate_background_dancers.sh`; `cargo fmt`; `./build.sh`.

## Acceptance Criteria

1. **Same log surface**
   - Given the pre-task `rg -o '"BackgroundDancers: [^"]*'` list over `lifecycle.rs`
   - When the same command runs over `lifecycle.rs` + `scene_window.rs` after the task
   - Then the multiset of gameplay strings is identical (only a `{tag}`/`{scope}` placeholder replaces the
     literal prefix / "this song" where they were factored).

2. **Same constants**
   - Given `scene_window.rs`
   - When its six constants are read
   - Then they equal 20_000 / 5_000 / 2 / 180 / 900 / 1200 (20·60).

3. **Orphan handling unchanged**
   - Given a live window whose scene is not `Done` when a new song window opens
   - When `request_load` runs
   - Then the previous `SceneWindow` is parked as the orphan (WARN "parking it"), an older orphan is
     dropped with its arcs forgotten (WARN), and `on_frame` drives the orphan with the `orphan scene` /
     `orphan window's ` labels.

4. **Builds**
   - Given the crate
   - When `cargo check --target x86_64-pc-windows-msvc`, `./scripts/validate_background_dancers.sh` and
     `./build.sh` run
   - Then all three are clean.

5. **Cabinet regression (maintainer)**
   - Given the release DLL
   - When one 1P song and one 2P song play with dancers
   - Then the log carries the same lifecycle lines as before the step (pick / `FileManager::Load accepted`
     / `parsed in` / `built` / `visible` / `song-window exit` / `destroy(s) queued` / `destroyed by the
     engine flush` / `arc handle(s) freed`), with no new WARN.

## Metadata
- **Complexity**: High
- **Labels**: background-dancers, refactor, engine-facing
- **Required Skills**: Rust, the mod's lifecycle state machine
- **Generated By**: code-task-generator 2026-09-21
- **Source Plan**: `.agents/planning/2026-09-21-background-dancers-selection-options/implementation/plan.md`
- **Plan Step**: Step 4: `Pick`/`Session` generalisation + `scene_window.rs` extraction
