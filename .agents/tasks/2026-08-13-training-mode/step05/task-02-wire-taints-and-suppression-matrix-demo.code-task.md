# Task: Wire the training + assist-tick taints and run the Step-5 suppression-matrix demo

## Description
The producer half of Step 5 (design §4.7, R5): call task-01's
`score_guard::set_training_taint(side)` from every point a training
session alters the current song, and level-write
`score_guard::set_assist_tick_taint(side, on)` from assist_tick's
per-song enable latch (a DELIBERATE behavior change to the shipped mod —
songs played with claps no longer submit scores). Closes plan Step 5
with the server-verified suppression-matrix demo.

## Background
The taint predicate is design §4.1's session-active class plus the
assist-tick latch. The complete, non-redundant producer set:

1. **`bounds::try_resolve_row_bounds`** — where `SESSION_ACTIVE` latches
   for row-derived engagement (`a_ms > 0 || b_ms > 0`) and for the LOOP
   latch: taint the resolution's `side` (the entered side). Both digest
   paths matter for the loop latch (the loop row is not song-scoped);
   the stale-rows path resolves bounds as defaults (no bound taint) but
   a latched loop still grinds ⇒ still taints.
2. **`bounds::set_marker`** — the A/B gestures: taint the pressing
   player's side (`on_input_event` already carries it; thread the side
   into `set_marker` or taint at the call site).
3. **A new `song_reset::on_song_reset` subscriber in the training mod**
   — `t_ms > 0` ⇒ taint the entered side (via `stage_records::
   side_entered`, the pre-shift side-choice fallback pattern; taint BOTH
   entered-unknown sides conservatively... prefer: exactly the sides
   `side_entered` reports true, falling back to both when unavailable).
   This is LOAD-BEARING for restart-from-A: `quick_restart_or_fail::
   trigger_restart` calls `score_guard::reset_song_taint()` at the
   trigger ("honest replays submit") BEFORE seeking to A — the
   subscriber re-taints when the reset actually lands at t > 0. It also
   uniformly covers the silent-start adjust (`adjust_run_to` notifies),
   loop iterations to A > 0, and any future seek path. Loop resets to
   t = 0 carry no taint from this source — the loop latch (producer 1)
   already tainted the song.
4. **assist_tick's GAMEPLAY-entry latch** (`LATCHED_ENABLED`): after the
   per-side latch is taken, `set_assist_tick_taint(side,
   latched_enabled[side])` for both sides — true AND false are written
   every song (level semantics; no reliance on any reset ordering).

Idempotence makes overlap harmless (e.g. a restart-from-A song has
producers 2 and 3 both firing). Triple-5 clearing markers does NOT
untaint — the song already played altered content (R5's "a section
bound engaged, or any seek fired").

NOTE the callback-ordering hazard that shaped this design (do not
"simplify" it away): `reset_song_taint()` fires from quick_restart's
scene callback at every fresh GAMEPLAY entry, and scene callbacks run in
mod-registration order — the training taints are only safe because
producers 1–3 all fire FRAMES after the scene change (resolution retry /
gestures / reset completion), and the assist-tick taint is only safe
because `reset_song_taint` never touches it.

Documentation (per R5's "deliberate behavior change"): update
assist_tick's module doc and the README's assist-tick section to state
that enabling ASSIST TICK now suppresses score submission for that side
(and the card-out save is sanitised), matching autoplay.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-13-training-mode/design/detailed-design.md (§4.7, §4.1 session-active predicate, §7 cabinet checklist / score-suppression matrix)

**Additional References (if relevant to this task):**
- src/mods/training_mode/bounds.rs (the SESSION_ACTIVE latch sites, resolution side, gesture sides)
- src/mods/training_mode/mod.rs (enable(): callback registration pattern — input/scene; add the on_song_reset subscriber beside them, removed in disable())
- src/mods/assist_tick.rs (the GAMEPLAY-entry `LATCHED_ENABLED` latch)
- src/services/stage_records.rs (`side_entered`)
- src/mods/quick_restart_or_fail.rs (`trigger_restart`'s reset_song_taint call — read-only context for the re-taint rationale)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Producers 1–3 in the training mod exactly as in the Background;
   the on_song_reset subscriber is registered at enable() and removed at
   disable() (the shipped callback-handle pattern), and its body is
   panic-free and allocation-light (it runs on the frame thread inside
   the reset completion).
2. Producer 4 in assist_tick beside the existing per-song latch; both
   sides written every gameplay entry.
3. Doc updates: assist_tick module doc + README assist-tick section
   (score-suppression note); training README section if one exists —
   otherwise leave README training coverage to Step 8.
4. No enforcement-path changes: `custom_options_persistence` and the
   score_guard election/sanitisation logic are untouched.
5. Host tests: none beyond task-01's (the producers are engine-facing);
   the full pre-existing suite stays green.
6. Step-5 readiness gates (harness → check → fmt → build) + the cabinet
   demo below.

## Dependencies
- task-01 (the taint setters + predicate extension).
- Steps 1–4 shipped (SESSION_ACTIVE latch sites, on_song_reset with
  nonzero t, the assist-tick latch).

## Implementation Approach
1. Training producers (bounds latch sites + the subscriber in mod.rs).
2. Assist-tick producer + docs.
3. Gates; cabinet + server demo closes plan Step 5.

## Acceptance Criteria

1. **The suppression matrix (the Step-5 demo, server-verified)**
   - Given each of: assist tick alone; bound rows alone (LOOP OFF
     partial-results song included); a mid-song marker gesture alone; a
     restart-from-A; a LOOP ON grind
   - When the song's results pass and the session card-outs
   - Then NO per-stage score reaches the server for the tainted side
     (log: `score_guard: ... savekind=2 save SUPPRESSED`), and the
     card-out save is sanitised (profile persists, scores stripped)
2. **Clean songs still submit**
   - Given an untouched song in the same session (no training rows, no
     gestures, assist tick OFF, autoplay OFF)
   - When its results pass
   - Then its score submits normally (server-verified)
3. **Honest replay after quick-restart stays clean**
   - Given a song with NO training state where triple-1 restarts it
   - When the replay finishes
   - Then the score submits (reset_song_taint's contract is preserved;
     the subscriber only taints t > 0 resets)
4. **Assist-tick taint tracks the played latch**
   - Given ASSIST TICK ON for one song, then OFF for the next
   - When each song's results pass
   - Then only the first song's save is suppressed
5. **Regression**
   - Given autoplay/quick-fail/rate suppression scenarios from the
     shipped matrix
   - When they re-run
   - Then behavior is unchanged

## Metadata
- **Complexity**: Medium
- **Labels**: training-mode, assist-tick, score-guard, engine-facing, cabinet-demo
- **Required Skills**: Rust, the score_guard model, the training mod's latch/driver structure
- **Generated By**: code-task-generator 2026-08-14
- **Source Plan**: .agents/planning/2026-08-13-training-mode/implementation/plan.md
- **Plan Step**: Step 5: Score containment (training + assist-tick taints)
