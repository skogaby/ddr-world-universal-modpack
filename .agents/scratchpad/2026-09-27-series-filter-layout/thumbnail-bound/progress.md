# Progress — thumbnail-bound
- [x] `model::thumbnail_bound` / `legacy_thumbnail_bound` (harness-tested)
- [x] Enhanced init probes `jacket_thumbnails_{ja,ua}_<N>.arc` (install `data/` + LayeredFS),
      patches the imm8 only when a bound exists; legacy clamps to 127 with one WARN
- [x] Cabinet demo (plan Step 7) — awaiting maintainer test
## Deviations
- Implemented before Step 6's cabinet test (bundled test session).
## Cabinet (2026-09-27)
Maintainer tested num_columns 1–5 on the 20260915 cabinet: every layout rendered and behaved
correctly. 5-column log: 20 labels declared at 32x20 (fresh-mode batch, rebuilt), detours
installed, 9 patches, count 20, scroll ACTIVATED (4 rows), row-limit probe 20/20; no enhanced or
LayeredFS warnings; no loose-PNG auto-injection. Probe removed afterwards; `MAX_GRID_ROWS` kept at
24 (geometry: entry row tops < 864 for rows 0..=23).

Status: Complete (uncommitted — maintainer commits manually; custom-arc and legacy-clamp cabinet checks carried into the Step 8 matrix)
