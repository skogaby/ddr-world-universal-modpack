# Progress — pure-cores (Step 2: task-01 + task-02 as revised)

Status: Complete (uncommitted — maintainer commits manually)

## Checklist
- [x] stub `mod.rs` + `src/mods/mod.rs` declaration; cargo check
- [x] eligibility.rs red → green (5 tests)
- [x] skill.rs red → green (7 tests) — **no-Boo correction**: `GOOD_WINDOW_MS = 124` outermost,
      `decide` Miss beyond ±124, `grade_for_offset` never 4, L1 miss-rate test 12..17 %
- [x] planner.rs red → green (16 tests) — `NoteView.kind` (tails never decided/pressed), floored
      `E > +124 ⇒ Miss`, one live event per panel per frame with reservation, note-order monotone
- [x] gauge RE (`docs/gauge_and_judge_scoring_research.md`): judge acceptance `grade < min(best,4)`,
      one accepted note/frame, freeze judge, `judge_submit` order + formulas, NORMAL gauge exact
      integer transcription (verified against hand-computed anchors)
- [x] `tools/bot_sim/` crate: chart.rs (7 tests), gauge.rs (7), scoring.rs (7), judge_model.rs (6),
      report.rs (2), main.rs (CLI/threads/`~` paths); `#[path]` mounts in `core/ssq/mod.rs`,
      `bot/mod.rs`; `src/core/ssq/timing.rs` +`entries()` accessor
- [x] `scripts/validate_multiplayer_bot.sh` (cargo test in the tool, `--report`), `scripts/bot_sim.sh`
- [x] `.gitignore`: `/tools/bot_sim/target`, default report outputs
- [x] Corpus run: 1,586 files / 6,621 single charts / 66,210 songs in 6.6 s, 0 parse errors,
      66 mismatches (0.003 %) — dense-stream one-per-frame race, documented in the report
- [x] HTML report rendered headlessly (Playwright) — tables, heatmap, scorecards all draw
- [x] Design A.5/§4.6/§4.7/§7.1 corrected + approval re-dated; plan Step 2 rewritten (as built)
      + ticked; task-02 file superseded note
- [x] `cargo check --target x86_64-pc-windows-msvc` clean; `cargo fmt` both crates; `./build.sh`
      clean; path hygiene clean

## TDD cycles
1. eligibility: red (missing items) → green 5/5.
2. skill: red → green 7/7 (first run caught a `0xDDR0` literal typo).
3. planner: red → green; the first stricter judge emulator exposed (a) a tally flush gap (last
   judged note never observed), (b) a panel-reservation gap (a blocked jump let a later note
   emit on a shared panel) — both fixed; then the Boo correction re-based 3 tests.
4. tool: gauge/scoring/chart/report tests green first run; judge_model 2 failures = the same tally
   flush at the loop exit → one post-loop `plan_frame` → 63/63.

## Deviations
- Step 2 scope change (maintainer-approved): synthetic `--report` harness → `tools/bot_sim`
  real-corpus simulator + gauge RE. Design/plan updated and re-dated.
- Scaffolding was reverted once by an external event mid-session; redone and verified.

## Corpus headline (1 seed)
Original constants (75/5/0.05/1.5): L1 55 % fail (b 4 / E 93), L2 0.5 %, L3+ 0 — a cliff;
L10 85.7 % MFC+ (10.2 % S-MFC). Maintainer judged L10 "effectively unbeatable".
**Tuned + codified (60/5.4/0.13/1.4):** L1 60 % fail (b 7 / B 46 / D 82 / E 95 / C 93),
L2 27 %, L3 5 %, L4 0.3 %, L5+ 0; L9 5 % MFC+; L10 71 % MFC+ (4 % S-MFC, rest PFC).
Tool gained `--pmiss-exp`, `--summary`, `--no-html` for the tuning loop.
