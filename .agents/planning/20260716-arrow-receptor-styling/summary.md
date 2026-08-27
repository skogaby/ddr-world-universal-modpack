# Summary — Playfield Styling (Arrow / Receptor Scale + Opacity)

PDD planning completed 2026-07-16.

## Artifacts

| File | Content |
|---|---|
| `rough-idea.md` | Initial concept + the culling consideration |
| `idea-honing.md` | 8 Q&As — anchor, scope, ranges, latch semantics, mechanics interactions, degradation gates, doubles/identity, acceptance criteria |
| `research/existing-code.md` | Repo leverage: `overlay_element_styling` template, arrow-render signatures, layout system, side attribution, constraints |
| `research/arrow-render-re.md` | Ghidra-verified RE: renderer class family, quad-fill call graph, collector culling (patch site, both builds), guideline draw/emitter path, constants, dead ends, hookability |
| `design/detailed-design.md` | Full design: 3-leg mechanism (fill detour, cull disp32 patch, guideline detours), components, data models, error handling, testing strategy, appendices |
| `implementation/plan.md` | 7 incremental steps with checklist, each cabinet-demoable |

## Design in one paragraph

A new mod `playfield-styling` registers two per-player Mods-tab options
(`arrow_scale` 25–150%, `arrow_opacity` 0–100%, `PersistMode::Full`), latched
per song at GAMEPLAY entry. One detour on the shared per-quad sprite fill
(`render_sprite_final` — un-detoured, and reached by real CALLs from the
arrow, freeze, shock, receptor, and hit-flash renderers) scales quad geometry
about the lane center / receptor row and composes opacity into the color
alpha, with side binding via RTTI vtable classification + the proven
presence-read/posX-split. A verified 4-byte disp32 redirect on the note
collector's 720.0 cull load (byte-identical site on both 2026 builds) extends
the collection window to `720/min(scale)` so shrunken playfields never pop
arrows in mid-screen. The measure guideline — which bypasses the fill — gets
a capture detour (Y-base pre-scale, exact for both scroll directions) plus a
transform detour on its single-caller bulk emitter, and the same cull-site
patch. Mines integrate same-crate via a small snapshot API. All-or-nothing
enable gate; no option rows if anything fails to resolve.

## Next steps

1. Begin implementation at `implementation/plan.md` Step 1 (resolution
   groundwork), maintaining `progress.md` per repo convention.
2. Steps 3–7 each end with a cabinet deploy — have the cabinet reachable
   (`/tmp/sshhost` etc.) and the label PNGs deployed at Step 2.
3. Watch items called out by the design: JudgeEffectRenderer deferred
   binding, the int3-cave placement for the cull float, and the Step 7
   cross-build derivation check on the 20260324-lineage build.

## Refinement candidates (non-blocking)

- Mixed-scale versus over-collection is accepted as cosmetic-free arena cost
  — revisit only if the stress test (lowest speed × 25%) shows pressure.
- HIDDEN/SUDDEN fade zones keep stock screen distances (accepted A5); a
  future enhancement could scale the four appearance fields per song.
