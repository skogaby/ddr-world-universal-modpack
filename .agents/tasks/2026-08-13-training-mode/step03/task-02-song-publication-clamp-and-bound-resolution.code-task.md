# Task: Selected-song publication + select-time clamp + bound resolution

## Description
The data plumbing between song select and gameplay (design §4.2/§4.6):
(1) extend `song_rate::wavebank_hook`'s create detour to publish
`{song_code_digest, audio_len_ms, generation}` on EVERY slot-5 dance-bank
create (armed or not); (2) `bounds.rs` consumes it for the select-time
effective clamp of the SKIP FIRST / OMIT LAST values; (3) at gameplay
entry, resolve the session's `{a_ms, b_ms}` bounds from the rows +
`chart_end_raw`, block-quantized, and extend the training-session-active
latch to row-driven sessions. Step 3 consumes `a_ms` (task-03's silent
start); `b_ms` is resolved + logged for Step 4's loop/early-end.

## Background
The preview player loads the SAME XWB the gameplay audio uses (one file,
two entries — research §8.2), and the wavebank create detour already
fires for it with the whole file resident (`file_table_source(id)` /
`file_table_path(id)` → `dance_bank_song_code`). Parsing just the header
(`xwb::parse_song_bank`, pure) yields the MAIN entry's duration + sample
rate ⇒ audio length in ms. The most recent publication while at scene 25
is the highlighted song. Audio length is an UPPER bound for the option
ranges (audio ≥ chart content, research §8.3) — the UI clamp uses it;
the authoritative runtime clamp stays `chart_end_raw` at gameplay.
Publication must be detour-legal: atomics only, no logging, no
allocation beyond the header parse's, publish-nothing on parse failure
(rows stay unclamped; the runtime clamp still protects).

Bound resolution (design §4.2): `a_ms = min(skip_first·1000,
chart_end_raw − MARGIN)`; `b_ms = clamp(chart_end_raw − omit_last·1000,
a_ms + MIN_SECTION, chart_end_raw)`; both block-quantized through the
seek composition (`bounds::quantize_marker` ships in Step 2). Gestures
REFINE the row-derived values mid-play (Step 2's setters already do);
per the design, triple-5 now RESTORES the row-derived bounds instead of
clearing to none — a specified refinement of Step 2's clear semantics.
Side choice: the entered side's row values (doubles/solo — same
side-choice class as assist_tick's chosen side); versus never gets here
(the scene-26 classifier fails ineligible sessions closed, and bounds
only engage in armed sessions).

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-13-training-mode/design/detailed-design.md (§4.1 session-active predicate, §4.2 bounds, §4.6 publication, §5 SelectedSongInfo data model)

**Additional References (if relevant to this task):**
- docs/training_mode_research.md §8 (selection source, audio length via the wavebank hook, domain note)
- src/services/song_rate/wavebank_hook.rs (the create detour + `file_table_source`/`file_table_path`; publication lives beside `record_bank_event`)
- src/mods/training_mode/bounds.rs (marker state + `quantize_marker`; this task extends it)
- src/services/song_reset/mod.rs (`chart_end_raw(side)` accessor)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Publication cell (design §5): `{code_digest: u64, audio_len_ms: u32,
   generation: u32}` in atomics with a generation-based torn-read guard
   (write: bump-odd → fields → bump-even, or an equivalent seqlock;
   reader retries/rejects torn states). Published from the create detour
   for every slot-5 dance-bank create; parse failure ⇒ no publication
   (the previous publication stays). Detour-legal (no logging/alloc).
2. `bounds.rs`: an effective-clamp helper — the row's seconds capped at
   `audio_len_ms / 1000` when a fresh publication exists (pure fn,
   host-tested; applied at USE time — the row's stored value is not
   rewritten).
3. Gameplay-entry bound resolution (per side entered; latched once per
   song alongside the existing marker clearing): `MARGIN` = the existing
   1000 ms end-margin class; `MIN_SECTION = 5_000` ms (named constant —
   maintainer-approved recommendation); both bounds block-quantized via
   `quantize_marker`; resolved values stored as the session's row-derived
   `{a_ms, b_ms}` and logged (one INFO when nonzero — the Step-3 demo's
   "bounds visible in logs").
4. Marker interaction: gestures overwrite the live `a_ms`/`b_ms`;
   triple-5 restores the ROW-DERIVED values (0/none when the rows are 0 —
   Step 2's behavior degenerates correctly). `active_section_start()`
   returns the live `a_ms` (row-derived or gesture-set).
5. Session-active latch (design §4.1): rows > 0 at gameplay entry counts
   as training-session-active (alongside Step 2's gesture/seek
   condition) — the driver arm (task-03) and Step 5's taint consume it.
6. Host tests: effective-clamp truncation cases; bound resolution against
   synthetic `chart_end_raw`/row values (skip past end, omit past start,
   MIN_SECTION floor, zero rows ⇒ no bounds); publication torn-read
   guard (reader never observes a mixed generation). Mount any new pure
   module in the host harness.

## Dependencies
- task-01 (the rows + their per-side value accessors).
- Step 2 shipped (`quantize_marker`, `chart_end_raw`, marker state).

## Implementation Approach
1. Publication cell (pure struct + tests) → wire into the create detour.
2. Pure clamp + resolution fns (TDD) → gameplay-entry latch wiring in
   bounds.rs, triple-5 restore semantics.
3. Keep everything engine-facing behind the existing fail-open pattern
   (no publication ⇒ unclamped; no chart end ⇒ no row-derived bounds,
   gestures still work).

## Acceptance Criteria

1. **Publication on every dance-bank create**
   - Given any slot-5 dance-bank create (armed or stock, any rate)
   - When the detour runs
   - Then `{code_digest, audio_len_ms}` is published with a fresh generation, and a torn read is impossible (host-tested guard)
2. **Effective clamp**
   - Given SKIP FIRST 599 on a 90 s song (publication present)
   - When the bounds resolve
   - Then the effective skip caps at the audio length (and further at `chart_end_raw − MARGIN` at entry); with no publication the audio cap is skipped and the chart clamp still holds
3. **Bound resolution**
   - Given rows `{skip_first, omit_last}` and a live `chart_end_raw`
   - When gameplay entry latches the session
   - Then `a_ms`/`b_ms` match the design formula (MARGIN, MIN_SECTION floor, block-quantized) and are logged once when nonzero
4. **Gesture interplay**
   - Given row-derived bounds and a mid-song triple-4
   - When triple-5 fires afterward
   - Then the bounds return to the row-derived values (not to none), and with zero rows the Step-2 clear-to-none behavior is unchanged

## Metadata
- **Complexity**: Medium
- **Labels**: training-mode, song-rate, bounds, host-tested
- **Required Skills**: Rust, the wavebank-hook detour discipline, bounds/seek domain math
- **Generated By**: code-task-generator 2026-08-13
- **Source Plan**: .agents/planning/2026-08-13-training-mode/implementation/plan.md
- **Plan Step**: Step 3: Bound rows, session persistence, silent skip-first start
