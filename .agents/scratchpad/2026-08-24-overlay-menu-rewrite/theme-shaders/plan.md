# Plan — task-01 theme-shaders

Status: Approved 2026-08-25 (auto mode — approved-planning descent; see
context.md)

## Files
- `shaders/src/themes/theme_common.hlsl` — shared `vs_theme_main`
  (vs_3_0): pos passthrough `float4(pos.xy,0,1)`; NDC→pixel; rect UV
  `(px − c48.yz)/c49.xy`; TEXCOORD1 = {time, p0, p1, aspect}; COLOR0
  passthrough.
- `shaders/src/themes/theme_arrows.hlsl` — `ps_main` (ps_3_0):
  2–3 layers of upward-scrolling chevron/arrow SDF cells over a deep
  blue/purple vertical gradient; low contrast.
- `shaders/src/themes/theme_bubbles.hlsl` — hash-grid SDF circles,
  slow upward drift + sine bob, dark teal base, faint warm rim.
- `shaders/src/themes/theme_wavefield.hlsl` — grid lines over a
  y-displaced traveling-wave field, charcoal base, green lines.
- `scripts/build_shaders.sh` — 4 new BLOBS lines →
  `theme_passthrough.vs.d3dbc`, `theme_{arrows,bubbles,wavefield}.ps.d3dbc`.

## Validation
- `./scripts/build_shaders.sh` clean on the fxc golden path (version
  pin passes); all 4 new blobs non-empty; existing 4 blobs byte-stable.
- Version tokens: VS 0xFFFE, PS 0xFFFF (script's stats dump +
  gsp_pack).
- Instruction counts comfortably inside SM3 (stats dump).
- Source review: only c48/c49 referenced in the VS; NO constant
  registers in any theme PS; every time-dependent term uses
  wrap-seamless frequencies (f·3600 ∈ ℤ).
