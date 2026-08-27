# Summary — Auto-Calibration (Timing Offsets)

Date: 2026-08-26
Status: COMPLETE — implemented (plan steps 1–5), cabinet-verified across two
deploys, docs shipped (README + AGENTS.md). Uncommitted; maintainer commits.

## Artifacts

- `rough-idea.md` — the initial concept
- `idea-honing.md` — 19-decision register, all Accepted; Readiness Confirmed 2026-08-26
- `research/orientation.md` — codebase findings (judge data feed, sound-offset
  write path, overlay row API, toast internals, scene/reset boundaries,
  overlay-hiding mechanisms) with file:line citations
- `design/detailed-design.md` — Approved 2026-08-26
- `implementation/plan.md` — Approved 2026-08-26 (5 steps)

## Design in one paragraph

A "Calibrate next song?" enum row at the top of the timing-offsets section on
the overlay menu's GLOBAL SETTINGS tab arms a one-song calibration session.
At GAMEPLAY entry (exactly one entered side, song rate 100 %), a lock-free tap
in power_user_statistics' existing `judge_submit` detour accumulates the
side's per-step ms errors (grades M–Boo), a pulsing "Calibrating..." toast
runs for the song, and every judgement-feedback surface is hidden (overlay
clips via an opacity override in overlay_element_styling; PUS timing readouts
via a suppression flag). At exit, `new = clamp(old + round(mean))` is written
through the mod's existing `set_offset(SOUND, …)` (guards: ≥30 samples,
|mean| ≤ 500 ms, no autoplay taint), a 5 s result toast reports old→new, and
the arm always flips OFF. Zero new detours/signatures/config keys/textures;
fail-open everywhere. The toast is promoted from Training Mode to
`src/services/toast.rs` with pulse + hold modes.

## Plan shape

1. Toast service promotion (+ host tests, validation script)
2. Row + arm/consume lifecycle, measurement stubbed
3. Measurement + decision core + apply (cabinet sign-direction verification)
4. Overlay hide + PUS readout suppression
5. Docs + full cabinet regression sweep

## Deliberate unknowns / watch items

- **Sign direction**: CABINET-VERIFIED (deploy #1) — a −40 ms mis-set
  converged back to baseline; `new = old + round(mean)` stands.
- Two cabinet-caught fixes landed post-deploy-#1: the apply now refreshes the
  overlay SOUND OFFSET row's displayed value (`refresh_overlay_row`), and the
  autoplay guard reads `score_guard::is_autoplay_tainted` (autoplay alone)
  instead of `is_stage_suppressed` (which misattributed quick-exits).
- Rate-guard ordering assumption held; shared `toast::dismiss()` remains
  unconditional (accepted, benign).

## Next steps

None — feature closed after deploy #2 verification. Optional: archive this
directory to `.agents/planning/_archive/` (update the AGENTS.md planning
pointer if so). Maintainer commits the work manually.
