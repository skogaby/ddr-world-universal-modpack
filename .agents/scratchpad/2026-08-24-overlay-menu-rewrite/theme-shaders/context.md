# Context — task-01 theme-shaders (Step 8, overlay-menu rewrite)

## Task
.agents/tasks/2026-08-24-overlay-menu-rewrite/step08/task-01-theme-shaders.code-task.md

## Approval chain (verified)
Task Generated-By code-task-generator 2026-08-25; source plan Status:
Approved 2026-08-24; design Status: Approved 2026-08-24; breakdown
maintainer-approved 2026-08-25 (layer-identity gate, wrapped time,
MINIMAL greyed, soak rides play). Mode: auto.

## Build command
`./scripts/build_shaders.sh` (fxc 9.29 in the CrossOver bottle — present
on this machine; version-pinned). No Rust changes this task.

## Facts
- Vertex declaration (from gs_screencommand_default.hlsl VSIn):
  `float3 pos : POSITION` (NDC), `float2 uv : TEXCOORD0`,
  `float4 col : COLOR0`. Stock VS maps NDC→pixel via
  `x_px=(pos.x+1)*640, y_px=(1-pos.y)*360` (1280×720 canvas).
- Theme program pairs its OWN VS+PS (self-contained — no stock-PS
  contract to preserve, unlike the persp VS).
- No PS-constant record exists: everything rides VS interpolators.
- c48 = {time, rect_x, rect_y, unused}; c49 = {rect_w, rect_h, p0, p1}
  (design §5). reg binding precedent: `register(c48)/(c49)` in the
  persp VS.
- BLOBS manifest supports subdir names (`themes/theme_arrows` →
  `shaders/src/themes/theme_arrows.hlsl`); fxc paths must be
  repo-relative.
- Alpha blending is active for tag-0x03 quads (POC's 50 % black quad
  rendered translucent).
- Time arrives wrapped mod 3600 s: every scroll speed / frequency is
  chosen so `speed*3600` (or `f*3600`) is an integer ⇒ seamless wrap.

## Decisions (auto mode)
- PS files are self-contained (local PSIn structs; SM3 links by
  semantics) — no fxc include-path risk under wine.
- PS alpha ≈ 0.92 base (near-opaque dark backdrop; the pattern shows
  through the translucent panel; lowering MENU OPACITY reveals more
  animation), multiplied by the quad's COLOR0 alpha so the emitter
  keeps a master-fade lever.
- Aspect ratio forwarded in the interpolator (rect_w/rect_h) so
  patterns can stay square.
