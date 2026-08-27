# Task: Driver skeleton + R15 silent skip-first start

## Description
The R15 requirement made real: with SKIP FIRST set at song select, the
song starts in SILENCE with the section's notes making a natural
approach, and the music enters exactly at A — the true beginning is
never audible (never even decoded into the output). Mechanism (design
§4.3): the binding is created ALREADY SHIFTED via Step 1's bind-time
pre-shift, the natural start sequence runs untouched, and a new
`src/mods/training_mode/driver.rs` (the per-frame driver skeleton later
steps extend) detects the first anchored frame and fires ONE synchronous
adjust — Step 2's anchor + rebuild-at-A + freeze-neutralization block
WITHOUT any cue stop/replay. Closes Step 3 with the cabinet demo.

## Background
Timeline: rows are set at scene 25; the bank create (scene 26 load)
consumes `runtime::initial_content_mapping_ms` (Step 1's
`BindContext.initial_mapping_ms`, converted at bind via
`Binding::ms_to_blocks` — the MAIN entry's SERVED-stream grid, so the
values passed are WALL-domain ms; at 100 % — the common training case —
wall == content, and at a non-identity desired rate the mod must convert
`content→wall` before setting, mirroring the Step-2 seek's composition.
Verify `ms_to_blocks`' grid during Explore and pin the domain with a
comment). The game then runs its stock start: READY panel, cue play, DPS
state 6's own `0x1044 {now}` — anchored at content 0 with playhead-0
records, while the audio serves `lead` silence then content from A. The
driver detects "first anchored frame" (every GamePlayActor at step 4 AND
anchor `+0x160` nonzero) and fires the adjust: broadcast
`0x1044 {now − wall(A) + lead_wall_ms}` + the rebuild trio at `a_q` +
neutralization writes — NO stop/replay, NO accumulator zeroing (the run
just started; accumulators are already zero). The 1–2 frame window
between the game's anchor and ours is silent and judge-inert (pre-A
notes become consumed-neutral in the rebuild; research §4.3/design
§4.3). Degraded path (design §6): pre-shift missed but binding live ⇒
fall back to a stop/replay seek (`request_reset(a_ms, TRAINING_LEAD_MS,
Zero, …)`) at the first gate frame — brief true-beginning audibility,
one WARN; no binding at all ⇒ no silent start, song plays normally.

The adjust primitive belongs in `song_reset` (it owns the anchor/trio/
neutralization machinery): extract Step 2's `perform_seek` core into a
shared block and expose e.g. `pub fn adjust_run_to(t_ms, lead_wall_ms)
-> bool` — gates (live DPS in-song, actors step 4, trio available,
range-validated inputs) + the anchor broadcast + trio + writes +
`notify_subscribers(t_q)`, WITHOUT the cue transaction and WITHOUT the
accumulator/gauge block. No locks across engine calls; the driver is a
render-thread self-requeueing callback (the shipped probe/driver
pattern), generation-tokened per song.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-13-training-mode/design/detailed-design.md (§4.3 skip-first + approach lead + driver, §6 error ladder rows "Pre-shift missed" and "Identity binding refused")

**Additional References (if relevant to this task):**
- docs/training_mode_research.md §5.4 (transaction order — the adjust is its tail), §6 (anchor math), §7 (0x1044 subscribers all-clear)
- src/services/song_reset/mod.rs (`perform_seek` — the block being factored; `seek::anchor_tick`)
- src/services/song_rate/runtime.rs (`set_initial_content_mapping_ms`, `initial_content_mapping_ms`)
- src/mods/training_mode/mod.rs + bounds.rs (Step-2/task-02 surfaces this consumes)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Pre-shift arming: while the mod is active, keep
   `set_initial_content_mapping_ms(wall(a_row_ms), TRAINING_LEAD_MS)`
   current with the resolved row-derived skip for the upcoming song
   (set/refreshed at scene 25/26 boundaries from task-02's effective
   values; cleared — `(0, 0)` — when skip is 0, the mod disables, or the
   session is ineligible). Wall conversion per the Background note.
2. `driver.rs`: render-thread self-requeueing callback armed at gameplay
   entry when the session is training-active AND a pre-shift was set;
   generation-tokened (scene exit / new song supersedes). Detects the
   first anchored frame (all actors step 4, `+0x160` ≠ 0, range-checked
   reads) and fires the adjust ONCE per song.
3. `song_reset::adjust_run_to(t_ms, lead_wall_ms) -> bool` (name
   flexible): the factored anchor+trio+neutralization block from
   `perform_seek`, gated and fail-closed exactly like the seek's
   completion (pre-validated side plans; false ⇒ nothing mutated),
   ending in `notify_subscribers(t_q)`. `perform_seek` itself must stay
   behavior-identical (shared-block refactor, no semantic change).
4. Fallback ladder (design §6): adjust gates fail OR the pre-shift was
   not applied to the live binding (compare `active_content_mapping()`
   against the expectation) ⇒ one WARN + a stop/replay seek via
   `request_reset(a_ms, TRAINING_LEAD_MS, Zero, None)`; that refusing too
   ⇒ song plays normally from 0 (no recovery scene-jump — nothing is
   broken).
5. Zero footprint: skip == 0 ⇒ no pre-shift, driver never arms, nothing
   fires; mod disabled ⇒ bit-for-bit shipped behavior.
6. Host tests where pure logic permits (e.g. the first-anchored-frame
   predicate on synthetic fields, the pre-shift wall conversion);
   the adjust itself is cabinet-validated by the demo.

## Dependencies
- task-01 (rows), task-02 (bound resolution + effective clamp +
  session-active latch).
- Steps 1–2 shipped (bind-time mapping, seek transaction internals).

## Implementation Approach
1. Factor the adjust primitive out of `perform_seek` (behavior-identical
   refactor first — full gates re-run before proceeding).
2. Pre-shift arming from the mod (scene callback), then driver.rs with
   the one-shot adjust + fallback ladder.
3. Readiness gates; cabinet demo closes plan Step 3.

## Acceptance Criteria

1. **Silent skip-first start (the Step-3 demo)**
   - Given SKIP FIRST 60 set at song select (row effective-clamped to the highlighted song's length)
   - When the song starts
   - Then it opens in silence with notes approaching, the music enters exactly at 1:00, the true beginning is never audible, and combo/score/judging behave like a natural start (pre-A notes consumed-neutral, never mass-missed)
2. **Works at rate**
   - Given the same setup at a non-100 % SONG SPEED
   - When the song starts
   - Then the silent start lands at content A with claps/clock aligned (wall-domain pre-shift correct)
3. **Fallback ladder**
   - Given a missed pre-shift (or refused adjust) with a live binding
   - When gameplay starts
   - Then ONE WARN is logged and the stop/replay seek delivers the section start (brief true-beginning audibility accepted); with no binding at all the song simply plays from 0
4. **Zero footprint**
   - Given skip 0 or the mod disabled
   - When songs are played
   - Then no pre-shift is set, the driver never fires, and behavior is bit-for-bit shipped
5. **OMIT LAST visibility**
   - Given OMIT LAST set
   - When gameplay entry resolves the bounds
   - Then `b_ms` appears in the log (consumption arrives in Step 4)

## Metadata
- **Complexity**: High
- **Labels**: training-mode, driver, song-reset, engine-facing, cabinet-demo
- **Required Skills**: Rust, the song_reset transaction model, song_rate mapping lifecycle
- **Generated By**: code-task-generator 2026-08-13
- **Source Plan**: .agents/planning/2026-08-13-training-mode/implementation/plan.md
- **Plan Step**: Step 3: Bound rows, session persistence, silent skip-first start
