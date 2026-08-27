# Task: Theme shaders — passthrough VS + three theme pixel shaders, build integration

## Description
Author the HLSL for the animated menu backgrounds and integrate them into
the shader build: one shared passthrough vertex shader (forwards the
c48/c49 time/rect/param constants to the pixel shader through
interpolators) and three `ps_3_0` theme pixel shaders — arrows (scrolling
low-contrast DDR-arrow field), bubbles (drifting translucent circles),
wavefield (geometric grid riding a 3-D wave surface) — under
`shaders/src/themes/`. Extend `scripts/build_shaders.sh`'s BLOBS manifest
and produce the compiled `.d3dbc` blobs under
`data_mods/shader_fixes/blobs/`.

## Background
Step 8 of the overlay-menu rewrite (design §4.7 + Appendix C). The
emitter (task-03) draws one scissored quad over the modal rect
(60,60,1160,600 on the 1280×720 canvas) with SetShader(default container,
theme program); the VS receives constants via tag-0x14 records in the
c48 window — **no PS-constant record exists in this engine**
(docs/overlay_draw_research.md:109), so every value the PS needs must
ride a VS interpolator.

Constant block (design §5):

| Register | Contents |
|----------|----------|
| c48 | `time_seconds, rect_x, rect_y, unused` |
| c49 | `rect_w, rect_h, theme_param0, theme_param1` |

Approved decisions (2026-08-25): time is monotonic seconds **wrapped
modulo 3600** — author every animation with a period that divides evenly
into the wrap (or is fast enough that a once-per-hour phase jump is
imperceptible); low-contrast subtle visuals (they sit BEHIND a
mostly-opaque gradient panel — design table calls arrows "low-contrast");
agent authors the visuals, maintainer tunes at the demo.

Current facts (verified 2026-08-25):
- `scripts/build_shaders.sh`: golden path = repo-committed fxc
  9.29.952.3111 (`tools/fxc/fxc.exe`) under the CrossOver bottle
  (`WINE_BOTTLE=bemani`), version-pinned; Docker/vkd3d fallback is for
  development only, never for committing. fxc rejects leading-/ POSIX
  paths — all paths handed to it must be repo-relative. BLOBS manifest
  lines are `"<hlsl name>:<profile>:<entry>:<output blob>"`;
  `SRC_DIR=shaders/src`, `OUT_DIR=data_mods/shader_fixes/blobs`. The
  script ends with a blob-stats dump via `gsp_pack.py`.
- Existing sources: `shaders/src/gs_screencommand_{arrow,default,judge}.hlsl`
  (profiles ps_3_0 / vs_3_0; entries `ps_main` / `vs_persp_main`). The
  persp VS binds `register(c48)`/`register(c49)` — the theme shaders
  reuse the same window with different semantics (safe: constants are
  re-emitted per pass).
- No `shaders/src/themes/` directory exists yet.
- The quad the emitter draws is an UNTEXTURED tag-0x03 quad whose
  vertices are screen-space corner positions + one D3DCOLOR; the theme
  VS must reconstruct normalized rect UVs for the PS from the
  interpolated position + the c48/c49 rect (the stock quad path supplies
  no UV stream the PS can rely on — derive `uv = (pos.xy − rect.xy) /
  rect.wh` in the VS and pass it through an interpolator).
  Study `shaders/src/gs_screencommand_default.hlsl`'s `vs_persp_main`
  input/output signature first and match the stock VS's input layout —
  the vertex declaration the engine binds for tag-0x03 quads is
  whatever the stock gs_screencommand VS consumes.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-24-overlay-menu-rewrite/design/detailed-design.md (§4.7 background rendering, §5 constant block, Appendix C Shadertoy porting workflow)

**Additional References (if relevant to this task):**
- docs/overlay_draw_research.md (tag map, VS-interpolator constraint)
- docs/shader_replacement_research.md (container/bytecode conventions)
- shaders/src/gs_screencommand_default.hlsl (existing c48/c49 VS precedent + I/O signatures)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. **`shaders/src/themes/theme_common.hlsl`** (or equivalent shared
   include): the passthrough VS entry `vs_theme_main` (profile
   `vs_3_0`) reading `register(c48)`/`register(c49)`, outputting
   position (stock transform — mirror how the stock/persp VS maps the
   2D-context screen coordinates to clip space), rect-normalized UV,
   and the time/theme params as TEXCOORD interpolators. ONE shared VS
   blob for all themes.
2. **Three PS files** under `shaders/src/themes/`:
   - `theme_arrows.hlsl` (`ps_main`, `ps_3_0`): scrolling field of DDR
     arrow silhouettes (procedural SDF chevron/arrow shapes, a few
     size/phase-varied layers drifting upward), deep blue/purple hues
     matching the RHYTHM palette, LOW contrast.
   - `theme_bubbles.hlsl`: drifting/bobbing translucent circles (SDF
     circles from a small hash grid, slow vertical drift + horizontal
     bob), dark-teal/warm hues matching BUBBLES.
   - `theme_wavefield.hlsl`: geometric grid riding a 3-D wave surface
     (perspective-ish grid lines displaced by traveling sine waves),
     charcoal/green matching WAVEFIELD.
   Each outputs opaque-ish dark colors with modest alpha — the panel
   gradient sits ON TOP (menu panel is a separate widget above the
   quad); the quad itself replaces the "static" look behind the panel,
   so it should read as a subtle animated backdrop, not a light show.
   Every animation loops with a period dividing 3600 s (use periodic
   functions of time; avoid unbounded phase accumulation).
3. **Manifest**: add BLOBS lines producing
   `theme_passthrough.vs.d3dbc`, `theme_arrows.ps.d3dbc`,
   `theme_bubbles.ps.d3dbc`, `theme_wavefield.ps.d3dbc` in
   `data_mods/shader_fixes/blobs/`. Keep the existing four lines
   untouched.
4. **Compile** via `./scripts/build_shaders.sh` (fxc golden path — the
   bottle is on this machine). All four blobs must compile within
   ps_3_0/vs_3_0 limits (this IS the feasibility gate; if a PS blows
   the instruction budget, simplify the effect and note it).
5. No Rust changes in this task (synthesis consumes the blobs in
   task-02).

## Dependencies
- None (first task of the step; pure asset/build work).

## Implementation Approach
1. Read the existing default/arrow HLSL for the engine's VS I/O
   conventions and the c48/c49 binding pattern.
2. Author theme_common VS; compile it alone first (fxc feasibility).
3. Author the three PS effects one at a time, compiling each; keep
   instruction counts comfortably inside ps_3_0 (the build script's
   stats dump shows per-blob instruction counts).
4. Full `./scripts/build_shaders.sh` run; verify all 8 blobs (4 old +
   4 new) byte-stable except the new ones.

## Acceptance Criteria

1. **Blobs build clean**
   - Given the extended manifest
   - When `./scripts/build_shaders.sh` runs on the fxc golden path
   - Then all four new `.d3dbc` blobs land in
     `data_mods/shader_fixes/blobs/`, version-pin check passes, and the
     existing four blobs are unchanged.

2. **Bytecode kinds are correct**
   - Given the new blobs
   - When inspected (`gsp_pack.py` stats / `validate_blob` convention)
   - Then the VS blob carries the `0xFFFE` version token and the three
     PS blobs carry `0xFFFF`, all within SM3 limits.

3. **Constant/interpolator contract**
   - Given the theme VS source
   - When reviewed against the design
   - Then it consumes ONLY c48/c49, derives rect-normalized UV, and
     forwards time + theme params via interpolators (no PS constant
     registers referenced in any theme PS).

## Metadata
- **Complexity**: Medium-High
- **Labels**: shaders, hlsl, build, mod-menu, theme
- **Required Skills**: HLSL SM3, procedural shader authoring, repo shader build conventions
- **Generated By**: code-task-generator 2026-08-25
- **Source Plan**: .agents/planning/2026-08-24-overlay-menu-rewrite/implementation/plan.md
- **Plan Step**: Step 8: Animated shader backgrounds
