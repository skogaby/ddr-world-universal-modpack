# Summary — DDR SELECTION: DDR A / DDR A3 (White) / DDR A3 (Gold)

Planning complete 2026-09-25 (design and plan approved). No implementation code was written.

## Artifacts

| File | What it holds |
|---|---|
| `rough-idea.md` | The request and the maintainer's carried-in decisions |
| `idea-honing.md` | Decision register D1–D26 (D12 / D13 overridden to include), Readiness Confirmed, approval notes |
| `research/orientation.md` | Starting state, data inventory, A3's skin-0 panel fill, danger actor, code touch points |
| `research/name-and-score-sets.md` | Gameplay name and panel score-set RE, incl. the §5 design-pass addendum (World record rank / clear-kind fields, A3 name colour / scale / binding) |
| `design/detailed-design.md` | The approved, self-contained design |
| `implementation/plan.md` | The approved 9-step plan with its checklist |
| `progress.md` | Live resume point |

Background research outside the planning folder: `docs/ddr_selection_a3_themes_research.md`.

## Design in brief

- Three appended row values:

  | Row value | Theme | Skin | Suffix |
  |---|---|---|---|
  | 7 | DDR A | 6 | `_v0` |
  | 8 | DDR A3 (White) | 7 | `_v2` |
  | 9 | DDR A3 (Gold) | 8 | `_v1` |

- The skin doubles as the `LayoutActor` record skin, so World's skin-0 branches run on the theme's
  own packages. `GameWork+0xA8` = 0 for a theme.
- AUTO:
  - series 17 → DDR A;
  - series 18–20 → A3, Gold on a gold cabinet (machine type 4, including the SMX GOLD force), else
    White.
- Most surfaces are policy rows plus widened adapter ranges. The texture-name number is 0 for
  themes, so names read `dance_combo0000_*`, `dance_score0000_*`, `stage_frame0000_stage_*`.
- Four new components:
  1. A3's skin-0 stage-panel fill.
  2. The panel's per-player score sets, from World's best record (rank and clear kind map 1:1 to
     A3's textures).
  3. The gameplay player name: a font-6 `agcs::BmpString` bound to `name_usr`.
  4. A scoped 2-byte danger-on-doubles patch.
- S-Marvelous comes last. White and Gold share one art set.
- Every new field and surface fails open.

## Plan in brief

1. Theme identity, whole-package swaps, song info, stage frame, **and the cross-generation
   texture-name spike**.
2. HUD adapters.
3. READY, banners, announcer, AUTO.
4. Stage-panel fill.
5. Score sets (RE first).
6. Gameplay name (RE first).
7. Danger patch.
8. S-Marvelous.
9. Release docs and the cabinet matrix.

## Next steps

1. Run the **code-task-generator** sop on `implementation/plan.md`. It processes one plan step at a
   time, starting with Step 1.
2. Run the **code-assist** sop on each generated task, in order.
   - Keep `progress.md` current after each step.
   - Tick the plan checklist.
   - The maintainer commits manually.

## Watch-outs before and during implementation

- **AS1: no texture-name bleed across generations.** It is untested until the Step 1 spike. A
  failure stops implementation and reopens design §6.3.
- **Name draw order.** Whether the name draws in the right list is decided by the Step 6 RE; the
  fallback is the READY-to-shutter visibility window.
- **Open RE owned by Step 5:**
  - the target resolver and its name / area getters;
  - whether World has a per-player area;
  - A3's empty-record display.
- **DDR A uses A3's skin-0 rules** (DDR A's own binary is unavailable). On FLARE gauges it behaves
  like the eras.
- **Each theme policy row lands with its adapter's range widening**, never before it.
