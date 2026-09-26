# Progress — DDR SELECTION: DDR A / DDR A3 (White) / DDR A3 (Gold)

Updated: 2026-09-25
Status: PDD Step 7 of 8 — design approved 2026-09-25; plan drafted, awaiting maintainer approval (no implementation yet)
NEXT ACTION: maintainer reviews `implementation/plan.md`; on approval set its `Status: Approved <date>`, write `summary.md` (PDD Step 8), then implementation starts at plan Step 1 (code-task-generator → code-assist).

Resume protocol: read `idea-honing.md` (D1–D26, Readiness Confirmed), then
`design/detailed-design.md`; research lives in `research/` (`orientation.md`,
`name-and-score-sets.md` incl. the §5 design-pass addendum).

## Done

- Step 1–4: workspace, orientation, decision register D1–D26 (D12 / D13 overridden → included),
  research (feasibility doc `docs/ddr_selection_a3_themes_research.md`, name / score-set RE).
- Step 5: Readiness Confirmed 2026-09-25 (two clarifications recorded in `idea-honing.md`).
- Step 6: `design/detailed-design.md` approved 2026-09-25 (maintainer: World has no edit charts — A3's edit-data handling dropped). Design-pass Ghidra checks closed several
  open RE items (World best-record rank `+4` / clear kind `+8`, both 1:1 with A3's tables; A3's
  name colour, scale and placeholder binding) — recorded in `research/name-and-score-sets.md` §5.

## In flight

- Step 7: `implementation/plan.md` drafted (9 steps; Step 1 = identity + cross-generation texture spike).

- Nothing uncommitted beyond the planning docs (the maintainer commits manually).

## Deploy & test log

- None yet (planning only).

## Deviations & open questions

- Open RE, each fail-open and owned by a plan step (design Appendix C): best-record anchor;
  target resolver + name / area getters; a per-player area in World; A3's no-record display;
  World's gameplay render list for the name widget, font-6 residency, World's equivalent of A3's
  `PlayerWork+1`.
- Assumption AS1 (no texture-name bleed across generations) is tested by plan Step 1's cabinet
  spike; a failure stops implementation and reopens the design (design §6.3).

## Key facts for a cold resume

- Row 7 / 8 / 9 = `DDR A` / `DDR A3 (White)` / `DDR A3 (Gold)` → skins 6 / 7 / 8 → suffixes `_v0` /
  `_v2` / `_v1`. Record skin = the skin; `GameWork+0xA8` = 0 for themes.
- AUTO: 14–16 → `2013-2014`, 17 → DDR A, 18–20 → Gold on machine type 4 else White.
- `tex_number(skin)` = 0 for themes (`dance_combo0000_*`, `dance_score0000_*`,
  `stage_frame0000_stage_*`).
- Never produce a bare `<base>0000`; theme names always carry `_vN` (World's probe reaches them via
  its bare rung).
