# DSU (Dancing Stage Unleashed) dancers — native-rig port + DSU-exact cel/outline

Research: `docs/dancing_stage_unleashed_dancers_port_feasibility.md` (Path B + §5 shaders).

## Decisions (maintainer, 2026-09-28)

- **Path B**: the two DSU dancers keep their OWN skeletons (35 / 32 bones) and play their OWN
  DSU clips. Precedent for an Omnimix of dancers from more past releases.
- **Per-dancer animations** (not stages): a custom dancer may carry its own clip pool.
- **Cel shading + scene outlines** are REPLACED globally by a DSU-exact look (every dancer and
  stage prop when those options are on). The LAYERED outline style and the per-kind width
  rows/keys are removed.
- Motion source: DSU `.ani` clips (30 Hz, exact trims) — written as 60 fps `.anm` with key times
  every 2 frames (no resampling; the evaluator slerps between keys like DSU's own sub-frame lerp).
- The generated dancer folders are tracked content under `data_mods/custom_models/dancers/`
  (the maintainer commits; agents never commit).

## Design

### D1 — per-dancer motion pool (DLL, pure + session)
- Body folder may hold `motion/<clip>.anm` → member `data/chara/pl_<key>/motion/<clip>.anm`
  (the folder walk already maps it; a ready `.arc` just carries that member).
- `custom_content::plan` collects the clip stems → `DancerCandidate.motion` (sorted). Empty =
  the stock pool of the candidate's sex (unchanged behaviour).
- `selection`: `playlist_for(rng, cand)` shuffles the own pool (one shuffle — same rng draw count
  as the stock path); `DancerCandidate::{motion_arc_name, clip_member}` route loads to the body
  arc for own motion.
- `pick::arcs_for` lists the motion arc per candidate (own ⇒ the body arc, already listed).
- `session::parse_pick` reads own clips from the body reader.

### D2 — role-bone aliases in `.b2it`
The DLL resolves Hips / Spine2 / Head / Left|RightToeBase (shadow, Big Head, parts) by NAME in
the body's `.b2it`. A foreign rig adds ALIAS entries (same index, extra name) — the World
engine never opens a body `.b2it`, only this DLL does; no code change per ported rig.

### D3 — converter (content)
`tools/blender_ddr_addon/examples/port_character_ultramix.py` (Blender): builds the DSU rig +
mesh from `.ddm` (Z-up bind → game Y-up, mirror, 0.1026 m/unit), exports with the add-on,
appends the b2it aliases, converts every `animations.csv` clip of that dancer with
`scripts/ultramix_k3d_dump.py::ani_to_anm` against the EXPORTED bind frames, verifies the round
trip against `skin_pose()`, writes the sidecar.

### D4 — DSU-exact cel + outline (`shaders/src/mdl_cel.hlsl`)
- CEL: `rgb = tex · col · ramp(N·L)`, N·L per VERTEX on the unnormalised skinned normal,
  ramp = bilinear-clamped `toon.tga` = `lerp(142/255, 1, saturate((ndl − 63.5/128)·128))`,
  DSU light position 2 (30° up, 45° to the dancer's right, front) = world
  `normalize(−0.673, 0.305, 0.673)`, white light, no rim ink. (Self-shadow map dropped.)
- OUTLINE: object-space push along the unnormalised skinned normal by
  `0.00308 + 0.000375·w` metres (DSU `0.03 + 0.3·w/1000` DSU units at 1.25 model scale,
  0.0821 m per DSU world unit), black, emulated front-face cull kept, no depth margin.
- DLL: one black hull layer; `ModelParameters.w` = 1.0 (a multiplier kept for later).
