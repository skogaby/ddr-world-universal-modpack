
## Checkpoint #2 run 1 → fixes (2026-09-05)
- `sysfont.rs` + `sysfont_set_position` AOB: redirect the system-sprite class's (3) + loading-layer (1)
  per-display-info loads to a fake {1280,720} block (offline-verified 3+1 on all four builds; sweep ALL GREEN;
  shape_diff identical). Off-by-one in the letterbox-fn load offset (0x13→0x16) caught by the offline check.
- `display_modes::spice_windowed()` (`-w` on the command line) skips the fullscreen mode check — the Mac's
  1512×982 logical desktop had refused 1080p, so run C was stock.

## Checkpoint #2 run 2 → pivot (2026-09-05)
- Root-7 re-canvas + sysfont redirect REVERTED (canvas_fix.rs, sysfont.rs, `sysfont_set_position` AOB removed):
  the footer/logos are agcs text objects positioned in screen px by app code (`FUN_1800092d0` etc.), not the
  sysfont class; too many readers to redirect safely. Replaced by `widget_renderer::{set_canvas_scale,canvas_scale}`
  applied in TextWidget::{set_position,set_scale}, ImageWidget::{set_position,set_size}, create_image_widget,
  and a default text scale in create_text_widget_with_wrapper. `plan.recanvas_root7` → `scale_widgets`.
- Gates: fmt · harness 24/24 · sweep ALL GREEN · build clean.

## Checkpoint #2 run 3 → D5 v3 (2026-09-05)
- Widget write-scaling reverted (widget_renderer/widgets back to stock). Root cause of the observed double
  scale = overlay_draw's mid-walk `set_context_2d(1280,720)` in a 640-canvas root (also the version-text shrink).
- `logical_screen.rs`: classify + redirect the per-display-info pointer loads (design 17–18 → {1280,720}; render 4
  → {render}; 16 physical untouched). Offline classifier reproduces the exact physical set on all four builds.
- Gates: fmt · harness 24/24 · sweep ALL GREEN · build clean.

## Checkpoint #2 run 4 → hardware-check regression fix (2026-09-05)
- Ark draw-callback family (percentage drawers, `100.0f` divisor) + system font (`DIVSS [r+0x74/0x78]`) moved
  to PHYSICAL — they draw into a bare screen-sized list. Design family now exactly 4 (gate `EXPECT_DESIGN`).
  Offline mirror: D={loading, footer, getter_w, getter_h} on all four builds. Gates green.

Status: Complete (uncommitted — maintainer commits manually)
Checkpoint #2 run 5 PASSED 2026-09-05 (SD + 1080p Tier A: title/footer/logos, mod menu, loading screens, attract text, hardware check, TEST menu all correct).
