# Progress — enhanced-scrolling
- [x] `series_filter_scroll::Tracking` (`Template2` legacy / `Registered`), `begin_build`,
      `register_entry`, `on_registered_visual` (layer id recorded/refreshed; activation when every
      registered button has one; re-mask on refresh while active)
- [x] Builder detour registers cells (row = index / columns); enhanced enable configures
      Registered tracking; legacy configure passes `Template2`
- [x] Cabinet demo (plan Step 6) — awaiting maintainer test
## Deviations
- Implemented before the Step 4 row-limit probe was observed (maintainer: test once); the probe
  stays in until the cabinet numbers confirm `MAX_GRID_ROWS`.
## Cabinet (2026-09-27)
Maintainer tested num_columns 1–5 on the 20260915 cabinet: every layout rendered and behaved
correctly. 5-column log: 20 labels declared at 32x20 (fresh-mode batch, rebuilt), detours
installed, 9 patches, count 20, scroll ACTIVATED (4 rows), row-limit probe 20/20; no enhanced or
LayeredFS warnings; no loose-PNG auto-injection. Probe removed afterwards; `MAX_GRID_ROWS` kept at
24 (geometry: entry row tops < 864 for rows 0..=23).

Status: Complete (uncommitted — maintainer commits manually)
