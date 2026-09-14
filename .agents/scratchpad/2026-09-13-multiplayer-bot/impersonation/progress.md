# Progress — impersonation

Status: Complete (uncommitted — maintainer commits manually)

## Checklist
- [x] `session.rs` tests (red — module absent) + mount line in `tools/bot_sim/src/bot/mod.rs`
- [x] `session.rs` implementation (green — 73 tests)
- [x] `impersonation.rs`
- [x] `mod.rs` wiring + init gate + docs
- [x] Gates: `cargo check` → `cargo fmt` (both) → `./build.sh` → `./scripts/validate_multiplayer_bot.sh` (73/73)

## Log
- `session.rs` (pure): `SONG_SELECT`/`GAMEPLAY`/`FLIP_TARGETS`/`PLAY_WINDOW`/`NAME_LEN`,
  `in_play_window`, `Edge { Flip, Reseed, Restore, None }`, `classify(prev, next, active)`,
  `format_bot_name(level) -> [u8; 9]` (allocation-free digit split, clamps). 6 tests
  (names fit 8+NUL, clamps, flip edges, restore edges, reseed incl. the quick-restart 28→27→28
  hop, window bounds). Mounted; harness 67 → 73 green.
- `impersonation.rs`: `const` asserts pin the literals to `types::scenes::scene`; offsets as
  named consts; `State { Idle, Active }` in a `Mutex` (poison recovered via `into_inner` — plain
  data), lock-free `ACTIVE_BOT` mirror → `active_bot_side()`; `on_scene_change` = `classify` +
  dispatch (never holds the lock across a service call); `gather_inputs` (GameWork probed for
  `max(course_off+8, 0xD4)`); `resolve_ptrs` (7 probes, each miss WARNs by name); `apply` = §4.5
  steps 1–8 with `Written` capturing EVERY written byte (3 snapshot items + the mirrored chart
  fields + the 0x68 Option bytes) so a controller-arm failure `undo`es all of it; `restore` writes
  back only the three snapshot items (design), disarms, re-syncs the autoplay taint to
  `controller(bot) == Perfect` (a cached autoplay ON keeps its taint), tally INFO, `filler::reset`;
  `reseed` on GAMEPLAY entry / song reset with `skill::seed(qpc, mcode, diff, level)`; 20 s
  render-thread watchdog (`WATCHDOG_GEN` + `SCENE_CHANGES`, diagnostic WARN only).
- `mod.rs`: `pub mod impersonation; pub mod session;`; scene callback runs `impersonation` FIRST
  then `self_test`; song_reset closure calls both; `disable` → rows hidden → `impersonation::shutdown()`
  → `self_test::shutdown()`; `init` additionally requires `player_option_offset().is_some()`.
- `self_test::qpc` promoted to `pub(super)` (shared seed source).
- Renamed the watchdog's `gen` local to `generation` (reserved keyword under 2024-edition parsing;
  the crate is 2021 but rust-analyzer flagged it).
- Gates: `cargo check` clean (no warnings); `cargo fmt` both crates; `./build.sh` clean (57 s);
  harness 73 passed.

## Deviations
- **Failure-path undo is broader than the design's snapshot.** §4.5 snapshots only `PW+0x4`, the
  name and `GameWork+0`; the task asked that "any failure after a write restores what was
  written", so `apply` additionally captures the mirrored `PW+0x50/+0x54/+0x5C`, `rec+0x04/+0x08`
  and the 0x68 Option bytes and puts them back ONLY on the arm-failure path. The normal `restore()`
  is exactly the design's three items. No interface/behaviour change.
- **Restore re-syncs the autoplay taint to autoplay's own state** instead of blindly clearing it
  (the self-test clears blindly). Conservative: a cached `autoplay = ON` on the bot side keeps the
  taint autoplay itself would have set.

## Cabinet log lines (maintainer)
- Flip: `MultiplayerBot: side 1 impersonated as "BOT LV10" for P1's song mcode=… diff=… (sigma=…ms p_miss=…% seed=0x…)`
  preceded by `MultiplayerBot: filler start side=1 level=10 …`.
- GAMEPLAY entry / quick restart / song reset: `MultiplayerBot: side 1 re-rolled on GAMEPLAY entry (seed=0x…)`.
- Exit of the window: `MultiplayerBot: side 1 restored (BOT LV10 seed=0x…) planned marv=… | judged … | mismatch=0 frames=…`.
- Refusals with the option ON: `MultiplayerBot: not engaging this song -- NotExactlyOneEntered|NotSingle|Course|EventMode|AlreadyVersus`.
- Any WARN from this module is a fail-open refusal (named pointer/gate) or the watchdog.
