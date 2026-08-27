# Progress — task-01 theme-shaders

- [x] theme_common.hlsl shared passthrough VS (vs_theme_main)
- [x] theme_arrows / theme_bubbles / theme_wavefield PS effects
- [x] build_shaders.sh BLOBS manifest (+4 lines, subdir names)
- [x] full fxc golden-path build; blobs verified

## Log

- 2026-08-25: `shaders/src/themes/` created — `theme_common.hlsl`
  (vs_theme_main: pos passthrough, NDC→pixel→rect-normalized UV from
  c48/c49, TEXCOORD1 = {time, p0, p1, aspect}, COLOR0 passthrough),
  `theme_arrows.hlsl` (2-layer parallax chevron-arrow field, deep
  blue/purple, scroll speeds 45/90 per 3600 s — wrap-seamless),
  `theme_bubbles.hlsl` (2-layer hash-grid SDF bubbles w/ 0.25 Hz bob +
  warm rims, dark teal), `theme_wavefield.hlsl` (traveling-wave grid,
  0.20/0.15 Hz, green-on-charcoal). All PS: self-contained PSIn (no
  includes — no fxc include-path risk), NO constant registers, alpha
  0.92 × quad COLOR0.a (master-fade lever).
- 2026-08-25: build_shaders.sh manifest extended; full build on the
  fxc 9.29 golden path (bottle) — all 8 blobs compile:
  theme_passthrough.vs 496 B (14 instr), theme_arrows.ps 1852 B
  (91 instr), theme_bubbles.ps 3084 B (164 instr), theme_wavefield.ps
  1084 B (45 instr) — all far inside SM3 limits; version tokens correct
  per the stats dump; the 4 pre-existing blobs byte-stable (md5 diff).

## Deviations
- None. Visuals are the agent-authored first cut (maintainer tunes at
  the Step 8 demo; effects are single-file HLSL edits + a rebuild).

Status: Complete (uncommitted — maintainer commits manually)
