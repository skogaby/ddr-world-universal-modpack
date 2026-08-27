# Research: Shader Background RE Spike Scope

Date: 2026-08-24. Goal: draw a procedurally-animated, custom-pixel-shader quad confined
to the modal rect, in ANY scene, above the game but below our widgets. Verdict:
**well-scoped, moderate; a 7-step spike with clear success criteria.** All claims cited.

## Key facts

1. **Command-list access is scene-agnostic.** `render_notes_hook::active_command_list()`
   reads the derived `screen_renderer_state` global (`DAT_1806f1fb8`, lives for the
   process): `state = *global`, `index = *(state+0x68)`, `list = *(state+0x40+index*8)`
   (`src/services/render_notes_hook.rs:230-246`; same helper in `mine_render.rs:613-620`).
   Only the lane-pass *hook site* is gameplay-specific. Records are consumed later on a
   worker thread (`docs/custom_arrow_renderer_research.md` §3, §8 Option C1), so emitting
   during frame building is safe.
2. **Best every-scene emission site: the already-installed
   `widget_renderer::wrapper_render_hook`** (`widget_renderer.rs:88-121`) — fires every
   frame in every scene on the render thread (already hosts `input_manager::poll`). Our
   wrapper is a node in the game's own render list, so at that point the engine is
   mid-layer-draw and the active list should be live. Needs one cabinet probe.
   Alternative: a new detour on the layer dispatcher `FUN_18002b530` (11 slots @
   `DAT_1806f1d20`) — needs a new AOB.
3. **Record sequence for one quad** (tag map in research doc §3):
   scissor 0x0C `{enable,x,y,w,h}` → SetShader 0x13 `{shaderObj@+8, programIdx@+0x10}`
   (`mine_render.rs:624-638`) → SetVSConstantF 0x14 (base c48, payload inlined into the
   arena — self-contained; `pass_rewrite.rs:495-520`) → optional SetTexture 0x11 →
   geometry: 0x04 textured quads / 0x03 untextured / 0x05-0x06 DrawVertices (pixel-space
   coords; per-vertex z reaches the VS but output z stays 0 — **painter's order only**,
   §4.3) → restore shader/texture/blend/scissor (`mine_render.rs:590-606` invariants).
   Write cursor = arena bump at `cl+0x0C/+0x10/+0x18`; **no emitter checks capacity —
   arena size undocumented (spike unknown).**
4. **The DEFAULT shader container (`gs_screencommand_default`) is boot-resident in ALL
   scenes** — cached into `DAT_1806f0558` at graphics init from `data/arc/shader.arc`
   (`docs/shader_replacement_research.md` §3); the DLL already derives that global
   (`signatures.rs:3413-3446`) and dereferences it (`pass_rewrite.rs:57,282-289`).
   Extending it with extra programs at synthesis time is the established mechanism
   (`shader_synthesis.rs:327-341`). Two wrinkles: (a) the default container is currently
   only overlaid when perspective is on — menu-bg must become a synthesis input; (b)
   `pass_rewrite` hardcodes perspective = program index 1 — menu-bg programs must append
   AFTER and keep index 1 stable in every synthesis configuration.
5. **The ≥program-count gate is mandatory** — the tag-0x13 handler has NO bounds check
   (`pass_rewrite.rs:16-19`); always verify `*(u32*)(shaderObj+4) ≥ programIdx+1`.
6. **Constants/time:** only a VS-constant record exists (c48+ window). No PS-constant
   record — per-frame time goes into c48+ and forwards to the PS via an interpolator
   (extra TEXCOORD in the vs_3_0 output). Per-frame animated constants are precedented
   (`pass_rewrite::emit_persp_constants` uploads c48/c49 every pass, every frame).
7. **Shader model / compile path:** SM3 (`vs_3_0`/`ps_3_0`) HLSL via fxc 9.29
   (`scripts/build_shaders.sh`, `tools/fxc/` under the CrossOver bottle).
8. **Z recipe:** emit the background quad into the widget layer's list BEFORE our render-
   list nodes draw (e.g. in `wrapper_render_hook` before `hook.call`), with menu widgets
   created after the wrapper → quad above game, widgets above quad. Fallback: emit late
   (above everything) and create the menu's widgets even later.

## Spike plan (front-loaded in the implementation plan)

1. **Static RE:** decode `FUN_18002b530` + `DAT_1806f1d20` slot table; identify the
   widget render list's slot. *Success:* slot→content map; dispatcher AOB drafted.
2. **Diagnostic deploy:** log `active_command_list()` per scene from the wrapper hook.
   *Success:* non-null advancing list in attract / select / gameplay.
3. **Untextured quad POC:** scissor + SetShader(default, prog 0) + tag-3 quad + restore.
   *Success:* visible tinted quad in all three scenes; no crash over a full loop.
4. **Z probe:** vary emission before/after `hook.call`; test widget creation order.
   *Success:* reproducible "above game, below widgets" recipe.
5. **Synthesis extension:** append a menu-bg program (persp stays index 1); trivial
   animated PS. *Success:* container validates; live program count reflects it.
6. **Animated bind POC:** tag-0x14 time constant + gated SetShader + quad.
   *Success:* animated procedural background; perspective/stock visuals unaffected.
7. **Scissor/state soak:** modal-rect clipping, restoration, versus gameplay, scene
   churn; measure arena headroom (read `cl+0x0C` extremes). *Success:* multi-hour soak
   clean; headroom documented.

## Cabinet-only unknowns / crash classes

- Active-list validity at the wrapper site in every scene; which layer slot we draw in.
- CommandList arena capacity (overflow = heap corruption; no bounds checks anywhere).
- Record-size discipline: one wrong `size` field desyncs the walker (crash).
- SetShader bounds gate (garbage handle forwarded to render thread if violated).
- State leakage corrupting downstream scene draws if restoration is incomplete.
- Program-index stability vs `player-perspective` across synthesis configs.

## Shadertoy portability (added after maintainer question)

Feasible with three constraints:

1. **Single-pass "Image" shaders only.** No render-to-texture in the command-list model —
   multi-pass Shadertoy (Buffer A/B/…) cannot be ported. Feedback/trail effects are out.
2. **GLSL → HLSL SM3 port + budget.** Manual but mechanical translation
   (`mainImage(fragColor, fragCoord)` → ps_3_0 main; iTime/iResolution → c48-window
   values forwarded through interpolators; iChannel textures → tag-0x11 bound PNGs we
   ship or synthesize). ps_3_0 instruction limits and cabinet-GPU fill rate rule out
   heavy raymarchers; plasma/tunnel/geometric/starfield-class shaders fit comfortably.
   Every port must compile through fxc 9.29 (the golden path) — that compile IS the
   feasibility test per shader.
3. **Licensing (public release).** Shadertoy's default license is CC BY-NC-SA 3.0;
   authors may override per shader (some CC0/MIT). For the open-source release: only
   incorporate shaders whose license permits redistribution, keep the author/URL/license
   header in the ported HLSL source, and prefer CC0/MIT/permissive ones. A ported
   CC BY-NC-SA shader would impose share-alike/non-commercial terms on that file.
