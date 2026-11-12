# Framework plumbing — exact touch points for the design

Verified against the source on 2026-09-30. All paths repo-relative.

## 1. Texture aliases (D11)

| Where | Today | Change |
|---|---|---|
| `src/services/custom_options/registry.rs:67` `RegisteredOption::label_texture_name()` | `format!("seop_item_{}", self.id)` | alias-or-id stem |
| `registry.rs:76` `preview_image_base_name()` | `format!("seop_image_{}", self.id)` | alias-or-id stem; `preview_image_name_for_value` / `preview_image_names` already build on it |
| `registry.rs:157` `try_register` | copies `RegisterSpec` fields | copy the two new `Option<&'static str>` fields |
| `src/services/custom_options/api.rs:417` `RegisterSpec` | no alias fields | `label_texture_like: Option<&'static str>`, `preview_texture_like: Option<&'static str>` + two builders |
| `src/services/custom_options/mod.rs:256` `register_option` | `asset_gen::register_label_for(id)` (builds `seop_item_{id}` at `asset_gen.rs:389`) | pass the label stem (alias or id); `register_label_for` already dedups (`asset_gen.rs:239`) |
| `mod.rs:269` | `state.preview_image_names_for(id)` | unchanged — reads through `preview_image_base_name()` |
| `header_rows_tests.rs:139` | asserts `seop_item_hdr_preview` | unchanged (no alias) |

Consumers of `label_texture_name()` in `rows.rs` (L1041, 1470, 1576, 1826) need no change. The
`validate_custom_options.sh` harness mounts `api.rs` + `registry.rs`, so alias resolution is
host-testable.

## 2. Family adjacency in row ordering (D10)

`src/services/custom_options/ordering.rs:104 compute_order(registered: &[&str], is_header: &[bool],
configured) -> (Vec<usize>, Vec<String>)`:

1. listed ids in listed order (L126–144);
2. unlisted non-headers appended in registration order (L148–152).

Callers build parallel slices from the per-open snapshot:
- `src/services/custom_options/builder_hook.rs:212–218` (ids + `is_header`), snapshot tuples are
  `(OptionHandle, id, RowKindTag)` — the `ShowWhen` parent is reachable via
  `state.options[idx].show_when` → `state.index_of(parent_id)` → position in the snapshot (or `None`
  when the parent is filtered out by availability/placement);
- `registry.rs:551 overlay_snapshot_rows(.., order_for: &dyn Fn(&[&str], &[bool]) -> Vec<usize>)`
  and its test helpers `identity_order` (L594) / `reversed` / `listed`.

Rule to add (a third parallel slice `parent: &[Option<usize>]`), applied to unlisted non-headers in
registration order after step 1:
- parent already in `order` ⇒ insert after the LAST placed member of that family (the parent or any
  of its placed children) — siblings keep registration order;
- otherwise, if any of its own children is placed ⇒ insert immediately BEFORE the first placed child;
- otherwise append at the end (today's behaviour).

Unconfigured fast path (L115–118) stays identity — registration order already keeps a parent before
its children. `ordering.rs` is harness-mounted (`validate_custom_options.sh`).

## 3. What the mod registers today vs. after

`src/mods/background_dancers/options.rs:163 register_one(id: &'static str, count, display_name,
description, on_change: fn(u8, i32))` → `RegisterSpec::scalar(id, 0, count, 1,
ScalarFormat::Dynamic(label)).step_coarse(5).default_value(0).persist_mode(Local)
.persist_transform(identity, clamp_load).in_game_only()…`. Every callback is a plain `fn` — no
captures — so per-row data (which catalog slice a row indexes) must be looked up by `option_id` in a
process-wide table (the existing `CATALOG: OnceLock` pattern, `options.rs:37`).

`&'static str` ids for runtime-discovered sources: `Box::leak(String)` once per process, cached in a
`OnceLock` beside the catalog (the `Duplicate` re-enable path reuses them).

`versus_mirror::register(ids: &[&'static str])` (`src/services/versus_mirror.rs:71`) — the stage
source row and every stage model row go in one call.

## 4. Preview driver touch points

`src/mods/background_dancers/preview/mod.rs:232 on_preview_request(side, option_id)` →
`options::kind_for_option(option_id)` (row → `Kind`) and `options::choice_key(kind, side)` (the
side's EFFECTIVE key). Both become row-aware: `kind_for_option` must accept every registered id of
the mod (source rows included); for the identity, a per-row reader `row_choice_key(option_id, side)`
returns the row's own value for model rows and the effective pick for source rows (D9).
`marker_for(kind)` (L399) reads the `OPT_DANCER` / `OPT_STAGE` chrome — unchanged (all rows of a kind
alias that chrome).

## 5. Pick path touch points

- `src/mods/background_dancers/lifecycle.rs:800 option_pick(rng, t, random_stages, sides, arc_exists)`
  reads `options::stage_choice(first side)` / `options::dancer_choice(side)` as `Option<String>`.
  Becomes a per-element `Choice` (key / within-source pool / any) read through new
  `options::stage_request(side)` / `dancer_request(side)`.
- `src/mods/background_dancers/selection.rs:608 resolve_choice(rng, stages, dancers, stage_key:
  Option<&str>, dancer_keys: &[Option<&str>])` — grows a per-element enum; the all-`Any` path must
  draw exactly `pick_stage` then `pick_dancers` (existing test `resolve_choice_rules`, L1283).
- `selection.rs:367 random_stage_pool(stages, keep)` is reused per source: pool = source rows kept by
  the screen filter, `NoneLeft` ⇒ all source rows (D8).
- `src/mods/background_dancers/pick.rs:127 Pick::summary()` renders `{random}`/`{option}`/`{pin}` per
  element via `PickSource::tag()` (`selection.rs:578`); a within-source draw needs its own tag.

## 6. Tables → catalog data flow today

`lifecycle.rs:150–181`: `plan.labels: Vec<(key, label)>` → `Tables.custom_labels` →
`custom_labels_snapshot()` → `catalog::build_catalog_with_custom(&stages, &dancers, &custom)`
(`catalog.rs:151`). The plan must carry the SOURCE per accepted entry; the catalog groups by source
(stock first). `Tables` needs the same grouping for the pick path (which keys belong to source S).

## 7. Harness legs

- `scripts/validate_background_dancers.sh` mounts `selection.rs`, `catalog.rs`, `pick.rs`,
  `custom_content.rs`, `movie_mode.rs` at the crate root (they reach each other via `super::`). New
  pure code (source classification of a listing, slugging, grouped catalog, request resolution) goes
  in those files or a new dependency-free sibling added to `MODULE_NAMES`/`MODULE_PATHS`.
- `scripts/validate_custom_options.sh` mounts `api.rs`, `observers.rs`, `ordering.rs`, `registry.rs`
  with log-macro stubs — covers D10 and D11.

## 8. Label art

`scripts/option_strings.py:260–269` holds `LABELS["background_dancer"/"background_stage"]` (en/ja/ko);
`scripts/gen_option_labels.py` renders `seop_item_<id>.png` into the three
`data_mods/custom_options/select_music_option_lang_{eng,jpn,kor}_v3_ifs/tex/` dirs. Two new entries
(`background_dancer_source`, `background_stage_source`). No `_TEMPLATE` chrome needed for the source
rows (they alias the dancer / stage chrome, D11).
