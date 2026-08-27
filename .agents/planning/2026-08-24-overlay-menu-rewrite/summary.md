# Summary: Overlay Menu Rewrite (Mod Menu v2)

Planning completed 2026-08-24. All PDD gates passed.

## Artifacts

| Artifact | Path | State |
|----------|------|-------|
| Rough idea | `.agents/planning/2026-08-24-overlay-menu-rewrite/rough-idea.md` | Captured |
| Decision register | `.agents/planning/2026-08-24-overlay-menu-rewrite/idea-honing.md` | 22 decisions, all Accepted/Overridden/Assumed; Readiness Confirmed 2026-08-24 |
| Research | `research/orientation.md`, `research/widget-rendering.md`, `research/shader-spike.md` | Complete |
| Detailed design | `design/detailed-design.md` | **Approved 2026-08-24** |
| Implementation plan | `implementation/plan.md` | **Approved 2026-08-24** (9 steps) |
| Progress tracker | `progress.md` | Live (resume point) |

## Design in brief

The triple-press-0 overlay menu is rewritten from a single white-text scroller into a
themed, tabbed modal rendered through the game's native pipeline:

- **Four tabs:** MODS (enable/disable), GLOBAL SETTINGS (cabinet-wide rows grouped per
  owning mod), PLAYER SETTINGS (options mirrored live from the in-game custom-options
  framework, with a P1/P2 selector and session gating), THEME (built-in themes,
  animated-background toggle, opacity 25–100 %).
- **Mirroring machinery (all additive to custom_options):** `MenuPlacement` on
  registration (default both menus), display-string fields with prettified-id fallback,
  a multicast value-changed observer, an `overlay_snapshot(side)` API, and the new
  `custom_options.option_menu_settings` config (ordered array of
  `{id, overlay?, in_game?}`) that replaces `row_order` (removed outright).
- **Presentation:** runtime-synthesized rounded-corner panel PNGs (background-thread
  encode, async loose-PNG load), ~12 dense rows, footer descriptions, scrollbar, new
  `overlay_menu` config section `{theme, animate_background, opacity}`.
- **Animated backgrounds:** SM3 pixel-shader programs appended to the game's
  boot-resident default shader container, emitted per frame into the game's command
  list, scissored to the modal rect — gated on a front-loaded RE spike with a designed
  static-gradient degrade path. Shadertoy ports supported (single-pass, fxc-compilable,
  permissively-licensed only).

## Plan in brief

Nine steps, each cabinet-demoable: (1) module restructure + widget-pool diagnostic,
(2) shader spike POC (go/no-go for step 8), (3) tabbed shell + density, (4) modal
chrome, (5) custom_options extensions + `row_order` removal, (6) PLAYER SETTINGS
mirroring, (7) THEME tab (static), (8) animated shader backgrounds, (9) registration
sweep + removals + docs.

## Assumptions / refinement areas

- D19/D20 were accepted as assumptions: music-wheel X/Y rows removed but config keys
  stay hand-editable; opacity row is 25–100 % step 5.
- Layout constants (§4.5) and theme palettes (§4.6) are first-deploy tunables, not
  contract.
- The widget pool size is unknown until the Step 1 diagnostic runs; the design
  tolerates exhaustion but the number may inform how much chrome ships.
- Spike unknowns (active-list validity at the wrapper site, arena headroom, z recipe)
  can only be resolved on the running game; Step 8 is explicitly skippable on a no-go.

## Next steps

Run the code-task-generator sop against `implementation/plan.md` one step at a time,
then code-assist per task. Cabinet/CrossOver validation per step; the maintainer is
needed for visual verification and any scene navigation beyond the attract loop
(boot-and-harvest-logs runs can be executed autonomously via the local CrossOver
install). Git: maintainer commits manually — task completion recorded as
`Complete (uncommitted)`.
