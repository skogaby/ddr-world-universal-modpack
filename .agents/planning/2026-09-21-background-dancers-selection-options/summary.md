# Summary — Background Dancer / Stage selection options with live 3D previews

Planning completed 2026-09-21 (light PDD pass). Design approved 2026-09-21; implementation plan
approved 2026-09-21. Ready for task generation.

## Artifacts

| File | Purpose |
|---|---|
| `rough-idea.md` | The maintainer's request as captured |
| `idea-honing.md` | Decision register D1–D15 (all Accepted/Assumed; `Readiness Confirmed 2026-09-21`) |
| `research/orientation.md` | Codebase orientation: candidate tables/labels, `custom_options` fit, WebUI preview plumbing, the compositing problem |
| `research/preview-compositing.md` | Ghidra findings (20260825): target lists, attach/detach, MODEL pass layout, worker viewport setup, D13/D14 resolution, camera math, the tag-0x10 correction, the seven derivations to add |
| `design/detailed-design.md` | Approved design (requirements, architecture, components, data models, error handling, testing, appendices) |
| `implementation/plan.md` | Six-step plan with checklist (Step 1 rows end-to-end → Step 2 signatures → Step 3 compositor smoke → Step 4 Session generalisation/extraction → Step 5 preview driver → Step 6 badge/polish/docs) |

## Design in brief

Two `custom_options` scalar rows (`background_dancer` per side, `background_stage` mirrored),
value 0 = `RANDOM`, values 1..N = catalog entries labelled from the rlist keys (`EMI #2`,
`CRYSTALDIUM`) through a new `ScalarFormat::Dynamic(fn)`; `PersistMode::Local`, in-game only,
listed under PLAYFIELD STYLING OPTIONS. Song-window entry resolves the choices (pin → option →
random). Live previews render the chosen dancer (fixed viewer camera, sex-pool routine) or stage
(own `_play_loop`s + `.camanm` choreography) into the row's 16:9 marker box via **mod-owned clones
of the engine's MODEL:OPACITY/TRANS passes attached into the RENDER_2D target list** with their own
viewport rect, matrices and a private node-mask bit per side, preceded by a mod-owned clear
viewport. RANDOM shows a static badge. Everything fails open to "rows work, box shows chrome".

## Next steps

1. Run the code-task-generator sop against `implementation/plan.md`, one step at a time, producing
   task files under `.agents/tasks/2026-09-21-background-dancers-selection-options/stepNN/`.
2. Run the code-assist sop on each task in order; keep `progress.md` in this directory current
   after every step (the resume point); cabinet-validate every step's Demo before the next.

## Areas that may need refinement during implementation

- Dancer-preview camera constants (§4.6) are a first guess; tune on the cabinet.
- Whether AFP 2D quads write depth was not decoded; the unconditional clear viewport makes it moot,
  but if D3D9 `Clear` turns out not to honour the viewport on some driver (CrossOver/D3DMetal),
  fall back to depth-only clears (colour clear off) — the box then shows the chrome behind the
  dancer.
- `:N` low-priority stage parts render through the TRANS clone in previews (one private bit per
  side); acceptable ordering drift inside the box.
- A live enable of the mod gets row textures at the next launch (framework-wide atlas flush).
