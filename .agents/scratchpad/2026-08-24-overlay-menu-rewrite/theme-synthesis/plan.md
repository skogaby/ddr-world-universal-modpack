# Plan — task-02 theme-synthesis

Status: Approved 2026-08-25 (auto mode — approved-planning descent; see
context.md)

## Implementation order

1. Pure `src/services/avs_layeredfs/shader_layout.rs` + host tests
   (mounted in validate_overlay_draw.sh): the full
   (aa, persp, themes) matrix — planned containers; default program
   tuples (stock 0; persp EXACTLY 1 when present; themes last three in
   arrows/bubbles/wavefield order, VS idx = 1 + persp, PS idx 1..3);
   theme indices None when !themes.
2. overlay_draw: `publish_theme_programs` / `theme_program_indices`
   (OnceCell).
3. shader_synthesis.rs: `Plan.themes` + theme blob constants + separate
   degrade-only theme-blob resolution; `planned_names` (default when
   `persp || themes`); build_all arrow/judge arms gated `aa || persp`;
   default arm consumes `shader_layout::default_programs` + assembles
   VS/PS tables to match, with a runtime persp-index-1 verification
   (mismatch ⇒ WARN + themes degraded, container still written for
   persp); fingerprint `v3` + ` themes={}`; publish indices on both
   success paths.
4. Gates + cabinet boots (themes-on synthesis, cache hit, inspect,
   shader-fixes-off stock).

## Test scenarios (pure, red-first)

- `planned_matrix`: all 8 combos — (F,F,F) ⇒ nothing; (T,F,F)/(F,T,F)/
  (T,T,F) match today's behavior; themes=T adds default everywhere;
  (F,F,T) ⇒ default only.
- `default_programs_matrix`: (persp=F, themes=F) ⇒ empty-not-built
  marker/(unused); (T,F) ⇒ [(0,0,0),(0,1,0)]; (F,T) ⇒ [(0,0,0),
  (0,1,1),(0,1,2),(0,1,3)]; (T,T) ⇒ [(0,0,0),(0,1,0),(0,2,1),(0,2,2),
  (0,2,3)] — persp at index 1 asserted positionally.
- `theme_indices`: (F,T) ⇒ [1,2,3]; (T,T) ⇒ [2,3,4]; themes=F ⇒ None.
- `vs_ps_counts`: counts derivable per combo (vs = 1+persp+themes,
  ps = 1+3·themes).

## Runtime validation

- Boot A (themes on, persp per cabinet config): log shows default
  synthesized with expected program count; v3 sidecar; second boot
  cache-hits AND still publishes indices; `gsp_pack.py inspect` clean.
- Boot B (shader-fixes disabled in mod-config): stock shaders, index
  export unset (verified via a task-03 gate or log absence).
