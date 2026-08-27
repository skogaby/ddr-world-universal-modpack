# Plan — player-tab-integration (Step 6 task-02)

Status: Approved 2026-08-25 (auto mode — verified chain + in-session
breakdown approval)

## Edit order
1. mod.rs — state fields (player_side, framework_unavailable, selector/
   banner widgets, observer token, scene cb id), gesture-side capture,
   `schedule_coalesced_refresh()` (REFRESH_PENDING latch → render-thread
   rebuild+refresh), enable() observer+scene wiring, disable() teardown.
2. tabs.rs — `editable_sides_now()` adapter (stage_records + scene band,
   fail-closed) + `build_player_rows(&mut state)` (framework flag, side
   resolve, overlay_snapshot → MirroredRowSnap convert, build_player_tab)
   + rebuild restructure (precompute player rows) + clamp_active via the
   pinned navigator/per-tab page.
3. render.rs — `visible_rows(tab)`/`list_start_y(tab)`/`navigator_for(tab,
   rows)` helpers; selector + banner text widgets (+2 TEXT budget) + banner
   backing strip (+1 CHROME, created with chrome); refresh_all: per-tab
   geometry repositioning (slots, header bars, cursor, selection bar,
   scrollbar track/thumb), selector line (state/focus render), banner
   (AllGated/framework-unavailable), formatted-scalar value text.
4. input.rs — navigator_for in navigate/selected_row; pinned-focus LEFT/
   RIGHT = side switch (Free only; editable targets only); Mirrored arm
   (shared value math; marshaled closure with gate re-check → set_value;
   refused ⇒ coalesced refresh).

## Validation
- Harness 30/30 (untouched); check/fmt/build.
- Autonomous boot: attract → menu → tab 3 renders full greyed list +
  NO ACTIVE SESSION banner; nav works; zero panics (log-based; keypad
  injection if available via spice API is a bonus — not required).
- Maintainer demo: mirroring both directions, versus side switch,
  persistence, logout race repaint.
