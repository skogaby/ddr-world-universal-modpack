# Context — player-tab-model (Step 6 task-01)

## Provenance (verified)
- Task: `.agents/tasks/2026-08-24-overlay-menu-rewrite/step06/task-01-player-tab-model.code-task.md`
  (Generated-By 2026-08-25; breakdown + 5 layout/nav/parity decisions
  user-approved in session). Plan/design Approved 2026-08-24. Mode: auto.

## Touch points (verified)
- model.rs: TabId :18-44 (+ ALL.len()==2 test :694); NavState :197-200;
  TabNav::new sizes by ALL (:220); Navigator :252-376 (new/selected/step/
  clamp_after_rebuild/follow_scroll/page_window/scroll_indicator/overflows);
  RowKind::Scalar :56-62; RowSource::Mirrored unit :82; test NavState
  literals at :515/:533/:553/:597/:625/:677; test Scalar literal :400.
- RowKind::Scalar constructions to update for `formatted`: model.rs:400
  (test helper), tabs.rs:95 (convert_kind → `formatted: None`),
  input.rs:173 (destructure — add `..`), render.rs:482 uses `..` (untouched
  this task; render consumes formatted in task-02).
- tabs.rs:42-48 exhaustive match — temporary `TabId::PlayerSettings =>
  Vec::new()` arm lands HERE to keep the crate green (task-02 replaces).

## Interpretations (auto mode record)
- Pinned focus = `NavState.pinned_focus: bool` (Copy/Default preserved;
  reset flows free via TabNav::new); Navigator gains `pinned: bool` via
  `new_with_pinned` (plain `new` = false ⇒ existing behavior byte-identical).
- Wrap cycle with pinned: selector sits at the TOP of the cycle — UP from
  first selectable → selector; UP from selector → last selectable; DOWN
  from selector → first selectable; DOWN from last selectable → selector.
- clamp_after_rebuild on a pinned navigator with nothing selectable parks
  focus on the selector (pinned_focus = true).
- Eligibility pure fns take plain inputs (`entered: [Option<bool>; 2]`,
  `in_attract_band: bool`) — impure adapter is task-02.

## Gates
validate_mod_menu.sh (existing 23 stay green + new tests) → check → fmt →
build.
