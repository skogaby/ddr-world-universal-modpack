# Training Mode Research — Section Practice, Looping, Rewind/Fast-Forward

Feasibility RE record for a DDR World training mode: skip the first X seconds
of a song, omit the last X seconds, loop a section until quick-fail, and
(lofty) rewind/fast-forward at will with a live content-time progress display.

All gamemdx addresses are file-relative to image base `0x180000000`, build
**2026-07-21** unless noted. Facts marked **[prior]** come from earlier
research records (cited); **[static]** = Ghidra decompilation this pass
(2026-08-13); **[infer]** = inferred, not yet read directly.

Prior art this builds on directly:

- `.agents/planning/20260812-inplace-restart/research/run_state_re.md` — the
  run-state RE record (GamePlayActor field map, judge records, gauges, audio
  stop/replay, msg `0x1044`). The in-place reset shipped from it
  (`src/services/song_reset.rs`) is the foundational primitive here.
- `docs/quick_restart_fail_speedup_research.md` §12.2 — where the in-place
  reset / Training Mode direction was first established.
- `docs/xact_audio_research.md` + `docs/xact_streaming_research.md` — the
  XACT engine surface and the read-callback interception (song-rate streaming).
- `docs/song_playback_speed.md` — the authoritative gameplay clock and the
  Q31 clock patch.
- `docs/ssq_format.md` §5.3/§5.4 — step-byte / freeze-block / shock encoding.

## 1. Feature → primitive map (summary)

| Feature | Verdict | Mechanism |
|---|---|---|
| Skip first X s (start at T) | **High** — designed-for extension | `song_reset::request_reset(T)`: back-dated `0x1044` tick (`now − wall(T)`, §6) + direct record rebuild at playhead `T` (§3) + shifted audio serving (§5) |
| Omit last X s (loop end bound) | **High** | Loop mode: per-frame content-time check ≥ `end_T` → reset to `start_T`; clamp `end_T` below the end-cascade thresholds (§4.3) |
| Omit last X s (early natural end → results) | **High** — simpler than expected | Two field writes: shrink ControlMessageActor's end thresholds (`+0x94`/`+0x98`); the game runs its own full natural tail (§4.4) |
| Loop until quick-fail | **High** | In-place reset already keeps DPS at step 7 forever; no stage consumed; quick-fail exits as shipped. One-way end cascade never completes if `end_T` is clamped (§4.3) |
| RW/FF gestures | **Medium-high** | Same seek primitive; ~0.15–0.3 s per seek (cue re-prepare) ⇒ discrete jumps, not smooth scrub. Forward seeks must clamp below the end thresholds (§4.3) |
| Content-time HUD / progress bar | **High**, no new RE | Read `actor+0x178` (raw ms) or `+0x168` (display domain) per frame; song end = ControlMessageActor `+0x94`/`+0x98` or last note `+0x08` (§4.2); widget system precedent (`timing_stats_widget`) |
| Full score/judgement rewind | Feasible, deferred | Periodic judge-record vector snapshot (`totalNotes × 0x40`, note pointers stable) — rewind quantizes to snapshot times. Fallback (no score, judgements/combo/EX only) matches rebuild-at-T semantics naturally |
| Score integrity | **Solved** | `score_guard` taint (autoplay/quick-fail/rate precedent): per-stage suppression + sanitised logout |

## 2. The core primitives already in hand **[prior]**

### 2.1 The engine's native rewind is seekable

`GamePlayActor`'s msg `0x1044` handler calls `FUN_18005bac0(actor, tick)`:
re-anchors the music clock (`+0x160 = tick`), clears + rebuilds the entire
judge-record vector from the pristine note list, rebuilds the density array,
resets note-flash renderers, re-enters the in-song step (StackStep 4),
broadcasts `0x1051 {side, totalNotes}`. The rebuild worker is

```
FUN_180060d40(out_records, notesBegin, notesEnd, &{actor, playhead})
```

The `0x1044` path hardcodes playhead 0; calling `FUN_180060990` (clear) /
`FUN_1800608d0` (reserve) / `FUN_180060d40` (rebuild) directly with
`playhead = T` produces a mid-song cursor. §3 pins the exact per-kind
semantics at nonzero T **[static]**.

### 2.2 The gameplay clock is one anchor subtraction

```
music_count = playerObj->vt+0x248()
            + frameTick@[DAT_1806f2cf0+0x1268] − SOUND_OFFSET@+0x16C − anchor@+0x160
```

Seek-to-T = broadcast `0x1044 {now_tick − wall(T)}` (back-dated anchor).
The v4 restart-delay knob already ships the future-dated variant (negative
clock lead-in), so anchor arithmetic in both directions is engine-tolerated.
Rate interaction: the Q31 clock patch scales raw mc by `effective_r`
downstream of the anchor subtraction, so `wall(T) = content_to_wall_ms(T)`
via the existing `song_rate::tick_domain` conversions (§6).

### 2.3 Audio stop/replay is cheap; XACT cannot seek

- Stop cue `FUN_1801aa7c0(handle @ DPS+0x128)` → replay
  `FUN_1801aa5c0(5, name)` → poll `FUN_1801aa630`: **~0.13 s** worst case,
  zero disk I/O (bank stays registered). Proven by the shipped reset.
- **No retail seek path exists**: `Play(timeOffset)` only fast-forwards the
  cue *event* timeline; a due wave starts at sample 0
  (`Wave_StartNow_NoSampleOffset`). Seeking into content requires shifting
  the sample bytes the engine reads.
- The song-rate streaming engine already owns the read path: gamemdx's XACT
  readFile/getOverlappedResult callbacks are detoured
  (`song_rate::io_callback_hook`), and a bound handle's every audible byte is
  served by the mod (virtual bank). MS-ADPCM blocks are self-contained
  (128 samples ≈ 2.90 ms), so a whole-block shift is exact. §5 specs the
  shifted-serving design.

### 2.4 The in-place reset service (shipped, v4.2 confirmed)

`src/services/song_reset.rs`: gates (GAMEPLAY, DPS step 7, actors step 4,
non-course, snapshot valid) → stop/replay cue → poll prepared → ONE
synchronous frame block: `0x1043` + `0x1044 {tick}` broadcast, zero
accumulators, ctor-mirror gauges + ScoreActor repaint sentinel, notify
subscribers (`on_song_reset(t_ms)`). `t_ms != 0` returns `Unsupported` —
the Training Mode extension point. Subscriber re-sync contracts (assist_tick,
PUS, score_guard, song_rate) are established.

## 3. The note time domains + FUN_180060d40 at playhead T **[static]**

### 3.1 Two time domains on every note (corrects the prior field map)

The 0x60-stride Note struct carries TWO time values, and the prior doc's
reading (`+0x04` = musicCount, `+0x08` = freeze end *(infer)*) was wrong:

| Offset | Domain | Proven by |
|---|---|---|
| `+0x04` | **display/chart-offset domain** (beat-proportional) | `FUN_1801c8d50` maps `+0x08 → +0x04` by binary search + interpolation, with the slope emitted as the BPM readout; ControlMessageActor thresholds and last-note-end use `+0x04` |
| `+0x08` | **raw music-count milliseconds** (the judge domain) | `judgeNotes` windows: late flag at `mc ≥ note+0x08 + 0xA0` (160 ms), cursor break at `mc < note+0x08 − 0x104` (260 ms); the rebuild playhead compares `note+0x08 < playhead` |

`FUN_1801c8d50(vec@actor+0x90, mc_raw, &bpm_out)` is the domain converter —
the per-frame count update `FUN_18005eb00` computes BOTH counts each frame:
`actor+0x168` = display-domain count, `actor+0x178` = raw-ms count (offset by
`RENDER_OFFSET − INPUT_OFFSET − playerObj->vt+0x240()`), then broadcasts
**`0x1045 {side, displayCount, rawCount, bpmFloat}`** to the parent tree.
The seek playhead `T` is therefore in the **raw-ms domain** — the same
domain as the clock anchor math. A Training Mode UI in "seconds into the
song" works directly in this domain (modulo rate scaling, §6).

### 3.2 Note-kind semantics in the rebuild (per-kind, playhead-aware)

`FUN_180060d40` walks the notes and appends one 0x40 record per non-control
note (kind byte ≥ 0; negatives −5/−6/−7 are control notes, skipped).
Record defaults: `judgedAt = −1`, `grade = 0xFF` (pending), per-panel hold
progress zeroed, wobble bytes `0xFF` — but if ANY per-panel duration
(`note+0x3C..`) > 0 the wobble/hold sentinel pair is zeroed instead
("this record participates in freeze processing").

| Kind | Meaning | Rebuild behavior at playhead T |
|---|---|---|
| 0 | **tap / jump / shock / freeze head** (shock = all-4-panels-of-a-side shape; freeze head = has per-panel durations) | `note+0x08 < T` ⇒ consumed: `judgedAt = note+0x08`, `grade = 6` if shock-shaped else `0`. Else pending (`0xFF`/−1) |
| 1 | armed marker, **never tap-judgeable** (grade 5, or 7 when shock-shaped; `judgedAt = note+0x08` pre-filled) — playhead-INDEPENDENT. Exact producer in the SSQ parse not yet pinned; safe under seek either way | always armed, identical at any T |
| 2 | **freeze-END marker** (the SSQ `0x00` step byte + freeze-block entry, `docs/ssq_format.md` §5.4) — NOT "shock" as the prior doc guessed | `grade = 7`, `judgedAt = note+0x08`. If `note+0x08 < T` (freeze fully before T): walks the records already emitted, finds the one whose panel flags match, and **back-patches its hold progress (+0x14..) to the full per-panel durations** — i.e., pre-T freezes are marked fully held |

Key proof that pre-T notes can never mass-miss: the tap-judge path in
`judgeNotes` (`FUN_18005ec70`) only touches records with
`judgedAt < 0 && grade == 0xFF`; consumed records (grade 0/6) and armed
markers (5/7) are permanently skipped. The miss path likewise only fires
inside that same pending-record branch. **[static]**

### 3.3 Freeze spanning T (head < T < end) — behavior + optional polish

The freeze processor `FUN_18005f790` (runs at the end of every judgeNotes)
gates on: record has freeze durations (wobble ≠ 0xFF) **and**
`judgedAt ≥ 0` **and** `grade != 5`. A spanning freeze's head is rebuilt as
consumed (grade 0, `judgedAt = note+0x08` — both gates pass), so at seek
time its body becomes live freeze processing with hold progress 0:

- Player holds the panel(s) through T ⇒ progress accumulates
  (`display_count − note+0x04` per panel, display domain), completes at the
  end marker ⇒ the kind-2 record is written `grade 6` + msg `0x102E`
  (freeze OK) exactly as normal.
- Player doesn't hold ⇒ the wobble counter drains and the freeze NGs
  (`0x102F`) shortly after T.

That is acceptable practice semantics (hold-through-the-loop-point works;
an ignored spanning freeze NGs once). If we prefer spanning freezes to be
NEUTRAL instead, the seek can post-process exactly like the engine's own
pre-T path: copy the durations into the head record's hold progress and set
the kind-2 record consumed — a ~10-line loop over the rebuilt vector.

The freeze end-of-hold matching walk searches forward for a kind-2 record
with `note+0x04 == headNote+0x04 + max(headNote durations)` — pure chart
data, unaffected by seeks. Shock judging is inline in judgeNotes (panel
shape + `note+0x08 ± 0x22/0x54` windows, "shock ng" debug string at the NG
site); pre-T shocks rebuild as grade 6 = passed-OK, no penalty. **[static]**

## 4. How the song ends naturally — the full chain **[static]**

### 4.1 The end is content-time driven, not chart-cursor driven

`FUN_18005bde0` (the DPS state-7 per-side poll) is only the death/give-up
predicate: finished ⇔ `StackStep ≥ 5`, or (`m_isDead@+0x1E8` && (give-up
option via playerObj vt+0x1D8, or miss-streak `+0x1E4 ≥ 0x32`, or an
event-mode edge)). It never looks at the chart. The NATURAL end comes from
**ControlMessageActor** (child of each GamePlayActor, ctor `FUN_180055d00`,
vtable `0x180360708` — no custom update; its entire logic is the msg
handler `FUN_180056090`, which handles ONLY `0x1045`):

```
ctor (from the chart's control notes; kinds −5/−6/−7 = 0xFB/0xFA/0xF9):
  +0x88 = intro threshold   (kind −6 note's +0x04; derived fallback: firstNote+0x04 − 0x2000)
  +0x8C/+0x90 = staged intro thresholds (fallback chains, clamped)
  +0x94 = LAST-NOTE-END     (last real note: +0x04 + max(its per-panel durations))  [display domain]
  +0x98 = OUTRO/song-over   (kind −7 note's +0x08)                                   [raw-ms domain]

on 0x1045 {side, displayCount, rawCount, bpm}: one-way StackStep cascade —
  step 0: displayCount ≥ +0x88 ⇒ broadcast 0x1047, step 1   (lane notice arm)
  step 1: displayCount ≥ +0x8C ⇒ broadcast 0x1048, step 2   (lane notice on)
  step 2: displayCount ≥ +0x90 ⇒ broadcast 0x1049, step 3
  step 3: displayCount ≥ +0x94 ⇒ broadcast 0x104A, step 4   (chart content over)
  step 4: rawCount     ≥ +0x98 ⇒ broadcast 0x104B, step 5   (SONG over)
```

A single `0x1045` **falls through multiple steps** — if the count jumps past
several thresholds at once, every remaining event fires in that one message.

GamePlayActor's handler (`FUN_18005e200`) responds: `0x1048`/`0x104A` drive
the lane-notice actor; **`0x104B` sets the GamePlayActor's own StackStep to
6** — which makes `FUN_18005bde0` return finished, the DPS state-7 loop
advances, broadcasts **`0x1053`** (sets `+0x1E9` judge-event suppression;
course league mirror), and states 8/9 run the banner
(`FUN_1800334f0(kind)`: 4 = cleared, 5 = all-dead, 3/8 = course advance;
event mode 0x9733 = 0) + cue stop (only if all dead) + `finish`.

### 4.2 Progress bar inputs fall out for free

Per-frame, no hooks needed: current = `GamePlayActor+0x178` (raw ms,
render-offset-adjusted) or `+0x168` (display domain); total =
ControlMessageActor `+0x94` (display) / `+0x98` (raw ms) or last real note's
`+0x08` from the note vector. ControlMessageActor is reachable from the
GamePlayActor's child list (RTTI `.?AVControlMessageActor@dance@sequence@@`),
or the thresholds can be recomputed from the note vector directly.

### 4.3 Loop-mode guard (the cascade is one-way and never resets)

ControlMessageActor has **no** `0x1044` handling — the in-place reset does
NOT rewind its cascade (fine for restart-at-0: fired intro steps are
cosmetic one-shots). For Training Mode this means:

- **Loop end bound must be clamped**: `end_T` strictly below the `+0x94`
  threshold (display domain) and `+0x98` (raw). If the loop reset fires
  before the thresholds, the cascade never reaches steps 4/5 and the song
  loops indefinitely at DPS step 7. Our own per-frame end-check triggers the
  reset, so the clamp is the only guard needed.
- **Forward seeks must respect the same clamp** — a seek past `+0x94`/`+0x98`
  fires `0x104A`+`0x104B` on the next frame's `0x1045` and hard-ends the
  song (StackStep 6 is past the `0x1044` handler's {3,4} gate: the run
  becomes unresettable). Clamp seek targets to `end_T_max` = last-note-end
  − margin.
- Intro lane-notice events (`0x1047..0x1049`) skipped by a mid-song start
  or replayed section: cosmetic only.

**Refinement (2026-08-14, cabinet-driven, supersedes the same-day t=0
note):** `0x104A` (chart content over, cascade step 3→4) is NOT loop-
compatible even though t=0 resets bypass the seek gate. Live-observed:
it triggers the full-combo celebration and the lane-notice actor then
STRIKES THE LANE FURNITURE (filter, background, guidelines) — one-way,
song-scoped, never re-arms across `0x1044` resets — and on subsequent
looped passes scored events go missing (deterministic sub-1MM cap with
a full marvelous combo, i.e. the freeze-OK class stops completing once
the "chart over" state is latched). **A loop must keep the cascade
below step 4 on EVERY pass.** The clean mechanism: under LOOP ON, RAISE
the CMA `+0x94` display threshold to an unreachable sentinel (stock pair
stashed for restore) — the cascade parks at its normal mid-song step 3,
the full chart plays and scores on every pass, seeks stay legal
(step < 4), and `+0x98` stays STOCK so every other reader (marker
clamps, seek clamps, the loop fire bound) remains honest. The raised
threshold MUST be restored whenever the loop stops governing the song's
end while the run continues (loop disarm): with the cascade parked,
`0x104B` can never fire and the song otherwise soft-locks at its
natural end.

### 4.4 "Omit last X seconds → early natural end" variant (results screen)

Because the whole end machinery reduces to two ControlMessageActor
thresholds, an early NATURAL end (with banner, results, real score save) is
just: at song start, write smaller values into `+0x94` (display-domain
end, convert via the note vector / `FUN_1801c8d50` equivalent) and `+0x98`
(raw ms). The game then runs its stock tail at the truncated time — no
scene surgery, no suppression needed. (Whether truncated-but-natural plays
should still be score-guarded is a policy question for the spec — the
un-played tail's notes count as unjudged misses in the record?? NO — they
remain pending/unjudged records, and the stage record writes whatever the
accumulators hold at leave; grade/rank math uses totals, so an early end
undercounts. Default to score-suppressing training plays.)

### 4.5 The money score's dynamic denominator (live-diagnosed 2026-08-14)

`judge_submit` (`FUN_18005fd30` on 20260721; the shipped `judge_submit`
signature) recomputes the money score on every judgment:

```
score = (floor(((marv + grade6 + perfect)·5 + great·3 + good) · 200000
               / (D · 10)) − good − great − perfect) · 10
D = [GPA+0x194] + [GPA+0x198] + [GPA+0x19C]
```

`+0x194`/`+0x198` are static chart populations (blli Expert: 438 rows /
18). **`+0x19C` is a per-run DYNAMIC counter**: the engine increments it
once per freeze-head "arm" conversion (per PANEL — blli's 19 freeze ends
carry 34 head panels), and each conversion simultaneously awards a
grade-6 judgment — numerator and denominator grow in lockstep, so a
natural full pass lands on exactly 1,000,000. Two persistence traps for
anything that replays a run in place:

- The engine's `0x1044` rewind does NOT rewind `+0x19C` (it only sums
  the trio for its record reserve), and the conversions' one-shot state
  lives in the NOTE vector (also never rebuilt) — a replayed pass keeps
  the fat denominator but can never re-earn the grade-6s. Cabinet
  signature: full-marvelous-combo passes deterministically capped below
  1MM (blli: 456×100,000/490 = 930,610).
- Fix class: latch `+0x19C` at song start (the gauge-snapshot probe runs
  pre-music, before any conversion) and restore it in the reset's
  accumulator block. The start-of-song reserve undershoots the record
  count the same way every natural song does — vector append growth is
  the engine's everyday path.

`song_reset::judge_diag(side)` exposes the populations + per-grade judge
counts + score/combo as a read-only diagnostic surface.

## 5. Shifted audio serving — the design (code-read pass, 2026-08-13)

Read against `src/services/song_rate/{binding,generator,lifecycle}.rs` +
`src/core/xact/virtual_bank.rs`. The song-rate streaming engine turns out to
be *almost* the whole answer; the seek adds one concept (a content shift) and
one lifecycle extension (arming at identity).

### 5.1 The load-bearing constraint: the header is parsed ONCE

The engine parses the virtual bank's header a single time, inside
`CreateStreamingWaveBank` (i.e. inside `wavebank_create`, once per song);
cue Prepare only primes stream contexts from the already-parsed entry
metadata (`docs/xact_streaming_research.md` §3). **Per-seek layout changes
are therefore ineffective — the entry's declared duration/data_len are
frozen for the bank's life.** The seek must be a pure *content remapping*
under a fixed layout:

```
virtual main-entry block i  ↦  content block (B(T) + i)        B(T) = T / 2.90ms (source-block grid)
blocks past (source_end − B(T))  ↦  encoded silence blocks     (machinery exists: STATE_SILENCE_FILL / copy_spans_silent)
```

Each cue replay re-reads the entry from virtual offset 0 with a fresh
stream context (no engine memory of prior consumption — proven by the
shipped reset's stop→replay), so after a seek, "offset 0 = content at T"
is exactly what the engine's next read sequence observes. The trailing
silence (engine believes the entry is full-length) is harmless: loop mode
stops the cue at every reset, DPS leave/state-8 stops it otherwise, and
§4.3's end-clamp bounds real play anyway.

### 5.2 Serving modes

- **Identity rate (100 %) — shifted passthrough.** The binding already
  serves the *side* (preview) entry verbatim from the resident source copy
  (`side_source_offset`, always-available spans in `check_spans`). The seek
  mode generalizes that to the MAIN entry: serve
  `source[main_data_offset + shift_bytes + within]`, silence past the tail.
  No producer thread, no ring, no DSP — allocation-free copies in the
  serve path, structurally identical to the shipped side-entry arm.
- **Non-identity rate — shifted stretch.** The producer's `Feed` already
  supports repositioning: `rewind_to(target)` restores a checkpoint (WSOLA)
  or seeks O(1) (`positioned_at`, resample) and re-produces
  deterministically. A seek sets the shift, bumps the ring seqlock
  (`ring_rewind` — the exact behind-window mechanism), and production
  restarts at output block 0 under the new mapping (content B(T)
  onward). Cost estimate: cue prepare needs the first ≤64 KiB packet
  (~0.35 s of audio); WSOLA at ~2.4× realtime produces it in ~0.15 s —
  inside the cue-prepare window (~0.13 s), so seeks at rate stay
  ~0.2–0.3 s.

  **Correction + decision (2026-08-13, Step-1 cabinet finding):** the cost
  estimate above implicitly assumed a resume point near the target. WSOLA
  is sequential — each output window's source position comes from a
  similarity search anchored on the PREVIOUS window's landing
  (`StretchCheckpoint.previous_start`), so canonical bytes at output P
  require the full alignment chain up to P. With only the loop-start
  checkpoint captured, a deep forward pre-shift produce-and-discards the
  whole gap (~25 s observed for a 60 s shift at 90 %). **Maintainer
  decision:** shift>0 seeks in pitch-preserved mode are served by a fresh
  stretch SEEDED at the shift-mapped source position — O(1), frame-exact
  (`output_total − shift` frames), byte-level alignment deliberately
  unpinned across epochs (imperceptible across the cue stop/replay
  discontinuity). See the design §4.5 amendment for the epoch/determinism
  contract.
- **Preview entry** stays verbatim passthrough as shipped, unaffected.

### 5.3 Lifecycle extension: training arms a binding at 100 %

Today `lifecycle` arms only non-identity percents; ordinary 100 % boots are
`GenerationPhase::Identity` — no binding, zero footprint (a hard design pin
for stock behavior). Training mode must NOT weaken that pin globally;
instead the training mod contributes a second arm reason at scene 26
("training session active" ⇒ arm even at 100 %). Everything downstream
(transaction commit order, movie suppression, score containment via the
rate ledger, `RateSnapshot` identity semantics for tick_domain/real_speed)
already handles the identity-committed shape or needs only the taint
policy tweak: **a training play at 100 % must be score-suppressed by its
own flag** (score_guard has the general per-stage + logout-sanitise
machinery; the rate ledger only covers non-100 %).

**Plan-shape correction (code-read finding):** `plan_entry(…, 100)` is NOT
stock-shaped — `rate::target_for_percent` block-quantizes the output
(`output_frames = round_half_up(frames/spb)·spb`), so a stock entry whose
real duration sits inside its final block would advertise a slightly
different duration and a ≈1-but-not-1 `RateRatio`. The identity arm must
therefore plan the MAIN entry with `passthrough_plan` (stock header values,
exactly like the side entry today) and serve it via the §5.2 shifted
passthrough — never through `plan_entry(100)` + generator.

**Silent-tail coverage (code-read finding, closes former Q9):**
`Binding::new` already pre-encodes one silent ADPCM block *per entry from
that entry's own parsed format* (`adpcm::encode_block(&zeros, format, …)`,
binding.rs ~908–929) — stereo dance banks are covered by construction; the
shifted tail reuses `copy_spans_silent`'s block-tiling as-is.

### 5.4 Seek transaction ordering (mirrors the shipped reset)

```
gesture → gates (song_reset Phase 0 + seek clamp §4.3)
  → stop cue (FUN_1801aa7c0)
  → publish shift atomically (binding.set_content_shift(B(T)); ring_rewind; regen from entry start)
  → replay cue (FUN_1801aa5c0(5, name))
  → poll prepared → ONE synchronous frame block:
      0x1043 + 0x1044 {now_tick − wall(T_q)}          (§6)
      rebuild records at playhead T_q (clear/reserve/rebuild trio, §3)
      [optional] spanning-freeze neutralization post-pass (§3.3)
      accumulator policy per mode (loop: keep or zero; RW/FF: zero + judgement-only stats)
      notify on_song_reset(T_q)
```

`song_reset::request_reset(t_ms, …)`'s `Unsupported` arm becomes the real
implementation; the audio-shift call is the only new step versus the
shipped `t_ms == 0` path.

## 6. Wall(T) anchor math + rate interaction **[verified against code]**

The clock site (§2.2, `docs/song_playback_speed.md` §5) computes
`raw_mc = opt248() + (frameTick − SOUND_OFFSET − anchor)`, and the Q31 stub
scales the COMPLETE signed value: `mc = round(raw_mc · r)`. The shipped
reset re-anchors with `anchor = tick_now` (the stock DPS state-6 protocol),
making the run start with the same small negative lead-in as a natural
start. A seek generalizes it exactly the way the v4 delay knob did in the
other direction:

```
T_q     = B(T) · block_ms                          (quantize to the source ADPCM block grid, ≤2.90 ms)
wall(T) = snapshot.effective_rate.content_to_wall_ms(T_q)   (identity ⇒ 1:1; existing tick_domain API)
anchor  = tick_now − wall(T)                        (back-dated; i64, +0x160)
```

Then `mc(now) ≈ T_q + r·(opt248 − SOUND_OFFSET)` — i.e. content time T_q
with the identical lead-in/calibration shape as a natural start, while the
replayed cue's audio (serving content from B(T)) becomes audible after the
same real-world latency. SOUND_OFFSET stays inside the actor-side
subtraction, wall-domain and unscaled — matching the tick_domain algebra
(`wall_pos = content_to_wall(t + jt − m0) − sound_offset`). Quantizing T to
the block grid FIRST keeps chart clock, claps, and audio mutually exact.

Consumers (all existing, all proven under reset-to-0):
- **assist_tick**: its `restart_skip_ms(mc − m0, …)` conversion and
  rebuild-on-reset already handle arbitrary `mc` jumps; the pre-mixed track
  rewrite shifts by whole blocks — same grid as B(T).
- **real_speed**: multiplier cluster lives on the surviving actor;
  unaffected.
- **PUS / score_guard**: subscriber callbacks receive `t_ms = T_q` and
  apply per-mode policy (§5.4).

## 7. 0x1044 subscribers under a back-dated tick **[static]** — all clear

- **ControlMessageActor**: no `0x1044` handler; one-way `0x1045` cascade
  (§4.3). No camera replay burst — the intro events are one-shots that
  simply don't re-fire. (The prior doc's "camera/control notes replay"
  concern is moot: the actor's only outputs are the five threshold
  broadcasts.)
- **SceneManageActor** (ctor `FUN_18007d0a0`, vtable `0x1803633e8`): msg
  handler `FUN_18007d5d0` handles ONLY `0x1001` (readiness veto). Its update
  starts the background system one-shot (`FUN_1800319b0`/`FUN_180031770`)
  then idles at step 2 forever. The background free-runs — a seek/reset
  never re-seeks it. Purely cosmetic, exactly as the shipped reset observed.
- **CalcCalorieActor**: msg handler (`FUN_1800534a0` on 721) handles ONLY
  `0x1043` (arms the measurement-window StackStep) and `0x1045` (stores the
  BPM/intensity payload word[3] into `+0xD0`). No `0x1044`, no anchor
  reads — kcal accumulates per real elapsed frame regardless of seeks.
  Matches the shipped reset's "kcal carries across resets" behavior;
  nothing to do for Training Mode.
- **GamePlayActor** `0x1044` handler epilogue (beyond `FUN_18005bac0`): stamps
  the receptor-frame renderer (+0x138 object) note-flash slots to
  `0xFFFFD8F1` (idle), resets the flash renderer (+0x150) write cursor, and
  broadcasts `0x1051 {side, totalNotes}`. All benign at any anchor value.

## 8. Selected-song identity + audio length at song select **[static]**

For clamping section-bound options ("skip first Y s" / "omit last X s") to
the highlighted song's real length at select time.

### 8.1 The selection source the game itself uses

The preview-request function (`FUN_18010eab0` on 20260721 — the function
already identified as the preview player in
`docs/xact_streaming_research.md` §5) reads the currently highlighted song
as a shared_ptr at **`DAT_1806f2d50 + 0x1B0`** (object) / `+0x1B8`
(refcount), takes the song's code via the music object's **vt+0x08**
(basename getter), builds `data/sound/win/dance/<code>` plus the preview
cue name `<code>_s`, and requests the FileManager load + slot-5 play
(`FUN_1801ccd10(mgr+0xC8, 5, path, cue)`). This is the authoritative
"cursor is on this song" source — the same object the preview pipeline
consumes.

### 8.2 Audio length for free via the existing wavebank hook

The preview plays the `<code>_s` entry of the **same XWB the gameplay
audio uses** (one file, two entries — cabinet-proven during the streaming
work). Consequences:

- `song_rate::wavebank_hook` already detours EVERY slot-5 dance-bank
  create, **including the preview player's**, and at create time the
  FileManager row holds the ENTIRE XWB resident (`DAT_1806f2f48` file
  table: buffer at row+0x8, size at row+0x14 — already derived by
  `derive_song_rate_io_callbacks`).
- Parsing just the header (`xwb::parse_song_bank` — pure, allocation-light,
  already exercised on every bind) yields the MAIN entry's duration
  (frames) + sample rate ⇒ **gameplay audio length in ms**, plus the song
  code via the existing `dance_bank_song_code()` path parser.
- So: a small extension of the create detour publishes
  `{code, audio_length_ms}` on every slot-5 dance-bank create — no new
  detours, no new signatures. The most recent publication while at scene 25
  = the highlighted song (the preview load fires when the wheel settles on
  a song — exactly when the options menu can be open). LayeredFS custom
  songs work identically (same FileManager path).
- Fallback / cross-check: read `DAT_1806f2d50+0x1B0` directly (one new
  signature for the manager global + the vt+0x08 code getter) if the
  passive approach ever proves stale; not expected to be needed.

### 8.3 Domain note

Audio length is an UPPER bound for the option ranges (audio ≥ chart
content). The UI clamp uses it (per the maintainer's decision — no SSQ
parsing at select time); the hard runtime clamp stays the
ControlMessageActor end thresholds at gameplay (§4.3), which are
chart-derived and authoritative. Both are content-domain; rate does not
scale them.

## 9. Errata for prior docs (found this pass)

1. `run_state_re.md` §3 note struct: `+0x04` is the **display/chart-offset
   domain** and `+0x08` is the **raw-ms judge time** (§3.1) — the prior
   labels ("+0x04 musicCount", "+0x08 freeze end *(infer)*") are wrong.
   Everything the prior doc said about the REBUILD still holds (its playhead
   statements were domain-agnostic).
2. `run_state_re.md` §3 kind byte: kind 2 is the **freeze-end marker**, not
   "shock". Shocks are kind-0 notes with the all-4-panels-of-a-side shape
   (consistent with `docs/ssq_format.md` §5.3, `FUN_1801c6d80`).
3. `run_state_re.md` §2.3: `FUN_180060340` is NOT shock processing — it is
   the **ghost/pacemaker score-target updater** (walks the ghost grade
   history at `+0x1F8`'s byte vector, computes money/EX target, broadcasts
   `0x1036` — the ScoreActor's rival-sync source). Shock judging is inline
   in `judgeNotes`.
4. `run_state_re.md` §2.1: `+0x174` "last-note end" — the authoritative
   end thresholds live on ControlMessageActor (`+0x94`/`+0x98`, §4.1);
   GamePlayActor's step-6 flip comes from msg `0x104B`, not from a local
   compare against `+0x174`.

## 10. Open questions / risks (running list)

1. Kind-1 notes: exact SSQ producer unknown (armed grade-5/7 records,
   never tap-judged — the judge's outer gate requires `judgedAt < 0 &&
   grade == 0xFF`, which kind-1 records never satisfy — and
   playhead-independent in the rebuild ⇒ decision-neutral for seeks).
   Static attempt this pass dead-ended: `FUN_1801c6d80` (the shock
   classifier per `docs/ssq_format.md`) is an rb-tree helper on 20260721 —
   that doc's function addresses are from an older build. Pin down
   opportunistically (chart hexdump diff, or re-locate the classifier on
   721).
2. Loop iteration cost: each reset re-prepares the cue (~0.13 s of silence).
   Acceptable for section grinding; a "seamless" loop would need gapless
   audio serving (out of scope v1).
3. Seek-target clamping (§4.3) needs the display-domain conversion for the
   `+0x94` compare — read ControlMessageActor's fields directly rather than
   recomputing.
4. Gesture allocation: pinpad 1/3 (restart/fail), 9 (logout, scene 25 only),
   0 (menu) are taken; RW/FF/loop-set gestures need a free pattern during
   gameplay.
5. Score/judgement rewind (lofty variant): judge-record snapshot ring —
   deferred until the simple variant proves out.
6. ~~`FUN_180024530`~~ RESOLVED **[static]**: it is the shock-shape
   predicate (all 4 panels of either side == 1) used by judgeNotes to split
   shock handling from tap handling — not a CUT/removed-note gate. (Note:
   per-panel flag value **4** is accepted alongside 1 in the tap path;
   producer unknown, not seek-relevant.)
7. Early-natural-end variant (§4.4): confirm the stage record/rank math
   tolerates unjudged tail notes, or simply score-guard training plays
   (recommended default).
8. Identity-arm surface area (§5.3): the "training arms at 100 %" lifecycle
   extension must not disturb the 100 %-is-literally-stock pin for
   NON-training sessions — needs its own gate + fail-open story (binding
   refusal at 100 % ⇒ training seeks unavailable, song plays normally).
9. ~~Silence-tail encoding~~ RESOLVED (§5.3): per-entry silent blocks are
   already encoded from each entry's own format at `Binding::new`.
10. Seek + versus/doubles: the shipped reset resets BOTH sides (one clock
    anchor, per-side rebuilds). A seek inherits that; per-side thresholds
    (§4.3 clamp) must use the max of both sides' ControlMessageActor
    values. Course stays blocked (existing gate).
11. Cue-handle churn under heavy RW/FF: each seek is a stop→replay pair
    allocating a fresh handle from the manager's cue-handle table (256
    entries, per-frame reaper destroys finished cues; exhaustion leaks
    rather than crashes). The shipped reset proves the pattern per-restart;
    RW/FF just raises the frequency (~one per few seconds — far below any
    plausible reap deficit). Deploy-watch, not a design risk.
12. `+0x174` (GamePlayActor's ctor-copied last-note-end): no reader found
    in any function decompiled across both passes; if the early-natural-end
    variant (§4.4) ever misbehaves, write the truncated value there too
    (trivially safe — it's a ctor-time copy of the same quantity).

## Addendum 2026-09-04 — loop/marker/timeline revision

Planning: `.agents/planning/2026-09-04-training-loop-revisions/`.

**READY-banner soft-lock (press 6, LOOP OFF).** Root cause chain: the
marker gestures read GamePlayActor `+0x178` with only a `scene == GAMEPLAY`
gate. Pre-anchor (DPS init states 0..=6 — the "READY?" window) that field
holds the raw frame tick (minutes-since-boot scale; the 2026-08-14 driver
finding), so `set_marker('B')` stored a garbage end and, with LOOP OFF, the
v1 early-natural-end write pushed it into the ControlMessageActor thresholds
of a run whose DPS never consults the end event pre-song. Fix: the shared
`song_reset::run_in_song()` predicate (`first_anchored_frame()` — DPS step 7
+ every GamePlayActor at its in-song step with a nonzero `+0x160` anchor —
AND `+0x178 < min(chart_end_raw)`), consumed by every training gesture
(4/5/6/7/9) and by the loop driver's initial bound compute (the same two
checks it previously carried inline). `first_anchored_frame` is a STATE
predicate despite the name (true for the whole in-song phase).

**Sections are loop-only.** SONG START/END TIME are `ShowWhen::Equals`
children of LOOP SONG (LOOP registered first — framework parent-first rule);
hidden values are retained-but-IGNORED at all three readers (GAMEPLAY-entry
`rows_engaged` = any side's loop row; `try_resolve_row_bounds` resolves as
defaults when `!loop_on`; `refresh_pre_shift` requires the governing side's
loop row and is also refreshed from `on_loop_song_change`). Marker gestures
4/5/6 gate on the per-song `loop_latched()` (one hint toast per song when
refused); 7/9 scrub does not. The timeline HUD's veil + A/B lines render
only while the loop is latched (cursor/readout/strip unconditional). The v1
§4.4 early-natural-end variant is therefore unreachable from input;
`section_math::end_policy`'s `WriteThresholds` arm stays as dead-defensive
code. Pure gate logic: `section_math::gesture_gate` /
`decorations_visible`, host-tested via `scripts/validate_training_mode.sh`.
