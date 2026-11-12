# Progress — enhanced-label-pipeline
- [x] `ifs_textures::prebuild_texture` (wrapper over the existing `cache_texture`)
- [x] `atlas_cloner::generate_cloned_atlases_cached_with` + `BatchOptions` (existing fn delegates
      with the old sidecar name and latch — unchanged for current callers)
- [x] `enhanced::labels::prepare` — resolve, normalise into `_cache/custom_series_labels/`,
      prebuild blobs, fresh cached batch (`cser_enh`, own sidecar, no reboot latch), stale-cache
      guard, empty-set guard, rescan only on rewrite; `model::label_donor` (harness-tested)
- [x] Cabinet demo (plan Step 5) — awaiting maintainer test (fresh vs donor mode unverified)
## Cabinet (2026-09-27)
Maintainer tested num_columns 1–5 on the 20260915 cabinet: every layout rendered and behaved
correctly. 5-column log: 20 labels declared at 32x20 (fresh-mode batch, rebuilt), detours
installed, 9 patches, count 20, scroll ACTIVATED (4 rows), row-limit probe 20/20; no enhanced or
LayeredFS warnings; no loose-PNG auto-injection. Probe removed afterwards; `MAX_GRID_ROWS` kept at
24 (geometry: entry row tops < 864 for rows 0..=23).

Status: Complete (uncommitted — maintainer commits manually)
