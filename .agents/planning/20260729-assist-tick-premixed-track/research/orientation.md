# Orientation — Assist Tick: Pre-Mixed Tick Track

Step-2 blind-spot pass. Sources: the shipped assist-tick feature and its planning record
(`.agents/planning/20260725-assist-tick/`), the durable RE record (`docs/xact_audio_research.md`),
the timing-offsets feature research (`.agents/planning/20260626-timing-offsets/research/`), a fresh
RE spike into `xactengine2_10.dll` (2026-07-29, findings below; key functions renamed +
plate-commented in the shared Ghidra project), and two maintainer-captured screenshots of the
in-game per-player Judge options (`capture/capture_20260729_1424*.jpg`).

## 1. The problem, precisely

The shipped mod triggers one `se_play` per tick from the per-frame judge clock
(`src/mods/assist_tick.rs::tick_clock`). Audible tick-to-tick jitter in 8th/16th bursts comes from
two independent quantizers stacking:

| Stage | Grid | Jitter contribution |
|---|---|---|
| 1. Detection — tick fires on the first judge-clock frame at which it is due | frame period | ±½ frame per tick (±8.3 ms @60 fps), independent per tick |
| 2. Engine start — XACT starts pending waves from its internal pump | DirectSound mix packet (~10 ms) | ±½ packet per tick (~±5 ms), independent per tick |

At 60 fps consecutive inter-clap intervals can swing by ~a full frame plus a packet — ~20–25% of a
16th-note interval at 170–180 BPM. Trained listeners detect ~2–5 ms of inter-onset jitter in
rhythmic sequences; both stages individually exceed that. The adaptive half-frame lead centres the
*mean* error only; the latency knob shifts the *constant* only. Neither touches variance.

## 2. RE spike findings (xactengine2_10.dll, 2026-07-29)

Full playback pipeline traced from `se_play` to the DirectSound voice start. Functions named in
the Ghidra project: `XACT2Engine_DoWork` (0x4122e0), `Engine_NotifyThreadPump` (0x411850),
`SoundBank_Play` (0x423990), `SoundBank_Stop` (0x423b80), `Sound_Play` (0x422ab0),
`Sound_PumpUpdate_DrainScheduledWaves` (0x422da0), `Wave_ComputeScheduledStartMs` (0x4136c0),
`Sound_InsertWaveSortedOrStartNow` (0x421b10), `Wave_StartNow_NoSampleOffset` (0x414180).

```
se_play → SoundBank::Play(vt+0x20, timeOffsetMs) → Sound_Play
  → per track/wave: scheduled_ms = qpc_now_ms + XSB_event_time_24bit
  → insert into the sound's time-sorted wave queue — or start synchronously if already due
  → queue drained ONLY by the notify-thread pump (signaled by the DirectSound render
    thread every ~10 ms packet): while (scheduled_ms <= pump_window) → voice->Start(1,0,0,0)
```

Decisive facts:

1. **Engine starts are packet-quantized (~10 ms), on an engine-internal thread, independent of
   the game's frame loop.** `DoWork` (game thread, per frame) only does cleanup/notifications —
   its cue-update twin (0x4231d0) never starts waves. This *corrects* `docs/xact_audio_research.md`
   §1's "audio submission is frame-quantized" — our *trigger* is frame-quantized; the engine's
   start grid is its own ~10 ms packet clock.
2. **No sample-accurate start primitive exists.** `voice->Start(1,0,0,0)` carries no sample
   offset; a started voice begins rendering at the next packet boundary.
3. **No runtime future-scheduling API.** `SoundBank::Play`'s `timeOffset` is seek-*into*-the-cue
   semantics (negative rejected). Future starts exist only via the XSB play-wave event's 24-bit ms
   time field — authored data (read fresh from the client-owned XSB buffer at each Prepare, so
   technically mutable, but it does not remove the packet-grid start quantization).
4. **`SoundBank::Stop(cueIndex, flags)` exists at vt+0x28** — game-exercised, takes the engine
   critsec like every API here. A stop path is available.
5. **The engine never copies wave data** — it reads from the client-owned (leaked) buffer for the
   bank's lifetime. Consequence: one fixed-size in-memory wave bank registered once at boot can
   have its sample bytes rewritten in place between songs.

Consequences for the option space:

- A precise trigger thread (option A) removes stage 1 but cannot beat the ~10 ms packet grid —
  the floor stays audible.
- **A pre-mixed whole-song tick track (option B) is the only architecture that eliminates
  inter-tick jitter**: clap spacing is baked in at sample resolution inside one continuous
  waveform riding the same mixer as the music. Both quantizers still apply — but exactly once,
  at track start, as a constant per-song offset (±½ frame + ±½ packet), absorbable by the knob.

## 3. The timing/offset chain — knowns and unknowns

The clap must land at **the moment the judgement is supposed to happen** (maintainer's framing).
Several offsets potentially sit between "chart timestamp in the Results vector" and that moment:

### Known (binary-verified, `.agents/planning/20260626-timing-offsets/research/r3-field-semantics.md`)

Four **cabinet-wide** offsets, published into the process-wide config map, latched into
`GamePlayActor` fields at construction (per song):

| Key | Actor field (20260324) | Semantics |
|---|---|---|
| `SOUND_OFFSET` (default 87) | `+0x16c` | shifts the music-count baseline: `beginTick = tick − soundOffset`; higher = audio later relative to steps. **`music_count` is tick(wall-clock)-derived, not audio-stream-derived** |
| `INPUT_OFFSET` (default 28) | `+0x170` | input/judge ("SSQ") reference; subtracted in the display formula |
| `RENDER_OFFSET` (default 17) | `+0x184` | display only (`dispMusicCount = musicCount + RENDER − INPUT`) |
| `BOMB_FRAME_OFFSET` | `+0x188` | shock effect only |

The shipped mod fires at `music_count == t_i` (Results-vector time) and the maintainer tuned the
residual constant by ear via the knob — so whatever constant the cabinet offsets contribute is
currently *inside* the knob's tuned value.

### New (maintainer captures, 2026-07-29)

The stock game exposes **two per-player** options on the Judge tab of the player Options menu:

- **DISPLAY TIMING** (±ms) — *"Change the position where the arrow overlaps the step zone …
  in accordance with the video"* — visual arrow-draw shift only. Irrelevant to the clap.
- **JUDGMENT TIMING** (±ms) — *"Change the **time at which the arrows are judged** …
  in accordance with the sound"* — a per-player judge offset. **This is the one that matters:**
  it moves the expected-perfect moment per side, so the clap for the tick-driving side should
  follow it.

### Open questions (research targets)

- **R-A: Where does JUDGMENT TIMING land?** If the engine bakes it into the Results-vector
  timestamps, the tick list inherits it for free (and the current mod already honors it). If it
  is applied at judge-compare time (a per-actor field consulted in `judgeNotes`), the mod must
  locate and read it per side and shift the tick times. Also: its sign convention, units,
  storage (profile field? actor field offset?), and when it is latched.
- **R-B: What exactly does "the judgement moment" equal in music_count domain?** Candidate:
  `t_i` + per-side JUDGMENT TIMING (+ possibly INPUT_OFFSET's role). The current by-ear-tuned
  behavior at JUDGMENT TIMING = 0 defines the baseline; research should express the target as
  "current behavior + per-side judgment shift" rather than re-deriving absolutes.
- **R-C: Does `music_count` drift against the music audio stream?** music_count is wall-clock
  derived; the audio stream is rendered by the engine. Over a 2-minute song, clock drift between
  the two would bend a pre-mixed track's alignment (the per-frame approach re-anchors every tick;
  a pre-mixed track anchors once). Expectation: both ultimately derive from QPC and the audio
  hardware clock; drift over ~2 min is sub-ms on native hardware — but this should be reasoned
  about (or bounded) explicitly, especially under CrossOver/Wine.
- **R-D: Track lifecycle mechanics.** In-place rewrite protocol for the boot-registered wave
  bank (when is no voice reading the buffer?), stop semantics of `SoundBank::Stop` vt+0x28 from
  our thread vs. the game thread, restart/fail-out/song-exit paths, max track duration sizing.
- **R-E: In-process synthesis cost.** Clap PCM decode + overlap mixing + ADPCM encode of a
  ~2-min mono track between the first judge dispatch and the first note (lead-in typically a few
  seconds; the shipped clap is 9,423 samples / ~214 ms; encoder lives in the sibling
  `ddr-chart-tools` — port vs. re-implement vs. try the structurally-accepted-but-unexercised
  PCM codec).

## 4. What the pre-mix changes about the two existing knobs

- **Latency knob (`assist_tick.offset_ms`)**: stays, with unchanged meaning (constant
  trigger-to-audible compensation, now applied once at track-start anchoring instead of per
  tick). The maintainer's tuned value should carry over — the pre-mixed track should be anchored
  so that a knob value tuned under the old implementation produces the same average alignment.
- **JUDGMENT TIMING (per-player)**: new input to tick-time computation (pending R-A) — applied
  as a per-side shift of the whole track content (all claps shift together, so it is a
  track-content offset, not a start-time offset — it can exceed the lead-in safely).

## 5. Constraints carried over from the shipped feature (unchanged)

- Audio through the game's own engine only (shares the music's output latency); banks never
  destroyed; `file_id` stays −1; game-thread-only for engine calls (to be re-examined for the
  synthesis work, which is pure CPU and can go off-thread — only the register/play/stop calls
  are game-ABI).
- Chart-time driven, never judgment driven; FR-2 eligibility predicate, coalescing, side
  selection (FR-5), per-song latching (FR-8) all stay as shipped.
- Verification split: maintainer does all listening/gameplay verification; agent does offline
  validation, build gates, and log reading.

## 6. Proposed sequence

Clarify-first, then research: the decision register can be drafted now (the architecture choice
is effectively settled by the RE spike; most decisions are about offset semantics, lifecycle, and
synthesis mechanics), with R-A/R-B/R-C/R-D/R-E feeding back into the register before readiness.
