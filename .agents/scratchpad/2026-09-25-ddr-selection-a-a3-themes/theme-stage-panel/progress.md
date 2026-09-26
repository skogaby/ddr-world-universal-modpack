# Progress — theme-stage-panel

- [x] Failing tests: `variant` / `root_package(_cstr)` per skin; the theme band over 3- and 5-stage
      sessions, the final override, extra, special stages, event chains; special stage ≡ silent stage
      call — red: 16 compile errors on the missing API (the machine's no-cut-in path is already
      covered by `no_cutin_shows_at_once`)
- [x] `panel_logic.rs`: `Variant`, `variant`, `root_package(_cstr)`, `special_stage`,
      `theme_stage_texture`
- [x] `panel.rs`: session variant + static root package; row patch writes / restores that pointer;
      theme sessions request nothing and skip the era-only banner-held check; adoption hides
      `choice_stage_usr2` (theme) instead of `choice_stage_usr` (era); `fill_theme` (band texture,
      song jacket, stage call at once)
- [x] Gate: harness 164 passed, `cargo check` / `cargo fmt` / `./build.sh` clean
- [ ] Cabinet demo (maintainer)

Status: Complete (uncommitted — maintainer commits manually; cabinet demo pending)
