# Idea Honing — Assist Tick: Pre-Mixed Tick Track

Decision register. Accepted wholesale by the maintainer 2026-07-29 (D2/D3/D5 revised in
discussion before acceptance; see rationale sections).

| ID | Decision | Why it matters | Recommendation | Status |
|---|---|---|---|---|
| D1 | Architecture | Whole feature shape | Pre-mixed whole-song tick track, one cue through the game's own engine, synthesized per song from the Results vector; per-tick `se_play` retired | Accepted |
| D2 | What moment the clap marks | Core correctness | Tick moment = `t_i` − `sound_offset` (cabinet offset, per-song latched actor field `+0x16c`) **+ tick side's JUDGMENT TIMING** (`timing_music`, per-side `ddr::player::Option+0x24`, read at song build) — **derived from game state, not tuned. Follows JUDGMENT TIMING (confirmed 2026-07-29).** R-A proved it is applied at judge-compare time and is NOT inherited by reading raw `mc`/chart `t_i`, so it must be added explicitly | Accepted |
| D3 | Latency knob | UX / config surface | **Deleted**: overlay row removed, `assist_tick.offset_ms` config key retired and ignored if present (stale values would double-compensate). Contingency: new config-only trim key (default 0, no UI) if listening shows a residual constant | Accepted |
| D4 | Which side's JUDGMENT TIMING | Versus behavior | The chosen tick actor's side (P1 when both enabled — same FR-5 rule as shipped). One centre-panned track cannot serve two different per-player offsets | Accepted |
| D5 | Failure behavior | Reliability contract | **No fallback path.** Boot-time prerequisite failure (encoder, bank registration, capacity alloc) ⇒ mod fails init and never appears. Per-song synthesis failure ⇒ that song silent + one WARN (mid-session unload impossible). The per-tick path is not kept as a runtime fallback | Accepted |
| D6 | Bank strategy | Memory/stability | One fixed-size in-memory bank registered once at boot; sample bytes rewritten in place per song (engine reads our leaked buffer; never destroyed; `file_id` stays −1). Capacity sized in R-D; over-length charts truncate ticks + WARN | Accepted |
| D7 | Codec | Risk | ADPCM, encoder ported into the DLL from the sibling `ddr-chart-tools`. PCM (codec 0) is structurally accepted but unexercised on this cabinet — not primary; may be tested as a research curiosity | Accepted |
| D8 | Stop lifecycle | Crash safety | `SoundBank::Stop` (vt+0x28) on gameplay exit / quick-restart / fail-out, game thread only, always before any buffer rewrite. New game-ABI surface added to `services/game_audio` | Accepted |
| D9 | Start/re-anchor mechanics | Correctness | Start at first judge dispatch (content offset `t_i − m0` plus D2 shifts); clock-rewind guard re-anchors via stop + re-Play with `timeOffset` **seek** (RE-verified semantics) rather than rebuilding | Accepted |
| D10 | Synthesis threading | Frame safety | Mix + encode on a background thread (pure CPU, no game ABI); register/play from the game thread when ready; if not ready before the first tick, start late with a `timeOffset` seek | Accepted |
| D11 | Eligibility/coalescing/side selection | Scope control | Unchanged from the shipped feature (FR-2 predicate, 4 ms coalesce window, FR-5 side choice, FR-8 per-song latch). Only the playback architecture changes | Accepted |

## Rationale details

### D2 + D3 — the timing model (settled in discussion, 2026-07-29)

The maintainer challenged keeping the latency knob. Analysis that settled it:

- `music_count` starts at **−sound_offset** when the music stream starts (timing-offsets RE:
  `beginTick = tick − soundOffset`; corroborated live by the Step-2 demo log's first-clap at
  `music_count=-87` under stock `sound_offset=87`). So `sound_offset` is the cabinet's
  *declared* audio output latency: the judge moment `count == t_i` coincides with the audible
  beat exactly when `sound_offset` matches the real chain latency.
- The old per-tick path triggered at `count == t_i`, i.e. `sound_offset` ms of stream-anchor
  compensation after the equivalent music position was submitted — hence claps late by
  ≈ `sound_offset` + per-tick quantization + Wine fresh-start extras ≈ the 87–150 ms the
  maintainer tuned away with the knob.
- **Pre-mixing alone does not remove that constant** — the tick track is a parallel stream, not
  samples inside the music waveform; started at `count == m0` its content lags the music's
  content by the same `sound_offset`. Jitter dies; the constant survives.
- Therefore the track content is shifted **earlier by `sound_offset`** (read from the per-song
  latched actor field) + the tick side's JUDGMENT TIMING. **The target is the judgement
  moment** (maintainer, 2026-07-29: "the clap should be an audible cue that the game thinks you
  should have hit the arrow just now" — alignment with the heard music is desirable but
  secondary). `sound_offset` is the game's own declaration of the audio chain's latency, so
  this lands the clap on the judgement moment *to the precision of the cabinet's calibration*;
  a mis-set `sound_offset` shifts claps and heard music away from true judgement together, and
  calibrating it (Timing Offsets mod) fixes both at once.
- **Evidence correction (maintainer, 2026-07-29):** the old knob's 125 ms tuning aligned the
  *heard clap* with the *heard music beat* under CrossOver with the stock `sound_offset = 87`
  still in place (never calibrated for that setup — the game itself does not feel on-sync
  there). It is NOT evidence of alignment with the judgement moment. Derived datum: heard-clap
  vs heard-music alignment needed ≈ 125 ms while the declared latency was 87 ⇒ ≈ 38 ms of
  fresh-start/per-trigger asymmetry on that setup — support for keeping the contingency trim
  key in reserve (a freshly started track voice may carry a similar small constant).
- The old knob was papering over cabinet calibration; its stale persisted values (125–150 on the
  dev install) must be ignored, not reinterpreted, under the new semantics.

### D5 — failure semantics (refined in discussion, 2026-07-29)

Maintainer directed: no fallback; if pre-mixing can't work, the mod fails to load. Refinement
accepted: "fails to load" is only mechanically possible at boot (init returns false). A per-song
synthesis failure mid-session yields a silent song + one WARN — same fail-hard spirit, no
per-tick fallback ever.

## Open items feeding research (Step 4) — ALL RESOLVED 2026-07-29

| ID | Question | Feeds | Resolution |
|---|---|---|---|
| R-A | Where does per-player JUDGMENT TIMING land? | D2 | **Applied at judge-compare time; NOT in `mc` or note timestamps.** `FUN_18005f100` proves the count path carries only RENDER/INPUT/DISPLAY-TIMING (in the separate `dispMusicCount`), never `mc`. So it is not inherited — D2 adds it explicitly, read per side from `ddr::player::Option+0x24`. DISPLAY TIMING = `Option` vt `+0x240`, display-count only (irrelevant). `research/ra-rb-timing-chain.md` |
| R-B | Validate the `−sound_offset` anchor model | D2 | **Confirmed.** `mc` is the raw wall-clock-baseline counter (`beginTick = tick − sound_offset`; live `mc=−87` at stock 87); offset-agnostic. Shift track content earlier by `sound_offset` (actor `+0x16c`). Same file |
| R-C | `mc` ↔ audio drift over a song | D9 | **Non-issue.** Tick track and music are co-rendered XACT voices on one mixer clock ⇒ ≈0 relative drift; `mc` used once for the start anchor only. One-shot anchoring is sound. `research/rc-rd-re-lifecycle-synthesis.md` |
| R-D | Rewrite/stop lifecycle + capacity | D6, D8 | Fixed-max single wave entry (header immutable), rewrite only sample bytes at next-song build after an immediate `Stop` (flag=1, vt+0x28); silence-padded tail; **300 s / ~7.3 MB cap**, truncate+WARN beyond. Same file |
| R-E | Synthesis cost + encoder port | D7, D10 | Low-tens-of-ms/song on a background thread. Port ADPCM encoder + XWB writer + XSB SE writer from `ddr-chart-tools`; ship clap as raw mono i16 PCM (no Vorbis dep). Same file |

Readiness gate: **Readiness Confirmed 2026-07-29** (maintainer). Capacity set to 300 s. All
decisions accepted, all research (R-A…R-E) resolved; no open decision affects data models,
interfaces, or user-visible behavior. Proceeding to detailed design.
