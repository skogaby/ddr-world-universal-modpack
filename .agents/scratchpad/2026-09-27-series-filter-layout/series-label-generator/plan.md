# Plan — series-label-generator

Status: Approved 2026-09-27 (upstream: approved design + plan; auto mode)

## Test scenarios (`scripts/test_gen_series_labels.py`, unittest)
1. `render_label(text, cols)` returns an RGBA image of `CANVAS[cols] × 20` for every canonical
   text and cols 1..5.
2. Alpha bbox right edge ≤ `INK_LIMIT[cols]` for every canonical label and for a long synthetic
   label ("SUPERCALIFRAGILISTIC", "A VERY LONG SERIES NAME").
3. `EXTREME` at 5 cols: `layout_for` reports mode `condensed` (single line); `SuperNOVA2` at 5 cols
   reports `stacked` with lines ("Super", "NOVA2").
4. Rendering the same label twice gives identical bytes (PNG encode of both).
5. `canonical_config()` → dict with `num_columns == 3`, 20 filters, first WORLD 21–21, 2014 15–16,
   last 1stMIX 1–1; `json.dumps` round-trips.
6. `cells_from_config(dict)` with a `world_ruby` / `WORLD RUBY` cell returns it; invalid texture
   is skipped with a warning.
7. `build_preview(cells, stock_dir=None)` returns an image wide enough for five panels.
8. `load_font(path)` on a missing path raises `SystemExit` whose message has the repo-relative path.

## Implementation
- Constants from the prototype research (font size 15, scale 0.90, embolden 0, stack 10.5 px,
  baselines 16 / 9,19, ink limits, min ratio 0.70, colour #00B68C).
- Supersampled strip render (8×) so the base scale and optional embolden are smooth.
- `Layout` result (`mode`, `ratio`, `lines`) for tests and reporting.
- CLI: default render; `--from-config`, `--emit-config`, `--preview [PATH]`, `--out-dir`.
