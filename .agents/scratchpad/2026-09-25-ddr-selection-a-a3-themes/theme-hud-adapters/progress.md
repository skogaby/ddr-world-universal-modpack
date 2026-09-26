# Progress — theme-hud-adapters

- [x] Failing tests: policy HUD rows + adapters; gauge theme fill; combo theme sheets; score theme
      textures — red: 3 failed (the gauge arm already behaved; made explicit)
- [x] `policy.rs` theme rows: `dance_gauge`, `dance_combo`, `dance_score`, `dance_option`
- [x] `gauge_math::fill_mode` explicit theme arm; `combo_math::sheet_prefix` and `score_math` via
      `tex_number`
- [x] Engine: `gauge.rs` / `combo.rs` / `score.rs` ranges 1..=`SKIN_MAX`, `option_icons.rs` 2..=8,
      `combo::package_usable(skin, name)` on the policy's package name (`PACKAGE_STATE` sized
      `SKIN_MAX + 1`), the difficulty frame's `name_usr` placeholder hidden post-init
- [x] Gate: harness 155 passed, `cargo check` / `cargo fmt` / `./build.sh` clean
- [ ] Cabinet demo (maintainer)

## Deviations
- The `name_usr` placeholder hide (A3 behaviour) lands here rather than with the name widget
  (plan Step 6), so the theme frames never show an unbound placeholder in between.

Status: Complete (uncommitted — maintainer commits manually; cabinet demo pending)
