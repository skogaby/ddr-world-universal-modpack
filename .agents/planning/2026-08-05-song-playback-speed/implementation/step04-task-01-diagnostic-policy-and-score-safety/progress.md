# Progress: Step 4 Task 1 — Build Diagnostic Policy and Score Safety

Updated: 2026-08-06
Status: Complete (no commit — the maintainer commits personally per repo
workflow; the handoff explicitly forbids committing without request)

## Checklist

- [x] Setup: working dir, approval lineage verified, project docs read
- [x] Explore: requirements + integration map in `context.md`
- [x] Plan: `plan.md` (test scenarios + shape)
- [x] Harness extension → TDD red (`logs/validate-red.log`: died on absent
      `lifecycle.rs`)
- [x] score_guard rings/latches/policy + tests
- [x] lifecycle.rs (+tests): validation, eligibility, state machine, sinks
- [x] stage_records stage-counter decode + accessor
- [x] config.rs `song_playback_speed` section
- [x] custom_options_persistence decode/league/reset/latches
- [x] runtime.rs + mod/lib wiring
- [x] Full gate suite green
- [x] Canonical progress.md updated (Task 2 next)

## TDD cycles

- RED: validator file-existence gate rejected the intentionally absent
  `src/services/song_rate/lifecycle.rs` (`logs/validate-red.log`).
- GREEN (pure layer): 111 host tests pass (71 pre-existing + 17 score_guard +
  23 lifecycle), full validator exit 0 (`logs/validate-iter1.log`).
- Windows integration (stage_records/config/cop/runtime/lib) compiled clean on
  first `cargo check --target x86_64-pc-windows-msvc`.
- Final gates: song-rate validator (111 tests + demos + stable report),
  se-bank validator, windows check (0 warnings), whole-crate `cargo fmt`,
  release `./build.sh` — all green, re-verified after the review edits
  (`logs/validate-postreview.log`, `logs/build-postreview.log`).

## What was built

- `src/services/song_rate/lifecycle.rs` (+`lifecycle_tests.rs`): all nine
  generation phases with lock-free CAS transitions; scene-26 eligibility
  classifier (solo/doubles arm, course/local-versus/unknown fail-closed);
  `DiagnosticSpec` validation (exactly 75%); `LifecycleSink` trait with
  ordering-asserted effects (identity reset strictly before movie clear at
  gameplay exit); XactInFlight supersession refusal; Task 2 phase entry
  points with legality validation.
- `src/services/score_guard.rs` (+`score_guard_tests.rs`): per-side 8-entry
  `RateSaveLedger` (Free/Init/Pending/Claimed/Consumed, oldest-first exact
  (side,stage) claims, generation-idempotent appends, sticky overflow,
  per-side positive-match reset); `is_stage_suppressed` pending/overflow
  backstop; `SanitizationReadiness` five-latch conjunction +
  `is_full_sanitization_available()`; `LeagueStripOutcome` tri-state policy.
- `src/services/song_rate/runtime.rs`: permanent scene callback (atomics/raw
  reads only, lazy game-memory reads when no diagnostic), production sink,
  bounded warnings.
- `src/services/custom_options_persistence.rs`: rate election before any side
  default (unknown side/stage fail closed while rate state exists, never
  default-P1, never consume unmatched); league strip returns tri-state and
  RemovalFailed fails the sender closed (return 0); sanitiser/league latches;
  ddrcode-deferred per-side rate reset at SONG_SELECT.
- `src/services/stage_records.rs`: non-fatal stage-counter decode hoist
  (validated `FF 41` INC bytes + range-checked disp8) and `stage_counter()`.
- `src/mods/config.rs`: `song_playback_speed.diagnostic` raw section;
  `src/lib.rs`: readiness latches + dev-gated diagnostic validation +
  `song_rate::runtime::init`.
- `scripts/validate_song_playback_speed.sh`: harness gains types/scenes,
  score_guard(+tests), lifecycle(+tests); no report-schema change.

## Deviations

- Plan scenario 36 (`decode_stage_counter_offset` host test): the decode
  lives in windows-bound `stage_records.rs`, outside the host harness; the
  byte validation mirrors premium_free's proven logic and is covered by the
  windows target check + the Task 3 cabinet log instead of a host test.
- No commit was created (SOP Step 6): repo workflow reserves commits for the
  maintainer, and the handoff explicitly prohibits unrequested commits. The
  worktree carries all Step 1–4.1 work uncommitted by design.
