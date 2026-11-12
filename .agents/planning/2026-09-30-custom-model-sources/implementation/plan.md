# Implementation Plan — Custom dancer / stage SOURCES

Status: Approved 2026-09-30
Design: `design/detailed-design.md` (Approved 2026-09-30). Section numbers below (§) refer to it.

- [x] Step 1: Framework — texture aliases + family-aware row ordering
- [x] Step 2: Discovery — `sources.rs`, three-level walk, planner carries the source
- [x] Step 3: Pure pick layer — grouped catalog, per-element choices, source pools
- [x] Step 4: Rows — source + per-source model rows, requests, preview, art, config (cabinet) — cabinet-validated 2026-09-30
- [x] Step 5: Documentation and closeout

Conventions for every step: readiness gate = `cargo check --target x86_64-pc-windows-msvc` clean →
`cargo fmt` (whole crate) → `./build.sh` clean → the step's harness green. Pure files stay
`crate::`-free so the harnesses can mount them. Maintain `progress.md` in this planning directory
after each step (Updated / Status / NEXT ACTION / Done / Deploy & test log). No commits — the
maintainer commits.

---

## Step 1: Framework — texture aliases + family-aware row ordering

**Objective.** Land the two general `custom_options` capabilities the rows depend on (§4.9), with no
behaviour change for any existing row.

**Implementation.**
- `src/services/custom_options/api.rs`: `RegisterSpec { label_texture_like, preview_texture_like:
  Option<&'static str> }` (default `None`) + builders `label_texture_like(id)` /
  `preview_texture_like(id)`; document the aliasing in the `RegisterSpec` doc where `seop_item_<id>`
  is described today.
- `src/services/custom_options/registry.rs`: carry both fields on `RegisteredOption` (`try_register`
  copies them); `label_texture_name()` / `preview_image_base_name()` return the alias stem when set.
- `src/services/custom_options/mod.rs::register_option`: register the label atlas entry under the
  label stem (alias or id) instead of `id`.
- `src/services/custom_options/ordering.rs`: `compute_order` gains `parent: &[Option<usize>]` and the
  three-branch placement rule for unlisted non-headers (§4.9); `display_order_for` takes the same
  slice; module doc gets the new rule beside the existing ones.
- `src/services/custom_options/builder_hook.rs` and `registry.rs::overlay_snapshot_rows` (+ its
  `order_for` callback type and test helpers): build `parent` from each snapshot entry's `ShowWhen`
  parent id → `state.index_of` → position in the snapshot (`None` when absent).

**Tests** (`scripts/validate_custom_options.sh`, which already mounts `api.rs`, `registry.rs`,
`ordering.rs`):
- registry: alias set ⇒ `label_texture_name` / `preview_image_base_name` / `preview_image_names` use
  it; alias unset ⇒ unchanged (the existing header test keeps passing).
- ordering: shipped-config scenario (`background_dancer`, `background_stage` listed; source rows and
  per-source rows unlisted) ⇒ `[…, dancer_source, dancer, dancer_a, dancer_b, stage_source, stage,
  stage_a, …]`; unconfigured ⇒ identity minus headers (byte-identical to today); listed parent with
  unlisted children (training-mode shape) ⇒ children directly after the parent; parent absent from the
  snapshot ⇒ append (today); duplicate listed ids and unknown ids unchanged; headers still excluded
  when unlisted.

**Integration.** Nothing registers an alias or an unlisted family member yet; every existing row's
textures and order are unchanged.

**Demo.** `./scripts/validate_custom_options.sh` green with the new tests; `./build.sh` clean; a
cabinet boot shows every option row exactly as before.

---

## Step 2: Discovery — `sources.rs`, three-level walk, planner carries the source

**Objective.** Recognise source folders on disk and tag every accepted custom entry with its
`SourceRef`, while the game keeps showing today's flat rows (§4.1–§4.3, §5.1).

**Implementation.**
- NEW `src/mods/background_dancers/sources.rs` (pure): `IMPLICIT_SOURCE`, `RESERVED_SLUGS`,
  `MAX_SLUG_BYTES`, `SourceRef`, `CustomEntry`, `slug`, `resolve_source` (label via
  `custom_content::label_from_folder` + `fit_label`), `DirRole`, `dir_role`, `has_model_content`,
  `source_row_id`, `model_row_id`. Declare it in `mod.rs`; add it to `MODULE_NAMES` / `MODULE_PATHS`
  in `scripts/validate_background_dancers.sh`.
- `custom_content.rs`: `PackDir.source: Option<String>`; `Plan.labels` → `Plan.entries:
  Vec<CustomEntry>`; `plan()` resolves the source per `PackDir` (refused ⇒ one WARN per folder, arcs
  skipped; same-slug spellings ⇒ one INFO), attaches the `SourceRef` to folder-labelled and key-rule
  entries alike; `//!` layout block rewritten to §5.1.
- `custom_scan.rs::walk_kind_root`: the three-level walk of §4.2 using `dir_role` /
  `has_model_content`; `read_pack_dir` takes `source`. The summary INFO gains the per-source
  breakdown.
- `lifecycle.rs`: `Tables.custom: Vec<CustomEntry>`, `custom_entries_snapshot()`; the "tables ready"
  INFO counts sources.
- `mod.rs::enable`: TEMPORARY adapter — map `custom_entries_snapshot()` to `(key, label)` pairs for
  today's `build_catalog_with_custom` (removed in Step 3), so the flat rows keep working.

**Tests** (`scripts/validate_background_dancers.sh`):
- `sources.rs`: `slug` cases (`DDR STRIKE`, `J.C.`, `UMX2`, cap at 32 bytes, all-symbol ⇒ `None`);
  `resolve_source(None)` = `custom`/`CUSTOM`; `Some("Custom")` and `Some("CUSTOM ")` resolve to the
  same slug; `Some("Source")` ⇒ `Err`; `dir_role` truth table; `has_model_content` for model dir /
  body arc / part arc / stage arc / `_g` only / nothing; `model_row_id(base, None)` = base.
- planner: a source with two friendlies + a flat model + a source-level sidecar; a root friendly ⇒
  CUSTOM; a root flat model ⇒ CUSTOM; a `Custom/` source merging with the implicit one; a refused
  source (reserved slug) mounts nothing and WARNs once; key collision across two sources refused;
  existing planner tests migrated from `labels` to `entries` (labels themselves unchanged).

**Integration.** Discovery output flows into the same `Tables` and (through the adapter) the same flat
catalog as today; `arc_set` mounts are unchanged in shape.

**Demo.** With a test folder `data_mods/custom_models/dancers/Test Source/Some Dancer/pl_<key>/`
(copy of an existing dancer with a fresh key), the boot log reads `custom content -- … in 2 source(s):
CUSTOM 115, TEST SOURCE 1`, the dancer appears in today's flat BACKGROUND DANCER row, and the untouched
115 folders log no new WARN.

---

## Step 3: Pure pick layer — grouped catalog, per-element choices, source pools

**Objective.** Everything the rows and the per-song pick need that can be host-tested (§4.4, §4.6),
wired into `lifecycle` with no behaviour change yet.

**Implementation.**
- `catalog.rs`: `SourceCatalog`, grouped `Catalog`, the accessor set of §4.4;
  `build_catalog_with_custom(stages, dancers, custom: &[CustomEntry])` (STOCK block byte-identical to
  `build_catalog`; custom sources sorted by `(label, slug)`, entries by `(label, key)`, dedup by key).
- `selection.rs`: `StageChoice` / `DancerChoice`; `resolve_choice(rng, stages, dancers, stage,
  dancer_choices)` per §4.6; `source_stage_pool` (subset + `random_stage_pool`), `source_dancer_pool`;
  `PickSource::Source` (`tag()` = `"source"`).
- `pick.rs`: no code change expected (`summary()` renders any tag); add the `{source}` assertion.
- `lifecycle.rs::option_pick`: adapt to the new `resolve_choice` signature only — today's
  `Option<String>` choices map to `Key` / `Random(random_stages)` / `Random(&t.dancers)`.
- `mod.rs::enable`: remove the Step 2 adapter; pass `&custom_entries_snapshot()` to the grouped
  builder; `options::register` receives the grouped `Catalog` and, FOR THIS STEP, registers the two
  rows from `sources(kind)[0]` only when `has_custom` is false, else from a flat view (a shim inside
  `options.rs` that keeps today's two rows working over `STOCK ++ all custom entries`; replaced in
  Step 4).

**Tests** (`scripts/validate_background_dancers.sh`):
- catalog: STOCK source equals today's `build_catalog` output (26 / 25, same labels, values, key ↔
  label pairing — migrate the existing tests to `count(kind, 0)` etc.); grouping and sort order across
  three sources; `source_label(0/1/2…)`; `keys(kind, i)`; entries for non-candidate keys dropped;
  `has_custom` false with no custom; `clamp_to_catalog` unchanged.
- selection: all-`Random(global)` reproduces `pick_stage` then `pick_dancers` under the same seed
  (migrate `resolve_choice_rules`); `Key` unknown ⇒ `None`; `Random(pool)` never leaves the pool;
  `source_stage_pool` ⇒ `All` / `Filtered` / `NoneLeft` relative to the source, row order preserved;
  `source_dancer_pool` preserves table order and drops unknown keys.
- pick: `summary()` renders `{source}`.

**Integration.** `option_pick` compiles against the new selection API with identical draws; the
grouped catalog feeds a flat shim so the in-game rows are unchanged.

**Demo.** Harness green; a cabinet build behaves exactly as before (same rows, same picks for the same
seed — compare two boot logs' `random stage pool` / pick summary lines).

---

## Step 4: Rows — source + per-source model rows, requests, preview, art, config (cabinet)

**Objective.** Ship the user-visible feature end to end (§4.5, §4.7, §4.8, §4.10, §5.2, §5.3).

**Implementation.**
- `options.rs` rewrite: `RowInfo` / `RowRole` table built once (leaked ids), registration per §4.5
  (source row with `step_coarse(1)` + `preview_texture_like`; model rows with `ShowWhen::Equals`,
  `label_texture_like` + `preview_texture_like`, `step_coarse(5)`; `Duplicate` ⇒ re-show; failure ⇒
  WARN + rows not read); per-id `label` / `clamp_load`; readers `kind_for_option`, `rows_live`,
  `set_available` (all rows; stage ids (un)registered with `versus_mirror`), `Request`,
  `dancer_request` / `stage_request`, `row_choice_key`; the stage-mirror observer subscribed once
  (§4.5 step 5, Appendix B); the atomics and `on_*_change` removed; module `//!` rewritten.
- `lifecycle.rs::option_pick`: consume `Request`s per §4.7 (unknown key ⇒ WARN + `Any`; stage
  `Within` ⇒ `source_stage_pool` with the song's screen filter, `NoneLeft` ⇒ WARN naming the source;
  dancer `Within` ⇒ `source_dancer_pool`, empty ⇒ WARN + `Any`); provenance `Random` / `Source` /
  `Option`; a `Within` stage logs its own pool line.
- `preview/mod.rs::on_preview_request`: `kind_for_option` + `row_choice_key` (§4.8).
- `scripts/option_strings.py`: `background_dancer_source` / `background_stage_source` labels (en/ja/ko);
  run `scripts/gen_option_labels.py`; commit the six `seop_item_*_source.png` under
  `data_mods/custom_options/select_music_option_lang_{eng,jpn,kor}_v3_ifs/tex/`.
- `mod-config.json`: list `background_dancer_source` before `background_dancer` and
  `background_stage_source` before `background_stage` (`overlay: false`, `in_game: true`).

**Tests.** The request-resolution mapping (source value, model value, catalog) → `Request` is pure:
factor it as `options_logic.rs`-style free functions over `(Option<i32>, Option<i32>, &Catalog)` in a
`crate::`-free file mounted in `validate_background_dancers.sh` (cases: no source row ⇒ STOCK
semantics; `0` ⇒ `Any`; `S`+`0` ⇒ `Within` with S's keys and label; `S`+`k` ⇒ `Key`; out-of-range ⇒
`Any`; `row_choice_key` for model vs source rows). Engine wiring is cabinet-validated.

**Integration.** Replaces the Step 3 shim; the framework capabilities from Step 1 are exercised for
the first time; the discovery of Step 2 and the pure layer of Step 3 are consumed unchanged.

**Demo (cabinet — deploy the DLL, `data_mods/`, and the config change; run §7.3 of the design).**
1. Boot INFOs: sources with counts; rows INFO (`DANCER SOURCE (K sources) / BACKGROUND DANCER × rows`).
2. Options modal: DANCER SOURCE / STAGE SOURCE directly above their model rows; stepping the source
   swaps the model row on the same frame; RANDOM hides it; `RANDOM` / `STOCK` / `CUSTOM` / source
   labels render; coarse step 5 on model rows.
3. Preview: model row ⇒ its value; source row ⇒ effective pick or badge (R21).
4. Songs: `{random}` (source RANDOM), `{source}` (S + RANDOM; on a movie song with a screen-less
   source: the WARN, then a stage of S), `{option}` (explicit).
5. Versus: P1's stage source edit mirrors to P2 and P2's visible stage row follows; dancer rows
   independent.
6. Reboot: values persist per row; the old `background_dancer` value ≥ 27 loaded as RANDOM.

---

## Step 5: Documentation and closeout

**Objective.** Make the new contract discoverable and leave the repository consistent.

**Implementation.**
- `docs/background_dancers_research.md` §6: the three-level layout, implicit CUSTOM, slug / reserved /
  merge rules, row ids and values, the within-source screen rule.
- `README.md`: the custom-models paragraph (source folders, `Custom/`), the option-row description
  (DANCER SOURCE / STAGE SOURCE), the config note (`option_menu_settings` ids).
- `.agents/learnings/learnings.md`: only if the cabinet pass surfaced a non-obvious trap (e.g. an
  ordering or alias edge); otherwise nothing.
- `.agents/summary/*` is generated — do not hand-edit; note in `progress.md` that the
  codebase-summary workflow should be re-run (hook-ownership and components tables mention the rows).
- `progress.md`: final status, the deploy log, and the maintainer's follow-up (move the content into
  source folders; expect one-time cache repacks).

**Tests.** None new; re-run both harnesses and the readiness gate after the doc edits (doc-only, but
the `//!` blocks are code).

**Integration.** No code change; the feature from Step 4 is complete.

**Demo.** `git grep -nE "/(Users|home)/[^/ ]+/" -- . ':!target'` adds no new hits; README and the
research note describe the layout the Step 4 cabinet run exercised.
