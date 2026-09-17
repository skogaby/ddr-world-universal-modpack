# Rough Idea: Enable Background Dancers

Captured: 2026-09-16

## As stated by the maintainer

A new top-level mod called **"Enable Background Dancers"**. The idea is to
revive the 3D background dancer functionality for DDR World — the animated
characters dancing on a 3D stage behind the lane that pre-World DDR versions
had — using the DDR A3 character/stage/motion/camera data that is still shipped
(unused) in the stock World install.

`docs/background_dancers_feasibility.md` is the design-input record: it
establishes that World's engine layer (`gs`/`agcs`/`me`) still contains the
whole 3D pipeline — KTMDL model loader, DDS/ANM file callbacks, the model
shaders, the four `MODEL:*` render passes (live, attached, driven every frame
with an empty item list), the `SceneGraph`/`SceneGraphManager`, and the camera
object — and that Konami deleted only the **game-side scene layer**: the
`ModelNode`/`TransformNode`/`AnimationNode` node types, the ANM evaluator and
pose chain, and the `CharaActor`/`StageActor`/`CameraActor` actors. The
recommended architecture is "Option A — hybrid": the DLL supplies that scene
layer (an ANM evaluator, a render-item builder matching the engine ABI, one
scene-graph node type, a camera driver, and the per-song selection logic) and
lets the stock engine do everything GPU-facing.

## User-facing requirements (v1, as stated)

- A **single top-level mod toggle** — no per-player configuration, no option
  rows, no persistence of any player preference.
- When the mod is enabled, **a random stage and a random background dancer
  are loaded during gameplay**.

## Out of scope for now (per the maintainer)

- Per-player character selection.
- Any other player-facing configuration.
