# Task: Extend Host Validation, Run the Single Cabinet Pass, and Close Steps 5+6

## Description
Extend the host validator with an on-demand cold/warm generation demo, then perform the ONE deployment of this merged delivery and run the combined cabinet matrix: on-demand generation across the scalar rate range (75%/125% primary oracles plus an extreme-rate observation), player-facing selection with card persistence round-trip, policy fail-closed cases, failure fallback, and score containment. On success, tick plan Steps 5 AND 6 and update the canonical progress record.

## Background
Tasks 1-3 are host-only by design; this task concentrates all manual validation in a single deployment (the maintainer directive carried forward from Step 4). The maintainer personally deploys, runs all cabinet tests, and commits.

The load-bearing live unknown is U1: on-demand cold-build blocking. Step 4 only ever imported a pre-generated file at open time; this pass is the first time the FileManager open blocks for the full DSP duration on cabinet hardware under CrossOver/Wine. Design requirement 24 governs it: an uncached generation may pause or extend the stage-loading screen; only the proven waiting sites block, for at most 30 seconds; a diagnostic log records caller thread identity and whether render frames continue. The second live unknown is rates above 100% (Q31 > identity has only ever run in host tests). The rate domain is the maintainer-approved scalar range: multiples of 5 in 25%..=175% (scene selection via the scalar row, granular step 5 / coarse step 10); extreme slow rates carry the longest builds and the largest outputs, so they are the U1 worst case and may legitimately refuse via memory/duration admission on long songs (early failure -> stock).

Cabinet environment: `$DDR_WORLD_INSTALL` is the CrossOver install; runtime log at `$DDR_WORLD_INSTALL/log.txt` (resets per boot). The Step 4 leftovers must be cleaned as part of staging: the `song_playback_speed.diagnostic` block in `mod-config.json` (retired by Task 1 — verify it is ignored gracefully), the staged `data_mods/_diag/abdt-75.xwb`, and the warm cache entry `dbea619e...` (from the old import path; safe to leave — startup recovery/eviction handles stale-version entries — but note whichever choice is made in the log evidence). Movies are globally suppressed on this cabinet by `non_native_os_support`, so movie-related checks are LOG-level only; native-Windows movie behavior remains deferred release evidence.

Per maintainer decision (2026-08-07), the Step 7 competitive-field score audit is deferred: the maintainer will perform a final audit once the feature is complete. This pass re-verifies the Step 4 containment oracles (suppressed rate-tainted stage saves, sanitised logout, backend absence) but does not expand the sanitizer.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-08-05-song-playback-speed/design/detailed-design.md` (Detailed Requirements 24, 48-53; Observability and Evidence; Testing Strategy)
- Plan Steps 5-6 (Tests + Demo sections): `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md`
- Progress: `.agents/planning/2026-08-05-song-playback-speed/progress.md` (Deploy & test log — the expected log-line vocabulary from Step 4's oracle runs)

**Additional References (if relevant to this task):**
- `scripts/validate_song_playback_speed.sh` (schema-v1 report; the Step 2 `cache` and Step 3 `identity_runtime` sections as the additive-section precedent)
- `src/services/song_rate/runtime.rs` (maintenance-drain log lines), `src/services/score_guard.rs`

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Extend `scripts/validate_song_playback_speed.sh` with an additive on-demand section in the stable `song-rate-validation/v1` report (no schema break, existing sections untouched): an end-to-end host demo driving the Task 1 open-redirect path against a temporary cache — cold build at 75 and 125, warm hit with zero transforms, and key invalidation — recording exact rates, digests, cold/warm latency, and per-check status. Fold in the still-meaningful half of the deferred Step 4 report extension: record song/source/generated/module/platform digests and the exact rate for the transaction demo.
2. Add the design-requirement-24 diagnostic evidence: the open-redirect path (non-detour side) logs caller-thread identity and cold-build wall time so the cabinet run can prove whether render frames continue during a cold build.
3. Update the modpack surface docs per the design's deliverables list, minimally: `AGENTS.md` key-entry-points row for the feature, `mod-config.json` example (`song_playback_speed.cache_limit_gib`, `custom_options.row_order` with `song_speed`), and a README configuration note. Durable RE-note completion remains Step 8.
4. Run all five host gates green, then hand the build to the maintainer for the single deployment (agent never deploys or commits).
5. Cabinet matrix (maintainer-run, log- and backend-verified), one session where possible:
   a. Card in; set SONG SPEED 75 on the entered side; play a song with a COLD cache: observe loading-screen behavior (U1 — pause length, render continuity, no watchdog/crash), then pitch-correct 75% audio with arrows in sync; log shows arm -> open-redirect -> exposed -> committed with the exact rate.
   b. Replay the same song (warm hit): near-instant load, same committed rate, `warm` evidence in the log.
   c. Play a different song at 75% (second cold build) to prove worker-thread reuse across songs.
   d. Set 125 and play: pitch-correct faster audio, arrows in sync (first live above-identity rate).
   e. One extreme-rate observation: play a song at a boundary-region rate (e.g. 50 or lower): observe the worst-case cold-build duration against the 30-second deadline and the loading-screen behavior; a memory/duration admission refusal on a long song is an ACCEPTED outcome if it falls back stock with one bounded warning.
   f. Set 100 and play: literal stock behavior — no open-redirect/exposed/committed lines, `path=Stock` bank creation, normal score save allowed.
   g. A static custom-song LayeredFS XWB at a non-100 rate: generation composes from the replacement source.
   h. Policy fail-closed spot checks: local versus (or a second entered side) and course/special chain arm identity per the log.
   i. Persistence round-trip: adjust the scalar row (verify granular 5 / coarse 10 stepping and the 25/175 clamps in the UI), card out (logout sanitised for rate-tainted sides; profile forwarded), card back in, row shows the persisted value; backend DB shows `opt_mod_song_speed` stored; backend has NO score rows for rate-played songs and HAS the 100% song's score.
   j. Failure fallback: one injected early failure via `DDR_SONG_RATE_FAULT` (dev mode) proving stock-100% fallback live; optionally a cache-eviction/space observation if practical.
6. If any matrix leg fails, stop and record the failure in progress.md (Deploy & test log) with the log evidence; do not tick the plan.
7. On full pass: tick plan checklist Steps 5 and 6, update `progress.md` (Status, Done, Deploy & test log, Deviations — including the maintainer-owned deferred final score audit and the deferred native-Windows evidence), and set the NEXT ACTION to Step 7 planning.

## Dependencies
- Tasks 1-3 complete with green gates.
- Maintainer availability for the deployment and cabinet session; backend DB access for the persistence/score checks.
- Cabinet staging cleanup of Step 4 diagnostic leftovers (config block, `_diag` bank).

## Implementation Approach
1. Write the validator extension test-first against the schema (new section required, existing checks unchanged).
2. Add the requirement-24 thread/latency diagnostics on the non-detour side.
3. Make the docs/config-example updates; run the five gates.
4. Prepare a concise cabinet run sheet from requirement 5 (expected log lines per leg, drawn from the Step 4 oracle vocabulary) for the maintainer.
5. After the maintainer session, transcribe evidence into progress.md and close the plan steps.

## Acceptance Criteria

1. **Validator Extension**
   - Given the extended script on a host with the sibling checkout
   - When `./scripts/validate_song_playback_speed.sh` runs
   - Then the schema-v1 report gains the on-demand section with cold/warm/invalidation checks at 75 and 125, all existing sections still pass, and `overall_pass` is true

2. **On-Demand Cabinet Proof (U1 + U2)**
   - Given a cold cache and a selected 75% rate
   - When the song loads and plays on the cabinet
   - Then the cold build completes within the deadline with documented loading behavior and render-continuity evidence, audio is pitch-correct at 75% with arrows in sync, the warm replay skips the transform, 125% passes the same oracle, and the extreme-rate leg's outcome (worst-case build time, or an accepted admission refusal falling back stock) is documented

3. **Policy and Persistence Live**
   - Given per-side selection, ineligible modes, and a card-out/card-in cycle
   - When the matrix legs run
   - Then only eligible solo/doubles arm the selected rate, 100% and ineligible modes are literally stock, the persisted value round-trips through `opt_mod_song_speed`, and the backend contains no competitive score rows for rate-played songs

4. **Failure Fallback Live**
   - Given one injected early failure on the cabinet
   - When the song loads
   - Then playback is stock 100% at identity clock with one bounded warning, no score taint, and the next song behaves normally

5. **Step Closure**
   - Given all matrix legs pass
   - When the records are updated
   - Then plan Steps 5 and 6 are ticked, progress.md carries the full evidence log plus the recorded deferrals (final score audit, native Windows), and the NEXT ACTION points at Step 7

## Metadata
- **Complexity**: Medium
- **Labels**: validation, cabinet, deployment, score-integrity, persistence, closure, step-5, step-6
- **Required Skills**: code-assist, verification, tdd-verification-workflow
- **Generated By**: code-task-generator 2026-08-07
- **Source Plan**: `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md`
- **Plan Step**: Steps 5+6 (merged delivery, maintainer-approved 2026-08-07) — closure task
