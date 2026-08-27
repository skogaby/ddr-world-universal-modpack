# Plan — Task 02: assist_tick skeleton + tick-list build

**Status: Approved** (auto mode — approval inherited from the verified approved plan/design chain)

## Verification scenarios (no unit harness; log-driven per the task's ACs)

1. AC1: boot log shows `Mod registered: Assist Tick (assist-tick)`, `Mod enabled`, bank-bytes-loaded
   line, no warning.
2. AC2 (negative): with bank files renamed away the mod declines with one warning naming the path —
   exercised once against the local install (agent's share of negative-path checks).
3. AC3: entry/exit lines exactly once per song, and by construction nothing reads an actor in the
   scene callback.
4. AC4/AC5/AC6: once-per-song build line with latched side, entry count, kept count, first
   timestamps (non-negative, strictly increasing after sort+dedup); one line per song; registration
   line only on the first (the service's idempotence, observed in Step 2).
5. AC7: malformed range → `for_each_result` returns nothing → kept=0 → "inert" log line once.
6. AC8: no `play_cue` call in the mod; `demo` module untouched.
7. AC9: no per-frame logging; per-frame path = one mutex lock + a couple of integer checks.
8. AC10: cargo check / fmt / build.sh clean.

Runtime log readings are batched with task 03's install (single boot serves both; the deliberate
negative path AC2 gets its own boot), consistent with the step-level verification split.

## Implementation shape

`src/mods/assist_tick.rs`:

- Constants: `XWB_REL`/`XSB_REL` (`banks/tick.{xwb,xsb}`), `BANK_NAME = "asti"`, `SIDE_NONE = -1`,
  `LOG_FIRST_TIMESTAMPS = 8`, `ACTOR_PLAY_SIDE_OFFSET = 0x84` (autoplay's documented constant).
- Statics: `BANK_BYTES: Mutex<Option<(Vec<u8>, Vec<u8>)>>`; `SONG: Lazy<Mutex<SongState>>` with
  `SongState { tick_side: i32, times: Vec<i32>, rebuild_pending: bool }`.
- `fn on_scene_change(prev, next)` logic in a plain fn called from the boxed closure.
- `fn tick_clock(actor: *mut u8, music_count: i32)` — judge pre-callback at Normal priority:
  rebuild branch (register bank → latch side → build list → log once), then latch identity check.
  Task 02 body ends there.
- `fn build_tick_list(actor) -> (entries, times)` — unsafe walk via the hoisted helpers; collect
  non-negative `music_count`; sort_unstable; dedup.
- `AssistTickMod { scene_cb_id: Option<usize>, judge_handle: Option<CallbackHandle> }` implementing
  `Mod` per design §4.2's lifecycle table; `required_signatures()` = `&[]` (prerequisites are
  services, gated in `init`).
- Registration: `pub mod assist_tick;` alphabetically in `mods/mod.rs` + doc bullet; constructor in
  `lib.rs`'s mod list.

Risks: none novel — every mechanism is an existing in-tree pattern (autoplay, PUS, the demo block).
