# Progress — player-tab-model (Step 6 task-01)

## Checklist

- [x] TabId::PlayerSettings + ALL + label + tests (tab_labels_stable → 3;
      tab_nav_memory_and_wrap rewritten for the 3-tab cycle)
- [x] RowKind::Scalar.formatted: Option<String> + site updates
      (model test helper, tabs.rs convert_kind `formatted: None`,
      input.rs destructure `..`; render.rs already used `..`)
- [x] NavState.pinned_focus (Copy/Default kept; 6 test literals updated via
      `..NavState::default()`) + Navigator pinned extension
      (`new_with_pinned`; selected None while focused; wrap cycle with the
      selector at the top; clamp_after_rebuild parks on selector for
      empty/all-greyed pinned lists and preserves focus across rebuilds;
      plain `new` byte-identical — existing suite green unchanged)
- [x] MirroredRowSnap/MirroredKindSnap + build_player_tab (visible omission,
      1:1 kind mapping with formatted carried, greyed-all on !editable,
      headers never greyed) + SelectorState/editable_sides/
      resolve_selected_side/selector_state (fail-closed)
- [x] tabs.rs placeholder arm (`TabId::PlayerSettings => Vec::new()`)
- [x] Gates: validate_mod_menu.sh 30/30 (23 carried + 7 new) → cargo check
      0 warnings → cargo fmt → ./build.sh clean

## Notes

- The menu now SHOWS a third (empty) PLAYER SETTINGS tab until task-02
  wires the builder — intentional intermediate state within the same step.
- Python bulk edit verified by count-assert + grep read-back both times
  (first regex under-matched and aborted before writing — no silent no-op).

## Deviations

- None from the task spec.

Status: Complete (uncommitted — maintainer commits manually)
