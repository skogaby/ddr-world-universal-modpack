# Task: Documentation Rewrite and Leftover Sweep

## Description

Bring every shipped document current with the streaming redesign as
actually built through plan Steps 1–6, and sweep the retired disk-cache
era's leftovers. The README and AGENTS.md still describe the RETIRED model
(on-demand generation with a 30 s bound, memory/duration admission, disk
cache under `cache_limit_gib`); the assist-tick row still says 300 s and a
content-only formula; the design doc still says both bank entries are
stretched (superseded by the maintainer-approved preview passthrough); and
the movie-hook custom instruction still says song-rate suppression "remains
false through identity-only Step 3". Plan Step 7's documentation half.

## Background

What shipped, per the working records:

- **Streaming model (Steps 1–5):** no disk cache, no worker deadline, no
  admission — the rate-adjusted bank is a VIRTUAL bank streamed through
  detoured XACT file-IO callbacks (`binding.rs` ring + side buffer,
  `generator.rs` producer, `io_callback_hook.rs` detour pair,
  `core/xact/virtual_bank.rs` layout/resolve), with the Q31 clock commit
  LAST. Live-proven: ~5 s normal loading at 25 %/175 %, a full 8.5-min 25 %
  song with 0 deferrals. `cache_limit_gib` was dropped OUTRIGHT (register
  decision D9): the config struct keeps only the parse-but-ignore
  `diagnostic` key (`src/mods/config.rs::SongPlaybackSpeedConfig`), and the
  repo `mod-config.json` no longer carries the block (verified).
- **Preview passthrough (Step 5, maintainer-approved deviation):** the
  non-main `<code>_s` entry is NOT stretched — stock header values, bytes
  served verbatim from the resident source copy (WSOLA at 47 kHz is only
  ~2.4× realtime under CrossOver; stretching the preview cost 23–25 s of
  loading). The song previews at song select therefore play at NORMAL
  speed. Emission follows the PARSER's layout rule (stock-shaped durations
  inside the final block are legal). Record:
  `.agents/planning/2026-08-08-song-rate-streaming/implementation/step05-fix-preview-side-buffer/progress.md`.
- **Step 6 dependent features:** assist-tick content→wall conversion
  (`song_rate/tick_domain.rs`; `TICK_CAPACITY_MS` 300 s → 1200 s wall;
  scaffold gate removed); rate-aware Real Speed
  (`song_rate/real_speed.rs` — the recompute writes the GamePlayActor's
  multiplier cluster `+0x290/+0x294/+0x29C` at the first judge dispatch,
  owned by the song_playback_speed mod, independent of the Real Speed Fix
  toggle); PUS CSV rate columns (`RateSnapshot::csv_rate_cells`,
  latched with the song identity).
- **Fresh RE worth a durable home:** the Real Speed consumer chain
  (Option speed-type/target/core offsets; the actor's per-frame
  renderer-speed copy from its latched floats) lives only in
  `.agents/planning/2026-08-08-song-rate-streaming/implementation/step06-task-02-real-speed-effective-rate/context.md`.

Approved documentation decisions (maintainer, 2026-08-11): design-doc
corrections as DATED inline amendment notes (never silent rewrites of an
Approved doc); `docs/song_playback_speed.md` gets a supersession banner +
targeted fixes only (body stays historical); the Real Speed RE lands as a
dated section appended to `docs/song_playback_speed.md`; README covers the
Step-6 features as brief in-row mentions.

## Reference Documentation

**Required:**
- Design: `.agents/planning/2026-08-08-song-rate-streaming/design/detailed-design.md`
  (the doc being amended; §Dependent features, the virtual-bank component
  spec around "Both entries are stretched", req 14's neighborhood)

**Additional References (if relevant to this task):**
- `.agents/planning/2026-08-08-song-rate-streaming/progress.md` — the
  authoritative what-shipped record (Done entries for Steps 1–6, deploy log)
- `.agents/planning/2026-08-08-song-rate-streaming/implementation/step05-fix-preview-side-buffer/progress.md`
  — preview passthrough + parser-rule emission findings
- `.agents/planning/2026-08-08-song-rate-streaming/implementation/step06-task-02-real-speed-effective-rate/context.md`
  — the Real Speed RE chain to be made durable
- `docs/xact_streaming_research.md` — the streaming RE note to refresh
- `docs/xact_audio_research.md` — the tick-track model (assist-tick row
  wording)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. **README.md:** rewrite the Song Playback Speed feature-table row —
   streaming description (no generation pause: a cold non-100 % load adds a
   few seconds on the loading screen, live-measured ~5 s; no disk cache /
   no `cache_limit_gib`; preview passthrough = song-select previews play at
   normal speed), score containment unchanged, brief mentions of the Step-6
   integrations (assist-tick claps stay judgment-aligned at any rate; Real
   Speed reads the effective tempo; the PUS CSV gains requested/effective
   rate columns). REMOVE the "Song Playback Speed cache" config section and
   the `"song_playback_speed"` block in the embedded mod-config example
   (only the retired parse-but-ignore `diagnostic` key exists — no operator
   keys remain; a leftover `cache_limit_gib` in a user config is silently
   ignored by serde).
2. **AGENTS.md:** rewrite the Song Playback Speed row to the streaming
   architecture (module pointer set: `lifecycle`/`runtime`,
   `binding`+`generator`+`io_callback_hook`, `core/xact/virtual_bank` +
   `stretch::StretchState`, `transaction` commit order Q31-LAST,
   two-region serving + preview passthrough, `tick_domain` + `real_speed` +
   `csv_rate_cells` integrations, validator, backend note, "no config
   keys — retired `cache_limit_gib` dropped outright / `diagnostic`
   parse-but-ignore"). Update the Assist-tick row: 1200 s wall capacity
   (D15), the rate-aware FR-3 formula
   (`content_to_wall_ms(t + SIGN·JT − m0) − SOUND_OFFSET` via the
   AwaitAnchor-latched snapshot), restart skips converted, scaffold gate
   gone. Fix the stale movie-hook custom instruction (line ~186): song-rate
   movie suppression is LIVE (tentative at non-identity arm, confirmed at
   commit) — no longer "false through identity-only Step 3".
3. **`docs/xact_streaming_research.md`:** append the implementation-time
   findings — preview passthrough (the WSOLA ~2.4× realtime measurement and
   the two-region serving model: side buffer for the non-main entry, ring
   for the main), and the parser-rule emission contract (stream-layout
   validation accepts stock-shaped final-block durations; every fixture
   made honest). Refresh the cross-version table only if any entry is now
   known stale.
4. **`docs/song_playback_speed.md`:** supersession banner at the top
   (points at `docs/xact_streaming_research.md` + the streaming feature;
   body remains the historical pre-pivot record) + targeted fixes ONLY
   where it asserts retired behavior as current. Append a dated
   "Rate-aware Real Speed (2026-08-11)" section carrying the durable RE:
   Option layout (+0x8 type / +0xC fixed / +0x10 derived / +0x14 target /
   +0x80/+0x88/+0x90 BPM doubles; vtable+0x218 active-multiplier getter;
   SetBPMs→SetScrollSpeed re-derivation), the GamePlayActor multiplier
   cluster (+0x290/+0x294/+0x29C; per-frame renderer copy), the write site
   (first judge dispatch), and the cross-build byte check (20260721 +
   20260616).
5. **Design amendments (dated inline notes, per approval):** at the
   virtual-bank spec's "Both entries are stretched at the same percent"
   (~line 430) and any sibling statement found during implementation
   (search "both entries"/req 14's stretched-duration wording) — e.g.
   "[Amended 2026-08-11: shipped as PREVIEW PASSTHROUGH — the non-main
   entry keeps stock header values and is served verbatim; see
   implementation/step05-fix-preview-side-buffer/]". Do not silently
   rewrite approved text.
6. **Sweep:** `grep` proves no `cache_limit_gib` reference remains outside
   `.agents/` planning records; no shipped doc (README, AGENTS.md,
   `docs/`) states the disk-cache/30 s/admission model as current; the
   repo `mod-config.json` stays clean (already verified — assert, don't
   edit); `README`'s retired-key note (`diagnostic`) preserved wherever the
   config docs land.
7. Docs-only change set — no `src/` or `scripts/` edits. All five standing
   gates re-run anyway (the standing contract; also proves the tree still
   builds after the sweep).

## Dependencies

- Plan Steps 1–6 complete (they are — records under
  `.agents/planning/2026-08-08-song-rate-streaming/implementation/`).

## Implementation Approach

1. Read the feature `progress.md` Done entries + the step05/step06 records;
   draft the two AGENTS.md rows and the README row from them (never from
   the pre-pivot design).
2. Apply README/AGENTS edits; then the two `docs/` files; then the design
   amendment notes.
3. Run the sweep greps; fix what they surface.
4. Full gate set; record per the planning-directory convention (NEVER
   `.agents/scratchpad/`).

## Acceptance Criteria

1. **Shipped docs describe the shipped system**
   - Given README.md and AGENTS.md after the rewrite
   - When read against the feature progress.md's Done entries
   - Then no retired-model claim remains (no on-demand cache, no 30 s
     bound, no admission, no `cache_limit_gib`; assist-tick says 1200 s +
     rate-aware conversion; the movie-hook instruction reflects live
     suppression), and the Step-6 features are covered

2. **The RE record is durable**
   - Given `docs/song_playback_speed.md` and `docs/xact_streaming_research.md`
   - When a future agent needs the Real Speed chain or the preview
     passthrough rationale
   - Then dated sections in `docs/` carry them (with pointers back to the
     planning records), and the old doc opens with a supersession banner

3. **Design corrections are amendments, not rewrites**
   - Given the design doc
   - When the "both entries stretched" statements are located
   - Then each carries a dated amendment note naming the shipped behavior
     and the deciding record, with the original text intact

4. **The sweep is clean**
   - Given `grep -rn cache_limit_gib` over the repo excluding `.agents/`
   - When run after the edits
   - Then zero hits; and `mod-config.json` carries no `song_playback_speed`
     block

5. **Tree is green**
   - Given the completed task
   - When running the five standing gates
   - Then all pass with the windows check at 0 warnings

## Metadata

- **Complexity**: Medium
- **Labels**: song-rate, documentation, sweep, release-prep
- **Required Skills**: technical writing against working records, the
  song-rate streaming architecture, repository doc conventions
- **Generated By**: code-task-generator 2026-08-11
- **Source Plan**: `.agents/planning/2026-08-08-song-rate-streaming/implementation/plan.md`
- **Plan Step**: Step 7: Hardening, documentation, and the release matrix
