# Progress — Skip Results on Fast Exit

Updated: 2026-08-20
Status: implementation complete — awaiting cabinet validation
NEXT ACTION: deploy to cabinet (`./scripts/deploy.sh`) and run the validation
checklist in `design.md` (OFF → fade + banner + results with partial score; ON →
today's instant cut; versus → presser's setting wins).

Resume protocol: read `design.md` in this directory (single-file light PDD:
idea + decisions + RE findings + design), then
`docs/quick_restart_fail_speedup_research.md` §13.

## Done

- RE pass (Ghidra, 20260721 + 20260526): direct-to-results `finish(DPS, 0x1E)`
  REJECTED — result commit (GamePlayActor vt+0x28) only runs in the natural
  song-end machinery; a mid-song jump shows an all-zero record. Findings in
  research doc §13.
- DLL: `SKIP_RESULTS` per-side atomics + `skip_results_fast_exit` bool row
  (default ON, `PersistMode::Full`) registered in
  `QuickRestartOrFailMod::enable()`; `trigger_fail(presser_side)` takes
  `fail_song(None, "quick-fail (show results)")` when the presser's value is
  OFF. `cargo check` clean.
- Textures: `option_strings.py` label + off/on PreviewSpecs (en/ja/ko);
  regenerated — 9 new PNGs across the three language dirs, zero diffs to
  existing textures.
- bemani-buddy: migration `017_ddr_world_skip_results_fast_exit.sql` +
  protocol/model/mysql/handler plumbing + 5 tests (mirrors preserve_pitch);
  `.sqlx` regenerated against the live local DB; `cargo build` + `cargo test`
  (all pass incl. new), clippy/fmt drift pre-existing only.
- Docs: research §13, module doc, AGENTS.md quick-fail row.

## Deploy & test log

- (pending first cabinet deploy)

## Deviations & open questions

- None. Direct-to-results door documented in research §13 if ever revisited
  (would need: manual commit per actor, stage bump, msg 0x1053, song stop,
  MDX1529 zero-judge hazard).

## Key facts for a cold resume

- OFF path == the existing predicate-fail fallback (`fail_song(None)`) — no new
  hooks, no new signatures, no new scene machinery.
- Governing side = `InputEvent.player` of the 3-press (P1=0, P2=1).
- Score taint (`set_quick_fail`) applies in BOTH modes; results display reads
  live state, unaffected by save suppression.
- Registration failure of the option row degrades to default (skip = ON).
