# Task: `mods::assist_tick` — Mod Skeleton, Lifecycle, and Tick-List Build

## Description

Add the assist-tick mod itself: its `Mod` trait implementation, its scene and judge wiring, and the
per-song build of the list of chart timestamps that will later be clapped.

This task deliberately stops short of playing anything. It proves the half of Step 3 that can be
checked by reading a log — that the mod's lifecycle is sound and that it reads the game's own note
records correctly — before the half that can only be checked by ear is layered on top. A wrong
note-record read and a wrong clock produce the same symptom (claps in the wrong places), so they are
implemented and verified separately.

Step 2's temporary demo trigger stays in place for this task, so the tree keeps making a sound and
the audio path stays demonstrably alive; the next task removes it as it takes over.

## Background

**Working directory: this repository** (the DDR World hook DLL / modpack).

The mod's job is to clap at each arrow's chart timestamp. It gets those timestamps from the game
rather than from the chart file, and it gets its per-frame clock from the existing shared judge
dispatcher rather than from a timer of its own.

Three properties of the ground this stands on:

- **The judge dispatcher is a clock, nothing more.** Its callback is
  `fn(actor: *mut u8, music_count: i32)`, fired once per frame **per side** during gameplay, where
  `music_count` is **milliseconds** and starts *negative* (the lead-in — Step 2's logs show the first
  gameplay frame at `-87`). The mod never triggers off judgement, because judgement follows the
  player's input and would make the clap follow their mistakes.
- **The actor's Results vector is the whole chart for that side** — one 0x40-byte record per note,
  sorted, built in the same call that enters the play state, so it is complete at the first judge
  dispatch. It **includes** shock arrows, freeze tails, THINOUT notes and this DLL's own injected
  mines, so a later filter must *reject* rather than assume.
- **Scene callbacks fire before the next scene is constructed**, so no actor exists yet when the
  gameplay scene event arrives. That is precisely why the list is built on the first judge tick and
  not on the scene event.

**Deliberately over-permissive, per the plan.** This task accepts **every** note with a non-negative
timestamp. The real eligibility predicate — taps only, not a shock, has a live panel — is Step 4's.
Do not implement it early: the plan's sequencing exists so that an audible mistake in Step 3 is
unambiguously a timing mistake.

## Reference Documentation

**Required:**
- Design: `.agents/planning/20260725-assist-tick/design/detailed-design.md` — §4.2 is the
  specification for this task (the lifecycle table, the option-row/scene/judge wiring, and §4.2.2's
  list build). §3.3's sequence diagram shows the order of operations across init, scene entry and the
  first judge dispatch. §5.1 is the structure/offset table and §5.2 the mod-state shape. §6's error
  table gives the required behaviour of each failure path
- `.agents/planning/20260725-assist-tick/implementation/plan.md` — Step 3, items 1, 2 and part of 3

**Additional References (if relevant to this task):**
- `.agents/planning/20260725-assist-tick/research/existing-mechanisms.md` — the codebase API survey.
  §B1 is an idiomatic `Mod` impl to copy the shape of, §B3 is scene wiring plus gameplay reset, §B4 is
  judge subscribe/unsubscribe, §C1 is gameplay entry/exit detection, §C2 is getting the
  `GamePlayActor` (one per side, not shared), §C3 is the units of `music_count`
- `.agents/planning/20260725-assist-tick/research/note-taxonomy-and-actors.md` — §"Results-vector
  coverage" for what is and is not in the vector, and §"Timing of availability" for why the first
  judge dispatch is the right moment. Its §"The tick predicate" is Step 4's, not this task's
- `.agents/planning/20260725-assist-tick/implementation/tasks/game-audio-service/progress.md` —
  "Notes for Step 3": how to load the bank bytes, what `is_available()` now promises, and the exact
  cue name
- `src/mods/mod_trait.rs` — the `Mod` trait, `ModContext`, and `required_signatures`
- `src/services/judge_hook.rs`, `src/services/scene_manager.rs`, `src/services/game_audio.rs`
- `src/types/game_note.rs` — the hoisted note-record helpers (task 01)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. Add `src/mods/assist_tick.rs`, registered in `src/mods/mod.rs` and constructed in `src/lib.rs`'s
   mod list, following the placement and style of the surrounding mods. Mod id `assist-tick`.
2. Implement the `Mod` trait per design §4.2's lifecycle table:
   - `init` verifies its prerequisites (`game_audio`, `judge_hook`, `scene_manager`) and loads the two
     bank files, keeping the bytes. Return `false` — mod skipped, one warning — if anything is
     missing. File IO belongs here (init thread), never on a per-frame path.
   - `enable` subscribes to scene changes and registers the judge pre-callback at `Normal` priority.
   - `disable` unregisters both and clears per-song state. Registered audio banks are **deliberately
     left in place** (design §6's "deliberate non-behaviours" — destroying an XACT bank a live cue may
     reference is a crash class this codebase has already been burned by, and an idle bank costs
     nothing).
3. Load the bank pair from `data_mods/assist_tick/banks/tick.{xwb,xsb}` through the existing
   mod-path resolver, using the same call Step 2's scaffolding used. A missing file is one warning
   naming the expected path, and the mod declines to init.
4. `game_audio::is_available()` is **necessary but not sufficient** — since Step 2's approved
   amendment it means "addresses resolved" and no longer implies the XACT engine module is loaded.
   Treat a wrong engine as something that surfaces later as one declined registration, not as
   something to pre-empt here.
5. On entry to the gameplay scene: clear song state and set a rebuild flag. On leaving: clear the
   tick list and the latch. Do **not** try to touch an actor from the scene callback — none exists
   yet.
6. On the first judge dispatch of a song: ensure the audio bank is registered (idempotent — first
   song only), latch the dispatched actor's side, and build the timestamp list from **that** actor's
   Results vector using the hoisted helpers. Subsequent dispatches for the other side are ignored by
   comparing against the latch, not by a second subscription — one callback receives both sides.
7. The list build must: read the Results range, walk the entries, take each note's `music_count`,
   accept every note with a **non-negative** timestamp, then sort and de-duplicate exactly. No
   eligibility filtering and no coalescing window in this task — both are Step 4's.
8. Build the list **once per song**, never per frame. A typical chart is on the order of a thousand
   `i32`s.
9. An empty, misaligned or reversed Results vector must yield an empty list and an inert song — no
   ticks, no crash. The hoisted walk helper already refuses a malformed range; rely on it rather than
   duplicating the checks.
10. **No playback in this task.** Do not call `play_cue`, and do not remove Step 2's scaffolding.
11. Diagnostic logging is this task's only verification, so it is a requirement. Log **once per
    song**: the latched side, the entry count in the Results vector, how many timestamps were kept,
    and the first several timestamps. Log **once** on each failure path. Nothing per frame.
12. Panic-free in every path a hook callback can reach: no `unwrap`, `expect`, indexing, or slicing
    that can panic. The judge dispatcher wraps subscribers in `catch_unwind`, but the callback body
    must not rely on that.
13. Per-frame work must stay O(1) once the list exists — the callback runs once per frame per side.
14. No new crate dependency, no config section, no changes to game data files, no new detour
    anywhere.

## Dependencies

- **Task 01 (hoist the note-record helpers)** must be complete — this task consumes
  `src/types/game_note.rs`
- **Step 2's `services::game_audio`** — for `is_available` and `register_bank`. Complete and verified
- `src/services/judge_hook.rs` — the per-frame clock; subscribe with `register_pre`, unsubscribe by
  handle
- `src/services/scene_manager.rs` — `on_scene_change` / `remove_callback`, and `types::scenes`'
  `GAMEPLAY` constant
- `src/services/avs_layeredfs/mod_paths.rs` — the `data_mods/` resolver
- `src/mods/mod.rs`, `src/lib.rs` — registration
- No new crate dependencies

## Implementation Approach

1. Read design §4.2 and §3.3 end to end, then §B1/§B3/§B4 of `existing-mechanisms.md` for the three
   wiring patterns this mod needs. Between them the lifecycle is fully determined.
2. Write the skeleton — `Mod` impl, state struct, scene subscription, judge subscription — and
   confirm on the local install that the mod registers, enables, and logs a gameplay entry, before
   writing a line that reads game memory.
3. Add the list build. Read the count and the first few timestamps out of `log.txt` and sanity-check
   them against the song actually played: the first timestamp should be small (the lead-in is
   negative and is excluded), the count should be plausible for the chart, and the values should be
   monotonically increasing.
4. Leave playback and the cursor alone.

## Acceptance Criteria

1. **The mod registers and enables**
   - Given the built DLL installed in the local game
   - When the game boots
   - Then the log shows the mod registered and enabled, with its bank bytes loaded, and no warning

2. **Missing prerequisites decline cleanly**
   - Given the bank files are renamed away (or a prerequisite service is unavailable)
   - When the game boots
   - Then the mod declines to init with exactly one warning naming what was missing, the game boots
     normally, and every other mod still works

3. **Gameplay entry and exit are observed**
   - Given the mod is enabled
   - When a song starts and then ends
   - Then the log shows song state being armed on entry and cleared on exit, exactly once each, with
     no attempt to read an actor from the scene callback

4. **The tick list is built once per song, from the game's own records**
   - Given a song is started
   - When the first judge dispatch arrives
   - Then the log reports, exactly once for that song: the latched side, the Results-vector entry
     count, the number of timestamps kept, and the first several timestamps — and the timestamps are
     non-negative and strictly increasing

5. **The list is plausible for the chart**
   - Given a song whose step count the maintainer can eyeball
   - When the per-song line is read
   - Then the kept count is in the right ballpark for that chart, and is **higher** than the eventual
     eligible-note count will be — this task is deliberately over-permissive, so shock arrows, freeze
     tails and THINOUT notes are all still included

6. **A second song rebuilds, and rebuilds only once**
   - Given several songs played in one session
   - When the log is read
   - Then each song produces exactly one build line, and the bank registration line appears only for
     the first

7. **A malformed Results vector is inert**
   - Given a Results range that is empty, misaligned or reversed
   - When the first judge dispatch arrives
   - Then the list is empty, the song is inert, one line records it, and no crash occurs

8. **Nothing plays yet, and Step 2's scaffolding is untouched**
   - Given the completed change
   - When the source and the log are inspected
   - Then the mod calls no playback function, the `demo` block in `services/game_audio.rs` is still
     present, and the only clap in a song is still the scaffolding's single one

9. **The per-frame path is quiet and cheap**
   - Given a full song
   - When the log is read
   - Then there is no per-frame logging, and the frame rate is visibly unaffected

10. **The build gates pass**
    - Given the completed change
    - When `cargo check --target x86_64-pc-windows-msvc`, then `cargo fmt`, then `./build.sh` are run
    - Then all three complete cleanly

## Metadata
- **Complexity**: Medium
- **Labels**: mod, lifecycle, judge-hook, scene-manager, note-records, diagnostics
- **Required Skills**: Rust with `unsafe` discipline; reading raw game structures through provided
  helpers; in-process hook-DLL constraints (no panics across FFI, thread affinity, hot-path budget);
  restraint about implementing a later step's filtering early
- **Generated By**: code-task-generator 2026-07-26
- **Source Plan**: `.agents/planning/20260725-assist-tick/implementation/plan.md`
- **Plan Step**: Step 3 — `mods::assist_tick` — end-to-end ticking on the dispatched actor
