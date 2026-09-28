# Summary — config-defined VERSION (series) filter layout

PDD run completed 2026-09-27. Design and plan approved 2026-09-27.

## Artifacts

| Path (repo-relative) | Content |
|---|---|
| `.agents/planning/2026-09-27-series-filter-layout/rough-idea.md` | the original request |
| `.agents/planning/2026-09-27-series-filter-layout/idea-honing.md` | decision register D1–D25 (all accepted; D2, D12, D19 overridden), `Readiness Confirmed 2026-09-27` |
| `.agents/planning/2026-09-27-series-filter-layout/research/orientation.md` | blind-spot pass: existing mod, game mechanics, findings that changed the idea |
| `.agents/planning/2026-09-27-series-filter-layout/research/builder-and-tabs.md` | VERSION builder, group tabs, selection primitives, CreateVisual timing — per-build addresses and AOBs |
| `.agents/planning/2026-09-27-series-filter-layout/research/predicate-and-persistence.md` | predicate `+0x34` patch, table readers, chip builder, mapper, 64-bit persistence, thumbnail loop |
| `.agents/planning/2026-09-27-series-filter-layout/research/label-pipeline.md` | per-image texture serving, donors, cached batch, mode-switch hazards |
| `.agents/planning/2026-09-27-series-filter-layout/research/label-prototype.md` | label look calibration (font, metrics, fitting rule) |
| `.agents/planning/2026-09-27-series-filter-layout/prototypes/` | throwaway label renderer + contact sheet (reference only; not implementation code) |
| `.agents/planning/2026-09-27-series-filter-layout/design/detailed-design.md` | approved design |
| `.agents/planning/2026-09-27-series-filter-layout/implementation/plan.md` | approved 8-step plan with checklist |
| `scripts/fonts/FOT-TSUKUGOPRO-B.OTF` | label font copied in at the maintainer's request (untracked so far) |

## Design in brief

`series_expansion.custom_series_enhanced` (`num_columns` 1–5 + `filters[]` of `label`,
`series_start`, `series_end`, `texture`, `group`) switches the mod into an enhanced mode that
replaces `custom_series`. The three stock GROUP tabs stay on the first line; config cells follow in
order, `num_columns` per row (template index = columns). The VERSION builder and the group-tab press
are detoured; the stock predicate is kept with a per-cell range end stored in table padding (one
4-byte patch); count, chip, per-song names, flare exclusion and scrolling are fed from the model.
Labels come from `data_mods/custom_series/series_labels/sefi_version_<texture>_<N>col.png`,
normalised and written straight into LayeredFS's per-image cache. Everything is fail-closed across
the five supported builds; legacy behaviour is unchanged when the key is absent.

## Plan in brief

1. label generator + art + contact sheet → 2. config schema, module split, pure model + harness →
3. signatures and derivations → 4. menu core (detours, table, predicate, chip, count, names) with
the row-limit probe → 5. label pipeline → 6. scrolling → 7. thumbnail policy + legacy clamp →
8. docs, release checks, cabinet matrix.

## Next steps

1. Run the code-task-generator sop against
   `.agents/planning/2026-09-27-series-filter-layout/implementation/plan.md` (one step at a time) to
   produce task files under `.agents/tasks/2026-09-27-series-filter-layout/step<NN>/`.
2. Run the code-assist sop on each task in order; keep `progress.md` in this directory current.

## Assumptions and open points to watch

- **Row limit (D24/R8):** ~24 grid rows get visuals; measured in Step 4. If the canonical 20 cells at
  1 column don't all get visuals, revisit before Step 6 (Component-level scrolling is the deferred
  alternative).
- **Fresh-mode texturelist entries** binding to the filter label layer is unverified; donor mode
  is the fallback (Step 5).
- **Label mount timing** after mod enable (no reboot splash, R19) holds today; re-check if boot
  order changes.
- **Network servers** may not round-trip saved `version` bits above the stock count.
- **Font licensing:** FOT-TsukuGo Pro is commercial — decide whether `scripts/fonts/` tracks it.
- **Selection bits follow cell order:** inserting or reordering cells remaps players' saved VERSION
  filters (accepted).
- The untracked stock extraction `select_music_option_v3_ifs/` at the repo root is reference-only;
  never stage it.
