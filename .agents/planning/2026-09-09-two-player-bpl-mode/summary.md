# 2-Player BPL Mode — PDD Summary

## Artifacts

- `rough-idea.md` — the request
- `idea-honing.md` — decision register D1–D13 (accepted 2026-09-09; D6 amended to a
  two-file module during design), readiness confirmation
- `research/orientation.md` — codebase findings (services to reuse, private API to promote,
  vtable-clone precedent, harness behaviour)
- `research/re-findings.md` — Ghidra pass R1–R7 on `gamemdx_20260825` (GameWork flip
  safety, name field layout, board sidedness, `addChild` shape, the network-block
  dependency correction, package pre-check anchor, GPA score-select shape, vtable facts)
- `design/detailed-design.md` — Approved 2026-09-09
- `implementation/plan.md` — Approved 2026-09-09 (5 steps)
- `progress.md` — live resume point during implementation

## Design in one paragraph

Construct the game's own `MatchingBattleFrameActor` inside the normal
`DancePlaySequence` when a local 2-player versus song starts, give the instance a
mod-owned 9-slot vtable clone, and replace two slots: `onInitialize` (wrap stock with a
scope-guarded `GameWork+0 → 0` flip so it builds the 2-participant `main_single` layout)
and `onUpdate` (feed each board's target score from the two live `GamePlayActor`s, then
the stock smoothing + stock rank function). Zero detours, zero byte patches. Gates:
versus ∧ both sides entered ∧ not event/course ∧ GAMEPLAY ∧ network idle
(`local_cabinet_idx == −1`) ∧ `dance_matching` resident. Fail-open everywhere;
per-DPS-instance latch; mod on/off only, default ON.

## Plan in one paragraph

Step 1 signatures/derivations + `song_reset` promotions with the four-build sweep green
(front-loaded risk) → Step 2 pure `logic.rs` + host harness → Step 3 mod skeleton with
gates/tree-walk and a dry-run placement log → Step 4 vtable clone + slot wrappers + real
creation (HUD visible) → Step 5 cabinet matrix (EX scoring, widget overlap) + AGENTS.md
row + RE-doc addendum + README.

## Next steps

Run the code-task-generator sop against `implementation/plan.md`, then code-assist on
each task in order — or, as the maintainer requested for this feature, implement the
steps directly in-session and hand over a build for cabinet testing.

## Assumptions / areas to watch

- The `local_cabinet_idx == −1` placeholder-block behaviour of the stock ctor is verified
  by disassembly on 20260825 and gated explicitly; the four-build sweep proves the ctor is
  byte-identical, so the same static init holds.
- The `matching_usr` anchor position and overlap with power_user_statistics widgets /
  training strip are cabinet-only observations (Step 5).
- EX-scoring cabinets: the frame's gauge max comes from the chart's EX max via the music
  DB lookup inside stock `onInitialize` — confirm on cabinet (design Cabinet item 3).
