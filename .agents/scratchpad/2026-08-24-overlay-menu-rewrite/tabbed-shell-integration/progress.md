# Progress — tabbed-shell-integration

Status: Complete (uncommitted — maintainer commits manually)

## Checklist

- [x] `mod.rs`: state swap (TabNav + per-tab row lists replace the flat
      rows/visible/cursor/scroll fields); open resets nav + rebuilds tabs
- [x] `tabs.rs` (new): registry/contributed snapshot assembly → model builders;
      `rebuild_and_refresh` post-edit path; `clamp_active`
- [x] `rows.rs`: registration API frozen (five registrants untouched);
      `visible_when` reinterpreted as owning-mod id (docs updated);
      `toggle_registry_mod` persists from a fresh registry read; dead code
      (rebuild_rows/rebuild_visible/row_value/clone_row) deleted
- [x] `input.rs`: navigation via `model::Navigator` (skip/wrap/per-tab memory);
      `1`/`3` tab switch; activation dispatches on `RowSource`; repeat thread intact
- [x] `render.rs`: full rewrite — 12-row dense layout in the amended modal footprint
      (1160×600 @ 60,60), tab bar with bracketed active tab, right-aligned value
      column, footer (selected description + key hints), N/M overflow indicator;
      31 widgets allocate-once
- [x] Gates: model harness 11 green · `cargo check` 0 warnings · `cargo fmt` clean ·
      `./build.sh` clean
- [x] Autonomous functional walkthrough (keypad injection): open logged; tab switch →
      GLOBAL SETTINGS; FPS TARGET 120→144→120 persisted to `fps_unlock.selected`;
      MODS tab navigation + hello-world OFF→ON→OFF persisted to the mods map with
      matching enable/disable log lines; zero panics/new WARNs
- [x] Maintainer verification: open AND close gestures confirmed working (the missing
      "closed" log in the injected session was injection cadence racing the 1250 ms
      gesture window — not a code bug)
- [x] Handoff screenshots captured (step3_mods_tab.jpg / step3_global_tab.jpg in this
      directory) — visual layout sign-off is the maintainer's (standing instruction)

## Post-sign-off feedback round (2026-08-24, maintainer) — all applied

- Scroll-trap fix in `model.rs::follow_scroll`: unselectable run directly above the
  cursor is pulled into view (leading/mid-list decorative headers can scroll back on
  screen — the same bug the in-game scroll driver once had). +2 regression tests
  (13 total green).
- Tabs: brackets removed; active = accent + scale 0.62, inactive = grey + scale 0.52
  (grow affordance). MODS relabeled "TOGGLE MODS" (model + test updated).
- Header rows render UPPERCASE (chrome backing bar deferred to Step 4).
- Pinpad navigation removed: menu buttons only for nav/adjust; pinpad = 0-0-0 close,
  1/3 tabs. Footer legend replaced with the maintainer's verbatim text.
- Footer description scale 0.45 → 0.55; ON/OFF brackets dropped; "(by skogaby)"
  credit added right of the title (CREDIT_X tuned from a maintainer screenshot:
  title ends ≈x=386 on canvas → +300 offset).
- Process note: a python bulk-patch hunk silently no-op'd (printed "patched" without
  verifying) leaving the old bracketed tab loop in place for one deploy — caught by
  the maintainer's visual test. Use the checked Edit tool for source edits.
- **Maintainer visual sign-off received: "everything looks great".**

## Deviations

- None from the task spec. Layout numbers follow the maintainer's mid-step margin
  amendment (~60 px unobscured; design §4.5 updated before implementation).
