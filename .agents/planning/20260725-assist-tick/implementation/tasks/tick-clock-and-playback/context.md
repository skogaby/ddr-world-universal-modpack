# Context — Task 03: Tick Clock — Cursor, Adaptive Lead, Playback

**Task file:** `.agents/tasks/20260725-assist-tick/step03/task-03-tick-clock-and-playback.code-task.md`
**Mode:** auto. Approval chain verified (same as tasks 01/02). Dependency task 02 is Complete.

## Requirements (task file + design §4.2 pseudocode, §4.2.3, §5.2, §6)

1. Judge-callback body per design §4.2: rebuild branch (exists) → identity check against latched
   side → rewind guard → `last_music_count` update → adaptive lead → cursor advance with a single
   `play_cue`.
2. **Adaptive lead** in one named place, with the frame-rate reasoning at its definition: half the
   observed `music_count` delta between the latched side's judge dispatches, clamped to a sane
   range, fixed fallback (half a 60 fps frame) before any delta exists. FPS-unlock means the frame
   period is not a constant.
3. When ticks are due: advance the cursor past **every** due timestamp, play exactly **one** clap
   (FR-4 — implemented here, not Step 4, per the approved decomposition decision #4).
4. Rewind guard: a drop in `music_count` beyond `REWIND_MS` re-seeks the cursor by binary search
   (`partition_point` over the sorted list) — quick restart resumes from the top. Log the re-seek.
5. Playback: `game_audio::play_cue(handle, c"asti", 0.0)` — always centre-panned (FR-6).
6. **Delete Step 2's scaffolding**: the whole `mod demo` block in `src/services/game_audio.rs` AND
   the `demo::install();` call at the end of that file's `init`. Nothing else in that service
   changes.
7. Measurement logging: first N ticks per song at **debug** — scheduled timestamp, actual
   `music_count`, delta; plus observed frame delta + computed lead once per song. Nothing per frame
   after the first N, nothing unbounded.
8. Per-frame path O(1) and allocation-free once the list exists.
9. Panic-free; timestamp list indexed via checked access only (`get`).
10. `play_cue == false` → no per-tick warn (the service warns once), no cursor stall/rewind.
11. No new crate dep, no config section, no new detour.

## Key facts

- `music_count` rises monotonically within a song, starts negative (~-87). A drop beyond the
  threshold means restart/new song. The rewind guard is belt-and-braces — the scene callback is the
  primary reset (research: quick-restart handling).
- Design §5.2: `SongState { tick_side, times, cursor, last_music_count, last_delta,
  rebuild_pending }` — no lock held across any game call, so the play decision is computed under
  the `SONG` lock and `play_cue` is called after it is released.
- Re-seek semantics: `cursor = times.partition_point(|&t| t <= music_count)` — everything at or
  before "now" is treated as passed; at restart `music_count` is negative so this is 0.
- The delta used for the lead must come only from the **latched side's** consecutive dispatches
  (the callback receives both sides each frame), so it is updated after the identity check.

## Build/test

Gates as before. Runtime verification: install the DLL, drive the game via `scripts/game_nav/`
(attract demo exercises real gameplay), read `log.txt`; the listening pass is the maintainer's.
