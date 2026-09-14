# Progress — option-rows-textures-menu

Status: Complete (uncommitted — maintainer commits manually)

## Checklist
- [x] `mod.rs`: atomics + accessors + callbacks + `register_rows()` + `disable` hiding + docs
- [x] `scripts/option_strings.py`: LABELS + PREVIEWS; regenerate PNGs
- [x] `mod-config.json`: placement after `autoplay`
- [x] Gates: `cargo check` → `cargo fmt` (both crates) → `./build.sh` → `./scripts/validate_multiplayer_bot.sh` (67/67) → JSON validity → path hygiene

## Log
- Setup/Explore/Plan done (context.md, plan.md).
- `src/mods/multiplayer_bot/mod.rs`: `OPT_ID`/`OPT_LEVEL_ID`/`DEFAULT_LEVEL`, `OPTION_ON`/`LEVEL`
  statics, `option_on(side)`/`level(side)` accessors, `on_option_change` (store + INFO) /
  `on_level_change` (clamped store), `clamp_level_transform` load transform, `register_rows()`
  (parent → child, `Duplicate` = reseed from `get_value` + `set_option_available(true)`, other
  errors WARN fail-open), `register_level_row()` gated on `row_injection_available()`; `enable`
  calls `register_rows()` after the CAPABLE gate; `disable` hides both rows first. Module doc
  updated to the Step 4 task-01 state.
- `scripts/option_strings.py`: `LABELS["bot_opponent"]` / `["bot_opponent_level"]` (en/ja/ko)
  after `autoplay`; three WIDE text-only `PreviewSpec`s (off / on / level) after
  `assist_tick_volume`. `python3 scripts/gen_option_labels.py` → 15 new PNGs (5 per language),
  `git status` shows ONLY additions under the three `tex/` dirs. Visually checked the eng label
  + previews on a dark backdrop (same white-on-transparent shape as `seop_image_assist_tick_on`).
- `mod-config.json`: `bot_opponent`, `bot_opponent_level` inserted after `autoplay` (both menus);
  `python3 -m json.tool` OK.
- Gates: `cargo check` clean; `cargo fmt` both crates; `./build.sh` clean (57 s);
  `./scripts/validate_multiplayer_bot.sh` 67 passed; `git grep` path hygiene adds no new hits
  (`.agents/**/logs` is gitignored).

## Deviations
- None from the task. Description strings chosen in auto mode (recorded in context.md).

## Notes for the maintainer
- A DLL-only deploy leaves the two in-game labels blank — ship the 15 new PNGs under
  `data_mods/custom_options/select_music_option_lang_{eng,jpn,kor}_v3_ifs/tex/` with the DLL.
  The overlay PLAYER SETTINGS tab needs no textures.
- Boot log lines to expect: `MultiplayerBot: registered BOT OPPONENT (1P ONLY) option`,
  `MultiplayerBot: registered BOT LEVEL option under BOT OPPONENT`, and one
  `MultiplayerBot: side=N BOT OPPONENT ON|OFF` per side (registration fires `on_change` for both).
