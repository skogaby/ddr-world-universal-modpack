# Progress — Auto-Calibration

Updated: 2026-08-26
Status: COMPLETE — all 5 plan steps done and cabinet-verified (deploy #2).
Work is uncommitted; maintainer commits manually.
NEXT ACTION: none (feature closed). Optional housekeeping: move this
planning dir to `.agents/planning/_archive/` whenever convenient (update the
AGENTS.md row's planning pointer if so).

Resume protocol: read `implementation/plan.md` (checklist), then
`design/detailed-design.md` (Approved 2026-08-26, amended post-validation),
then the scratchpad records under
`.agents/scratchpad/2026-08-26-auto-calibration/`.

## Done

- Steps 1–4 implemented (see scratchpad records): toast service promotion
  (`src/services/toast/`), timing_offsets restructure + calibrate row +
  session lifecycle, data_feed tap + `compute` core + apply, D18/D19 hides.
- Step 5 docs: AGENTS.md Key Entry Points row (Auto-Calibration) + config
  bullet; README.md mods table + Timing offsets section (operator guide);
  design doc amended (guard correction + row refresh) and sign recorded as
  CABINET-VERIFIED.
- Post-validation fixes (2026-08-26, deploy #1 feedback):
  1. Overlay row display staleness — `set_offset` applied correctly (map +
     JSON) but the SOUND OFFSET row kept showing the old value (row store
     only updates via menu edits; a later edit would step from the stale
     value). Fix: `refresh_overlay_row(SOUND_IDX)` (idempotent scalar-row
     re-registration) after the calibration apply.
  2. Autoplay misattribution on quick-exit — the guard read
     `score_guard::is_stage_suppressed`, which ORs quick-fail/training/
     assist-tick/rate score taints. Fix: new
     `score_guard::is_autoplay_tainted` (autoplay bit alone); quick-exited
     calibration songs now apply honestly.
- Gates after fixes: cargo check clean, fmt applied, harness 14/14,
  ./build.sh release clean. All work uncommitted (maintainer commits).

## Deploy & test log

- Deploy #1 (2026-08-26, maintainer): sign direction VERIFIED (−40 ms
  mis-set converged to ~baseline; post-calibration play timed great).
  Toasts, arm consumption, hiding, regressions all good. Two issues found →
  both fixed (above). Quick-restart sample reset (plan Step 3 demo item 4)
  not yet tested.
- Deploy #2 (2026-08-26, maintainer): everything working as expected —
  overlay SOUND OFFSET row now displays the calibrated value (auto-adjust
  confirmed), quick-exit attribution fixed, full calibration flow verified.
  (The `count=` INFO line wasn't observed on the quick-restart check —
  likely a sub-30-sample WARN path instead — but behavior was correct;
  feature closed on maintainer's call.)

## Deviations & open questions

- compute() rounds the MEAN (displayed delta) then adds — toast delta ==
  written delta exactly.
- Design §6 realized as `src/services/toast/` (mod.rs + pure curve.rs).
- Quick-exit with ≥30 valid samples now APPLIES the calibration (was:
  refused as "autoplay"). Deliberate: the steps are humanly real; D16 still
  consumes the arm.

## Key facts for a cold resume

- Row: `timing_calibrate_next`, top of the timing-offsets GLOBAL SETTINGS
  section; in-memory only; D16 one-rule (any song end while ON flips OFF).
- Guards: exactly-one entered side (census), rate 100 % (entry + apply),
  `is_autoplay_tainted` at apply (NOT `is_stage_suppressed`).
- Formula: `new = clamp(old + round(mean(delta_ms)))`; errors negative=early
  / positive=late; SOUND_OFFSET higher = audio later. Sign cabinet-verified.
- Apply path: `set_offset(0, new)` → `refresh_overlay_row(0)` → 5 s toast +
  INFO old/mean/count/stddev/new.
- Host tests: `scripts/validate_auto_calibration.sh` (toast curves + census +
  compute, 14 tests).
