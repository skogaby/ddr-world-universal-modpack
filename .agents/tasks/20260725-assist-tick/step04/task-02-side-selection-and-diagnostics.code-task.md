# Task: Side Selection and the First-Tick Diagnostic

## Description

Choose the tick side from the game's **live actor set** instead of latching whichever actor's
judge dispatch happens to arrive first, and emit the one-shot per-song diagnostic line that closes
design §7.2's first open question (whether the sibling actor list holds both actors at the first
judge dispatch).

This is the second half of Step 4. After it, the FR-5 table holds: a solo player on either
cabinet side ticks their own chart (including solo on the 2P side), a two-player session ticks
P1's chart, and doubles ticks the single actor — chosen once per song, deterministically, not by
dispatch order.

## Background

**Working directory: this repository** (the DDR World hook DLL / modpack).

Step 3 latches the side of whichever actor is dispatched first. That is correct for solo and
doubles (one actor), but in a two-player session the dispatch order of the two actors within a
frame is not guaranteed — child lists are plausibly built by prepend, i.e. reverse order — so
"first dispatched" can latch P2 where FR-5 says P1 wins.

**The sibling walk.** The dispatched actor is the entry point into the game's actor tree:

```
dps   = *(actor + 0x08)        // parent DancePlaySequence
child = *(dps   + 0x18)        // first child
next  = *(child + 0x10)        // sibling chain
match: *(child as **u8) == resolved `gameplay_actor_vtable`   // raw vtable compare
side  = *(i32*)(child + 0x84)
style = *(i32*)(child + 0x88)  // int enum; 1 == DOUBLE
```

The walk is provably valid at the first judge dispatch: the engine itself walks this exact
`+0x18`/`+0x10` chain one call earlier in the same frame (the `0x1045` per-frame broadcast), and
the Results vectors are built in the same call that enters the play state. The
`gameplay_actor_vtable` signature is already resolved by RTTI (`quick_restart_or_fail` consumes
it today); read it through the signature store, and treat "unresolved" as the degraded case below
— it is deliberately NOT added to `required_signatures()`, so the mod still functions (degraded)
if RTTI resolution ever fails.

**Two research corrections this task must not regress on:**

1. **Never dereference `actor+0x88` as a pointer.** It is the play-style `int` (compared against
   `1` by three engine functions). The `ACTOR_SESSION_OFFSET = 0x88` constant in
   `power_user_statistics/data_feed.rs` and the session-struct chain recommended by
   `existing-mechanisms.md` §C4(b) are **wrong** — dead code today, but do not copy them.
2. **Never assume `doubles ⇒ side 0`.** The `autoplay.rs` comment claiming `+0x84` is 0 in
   doubles is unverified; a doubles session started from the P2 card reader may plausibly read 1.
   The style field is what distinguishes doubles from "solo on the P2 side" — that is why it is
   required, not optional.

**Enable state does not exist yet.** The option row is Step 5's; in this task every side is
treated as enabled, so FR-5 reduces to: doubles → the actor; solo → that actor; two actors →
side 0. The plan is explicit that Step 5 "replaces Step 4's 'tick the selected side
unconditionally' with the real gate", so structure the choice so that inserting a per-side
enabled predicate later is an addition, not a rewrite.

**Latch identity moves from side value to actor pointer** (maintainer-approved at decomposition,
2026-07-26, adopting the research's recommended algorithm): latch the **chosen actor pointer**
and compare `actor != latched_actor` per dispatch, keeping the side alongside it for logging and
for Step 5's enable gate. The scene callback remains the primary per-song reset (it already
clears state on GAMEPLAY entry and exit), so allocator address-reuse across a restart cannot
leave a stale actor latch standing.

## Reference Documentation

**Required:**
- Design: `.agents/planning/20260725-assist-tick/design/detailed-design.md` — §2.1 FR-5 (the side
  table); §4.2.1 (the walk, the classification, and the degraded fallback); §7.2 (the diagnostic
  questions this task's log line answers, items 1 and 3)
- `.agents/planning/20260725-assist-tick/implementation/plan.md` — Step 4, item 4

**Additional References (if relevant to this task):**
- `.agents/planning/20260725-assist-tick/research/note-taxonomy-and-actors.md` — §"Actor
  enumeration (what exists today)" (the walk, its offsets, and the engine's own use of the same
  chain), §"Timing of availability" (why the list is complete at the first tick, and the
  diagnostic line's expected shape), §"Side determination" (the two corrections above, and the
  classification table), §"Recommended algorithm" (the full once-per-song shape, including the
  containment check and the critique of dispatch-order latching)
- `src/mods/quick_restart_or_fail.rs` — `find_gameplay_actors` (the in-tree precedent for the
  walk and the vtable compare; note this task walks from `*(actor+0x08)` rather than from the
  TransitionSequence, which removes the `TS+0x58` layout assumption entirely)
- `src/mods/assist_tick.rs` — `rebuild_for` and the latch this task replaces

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. On the rebuild (first judge dispatch of a song), enumerate the live gameplay actors by walking
   the sibling chain from the dispatched actor's parent (`*(actor+0x08)`), matching children by
   raw vtable compare against the `gameplay_actor_vtable` address from the signature store.
   Include the null checks the research shows (`dps` null, vtable unresolved).
2. **Containment check as the validity proof:** if the walked list does not contain the
   dispatched actor itself, distrust the whole walk — fall back to `[dispatched actor]` and warn
   once. This is the cheap end-to-end validation of the chain's layout assumptions.
3. Classify per the research table: any actor with `style == 1` → doubles (expect exactly one
   actor; two actors with doubles style is impossible by construction — treat as 2P and warn
   once); otherwise one actor → solo on its own side; two actors → sort candidates **by the side
   field, never by list position**, and choose side 0 (every side is enabled in this step).
4. Build the tick list from the **chosen** actor's Results vector (which may not be the
   dispatched actor — the walk makes the other side's actor reachable), using the task-01
   predicate unchanged.
5. Latch the chosen actor pointer and its side; the per-dispatch identity check becomes an actor
   pointer compare. The rewind guard, lead, cursor and playback are untouched.
6. **Degraded mode** (walk yields nothing / vtable unresolved / parent null): latch the
   dispatched actor itself, exactly as Step 3 behaves today, with one WARN naming the degradation
   — its only misbehaviour is "in 2P we may follow P2's chart", which is audible-but-benign.
7. Emit the one-shot per-song diagnostic line from the research (design §7.2 items 1 and 3):
   dispatched actor, parent, sibling count, each actor's side and style, the chosen side, the
   Results entry count and kept tick count — one line, once per song, replacing (or folding into)
   the existing build line. Nothing per frame.
8. `required_signatures()` stays `&[]`; `gameplay_actor_vtable` is consumed opportunistically.
9. Panic-free throughout (the walk is raw pointer chasing inside the judge callback's reach);
   bound the sibling walk (e.g. a generous max-iteration cap) so a corrupted `+0x10` chain cannot
   loop forever inside the judge dispatch.
10. No new crate dependency, no config change, no new detour.

## Dependencies

- **Task 01 (eligibility predicate + coalescing)** must be complete — this task rebuilds the list
  through the same narrowed `build_tick_list`
- `gameplay_actor_vtable` — existing RTTI-resolved signature (consumed via the signature store at
  init, stashed in a static; see how `quick_restart_or_fail` stores it)
- `src/services/judge_hook.rs`, `src/services/scene_manager.rs` — unchanged, already wired
- No new crate dependencies

## Implementation Approach

1. Read design §4.2.1 and the research's §"Recommended algorithm" end to end; the shape is fully
   determined, including the containment check and the degraded mode.
2. Stash the `gameplay_actor_vtable` address at `init` (the mod already receives `ctx`); add the
   walk + classification as pure functions returning the candidate list, so the FR-5 choice reads
   as a table.
3. Rework `rebuild_for` to enumerate → classify → choose → build from the chosen actor → latch
   `(actor, side)`; switch `tick_clock`'s identity check to the pointer compare.
4. Build, install, and verify solo behaviour is unchanged (same chart as before: same kept count,
   claps unchanged), and that the diagnostic line reports `siblings=1` with the expected side.
   The 2P / doubles / solo-P2 rows of the matrix are the maintainer's listening passes — the
   diagnostic line is what makes each row checkable in the log afterwards.

## Acceptance Criteria

1. **Solo, either cabinet side**
   - Given a solo session on the P1 side, and separately one on the P2 side
   - When a song is played
   - Then the ticks follow that player's own chart, and the diagnostic line shows `siblings=1`
     with that actor's side (the solo-P2 case is the one dispatch-order latching could never
     prove; the maintainer listens, the log corroborates)

2. **Two players tick P1's chart**
   - Given a two-player session (same difficulty, and separately different difficulties)
   - When a song is played
   - Then one clap stream follows P1's chart regardless of which actor dispatched first, and the
     diagnostic line shows `siblings=2 sides=[0,1]` with side 0 chosen

3. **Doubles ticks the single actor**
   - Given a doubles session
   - When a song is played
   - Then claps cover all eight panels' notes, and the diagnostic line shows `siblings=1` with
     `style=1` (if the cabinet allows starting doubles from the P2 reader, that case settles the
     `+0x84`-in-doubles question — record what the line says)

4. **The walk is validated, and degrades safely**
   - Given the containment check or the vtable resolution failing (if not naturally reachable,
     verified by inspection plus a one-boot forced-failure probe, reverted afterwards)
   - When a song starts
   - Then the mod latches the dispatched actor with exactly one WARN, and behaves exactly as
     Step 3 did

5. **The choice is once per song**
   - Given several songs and a mid-song quick restart
   - When the log is read
   - Then each song (and the restarted song) produced exactly one diagnostic/choice line, and no
     re-evaluation happened mid-song

6. **Solo behaviour is regression-free**
   - Given the same chart used for Step 3's verification
   - When it is played solo
   - Then the kept count and the audible behaviour are identical to task 01's result (the walk
     changed *whose* chart only in multi-actor sessions)

7. **The per-frame path stays O(1)**
   - Given the diff and a full song
   - When reviewed and played
   - Then the walk runs only inside the once-per-song rebuild; the per-dispatch cost is the
     existing lock plus one pointer compare

8. **The build gates pass**
   - Given the completed change
   - When `cargo check --target x86_64-pc-windows-msvc`, then `cargo fmt`, then `./build.sh` are run
   - Then all three complete cleanly

## Metadata
- **Complexity**: Medium
- **Labels**: side-selection, actor-tree, vtable, diagnostics, degraded-mode, hot-path-discipline
- **Required Skills**: Rust with `unsafe` discipline; raw pointer chain walking with validity
  proofs; in-process hook-DLL constraints (panic-free callbacks, bounded loops in game-thread
  paths)
- **Generated By**: code-task-generator 2026-07-26
- **Source Plan**: `.agents/planning/20260725-assist-tick/implementation/plan.md`
- **Plan Step**: Step 4 — Eligibility predicate, side selection, and tick coalescing
