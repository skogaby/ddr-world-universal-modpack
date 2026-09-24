# DDR SELECTION — PDD summary

Artifacts (`.agents/planning/2026-09-22-ddr-selection/`):

- `rough-idea.md` — the request.
- `idea-honing.md` — decision register D1–D29, Readiness Confirmed 2026-09-22.
- `research/orientation.md`, `research/hud-actors.md`,
  `research/intro-and-skin-surface.md`, `research/sounds-options-folder.md` —
  RE findings (A3 20240402 vs World 20260825 / 20250805).
- `design/detailed-design.md` — Approved 2026-09-22.
- `implementation/plan.md` — 14 steps, Approved 2026-09-22.
- `progress.md` — live resume point.

Design in one paragraph: restore A3's `%04d` package-name append in World's
`LayoutActor` per-package helper and write `GameWork+0xA8`, so World's own
plumbing loads the legacy skin packages it still ships; re-implement what
Konami deleted (READY/HERE WE GO, the legacy stage panel and cut-in, end
banners, A3 combo/score behaviour, the A3 announcer) inside World's objects via
gated per-function detours; build an era sound bank from World's unloaded `_n`
banks; force 1st-5th options for the song only; trigger from a per-player
OPTIONS row (OFF / AUTO / era).

Next steps: execute the plan step by step (code-assist per step, or directly),
cabinet-testing at each step's Demo.

Areas that may need refinement: the shutter state-machine seams (Step 5 opens
with RE), World's AFP sound routing (Step 3 opens with RE), stage-panel score
fields without a World data source, libafp's visible behaviour for missing
labels/textures (A3 relied on it).
