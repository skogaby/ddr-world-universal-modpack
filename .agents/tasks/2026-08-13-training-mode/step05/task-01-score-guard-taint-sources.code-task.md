# Task: score_guard training + assist-tick taint sources

## Description
The pure half of Step 5 (design §4.7, R5): two new per-side taint sources
in `score_guard` — TRAINING (a section bound engaged, a marker was set, or
a seek fired this song) and ASSIST TICK (the side played the song with
claps enabled) — OR'd into the existing per-stage suppression predicate,
with the correct per-song / per-session clearing semantics. Zero behavior
change until task-02 wires the producers.

## Background
The enforcement machinery is ALL shipped and stays untouched:
`custom_options_persistence`'s `save_sender` trampoline suppresses
per-stage saves (`savekind == 2`) when `score_guard::is_stage_suppressed
(side)` is true, latches `mark_session_tainted` on every suppression, and
sanitises (score-strips) the card-out logout save via the session-sticky
flag. Step 5 only ADDS taint inputs to that predicate, exactly like the
existing autoplay/quick-fail sources.

Lifecycle facts the clearing semantics must match (all verified in the
shipped code):
- The per-stage save fires at RESULTS, after gameplay — so a per-song
  taint must survive gameplay exit and clear at the NEXT song's start.
  The existing hook: `reset_song_taint()` fires at every fresh GAMEPLAY
  entry (quick_restart_or_fail's scene callback) AND at every quick-
  restart trigger ("an honest replay must be allowed to submit"). The
  TRAINING taint rides it: a triple-1 restart clears the taint, and
  task-02's on_song_reset(t>0) subscriber re-taints if the restart
  actually seeks to a marker (restart-from-A).
- The ASSIST TICK taint must NOT be cleared by `reset_song_taint()`:
  it is level-written (true or false) at every GAMEPLAY entry by
  assist_tick's own per-song latch (task-02), and clearing it from
  quick_restart's scene callback would create a cross-mod callback-
  ORDER dependence (both fire on the same scene change; registration
  order is incidental). Autoplay's taint has the same level-driven
  shape and is likewise untouched by the song reset.
- `reset_session()` (card-in) clears everything for the next player.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-13-training-mode/design/detailed-design.md (§4.7 taint sources, §4.1 session-active predicate — R5)

**Additional References (if relevant to this task):**
- src/services/score_guard.rs (the autoplay/quick-fail source model, `is_stage_suppressed`, `reset_song_taint`, `reset_session`)
- src/services/score_guard_tests.rs (host-test conventions; harness-mounted)
- src/services/custom_options_persistence.rs (the consuming trampoline — read-only context, do not modify)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `TRAINING_TAINT: [AtomicBool; SIDES]` +
   `pub fn set_training_taint(side: usize)` (one-way per song, idempotent,
   out-of-range side ignored) — doc comment naming the producers (bound
   engagement / marker set / seek fired; wired in task-02).
2. `ASSIST_TICK_TAINT: [AtomicBool; SIDES]` +
   `pub fn set_assist_tick_taint(side: usize, on: bool)` (level-written,
   the autoplay model; out-of-range side ignored).
3. `is_stage_suppressed(side)` gains both sources in its OR (after the
   existing terms; out-of-range side still reads not-suppressed).
4. `reset_song_taint()` additionally clears TRAINING (both sides); it
   MUST NOT touch ASSIST_TICK (see Background). `reset_session()` clears
   both new sources (both sides).
5. Host tests (score_guard_tests.rs, harness-mounted): each new source
   alone suppresses its side only; both compose with the existing
   sources; `reset_song_taint` clears training but not assist-tick;
   `reset_session` clears both; out-of-range sides are no-ops. Any test
   that mutates the PROCESS-GLOBAL statics must restore them (or the
   suite must stay order-independent — follow the existing tests'
   discipline).
6. Zero behavior change: no callers added; the full pre-existing suite
   stays green.

## Dependencies
- None new — pure additions to the shipped score_guard.

## Implementation Approach
1. TDD in the harness: suppression-matrix + reset-semantics tests first.
2. Statics + setters + predicate/reset extensions in score_guard.rs.
3. Full gates (harness → check → fmt).

## Acceptance Criteria

1. **Training taint suppresses its side**
   - Given `set_training_taint(0)` and no other taints
   - When `is_stage_suppressed` is evaluated
   - Then side 0 reads suppressed and side 1 reads clean
2. **Assist-tick taint is level-driven**
   - Given `set_assist_tick_taint(0, true)` then `(0, false)`
   - When `is_stage_suppressed(0)` is evaluated after each
   - Then it reads suppressed, then clean
3. **Per-song vs per-session clearing**
   - Given both new taints set on side 0
   - When `reset_song_taint()` runs
   - Then the training taint is cleared and the assist-tick taint is NOT;
     and when `reset_session()` runs, both are cleared
4. **Existing behavior unchanged**
   - Given the existing suites
   - When the harness runs
   - Then every pre-existing test passes unchanged

## Metadata
- **Complexity**: Low
- **Labels**: training-mode, score-guard, host-tested
- **Required Skills**: Rust, the score_guard taint/suppression model
- **Generated By**: code-task-generator 2026-08-14
- **Source Plan**: .agents/planning/2026-08-13-training-mode/implementation/plan.md
- **Plan Step**: Step 5: Score containment (training + assist-tick taints)
