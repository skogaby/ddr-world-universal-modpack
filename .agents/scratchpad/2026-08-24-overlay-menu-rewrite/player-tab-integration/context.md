# Context — player-tab-integration (Step 6 task-02)

## Provenance (verified)
- Task: `.agents/tasks/2026-08-24-overlay-menu-rewrite/step06/task-02-player-tab-integration.code-task.md`
  (Generated-By 2026-08-25; approved breakdown + decisions). Depends on
  task-01 (Complete). Plan/design Approved. Mode: auto.

## Verified integration facts (beyond the task file)
- input.rs full read: `handle_exclusive_input` :33-78; `switch_tab` :83;
  `navigate` :106 (Navigator::new(rows, VISIBLE_ROWS)); `selected_row`
  :135-145; `activate_selected` :152-224 with the Contributed value math
  :166-213 and the `Mirrored | Theme => {}` stub :222; repeat thread funnels
  via `activate_selected(d)` :325; `selected_repeats` :260 uses selected_row
  (returns None while pinned-focused ⇒ repeat inert on the selector — good).
- Slot/bar/scroll-track positions are set at WIDGET CREATION in render.rs —
  the PLAYER tab's shifted geometry (content +ROW_H, 11 rows) requires
  per-refresh repositioning of slot labels/values, header bars, and the
  scrollbar track (cursor/selection already position per refresh).
- tabs.rs `rebuild_tabs` maps over ALL with locals; the player arm needs
  &mut state (player_side resolve + framework flag) ⇒ precompute before the
  map.
- Lock pairs: menu → registry (existing); menu → scene_manager (new via
  current_scene inside rebuild — scene callbacks dispatch OUTSIDE the scene
  lock and my callback only flips an atomic + queues, so no inversion).

## Interpretations (auto mode record)
- Selector text: Free ⇒ "CONFIGURING:  < PLAYER n >"; Locked ⇒
  "CONFIGURING:  PLAYER n"; AllGated ⇒ "CONFIGURING:  PLAYER n" greyed.
  ASCII arrows (bmpfont glyph coverage unknown for ◄►; visual round tunes).
- Banner text: "NO ACTIVE SESSION" / "OPTIONS FRAMEWORK UNAVAILABLE",
  center-aligned at the content-area midline over a dark strip backing.
- Edit-side capture: the side is resolved at rebuild (state.player_side
  normalized via resolve_selected_side) and read under the same lock as
  selected_row when an edit starts.
- Scene unknown (manager unavailable or scene < 0) ⇒ in-band (fail-closed).
- Refused marshaled edit repaints via the coalesced refresh (shows greyed).

## Gates
validate_mod_menu.sh 30/30 → check → fmt → build → boot + attract
walkthrough (banner + greyed + nav) → maintainer demo (card-in mirroring,
versus, persistence).
