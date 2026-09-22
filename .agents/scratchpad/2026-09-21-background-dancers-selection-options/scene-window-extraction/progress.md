# progress — scene-window-extraction (Step 4 task-03)
- [x] `scene_window.rs`: the six constants (`pub(super)`), `AssetPhase`, `ScenePhase`, `LoadedArcs` + `load_arcs(pick, opts)`,
      `SceneWindow::{start, drive_assets(make_session) -> has_built, park_engine_destroyed, publish, retry_textures,
      attached_diagnostics, begin_teardown(exit_label) -> bool, drive_teardown(label), finish(label), finish_silent,
      forget_arcs, neutralise() -> leaked}` + accessors. Bodies moved verbatim (`w.` → `self.`), `tag`/`scope` prefixes.
- [x] `lifecycle.rs` (1426 → ~940 lines): `Window { generation, scene: SceneWindow, clock, tempo*, latches }`,
      `State.orphan: Option<SceneWindow>`; `request_load` = `load_arcs` → generation check (`loaded.free()`) → orphan
      parking (`forget_arcs` / `finish_silent`) → `SceneWindow::start("BackgroundDancers", "this song", …)`; `drive_live`
      = `drive_assets(|pick, parsed, requested_at| Session::new(.., style::effective(), hull_plan, 0, None))` then the
      unchanged gameplay body; `begin_teardown` / `on_frame` / `neutralise_on_disable` delegate. Module doc updated.
- [x] Log-surface check: `logs/log-strings-before.txt` vs `logs/log-strings-after.txt` (factored `{tag}`/`{scope}`/exit-label
      placeholders normalised back) — 55 strings, identical multiset. Constants 20_000/5_000/2/180/900/1200 unchanged.
- [x] `cargo check` 0 warnings (`logs/cargo-check.log`); harness 141 ✓ (`logs/harness.log`); `./build.sh` release clean
      (`logs/build.log`); `cargo fmt` no unrelated churn.
## Deviations
- The camera-director INFO's `{:?}` camera lists are cloned once (only until `camera_written`) because the pick now sits
  behind `SceneWindow` (immutable borrow) while `camera_frame` needs the session mutably — same text, no per-frame cost.
- `finish_silent` / `forget_arcs` added for the two orphan-parking arms that touched `arcs` directly.
## Cabinet regression (maintainer)
- One 1P + one 2P song with dancers: the lifecycle lines (pick / `FileManager::Load accepted` / `parsed in` / `built` /
  `visible` / `song-window exit` / `destroy(s) queued` / `destroyed by the engine flush` / `arc handle(s) freed`) must match
  the pre-step shape; no new WARN.
Status: Complete (uncommitted — maintainer commits manually); cabinet regression pending
