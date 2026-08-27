# Plan — chrome-synthesis (Step 4 task-01)

Status: Approved 2026-08-24 (auto mode — approval supplied by the verified chain:
task Generated-By + plan/design both `Status: Approved 2026-08-24`; see context.md)

## Implementation shape

One new pure module `src/mods/mod_menu/chrome.rs` (no `crate::` imports; only
`image`), mirroring `model.rs`'s harness-mountable shape and `strip_synth.rs`'s
encode/error idioms. Wire `pub(crate) mod chrome;` into `mod.rs`. Extend
`scripts/validate_mod_menu.sh`: `MODULES=(model.rs chrome.rs)` + `image = "0.25"`
in the generated temp-crate `[dependencies]`.

Internal structure:
- Constants: `LAYOUT_VERSION=1`, `PANEL_W=1160`, `PANEL_H=600`,
  `PANEL_CORNER_RADIUS=20.0`, `STRIP_W=64`, `STRIP_H=16`, `STRIP_CORNER_RADIUS=6.0`,
  `OPACITY_MIN=25`, `OPACITY_MAX=100`, `OPACITY_STEP=5`.
- `clamp_opacity`: clamp → `(c + 2).div_euclid(5) * 5` → re-clamp (snap_rate_percent
  formula).
- `opacity_alpha(percent) -> u8`: `round(percent * 255 / 100)` (integer:
  `(p*255 + 50)/100`).
- Shared coverage helper `rounded_rect_coverage(x, y, w, h, r) -> f32`:
  signed distance to the rounded-rect boundary at the pixel center;
  coverage = `(0.5 - d).clamp(0.0, 1.0)` (1-px AA band; interior 1.0, exterior 0.0).
- `synthesize_panel`: per-row RGB lerp of gradient stops (row t = y/(h-1)); per-pixel
  alpha = `round(opacity_alpha × coverage)`. RGB written unpremultiplied.
- `synthesize_strip`: white RGB, alpha = `round(255 × coverage)`.
- `encode_png` + `ChromeError { Png }` with `describe()`.
- `cache_key_material(theme, opacity)` = `format!("chrome:v{LAYOUT_VERSION}:theme={theme}:opacity={opacity}")`.
- `panel_file_stem` = `format!("mm_panel_{theme}_{opacity}")`; `strip_file_stem()` =
  `"mm_strip"`.
- `DEFAULT_GRADIENT`: dark navy-ish top → near-black bottom (matches the pre-theme
  COL_* direction; exact bytes are a visual-tuning knob for the maintainer).

## Test scenarios (fail against absent implementation; all in-module #[cfg(test)])

1. `panel_dimensions` — synthesize @ 80 ⇒ 1160×600.
2. `strip_dimensions` — 64×16.
3. `panel_corner_profile` — (0,0) alpha 0; center alpha 204 (80 %); top-edge midpoint
   (W/2, 0) alpha 204; a diagonal sample just inside the corner arc (e.g. (5,5) with
   r=20 is OUTSIDE ⇒ 0, (16,16) is inside ⇒ >0 and ≤204); at least one pixel along
   the arc strictly between 0 and 204 (AA band exists).
4. `strip_corner_profile` — (0,0) alpha 0; center alpha 255, RGB white; AA pixel
   strictly between.
5. `opacity_mapping` — center alpha: 100 ⇒ 255; 25 ⇒ 64 (round(63.75)); 50 ⇒ 128.
6. `clamp_table` — (0,25) (24,25) (25,25) (82,80) (83,85) (100,100) (101,100)
   (-10,25) (60,60).
7. `gradient_endpoints` — top-row center RGB == top stop, bottom-row center == bottom
   stop, mid-row strictly between per channel (for stops chosen distinct).
8. `cache_key_stability` — same inputs identical; opacity differs ⇒ differs; theme
   differs ⇒ differs; contains the layout version. Stems likewise distinct.
9. `png_magic` — encode_png ok, first 8 bytes == PNG signature.

## Work checklist → progress.md

## Risks
- f32 coverage rounding at exact edges: tests sample well inside/outside the AA band
  except the one strict-between probe, which scans the arc for ANY intermediate
  pixel rather than pinning a coordinate.
