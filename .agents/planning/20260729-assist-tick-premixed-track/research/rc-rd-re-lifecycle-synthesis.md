# R-C / R-D / R-E — Drift, lifecycle/capacity, synthesis cost

**Status: resolved (2026-07-29), from the RE spike + shipped-feature facts + existing docs.
No new Ghidra needed beyond what's cited.**

## R-C — `mc` ↔ audio-stream drift over a full song (feeds D9)

**Finding: within-song relative drift between the tick track and the music is ≈ 0 by
construction. Only a one-shot constant start-anchor error remains — already in scope.**

Reasoning:

- The song audio is itself an XACT voice: "*Everything audible — menu BGM, voice, every sound
  effect, and the song audio itself — passes through one engine instance and one final mix*"
  (`docs/xact_audio_research.md` §1).
- Our pre-mixed tick track is another XACT voice (in-memory wave bank cue). All voices are mixed
  by the one render thread against the **one output sample clock** (native DirectSound or Wine
  dsound alike). Two voices on the same mixer clock advance sample-for-sample together.
- Therefore once both are playing, the claps stay locked to the music **regardless of what `mc`
  does** — the baked clap spacing is in the track's own samples, not re-derived from `mc` per
  tick. This is the structural advantage over the shipped per-tick design, which re-reads `mc`
  every clap and so inherits any `mc`↔audio wobble.
- `mc` is used exactly **once**, to anchor the track start (content offset = `t_i − m0` at the
  first judge dispatch, m0 = that frame's `mc`). Whether `mc` is wall-clock- or
  audio-position-derived only affects the *constant* start error (±½ frame + ½ packet, per D1),
  not any accumulating drift. CrossOver/Wine changes the packet size, not this conclusion.

Consequence for D9: one-shot anchoring is sound. The rewind guard (stop + re-Play with
`timeOffset` seek) re-anchors on restart; no periodic re-sync is needed.

## R-D — Rewrite/stop lifecycle + capacity (feeds D6, D8)

### Rewrite-in-place safety

The engine parses the wave-bank **header** (entry table: offset, sample count, format) once at
`CreateInMemoryWaveBank`; it reads **sample bytes** from our buffer lazily during playback (never
copies — RE spike finding 5). So the safe in-place-rewrite contract is:

- **Header/entry table is fixed for the process lifetime.** One wave entry, declared at the
  **maximum** duration (capacity below). ADPCM is block-fixed-rate, so max duration ⇒ fixed
  sample-data byte length ⇒ the header never changes.
- **Per song we rewrite only the sample-data bytes:** ADPCM-encode `[0, song_len]` with the claps
  mixed in, then zero-fill (encoded silence) the remainder to the fixed length. The cue plays the
  full max duration but is silent after the last clap (and is stopped at song exit anyway).
- **Rewrite only when no voice is reading the buffer.** Protocol: `SoundBank::Stop` at gameplay
  exit / quick-restart / fail-out (D8), then rewrite at the **next** song's build (first judge
  dispatch — seconds later), by which point the stopped cue has been reaped by the per-frame cue
  reaper. Belt-and-braces: guard the rewrite on "no active tick cue" (we hold the play handle).
  Double-buffering (two wave entries, ping-pong the cue's wave index) is the fallback if the
  stop→reap window ever proves racy — noted, not adopted (adds an XSB with two cues).

### Stop semantics

`SoundBank::Stop(cueIndex, dwFlags)` (vt+0x28, `0x423b80`) → `FUN_004127a0(engine, bank,
cueIndex, dwFlags & 1, 0)`. The `& 1` is `XACT_FLAG_STOP_IMMEDIATE`: **pass flags = 1 for an
immediate stop** (flags = 0 = "as authored", lets tails/release play). We want immediate. Takes
the engine critsec like every engine API → game-thread call, consistent with the shipped
game-audio threading rule.

### Capacity

- DDR World standard charts run ≲ 2:00. Size the fixed entry at **5:00 (300 s)** as a safe cap
  (maintainer-chosen headroom over the observed max).
- Mono, 44100 Hz (matches the shipped clap; no resampling). MS-ADPCM ≈ 0.55 byte/sample →
  300 s × 44100 × ~0.55 ≈ **~7.3 MB** for the sample-data segment. One-time, negligible.
- Charts exceeding the cap: truncate ticks past the cap + one WARN (D6). Course/nonstop modes (if
  they present one continuous actor longer than the cap) would hit this — acceptable; the tail
  simply has no claps. If real-world courses need more, the cap is a single constant to raise.

## R-E — In-process synthesis cost + encoder port scope (feeds D7, D10)

**Finding: well-scoped port of existing offline code; per-song cost is low-tens-of-ms on a
background thread, far inside the multi-second lead-in.**

### What runs per song (background thread, pure CPU — no game ABI)

1. Allocate/clear a PCM mix buffer at the fixed max length (i16 mono; ~7.9 M samples @ 180 s).
2. For each tick `t_i`: add (mix) the pre-decoded clap PCM at sample `round(t_i_ms/1000 · rate)`,
   with saturating add (claps overlap on 16th bursts; the clap is ~214 ms ≫ a 16th at speed).
   ~400 claps × 9,423 samples ≈ ~3.8 M adds — trivial.
3. MS-ADPCM-encode the whole buffer (block-based; predictor search + per-sample quantize). Low
   tens of ms on a modern CPU; on CrossOver still ≪ the lead-in.
4. Hand the encoded bytes to the game thread, which rewrites the wave-bank sample segment (R-D)
   and (re-)plays the cue.

### Port scope (from the sibling `ddr-chart-tools`, all already offline-proven)

- **MS-ADPCM encoder** (`adpcm::encode` — shared with the song-conversion path; SNR 17.4 dB per
  the shipped feature's open item E; adequate for a clap).
- **XWB writer** (in-memory wave bank: header + one ADPCM entry at max length). Rewrite path
  touches only the sample-data segment.
- **XSB SE writer** (`xsb::write_se`: one cue → wave index 0, mix category 6, no RPC) — built
  once at boot.
- **Clap source:** ship as **raw mono i16 PCM** (not ogg), so the DLL needs no Vorbis decoder —
  decode is a file read. (Asset-format change from the shipped `clap.ogg`; the offline
  `build_assist_tick_bank.sh` pipeline is superseded by in-DLL synthesis.)

The existing `services/game_audio` already owns `register_bank` (CreateInMemoryWaveBank +
CreateSoundBank) and `play_cue`; this feature adds `stop_cue` (vt+0x28) and a
`rewrite_bank_samples` path, plus the three porters above (likely a new `services` module for
the WAV/ADPCM/XWB/XSB synthesis, or a sibling-crate dependency if the repo can vendor it).

## Net effect on the register

All of D6–D10 hold as accepted. Refinements folded in: fixed-max-length single wave entry with
silence-padded tail (R-D); immediate-stop flag = 1 (R-D); 180 s / ~4.4 MB capacity (R-D); clap
ships as raw PCM, three porters from ddr-chart-tools (R-E); one-shot anchor is drift-safe (R-C).
No decision is invalidated.
