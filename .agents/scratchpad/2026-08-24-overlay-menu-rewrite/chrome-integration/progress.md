# Progress — chrome-integration (Step 4 task-02)

## Checklist

- [x] config.rs: OverlayMenuConfig + field + both default blocks
- [x] widget_renderer.rs: `walk_free_pool()` extraction + pub `free_node_count()`
      (diagnostic keeps its per-reason unavailability lines)
- [x] chrome_loader.rs: kick/synthesis/cache/pump/status + ladder +
      `DDR_MOD_MENU_CHROME_FAULT` fault env
- [x] mod.rs: 6 chrome widget state fields + `mod chrome_loader;` + enable() kick
- [x] render.rs: MODAL_H + chrome geometry/tint constants + `allocate_chrome_widgets`
      FIRST (headroom check: skip all chrome + 1 WARN when free < 17+32; walk
      unavailable ⇒ proceed) + refresh wiring (panel bind w/ strip-stretch solid
      rung, header bars per visible Header slot, tab underline, selection bar,
      proportional scrollbar on overflow) + hide_all/destroy coverage
- [x] Gates: validate_mod_menu.sh 23/23 + validate_overlay_draw.sh 12/12 →
      cargo check 0 warnings → cargo fmt → ./build.sh clean
- [x] Deploy + boot log verification (autonomous, 3 boots):
      cold (cache cleared): synthesized both PNGs + sidecars → both textures
      resolved <1 s after first wrapper frame; free pool 254; no new WARNs.
      warm: both cache hits → resolved. fault (DDR_MOD_MENU_CHROME_FAULT=panel):
      exactly one WARN, panel Failed, strip resolved independently (solid rung
      armed). No panics/crashes in any boot.
- [x] Maintainer visual sign-off (the step's demo gate) — received 2026-08-24
      after one feedback round (tab-label centering); "everything looks perfect"

## Log

- 2026-08-24: setup + context + plan (auto mode; approval chain verified).
- 2026-08-24: implemented all 5 pieces; first check failed on a private re-export
  (`widget_renderer::ImageWidgetConfig`) — fixed to import from
  `widgets::image_widget` (strip_hud's pattern). Second check clean.
- 2026-08-24: gates green; deployed to the bottle install; 3 autonomous boots
  (logs: cold/warm/fault greps recorded above).
- 2026-08-24: maintainer visual feedback round 1 — "everything looking great";
  one nit: tab labels were left-justified against the fixed-width underlines.
  Fix: tab labels now `TextAlignment::Center` anchored at the underline
  midpoint (`TAB_TEXT_CENTER_OFF = TAB_IND_X_OFF + TAB_IND_W/2`; underline
  placement unchanged, grow affordance now symmetric). Gates re-run clean;
  redeployed for confirmation.

## Deviations

- `TEXT_WIDGET_COUNT` formula initially undercounted by 1 (5+… vs 6+…);
  corrected before check.
- `chrome::opacity_alpha` promoted `pub` (fallback tint needs the panel's
  percent→alpha mapping) — pure-layer API addition, covered by existing
  opacity tests.
- `ChromeStatus` carries no `strip_failed` (renderer treats absent strip as
  "hide element"); the STRIP_FAILED atomic still latches for Step 7.

## Consistency review

- Pump/mailbox/fault-env idioms match strip_hud; warn_once latching matches
  repo one-WARN-per-class convention; no unwrap/indexing in hook-reachable
  paths (pump closures use lock-or-return, `checked_sub`, `unwrap_or_default`).

Status: Complete (uncommitted — maintainer commits manually)
