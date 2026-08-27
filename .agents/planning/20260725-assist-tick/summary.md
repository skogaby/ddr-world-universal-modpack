# Summary — Assist Tick

**Date:** 2026-07-25
**Status:** PDD complete — design and plan both Approved

## What this is

A feature spec (Prompt-Driven Development) for **Assist Tick**: a mod that plays a short clap at
the exact moment each arrow in the chart is meant to be stepped on, as an audible timing
reference. The DDR World equivalent of StepMania's assist tick, toggled per player from the
in-game MODS options tab.

## Artifacts created

```
.agents/planning/20260725-assist-tick/
├── rough-idea.md                          # the original request + the source sample's properties
├── idea-honing.md                         # decision register: 18 decisions, all Accepted/Assumed
├── research/
│   ├── orientation.md                     # blind-spot pass; what the idea has to live in
│   ├── game-sound-engine.md               # RE of the game's XACT audio subsystem
│   ├── audio-output-feasibility.md        # the self-hosted-audio alternative, investigated + rejected
│   ├── existing-mechanisms.md             # codebase API survey (options, judge hook, actors, config)
│   ├── bank-slot-and-anchors.md           # slot-4 lifetime proof + cross-build signature verification
│   ├── xact-bank-format.md                # XWB/XSB format, engine validator rules, generation recipe
│   └── note-taxonomy-and-actors.md        # note-kind taxonomy, shock test, actor enumeration
├── design/detailed-design.md              # Approved 2026-07-25 — self-contained
├── implementation/plan.md                 # Approved 2026-07-25 — 6 steps + checklist
└── summary.md                             # this file
```

## Design in one paragraph

The clap plays through the game's **own audio engine** (Microsoft XACT 2 — the same engine and the
same final mix that carries the song audio, so the tick inherits the music's exact output
latency). A new `services::game_audio` creates a mod-owned wave bank + sound bank at runtime and
parks the sound bank in a **free slot on the game's audio manager**, leaving that slot's file id
at `-1` — which is what makes it invisible to the only code in the game that destroys bank slots,
and therefore survives every song load for the process lifetime. A new `mods::assist_tick` owns
policy: a per-player OFF/ON row on the MODS tab, per-side latching at gameplay entry, tick-side
selection from the live actor tree, and a per-frame cursor over a sorted list of note timestamps
built once per song from the actor's Results vector. Triggering is **chart-time driven, not
judgment driven**, so ticks stay on the beat through missed notes. The clap ships as a pre-built
XACT bank pair under `data_mods/assist_tick/`, generated offline and swappable without a rebuild.

## Decisions worth remembering

- **Chart time, not judgment.** The rough idea assumed the judgment hooks were the trigger point;
  they aren't — judgment fires on player input, so a judgment-driven clap would track the player's
  mistakes rather than the beat. The judge hook is used purely as a per-frame clock.
- **One centre-panned stream.** An agent proposal for per-side panned streams was overridden: an
  arcade cabinet is a single shared mix in one room, so panning buys no player isolation and two
  streams would just be noise. Solo side wins regardless of cabinet side; 2P with both enabled
  follows P1.
- **`length[]` is not part of the eligibility test.** A freeze head is an ordinary tap; reading the
  freeze-length array would break under the `FREEZE ARROW: OFF` modifier, which zeroes it while
  leaving the steppable head.
- **No config section, no volume control, no score taint** — all three deliberately deferred.

## Implementation plan (6 steps)

1. Generate and commit the clap bank pair (includes the one cross-repo prerequisite).
2. `services::game_audio` — signatures, bank registration, cue playback. **Audible clap here.**
3. `mods::assist_tick` — end-to-end ticking, deliberately over-permissive.
4. Eligibility predicate, side selection, coalescing, one-tick-per-frame.
5. Option row, latching, persistence.
6. Diagnostic pass, NFR audit, docs, final gates.

Risk is front-loaded: the design's two biggest unknowns — whether our bank registers and plays,
and whether frame quantization sounds tight enough — both surface in Steps 2 and 3, before any
player-facing code exists.

## Next steps

1. Run the **code-task-generator** sop against `implementation/plan.md` to produce task files,
   one implementation step at a time.
2. Run **code-assist** on each task in order.
3. Keep a `progress.md` in this directory current throughout implementation (repo convention) —
   it is the cold-resume point after a context reset.

## Dependencies and prerequisites

- **Cross-repo:** `ddr-chart-tools` needs an additive SE-profile entry point on its XSB writer
  (mix category 6, bare sound entry, no runtime-parameter curve) so the tick rides the SE bus
  rather than the music bus. This gates Step 1, which gates everything else.
- **Local install:** the `data_mods/assist_tick/` tree must be copied into the CrossOver install
  once (`$DDR_WORLD_INSTALL/data_mods/`) before ticks are audible.

## Verification split

The maintainer runs all gameplay verification against the local CrossOver install, using their own
chart selection for the note-type cases. The agent's share is offline asset validation, build
gates, reading `log.txt`, and the negative paths observable purely in the log.

## Assumptions and areas that may need refinement

1. **SE mute filter.** Playback is assumed not to be vetoed during gameplay for our bank id —
   strongly evidenced (the game's own shock-arrow SE passes the same filter on a different bank and
   is audible) but not proven until Step 2 runs. Documented mitigation: switch to the inner play
   entry point, which skips the filter.
2. **Mix bus.** Whether the tick genuinely follows the operator's SE volume is confirmed by one
   matrix row in Step 5; if it doesn't, the sound-bank category is the thing to revisit.
3. **Frame quantization.** ~±8 ms with adaptive half-frame lead centring. If it sounds loose, the
   escalation ladder is in the design: verify at higher frame rates first, then the engine's
   scheduled-start semantics, then sub-frame placement.
4. **Four questions deferred to a diagnostic build** (Step 6), none of them design-blocking:
   sibling-actor list completeness at the first judge dispatch; whether anything produces
   per-panel state above 1; doubles started from the P2 card reader; the real coalescing window on
   TPS-150 charts.
5. **Sample provenance.** The StepMania clap is committed to this repository on the assumption that
   this is acceptable for a private-cabinet modpack, with attribution in the README.

## Incidental fix made during this session

`src/mods/power_user_statistics/data_feed.rs` — deleted seven dead offset constants, one of which
(`ACTOR_SESSION_OFFSET = 0x88`) documented `GamePlayActor+0x88` as a session-struct pointer when it
is in fact the play-style int. All seven were unreferenced (masked by the crate-wide
`allow(dead_code)`), so nothing was misreading memory; the risk was a future reader trusting them.
Replaced with a comment recording what `+0x88` actually is and where the live song-identity path
runs. Uncommitted, `cargo check` + `cargo fmt` clean.
