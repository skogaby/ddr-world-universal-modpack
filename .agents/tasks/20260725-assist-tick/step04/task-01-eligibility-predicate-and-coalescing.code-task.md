# Task: Eligibility Predicate and Tick Coalescing

## Description

Replace Step 3's deliberately over-permissive "every note with a non-negative timestamp" list
build with the real FR-2 eligibility predicate, and add FR-3's coalescing window. After this task
the tick list contains exactly the rows a player is expected to step on — taps, jumps (one tick
per row) and freeze heads — and nothing else.

This is the first half of Step 4, split from side selection because the two halves have
independent audible failure modes: a wrong predicate claps on the wrong *notes*; a wrong side
selection claps on the wrong *chart*. Implemented and verified separately, a listening mistake is
attributable.

## Background

**Working directory: this repository** (the DDR World hook DLL / modpack).

Step 3 (`src/mods/assist_tick.rs`) builds the tick list in `build_tick_list` by walking the
dispatched actor's Results vector via `types::game_note::{actor_results_range, for_each_result}`
and keeping every note with `music_count >= 0`. The Results vector is the whole chart for that
side and **includes** shock arrows, freeze tails, THINOUT notes and this DLL's own injected mines
— so this task's predicate must *reject*, never assume.

The predicate is fully specified by the research (it was read off the game's own classifiers, and
the shock test is the engine's own discriminator verbatim):

```
kind == 0                                             // vanilla step rows ONLY (whitelist)
&& !(state[0..=3] all == 1 || state[4..=7] all == 1)  // not a shock arrow
&& state[] has at least one non-zero entry            // note exists post-trim
&& music_count >= 0                                   // not a pre-chart auto-credited note
```

The `kind == 0` whitelist is what excludes freeze tails (`kind == 2`), THINOUT/modifier-suppressed
notes (`kind == 1`), tempo/event markers (`kind < 0`), and every mod-injected kind present and
future (`MINE == 20`, plus whatever `NoteTypeRegistry` adds next). All the constants are already
public in `types::game_note` (`kind`, `state`).

**The trap this task must document rather than fall into:** `length[]` (per-panel freeze length,
`+0x3C`) is deliberately **not** consulted. A freeze head is an ordinary `kind == 0` tap the
player steps on; the tail is already excluded by kind. Reading `length[]` would break under the
`FREEZE ARROW: OFF` player modifier, which zeroes that array while leaving the steppable head in
place. The reasoning belongs in a comment at the call site, because "improving" the predicate by
reading `length[]` is exactly the tempting mistake.

**Coalescing (FR-3).** Jumps are one record and therefore one timestamp, and exact de-duplication
already exists — but charts authored at TPS 150 round to whole milliseconds and can place two
adjacent rows on the same or *adjacent* millisecond. Timestamps closer together than a named
`COALESCE_MS` (default 4 ms) collapse to one tick. Step 6's diagnostic pass re-measures the real
window needed on TPS-150 charts (design §7.2 question 4), so the constant carries a comment saying
it is provisional.

## Reference Documentation

**Required:**
- Design: `.agents/planning/20260725-assist-tick/design/detailed-design.md` — §2.1 FR-2 (the
  predicate, the exclusion table, and the `length[]` reasoning) and FR-3 (coalescing); §4.2.2 (the
  list build this narrows); §5.1 (the note-record offsets)
- `.agents/planning/20260725-assist-tick/implementation/plan.md` — Step 4, items 1 and 2

**Additional References (if relevant to this task):**
- `.agents/planning/20260725-assist-tick/research/note-taxonomy-and-actors.md` — §"The tick
  predicate" is a complete reference implementation with the per-branch rationale and the
  engine-address provenance of each test; §"Note-record kinds", §"`state[]` values" and
  §"Results-vector coverage" are the authority for what the constants mean; §"Failure modes if a
  claim is wrong" shows why every branch is crash-safe (pure reads of the 0x60-byte record)
- `src/types/game_note.rs` — the hoisted note-record layout and constants the predicate consumes
- `src/mods/assist_tick.rs` — `build_tick_list`, the function this task narrows

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. Implement the FR-2 predicate as **one small pure function** taking a note pointer, in
   `src/mods/assist_tick.rs`, following the research's reference implementation: the `kind == 0`
   whitelist, the engine's own shock test (all four of `state[0..=3]` == `state::TRG`, OR all four
   of `state[4..=7]` — an OR over the two panel groups, never consulting the actor's side, which
   is what makes it correct for 1P-side, 2P-side and doubles alike), the any-non-zero-panel
   invariant guard (`!= 0`, not `== 1` — matches what the renderer draws), and the
   `music_count >= 0` cutoff.
2. Document at the predicate (or its call site) why `length[]` is not consulted — the
   `FREEZE ARROW: OFF` reasoning above, per the plan's explicit instruction.
3. Apply the predicate inside the existing `build_tick_list` walk; keep the existing sort and
   exact dedup, then coalesce: any timestamp within `COALESCE_MS` of the previously kept one
   collapses into it (keep the earlier). `COALESCE_MS = 4`, one named constant, commented as
   provisional pending Step 6's TPS-150 measurement.
4. Extend the once-per-song build line with per-reason rejection counts so the behavior matrix is
   log-assisted: how many entries were rejected by kind, by the shock test, by the no-live-panel
   guard, by the negative-timestamp cutoff, and how many timestamps the coalescing pass merged.
   Counting is once per song inside the existing build — nothing per frame.
5. The predicate runs during the once-per-song build only. The per-frame path is untouched.
6. Panic-free: the predicate is pure reads of the `#[repr(C)]` record through an already
   null-checked pointer (the walk helper skips null note pointers); no indexing that can panic —
   the `state` array is a fixed `[i32; 8]` field, so field access and iteration are fine.
7. No behaviour outside the list build changes: cursor, lead, offset, playback, scene wiring and
   bank registration are all untouched by this task.
8. No new crate dependency, no config change, no new detour.

## Dependencies

- **Step 3 (complete)** — `src/mods/assist_tick.rs`'s `build_tick_list` and its once-per-song
  logging are the surface this task modifies
- `src/types/game_note.rs` — `GameNote`, `kind`, `state` constants (hoisted in Step 3 task 01)
- No new crate dependencies

## Implementation Approach

1. Read the research's §"The tick predicate" reference implementation and FR-2's table end to
   end; the predicate is fully determined — the only writing is transcription plus the doc
   comments.
2. Add the predicate function and the rejection counters; thread them through `build_tick_list`.
3. Add the coalescing pass after sort+dedup, and the merged-count to the log line.
4. Build, install, play the same chart as Step 3's verification (Ace out, Challenge 10) and
   compare the kept count against Step 3's `kept=437`: it must **drop** (freeze tails and any
   shock/THINOUT content leave the list), and the rejection counts must account exactly for
   `results - kept - coalesced`.

## Acceptance Criteria

1. **Taps, jumps and freeze heads tick; one clap per row**
   - Given a chart with jumps and freezes
   - When it is played
   - Then every tap and freeze head produces exactly one clap per row (jumps are not doubled),
     and freeze **tails** produce none (maintainer's listening pass)

2. **Shock arrows do not tick**
   - Given a chart with shock arrows
   - When it is played
   - Then shocks produce no clap, and the per-song line's shock-rejection count matches the
     chart's shock count (maintainer's chart; log cross-check is the agent's)

3. **Mod-injected mines do not tick**
   - Given a chart with this repository's injected mines
   - When it is played
   - Then mines produce no clap (they are `kind == 20`, rejected by the whitelist), and mine
     rendering/judging is unaffected

4. **CUT / JUMP-OFF removed notes do not tick**
   - Given the CUT or JUMP-OFF modifier active
   - When the song is played
   - Then removed (THINOUT, `kind == 1`) notes produce no clap

5. **`FREEZE ARROW: OFF` keeps heads ticking**
   - Given the `FREEZE ARROW: OFF` player modifier
   - When a freeze-heavy chart is played
   - Then freeze heads still clap — the predicate never consulted `length[]`

6. **The counts reconcile**
   - Given any song
   - When the per-song build line is read from `log.txt`
   - Then `results == kept + rejected_by_kind + rejected_shock + rejected_no_panel +
     rejected_negative + coalesced_away` (exact arithmetic, no unexplained entries), and the kept
     count is lower than Step 3's over-permissive count on the same chart

7. **The per-frame path is untouched**
   - Given the diff
   - When it is reviewed
   - Then only the list build and its logging changed; cursor/lead/offset/playback are identical

8. **The build gates pass**
   - Given the completed change
   - When `cargo check --target x86_64-pc-windows-msvc`, then `cargo fmt`, then `./build.sh` are run
   - Then all three complete cleanly

## Metadata
- **Complexity**: Low
- **Labels**: predicate, note-records, coalescing, diagnostics, no-hot-path-change
- **Required Skills**: Rust; reading reverse-engineered structure semantics faithfully; the
  restraint to transcribe a fully-specified predicate rather than "improve" it
- **Generated By**: code-task-generator 2026-07-26
- **Source Plan**: `.agents/planning/20260725-assist-tick/implementation/plan.md`
- **Plan Step**: Step 4 — Eligibility predicate, side selection, and tick coalescing
