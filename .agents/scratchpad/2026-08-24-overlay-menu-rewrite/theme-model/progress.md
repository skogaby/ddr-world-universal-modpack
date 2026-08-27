# Progress — task-01 theme-model

- [x] theme.rs types + table + resolution fns + tests
- [x] model.rs TabId::Theme + test updates
- [x] model.rs build_theme_tab + tests
- [x] tabs.rs temporary arm
- [x] harness MODULES += theme.rs; harness green
- [x] gates: cargo check → cargo fmt → ./build.sh

## Log

- 2026-08-25: `src/mods/mod_menu/theme.rs` created — `Palette` (11 text
  `[f32;3]`, 3 tint `[u8;3]`, 2 panel-stop `[u8;4]`, `gradient()`),
  `Background::Static`, `Theme`, `THEMES` ×4 (authored palette values
  from the task), `DEFAULT_THEME_INDEX = 0`, `resolve_theme_index`,
  `theme()` (clamped). A `BASE_TEXT` const carries the shared text
  colors via struct-update so future themes can diverge per-field.
  4 tests (table integrity incl. stem-charset ids + non-degenerate
  gradients, gradient stop mapping, resolution incl. bogus fallback +
  clamp, backgrounds-all-static).
- 2026-08-25: model.rs — `TabId::Theme` appended (ALL len 4, label
  "THEME"); `build_theme_tab` + `THEME_ROW_KEY`/`ANIMATE_ROW_KEY`/
  `OPACITY_ROW_KEY` consts (keys `theme`/`animate_bg`/`opacity`); enum
  index clamps into the label table (defensive). Tests:
  `tab_labels_stable` + `tab_nav_memory_and_wrap` updated to the 4-tab
  cycle; new `theme_tab_rows` + `theme_tab_animate_greyed`.
- 2026-08-25: tabs.rs temporary `TabId::Theme => Vec::new()` arm
  (task-02 replaces); mod.rs `pub(crate) mod theme;`; harness MODULES
  += theme.rs.
- 2026-08-25: harness 36/36 (was 30); `cargo check` 0 warnings;
  `cargo fmt`; `./build.sh` clean (logs/).

## Deviations
- Red-first via compile-error is the practical red phase for a new
  static-table module; tests authored from the task spec alongside the
  implementation, honesty verified by review + harness run.
- Post-demo feedback (2026-08-25, maintainer): the 4th tab's label is
  **"APPEARANCE"** (was "THEME") — model.rs `label()` arm + the
  `tab_labels_stable` assert updated; enum variant stays `TabId::Theme`;
  the THEME row label inside the tab is unchanged. Gates re-run green,
  DLL redeployed.

Status: Complete (uncommitted — maintainer commits manually)
