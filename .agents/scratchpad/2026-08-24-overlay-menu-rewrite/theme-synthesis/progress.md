# Progress — task-02 theme-synthesis

- [x] pure shader_layout.rs + host tests (harness mount)
- [x] overlay_draw index export (publish/read)
- [x] shader_synthesis: themes plan input + degrade-only blob resolution
- [x] shader_synthesis: default-in-all-theme-configs + append-last + persp assert
- [x] fingerprint v3
- [x] gates: harnesses → cargo check → cargo fmt → ./build.sh
- [x] cabinet boots: synthesis + cache-hit + inspect + shader-fixes-off

## Log

- 2026-08-25: pure `src/services/avs_layeredfs/shader_layout.rs`
  (`planned` / `default_programs` / `default_theme_indices` /
  `default_table_counts` / `PERSP_PROGRAM_INDEX=1`; 5 tests covering
  the full (aa,persp,themes) matrix, positional persp assert, indices =
  last three); mounted in validate_overlay_draw.sh (17 tests total).
- 2026-08-25: overlay_draw `publish_theme_programs([u32;3])` +
  `theme_program_indices()` (OnceCell; first write wins; one INFO).
- 2026-08-25: shader_synthesis — `Plan.themes` (shader-fixes ∧ mod-menu
  ∧ all 4 theme blobs; missing theme blob = soft degrade w/ one WARN,
  AA/persp untouched); plan viable on `aa||persp||themes`; arrow/judge
  arms gated `aa||persp`; default arm built when `persp||themes`, VS
  `[stock, persp?, theme_vs?]` / PS `[stock, arrows?, bubbles?,
  wavefield?]`, programs from `shader_layout::default_programs` with a
  defensive persp-at-1 + table-count re-verification (violation ⇒ Err ⇒
  stock — a wrong container must never ship); fingerprint v3 + themes
  bit (theme blob hashes ride the existing blob_paths loop);
  `publish_theme_indices` on BOTH the fresh-build and cache-hit success
  paths (the cache-hit path skips build_all — caught in planning).
- Gates: overlay_draw harness 17/17, check 0 warnings, fmt, build clean.

## Deploy & runtime validation (2026-08-25, cabinet)

- Boot A (aa+persp+themes): `synthesizing (aa=true, persp=true,
  themes=true)`; default → 8236 B, **5 programs, 3 VS, 4 PS**;
  `theme programs published (arrows=2, bubbles=3, wavefield=4)`.
  `gsp_pack.py inspect` confirms program table [(0,0,0),(1,0),(2,1),
  (2,2),(2,3)] + the exact blob sizes/instr counts from task-01;
  fingerprint sidecar `v3 aa=true persp=true themes=true ...`.
- Boot B (cache hit): `cache up to date (3 containers)` AND the index
  publish still fires. 0 panics.
- Boot C (shader-fixes disabled): `shader-fixes mod disabled — stock
  shaders`, NO publish line (export unset). Config restored.

## Deviations
- The runtime persp-contract check fails the WHOLE default build (Err ⇒
  stock) instead of degrading themes only: if programs[1] weren't the
  persp program the container would be wrong for pass_rewrite, which
  binds program 1 blind — shipping it in any form is unsafe.

Status: Complete (uncommitted — maintainer commits manually)
