# R-A / R-B — The timing chain: where each offset lands, and what the tick clock actually reads

**Status: RE core resolved (2026-07-29). One design decision surfaced (does the clap follow
JUDGMENT TIMING?) + one cheap empirical confirmation left to the maintainer.**

All addresses gamemdx 20260324 (file-relative to `0x180000000`). Decisive function:
`FUN_18005f100` — the per-frame gameplay count computation (it also emits the `0x1045`
sibling broadcast, so it is on the hot path once per frame per side).

## The per-frame count computation (verbatim decompile, annotated)

```c
// param_1 = GamePlayActor, param_2 = mc  (the RAW song-time counter, ms; starts negative)
beat_count(+0x168) = FUN_1801c7450(actor+0x90, mc, 0);          // chart beat position for mc

// DISPLAY count (what the arrows are drawn against):
dispMusicCount(+0x17c) = (RENDER_OFFSET(+0x184) − INPUT_OFFSET(+0x170)) + mc;
plVar6 = FUN_1801e7530(DAT_1806ebe50[side], 0);                 // per-side option/context object
dispMusicCount(+0x17c) -= (**(plVar6 + 0x240))(plVar6);        // − DISPLAY TIMING (timing_disp)

// 0x1045 broadcast payload to sibling actors = { side, beat_count(+0x168), mc }
```

### What this proves

1. **The judge/assist clock is the RAW `mc`.** `judgeNotes` (`FUN_18005EC70(actor, musicCount)`)
   is called with the same `mc` (`param_2`), not the display count. The shipped assist-tick's
   `tick_clock(actor, music_count)` therefore receives raw `mc`.
2. **`mc` excludes every per-player offset and excludes RENDER/INPUT.** RENDER_OFFSET,
   INPUT_OFFSET and DISPLAY TIMING appear **only** in the separate `dispMusicCount(+0x17c)`
   (the render reference). They never touch `mc` or the note timestamps.
3. **DISPLAY TIMING = the per-side `+0x240` vtable getter, subtracted from the DISPLAY count
   only.** [CONFIRMED] Pure visual (matches the in-game text "where the arrow overlaps the step
   zone … in accordance with the video"). **Irrelevant to the clap** — confirms orientation §3.
4. **JUDGMENT TIMING (`timing_music`, Option `+0x24`, ±100 ms) is NOT in the count path at
   all.** [STRONG INFERENCE] It is not in `mc`, not in the note timestamps, and not in
   `dispMusicCount`. The only place left for a "change the time at which the arrows are judged"
   knob is inside the judge comparison in `judgeNotes` (shifting the effective note time / the
   player's compared step time). It is therefore **applied at judge-compare time**.

## Consequence for D2

The shipped mod fires at `mc == t_i` (chart timestamp). Because `mc` and `t_i` are both
judgment-offset-agnostic:

- **JUDGMENT TIMING is NOT inherited by the current mod, nor would it be by a pre-mixed track
  built from the same `t_i` values.** If we want the clap to follow the player's judge offset,
  we must read `Option+0x24` for the tick side and add it explicitly. If we don't, the clap
  marks the objective chart/music beat regardless of the player's judge bias.
- This is a genuine **design decision**, not an RE fact (see the register update). The maintainer
  framed the goal as "play at the moment the judgement is supposed to happen," which argues for
  following it; the StepMania-assist-tick tradition argues the opposite (an objective metronome).

## R-B: the sound_offset anchor model

Corroborated, not contradicted, by the above:

- `mc` is the raw counter with baseline `beginTick = tick − sound_offset` (timing-offsets RE
  `r3-field-semantics.md`; the Step-2 demo log's first clap at `mc = −87` under stock
  `sound_offset = 87` is the live confirmation).
- `mc` is **wall-clock/tick-derived**, independent of RENDER/INPUT/DISPLAY/JUDGMENT offsets
  (now proven — they live in `dispMusicCount`, a separate field).
- So the D2 model holds: to make a clap audible at the judgement moment, shift the track content
  earlier by `sound_offset` (per-song latched actor field `+0x16c`) — accurate to the precision
  of the cabinet's `sound_offset` calibration, which is the game's own declaration of its audio
  latency and the only in-process source for it.
- **Correction (maintainer, 2026-07-29):** the old knob's 125 ms value is NOT validation of any
  judgement-alignment constant. It aligned the *heard clap* with the *heard music* under
  CrossOver with the stock (uncalibrated) `sound_offset = 87` — the game itself does not feel
  on-sync on that setup. Useful derived datum: 125 heard-vs-heard vs 87 declared ⇒ ≈ 38 ms of
  per-trigger fresh-start asymmetry on that setup, motivating the reserved config-only trim key
  (D3 contingency) in case a freshly started *track* voice shows a similar small constant.

## Cheap empirical confirmations for the maintainer (with the CURRENT shipped mod)

1. **JUDGMENT TIMING inheritance (confirms finding 4 live):** set P1 JUDGMENT TIMING to +100 ms
   in the Options menu, play, and listen. Prediction: **the claps do NOT move** relative to the
   music (current mod reads raw `mc` + chart `t_i`). If they don't move, the design's "must add
   it explicitly" is confirmed.
2. **DISPLAY TIMING irrelevance:** set DISPLAY TIMING to ±100 ms. Prediction: claps do not move
   (and only the arrows' visual position shifts). Confirms finding 3.

Neither is required to proceed — both are predictions the design already assumes — but they are
5-minute confirmations if the maintainer wants them before implementation.

## Remaining research (unchanged targets)

- R-C: `mc`↔audio-stream drift over a full song (native + CrossOver) — feeds D9.
- R-D: rewrite/stop lifecycle + capacity sizing — feeds D6/D8.
- R-E: in-process synthesis cost + encoder port scope — feeds D7/D10.
