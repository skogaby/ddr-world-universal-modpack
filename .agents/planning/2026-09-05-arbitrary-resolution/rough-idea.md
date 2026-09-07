# Rough Idea — Arbitrary Resolution Rendering

Captured 2026-09-05 from the maintainer's request.

Support arbitrary render resolutions for DDR World beyond the stock 1280×720:
480p, 1080p, 1440p, 4K as concrete targets, with ultra-widescreen as a
"potential" stretch target.

Constraints stated up front:

- **No offline asset reworking or remastering** as an up-front exercise. The
  target set is wide (480p through 4K), so a hi-res asset pack is out of scope
  for this feature.
- **Rely on altering rendering resolutions** for any 3D rendering (and, by
  extension, everything the engine rasterises from geometry — AFP shapes,
  HUD quads, arrows through the modpack's own shaders).
- **Where necessary, apply filters** (upscale / sampling filters) to help with
  2D bitmap asset scaling at non-native densities.

Starting point: `docs/arbitrary_resolution_research.md` (static RE, 2026-09-02,
primary build 20260616, AOB-verified across 20250805 / 20260721 / 20260825) —
its Tier A (native output + engine upscale), Tier A+ (better upscale shader),
and Tier B (native internal resolution) map directly onto this request; its
Tier C (hi-res asset pipeline) is explicitly excluded here.
