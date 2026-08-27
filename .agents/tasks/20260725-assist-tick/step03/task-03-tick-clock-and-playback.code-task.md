# Task: The Tick Clock — Cursor, Adaptive Lead, and Playback

## Description

Make the mod actually clap: advance a cursor through the timestamp list built by the previous task,
fire one clap when a timestamp comes due, and compensate for the fact that playback is quantised to
the frame. Then delete Step 2's temporary scaffolding, which this replaces.

This is the task that proves the feature's timing model. Everything before it can be checked by
reading a log; this can only be settled by listening, so it is deliberately the last thing added
before the eligibility and side-selection work of Step 4.

## Background

**Working directory: this repository** (the DDR World hook DLL / modpack).

**Why a lead is needed at all.** The game submits audio work once per frame and never uses a
scheduled start time, so a clap lands on the first frame at which it is *detected*. Firing when
`times[cursor] <= music_count` alone would therefore make every clap systematically **late**, by
between zero and one whole frame period. Applying a **half-frame lead** centres the error at ±½ frame
instead of skewing it late.

**Why the lead must be derived, not hardcoded.** This codebase ships an FPS-unlock mod, so the frame
period is not a constant — a cabinet may run at 60, 120, 144, 165, 240 or 360 Hz. The lead is
therefore computed from the observed `music_count` delta between judge dispatches, clamped to a sane
range, with a fixed fallback on the first frame where no delta exists yet.

**Why at most one clap per frame.** If several timestamps come due in a single frame — a lag spike,
or a burst tighter than the frame period — replaying the backlog would machine-gun stale claps. The
cursor advances past all of them and exactly one clap is played. (This is FR-4; the plan assigns it to
Step 4, but the cursor-advance loop is where it naturally lives, so implement it here and note it.)

**Restart handling.** `music_count` rises monotonically within a song, so a *drop* means a new song or
a mid-song restart. The mod re-seeks rather than assuming the cursor is still valid.

## Reference Documentation

**Required:**
- Design: `.agents/planning/20260725-assist-tick/design/detailed-design.md` — §4.2's "Judge callback
  — the clock" pseudocode is the specification for this task's body, and §4.2.3 is the full rationale
  for the adaptive lead. §2.1's FR-1, FR-4, FR-6 and FR-11 are the requirements it satisfies; §5.2 is
  the state shape; §6's error table gives the failure behaviour
- `.agents/planning/20260725-assist-tick/implementation/plan.md` — Step 3, items 3 and 4

**Additional References (if relevant to this task):**
- `.agents/planning/20260725-assist-tick/research/existing-mechanisms.md` — §C3 on the units and
  behaviour of `music_count`
- `.agents/planning/20260725-assist-tick/research/note-taxonomy-and-actors.md` — §"`music_count` units
  — independently re-confirmed", and §"Quick-restart handling"
- `.agents/planning/20260725-assist-tick/implementation/tasks/game-audio-service/progress.md` —
  "Notes for Step 3": the cue name and pan to pass, and what remains scaffolding
- `src/services/game_audio.rs` — `play_cue`, and the `mod demo` block this task deletes

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. Implement the judge-callback body exactly as design §4.2 gives it: the rebuild branch, the
   identity check against the latched side, the rewind guard, the lead computation, and the cursor
   advance with a single play.
2. The **adaptive lead** lives in one named place — a single function or constant with the frame-rate
   reasoning at its definition — so that promoting it to an operator-tunable offset later (design
   Appendix C) is an addition rather than a refactor. It must be derived from the observed
   `music_count` delta, clamped to a sane range, with a fixed fallback before any delta exists.
3. When a tick is due, advance the cursor past **every** timestamp that is due and play exactly
   **one** clap. Never loop the plays.
4. A drop in `music_count` beyond a named threshold re-seeks the cursor (binary search over the
   sorted list), so a mid-song quick restart resumes correctly from the top rather than staying
   parked at the old position.
5. Play through `game_audio::play_cue` with the cue name `asti` and pan `0.0`. **Always
   centre-panned** — never side-panned, per FR-6: a cabinet is one shared stereo mix in one room, so
   panning does not isolate players.
6. **Delete Step 2's scaffolding** as part of this task: the whole `mod demo` block at the bottom of
   `src/services/game_audio.rs` **and** the marked `demo::install();` call at the end of that file's
   `init`. Nothing else in that service is scaffolding. After this, `game_audio` has no consumer
   other than the mod.
7. Add measurement logging that makes lateness a number rather than an impression: for the first N
   ticks of a song only, at debug level, log the scheduled timestamp, the `music_count` it actually
   fired at, and the difference. Also log the observed frame delta and the computed lead once per
   song. Nothing unbounded, nothing per frame after the first N.
8. The per-frame path must be O(1) and allocation-free once the list exists: a comparison against the
   cursor position and at most one audio call.
9. Panic-free: no `unwrap`, `expect`, indexing, or slicing that can panic anywhere the judge callback
   can reach. Index the timestamp list through checked access only — an off-by-one here would be a
   panic inside an `extern "C"` dispatch.
10. Failures stay silent and singular: a `play_cue` that returns false must not warn per tick (the
    service already warns once), and must not stall or rewind the cursor.
11. No new crate dependency, no config section, no new detour.

## Dependencies

- **Task 02 (mod skeleton and tick-list build)** must be complete — this task consumes its list,
  latch and per-song state
- **Step 2's `services::game_audio`** — `play_cue`; and its `demo` block, which this task removes
- No new crate dependencies

## Implementation Approach

1. Read design §4.2's pseudocode and §4.2.3 in full first. Between them the body is fully specified;
   the only real judgement is the clamp range and the fallback value for the lead.
2. Implement the cursor and the lead, with the measurement logging, and read the scheduled-vs-actual
   numbers out of `log.txt` before forming any opinion by ear — the point of that logging is that
   "sounds slightly late" and "is 8 ms late" are different kinds of finding.
3. Delete the scaffolding last, so that if the new trigger misbehaves there is a known-good sound to
   compare against during the same session.
4. Hand over for the listening pass, reporting the measured mean and spread of the scheduled-vs-actual
   delta.

## Acceptance Criteria

1. **Claps land on the beat**
   - Given a song with a steady rhythm
   - When it is played
   - Then a clap is heard on every note, in time with the music (the maintainer's listening pass)

2. **Lateness is measured, not guessed**
   - Given the measurement logging
   - When the first N ticks of a song are read from `log.txt`
   - Then each line carries the scheduled timestamp, the actual `music_count`, and the delta; the
     deltas are centred near zero rather than consistently positive, and their spread is within about
     half a frame period

3. **The lead adapts to the frame rate**
   - Given the observed frame delta logged once per song
   - When the cabinet runs at the stock 60 fps and again with FPS Unlock raising it
   - Then the logged delta and computed lead change accordingly, with no code change, and the ticks
     get tighter rather than doubling or dropping

4. **Ticks are chart-driven, not judgement-driven**
   - Given a song played with deliberate misses
   - When it is played
   - Then the claps keep perfect time regardless — this is the core consequence of the whole design
     and the single most important audible check in the step

5. **One clap per frame, maximum**
   - Given a burst of notes tighter than the frame period, or a lag spike
   - When it is crossed
   - Then exactly one clap is played and the cursor has advanced past all of the due timestamps — no
     machine-gunning, no backlog replay

6. **A quick restart resumes from the top**
   - Given a mid-song quick restart
   - When play resumes
   - Then the claps resume correctly from the beginning of the chart, and the log shows the re-seek
     happening once

7. **Multiple songs in one session stay correct**
   - Given several songs played in a row
   - When each is played
   - Then each claps correctly from its own start, with no drift, and the audio bank is registered
     only once for the session

8. **Step 2's scaffolding is gone**
   - Given the completed change
   - When `src/services/game_audio.rs` is inspected
   - Then the `mod demo` block and its `demo::install();` call are both absent, no other part of that
     service changed, and the only clap in a song comes from the mod

9. **The per-frame path is clean**
   - Given a full song
   - When the log is read and the frame rate observed
   - Then there is no unbounded or per-frame logging after the first N ticks, and the frame rate is
     unaffected

10. **The build gates pass**
    - Given the completed change
    - When `cargo check --target x86_64-pc-windows-msvc`, then `cargo fmt`, then `./build.sh` are run
    - Then all three complete cleanly

## Metadata
- **Complexity**: Medium
- **Labels**: timing, hot-path, audio, judge-hook, diagnostics, scaffolding-removal
- **Required Skills**: Rust; frame-quantised timing and lead compensation; writing a panic-free
  hot-path callback; measuring rather than guessing at latency
- **Generated By**: code-task-generator 2026-07-26
- **Source Plan**: `.agents/planning/20260725-assist-tick/implementation/plan.md`
- **Plan Step**: Step 3 — `mods::assist_tick` — end-to-end ticking on the dispatched actor
