# Progress — series-label-generator

- [x] Tests written and failing (import error before the module existed)
- [x] Rendering helpers
- [x] Canonical table + emit-config
- [x] from-config
- [x] preview
- [x] Generate art + review preview

## Cycles
1. `scripts/test_gen_series_labels.py` (12 tests) — failed: module missing.
2. `scripts/gen_series_labels.py` — 12/12 pass (`python3 -m unittest test_gen_series_labels`, from `scripts/`).
3. Default run: 100 PNGs in `data_mods/custom_series/series_labels/`; two runs byte-identical;
   `--preview` → `target/series_labels_preview.png` matches the approved prototype look;
   `--from-config` renders 5 widths for a `world_ruby` cell; `--emit-config` prints the block.

## Deviations
- `--preview` writes only the contact sheet (no label files), so collaborators can preview without
  touching committed art.
- `--emit-config` prints a `"custom_series_enhanced": {...}` fragment (paste-ready inside
  `series_expansion`); wrapping it in braces yields valid JSON for fixtures.

Status: Complete (uncommitted — maintainer commits manually)
