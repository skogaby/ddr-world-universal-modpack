# Rough idea — movies on the stage monitors (2026-09-23)

Source: maintainer request following the RE in `docs/background_dancers_research.md` §8 (A3's OFFSCREEN1
route for movies on the `monitor*` / `replicant*` stage screens).

- Implement the §8.6 suggestions in the Background Dancers mod, including keeping the screen
  materials unlit (no Lighting Style restyle, no outline hulls on them).
- Expose two more values on the GLOBAL SETTINGS "Background Movies" row:
  1. **Disable the background dancers entirely when the song has a background movie** — what A3 did by
     default (an ordinary movie song showed the movie instead of the 3D scene).
  2. **Render the movie on the stage screens** when the song's stage has them; when it does not, force
     the movie to THUMBNAIL.
- Songs without a movie: monitor stages show black screens. Acceptable for now; they stay in the random
  rotation until someone complains.
- Proof of concept for custom stages: re-export the shipped Griffin House stage
  (`data_mods/custom_models/stages/Griffin House/mapset_griffin00/`) from its Blender source (the
  maintainer's `ddr-peter-griffin/livingroom` Blender project, outside the repository) with the living-room
  TV screen sampling the `offscreen1` render target, so movies play on the TV. Use the local Blender install
  (`blender-local` skill).
