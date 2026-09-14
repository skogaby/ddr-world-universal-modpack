# Task: Bot controller — `BotFootPanel` + cloned vtable, `filler.rs`, mod skeleton, dev self-test

## Description

Make the `foot_panel_swap` service's `Bot` controller real (design §4.1 internals, §5.2),
add the engine-facing filler that feeds the DLL's pure planner from a live `GamePlayActor`
(§4.8), register a `MultiplayerBotMod` skeleton (§4.3 minus option rows/impersonation), and
add a dev-mode self-test that arms the bot on the HUMAN's own entered side so the cabinet can
prove the judge grades a DLL-owned panel exactly where the planner said — before any player
entry is touched.

## Background

Step 1 left `foot_panel_swap::swap_in`'s `Bot` arm as a WARN + `Off`. The design's
load-bearing controller fact (A.5): the judge computes `event = mc − getPressAge(panel)`,
so a panel object whose `getPressAge` returns `CURRENT_MC − event_mc[panel]` places every
graded event exactly at the planner's `E` on every build. Step 2 delivered the pure
`planner::plan_frame` (with `NoteView.kind`, panel reservation, one-event-per-panel-per-
frame) and `tools/bot_sim`, whose judge model reports `mismatches` — the same
planner-vs-judge check this step ports to the cabinet.

## Reference Documentation

**Required:**
- Design: `.agents/planning/2026-09-13-multiplayer-bot/design/detailed-design.md` — §4.1
  (Bot vtable/slots/arbitration), §4.3 (mod skeleton: init gates, `is_active`), §4.8
  (filler), §5.2 (`BotFootPanel` layout), §6 (error handling), A.5 (judge algebra).

**Additional References (if relevant to this task):**
- `docs/gauge_and_judge_scoring_research.md` §1 (judge acceptance, one note per frame).
- `.agents/planning/2026-09-13-multiplayer-bot/research/bot-controller-re.md` §1 (vtable
  slots per build), §4 (controller design).
- `src/services/foot_panel_swap/{mod.rs,layout.rs}` (Step 1 service — this task fills it in).
- `src/mods/multiplayer_bot/planner.rs` (`NoteView`, `PanelFlags`, `SongState`, `plan_frame`).
- `src/mods/two_player_bpl_mode/logic.rs::clone_vtable_image` + `mod.rs` (vtable-clone
  precedent; `DDR_BPL_DRY_RUN` dev-mode gating shape via `config::get().layeredfs.developer_mode`).
- `src/types/game_note.rs` (`actor_results_range`, `for_each_result`, `result::*` offsets,
  `GameNote`).
- `src/services/song_reset/mod.rs` (`on_song_reset`, `remove_callback`).
- `tools/bot_sim/src/judge_model.rs` (`simulate` — the host twin of `filler::fill`; keep the
  NoteView construction identical: `unjudged = ts < 0 && grade == 0xFF`).

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. `services/foot_panel_swap/layout.rs` (pure, host-tested via `tools/bot_sim`):
   `#[repr(C)] pub struct BotFootPanel { vtable: *const *const u8 /*+0x00*/, pub is_held:
   [u8; 8] /*+0x08*/, pub was_just_pressed: [u8; 8] /*+0x10*/, pub event_mc: [i32; 8]
   /*+0x18*/, _reserve: [u8; 0x58 − 0x38] }` with `const` assertions on the offsets and
   `size_of == PANEL_OBJECT_SIZE`; `pub const VTABLE_SLOTS: usize = 7; SLOT_GET_PRESS_AGE
   = 5; SLOT_CONSUME_PRESS = 6`; `pub fn bot_vtable_image(donor: &[usize; 7], col: usize,
   get_press_age: usize, consume_press: usize) -> [usize; 8]` (COL at `[0]`, slots 0–4
   verbatim, 5/6 replaced — the `clone_vtable_image` shape); `impl BotFootPanel { pub fn
   apply(&mut self, flags: &BotPanelFlags) }` copying the three arrays.
2. `services/foot_panel_swap/mod.rs`: at `init`, read the 7 stock slots and the COL
   (`vtable[-1]`) from the RTTI `auto_foot_panel_vtable` (probe with `memory::is_readable`),
   build the image into a `memory::alloc_zeroed` region, place TWO `BotFootPanel` objects
   (one per side) in the same region with `vtable = image_ptr + 8`; the two slots:
   `unsafe extern "C" fn bot_get_press_age(this: *mut BotFootPanel, panel: i32) -> i32 {
   CURRENT_MC[side_of(this)] − (*this).event_mc[(panel as usize) & 7] }` and
   `bot_consume_press` zeroing `event_mc[panel & 7]`; `side_of` compares `this` against the
   two static object pointers (unknown ⇒ 0). Any failure ⇒ WARN, `arm_bot` returns `false`
   thereafter (`BOT_OBJECTS_READY` flag) — autoplay keeps working. Implement the `Bot` arm of
   `swap_in`: stash the original pointer, `CURRENT_MC[side] = mc`, call the armed `BotFillFn`
   with a stack `BotPanelFlags::default()`, `apply` it to the side's object, write the object
   into the slot. Keep `swap_out` as is. `arm_bot` while `PERFECT[side]` ⇒ the existing INFO.
3. `src/mods/multiplayer_bot/filler.rs` (engine-facing): per-side `Mutex<Option<SongCtx>>`
   where `SongCtx { st: SongState, rng: Rng, curve: Curve, views: Vec<NoteView>, level: u8,
   seed: u64, validated: bool, frames: u32 }`; API `start_song(side, level, seed)`,
   `reset(side)` (drop the ctx — plans/cursor/seed re-roll happens at the next `start_song`),
   `tally(side) -> Option<[u32; 6]>`, and `pub fn fill(side: usize, actor: *mut u8, mc: i32,
   out: &mut BotPanelFlags)` = the registered `BotFillFn`: `try_lock` (contention/poison ⇒
   empty flags + one rate-limited WARN), first call per song probes `memory::is_readable`
   on `actor + 0x84..+0x170` and on the results range; read `cur_beat = *(actor +
   layout::ACTOR_CUR_BEAT)`, walk `for_each_result(begin, end)` rebuilding `views` IN PLACE
   (same length ⇒ update `unjudged` only; length changed ⇒ rebuild — a `song_reset` rebuilds
   the vector) with `NoteView { idx, kind: note.kind, music_count, beat_count, state, length,
   unjudged: ts < 0 && grade == 0xFF }`; count sanity `≤ 8192` entries; then
   `planner::plan_frame(&views, &mut st, &mut rng, &curve, mc, cur_beat, &mut PanelFlags)`
   and copy into `out`. Whole body inside `catch_unwind` (panic ⇒ empty flags + one WARN).
   **Self-check:** on each frame, for every view that flipped unjudged→judged this frame with
   `kind == 0` and not a shock, compare the game's grade (`result+0x0C`, 0..=3 / 5) with the
   planner's resolved plan (`grade_for_offset(d)` or Miss); count `mismatches` in the ctx
   (grade 4 from the game would be a mismatch too). `tally` returns `[marv, perf, great,
   good, miss, mismatches]`-style data via a small struct `SongSummary { tally: [u32; 6],
   mismatches: u32, frames: u32 }`.
4. `src/mods/multiplayer_bot/mod.rs`: `MultiplayerBotMod` (id `multiplayer-bot`, name
   "Multiplayer Bot", description per design; `required_signatures() -> &[]`; `init` requires
   `foot_panel_swap::is_available()` + `stage_records::is_available()` +
   `scene_manager::is_available()` + `score_guard::is_available()` — each miss WARNs and
   returns `false`; `is_active` = init succeeded). `enable`: register ONE
   `scene_manager::on_scene_change` callback (kept in the struct; removed on `disable`) that
   drives the dev self-test only in this step; `disable`: disarm any armed side, taint off.
   **Dev self-test** (`self_test.rs`): active iff `config::get().layeredfs.developer_mode`
   AND `std::env::var("DDR_BOT_SELF_TEST")` parses as a level 1..=10 (read once at `enable`,
   INFO "self-test armed at LV n"). On `next == GAMEPLAY`: for each side with
   `stage_records::side_entered(side) == Some(true)`: `filler::start_song(side, level,
   skill::seed(qpc, 0, 0, level))`, `foot_panel_swap::arm_bot(side, filler::fill)`,
   `score_guard::set_autoplay_taint(side, true)`; INFO with σ/p/seed. On `next ∉ {28, 29,
   30}` while armed: `disarm_bot`, taint off, INFO `self-test tally side=N marv=… perf=…
   great=… good=… miss=… mismatch=… frames=…`, `filler::reset`. Also subscribe
   `song_reset::on_song_reset` while armed ⇒ `start_song` again (fresh seed).
5. Register in `src/lib.rs` `mods_to_register` right after `autoplay`; add
   `"multiplayer-bot": true` to `mod-config.json` `mods`. `pub mod multiplayer_bot;` already
   exists in `src/mods/mod.rs`.
6. `tools/bot_sim/src/main.rs` needs no change (it mounts `layout.rs`; new `layout` tests run
   there). Add the `layout` tests: `BotFootPanel` offsets/size, `bot_vtable_image` layout,
   `apply` round-trip.
7. Hook-path rules: no `unwrap`/`expect`/unmasked indexing in `fill`, the two vtable slots, or
   `swap_in`; panel index masked `& 7`; allocation-free per frame after the first
   (`views` capacity retained; `PanelFlags` on the stack).

## Dependencies

- Step 1 service and Step 2 pure cores (in tree).
- `song_reset::{on_song_reset, remove_callback}`, `scene_manager::{on_scene_change,
  remove_callback}`, `stage_records::side_entered`, `score_guard::set_autoplay_taint`,
  `types::game_note`, `core::memory::{is_readable, read_i32, alloc_zeroed, write_ptr, read_ptr}`.

## Implementation Approach

1. `layout.rs`: tests first (offsets, image, apply) → implement → `./scripts/validate_multiplayer_bot.sh`.
2. `foot_panel_swap/mod.rs`: vtable clone + objects + the two slots + the `Bot` arm.
3. `filler.rs`, then `self_test.rs`, then `mod.rs` + `lib.rs` + `mod-config.json`.
4. `cargo check --target x86_64-pc-windows-msvc` → `cargo fmt` (both crates) → `./build.sh`.

## Acceptance Criteria

1. **Layout pinned** — Given `layout.rs` tests, When run, Then `BotFootPanel` has `is_held`
   at +0x08, `was_just_pressed` at +0x10, `event_mc` at +0x18, size 0x58; `bot_vtable_image`
   keeps slots 0–4 and the COL, replaces 5/6; `apply` copies all 24 values.
2. **Bot arm is live** — Given `arm_bot(side, fill)` succeeded, When the side's `judgeNotes`
   runs, Then the slot holds that side's `BotFootPanel` during the call, `getPressAge` returns
   `mc − event_mc[panel]`, and the slot is restored after.
3. **Self-test on cabinet (maintainer)** — Given dev mode + `DDR_BOT_SELF_TEST=10`, When the
   human plays hands-off, Then the game grades ~99.8 % Marvelous, the arm INFO shows σ/p/seed,
   and the song-end INFO shows `mismatch=0` (a handful is acceptable on dense Challenge charts
   — the one-judgement-per-frame race documented in the bot_sim report). With
   `DDR_BOT_SELF_TEST=1`: visible Greats/Goods/Misses, the gauge drains, `mismatch` still ≈0.
   With the variable unset: nothing armed, zero per-frame logging. Autoplay ON + self-test ⇒
   the precedence INFO once and the bot drives (not Perfect).
4. **Fail-open** — Given the vtable read or allocation fails at service init, When the mod
   arms, Then `arm_bot` returns `false`, one WARN, autoplay unaffected.
5. **Build gates** — `cargo check` clean, `cargo fmt`, `./build.sh` clean,
   `./scripts/validate_multiplayer_bot.sh` green, no local paths.

## Metadata
- **Complexity**: High
- **Labels**: hooking, vtable-clone, judge-hook, multiplayer-bot
- **Required Skills**: Rust unsafe FFI, this repo's judge_hook/memory/scene_manager conventions
- **Generated By**: code-task-generator 2026-09-13
- **Source Plan**: `.agents/planning/2026-09-13-multiplayer-bot/implementation/plan.md`
- **Plan Step**: Step 3: Bot controller — cloned-vtable `BotFootPanel`, `filler`, mod skeleton, dev self-test on the human's side
