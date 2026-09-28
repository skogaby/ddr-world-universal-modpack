# Progress — config-defined VERSION (series) filter layout

Updated: 2026-09-27
Status: Step 8 of 8 — in progress (docs + release checks done; remaining cabinet matrix items with
the maintainer). Everything uncommitted.
NEXT ACTION: maintainer runs the "Remaining matrix" list below; record results in the deploy log,
fix anything found, then tick Step 8 and close
`.agents/scratchpad/2026-09-27-series-filter-layout/docs-and-release/progress.md`.
Resume protocol: read `implementation/plan.md` (checklist + Step N), then `design/detailed-design.md`;
per-task records live in `.agents/scratchpad/2026-09-27-series-filter-layout/<task>/`.

## Done

- Step 1 — `scripts/gen_series_labels.py` (+ `scripts/test_gen_series_labels.py`), 100 labels in
  `data_mods/custom_series/series_labels/`.
- Step 2 — module split to `src/mods/series_expansion/`, lenient `SeriesConfig`,
  `enhanced/model.rs` + `scripts/validate_series_expansion.sh`.
- Step 3 — four signatures + `derive_series_enhanced` + `series_enhanced_sites()`.
- Step 4 — table, predicate `+0xB8 → +0x34`, chip/count/names, builder + group-press detours.
- Step 5 — `ifs_textures::prebuild_texture`, `atlas_cloner::generate_cloned_atlases_cached_with`,
  `enhanced/labels.rs` (fresh-mode batch works; no donor-mode fallback needed).
- Step 6 — `series_filter_scroll::Tracking::Registered`.
- Step 7 — thumbnail bound from existing custom arcs; legacy clamp to 127.
- Step 8 (part) — module docs, RE-doc addenda (§15), learnings, release-archive comment, probe
  removed, all gates green.

## Deploy & test log

- 2026-09-27 — cabinet (20260915), canonical 20 cells, `num_columns` 1–5 one boot each:
  maintainer reports every layout looked right in game. 5-column boot log: 20 labels declared at
  32x20 (fresh-mode batch rebuilt), builder + group press detoured, 9 patches, VERSION count 20,
  scroll ACTIVATED (4 rows / 9 visible), row-limit probe 20 of 20, "no custom jacket_thumbnails
  arcs — thumbnail loop left stock", no enhanced / atlas_cloner / LayeredFS warnings, no `ctex`
  auto-injection. Probe then removed.

## Remaining matrix (design §7; maintainer)

1. Filtering Results per cell (stock musicdb: WORLD 289, 2014 107, 1stMIX 3); multi-select;
   AND with LEVEL; chip text (`DDR WORLD～A20` for the first four).
2. GROUP GOLD / WHITE / CLASSIC select 4 / 3 / 13 cells; Simple and Normal; range select.
3. Selection survives a credit.
4. `"filters": []` → tabs only; one invalid cell → one WARN, rest load; a missing texture → WARN,
   blank cell.
5. Enhanced key removed → legacy menu exactly as before (WORLD RUBY / SAPPHIRE labels); legacy →
   enhanced → legacy → enhanced boots keep correct labels; mod toggled off in the overlay menu →
   stock menu.
6. Optional: a `jacket_thumbnails_ja_30.arc` in a mod folder → log "loaded up to series 30"; a
   legacy `series_value` 200 → boot completes with one WARN.

## Deviations & open questions

- Maintainer instruction (2026-09-27): proceed autonomously until a cabinet test is needed; Steps
  2–7 were implemented ahead of one bundled cabinet test.
- Font `scripts/fonts/FOT-TSUKUGOPRO-B.OTF` may be committed (maintainer, 2026-09-27).
- `MAX_GRID_ROWS` = 24 (geometry; 20 rows at 1 column confirmed on the cabinet).
- Follow-up: regenerate `.agents/summary/` so the hook-ownership map lists the two new detours.

## Key facts for a cold resume

- Enhanced mode is keyed on `series_expansion.custom_series_enhanced` (`num_columns`, `filters[]`).
- Label art: `data_mods/custom_series/series_labels/sefi_version_<texture>_<N>col.png`.
- Never stage the untracked stock extractions `select_music_option_v3_ifs/`,
  `select_music_option_lang_eng_v3_ifs/` at the repo root.
- Never add `custom_series_enhanced` to the committed `mod-config.json`.
- `git mv` already staged the `series_expansion.rs` → `series_expansion/mod.rs` rename.
