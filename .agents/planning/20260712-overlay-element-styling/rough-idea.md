# Rough Idea — Gameplay Overlay Element Styling (Scale & Opacity)

Add a mod exposing user-settable **scale** and **opacity** for the dynamic
feedback elements drawn over the playfield during gameplay:

- the **combo counter** (`dance_combo_root{1,2,3}`, `ComboActor`)
- the **judgement text** (`dance_judge`), including the **freeze O.K./N.G.**
  clips (`dance_judge_for_freeze`) and the **FAST/SLOW** display
  (`dance_fast_slow`)
- the **pacemaker score tracker** (`dance_score_compare`)

**Explicitly excluded** (maintainer decision): the receptor hit flashes
(`dance_effect` clips) are not to be modified.

## Prior work

The RE groundwork is complete and validated on both supported gamemdx builds
(20260616 + 20260324) in `docs/gameplay_overlay_elements_research.md`:

- All elements are BM2D CMovieClip pool wrappers around engine AFP layers.
- **Capture**: one cold-path detour on `CMovieClip::Create` (template name
  arrives as a C string in R8) identifies every element wrapper at creation.
- **Scale**: one-shot `afp_layer_set_matrix(id, {s,0,0,s,0,0})` — the layer
  matrix has exactly one writer in gamemdx (SetRotation, never called on these
  clips), composes with position, and anchors at the element's visual center.
- **Opacity**: multiplicative color-transform alpha. Judge/freeze/fast-slow are
  never colored by the game → one-shot `afp_layer_set_color(id,1,1,1,a)`.
  Combo and pacemaker alpha is game-managed state (visibility gating / negative
  dim) → compose multiplicatively in a detour on the wrapper SetColor method
  (arg order `(this, a, r, g, b)` — alpha first), optionally also the
  int-percent variant.
- AOB signatures for all targets built and verified unique/disambiguated on
  both builds (color-twin IAT disambiguation is mandatory — twin order flips
  between builds).
