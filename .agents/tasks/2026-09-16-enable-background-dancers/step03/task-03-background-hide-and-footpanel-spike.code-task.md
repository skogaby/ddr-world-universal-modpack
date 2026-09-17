# Task: `background_hide.rs` (live `bg_root` alpha-0) and the footpanel GO/NO-GO spike harness

## Description
Add the design's final 2D-background mechanism (`mods/background_dancers/background_hide.rs`: per frame
while armed, find the live `bg_root` CMovieClip through `BgMovieActor → BackgroundFrame → clip slot`,
validate it against the CMovieClip pool, and set its AFP layer colour to alpha 0; restore alpha 1 once on
disarm) and extend the Step 2 spike harness so that, once `gm_boom00_footpanel` is resident, it builds the
render item + node (task 01/02), attaches it under the root, writes the fixed camera, arms the hide, and
at window exit disables the node, queues its destroy, disarms the hide and frees the arc only after the
dtor ran (5 s cap ⇒ leak + WARN). Every stage logs one line. This is the GO/NO-GO deploy.

## Background
The stock 2D background is `bg_root`, a CMovieClip created by `FUN_18003e5b0` into the BackgroundFrame's
slot `+0x140` (`scene3d_bg_clip_slot_off`); `BgMovieActor` is the singleton at `scene3d_bgmovie_actor`,
its frame at `+0x58` (`scene3d_bgframe_off`). The clip's AFP layer id is the u32 at `clip+0x08` (the same
field `overlay_element_styling` reads on its captured clips); `bm2d_api::layer_set_color_raw(layer,
r,g,b,a)` is the non-owning libafp colour setter already used on game-owned layers. The clip object must
lie inside the 0x400-slot pool at `scene3d_cmovieclip_pool` (stride 0x240) on a slot boundary — the
validation that makes the per-frame write safe when the frame/clip is torn down under us. Maintainer
override: NEVER suppress a movie; the hide targets only the 2D background clip.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-16-enable-background-dancers/design/detailed-design.md` (§4.3.6 `background_hide.rs`, §7.3 spike protocol items 1–2, §6 error handling rows for bg-hide + dtor timeout)
- RE record: `docs/background_dancers_research.md` §1.6 (background objects), §2.4 (footpanel geometry + camera framing), §1.3/§2 for what the log must prove

**Additional References (if relevant to this task):**
- `src/mods/background_dancers/spike.rs` (the Step 2 harness this extends — keep its residency logging)
- `src/mods/overlay_element_styling/capture.rs` (`SHARED_CAPTURE` — the design's FALLBACK path when the derivations are absent; Step 3 wires the primary path only and logs a WARN naming the fallback if the primary is unavailable)
- `src/services/bm2d_api.rs::layer_set_color_raw` / `layer_color_available`
- `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md` Step 3 (demo + cabinet test list)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `src/mods/background_dancers/background_hide.rs`: `pub fn arm()`, `pub fn disarm()`, `pub fn on_frame()` (game thread, called from the mod's frame callback): while armed, `live_bg_layer()` → `layer_set_color_raw(layer, 1,1,1,0)`; remember the last layer id; on disarm write `(1,1,1,1)` to it once. `fn live_bg_layer() -> Option<u32>`: `actor = *(sites.bgmovie_actor)` (readable, non-null) → `frame = *(actor + bgframe_off)` → `clip = *(frame + bg_clip_slot_off)`; validate `clip ∈ [pool, pool + count*stride)`, `(clip − pool) % stride == 0`, `is_readable(clip, 0x10)`; `layer = *(clip + 0x08) as u32`, 0 ⇒ `None`. Miss counter: after 60 consecutive misses while armed, ONE WARN per arm (`bg-hide: no live bg_root clip (…)`). `layer_color_available() == false` ⇒ arm logs one WARN and stays inert.
2. Spike harness (`spike.rs`) extension, all on the game thread: when `gm_boom00_footpanel` becomes resident (the existing per-frame poll): `ResourceView::new(res)` → `render_item::build(&view, 4)` → INFO `BackgroundDancers[spike]: footpanel item built (mode=0x… bones=… records=… mats=… pals=… textures total=N load=N re=N default=N)`; `node::new_node(item, 4, 0)` → `scene_graph::attach_under_root(node)` → INFO `… node attached under root (node=0x… item=0x…)`; `scene_graph::write_camera0(&CamSample::perspective([0,1.2,4],[0,0.8,0],[0,1,0], 0.5, 16/9, 0.1, 100))` → INFO `… camera slot 0 written`; `background_hide::arm()`. Any step failing ⇒ one WARN naming it, the built pieces freed/left alone as safe (an unattached node's item is freed through our own dtor call path: call `node_dtor(node, 1)` directly), and the spike continues without rendering.
3. Window exit (leaving {26,27,28}) and `teardown()`: `set_enabled(node, false)`, `set_hidden(node, true)`, `queue_destroy(node)` (retry each frame while it returns false, WARN after 60 frames), `background_hide::disarm()`; then poll `is_destroyed(node)` per frame; when true: INFO `… node destroyed after N ms` and free the arc (existing `release`); after 5 s without the dtor: WARN `… node dtor not observed in 5000 ms -- leaking node+item, freeing arc anyway` and free the arc (leak preferred to use-after-free). Also handle re-entry into a new window while a destroy is pending (fresh generation; the old node is only polled, never touched).
4. Per-frame while attached: log ONCE (first frame after attach) `… first frame with node attached: enabled=… hidden=… visible-vector len=N items=N` by reading `graph+0x58/+0x60` and `*(graph+0x30)+0x08/+0x00` (probed) — the diagnostic that distinguishes "not pushed" from "pushed but culled/rejected" on a NO-GO.
5. `mod.rs`: route the frame callback to `background_hide::on_frame()` + `spike::on_frame()`; `disable()` disarms the hide (restoring alpha) before tearing the spike down.
6. Rust Quality Rules: no `unwrap`/`expect`/indexing in the frame/scene callbacks; every engine pointer probed; no state mutex held across `run_on_render_thread`; every write into game memory (layer colour) is idempotent-per-frame and paired with the restore.

## Dependencies
- Tasks 01 and 02
- `scene3d::sites()` background fields; `bm2d_api::layer_set_color_raw`
- Cabinet: `mods["background-dancers"] = true`, env `DDR_DANCERS_SPIKE=1`

## Implementation Approach
1. `background_hide.rs` + wiring in `mod.rs`.
2. Spike state machine: `Loading → Built/Attached → Destroying → Done` per window generation; item/node build on residency; teardown + dtor poll.
3. First-frame diagnostic line.
4. `cargo check` → `cargo fmt` → `./build.sh`; hand the DLL to the maintainer with the expected log lines.

## Acceptance Criteria

1. **GO**
   - Given the mod ON + `DDR_DANCERS_SPIKE=1` on the cabinet
   - When a song plays
   - Then the boom00 footpanel is visible behind the lane with the 2D background hidden, the log shows `item built` → `node attached` → `camera slot 0 written` → `first frame … visible-vector len≥1`, at song exit `node destroyed after N ms` → `mapset_boom00 freed`, background restored; three songs + quick-restart (1) + quick-fail (3) mid-song reproduce the shape with zero WARNs

2. **NO-GO diagnosis**
   - Given nothing is drawn
   - When the log is read
   - Then the first-frame line pins the failing stage (visible-vector len 0 ⇒ pass-4 gate/push; len ≥1 and items 0 ⇒ item push; items ≥1 ⇒ collector/cull/draw) and the texture stats line says whether materials resolved

3. **Background hide is safe**
   - Given a song whose background clip is torn down mid-song (quick-fail)
   - When frames keep running
   - Then no fault occurs (pool validation), at most one WARN after 60 misses, and the next song's background is stock-visible again after disarm

4. **Gates**
   - Given the finished change
   - When `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`, `./build.sh`, `./scripts/validate_background_dancers.sh` run
   - Then all are clean/green

## Metadata
- **Complexity**: Medium
- **Labels**: background-dancers, step-3, spike, go-no-go, background-hide
- **Required Skills**: the repo's frame/scene callback idioms, the bm2d_api raw layer setters
- **Generated By**: code-task-generator 2026-09-16
- **Source Plan**: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md`
- **Plan Step**: Step 3: Render item, scene node, root attach/destroy, camera slot 0, background hide — static footpanel (spike GO/NO-GO)
