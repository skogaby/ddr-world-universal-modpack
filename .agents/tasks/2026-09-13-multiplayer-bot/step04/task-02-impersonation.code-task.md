# Task: Impersonation flip/restore state machine (`impersonation.rs` + pure `session.rs`)

## Description

Implement the windowed impersonation (design §4.5, §5.1, §6; register D1–D3, D5, D7/D24,
D10): at the song-select → stage transition, when the eligibility gate passes, make the
non-entered side a genuine second player for ONE song — mirror the human's chart identity
into the bot's PlayerWork/record, copy the human's lane options (gauge forced NORMAL), write
the `BOT LV<n>` name plate, set `PlayerWork[bot]+0x4 = 1` and `GameWork+0x0 = 1`, taint the
bot side, arm the Step 3 controller — and undo all of it at the first scene change out of
the play window {26..=30}. Zero new detours; every address comes from `stage_records`.

## Background

The two load-bearing RE facts (design §1, research §1/§3): the GAMEPLAY loader
(`createNextSequence` case 0x1d) copies `PlayerWork+0x4` into the DPS ctor struct and the
DPS creates a `GamePlayActor` per side with `entered != 0`; every play-window reader of
`GameWork+0x0` is a display/layout selector (results panes, HUD, SE pan, save staging). The
song-select commit prepares BOTH sides' records but the non-entered side's difficulty comes
from its own unset cursor (research §4), hence the mirroring and the
`rec_bot+0x00 == rec_h+0x00` refusal. Scene callbacks fire BEFORE the original
`createNextSequence` runs, so a flip on the 25→26 edge lands before the 27/28 loaders read
`PW+0x4`. Task 01 supplied `mod::option_on(side)` / `mod::level(side)`; Step 3 supplied
`filler::{start_song, reset, summary, fill}` and `foot_panel_swap::{arm_bot, disarm_bot,
controller}`. The dev self-test already skips a side whose controller is `Bot` and that it
did not arm itself, so the two never both drive one side; the impersonation must run FIRST
in the shared scene callback.

## Reference Documentation

**Required:**
- Design: `.agents/planning/2026-09-13-multiplayer-bot/design/detailed-design.md` — §4.3
  (`enable`/`disable`, `init` gate incl. `player_option_offset().is_some()`), §4.5 (the state
  machine — steps 1–8 are normative), §5.1 (every offset), §6 (error table: undo-on-failure,
  20 s watchdog), §7.3 items 2–5, 7, 8 (cabinet pass), A.1–A.3, A.6, A.7.

**Additional References (if relevant to this task):**
- `.agents/planning/2026-09-13-multiplayer-bot/research/versus-impersonation-re.md` §1–§5,
  §9 (flip/restore recipe), §10 (build-invariant header offsets).
- `src/services/stage_records.rs` — `game_work`, `player_work`, `side_entered`,
  `stage_record`, `stage_counter`, `event_mode`, `course_field_offset`,
  `player_option_offset`, `MAX_STAGE_RECORDS`.
- `src/mods/two_player_bpl_mode/mod.rs` ~460–475 (gathering `GateInputs` from
  `stage_records` — the `Option<T>` shape `eligibility::Inputs` mirrors).
- `src/mods/premium_free/mod.rs` `virginize_frozen_stage_records` (guarded record-header
  writes), `src/mods/quick_restart_or_fail.rs` ~1324–1370 (render-thread self-requeue
  watchdog shape).
- `src/mods/multiplayer_bot/{self_test.rs, filler.rs, eligibility.rs, skill.rs}` and
  `src/services/foot_panel_swap/mod.rs` (public API).
- `.agents/learnings/learnings.md` "Per-side option values OUTLIVE the player" and
  "Anything that skips the stage bump inherits the stale-record bug".
- `tools/bot_sim/src/bot/mod.rs` (mount list — add the new pure file).

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. **Pure `src/mods/multiplayer_bot/session.rs`** (no `crate::` imports; mounted in
   `tools/bot_sim/src/bot/mod.rs`; host-tested):
   - Scene constants as 0-indexed literals with doc comments: `SONG_SELECT = 25`,
     `FLIP_TARGETS = [26, 27, 28]`, `GAMEPLAY = 28`, `PLAY_WINDOW = [26, 27, 28, 29, 30]`;
     `pub fn in_play_window(scene: i32) -> bool`.
   - `pub enum Edge { Flip, Reseed, Restore, None }` and
     `pub fn classify(prev: i32, next: i32, active: bool) -> Edge`: `!active && prev == 25 &&
     FLIP_TARGETS.contains(next)` ⇒ `Flip`; `active && !in_play_window(next)` ⇒ `Restore`;
     `active && next == 28` ⇒ `Reseed`; else `None`.
   - `pub const NAME_LEN: usize = 9;` `pub fn format_bot_name(level: u8) -> [u8; NAME_LEN]` —
     `b"BOT LV"` + decimal level (clamped 1..=10) + NUL padding; every level fits 8 chars + NUL.
   - Tests: every level's name (`BOT LV1\0…`, `BOT LV10\0`, all ≤ 8 chars before NUL, NUL
     present); `classify` truth table (flip only from 25 into 26/27/28 when idle; 25→24 no
     flip; active 28→27→28 (quick restart) yields `None` then `Reseed`; active →31 / →24 /
     →34 ⇒ `Restore`; idle → anything else ⇒ `None`); `in_play_window` bounds.
   - `impersonation.rs` `const _: () = assert!(session::GAMEPLAY == scene::GAMEPLAY && …)`
     pins the literals to `crate::types::scenes::scene`.
2. **`src/mods/multiplayer_bot/impersonation.rs`** (engine-facing):
   - `enum State { Idle, Active { bot: usize, human: usize, level: u8, snap: Snapshot } }`,
     `struct Snapshot { entered_byte: u8, name: [u8; 9], versus_word: i32 }`, in a
     `static STATE: Mutex<State>`; `pub fn active_bot_side() -> Option<usize>` (lock-free
     mirror in an `AtomicI32`, −1 = none — Step 5's detour reads it on the game thread).
   - `pub fn on_scene_change(prev: i32, next: i32)` — `classify` on the current state:
     - `Flip` ⇒ `gather_inputs()` → `eligibility::evaluate`:
       `Ok(plan)` ⇒ `apply(plan)`; `Err(Unavailable(what))` ⇒ one WARN per boot naming
       `what`; any other `Err(r)` ⇒ one INFO naming the refusal, only when some ENTERED side has
       the option ON (never log for `OptionOff`).
     - `Reseed` ⇒ `filler::start_song(bot, level, skill::seed(qpc, mcode, diff, level))` (fresh
       seed; mcode/diff read from `PW_h+0x54`/`+0x5C`, probed, 0 on failure) + INFO.
     - `Restore` ⇒ `restore()`.
     - While `Active`, also increment `SCENE_CHANGES` (the watchdog's signal).
   - `gather_inputs() -> eligibility::Inputs`: `entered = [side_entered(0), side_entered(1)]`;
     `style = game_work().map(read_i32(gw+0x4))`; `course_word =
     game_work().map(read_u64(gw + course_field_offset()))`; `event_mode =
     stage_records::event_mode()`; `versus = game_work().map(read_i32(gw+0x0))`;
     `option_on = [super::option_on(0), super::option_on(1)]`; `level = [super::level(0) as
     i32, super::level(1) as i32]`. Probe `gw` for `max(course_field_offset()+8, 0xD4)` bytes
     first; unreadable ⇒ the affected fields `None`.
   - `apply(plan)` — design §4.5 steps 1–8, EVERY pointer `memory::is_readable`-probed before
     the read/write (`pw_*`: `0x60` bytes; `opt_*`: `0x70` bytes; `rec_*`: `0x0C` bytes; `gw`:
     `0x08`), with an explicit undo list so any failure after a write restores what was
     written, WARNs once, disarms, and leaves `Idle`:
     1. `pw_h = player_work(human)`, `pw_b = player_work(bot)`, `stage = stage_counter()` (must
        be `0..MAX_STAGE_RECORDS`), `rec_h/rec_b = stage_record(side, stage)`, `opt_off =
        player_option_offset()`, `gw = game_work()` — any `None` ⇒ WARN + refuse.
     2. Refuse (WARN) if `read_i32(rec_b) != read_i32(rec_h)` (commit did not prepare the bot
        record) or `read_i32(rec_h) < 0`.
     3. Snapshot `*(pw_b+0x4)`, `pw_b+0xC..+0x15`, `*(gw+0x0)`.
     4. Mirror `pw_b+0x50/+0x54/+0x5C ← pw_h`; `rec_b+0x04/+0x08 ← rec_h`.
     5. Copy `0x68` bytes `opt_h+0x08..=0x6C → opt_b+0x08` (`opt = pw + opt_off`; never touch
        `+0x00`); write `opt_b+0x18 = 0` (NORMAL gauge).
     6. Write `session::format_bot_name(level)` to `pw_b+0xC` (9 bytes).
     7. `*(pw_b+0x4) = 1`; `*(gw+0x0) = 1`.
     8. `score_guard::set_autoplay_taint(bot, true)`; `filler::start_song(bot, level, seed)`;
        `foot_panel_swap::arm_bot(bot, filler::fill)` (false ⇒ undo everything, WARN);
        `STATE = Active`, publish `active_bot_side`, start the watchdog.
     INFO: `MultiplayerBot: side {bot} impersonated as "BOT LV{n}" for P{human+1}'s song
     mcode={} diff={} (sigma={:.1}ms p_miss={:.2}% seed={:#x})`.
   - `restore()` — idempotent (no-op when `Idle`): probe then write back `*(pw_b+0x4)`, the 9
     name bytes, `*(gw+0x0)` (unreadable ⇒ WARN, still proceed); `foot_panel_swap::disarm_bot`;
     `score_guard::set_autoplay_taint(bot, false)`; INFO with `filler::summary(bot)` (level,
     seed, planned/judged tallies, mismatches, frames — the self-test's line shape) then
     `filler::reset(bot)`; `STATE = Idle`, clear `active_bot_side`.
   - `pub fn on_song_reset(_t_ms: i32)` — while `Active`: `filler::start_song` with a fresh
     seed + INFO.
   - `pub fn shutdown()` — `restore()` (mod disable).
   - Watchdog (diagnostic only): at apply, bump `WATCHDOG_GEN` and self-requeue on the render
     thread (`widget_renderer::run_on_render_thread`, skip when
     `!frame_dispatch_available()`); every tick: newer generation or `Idle` ⇒ stop; `SCENE_CHANGES
     > 0` since apply ⇒ stop silently; > 20 s ⇒ one WARN `MultiplayerBot[watchdog]: no scene
     change 20 s after the flip (scene N) -- bot session may be stuck in the loader`.
   - No lock held across calls into `scene_manager`, `foot_panel_swap`, `filler`, or
     `score_guard`: take what you need from `STATE`, drop the guard, act, re-lock to store.
     `STATE` poison ⇒ treat as `Idle` + one WARN.
3. **`mod.rs` wiring**: the single scene callback calls `impersonation::on_scene_change(prev,
   next)` FIRST, then `self_test::on_scene_change`; the `song_reset` subscription calls both
   `on_song_reset`s; `disable` calls `impersonation::shutdown()` before `self_test::shutdown()`
   and the row hiding from task 01; `init` additionally requires
   `stage_records::player_option_offset().is_some()` (WARN "Option offset underived -- mod
   inactive" on miss). Module doc comment updated to the Step 4 state.
4. **Hook-path rules**: `on_scene_change` runs inside the scene hook — no `unwrap`/`expect`/
   unmasked indexing; sides always `< 2`; all game reads/writes through `core::memory` after
   `is_readable`. Nothing here runs per frame.
5. **Never**: write `PW+0x4` outside the flip/restore pair; copy the Option vtable; restore
   the mirrored chart/Option fields (design snapshot is the three items only); log per frame.

## Dependencies

- Task 01 (`super::option_on` / `super::level`), Step 3 (`filler`, `foot_panel_swap`,
  `self_test`), `stage_records`, `score_guard::set_autoplay_taint`, `scene_manager`,
  `song_reset::on_song_reset`, `widget_renderer::{run_on_render_thread,
  frame_dispatch_available}`, `core::memory::{is_readable, read_u8, read_i32, read_u64,
  write_u8, write_i32}` + a byte copy via `core::ptr::copy_nonoverlapping` on probed ranges.

## Implementation Approach

1. `session.rs` tests first (names, `classify`, window) → implement → add the mount line →
   `./scripts/validate_multiplayer_bot.sh`.
2. `impersonation.rs`: state + `gather_inputs` + `apply` with the undo list + `restore` +
   reseed + watchdog.
3. `mod.rs` wiring + `init` gate + doc comment.
4. `cargo check --target x86_64-pc-windows-msvc` → `cargo fmt` (both crates) → `./build.sh`
   → `./scripts/validate_multiplayer_bot.sh`.

## Acceptance Criteria

1. **Pure session facts pinned** — Given `session.rs` tests, When run, Then every level's
   name is `BOT LV<n>\0` within 9 bytes, `classify` matches the design's transitions
   (flip only on 25→{26,27,28} while idle; restore on any exit from {26..=30} while active;
   reseed on GAMEPLAY re-entry), and the literals equal `types::scenes::scene`.
2. **Full bot session (cabinet, maintainer — §7.3 items 2–5, 7, 8)** — Given BOT OPPONENT ON +
   L10 on P1, When a song is committed, Then the log shows `side 1 impersonated as "BOT LV10"`,
   two READY panels appear, P2's lane scrolls at P1's speed/skin, the name plate reads
   `BOT LV10`, the bot is mostly Marvelous, two results panes render, TOTAL RESULTS shows P1
   only, and the restore INFO with the tally follows scene 30's exit with no WARN; P1's per-stage
   save proceeds and no side-1 `save_sender` reaches the wire. L1 ⇒ a visibly failing bot.
   Human on P2 ⇒ `side 0 impersonated`. Quick restart ⇒ a re-roll INFO with a new seed; quick
   fail ⇒ restore + a 1P song select; option OFF ⇒ next song plain 1P; real 2P / doubles /
   course ⇒ no flip and one INFO naming the refusal.
3. **Undo-on-failure** — Given a probe fails mid-`apply` (simulate by review), When `apply`
   returns, Then every already-written byte is back to its snapshot, one WARN, state `Idle`, no
   controller armed, no taint.
4. **Self-test coexistence** — Given dev mode + `DDR_BOT_SELF_TEST` + a bot session, When
   GAMEPLAY is entered, Then the bot side is driven by the impersonation only (no double arm)
   and the human's side by the self-test.
5. **Build gates** — `cargo check` clean, `cargo fmt` (both crates), `./build.sh` clean,
   `./scripts/validate_multiplayer_bot.sh` green (new `session` tests included), no local paths.

## Metadata
- **Complexity**: High
- **Labels**: game-memory, scene-manager, state-machine, multiplayer-bot
- **Required Skills**: Rust unsafe memory access, this repo's `stage_records`/`scene_manager`/`memory` conventions
- **Generated By**: code-task-generator 2026-09-13
- **Source Plan**: `.agents/planning/2026-09-13-multiplayer-bot/implementation/plan.md`
- **Plan Step**: Step 4: Impersonation flip/restore, option rows, textures, menu placement — full bot session
