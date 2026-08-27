# Rough Idea — Assist Tick

Captured verbatim from the maintainer, 2026-07-25.

---

I want to add a mod for adding "assist tick" to DDR World. Essentially, this is a mod that
should play a "clapping" sound at the same time each in-game arrow is meant to be hit. This
is directly analogous to the assist tick option that StepMania already supports.

We should already have hooks into the arrow judgement functions, which I'm assuming gives us
a good trigger point to actually play the audio clip. The main things we're missing are audio
playback itself, in addition to an in-game custom option that lets users toggle assist tick.

I'm envisioning the option residing in-game in the MODS options tab, rather than in the global
mod overlay.

However, since there's potential for each player (in a 2P session) to be playing on different
difficulties, if both players have assist tick enabled, the tick just plays based on the P1
difficulty and chart. If both players are on the same difficulty, nothing should be noticed.
If they're on different difficulties, the clap sound will only end up aligning with the P1
arrows.

Additionally, I extracted the OGG file from StepMania for the assist tick clap sound, this is
ideally what I'd like to re-use for our DDR implementation. It's located at `clap.ogg` on my
machine. [Path elided — the sample is committed to this repository as part of Step 1; see below.]

---

## Asset as provided

The sample, as supplied — Ogg Vorbis, mono, 44100 Hz, 0.2137 s (9423 samples), 10,704 bytes,
encoder libVorbis 1.3.4. This is StepMania's `assist_tick` clap. It is committed to this
repository at `data_mods/assist_tick/source/clap.ogg` by Step 1 of the implementation plan, so
the asset pipeline is reproducible from repository contents alone.
