# Progress — Task 02: assist_tick skeleton + tick-list build

- [x] `src/mods/assist_tick.rs` — module doc, constants, statics (`BANK_BYTES`, `BANK_HANDLE`,
      `SONG: Lazy<Mutex<SongState>>`)
- [x] `Mod` impl per design §4.2 lifecycle table (`init` gates on the three services + loads bank
      bytes; `enable` = judge pre @ Normal + scene subscription; `disable` unregisters both, clears
      song state, banks deliberately left in place)
- [x] Scene callback: entry → clear + arm rebuild (also covers quick restart's re-entry); exit →
      clear. No actor touched (none exists yet)
- [x] `tick_clock`: rebuild decision under the lock, build with the lock released; no playback, no
      latch consumption yet (task 03's)
- [x] `rebuild_for` / `build_tick_list`: register bank (idempotent, handle stashed), latch side
      (`actor+0x84`, doubles→0), walk `types::game_note::{actor_results_range, for_each_result}`,
      keep every `music_count >= 0`, sort_unstable + exact dedup; empty list → warn once, inert song
- [x] Once-per-song diagnostic: `AssistTick: song build -- side= results= kept= first=[…]`
- [x] Registered: `pub mod assist_tick;` + doc bullet in `mods/mod.rs`; constructed in `lib.rs`
- [x] Gates: `cargo check` exit 0, `cargo fmt` clean, `./build.sh` exit 0 (logs/)

## Decisions (auto mode)

- `required_signatures()` = `&[]`: the prerequisites are *services* (game_audio owns the audio
  signatures, judge_hook owns judge_notes), gated in `init` with one warning naming what's missing —
  matches the task's requirement 2 wording exactly.
- Bank bytes cloned per registration attempt (service note: idempotent, repeat calls hit the cheap
  name-lookup path; also what makes "registered exactly once" observable in the log).
- `tick_clock` ends after the rebuild branch in this task — the latch identity check is only
  meaningful with the cursor, so it lands with it in task 03 rather than as an empty `if` here.

## Verification

- Source-inspection ACs (1, 8): no `play_cue` call anywhere in the mod; `game_audio.rs`'s `demo`
  block untouched (`git diff src/services/game_audio.rs` is empty).
- Runtime/log ACs (1–7, 9): read in one combined boot after task 03 (single install serves the whole
  step); AC2's deliberate negative path (banks renamed away) gets its own boot then.
- No per-frame logging; steady-state per-frame path = null check + one mutex lock + one bool read.

No deviations from the approved design. Commit deliberately not made (maintainer owns commits).

Status: Complete
