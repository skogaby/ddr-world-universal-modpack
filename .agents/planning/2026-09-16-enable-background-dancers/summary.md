# Summary — Enable Background Dancers (PDD complete, 2026-09-16)

## Artifacts

| File | Purpose |
|---|---|
| `rough-idea.md` | The maintainer's idea as stated (single toggle, random stage + random dancer per song). |
| `idea-honing.md` | Decision register: 23 decisions, all Accepted (D1/D2 overridden by the maintainer: thumbnail movie instead of suppression; hide the real 2D background instead of a placeholder arc). `Readiness Confirmed 2026-09-16`. |
| `research/orientation.md` | Codebase integration surfaces (what exists / what is new), data inventory, compositing problem, unknowns. |
| `research/a3-runtime-rules.md` | Ghidra RE of A3 20240402: playlist/cut rule, no idle, no BPM scaling in retail, camera stage/music modes, placement, shadow, animation-node mechanics. Supersedes the feasibility doc where they differ. |
| `research/world-background-and-movie.md` | Ghidra RE of World 20260825: `BgMovieActor → BackgroundFrame+0x140` bg_root clip graph, the id-source lambda, the movie-size read site (DPS step 2), the DPS step-5 scene-graph enable, deferred-destroy semantics; three pre-design validations. |
| `research/formats-and-data.md` | Format facts with Python reference pointers, rlist dumps, arc inventory, present-chain placement, texture API status. |
| `design/detailed-design.md` | **Approved 2026-09-16.** Self-contained design: FR-1..15, NFR-1..7, architecture, components/interfaces, data models (item/node/camera layouts), error handling, testing, appendices (World facts, A3 rules ported, alternatives, phase 2). |
| `implementation/plan.md` | **Approved 2026-09-16.** 10 steps with checklist; Steps 1–4 = the cabinet spike (Step 3 = GO/NO-GO on the render-item ABI). |

## Design in one paragraph

World's engine still contains the whole 3D pipeline (KTMDL loader, model shaders, the `MODEL:*` passes
running with an empty item list, `SceneGraph`/`SceneGraphManager`, camera object); Konami removed only the
game-side scene layer. The mod supplies that layer in Rust: a pure `core/anm` codec (ANM/CAMANM/B2IT/MRL0/
KTMDL bones), an engine-facing `services/scene3d` (arc loading, model-registry readiness, bone textures,
hand-built 0xC8 render items, one flat mod-owned node type, root attach / deferred destroy, camera slot 0),
and `mods/background_dancers` (seeded random stage + dancer(s) per song window, the A3 shuffled-playlist
1.5 s hard-cut rule, A3 stage-mode camera sequencing with `_non` cut-aways, part attachment + shadow, a
game-thread `FrameState` producer that `visit` memcpys into items, per-frame alpha-0 on the live `bg_root`
layer, and a per-song restored thumbnail override of `Customize+0x30`). Zero new detours; all-or-nothing
derivations swept over four builds; fail-open everywhere; default OFF until cabinet-proven.

## Next steps

1. Run the **code-task-generator** sop against `implementation/plan.md`, one step at a time (start with
   Step 1), producing task files under `.agents/tasks/2026-09-16-enable-background-dancers/stepNN/`.
2. Run **code-assist** on each task in order; keep `progress.md` in this directory current after each step
   (Updated / Status / NEXT ACTION / Done / In flight / Deploy & test log / Deviations).
3. Treat Step 3's cabinet result as the architecture gate: a render-item ABI failure after reasonable fixes
   means STOP and open a separate PDD for Option B.

## Assumptions to re-check during implementation

- Render-item ABI acceptance and the pass-4 visible-push context (Step 1 RE + Step 3 cabinet).
- Texture create/release pair on 20260825 and the other three builds (Step 1 RE).
- D3DMetal vertex-texture fetch for the skinning VS (Step 4, CrossOver).
- `bg_root` alpha-0 actually hides the background and the game does not re-set it (Step 3).
- Movie thumbnail geometry leaves the 3D visible (Step 10).
- The 1-frame skew between camera (game thread) and poses (update job) is imperceptible (Step 9).
