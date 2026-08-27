# Summary — Loading-screen speedup

Project: `.agents/planning/2026-08-11-loading-screen-speedup/`
Completed PDD run 2026-08-11; design + plan both `Status: Approved 2026-08-11`.

## Artifacts

- `rough-idea.md` — the two-change idea (RAM cache preload + enum→scalar) with
  the diagnosis context.
- `idea-honing.md` — 15-entry decision register, all Accepted (D9 Overridden to
  1-based display); phasing note; Readiness Confirmed 2026-08-11.
- `research/orientation.md` — code/RE findings + the post-design empirical
  reconciliation (138/208 slow-path CAUTION opens are `seop_op_item_*`; scene 18
  carries no injected textures).
- `design/detailed-design.md` — Phase 1 fully specified; Phase 2 (RAM cache,
  mechanism B) deliberately a sketch.
- `implementation/plan.md` — 4-step Phase 1 plan with checklist.

## Design in brief

Convert the 9 `EnumIndexed` WebUI cosmetic categories to scalar rows (native
digit rendering, 1-based display via a new display-only
`ScalarFormat::OffsetInteger { display_offset }`), keeping chrome + live-art
previews (`generate_chrome` moves to the scalar arm). Remove
`RenderMode::EnumIndexed`, `build_indexed_enum_values`, the
`ITEM_RIBBON_COUNT` generation in `scripts/gen_option_labels.py`, and the 150
committed `seop_op_item_*.png`. Expected: ~66% of CAUTION's slow-path texture
opens disappear. Phase 2 (preload `_cache/` into RAM, serve `avs_fs_read` from
memory) stays shelved unless Phase 1's measured win is insufficient.

## Next steps

Per user direction, code-task-generator is skipped; implementation proceeds
directly (code-assist style) through `implementation/plan.md` Steps 1–4 in
order, maintaining `progress.md` in this directory per repo convention.
Validation is `cargo check` → `cargo fmt` → `./build.sh` plus the Step-4
cabinet protocol (opens ~208 → ~70; scene-21 wall time vs ~7 s baseline; Phase-2
go/no-go decision).

## Assumptions / watch items

- D13: VIDEO SIZE (`EnumFixed`) untouched. D14: save wire format unchanged.
- The bespoke ribbons (`fullscreen`, `overhead`, `hallway`, `distant`) and all
  labels/previews continue to be generated and shipped.
- Cabinet `_cache/` keeps 150 dead files until an operator deletes `_cache/`
  (harmless — never requested again after the atlas rebuild).
