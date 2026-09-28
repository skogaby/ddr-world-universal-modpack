# Progress — enhanced-menu-core
- [x] `enhanced::model::encode_row` (pure, harness-tested: SSO ≤ 15, heap form > 15, inert rows)
- [x] `enhanced::build_table` (near-allocated, never freed; leaked long labels)
- [x] `enhanced::hooks` — builder detour (tabs + cells, factory destroyed once), group-press
      detour (clear / set members / notify), pass-through unless active, one-shot row-limit probe
- [x] `SeriesExpansionMod`: `init_enhanced` / `enable_enhanced` (detours before patches; predicate
      LEA + `+0xB8 → +0x34`; chip count N; names; flare; count detour); `disable` restores in reverse;
      legacy code split into shared helpers without behaviour change
- [x] cargo check / fmt / build clean; harness 14/14; sweep ALL GREEN
- [x] Cabinet demo (plan Step 4) — awaiting maintainer test
## Cabinet (2026-09-27)
Maintainer tested num_columns 1–5 on the 20260915 cabinet: every layout rendered and behaved
correctly. 5-column log: 20 labels declared at 32x20 (fresh-mode batch, rebuilt), detours
installed, 9 patches, count 20, scroll ACTIVATED (4 rows), row-limit probe 20/20; no enhanced or
LayeredFS warnings; no loose-PNG auto-injection. Probe removed afterwards; `MAX_GRID_ROWS` kept at
24 (geometry: entry row tops < 864 for rows 0..=23).

Status: Complete (uncommitted — maintainer commits manually)
