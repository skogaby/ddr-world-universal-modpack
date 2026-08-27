# Summary: Preserve Song Pitch sub-option

Planning completed 2026-08-12; **implementation completed and
cabinet-validated 2026-08-13** (all 6 plan steps; maintainer tested multiple
songs at multiple rates in both pitch modes — audio correct, option
round-tripped). All gates passed: register accepted (Readiness Confirmed
2026-08-12), design approved 2026-08-12, plan approved 2026-08-12.

## Artifacts

| Artifact | Path |
|---|---|
| Rough idea | `.agents/planning/2026-08-12-preserve-pitch-toggle/rough-idea.md` |
| Decision register (D1–D14) | `.agents/planning/2026-08-12-preserve-pitch-toggle/idea-honing.md` |
| Orientation research (both repos' seams) | `.agents/planning/2026-08-12-preserve-pitch-toggle/research/orientation.md` |
| Backend pattern research (bemani-buddy 012/013 in-flight diff) | `.agents/planning/2026-08-12-preserve-pitch-toggle/research/backend-bemani-buddy.md` |
| Detailed design (approved) | `.agents/planning/2026-08-12-preserve-pitch-toggle/design/detailed-design.md` |
| Implementation plan (approved, 6 steps) | `.agents/planning/2026-08-12-preserve-pitch-toggle/implementation/plan.md` |

## What was designed

A per-player boolean **PRESERVE SONG PITCH** sub-option under SONG SPEED on
the MODS tab — visible only while that side's speed ≠ 100 % (live, per-side,
via a new `ShowWhen::NotEquals` framework variant). ON (default) keeps
today's pitch-preserved WSOLA; OFF plays the song through a **new
deterministic streaming resampler** (record-player pitch), swapped in at the
`Feed` seam in the generator so the entire downstream (virtual bank, clock,
ticks, Real Speed, score containment) is byte-agnostic to the mode. The flag
latches at scene-26 from the entered side, mirroring the percent. Persists
as `mod_preserve_pitch` (PersistMode::Full) with the bemani-buddy backend
change **in scope** (migration 014, JSON-model → codegen → Rust pipeline,
stacked on the in-flight 012/013 working-tree changes). Textures generated
by `scripts/gen_option_labels.py` with the user-specified preview copy.

## Plan shape

1. Resampler core + host tests (front-loaded risk)
2. Generator/binding mode seam (callers hardwired ON — behavior unchanged)
3. Flag carriage runtime → lifecycle → bind
4. `ShowWhen::NotEquals` + row + textures — core end-to-end cabinet demo
5. bemani-buddy backend persistence
6. Validation-script `resample` section, docs, full cabinet checklist

## Assumptions / refinement candidates before implementation

- **Preview copy fit:** the two preview panels' second lines were drafted in
  the design (FR-7); watch `gen_option_labels.py` overflow warnings and trim
  if needed.
- **Loop-seam behavior at OFF** is arithmetic-exact by design but has no
  live precedent — the Step 6 cabinet checklist includes a looping-bank
  listen test; if a click is ever audible at the seam, a short crossfade at
  the loop restart is the contained fix.
- **Linear interpolation quality** is a deliberate floor (D6); windowed sinc
  is the upgrade path only if audibly needed on real hardware.
- **bemani-buddy stacking:** the backend work assumes migrations 012/013
  remain uncommitted-in-flight as of planning; if they land or renumber
  first, take the next free migration number.
- Maintainer instruction recorded in the plan: `cargo fmt` churn in
  bemani-buddy is left in the working tree for the maintainer's single
  commit.

## Next steps

Implementation is complete (code-task-generator was skipped by user
direction; code-assist ran per plan step with in-session approvals —
per-step records under `tasks/step0*/`). Remaining: the maintainer commits
the working trees in this repo and `../bemani-buddy` (both left uncommitted
by convention; the bemani-buddy change stacks on the in-flight 012/013
work).
