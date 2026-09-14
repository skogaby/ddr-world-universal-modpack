# Progress — bot-controller (Step 3, task-01)

Status: Complete (uncommitted — maintainer commits manually)

## Checklist
- [x] layout.rs: `BotFootPanel` (offsets +0x08/+0x10/+0x18, size 0x58), `bot_vtable_image`,
      `panel_index` mask, `apply` — red (4 new tests) → green (67/67 via tools/bot_sim)
- [x] foot_panel_swap/mod.rs: `build_bot_objects` (probe COL+7 slots, one RWX region
      `[COL][slots][obj0][obj1]`), `bot_get_press_age` = `CURRENT_MC[side] − event_mc[panel&7]`,
      `bot_consume_press`, live `Bot` arm of `swap_in` (stash → CURRENT_MC → fill → apply →
      write slot), `bot_objects_ready()`, `arm_bot` refuses without objects; `ACTOR_*` re-exports
- [x] filler.rs: per-side `Mutex<Option<SongCtx>>` + `try_lock`; one-time actor probe; raw
      Results walk (index-aligned, holes = kind −1); in-place view refresh; `plan_frame`; permanent
      planner-vs-judge self-check (`mismatches`, judged tally); `catch_unwind`; rate-limited WARNs
- [x] self_test.rs: `developer_mode` + `DDR_BOT_SELF_TEST=<1..10>` gate; arms every entered side
      at GAMEPLAY, disarms + tally INFO outside {28,29,30}; song_reset re-roll; `shutdown`
- [x] mod.rs `Mod` impl (init gates incl. `bot_objects_ready`, one scene callback + song_reset sub,
      `is_active` = capable); registered in `lib.rs` after autoplay; `"multiplayer-bot": true`
- [x] gates: cargo check clean · cargo fmt (both) · ./build.sh clean · 67 host tests · hygiene

## Deviations
- None from the task file.

## Cabinet validation pending (maintainer) — design §7.3 / task AC3
Set `layeredfs.developer_mode: true` and launch with `DDR_BOT_SELF_TEST=10` (then `=1`):
boot log `FootPanelSwap: registered judge swap (… bot objects ready)` and
`MultiplayerBot: SELF-TEST armed at LV10`; at GAMEPLAY `SELF-TEST bot armed on side N`; play
hands-off ⇒ ~99.8 % Marvelous (LV1: Greats/Goods/Misses, gauge drains); song end ⇒
`SELF-TEST tally … mismatch=0 frames=…` (a handful of mismatches on dense Challenge = the
one-judgement-per-frame race; more = the controller assumption broke). Unset ⇒ no arm lines.
Autoplay ON + self-test ⇒ `bot controller takes precedence over autoplay on side N` once.
Repeat on 20250805/20260224 for §7.3 item 10.
