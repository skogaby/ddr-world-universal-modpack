# Task: Display-strings sweep across all custom-option registrations (+ lint)

## Description
Give every custom-options row an explicit, curated display name and footer
description (replacing the prettified-id fallbacks), label the three unlabeled
enums, and move the webui cosmetic categories to in-game-only placement. Add a
lint leg so fallback-reliant registrations can't silently reappear.

## Background
Step 5 added `RegisterSpec.display_name/description`, `MenuPlacement` (+
`in_game_only()` builder), and `EnumValue.display_label`/`with_display` — but
no registrant was swept (fallbacks carried Steps 6–8). Survey (2026-08-25):
41 rows across ~14 files all use `prettify_id` fallbacks with empty
descriptions; 3 enum rows lack per-value labels (`perspective`,
`training_progress_pos`, `customize_movie_size` — every `bool_toggle` already
gets OFF/ON from the builder); zero `MenuPlacement` uses anywhere. webui's
`discovery::CategoryDef.display_name` already holds curated strings ("APPEAL
BOARD", …) but isn't wired into the specs. Maintainer approved: agent-authored
strings (table edits later); cosmetics in-game-only; the two webui profile
rows + 4 decorative headers stay both-menus; the 9 mod-menu contributed rows
(already labeled) untouched.

## Technical Requirements
1. Add `.display_name("…")` and `.description("…")` to ALL 41
   `custom_options::register_option` call sites (survey table in the step-09
   breakdown; files include autoplay.rs, announcer_mute.rs, premium_free.rs,
   quick_restart_or_fail.rs, assist_tick.rs, song_playback_speed.rs,
   player_perspective/mod.rs, power_user_statistics/mod.rs,
   overlay_element_styling/mod.rs, playfield_styling/mod.rs,
   training_mode/mod.rs, per_song_judgement_offsets/ui.rs,
   center_arrows_single.rs, webui_options/{mod.rs,profile_fields.rs},
   decorative_option_headers.rs).
2. Display names: the label style already established by the in-game menu
   textures (uppercase option names, e.g. "SONG SPEED", "ASSIST TICK",
   "PREMIUM FREE"); descriptions: one sentence, footer-sized (fits the
   overlay's 0.55-scale footer line), plain language, no jargon.
3. Add per-value `display_label`s: `perspective` (OVERHEAD/HALLWAY/DISTANT),
   `training_progress_pos` (OFF/LEFT/RIGHT), `customize_movie_size`
   (FULLSCREEN/ON/OFF) — preserve existing `with_preview` keys (use the
   chainable `.display_label()` so preview keys survive).
4. Wire webui cosmetics: `RegisterSpec.display_name` from the existing
   `CategoryDef.display_name`; add `.in_game_only()` to the 10 cosmetic
   category rows ONLY (profile rows `is_disp_weight`/`weight` stay
   both-menus with their own new display strings; headers stay both-menus).
   `CategoryDef.display_name` is `&'static str` so it satisfies the builder.
   Descriptions for cosmetics may be a shared pattern ("Choose the …
   cosmetic" style) — but each row still gets one.
5. Lint: extend `scripts/validate_custom_options.sh` with a grep-based leg
   that fails when (a) any `register_option`-bound spec chain in `src/mods/`
   lacks `.display_name(` or `.description(`, and (b) any `EnumValue::new(`
   appears outside `src/services/custom_options/` (registrants must use
   `with_display`/`with_preview(..).display_label(..)`). Structure the check
   so multi-line builder chains are handled (per-call-site block scan, not
   single-line grep).

## Dependencies
- Step 5's api.rs builders (already landed and host-tested).
- Survey table (this breakdown / explore report) as the authoritative site
  list — re-verify count at implementation time with a fresh grep.

## Implementation Approach
1. Fresh grep for all registration sites; diff against the survey's 41.
2. Sweep file-by-file; keep each chain's existing builder order readable
   (display_name/description adjacent to the constructor line).
3. Author strings in one pass per file; keep a consistent voice.
4. Add the lint leg last; run the full harness + `cargo check` + `cargo fmt`
   + `./build.sh`.
5. Cabinet validation: boot, open both menus, spot-check labels/footers on
   several mods' rows; verify webui cosmetics absent from the overlay tabs
   but present in-game; verify the 3 enums show their labels in the overlay.

## Acceptance Criteria

1. **No fallback display names remain**
   - Given the lint leg in validate_custom_options.sh
   - When the harness runs
   - Then it passes, and manually removing a `.display_name(` from any one
     site makes it fail

2. **Enum labels render**
   - Given the overlay menu open on a PLAYER tab with a carded-in session
   - When PERSPECTIVE / TRAINING PROGRESS POSITION rows are selected
   - Then their values display OVERHEAD/HALLWAY/DISTANT and OFF/LEFT/RIGHT
     (not prettified texture suffixes)

3. **Cosmetics are in-game-only**
   - Given the overlay menu open
   - When browsing every tab
   - Then no webui cosmetic category rows appear, while the in-game MODS tab
     still lists them with their curated names; WEIGHT and DISPLAY BURNED
     CALORIES still appear in both menus

4. **In-game menu unaffected visually**
   - Given the in-game options MODS tab
   - When compared with pre-sweep behavior
   - Then rows render identically (labels come from textures there; display
     strings only affect the overlay) and persistence behavior is unchanged

5. **Gates green**
   - Given the full readiness gates
   - When `validate_custom_options.sh` (incl. new lint), `validate_mod_menu.sh`,
     `validate_overlay_draw.sh`, `cargo check`, `cargo fmt`, `./build.sh` run
   - Then all pass with no new warnings

## Metadata
- **Complexity**: Medium
- **Labels**: rust, custom-options, ui-strings, lint
- **Required Skills**: repo conventions (AGENTS.md), custom_options framework
