# Summary: Perspective Expansion

PDD + implementation complete, cabinet-verified 2026-08-05 (three live passes).

## What shipped

The `player_perspective` mod's PERSPECTIVE row grew from OVERHEAD/HALLWAY to
**OVERHEAD / HALLWAY / DISTANT**. DISTANT is the StepMania positive-tilt
perspective: notes start large and shrink toward the receptors, which sit
toward the horizon at their stock screen height. Implemented as a constant
preset against the existing single perspective VS program, with two new
constants: base zoom `z0` (c49.y) and the receptor-row realignment shift `ty`
(c49.z) — both exactly identity for HALLWAY.

**INCOMING and SPACE were implemented, live-evaluated, and removed** (maintainer
call, second live pass): unpleasant to play, and their screen-center
convergence exits the stock filter band in versus. Old persisted values 3/4
clamp to DISTANT; `PerspConstants.cx` remains a free constant so re-adding a
skew preset is a table change.

Two live-test-driven corrections became permanent, preset-generic machinery:

- **Receptor hit flash** (AFP clip, invisible to the VS): tracked CPU-side by
  playfield_styling's `lane_hook`, composing the published per-side
  `PerspConstants::map_point` after the playfield-scale step. The
  `note_result_setup` capture is consumer-refcounted (guideline_hook pattern);
  the perspective lane pass doubles as a drain site, so it works with
  playfield_styling config-disabled.
- **Tap hit-burst + freeze-hold glow** (`screen::JudgeEffectRenderer`, arrow-
  sheet cells with its own SetShader binding the JUDGE container): a new
  best-effort `judge_effect_render` detour rewrites its pass to the judge
  container's new perspective program 1 (which reuses the ARROW persp VS blob
  — the stock judge VS is byte-identical). Side bound presence-first (versus →
  posX split, no cross-side fallback; single/doubles → whichever side
  published). Synthesis minimal-overlay rule updated: arrow+judge iff AA∨persp
  (fingerprint bumped v1→v2).

## Artifacts

- `rough-idea.md`, `idea-honing.md` (D1–D9 Accepted; Readiness Confirmed)
- `research/orientation.md`, `research/stepmania-perspective-math.md`
- `design/detailed-design.md` — Approved 2026-07-31, Revised 2026-08-04 (the
  revision note at the top records the INCOMING/SPACE removal, ty, and the
  flash tracking; superseded sections retained as record)
- `implementation/plan.md` — Approved 2026-07-31, all 4 steps ticked
- `progress.md` — Status: DONE; full deploy & test log (three passes) and the
  Ghidra evidence for the JudgeEffectRenderer findings

## Code surface (uncommitted — maintainer commits)

`src/mods/player_perspective/{mod.rs,pass_rewrite.rs}`,
`src/mods/playfield_styling/{mod.rs,guideline_hook.rs,lane_hook.rs}`,
`src/mods/config.rs` (distant_focal=3000, distant_zoom=0.9),
`src/core/signatures.rs` (judge_effect_render),
`src/services/avs_layeredfs/shader_synthesis.rs` (judge prog 1, v2 fingerprint),
`shaders/src/gs_screencommand_{arrow,default}.hlsl` + the two recommitted persp
blobs, `scripts/gen_option_labels.py` + 2 net-new PNGs (distant chip/preview),
AGENTS.md + `docs/shader_replacement_research.md` updates.

## Notes for future work

- Re-adding a skewed preset: preset-table change (cx lerp) + enum value +
  assets; the filter-band spill limitation stands unless the dressing gains a
  non-affine path.
- The published-constants block (`published_constants(side)`) is the intended
  surface for any future CPU-side consumer that must track the mapped lane.
