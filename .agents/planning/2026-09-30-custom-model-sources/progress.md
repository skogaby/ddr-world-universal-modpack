# Progress — Custom dancer / stage SOURCES

Updated: 2026-09-30
Status: Steps 1–5 code + docs complete — CABINET VALIDATION of Step 4 PENDING (maintainer)
NEXT ACTION (maintainer): deploy `target/x86_64-pc-windows-msvc/release/ddr_world_hook.dll` + the whole
`data_mods/` tree (six new `seop_item_background_{dancer,stage}_source.png`) + the `mod-config.json`
`option_menu_settings` change (or add the two `*_source` ids to the cabinet's own config), then run the
checklist under "Deploy & test log" below. Optionally create `data_mods/custom_models/dancers/Test Source/`
holding a copy of one dancer folder with a fresh key to see a second source appear before moving content.

Resume protocol: read `implementation/plan.md` (checklist + steps), `design/detailed-design.md`
(§ references), then this file. Per-task working records live under
`.agents/scratchpad/2026-09-30-custom-model-sources/<task>/` (code-assist's `progress.md` with a
`Status: Complete` line marks a finished task). Maintainer authorised autonomous progression through the
steps until a cabinet test is needed (2026-09-30); no commits — maintainer commits.

## Done

- PDD: register accepted, design + plan approved (2026-09-30).
- Step 1 (framework): `RegisterSpec::label_texture_like` / `preview_texture_like` (+ `RegisteredOption`
  stems, `register_option` registers the label under the stem); family-aware `compute_order`
  (`parent_positions`, 3-branch rule) threaded through `builder_hook` and `overlay_snapshot_rows`.
  `validate_custom_options.sh` 72/72 (was 59); `cargo check` clean. Task records:
  `.agents/scratchpad/2026-09-30-custom-model-sources/{texture-aliases,family-aware-ordering}/`.

- Step 2 (discovery): NEW pure `sources.rs` (slug / resolve_source / dir_role / has_model_content /
  row ids); `custom_content.rs` `PackDir.source`, `Plan.entries: Vec<CustomEntry>` (+ `labels()`,
  `source_counts()`), `SourceResolver` (refused ⇒ WARN + skip; same-slug ⇒ merge INFO), `//!` contract;
  `custom_scan.rs` three-level walk + per-source INFO; `lifecycle::Tables.custom` /
  `custom_entries_snapshot()`; `mod.rs` flat adapter (Step 3 replaces). background_dancers harness
  207/207 (was 196); `./build.sh` clean. Walker itself is cabinet-validated at Step 4.

- Step 3 (pure pick layer): `catalog.rs` grouped (`SourceCatalog`, `Catalog::{sources, has_custom,
  source_count, source_label, count, entry, label, key, keys, flat_entries}`, STOCK block byte-identical);
  `selection.rs` `StageChoice` / `DancerChoice` / `resolve_choice` / `source_stage_pool` /
  `source_dancer_pool` / `PickSource::Source`; `option_pick` adapted (identical draws); `options.rs`
  carries a Step-3 `Flat` shim (Step 4 replaces). Harness 211/211; `./build.sh` clean.

- Step 4 (rows — code): NEW pure `options_logic.rs` (row table, `Request`, `request_for`,
  `row_choice_key`, `row_max`, `label_for_row`; 4 tests); `options.rs` rewritten (source row +
  per-source model rows with `ShowWhen::Equals` + texture aliases, `Duplicate` re-show, versus mirror via
  a value-changed observer, `stage_request` / `dancer_request` / `row_choice_key`); `lifecycle::option_pick`
  consumes requests (within-source pools, screen-rule fallback WARN, `{source}` provenance);
  `preview/mod.rs` wired; `DANCER SOURCE` / `STAGE SOURCE` label PNGs (en/ja/ko); shipped `mod-config.json`
  lists the two source ids. Gate: `cargo check` clean, `cargo fmt`, harnesses 215 / 72, `./build.sh` clean.

- Step 5 (docs): `docs/background_dancers_research.md` §6.1 "Sources" (layout, identity, rows, pick,
  pure-vs-engine); README "Custom Background Dancers and Stages" paragraph + mod table mention. No
  learnings entry yet (none surfaced before the cabinet pass). `.agents/summary/*` is generated — re-run
  the codebase-summary workflow after the pass (components / hook-ownership / data-models mention the rows).

## In flight

- Cabinet validation of Step 4 (maintainer) — checklist below. Fix-ups from the pass, a learnings entry
  if the pass surfaces a trap, then the maintainer's content move into source folders.

## Deploy & test log

Checklist for the Step 4 pass (design §7.3) — record outcomes here:
1. Boot: `custom content -- N dancer(s) + M stage(s) … in K source(s): CUSTOM 115[, TEST SOURCE 1]`;
   `tables ready … custom in K source(s)`; `option rows live -- DANCER: source row + K model rows (…);
   STAGE: stock row only (…)` (stages have no custom source until stages move too — Grove Street /
   Griffin House are legacy ⇒ CUSTOM ⇒ a STAGE SOURCE row with RANDOM · STOCK · CUSTOM appears).
   No new WARNs for the untouched legacy folders.
2. Options modal: DANCER SOURCE directly above BACKGROUND DANCER; stepping the source swaps the model row
   on the same frame; RANDOM hides it; labels `RANDOM` / `STOCK` / `CUSTOM` / source names render; the
   source row steps by 1, model rows coarse-step by 5 (Start held).
3. Preview: a model row focused previews its value; the source row focused previews the effective pick
   (or the RANDOM badge).
4. Songs: source RANDOM ⇒ `{random}`; source S + RANDOM ⇒ `{source}` (+ the `no stage in source …`
   WARN on a movie song when S has no screen stage, then a stage of S); explicit ⇒ `{option}`.
5. Versus: P1's STAGE SOURCE / stage row edits mirror to P2 (and P2's visible stage row follows);
   dancer rows independent.
6. Reboot: every row value persists (`custom_options.p1/p2.background_dancer_*`); an old
   `background_dancer` value ≥ 27 loads as RANDOM; a value under a removed source id disappears at the
   next save.

## Deviations & open questions

- D5 refined at design time: same-slug source folders MERGE (not skip); only the reserved slug `source`
  and unprintable names are refused.
- `source_stage_pool` returns `(subset, StagePool)` (the task allowed either shape).
- Legacy friendly folders holding a nested model-bearing subdirectory now promote to a SOURCE (was: the
  nested dir was ignored) — documented in the `custom_content.rs` layout block; note in README at Step 5.

## Key facts for a cold resume

- Feature lives in `src/mods/background_dancers/`; framework in `src/services/custom_options/`.
- Harnesses: `scripts/validate_custom_options.sh` (framework), `scripts/validate_background_dancers.sh`
  (mod pure layers). Engine wiring is cabinet-only (Step 4).
- Readiness gate per step: `cargo check --target x86_64-pc-windows-msvc` → `cargo fmt` → `./build.sh`.
- Step 4 deploy must carry `data_mods/` (new label PNGs) and the shipped `mod-config.json` change.
