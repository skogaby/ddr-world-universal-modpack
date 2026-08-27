# Task: LOOP ON driver + Step-4 cabinet demo

## Description
The loop itself (design §4.3): while a LOOP ON training session is live,
the per-frame driver watches the raw music count and, at the clamped
section end, fires `song_reset::request_reset(a_live, TRAINING_LEAD_MS,
Zero, None)` — the shipped in-place reset/seek with accumulator zeroing —
back to the section start, indefinitely, until quick-fail/quick-restart or
song exit. One in-flight reset at a time (a cooling latch until the count
rewinds); a refused iteration retries once next frame then disarms the
loop for the song with one WARN (design §6). Closes plan Step 4 with the
cabinet demo.

## Background
The fire bound is the load-bearing subtlety (research §4.3 + the approved
breakdown decision #1): the CMA cascade is one-way and the seek gate
refuses at cascade step ≥ 4, so the loop must fire strictly below BOTH
live thresholds — `fire_bound = min(b_live, raw_for_display(notes, t94),
t98) − MARGIN`, where `(t94, t98)` are read live via
`chart_end_thresholds` (LOOP ON never wrote them — they are stock),
`b_live` is the live section end (`B_MS`; none ⇒ skipped from the min —
loop-ON-alone loops the whole song, breakdown decision #2), and MARGIN is
the existing 1000 ms end-margin class (also covers the ~150–300 ms
stop/replay prepare window during which the pre-completion anchor keeps
counting). A `raw_for_display` failure drops that term with one WARN
(the t98 − MARGIN clamp still guards the fatal step-5 edge; step 4 may
then fire and disable further seeks — degraded to a one-shot loop,
consistent with the §6 ladder).

The reset target is the LIVE section start (`A_MS` via
`active_section_start()` — row-derived or gesture-refined; none ⇒ 0, the
binding-free plain restart, which is also the §6 "identity binding
refused" degradation). `request_reset` supplies the approach lead
(`TRAINING_LEAD_MS` future-dating = the silent scroll-in the design's
loop-iteration cost budgets), the accumulator zeroing, the R14 freeze
neutralization, and the `on_song_reset` subscriber fan-out (assist tick
re-claps from A) — the driver only decides WHEN.

Driver shape: Step 3's `training_mode/driver.rs` gameplay loop already
runs per frame while armed; task-02's latch keeps it armed for loop
sessions. Add the loop leg after the resolution/adjust legs: compute the
fire bound once per song (after resolution; thresholds/notes are stable),
then per frame compare `song_reset::current_raw_music_count()`. After a
`Started` reset, hold a cooling latch until the observed count drops
below the bound (the completion rewinds it — also naturally covers the
prepare window where the count still climbs); after `Refused`, retry
exactly once on the next frame, then disarm with one WARN. Generation
tokens already kill the loop on scene exit; triple-3/triple-1 win by the
same one-in-flight rule the shipped resets use.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-13-training-mode/design/detailed-design.md (§4.3 loop bullet, §6 ladder rows "Seek Refused/Unsupported mid-loop" + "Identity binding refused")

**Additional References (if relevant to this task):**
- docs/training_mode_research.md §4.3 (the one-way cascade + clamp rationale), §2.4 (the reset service contract)
- src/mods/training_mode/driver.rs (the gameplay step loop this extends)
- src/services/song_reset/mod.rs (`request_reset`, `current_raw_music_count`, task-01's `chart_end_thresholds`/`decoded_notes`)
- src/mods/training_mode/bounds.rs (`active_section_start`, `section_end`, task-02's loop latch)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Fire-bound computation (once per song, after resolution settles):
   `min(b_live?, raw_for_display(notes, t94)?, t98) − 1000 ms`, terms
   dropping out when unavailable per the Background; a bound ≤ 0 (or below
   the current count at compute time + a section start at/above it)
   disarms the loop with one WARN (degenerate section).
2. Per-frame loop leg in the driver (LOOP ON latch only): current count ≥
   fire bound ⇒ `request_reset(active_section_start().unwrap_or(0),
   TRAINING_LEAD_MS as i32, AccumulatorPolicy::Zero, None)`.
   `Started` ⇒ cooling until the count reads below the bound;
   `Refused` ⇒ one retry next frame, then disarm + one WARN (natural
   continue — the stock thresholds still end the song).
3. Gesture interplay: a mid-song gesture that moves A or B updates the
   NEXT iteration (B changes recompute the fire bound; the live `A_MS`
   read happens at fire time). Bounds cleared to none under LOOP ON ⇒
   loop-whole-song semantics continue (bound from thresholds only).
4. Driver lifetime: the step loop must keep requeueing while the loop is
   armed (it currently exits when resolution + adjust settle); the
   60 s soft timeout must NOT apply to a live armed loop (a grind session
   is legitimately long — scope the timeout to the pre-anchor phase).
5. Zero footprint: LOOP OFF sessions take none of this path; the loop leg
   is gated on task-02's per-song latch.
6. Host tests where pure logic permits (the fire-bound min/margin
   composition as a pure helper in `section_math` with dropped-term
   cases); the loop behavior itself is cabinet-validated by the step demo.
7. Step-4 readiness gates + the cabinet demo (below), including the
   task-02 legs; remind the maintainer the loop-row PNGs must reach the
   cabinet's `data_mods/` first.

## Dependencies
- task-01 (converters + threshold reads), task-02 (loop latch +
  session-active extension + LOOP OFF legs of the demo).
- Steps 1–3 shipped (`request_reset` with nonzero T, the driver, bounds).

## Implementation Approach
1. Pure fire-bound helper + tests (harness).
2. Driver loop leg (compute-once bound, cooling latch, retry/disarm
   ladder, timeout scoping).
3. Readiness gates; cabinet demo closes plan Step 4.

## Acceptance Criteria

1. **The grind loop (the Step-4 demo)**
   - Given LOOP ON with a section set (e.g. START 30 / END 60)
   - When the run reaches the section end
   - Then it resets in place to the section start behind the 2.5 s
     approach lead — combo/score/gauge zeroed, claps re-aligned —
     indefinitely, until triple-3 (or triple-1) exits as shipped
2. **Loop-whole-song**
   - Given LOOP ON with no bounds set
   - When the run nears the natural end
   - Then it resets to 0 strictly before the end cascade fires and keeps
     looping (works even when the identity binding was refused)
3. **LOOP OFF unaffected**
   - Given LOOP OFF and a section end
   - When the run reaches it
   - Then the stock banner → results tail runs with the partial stats
     (task-02's leg — no loop machinery engages)
4. **Refusal ladder**
   - Given a mid-loop reset refusal (e.g. injected via the existing dry-run
     or fault knobs where applicable)
   - When the iteration fires
   - Then one retry occurs next frame, then the loop disarms with one WARN
     and the song continues to its (threshold-truncated or natural) end
5. **Cascade never trips mid-grind**
   - Given a long grind session at any rate (75 %/100 %/125 %)
   - When many iterations elapse
   - Then seeks keep working every iteration (the cascade never reaches
     step 4 — the clamp held) and the log shows no unexpected WARNs

## Metadata
- **Complexity**: High
- **Labels**: training-mode, driver, song-reset, engine-facing, cabinet-demo
- **Required Skills**: Rust, the song_reset transaction model, the Step-3 driver
- **Generated By**: code-task-generator 2026-08-14
- **Source Plan**: .agents/planning/2026-08-13-training-mode/implementation/plan.md
- **Plan Step**: Step 4: LOOP SONG — loop driver + early natural end
