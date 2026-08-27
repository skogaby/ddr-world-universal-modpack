# Progress — mod-menu-pure-model

Status: Complete (uncommitted — maintainer commits manually)

## Checklist

- [x] `src/mods/mod_menu/model.rs` — dependency-free: `TabId` (+ALL/label/next/prev),
      `RowKind` (Boolean/Scalar/Enum/Header), `RowSource`, `Row` (+`selectable()`),
      snapshot inputs, `build_mods_tab`, `build_global_tab` (enabled-owner grouping,
      unowned tail), `NavState`/`TabNav` (per-tab memory, reset), `Navigator`
      (skip/wrap up/down, `clamp_after_rebuild` incl. the legacy underflow guards,
      `follow_scroll`, `page_window`, `scroll_indicator`, `overflows`)
- [x] `scripts/validate_mod_menu.sh` — **11 tests pass**
- [x] `pub(crate) mod model;` wired; no consumers yet (task-02)
- [x] `cargo check` 0 warnings → `cargo fmt` no churn → `./build.sh` clean

## Deviations

- Same tests-with-implementation authoring note as the encoder task (the scenarios
  were specified in plan.md first; the state-machine semantics were transcribed from
  the legacy guards into test cases before coding the Navigator).
- `TabNav.states` is a `Vec` parallel to `TabId::ALL` (not a fixed array) so appending
  tab variants in later steps is a one-line enum change.
