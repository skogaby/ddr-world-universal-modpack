# Task: Release-Matrix Run Sheet and Final Build Handoff

## Description

Prepare the feature's release: compose the req-42 live-matrix run sheet
(including the Step-6 oracles) into the feature `progress.md`, re-run all
five standing gates on the final tree, and produce the release DLL for the
maintainer's deployment. The live matrix itself is MAINTAINER-RUN — this
repo's only real validation is live deployment plus log observation. This
task completes at handoff: run sheet ready, gates green, DLL built. The
matrix results and the plan Step 7 tick land afterwards, when the
maintainer reports back.

## Background

Design req 42 (release requirements): host validator green; the live
throughput benchmark (already PASSED at Step 5 — production ~21–22×
realtime, an 8.5-min 25 % song with 0 deferrals); the maintainer's live
matrix incl. slow (≤ 50 %), fast (> 100 %), Quick Restart, assist-tick
alignment, score containment re-oracle, and 100 % literal-stock
verification; and the standard check/format/release-build gates. The plan
adds: Premium Free interaction, a long-session soak (multiple rate songs
back-to-back watching the drain's reclamation diagnostics for growth), and
the leftover sweep (task-01). Step 6 added three more live oracles that
the plan predates: tick alignment must now be checked AT RATE (50 %),
Real Speed velocity (both Real Speed Fix states), and the PUS CSV rate
columns. Several legs already carry final-build live evidence from the
Step-5 deploys (Quick Restart, score containment, 100 % literal stock,
fault-injection silence) — the run sheet should mark those as
re-confirmation legs rather than first-time checks.

The run sheet is the acceptance record (plan Step 7's Tests note), logged
in the feature `progress.md`'s Deploy & test log.

## Reference Documentation

**Required:**
- Design: `.agents/planning/2026-08-08-song-rate-streaming/design/detailed-design.md`
  (req 42; §Testing strategy's live-matrix notes)

**Additional References (if relevant to this task):**
- `.agents/planning/2026-08-08-song-rate-streaming/progress.md` — Deploy &
  test log (the Step-5 evidence that turns several legs into
  re-confirmations) and the record this task extends
- `.agents/planning/2026-08-08-song-rate-streaming/implementation/step06-task-01-assist-tick-rate-conversion/progress.md`
  — the tick-alignment oracle's terms (rate={}% synthesis log line)
- `.agents/planning/2026-08-08-song-rate-streaming/implementation/step06-task-02-real-speed-effective-rate/progress.md`
  — the Real Speed velocity oracle + expected INFO log lines
- `.agents/planning/2026-08-08-song-rate-streaming/implementation/step06-task-03-pus-csv-rate-columns/progress.md`
  — the CSV spot-check terms

## Technical Requirements

1. **Run sheet** appended to the feature `progress.md` (a "Deploy #5 —
   release matrix (PENDING maintainer run)" entry), one line per leg with
   its oracle and, where applicable, the log line that evidences it:
   - Slow song ≤ 50 %: pitch-correct slow audio, arrows/judging in sync,
     loading a few seconds (not production-bound).
   - Fast song > 100 %: same, sped up.
   - Assist-tick alignment at 50 % AND 100 %: claps on judgment moments
     (the headline D6 use case); synthesis INFO carries `rate={}%`; 100 %
     placement audibly unchanged.
   - Real Speed velocity: a Real-Speed-mode player at 50 % sees the same
     on-screen arrow velocity as at 100 % (multiplier doubles — the
     `song_rate/real_speed` INFO line reports it), checked with the Real
     Speed Fix mod ON and OFF; fixed-multiplier mode untouched.
   - PUS CSV spot-check: a 50 % export carries
     `Song Rate Requested (%)`/`Song Rate Effective` cells (50 + the exact
     fraction); a 100 % export appends only the uniform identity cells.
   - Quick Restart at a non-identity rate (re-confirmation — final-build
     evidence exists from deploy #4).
   - Premium Free interaction: rate songs inside a Premium Free session
     behave; stage flow uninterrupted.
   - Score containment re-oracle (re-confirmation): rate stage saves
     suppressed; card-out logout save score-stripped; backend shows NO
     competitive record of rate songs and DOES show interleaved 100 %
     scores.
   - 100 % literal-stock verification (re-confirmation): no redirect, no
     bind, normal saves.
   - Long-session soak: several rate songs back-to-back; the drain's
     per-generation reclamation INFO shows no growth (bindings reclaimed,
     no slot leakage, no deferral creep).
2. **Final gates:** all five re-run on the final tree (after task-01's doc
   edits), logs in this task's `logs/`.
3. **Release build:** `./build.sh` output DLL confirmed present; the
   handoff note names the DLL path and `scripts/deploy.sh` as the
   maintainer's deployment route (do NOT deploy — maintainer-run).
4. This task's record closes at HANDOFF (`Status: Complete` = run sheet +
   gates + build). It must state explicitly that the plan Step 7 checkbox
   stays UNTICKED until the maintainer's matrix passes and the results are
   logged in the feature `progress.md`.

## Dependencies

- task-01 (documentation + sweep) — the final build should carry the
  finished doc tree.

## Implementation Approach

1. Draft the run sheet from req 42 + the plan guidance + the three Step-6
   records; mark re-confirmation legs.
2. Append it to the feature `progress.md` (Deploy & test log) and set the
   NEXT ACTION to the maintainer's matrix run.
3. Re-run the five gates; build; write the handoff note in the task record.

## Acceptance Criteria

1. **The run sheet is executable by the maintainer alone**
   - Given the appended Deploy #5 entry
   - When the maintainer runs the matrix with no agent present
   - Then every leg names its setup, its pass/fail oracle, and the log
     evidence to capture

2. **The tree is release-ready**
   - Given the final tree (task-01 included)
   - When the five gates run
   - Then all pass (validator green, se-bank green, 0-warning windows
     check, clean fmt, release DLL built)

3. **Nothing is over-claimed**
   - Given the task record and the plan
   - When this task closes
   - Then the plan Step 7 checkbox is unticked, the record says why, and
     the feature progress.md NEXT ACTION points at the maintainer's
     matrix run

## Metadata

- **Complexity**: Low
- **Labels**: song-rate, release-matrix, run-sheet, handoff
- **Required Skills**: the song-rate feature's live-evidence conventions,
  repository gate discipline
- **Generated By**: code-task-generator 2026-08-11
- **Source Plan**: `.agents/planning/2026-08-08-song-rate-streaming/implementation/plan.md`
- **Plan Step**: Step 7: Hardening, documentation, and the release matrix
