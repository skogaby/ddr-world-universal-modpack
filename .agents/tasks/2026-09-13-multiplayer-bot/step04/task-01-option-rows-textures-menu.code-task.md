# Task: Option rows `bot_opponent` / `bot_opponent_level`, label textures, menu placement

## Description

Give the Multiplayer Bot its user-facing surface (design §4.3, §4.11, §5.3, R1): a bool
parent row **BOT OPPONENT (1P ONLY)** and a child scalar row **BOT LEVEL** (1..=10, default
5, shown only while the parent is ON) registered per player through `custom_options`, both
`PersistMode::Full`; the per-side values land in `OPTION_ON: [AtomicBool; 2]` /
`LEVEL: [AtomicI32; 2]` in `src/mods/multiplayer_bot/mod.rs` for the impersonation
(task 02) to read at the song-select → stage transition. Ship the in-game label textures
(all three languages) and place both rows right after `autoplay` in
`mod-config.json` `option_menu_settings`.

## Background

Step 3 left `MultiplayerBotMod` with a scene callback that only drives the dev self-test.
The mod's option rows are the ONLY enable source for the feature; without them the
impersonation has nothing to read. Rows are registered in `enable` (parent first — the
framework validates `ShowWhen` parents synchronously), following the bool-parent +
scalar-child precedent in `src/mods/assist_tick.rs` (`register_volume_row` /
the `OPT_ID` registration block). In-game labels are `seop_item_<id>.png` textures
generated from `scripts/option_strings.py`; a DLL-only deploy shows a blank label. The
overlay PLAYER SETTINGS tab needs no textures.

## Reference Documentation

**Required:**
- Design: `.agents/planning/2026-09-13-multiplayer-bot/design/detailed-design.md` — §4.3
  (`mod.rs`: rows, atomics, `enable`/`disable`), §4.11 (textures + placement), §5.3
  (persistence), R1, §6 (error handling: `Duplicate` = success).

**Additional References (if relevant to this task):**
- `.agents/planning/2026-09-13-multiplayer-bot/research/option-framework.md` — exact API
  shapes, texture checklist, the 12 traps (argument order `get_value(side, id)` vs
  `set_value(id, side, v)`; parent before child; bool rows on `is_available()`, scalar rows on
  `row_injection_available()`; `on_change` fires for both sides at registration from a
  non-render thread and must never panic).
- `src/mods/assist_tick.rs` ~1045–1074 (callbacks), ~1091–1133 (child scalar row),
  ~1531–1558 (parent bool row + `Duplicate` reseed).
- `src/services/custom_options/api.rs` (`RegisterSpec::bool_toggle`, `::scalar`,
  `ScalarFormat::Integer`, `.show_when(ShowWhen::Equals{..})`, `.persist_transform(save, load)`,
  `.display_name`, `.description`, `.default_value`).
- `scripts/option_strings.py` `LABELS` (~64–160) and `PREVIEWS` (`assist_tick` off/on and
  `assist_tick_volume` entries ~491–545 are the text-only WIDE precedents);
  `scripts/gen_option_labels.py` (renderer — run, never edit output PNGs).
- `mod-config.json` `option_menu_settings` (~165–198: `header_training_options`, `autoplay`,
  `song_speed`…).
- `src/mods/multiplayer_bot/eligibility.rs` (`MIN_LEVEL`/`MAX_LEVEL`/`clamp_level` — reuse
  for the load transform and the atomics' clamp).

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. `src/mods/multiplayer_bot/mod.rs`:
   - `pub(super)`/module-level statics `OPTION_ON: [AtomicBool; 2]` (default false) and
     `LEVEL: [AtomicI32; 2]` (default `DEFAULT_LEVEL = 5`); accessor fns
     `pub fn option_on(side: usize) -> bool` and `pub fn level(side: usize) -> u8`
     (clamped via `eligibility::clamp_level`; out-of-range side ⇒ `false` / default).
   - Constants `OPT_ID: &str = "bot_opponent"`, `OPT_LEVEL_ID: &str = "bot_opponent_level"`.
   - Change callbacks are plain `fn(u8, i32)`, panic-free (bounds-checked `get(side)`):
     `on_option_change` stores `v != 0` + one INFO `MultiplayerBot: side=N BOT OPPONENT ON|OFF`;
     `on_level_change` stores `clamp_level(v)`. No versus mirror (the option is 1P-only by
     construction).
   - `enable`: after the `CAPABLE` gate, `register_rows()`:
     - if `!custom_options::is_available()` ⇒ one WARN "no enable source", skip both rows;
     - parent: `RegisterSpec::bool_toggle(OPT_ID).display_name("Bot Opponent (1P Only)")
       .description("Play VERSUS against a computer opponent on the empty pad")
       .default_value(0).on_change(on_option_change)`; `Ok` ⇒ INFO; `Err(Duplicate)` ⇒
       reseed both sides from `custom_options::get_value(side, OPT_ID).unwrap_or(0)` through
       `on_option_change`; other `Err` ⇒ WARN and return (no child without a parent);
     - child (only after the parent is known registered, and only if
       `custom_options::row_injection_available()` — else one WARN "BOT LEVEL row absent,
       level stays at default"): `RegisterSpec::scalar(OPT_LEVEL_ID, 1, 10, 1,
       ScalarFormat::Integer).display_name("Bot Level").description("1 = beginner, 10 = expert")
       .default_value(5).show_when(ShowWhen::Equals { parent_id: OPT_ID.into(), value: 1 })
       .persist_transform(|_, v| v, |_, v| clamp_level(v) as i32).on_change(on_level_change)`;
       same `Ok` / `Duplicate`-reseed / `Err`-WARN handling.
   - `disable`: `custom_options::set_option_available(OPT_ID, false)` and
     `set_option_available(OPT_LEVEL_ID, false)` (there is no unregister), BEFORE the existing
     self-test shutdown / callback removal. Re-`enable` then hits `Duplicate` and must call
     `set_option_available(id, true)` for both rows after the reseed (mirror
     `song_playback_speed.rs`'s refuse/re-show shape if it exists; otherwise show on
     `Duplicate`).
   - Update the module doc comment: rows exist; impersonation still task 02.
2. `scripts/option_strings.py`:
   - `LABELS["bot_opponent"] = {"en": 'BOT OPPONENT (1P ONLY)', "ja": 'ボット対戦 (1P専用)',
     "ko": '봇 대전 (1P 전용)'}` and `LABELS["bot_opponent_level"] = {"en": 'BOT LEVEL',
     "ja": 'ボットレベル', "ko": '봇 레벨'}` — insert right after the `autoplay` entry.
     (ja/ko wordings are proposals; keep them ≤ the label box like the existing entries.)
   - Optional but recommended: WIDE text-only `PreviewSpec`s — `("bot_opponent", "off", …)`,
     `("bot_opponent", "on", …)` and `("bot_opponent_level", None, …)` — placed next to the
     `assist_tick` previews; copy explains: OFF = normal 1P play; ON = the next song is a
     VERSUS session against a computer player on the empty pad (its score is never saved);
     LEVEL = 1 (a real chance to fail) … 10 (Marvelous Full Combos), all three languages.
   - Run `python3 scripts/gen_option_labels.py`; verify the new
     `seop_item_bot_opponent.png` / `seop_item_bot_opponent_level.png` (and any
     `seop_image_bot_opponent*.png`) exist under all three
     `data_mods/custom_options/select_music_option_lang_{eng,jpn,kor}_v3_ifs/tex/` dirs and
     that no pre-existing PNG changed (`git status` on those dirs shows only additions).
3. `mod-config.json` `option_menu_settings`: insert
   `{"id": "bot_opponent", "overlay": true, "in_game": true}` and
   `{"id": "bot_opponent_level", "overlay": true, "in_game": true}` immediately after the
   `autoplay` entry (before `song_speed`). Keep the file valid JSON (`python3 -m json.tool`).
4. No config section, no writes to `mod-config.json` by the DLL, no new signatures.

## Dependencies

- Step 3 `MultiplayerBotMod` (in tree), `custom_options::{is_available,
  row_injection_available, register_option, get_value, set_option_available, RegisterSpec,
  RegisterError, ScalarFormat, ShowWhen}`, `eligibility::clamp_level`.
- Python 3 + the repo's `scripts/gen_option_labels.py` toolchain (fonts vendored under
  `scripts/`).

## Implementation Approach

1. `mod.rs`: statics + accessors + callbacks + `register_rows()` + `disable` hiding.
2. `option_strings.py` labels (+ previews) → `python3 scripts/gen_option_labels.py` → check
   the generated files.
3. `mod-config.json` placement.
4. `cargo check --target x86_64-pc-windows-msvc` → `cargo fmt` (whole crate) → `./build.sh`
   → `./scripts/validate_multiplayer_bot.sh` (unchanged tests must stay green).

## Acceptance Criteria

1. **Rows registered** — Given the mod enables with `custom_options` available, When the boot
   log is read, Then it shows the parent and child registration INFOs, and both menus list
   BOT OPPONENT (1P ONLY) directly after AUTOPLAY with BOT LEVEL visible only while the parent
   is ON (cabinet, maintainer).
2. **Atomics follow the rows** — Given a side toggles the parent or scrolls the level, When
   `on_change` fires, Then `option_on(side)` / `level(side)` reflect it (level clamped 1..=10),
   and a stale JSON-cached level of 0 or 99 loads as 1 / 10.
3. **Re-enable is idempotent** — Given the mod is disabled then enabled from the Mods tab,
   When rows report `Duplicate`, Then the atomics are reseeded from `get_value` and the rows are
   visible again; disabling hides both rows.
4. **Textures shipped** — Given `python3 scripts/gen_option_labels.py` ran, When the three
   `tex/` dirs are listed, Then each contains `seop_item_bot_opponent.png` and
   `seop_item_bot_opponent_level.png` and no other file changed.
5. **Fail-open** — Given `custom_options` or the scalar machinery is unavailable, When the mod
   enables, Then one WARN names the missing row(s) and nothing else breaks (self-test path
   unaffected).
6. **Build gates** — `cargo check` clean, `cargo fmt`, `./build.sh` clean,
   `./scripts/validate_multiplayer_bot.sh` green, `mod-config.json` valid JSON, no local paths.

## Metadata
- **Complexity**: Low
- **Labels**: custom-options, textures, multiplayer-bot
- **Required Skills**: Rust, this repo's `custom_options` conventions, Python (label generator)
- **Generated By**: code-task-generator 2026-09-13
- **Source Plan**: `.agents/planning/2026-09-13-multiplayer-bot/implementation/plan.md`
- **Plan Step**: Step 4: Impersonation flip/restore, option rows, textures, menu placement — full bot session
