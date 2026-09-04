# Training loop / marker / timeline revisions — PDD Summary

Completed 2026-09-04 in one session (register accepted wholesale, Readiness
Confirmed, design + plan Approved, all 5 plan steps implemented,
cabinet-validated by the maintainer 2026-09-04). Code is UNCOMMITTED —
the maintainer commits manually.

## Artifacts
| Artifact | Path |
|---|---|
| Rough idea | `rough-idea.md` |
| Decision register D1–D13 (all Accepted) | `idea-honing.md` |
| Orientation research | `research/orientation.md` |
| Detailed design (Approved 2026-09-04) | `design/detailed-design.md` |
| Plan, 5 steps (Approved 2026-09-04, all ticked) | `implementation/plan.md` |
| Live resume point + cabinet checklist | `progress.md` |
| Durable RE addendum | `docs/training_mode_research.md` (2026-09-04 addendum) |
| Host harness | `scripts/validate_training_mode.sh` |

## What changed
- **READY soft-lock closed:** every training gesture (4/5/6/7/9) waits for the
  new shared `song_reset::run_in_song()` (anchor landed + credible `+0x178`).
  The loop driver uses the same predicate.
- **Sections are loop-only:** SONG START/END TIME are LOOP SONG's child rows
  (retained-but-ignored while hidden at all three readers incl. the bind-time
  pre-shift); 4/5/6 markers require the per-song loop latch (one hint toast
  per song); 7/9 scrub unaffected. v1's LOOP-OFF early-natural-end retired.
- **Timeline HUD:** veil + A/B lines only on looping songs; cursor/readout/
  strip always.
- Config order, README, AGENTS.md, research doc updated.

## Next steps
1. Commit the 10 modified files + 2 new paths (`scripts/validate_training_mode.sh`,
   this planning dir) — maintainer-driven; plain conventional message, no trailers.

## Callouts
- The hint toast fires after a mid-song loop DISARM too (latch dropped) —
  correct by design, possibly surprising; see `progress.md` deviations.
- `section_math::end_policy`'s `WriteThresholds` arm and
  `bounds::write_end_thresholds` are now dead-defensive code paths; remove
  in a later cleanup if desired.
