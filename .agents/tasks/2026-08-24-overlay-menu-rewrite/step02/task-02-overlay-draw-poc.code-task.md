# Task: overlay_draw diagnostics + tinted-quad POC (env-gated)

## Description
Add the impure half of the `overlay_draw` spike: a per-frame entry point called from
the widget wrapper render hook that (a) logs per-scene active-command-list diagnostics,
and (b) — behind the `DDR_OVERLAY_DRAW_POC` env var — emits a scissored, semi-transparent
tinted quad using the game's stock shader program 0, with full state restoration. This
is the go/no-go probe for the animated theme backgrounds (plan Step 8).

## Background
The design's only genuinely new RE is proving that the DLL can draw its own quad in ANY
scene via the game's command list. Everything needed is already resolved or shipped:
`render_notes_hook::active_command_list()` reads the scene-agnostic screen-renderer
global; the default shader container global is derived (`pass_rewrite.rs`
`DEFAULT_SHADER_GLOBAL`); mine_render shows the arena write-cursor mechanics and the
state-restore invariants. The unknowns are cabinet-only: whether the active list is
valid at the wrapper-render site in every scene, arena headroom, and where in the frame
our emission lands relative to game content and our own widgets (z recipe).

Production builds must be unaffected: everything here is inert without the env var
except the diagnostics, which must be cheap and bounded (once per scene id).

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-24-overlay-menu-rewrite/design/detailed-design.md (§4.7 emission + gates, §6 degradation ladder)

**Additional References (if relevant to this task):**
- .agents/planning/2026-08-24-overlay-menu-rewrite/research/shader-spike.md (spike plan stages 2–4, §1 emission sites, §6 risks)
- docs/custom_arrow_renderer_research.md (§3 layer slots, §8 emission timing safety)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `overlay_draw::on_wrapper_render()` called from `wrapper_render_hook` in
   `src/services/widget_renderer.rs` (before the original call), panic-free, costing
   one relaxed atomic check when fully inactive.
2. **Diagnostics (always on, bounded):** on first frame in each distinct scene id
   (via `scene_manager::current_scene()`), log one INFO with: scene id, active list
   pointer, the arena fields (`size @ cl+0x0C`, write ptr `+0x10`, base `+0x18`), and
   the default-shader global's program count (`*(u32*)(shaderObj+4)`) — every read
   null/range-guarded; unavailable states log once per scene id too. Cap the
   scene-id log set (e.g. 64 entries) so a pathological scene churn can't spam.
3. **POC emission (only when `DDR_OVERLAY_DRAW_POC` is set at process start, latched
   once):** per frame, behind ALL gates (list non-null, plausible cursor, shader object
   resolves, program count ≥ 1, arena headroom > emitted size + margin):
   - build the record block with the task-01 encoder: scissor-on (test rect, e.g.
     x=200 y=100 w=880 h=520) → SetVSConstantF (c48 block: time + rect, per design §5)
     → SetShader(default container, program 0) → one untextured quad (semi-transparent
     tint, e.g. 50 % black) → SetShader restore to the previously-active program if
     capturable, else program 0 → scissor-off
   - copy into the arena and bump the cursor exactly like mine_render does
   - on any gate failure: skip silently this frame; latch one WARN per failure class
4. Environment-variable gate follows the repo's existing dev-env pattern (see
   `DDR_SONG_RATE_FAULT` / `DDR_MOVIE_SYNC_PROBE` handling for precedent).
5. State restoration discipline per mine_render's invariants — blend untouched (the
   quad uses whatever blend is active; acceptable for a POC), shader restored, scissor
   disabled after.
6. No new AOBs expected; consume existing derived globals via their public accessors
   (add narrow `pub(crate)` accessors where needed rather than re-deriving).

## Dependencies
- task-01 (record encoder) — the POC emits through it.

## Implementation Approach
1. Read `render_notes_hook.rs` (active_command_list), `mine_render.rs` (cursor bump +
   restore), `pass_rewrite.rs` (shader global + program-count gate) before writing.
2. Implement diagnostics first; validate autonomously (boot → attract covers the
   attract band incl. the demo-gameplay loop; harvest log).
3. Implement the POC emission; validate: boot with the env var, confirm no crash
   through several attract cycles, then HAND OFF to the maintainer for visual
   confirmation (quad visible/positioned/tinted in attract, song select, gameplay)
   and the z-probe (quad vs game UI vs our widgets).
4. Record findings (per-scene list validity, arena sizes observed, z outcome) in
   `docs/` notes per the spike plan — the go/no-go input for Step 8.

## Acceptance Criteria

1. **Inert in production**
   - Given the env var is unset
   - When the game runs
   - Then behavior is unchanged except ≤1 diagnostic INFO line per scene id, and the
     per-frame cost is one atomic check + a scene-id comparison.

2. **Diagnostics observed autonomously**
   - Given an autonomous CrossOver boot to attract
   - When the log is harvested
   - Then per-scene INFO lines show a non-null, advancing command list and the
     default-shader program count for every scene in the attract band.

3. **POC stability**
   - Given the env var is set
   - When the game runs through several full attract cycles
   - Then no crash, no visual corruption outside the test rect, and any gate
     refusals are latched WARNs (not spam).

4. **Visual + z verification (maintainer)**
   - Given the maintainer runs with the env var
   - When they observe attract / song select / gameplay
   - Then the tinted quad renders inside the test rect in all three, and its layering
     relative to game UI and DLL widgets is recorded for the z recipe.

## Metadata
- **Complexity**: High
- **Labels**: overlay-draw, spike, RE, cabinet-validation
- **Required Skills**: Rust unsafe/game-memory discipline, repo hook conventions, command-list internals
- **Generated By**: code-task-generator 2026-08-24
- **Source Plan**: .agents/planning/2026-08-24-overlay-menu-rewrite/implementation/plan.md
- **Plan Step**: Step 2: Shader spike — command-list emitter POC (go/no-go for animated themes)
