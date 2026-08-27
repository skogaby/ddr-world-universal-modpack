# Plan — task-01 theme-model

Status: Approved 2026-08-25 (auto mode — descends from the approved plan
step: plan.md Status: Approved 2026-08-24, design Status: Approved
2026-08-24, task Generated-By code-task-generator 2026-08-25, breakdown
maintainer-approved in-session)

## Implementation shape

1. `src/mods/mod_menu/theme.rs` (new, pure; imports only `super::chrome`):
   - `Palette` struct: text `[f32;3]` fields (title, tab_active,
     tab_inactive, header, label, value, greyed, on_value, off_value,
     footer, hints), tint `[u8;3]` fields (accent, header_bar,
     banner_back), `panel_top`/`panel_bottom` `[u8;4]`.
   - `impl Palette { pub fn gradient(&self) -> super::chrome::PanelGradient }`.
   - `Background` enum (`Static` only; Step 8 doc note), `Theme` struct.
   - `THEMES: &[Theme]` — 4 entries with the task-authored values.
   - `DEFAULT_THEME_INDEX`, `resolve_theme_index`, `theme(index)`.
2. model.rs: `TabId::Theme` variant + ALL + label; `build_theme_tab` in a
   new "THEME tab" section following the build_player_tab style.
3. tabs.rs: temporary `TabId::Theme => Vec::new()` arm.
4. scripts/validate_mod_menu.sh: MODULES += theme.rs.

## Test scenarios (must fail against an absent/incorrect implementation)

theme.rs tests:
- `table_integrity`: len == 4; ids exactly
  [arrows, bubbles, wavefield, minimal] in order; displays
  [RHYTHM, BUBBLES, WAVEFIELD, MINIMAL]; ids unique + stem-charset-safe
  (`[a-z0-9_]`); displays unique; every theme `panel_top != panel_bottom`.
- `gradient_maps_stops`: `THEMES[0].palette.gradient()` == PanelGradient
  with arrows' stops.
- `resolution`: `resolve_theme_index(None) == (0, true)`; each id
  round-trips `(i, true)`; `Some("bogus") == (0, false)`;
  `theme(usize::MAX)` returns the last entry (clamp, no panic).
- `backgrounds_all_static_for_now`: every entry matches
  `Background::Static` (Step 8 flips arrows/bubbles/wavefield).

model.rs tests:
- `theme_tab_rows` (new): `build_theme_tab(2, &labels4, true, false, 80)`
  ⇒ 3 rows [theme Enum{index 2, values 0..=3, labels==labels4},
  animate_bg Boolean{true} not greyed, opacity Scalar{80,25,100,5,10,
  formatted Some("80%")}], all RowSource::Theme; labels THEME /
  ANIMATED BACKGROUND / MENU OPACITY.
- `theme_tab_animate_greyed` (new): `animate_greyed = true` greys only
  the animate row.
- `tab_labels_stable` (update): len 4 + `TabId::Theme.label() == "THEME"`.
- `tab_nav_memory_and_wrap` (update): wrap walk now
  Mods→Global→Player→Theme→Mods; prev from Mods lands on Theme.

## Checklist mirror
See progress.md.
