# Plan — Task 03: tick clock + playback

**Status: Approved** (auto mode — approval inherited from the verified approved plan/design chain)

## Verification scenarios (log + maintainer's listening pass, per the task's ACs)

1. AC2/AC3: first-N-ticks debug lines carry scheduled/actual/delta; once-per-song line carries the
   observed frame delta and computed lead. Read from `log.txt` after install; report mean/spread.
2. AC5: cursor advance is a loop, play is a single call after it — by construction plus the debug
   lines (no two tick lines with the same `actual`).
3. AC6: rewind re-seek logs once (`AssistTick: clock rewind …`).
4. AC7: attract loop's multiple demo songs each produce their own build + tick lines; registration
   line appears once.
5. AC8: `git diff src/services/game_audio.rs` shows only the `demo` block + `demo::install();`
   removal.
6. AC1/AC4 (claps on the beat, chart-driven through misses): maintainer's listening pass.
7. AC10: cargo check / fmt / build.sh clean.

## Implementation shape

- `SongState` gains `cursor: usize`, `last_music_count: i32` (sentinel `i32::MIN`),
  `last_delta: i32` (0 = none yet), `ticks_logged: u32`, `lead_logged: bool`.
- Named constants: `REWIND_MS = 1000` (same threshold the Step-2 demo proved out),
  `LEAD_FALLBACK_MS = 8` (half a 60 fps frame), `DELTA_CLAMP_MS = 2..=34` (360 Hz … ~30 fps),
  `TICK_DEBUG_LOG_COUNT = 10`.
- `fn adaptive_lead(last_delta: i32) -> i32` — the one named place (design §4.2.3 rationale in its
  doc comment; promoting to an operator-tunable offset later is an addition).
- `tick_clock` after the rebuild branch: read handle; under the `SONG` lock do identity check →
  rewind guard (`partition_point` re-seek, log) → delta update → lead → advance cursor past all due
  timestamps via checked `get` → record the play decision + debug numbers; drop the lock; then
  `play_cue(handle, c"asti", 0.0)` (no lock across a game call, per design §5.2).
- Delete `mod demo` + its install call from `services/game_audio.rs`; scaffolding-only comment at
  the top of the block goes with it.
