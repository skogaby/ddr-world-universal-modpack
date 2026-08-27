# Rough Idea: Song Playback Speed — Streaming-Only Rate Engine

Captured 2026-08-08 from the maintainer's pivot decision + the handoff brief. This
supersedes the audio-generation internals of the approved
`.agents/planning/2026-08-05-song-playback-speed/design/detailed-design.md` (its
policy/score/clock/option/backend sections remain accurate KEEPERS).

## Objective

Redesign the Song Playback Speed feature's audio delivery from
**whole-file up-front generation + on-disk LRU cache** to a
**streaming-only engine**: synthesize the time-stretched (WSOLA, pitch-preserved)
MS-ADPCM audio incrementally as the game's XACT engine consumes it, behind the
unchanged option/policy/score surface.

## Why (maintainer decision, 2026-08-08 — hard constraints)

All cabinet-observed against the old model:

- 25 % on a ~129 s song refused up front on the 128 MiB transform admission
  ceiling (whole-song workspace ≈ 148 MB at 4× output). Arbitrary song length
  MUST work.
- The 50 % cold build took 22.4 s of the 30 s worker deadline on the
  maintainer's MacBook; real cabinets are significantly slower (4–8 GB RAM).
  The loading-screen stall is unacceptable there. 25 % would blow the deadline
  (~45 s extrapolated — an ESTIMATE, not measured).
- The on-disk cache has been an unwanted design element all along.

The old model is rejected for release AND rejected as a fallback. Remove ALL of:
the on-disk cache, cache keys/invalidation, eviction/leases/quarantine
tombstones, the `cache_limit_gib` config, the 30 s generation deadline, the
memory admission ceiling, whole-song up-front generation. NO fallback to the
old model.

## Feasibility basis (RE facts already pinned — verify, don't re-derive)

Cabinet-proven 2026-08-07, recorded in
`.agents/planning/2026-08-05-song-playback-speed/progress.md` Deploy & test log:

- For slot-5 (per-song dance) banks, the game loads the whole XWB into a
  FileManager RAM buffer at song confirm (via `avs_fs_open`/`avs_fs_read`), and
  the engine's registered XACT file-read callback — gamemdx `FUN_1801aa250`
  (build 20260721; the binary's only other `ReadFile` user) — services every
  streaming read by memcpying the requested (offset, length) from that RAM
  buffer (`[DAT_1806f2f48+8] + file_id*0x40 + 8`, size @+0x14), faking the
  OVERLAPPED result. The streaming CreateFileA handle is opened, held, and
  NEVER read. `wavebank_create` (+0x1AB050) inserts `{win32_handle → file_id}`
  into a sorted vector at `manager+0x419*8` (slot-5 gated) — the handle→file
  lookup key.
- So XACT's consumption is already incremental. The streaming design: detour
  the read callback (or own its lookup); for a rate-bound bank serve (a)
  header/metadata-region reads from a synthesized XWB header carrying the
  stretched entry durations (exact rate math only — milliseconds, no DSP), and
  (b) data-region reads from a sequential WSOLA generator. Everything not ours
  passes through untouched.
- No retail seek path: BGM start is sound-bank cue Prepare→IsPrepared→Play from
  offset 0; the only jump target is the loop start. (Read patterns and packet
  sizes still need verification during research.)
- Throughput margin: 11.39 M output frames took 22.4 s inside the game process
  under Wine ≈ 11× realtime (need ≥ 1× + pre-roll). Cost per OUTPUT second is
  flat across rates. Cabinet CPUs are slower — benchmark the incremental path.
- MS-ADPCM blocks are self-contained (shifted concatenation exact), so
  per-block encode-on-demand is sound.
- The song-select preview player uses the same file/slot-5 machinery
  (`FUN_18010eab0`, cue `<code>_s`) — no rate armed at select time ⇒ the detour
  must pass it through stock.

## Keepers (design around them)

- `src/mods/song_playback_speed.rs` — the `song_speed` scalar row (25–175 %
  step 5, coarse 10, default 100, `PersistMode::Full` → wire `mod_song_speed`,
  `snap_rate_percent` load transform, availability APIs, per-side
  desired-percent atomics).
- The bemani-buddy backend (sibling repo): `mod_song_speed` end to end, stored
  VERBATIM (no server-side range validation — settled). Uncommitted in that
  repo; untouched by the redesign.
- Step-4 machinery: `src/services/song_rate/lifecycle.rs` (scene-26 arm
  eligibility: ordinary solo/doubles; versus/course/unknown fail closed), the
  Q31 clock patch (`src/services/song_rate/clock_patch.rs`, wall-driven clock,
  commit Q31 LAST), `src/services/score_guard.rs` pending rate-save ledger
  (per-stage saves of rate-played songs suppressed, card-out logout save
  score-stripped).
- Pure DSP/format foundations in `src/core/xact/`: XWB parse/serialize,
  MS-ADPCM codec, exact reduced-rate math (`rate.rs`), and the WSOLA stretch
  CORE (`stretch.rs`) — the stretch needs an incremental reformulation
  (currently whole-buffer with fixed endpoint anchoring; the streaming version
  must still hit the exact declared length and restart at the stretched loop
  point).

## Removed by the redesign

`src/services/song_rate/cache.rs`, most/all of `worker.rs` (store, coordinator,
leases, eviction, quarantine, admission, deadline), the two-stage file redirect
in `conversion.rs` (open-time redirect + generated-path exposure seam), the
`cache_limit_gib` config, the cabinet cache directory.

## Open design questions the research/design must answer

- Where the commit authority moves (likely: callback binding = the audio
  authority; Q31 commits only after binding succeeds).
- What `wavebank_create`'s transaction becomes.
- Pre-roll amount during cue Prepare.
- Underrun policy (a mid-song synthesis failure is now an audible event —
  the old model failed CLOSED before audio started).
- Whether the generator reads source bytes from the stock FileManager RAM copy
  or from disk.
- Threading (synthesize in the callback vs a producer thread + ring buffer).
- Host validation shape (the validator's `cache` and `on_demand` sections
  describe machinery that will no longer exist — schema evolution for
  `scripts/validate_song_playback_speed.sh` is a design decision; the existing
  harness pattern of mounting real sources via `#[path]` is very reusable for
  a streaming synth driven by synthetic read patterns).
- What remains of `DDR_SONG_RATE_FAULT` fault injection.

## Settled decisions — do NOT relitigate

- Scalar domain 25..=175 step 5 default 100.
- Streaming-only; no cache; no up-front-generation fallback.
- No server-side validation of `mod_song_speed`.
- No latency knob for audio features.
- Score containment semantics as shipped in Step 4.

## Process constraints

- Design only — implementation follows maintainer approval of design + plan.
- No deployment or cabinet testing during PDD; the maintainer re-tests
  everything (including >100 % rates) against the reworked feature.
- Detours never log or allocate (repo law); the read-callback detour is the
  hottest path this project has ever hooked — audio-thread budget.
- One detour per target function (shared-dispatcher pattern if a second
  consumer ever needs `FUN_1801aa250`).
- Three allocator heaps (CRT / AGCS / VirtualAlloc) — see
  `.agents/summary/data_models.md`.
- The clock is WALL-driven; the Q31 patch semantics are proven — don't
  redesign the clock side.
- The stretched XWB entry duration field is 28-bit — the header synth must
  respect it.
- The research phase output should become the durable `docs/` song-rate RE
  note (deferred from the old plan's Step 8) rather than more scattered
  planning-dir findings.

## Ghidra anchors to start the callback research (build 20260721)

Read callback `FUN_1801aa250`; `wavebank_create` +0x1AB050; unregister
+0x1AB3D0 (closes the handle synchronously); slot dispatcher `FUN_1801aa3c0`;
FileManager xwb callback `FUN_1801ac650` (returns NULL for slot 5 ⇒ whole-file
RAM load); file-table find-or-add `FUN_1801fef30`; preview player
`FUN_18010eab0`. Ghidra instance: `DDRWorld_Ghidra` (always pass `program`
explicitly).
