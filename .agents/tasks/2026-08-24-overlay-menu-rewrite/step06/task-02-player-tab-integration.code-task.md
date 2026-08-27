# Task: Player-tab integration — snapshot wiring, marshaled edits, gating, selector/banner

## Description
Wire the PLAYER SETTINGS tab into the live menu: tabs.rs builds it from
`custom_options::overlay_snapshot(side)`; input.rs fills the `Mirrored`
edit stub (marshaled `set_value` with the gate re-checked inside the
closure) and routes LEFT/RIGHT to side-switching while the pinned selector
is focused; mod.rs captures the gesture side, subscribes the value-changed
observer (coalesced repaint) and a scene-change callback (gating dirty);
render.rs adds the `CONFIGURING: ◄ PLAYER 1 ►` selector line, the
`NO ACTIVE SESSION` / `OPTIONS FRAMEWORK UNAVAILABLE` banner, and the
PLAYER-tab-conditional list geometry (11 content rows there, 12 elsewhere).

## Background
Step 6 of the overlay-menu rewrite (design §4.2/§4.8/§4.9, FR-4/5/9;
sequence diagram design:190-194). Task-01 provides the pure pieces
(TabId::PlayerSettings, `MirroredRowSnap`/`build_player_tab`,
`editable_sides`/`resolve_selected_side`/`selector_state`, pinned-slot
navigation, `RowKind::Scalar.formatted`).

Verified integration facts:
- tabs.rs:42-48 exhaustive `match` over `TabId::ALL` (task-01 left a
  `Vec::new()` placeholder arm); `rebuild_and_refresh()` at tabs.rs:66-81
  is the post-edit repaint path. Lock order: menu state lock → registry
  lock (tabs.rs:14-16) — `overlay_snapshot` from inside `rebuild_tabs`
  preserves it (snapshot takes only the registry STATE lock internally).
- input.rs: `RowSource::Mirrored | Theme => {}` no-op stub at :220-221
  inside `activate_selected` — the repeat thread funnels through the same
  fn (:324), so one arm covers press + hold-repeat. The Contributed arm's
  Boolean/Scalar/Enum new-value math (:167-210) is source-agnostic —
  factor it out and reuse. `coarse_held()` :228-235.
- mod.rs open-gesture closure :150-169 — `event.player` (`Player` enum,
  P1=0/P2=1, src/types/buttons.rs:56-62) available and unused; capture the
  COMPLETING press's side.
- `stage_records::side_entered(side: usize) -> Option<bool>`
  (stage_records.rs:374-378; cheap, lock-free, None = unavailable ⇒
  fail-closed).
- `scene_manager::current_scene() -> i32` (:249-251, 0-indexed, -1 until
  first transition); `on_scene_change(Box<dyn Fn(i32, i32)>) -> usize`
  (:274-281, args (prev, next), fired outside the manager lock);
  `remove_callback` :287-290. Attract band constants:
  `types/scenes.rs:85-86` `ATTRACT_SCENE_MIN/MAX` (2/16) — use these.
- custom_options: `overlay_snapshot(side: u8) -> Vec<OverlayRowInfo>`
  (mod.rs:486-500), `set_value(option_id, player_side, value)` (:305-321),
  `subscribe_value_changed(Arc<dyn Fn(&str, u8, i32) + Send + Sync>) ->
  usize` / `unsubscribe_value_changed` (observers.rs), `is_available()`.
  Observer contract: fires with NO framework lock held; registration prime
  does not fire.
- render.rs: tab band TAB_Y=116/underline 146/LIST_START_Y=156/ROW_H=34;
  budgets `TEXT_WIDGET_COUNT` :136 and `CHROME_WIDGET_COUNT` :139 must
  grow (selector text 1 + banner text 1 + banner backing strip 1; the tab
  label loop auto-extends by TabId::ALL). Strip-texture tinting pattern at
  :370+ (chrome_loader::status()).

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-24-overlay-menu-rewrite/design/detailed-design.md (§4.2, §4.8, §4.9, §6 banner rows, FR-4/FR-5/FR-9)

**Additional References (if relevant to this task):**
- .agents/tasks/2026-08-24-overlay-menu-rewrite/step06/task-01-player-tab-model.code-task.md (the pure API this consumes)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. **State (mod.rs)**: `player_side: u8` (desired/selected side, default 0),
   gesture capture in the open closure (the press that completes the
   triple-0 sets `player_side = event.player as u8`); observer token +
   scene-callback id fields (unsubscribed/removed on disable); new widgets:
   `side_selector_widget: Option<TextWidget>`, `banner_widget:
   Option<TextWidget>`, `banner_backing_widget: Option<ImageWidget>`.
2. **Gating evaluation** (impure adapter): `editable_sides()` from
   `stage_records::side_entered(0/1)` + `scene_manager::current_scene()`
   vs the `ATTRACT_SCENE_MIN..=ATTRACT_SCENE_MAX` band (scene_manager or
   stage_records unavailable ⇒ fail-closed per leg); resolved via task-01's
   pure fns. Evaluated: on open, on tab switch TO PlayerSettings, on side
   switch, on scene change while open (callback ⇒ coalesced repaint), and
   INSIDE the marshaled edit closure.
3. **tabs.rs**: replace the placeholder arm — when
   `custom_options::is_available()`: resolve side (task-01
   `resolve_selected_side(state.player_side, editable)`), convert
   `overlay_snapshot(side)` rows into `MirroredRowSnap`s, call
   `build_player_tab(rows, editable[side])`; when unavailable: empty list +
   a flag the renderer maps to the "OPTIONS FRAMEWORK UNAVAILABLE" banner.
   The PLAYER tab's Navigator uses the pinned-slot variant and 11-row page.
4. **input.rs**:
   - Factor the Contributed arm's new-value computation into a shared
     helper; add the `Mirrored` arm: compute new value from the cloned row
     (same math incl. `coarse_held()`), then
     `widget_renderer::run_on_render_thread(move || { if editable(side)
     { custom_options::set_value(&key, side, value); } else
     { /* refused: coalesced repaint shows greyed */ schedule repaint } })`
     — the gate re-check lives INSIDE the closure (design :471-473, :542).
     No direct `rebuild_and_refresh` — the observer repaints on success.
   - While the pinned selector is focused (PLAYER tab): LEFT/RIGHT switch
     `player_side` among EDITABLE sides only (task-01 selector_state:
     Locked/AllGated ⇒ no-op), then rebuild + refresh; UP/DOWN move focus
     per the pinned navigation model.
5. **render.rs**:
   - Per-tab geometry: `list_start_y(tab)` and `visible_rows(tab)` (PLAYER:
     LIST_START_Y + ROW_H and 11; others unchanged 12) threaded through the
     slot paint loop, cursor/selection-bar math, scrollbar geometry, and
     `Navigator::new` page arguments (tabs.rs clamp too). The 12 slot/bar
     widgets are REUSED (the 12th slot simply hides on the PLAYER tab).
   - Selector line at the first row band (PLAYER tab only):
     `CONFIGURING: ◄ PLAYER 1 ►` (text arrows; greyed when Locked/AllGated;
     accent when focused — selection bar tracks the pinned focus).
   - Banner: centered text over a tinted strip backing in the content area
     (`NO ACTIVE SESSION` when AllGated; `OPTIONS FRAMEWORK UNAVAILABLE`
     when the framework is missing); rows render greyed beneath (builder
     already greys them).
   - Budgets: TEXT_WIDGET_COUNT +2 (selector, banner), CHROME +1 (backing).
6. **Observer + scene callback (mod.rs enable/disable)**: subscribe once at
   enable; callback = coalescing pending-latch (AtomicBool) →
   `run_on_render_thread(rebuild_and_refresh_once)` clearing the latch —
   bursts (card-in resets) collapse to one rebuild; no-op while closed
   (rebuild_and_refresh already early-outs). Scene callback likewise marks
   gating dirty + coalesced repaint while open. Both torn down on disable.
7. Panic-freedom: all new callback bodies lock-or-return; no unwrap in
   hook-reachable paths; every gating read fail-closed; logging via log_*.
8. **Gates**: validate_mod_menu.sh (model tests incl. task-01's) → check →
   fmt → build; autonomous boot + keypad-injected functional walkthrough
   where feasible (tab switch to PLAYER SETTINGS in attract ⇒ banner +
   all-greyed, no panics); maintainer demo per the plan (card-in mirroring,
   both directions, persistence, versus independence).

## Dependencies
- task-01 (pure model) — must land first. Steps 4–5 complete.

## Implementation Approach
1. Read the design sequence diagram + §4.8/4.9 once more; land state/gating
   adapters (small).
2. tabs.rs arm + conversion; render geometry + selector/banner (deploy-able
   checkpoint: tab renders greyed in attract).
3. input.rs edit arm + selector focus routing; observer + scene wiring.
4. Gates; boot walkthrough (attract: banner + greyed rows + tab nav OK);
   deploy + maintainer demo.

## Acceptance Criteria

1. **Attract behavior (autonomous)**
   - Given the DLL booted to attract and the menu opened
   - When switching to PLAYER SETTINGS
   - Then the tab renders the full option list greyed with the
     NO ACTIVE SESSION banner, selector greyed, nav/tab switching works,
     zero panics/new WARNs.

2. **Mirroring (maintainer demo)**
   - Given a carded-in P1 at song select
   - When editing PREMIUM FREE in the overlay and then checking the in-game
     MODS tab (and vice versa)
   - Then both menus show the same value; values persist per PersistMode
     through card-out.

3. **Marshaled gated edits**
   - Given a session that ends while the menu is open (logout race)
   - When an edit is attempted
   - Then the closure's gate re-check refuses it and the tab repaints
     greyed; no value change occurs.

4. **Side selector**
   - Given versus play (both sides entered)
   - When focusing the selector and pressing LEFT/RIGHT
   - Then the tab flips between P1/P2 values (independent edits); with one
     side entered the selector is locked to it.

## Metadata
- **Complexity**: High
- **Labels**: mod-menu, player-tab, custom-options, integration
- **Required Skills**: Rust, repo mod_menu/custom_options conventions
- **Generated By**: code-task-generator 2026-08-25
- **Source Plan**: .agents/planning/2026-08-24-overlay-menu-rewrite/implementation/plan.md
- **Plan Step**: Step 6: PLAYER SETTINGS tab — mirroring, side selector, session gating
