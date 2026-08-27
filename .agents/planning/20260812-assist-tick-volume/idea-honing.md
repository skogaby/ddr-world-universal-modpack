# Idea Honing: Assist Tick Volume Option

All 12 decisions accepted wholesale by the user on 2026-08-12.

Readiness Confirmed 2026-08-12.

## Decision Register

| ID | Decision | Why it matters | Recommendation | Status |
|----|----------|----------------|----------------|--------|
| D1 | Whose volume applies (one tick track, per-player option) | User-visible behavior in versus | The FR-5 **chosen side's** value (the side whose chart drives the claps) | Accepted |
| D2 | Volume application point | Architecture; determines when changes take effect | Pre-scale the clap PCM once per song on the synthesis thread (pure CPU, no new engine surface); **applies next song** like every other assist-tick parameter | Accepted |
| D3 | Option id / wire field | Persistence schema, row_order docs, backend column | `assist_tick_volume` → wire `mod_assist_tick_volume` | Accepted |
| D4 | Backend (bemani-buddy) companion change | Network persistence needs a dedicated `opt_mod_assist_tick_volume` column + round-trip | In scope as a companion plan step (precedent: `mod_song_speed`); offline JSON cache works meanwhile | Accepted |
| D5 | Gain semantics | Sound character | Linear amplitude: `sample × percent / 100`, i32 headroom, saturate to i16 (existing mixer convention). >100 % may soft-clip if the clap nears full scale — accepted | Accepted |
| D6 | Range / steps / default | User-specified | 25–175, fine 5, coarse 10 (identical to song_speed), default **100** = unity gain | Accepted |
| D7 | Latch timing | Consistency with LATCHED_ENABLED | Latch per-side volume at GAMEPLAY entry alongside the enables; in-place quick-restart keeps the latch (same song, same latch) | Accepted |
| D8 | Persisted-value sanitization | Hand-edited JSON / stale server values | `load_transform` clamps 25–175 and snaps to the nearest 5 (song_speed's `load_normalize` pattern, private helper in assist_tick) | Accepted |
| D9 | Registration gating & failure mode | Fail-open behavior | Child registers right after the parent in `enable()` (ShowWhen needs the parent first); scalar row additionally requires `row_injection_available()` — if unavailable, warn once, row absent, unity volume. `Duplicate` on re-enable ⇒ reseed atomics | Accepted |
| D10 | Value display format | Row rendering | `ScalarFormat::Integer` — bare number, "%" lives in the label "TICK EFFECT VOLUME (%)" (song_speed convention) | Accepted |
| D11 | Preview texture content | Player-facing copy | WIDE layout mirroring song_speed's: "Adjusts the volume of the clap sound played by the assist tick during gameplay." / "Less than 100% makes the clap quieter. Greater than 100% makes it louder." | Accepted |
| D12 | Docs updates | Operator docs list every option id | README `row_order` complete example + option-id list gain `assist_tick_volume` (after `assist_tick`); README Assist Tick feature row + AGENTS.md assist-tick entry updated | Accepted |

## Q&A / Rationale

### D1 — Whose volume applies
The mod produces exactly one centre-panned tick stream per song, following the FR-5 chosen
side (solo = that player; versus = P1 or the only enabled side; doubles = the single chart).
A per-player volume can therefore only ever apply through the chosen side. Alternative
(max/average of both sides' values in versus) rejected: arbitrary, and inconsistent with the
existing rule that the chosen side's chart, judgment timing, and enable already win.

### D2 — Application point
Alternatives rejected: (a) live XACT cue/bank volume — no existing API in `game_audio`,
would need new RE work and per-frame surface for marginal benefit; (b) scaling inside the
mix loop per-sample-write — equivalent output, marginally more work per overlapping clap.
Pre-scaling the ~214 ms clap once per song on the synthesis thread is the cheapest correct
point and keeps `synthesize_track`'s signature change minimal. Consequence: volume is baked
into the encoded track — mid-song option changes can't apply (they can't anyway: the options
screen is unreachable mid-song) and rewind/restart reuse the same track at the same volume.

### D3 — Option id
`assist_tick_volume` groups visually with its parent in row_order and config JSON, and reads
unambiguously in the wire format. Alternative `tick_volume` (shorter) rejected: loses the
parent association that `pacemaker_threshold`-style naming provides.

### D5 — Gain semantics
"%" naturally reads as linear amplitude. Perceptual (dB) mapping rejected: over a 25–175 %
span the difference is minor and the linear rule is explainable in one preview line.

### D6 — Range
User-specified: identical to playback speed (25–175, fine 5, coarse 10). Note 25 is the
floor — the row cannot fully mute the tick (that's what the parent toggle is for).

### D9 — Failure mode
If the scalar-row machinery isn't ready (`row_injection_available()` false) the parent bool
still registers (it doesn't need the scalar donor) and ticks play at unity volume — the
feature degrades to exactly today's behavior.
