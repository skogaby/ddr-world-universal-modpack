# Rough Idea — Assist Tick: Pre-Mixed Tick Track

The shipped **Assist Tick** mod (`.agents/planning/20260725-assist-tick/`, `src/mods/assist_tick.rs`)
plays a clap per arrow by calling `se_play` from the per-frame judge clock. It works, but players
hear **timing imprecision during bursts of 8th/16th notes** — confirmed by a high-level player and
by the maintainer on headphones. The latency knob (constant offset) is fine; the defect is
tick-to-tick **jitter**.

Root cause (diagnosed, then confirmed by an RE spike into `xactengine2_10.dll`):

1. **Detection is frame-quantized** — the tick fires on the first judge-clock frame at which it is
   due (±½ frame of independent jitter per clap; ±8.3 ms at 60 fps).
2. **The engine's cue starts are packet-quantized** — XACT starts pending waves from its internal
   notify-thread pump at the DirectSound mix-packet cadence (~10 ms), with **no sample-offset
   start primitive anywhere** (voice `Start(1,0,0,0)`). So even a perfectly timed trigger inherits
   ±5 ms. There is no runtime future-scheduling API (`SoundBank::Play`'s timeOffset is
   seek-into-cue semantics).

**Proposal:** eliminate per-tick triggering entirely. At song start, synthesize **one continuous
waveform** with every clap mixed in at its exact sample position (the whole-song "look-ahead
buffer"), wrap it in an in-memory wave bank, and play it **once** as a single cue through the
game's own engine. Inter-tick spacing becomes sample-exact by construction; the only residual
error is one constant per-song start offset (±½ frame + ±½ packet, once), absorbed by the
existing latency knob.

Maintainer-flagged considerations for planning:

- **Latency knob:** may be less needed (or differently interpreted) when pre-mixing against the
  game's own beat cues — reassess its role.
- **Player judgement-offset preferences:** the stock game exposes per-player visual and audio
  offset options in the in-game options menu. The clap should sound at the moment the judgement
  is *supposed* to happen, so at minimum the per-player **audio offset** likely needs to be
  taken into account.
- **Cabinet-level offsets:** the global timing offsets (e.g. `sound_offset` from the
  timing-offsets work) may also shift where the claps should land. Needs investigation during
  planning: what `music_count` is actually derived from, and which offsets shift the music, the
  judgement window, and the chart timestamps relative to each other.

Enabling facts from the RE spike (functions named + plate-commented in the Ghidra project,
`xactengine2_10.dll`):

- `SoundBank::Stop(cueIndex, flags)` exists at vt+0x28 (`0x423b80`) — a usable stop path for
  quick-restart / fail-out / song-exit.
- The engine reads wave data **from the client-owned buffer** (never copies it) — so one
  fixed-size in-memory wave bank allocated at boot can have its sample bytes **rewritten in place
  per song** (no per-song bank leak, no bank destruction, which is a known crash class).
- Bank registration/slot machinery, ADPCM format constraints, and the offline encoder already
  exist (`src/services/game_audio.rs`, sibling `ddr-chart-tools`, `docs/xact_audio_research.md`).
