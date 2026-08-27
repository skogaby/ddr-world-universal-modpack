# Task: LOOP SONG row + LOOP OFF early natural end

## Description
The player-facing half of Step 4 (design §4.1/§4.2): the
`training_loop_song` bool row ("LOOP SONG", OFF/ON, default OFF,
`PersistMode::Session`) with its textures; the entered-side loop latch at
resolution; the session-active predicate extended to loop-driven sessions;
and the LOOP OFF wiring — when a section end exists, the
ControlMessageActor end thresholds are written to `b_ms` (raw + converted
display) so the game runs its OWN stock tail early (banner → results with
the partial play's stats), with the stock values stashed and restored if
the section end clears mid-song.

## Background
Row model: task-01 of Step 3 (`bool_toggle` + `.persist_mode(Session)` +
the registration/availability/atomics-seeding shape in
`src/mods/training_mode/mod.rs::register_bound_rows`). The loop row is a
PLAIN Session row per the approved breakdown decision #3 — NOT
song-scoped (grind mode survives song switches within the session; the
card-in reset from Step 3 restores OFF for the next player). It therefore
does NOT participate in the highlight seeder or the digest stamp.

LOOP OFF semantics (design §4.2): with a resolved/gesture section end
`b_ms` (live `B_MS` > 0), write CMA `+0x98 = b_ms` and
`+0x94 = display_for_raw(notes, b_ms)` via task-01's surface. The game's
one-way cascade then fires `0x104A`/`0x104B` at the truncated times and
runs the stock end (research §4.4 — no scene surgery). Write points:
resolution completion (the driver already retries it), a gesture B-set,
and a triple-5 restore; a B that clears to none restores the STASHED
stock values. A gesture-set B behind the current position ends the song
on the next frame's `0x1045` — accepted "end here" semantics (design
§4.2). LOOP ON must NEVER write thresholds (task-01's end policy is the
single decision point); LOOP toggling is unreachable mid-song (the
options modal is select-only), so the loop state latches once per song at
resolution, entered side, alongside the bounds.

Ladder (design §6): converter returns `None` (degenerate note vector) or
the threshold write refuses ⇒ ONE WARN, no writes, the song plays to its
natural end — the section end is ignored, never half-applied. NOTE
(plan-approved ordering): score containment is Step 5 — a LOOP OFF
truncated play's partial result WILL submit until then (research §4.4
notes the record undercounts; flag in the demo notes).

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-13-training-mode/design/detailed-design.md (§4.1 row table + session-active predicate, §4.2 LOOP OFF wiring, §6 ladder)

**Additional References (if relevant to this task):**
- docs/training_mode_research.md §4.1 (threshold semantics/domains), §4.4 (early natural end + the partial-stats caveat)
- src/mods/training_mode/mod.rs (row registration model, resolution-adjacent latches)
- src/mods/training_mode/bounds.rs (`try_resolve_row_bounds`, `restore_row_bounds`, `set_marker`, `SESSION_ACTIVE`, `clear_session_state`)
- scripts/gen_option_labels.py (label + on/off preview textures)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Row `training_loop_song`: `RegisterSpec::bool_toggle` + Session +
   default OFF, registered beside the bound rows (same
   availability/degradation shape; label "LOOP SONG"); per-side value
   atomics + accessor mirroring the bound rows. Textures:
   `seop_item_training_loop_song` + `seop_image_training_loop_song_{off,on}`
   via `scripts/gen_option_labels.py` (generated in-repo; deploying to the
   cabinet's `data_mods/` is a maintainer step — flag at demo handoff).
2. Loop latch: at resolution (`try_resolve_row_bounds`), latch the entered
   side's loop value for the song (a per-song atomic beside the bounds;
   cleared by `clear_session_state`). Session-active predicate (design
   §4.1) gains "loop ON": the latch sets `SESSION_ACTIVE`, and the
   GAMEPLAY-entry `rows_engaged`/driver arming treats loop-ON as engaged
   (the driver must run for loop sessions even with no bounds set —
   breakdown decision #2: loop-ON alone loops the whole song, task-03).
3. LOOP OFF threshold application (a `bounds.rs`-owned apply function,
   driven from the resolution path and the gesture arms): end policy from
   task-01 decides; `WriteThresholds` ⇒ stash the stock
   `chart_end_thresholds` once per song, then
   `set_chart_end_thresholds(display_for_raw(notes, b), b)`. Re-applied on
   gesture B-set and triple-5 restore (new b ⇒ new write; b cleared to
   none ⇒ restore the stash). Track applied-state per song
   (design §5's `thresholds_written` class) so restore/rewrite is
   idempotent and `clear_session_state` resets it.
4. Ladder: converter `None` / notes unavailable / write refused ⇒ one WARN
   per song, thresholds untouched, natural end (the run itself is never
   degraded). LOOP ON ⇒ the apply function is never entered (policy
   exclusivity).
5. Host tests where pure logic permits (the latch/predicate composition is
   thin — the policy exclusivity and converters are task-01's tests; add
   any apply-state transition table that factors purely). The threshold
   write/restore behavior is cabinet-validated (task-03's demo).

## Dependencies
- task-01 (converters, end policy, threshold surface).
- Step 3 shipped (Session rows, resolution, restore semantics, driver).

## Implementation Approach
1. Row + textures + latch + predicate extension (the Step-3 row model).
2. The apply/stash/restore state in bounds.rs, wired into resolution +
   gesture arms behind the end policy.
3. Gates; the on-cabinet validation folds into task-03's step demo.

## Acceptance Criteria

1. **Row registered and session-scoped**
   - Given the mod enabled with textures deployed
   - When the MODS tab is opened
   - Then LOOP SONG renders OFF/ON (default OFF), survives song switches
     within a session, and resets to OFF at card-in
2. **LOOP OFF early natural end**
   - Given LOOP OFF and SONG END TIME below the song's length
   - When the run reaches the truncated end
   - Then the game runs its stock tail (banner → results, partial stats)
     at the section end — no scene surgery, no hang
3. **Restore on clear**
   - Given written thresholds and a triple-5 that restores to no section end
   - When the bounds clear
   - Then the stock thresholds are restored and the song plays to its
     natural end
4. **LOOP ON exclusivity**
   - Given LOOP ON latched for the song
   - When the resolution and gesture paths run
   - Then the thresholds are NEVER written (the loop driver owns the end)
5. **Ladder**
   - Given a degenerate note vector or an unresolvable CMA
   - When the apply path runs
   - Then one WARN fires, nothing is written, and the song plays to its
     natural end

## Metadata
- **Complexity**: Medium
- **Labels**: training-mode, custom-options, bounds, engine-facing
- **Required Skills**: Rust, the Step-3 row/latch model, the CMA end chain
- **Generated By**: code-task-generator 2026-08-14
- **Source Plan**: .agents/planning/2026-08-13-training-mode/implementation/plan.md
- **Plan Step**: Step 4: LOOP SONG — loop driver + early natural end
