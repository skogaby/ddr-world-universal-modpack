# Task: Option Row, Per-Side Enable Gate, and Lifecycle

## Description

Make assist tick player-facing: a per-player `ASSIST TICK` OFF/ON row on the MODS tab of the
game's own options screen, persisted with the card, gating the tick stream per FR-5's full table
and FR-8's latch semantics. Replaces Step 4's "every side is enabled" placeholder with the real
gate.

**Verification is deliberately light in this task** (maintainer's call, 2026-07-27): implement
the code; the full Step 5 behaviour list (persistence, mid-session toggle semantics, runtime
disable, FR-10 score upload, label rendering) is the maintainer's manual pass at the end of the
step. The agent's share is the build gates plus one boot-sanity check.

## Background

**Working directory: this repository.**

The custom-options framework does nearly everything: `RegisterSpec::bool_toggle("assist_tick")`
IS the two-value OFF/ON row with stock ribbons, and the builder default is `PersistMode::Full`
(network save + load + offline JSON cache = follows the card). What the mod owns:

- **`on_change` writes per-side atomics only.** It fires on the registering mod's `enable()`
  thread, the render thread, the ess save/load hook's thread, and a spawned JSON-prime background
  thread — it must be thread-agnostic, and a panic inside it permanently no-ops the callback.
- **`Duplicate` on re-registration is success** (there is no unregister API), and a re-enabling
  mod must reseed its atomics from the registry since the duplicate path does not re-fire
  `on_change` (autoplay/playfield_styling precedent).
- **FR-8 latch:** per-side enables are read into latched atomics at GAMEPLAY entry (the scene
  callback the mod already has); mid-session changes apply from the next song. The judge path
  never calls `get_value` (mutex on a hot path).
- **FR-5, completed:** `choose_actor` filters candidates by the latched enables — solo → that
  actor iff enabled; 2P both → side 0; 2P one → that side; doubles → the actor iff its own
  side (`+0x84`) enabled; none enabled → the song is inert and **no tick list is built at all**.
- **Degraded-mode refinement** (approved at decomposition, from the research's critique): when
  the sibling walk is unavailable AND the dispatched actor's side is disabled, do **not** consume
  the rebuild flag — return and leave it armed, so the other side's actor can claim the latch on
  its own dispatch. Converts "P1 disabled, P2 enabled" from silence into correct behaviour
  without frame delimiters.
- **FR-10:** no score-guard wiring — assisted plays upload normally. This is a deliberate,
  recorded design decision (design §2.1 FR-10); do not mirror autoplay's taint.

## Reference Documentation

**Required:**
- Design: `.agents/planning/20260725-assist-tick/design/detailed-design.md` — §4.2 ("Option row"
  and the two framework behaviours to respect), §2.1 FR-5/FR-7/FR-8/FR-10
- `.agents/planning/20260725-assist-tick/implementation/plan.md` — Step 5, items 2–4

**Additional References (if relevant to this task):**
- `.agents/planning/20260725-assist-tick/research/existing-mechanisms.md` — §A1 (a copy-pasteable
  assist-tick `bool_toggle` spec), §A5 (on_change thread rules), §A3 (why `Full`), §B1
  (Duplicate-as-success + reseed), §B2 (the gameplay-entry latch shape)
- `.agents/planning/20260725-assist-tick/research/note-taxonomy-and-actors.md` — §"Recommended
  algorithm" step 3 (enabled-filtered choice) and the degraded-mode refinement bullet
- `src/mods/assist_tick.rs` — `choose_actor`, `rebuild_for`, `tick_clock`, the scene callback

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. Register the row in `enable()` when `custom_options::is_available()`:
   `RegisterSpec::bool_toggle("assist_tick").default_value(0).on_change(...)` — builder-default
   persistence (`Full`). `Duplicate` = success; on that path reseed the per-side atomics from
   `custom_options::get_value`. Registration failure = warn, mod stays functional-but-gateless?
   No — without a row no side can ever enable, so warn and leave both enables OFF (silent mod),
   matching "graceful degradation" (the row is the only enable source; default OFF).
2. `on_change(side, value)`: bounds-check the side, store `value != 0` into
   `ASSIST_TICK_ENABLED[side]`. Nothing else — no logging beyond `log_debug!`-tier, no game
   memory, no re-entry into custom_options.
3. At GAMEPLAY entry (existing scene callback): copy each side's current enable into
   `LATCHED_ENABLED[side]` atomics before arming the rebuild. On exit, latches may stay (they are
   re-written on every entry).
4. `choose_actor` consumes the latched enables per FR-5's full table (doubles gate = the actor's
   own `+0x84` side). No enabled candidate → inert song: `rebuild_for` sets the inert marker
   without walking the Results vector, and logs one line saying the song is inert because no
   participating side has the option on.
5. Degraded-mode refinement: in `tick_clock`'s rebuild branch, when enumeration will be degraded
   and the dispatched actor's side is not latched-enabled, leave `rebuild_pending` set and
   return. (Shape this so the normal path is unchanged — the check only bites in degraded mode.)
6. `disable()`: existing cleanup plus reset both enable/latch atomic pairs. Banks stay (already
   the case). The option row cannot be unregistered — expected.
7. Panic-free everywhere reachable from callbacks; no `get_value` on the judge path; no new crate
   dependency; no new detour; no score_guard reference.

## Dependencies

- **Task 01 (label asset)** — should land first so the first boot with the row registers its
  texture
- `src/services/custom_options/` — existing framework, already initialized before mods
- No new crate dependencies

## Implementation Approach

1. Read §A1/§A5/§B1/§B2 of `existing-mechanisms.md`; the wiring is fully precedented.
2. Add the two atomic pairs + `on_change` + registration in `enable()` (after the judge/scene
   wiring succeeds), reseed-on-duplicate.
3. Latch in the scene callback; thread the enables through `choose_actor` and the inert path;
   add the degraded-mode refinement.
4. Gates, install, one boot-sanity check (row registered in the log; a solo song with the option
   ON — via the framework's default-OFF meaning the agent flips it through the native options
   overlay via the nav harness, or simply verifies the OFF case: no list built). Everything else
   is the maintainer's end-of-step manual pass.

## Acceptance Criteria

1. **The row registers**
   - Given the built DLL installed
   - When the game boots
   - Then the log shows the option registered on the MODS tab and the mod enabled, no warnings

2. **OFF is genuinely inert**
   - Given both sides OFF (the default)
   - When a song is played
   - Then no tick list is built (the inert log line appears; no `song build` line), and nothing
     plays

3. **The gate completes FR-5**
   - Given the maintainer's end-of-step manual pass (persistence across card-out/in and relaunch,
     mid-session toggle → next song, 2P one-side-enabled → that side, runtime disable, FR-10
     upload, label after one relaunch)
   - Then behaviour matches the design; deviations come back as findings

4. **The build gates pass**
   - Given the completed change
   - When `cargo check --target x86_64-pc-windows-msvc`, then `cargo fmt`, then `./build.sh` are run
   - Then all three complete cleanly

## Metadata
- **Complexity**: Medium
- **Labels**: custom-options, persistence, latch, lifecycle, per-player
- **Required Skills**: Rust; the custom-options framework's threading rules; restraint about
  hot-path reads
- **Generated By**: code-task-generator 2026-07-27
- **Source Plan**: `.agents/planning/20260725-assist-tick/implementation/plan.md`
- **Plan Step**: Step 5 — Option row, latching, and lifecycle
