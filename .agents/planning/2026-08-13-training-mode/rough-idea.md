# Training Mode — Rough Idea

Captured 2026-08-13 from the maintainer's description (feasibility RE was
front-loaded before this PDD run; see `docs/training_mode_research.md`).

A training mode for DDR World akin to what the console releases had, and
what rhythm games like Clone Hero have. Component features:

1. **Skip the first X seconds of a song** — when the song begins, it skips
   right to the desired section.
2. **Omit the last X seconds of the song** — so together with (1) you can
   isolate one specific section.
3. **Loop the song repeatedly** until the user quick-fails out, rather than
   playing once and going to the results screen. Combined with (1) and (2),
   this lets you grind one specific section — especially combined with
   Song Playback Speed and Assist Tick.
4. **(Loftier) Rewind / fast-forward at will during play**, operated through
   a button combination or gesture trigger. Ideally with a live view of the
   current song timestamp (content-time, not wall-time), the overall song
   length, and a progress bar that tracks play position and adjusts on
   seeks — as would the song audio, chart, etc.
5. **Score handling for the lofty rewind**: ideally rewind the score and
   judgement buffers back to their state at that timestamp; if infeasible,
   disable score accumulation while RW/FF is enabled and track judgements
   only, so the user still sees combo and EX score for the section being
   practiced.

## Addendum (2026-08-13): options-menu grouping

The training-related options should be visually grouped on the MODS tab
under a **header row labeled "TRAINING OPTIONS"** — a non-selectable,
display-only row (the mechanism RE'd in
`docs/option_header_rows_research.md`, modeled on the native gray
scroll-speed info row). Below the header: assist tick, playback speed
adjustment (song speed), realtime gameplay statistics, pacemaker→ms-error,
and whatever new training mode options this feature lands. The header
should be **full width** (unlike the ~70%-width native gray row) and
**half-height / slimmer** than a normal row (per-row y-extent at
`row+0xA8` is a per-row layout input — confirmed controllable).
