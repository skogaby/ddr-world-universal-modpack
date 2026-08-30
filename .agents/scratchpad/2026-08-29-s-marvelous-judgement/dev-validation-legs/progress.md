# Progress — task-03 dev-validation-legs

Status: Complete (uncommitted — maintainer commits manually)

## Checklist
- [x] `scripts/validate_s_marvelous.sh` extended: env-gated dev legs
  (`DDR_WORLD_INSTALL` + `BEMANIUTILS_DIR`, default sibling checkout),
  `[skip]` + exit 0 when absent (verified)
- [x] `ap2check` `[[bin]]` generated in the temp crate: `roundtrip` (BSI +
  string-cipher descramble → parse → serialize → byte-compare) and `info`
  (normalized structural lines) subcommands
- [x] Leg A: extraction via bemaniutils arcutils/ifsutils into temp dirs;
  round-trip over ALL templates of dance_judge0000_v0, dance_fullcombo0000_v0,
  scene_result_v3 — **76/76 byte-identical**
- [x] Leg B: `afputils parseafp` JSON vs `ap2check info` (exported name,
  frame count, per-frame spans) — 3/3 structural match (dance_judge,
  marvelous, body_tab_detail_result)
- [x] Repo hygiene: temp dirs only, nothing written into the repo; skip run
  verified; zero harness warnings after the `#![allow(dead_code)]` polish

## Interactive pre-work (informed the script)
- Proved extraction + round-trip manually before encoding (throwaway crate
  in the system temp dir). Found: arcutils/ifsutils have no subcommand (bare
  `file -d dir`), and template inventory intel for later steps (below).

## Findings for later steps (recorded in feature progress.md)
- The score-tab template FILE/export name is `body_tab_detail_result` (the
  RE's `"detail_result"` string is the game's creation-call arg — mapping to
  confirm in Step 7 before keying the afp_patcher patch).
- dance_judge package: the MARVELOUS word is its own template `marvelous`
  (1172 bytes) with dedicated geo (`marvelous_shape*`) and texture
  (`tex/dance_judge0000_marvelous`) — a clean, small clone donor for Step 4.
- dance_fullcombo package: 16 templates — the four `*_fullcombo_*` splash
  templates PLUS per-grade `effect_*`/`which_fullcombo_*` families that may
  also need `s_marbelous` variants for full fidelity (Step 6 decision).

## Deviations
- None beyond the plan. Commit step skipped per repo AGENTS.md git rules.

## Step 2 sibling status
- task-01 ap2-model-parser: Complete
- task-02 ap2-serializer: Complete
- task-03 dev-validation-legs: Complete (this)
→ Step 2 checklist item ticked in the source plan.
