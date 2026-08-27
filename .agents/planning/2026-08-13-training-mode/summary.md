# Training Mode (v1: Section Practice) — PDD Summary

Completed 2026-08-13. All gates passed: register accepted (17/17),
Readiness Confirmed, design Approved, plan Approved.

## Artifacts

| Artifact | Path |
|---|---|
| Rough idea (incl. grouping addendum) | `.agents/planning/2026-08-13-training-mode/rough-idea.md` |
| Decision register (D1–D17, all Accepted) | `.agents/planning/2026-08-13-training-mode/idea-honing.md` |
| Orientation / research pointer map | `.agents/planning/2026-08-13-training-mode/research/orientation.md` |
| Detailed design (Approved 2026-08-13) | `.agents/planning/2026-08-13-training-mode/design/detailed-design.md` |
| Implementation plan, 8 steps (Approved 2026-08-13) | `.agents/planning/2026-08-13-training-mode/implementation/plan.md` |
| Durable RE: seek/loop/end-chain/audio/anchor/song-length | `docs/training_mode_research.md` |
| Durable RE: non-selectable header rows | `docs/option_header_rows_research.md` |

## What v1 delivers

New top-level mod `training-mode`: SKIP FIRST / OMIT LAST section bounds
(select-time clamped to the song's audio length; session-scoped), LOOP
SONG (default OFF — OFF ends the section at the game's own banner/results,
ON grinds until quick-fail), live A/B refinement gestures (triple-4/5/6 =
A/clear/B on the pinpad's middle row, per the amended D3),
restart-from-A, a shared content-time progress HUD with TOP/BOTTOM
placement row, and full score containment (including the deliberate
assist-tick taint change). Skip-first starts are **silent** — the song's
true beginning is never audible (bind-time pre-shift + 2.5 s approach
lead, `TRAINING_LEAD_MS`). All training-related rows group under a slim
full-width **TRAINING OPTIONS** header row, expressed purely via
`row_order` (headers hidden when unlisted). Assist tick and song playback
speed remain standalone mods. v2 (FF/RW on pinpad 4/6 +
`training_mode` config block) and v3 (judgement-state rewind) are designed
-for sketches.

## Plan shape (8 steps)

1. Identity arm + shifted serving (song_rate) — riskiest, front-loaded
2. Seek-to-T + A/B gestures + restart-from-A
3. Bound rows + session persistence + silent skip-first start
4. LOOP SONG (loop driver / early natural end)
5. Score containment
6. Progress HUD + placement row
7. TRAINING OPTIONS header row + grouping
8. Docs, default config, full regression

## Next steps

1. Run the **code-task-generator** sop against
   `.agents/planning/2026-08-13-training-mode/implementation/plan.md`
   (one step at a time), producing task files under
   `.agents/tasks/2026-08-13-training-mode/step<NN>/`.
2. Implement each task with the **code-assist** sop, in step order.
3. Maintain `progress.md` in this planning directory per the repo's PDD
   progress-tracking convention (AGENTS.md → Custom Instructions).

## Callouts for implementation time

- `PersistMode::Session` is a new framework variant — small but touches
  the persistence matrix; introduce it in Step 3 with its rows.
- The assist-tick taint (Step 5) changes shipped behavior — release notes
  / README must call it out (Step 8 covers this).
- Cabinet items to watch (design §7 checklist + research §10 list):
  spanning-freeze feel at A, loop-fire margin below the end thresholds,
  cue-handle churn under rapid loops, header slot height on the 7-row
  scroll window.
- Every engine-facing step ends in a live cabinet demo; no dry-run modes
  by maintainer preference.
