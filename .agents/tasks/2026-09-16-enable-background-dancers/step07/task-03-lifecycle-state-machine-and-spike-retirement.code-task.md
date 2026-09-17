# Task: `lifecycle.rs` — the per-song window state machine, clock, eligibility, teardown; the spike retired

## Description
Replace `spike.rs` with the real lifecycle (design §4.3.1, §3.2): rlists loaded once at `enable`
(`startup.arc` → `core::arc` → `core::anm::rlist`), candidates built with an `exists` over
`arc_set::resolve_path`; at the FIRST entry into the song window {26, 27, 28}: eligibility (mod enabled ∧
`scene3d::is_available()` ∧ rlists ∧ ≥ 1 entered side via `stage_records::side_entered`), seed
(`seed_from(QPC, scene)` or the `DDR_DANCERS_PIN` override under `layeredfs.developer_mode`), pick, INFO,
`FileManager::Load` of the pick's arcs (render thread), the parse thread; per frame: `Requested` → `Built`
(parse done ∧ `build_pending` built every buildable instance; 20 s ⇒ `Abandoned` WARN), `Built` →
`Visible` when the graph is enabled (`scene_graph::graph_stats().enabled` — DPS step 5, FR-9: the scene
appears in its t = 0 pose), → `Playing` when `song_reset::first_anchored_frame()` (t0 = the music count;
INFO), then `t = (count − t0)/1000` with a rewind re-latch (count decreased ⇒ t0 = count) and
`director::produce` every frame; the Step 3 fixed camera written once at `Built`; `background_hide` armed
at `Visible`; window exit ⇒ the spike's proven teardown (disable + hide nodes → unlisted 2 frames → queue
destroys → dtors → node blocks → arcs, 5 s caps) + hide disarm + `frame_board::clear`. `spike.rs` is
DELETED; `mod.rs` wires the lifecycle; `is_active()` = service available ∧ rlists loaded ∧ candidates.

## Background
FR-8/FR-9/FR-12/FR-13/FR-14 and the §6 error table are the contract. The clock: `+0x178` is a per-frame
CACHED raw count that holds the frame tick until the anchor lands (`song_reset::current_raw_music_count`
already sanity-bounds it; `first_anchored_frame()` is a STATE predicate true for the whole in-song phase).
In-place `song_reset`s rewrite the anchor and the count jumps back ⇒ the rewind re-latch restarts the dance
from clip 0 (A3 restarts the playlist on every song start). Training scrubs forward jump `t` forward (the
schedule re-simulates). The `finish`-path quick restart leaves the window (28→27→28 stays INSIDE it —
FR-12: same pick, same arcs; the lifecycle only re-arms the Playing gate). Quick-fail with skip-results goes
29→24 (window exit). The spike's teardown state machine (Attached → Detaching → Destroying, the orphan
slot, the destroy-vector buffer, the 5 s caps) is cabinet-proven — port it, do not redesign it.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-16-enable-background-dancers/design/detailed-design.md` (§3.2, §4.3.1,
  §4.3.8 diagnostics, §6, FR-8/9/12/13/14)
- Plan: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md` Step 7 (tests +
  demo)

**Additional References (if relevant to this task):**
- `src/mods/background_dancers/spike.rs` (teardown state machine, diagnostics — port verbatim where it
  applies), `src/services/song_reset/mod.rs` (`current_raw_music_count`, `first_anchored_frame`,
  `on_song_reset`), `src/services/stage_records.rs` (`side_entered`), `src/services/scene3d/scene_graph.rs`
  (`graph_stats`, `write_camera0`, `item_listed`, `queue_destroy`)
- `.agents/planning/2026-09-16-enable-background-dancers/progress.md` (cabinet expected-lines for Steps
  3/4 — the new lines must stay recognisable)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `lifecycle.rs`: `pub fn init_tables() -> bool` (enable-time: read `data/arc/startup.arc` via
   `arc_set::read_bytes` → `core::arc::parse/extract` the four rlists → `rlist::parse`; build
   `stage_candidates`/`dancer_candidates` with `exists = |arc| arc_set::resolve_path(&format!("data/arc/
   {arc}")).is_some()`; log counts; false + WARN when no stage or no dancer); `pub fn on_scene_change(prev,
   next)`; `pub fn on_frame()`; `pub fn teardown_on_disable()`; `pub fn tables_ready() -> bool`.
2. State (`Mutex<State>` + a few atomics for the O(1) idle path): `Phase::{Idle, Requested, Built, Visible,
   Playing, Abandoned}` for the live window + the spike's teardown phases for the scene, `generation`,
   `Option<Session>`, the parse handle (`Arc<Mutex<Option<Parsed>>>` + `JoinHandle`), `t0: Option<i32>`,
   `last_count`, the camera-written flag, the orphan.
3. Window entry (scene ∈ {26,27,28} from outside): eligibility; `n = entered sides` (0 ⇒ skip this window
   silently... unless `stage_records` is unavailable ⇒ WARN once); seed: `DDR_DANCERS_PIN` honoured only when
   `config::layeredfs().developer_mode` (check the crate's config accessor) — pinned stage/dancers replace
   the random ones, logged; `make_pick`; INFO `background-dancers: stage=<key>[row] dancers=[<key>(<sex>)
   x=…] clips=[first 3…] arcs=K seed=0x…`; `run_on_render_thread(request_load(gen))` = `arc_set::load` +
   spawn the parse thread; `Requested`.
4. Per frame: `Requested`: parse done? → build; every instance built or skipped ⇒ `Built` (INFO with ms +
   counts); > 20 s ⇒ `Abandoned` (WARN, arcs stay until exit). `Built`/`Visible`/`Playing`: camera once;
   `graph_stats().enabled` ⇒ `Visible` (arm hide, `produce(t = 0, visible = true)`); `first_anchored_frame()
   && count.is_some()` ⇒ `Playing` (t0 latch, INFO `playing t0=<count> ms`); Playing: `count < last_count −
   50` ⇒ re-latch (INFO once per song "rewind → re-latched"); `t = (count − t0) / 1000`; `produce(t, true)`;
   texture retries + the Step 3/4 attached diagnostics (keep the log line shapes, prefix `BackgroundDancers:`).
5. Window exit / disable: the spike's `begin_teardown` / `drive_one` / `finish_arcs` / `neutralise_on_disable`
   ported over `Session.instances`; plus `frame_board::clear` for every slot at teardown start and the
   parse thread joined or detached (never blocked on — if still running at exit, its result is dropped by
   generation check).
6. `mod.rs`: remove `spike`; `enable()` = `lifecycle::init_tables()` + callbacks; `is_active()` =
   `scene3d::is_available() && lifecycle::tables_ready()`; module doc updated to Step 7.
7. Host tests: none new beyond the pure pieces already covered (the clock rule `t = (count − t0)/1000` +
   rewind re-latch as a tiny pure `clock.rs`/function with 3 tests: first latch, forward, rewind).
8. Diagnostics per §4.3.8; every WARN once per song.

## Dependencies
- Tasks 01–02.

## Implementation Approach
1. Port the spike's state machine into `lifecycle.rs` around a `Session`; add the window-entry pick path
   and the per-frame Built/Visible/Playing driver.
2. Wire `mod.rs`; delete `spike.rs`; `cargo check --target x86_64-pc-windows-msvc`; `cargo fmt`; `./build.sh`;
   harness green.
3. Hand the maintainer the deploy checklist (below) and read `$DDR_WORLD_INSTALL/log.txt` afterwards.

## Acceptance Criteria

1. **Cabinet (maintainer deploy)**
   - Given `mods["background-dancers"]: true`
   - When 10 consecutive random songs are played (1P and 2P), plus SONG SPEED 150 %, a quick restart, a
     training scrub forward/backward + loop, a quick-fail with skip-results, and a course stage
   - Then every song shows a random stage with random dancer(s) dancing from the song-start edge, torn down
     at exit; the log carries the pick / Built / Visible / Playing lines and zero WARNs

2. **Fail-open**
   - Given an install missing `mc_female.arc`
   - When a female dancer is picked
   - Then the parse warnings drop that dancer, the stage still renders, and one WARN names the file

3. **Gates**
   - Given the finished change
   - When `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`, `./build.sh`,
     `./scripts/validate_background_dancers.sh` run
   - Then all are clean/green

## Metadata
- **Complexity**: High
- **Labels**: background-dancers, step-7, lifecycle, teardown, clock
- **Required Skills**: the repo's scene-callback / render-thread / song_reset services, careful state machines
- **Generated By**: code-task-generator 2026-09-16
- **Source Plan**: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md`
- **Plan Step**: Step 7: Director, session and lifecycle — first fully animated random song
