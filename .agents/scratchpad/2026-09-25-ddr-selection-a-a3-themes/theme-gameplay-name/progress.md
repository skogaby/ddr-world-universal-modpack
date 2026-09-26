# Progress — theme-gameplay-name

- [x] RE (Appendix C row 5) — slot-8 list + key, font getter / residency, `PlayerWork+1` ≡ `+5`
- [x] `score_name_logic.rs` + tests (text rule, A3 binding maths hand-computed incl. truncation
      toward zero, style constants bit-exact incl. SD), registered in the harness. Tests written
      with the implementation (the binding test's first expectation was corrected against
      A3's arithmetic before the first run).
- [x] `signatures.rs` (additions only): `ddr_sel_gameplay_list_push`, `ddr_sel_font_by_id`,
      `derive_ddr_sel_name`, `DdrSelNameSites` + accessor
- [x] `widget_renderer.rs`: `WidgetStyle`, `RenderList`, `create_text_widget_with_font`,
      `scene_manager_global()`; `register_in_list(offset)` (overlay path unchanged);
      `text_widget.rs`: `set_vertical_alignment`, `set_box`
- [x] `score_name.rs` (engine) + `score.rs` handoff + `mod.rs` wiring (init, scene exit, disarm)
- [x] Gate: `cargo check` / `cargo fmt` / `./build.sh` clean; harness 184 (was 181); sweep ALL
      GREEN (all new names on all five builds; the derived screen graph equals the widget
      renderer's heuristic global on every build, checked offline); `shape_diff.py` identical
- [x] Cabinet demo (maintainer) — passed 2026-09-26

Status: Complete (uncommitted — maintainer commits manually)
