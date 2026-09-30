# Orientation — where the idea has to live

Findings from reading the code before proposing anything. Paths are repo-relative; line numbers are
as of 2026-09-30.

## 1. "Custom models" is a sub-feature of `background-dancers`

There is no `custom_models` mod. Everything lives in `src/mods/background_dancers/`:

| File | Role today | Touched by this feature |
|---|---|---|
| `custom_scan.rs` (457) | IMPURE: walks `data_mods/custom_models/{dancers,stages}`, packs model folders into `data_mods/_cache/custom_models/<name>-<hash8>.arc`, reads sidecars, feeds the planner, mounts. `walk_kind_root` (L150) is **exactly two levels deep**: `<kind>/<model>` and `<kind>/<Friendly>/<model>`. Non-model subdirectories inside a friendly folder are silently ignored — a third level is invisible today. | Yes — the walk must recognise a SOURCE level. |
| `custom_content.rs` (1574) | PURE planner: `PackDir { dir, folder: Option<String>, arcs, *_rows }` (L263) — no parent/source slot. `plan()` (L430) emits `Plan { stages, camera_rows, dancers, labels: Vec<(key,label)>, mounts, notes, warnings }`. Labels: folder name → `label_from_folder` (≤15 bytes via `fit_label`), flat → key rule. Key collisions with stock or an earlier folder are REFUSED (L457, L605). | Yes — carry the source per entry. |
| `catalog.rs` (455) | `Catalog { dancers, stages }` of `CatalogEntry { key, label }`; value 0 = RANDOM, k ≥ 1 = `entries[k-1]`. `build_catalog_with_custom`: stock block first (byte-sorted by key), custom block after (sorted by label). `clamp_to_catalog` for load. Dependency-free (harness-mounted). | Yes — becomes source-grouped. |
| `options.rs` (265) | The two `custom_options` rows `background_dancer` / `background_stage`: `RegisterSpec::scalar(id, 0, count, 1, ScalarFormat::Dynamic(label))`, coarse step 5, `PersistMode::Local`, `persist_transform(identity, clamp_load)`, `in_game_only()`, no `show_when`. Stage row mirrored across sides (`versus_mirror::register(&[OPT_STAGE])`). Readers: `stage_choice(side)` / `dancer_choice(side)` / `choice_key(kind, side)` / `value(kind, side)` / `kind_for_option(id)`. `CATALOG: OnceLock` — built once per process. | Yes — rewritten around sources. |
| `lifecycle.rs` (1372) | `init_tables()` (L100): stock rlists from `startup.arc` + `custom_scan::discover_and_mount` appended after the stock rows; `Tables { stages, camera_rows, dancers, pin, custom_labels, screen_stages }`. `window_entry()` (L628): per-song pick — screen filter (`movie_mode::random_pool_filter`) → `random_stage_pool` → pin / `option_pick` / `make_pick`. `option_pick()` (L800): first entered side's stage choice + each side's dancer choice; unknown key ⇒ WARN + RANDOM for that element; chosen stage looked up in the WHOLE table (never screen-filtered), RANDOM stage drawn from the filtered pool. | Yes — per-source random pools. |
| `selection.rs` (1342) | Candidates, `Rng`, `pick_stage` (uniform over distinct keys then rows), `random_stage_pool` → `StagePool::{All, Filtered, NoneLeft}`, `pick_dancers` (n uniform draws over ALL dancers), `resolve_choice(rng, stages, dancers, stage_key: Option<&str>, dancer_keys: &[Option<&str>])`. Pure, harness-mounted. | Yes — choices gain a "random within pool" arm. |
| `movie_mode.rs` (784) | `random_pool_filter(mode, song) → ScreenFilter::{WithScreens, WithoutScreens}`; `routes_to_screens(mode)` = STAGE SCREENS only. `ScreenFilter::keeps(has_screens)`. | No (reused). |
| `preview/mod.rs` (552) | `on_preview_request(side, option_id)` → `options::kind_for_option(id)` + `options::choice_key(kind, side)`; RANDOM ⇒ badge; `marker_for(kind)` reads the chrome template of `OPT_DANCER` / `OPT_STAGE`. | Yes — map the new row ids. |
| `style.rs` (480) | `BackgroundDancersConfig` seeding, whole-section `persist_section` (must re-emit every key), GLOBAL SETTINGS rows incl. "Custom Dancers & Stages" (next-launch toggle). | Only if a config key is added. |
| `mod.rs` (286) | `enable()`: `lifecycle::init_tables()` → `options::register(catalog::build_catalog_with_custom(...))`. | Yes. |

Services: `src/services/custom_options/*` (option-row framework), `src/services/versus_mirror.rs`
(`register(ids: &[&'static str])`), `src/services/scene3d/arc_set.rs` (mounts — untouched).

## 2. On-disk reality

`data_mods/custom_models/dancers/` holds **115** friendly folders, all at the 2-level layout, with
the source already spelled into the folder name: `3rdMIX *` (16), `4thMIX *` (18), `5thMIX *` (15),
`Strike *` (45), `UMX *` (2), `UMX2 *` (6), `UMX3 *` (8), and 5 singles (Big Smoke, Carl Johnson,
Hatsune Miku, Kasane Teto, Peter Griffin). `stages/` holds 2 (Griffin House, Grove Street). All of it
is tracked repo content (2489 files; `.gitignore` allow-lists `data_mods/custom_models/**/*.arc`).

Consequences: the source prefix consumes the 15-byte label budget today (`4thMIX Spacef B` is
exactly 15; anything longer is cut with a WARN). A source level lets the friendly names drop the
prefix. Reorganising the tracked folders is a `git mv` the maintainer owns (content, not code).

## 3. Framework facts that shape the design

- **`ShowWhen`** (`api.rs:381`): `Always | Equals { parent_id, value } | NotEquals { parent_id, value }`.
  One parent, exact match, parent registered first. Evaluated per side, live
  (`registry.rs:411 show_when_satisfied`); children re-filter on the same frame as a parent press
  (`rows.rs:2423 update_children_visibility` → `options_scroll::reapply_mask_for_side`). Hidden rows
  are still allocated (`+0xB8 = 0`), just filtered from the scroll driver.
- **Scalar bounds are per OPTION, not per side** (`registry.rs:300 set_scalar_bounds` writes the one
  `UiKind::Scalar { min, max }`). Two players on different sources cannot share one model row whose
  range differs per side.
- **`DynamicLabelFn = fn(option_id, value) -> Option<String>`** (`api.rs:154`) — no side parameter.
  A single model row whose label depends on the side's source cannot be labelled. Two users today
  (`background_dancers/options.rs`, `ddr_selection/options.rs`).
- **`load_transform: fn(id, value)`** — no side either; a load-time clamp cannot consult the side's
  source.
- **Value text is re-pushed every render tick** (`rows.rs:1896 push_scalar_value_text`; the
  `last_value_text` cache field is unused), so a label that depends on another row's value would
  refresh without a value change.
- **Label / preview textures are derived from the id** (`registry.rs:67 label_texture_name` =
  `seop_item_<id>`, `:76 preview_image_base_name` = `seop_image_<id>`); `asset_gen::register_label_for`
  dedups by name (`asset_gen.rs:239`). The atlas is flushed once at boot (`lib.rs`), so a row id needs
  its PNG on disk before the flush. `preview_gen::generate_chrome(id)` already writes generated
  `seop_image_<id>.png` into the language tex dirs at boot (untracked outputs beside tracked
  `_TEMPLATE.png`s).
- **Ids are `&'static str`** (`RegisterSpec.id`); `versus_mirror::register(&[&'static str])`.
- **Row order** (`ordering.rs:104 compute_order`): operator-listed ids first (`option_menu_settings`),
  unlisted appended in registration order. The shipped `mod-config.json` LISTS `background_dancer` and
  `background_stage` (L173/178). Any NEW id (a source row) is unlisted ⇒ appended at the END of the
  menu, far from its listed child — unless the ordering learns family adjacency or the shipped config
  is edited (existing installs keep their config: `option_menu_settings` is operator-owned, never
  rewritten by the DLL).
- **Persistence**: `PersistMode::Local` ⇒ `custom_options.p{1,2}.<id>` in `mod-config.json` only;
  loads pass through `load_transform` then `on_change`.
- **Overlay**: rows are `in_game_only()`; `display_name` / `description` are overlay-only text.

## 4. Two candidate shapes

**A — two rows per kind (Source + Model), one Model row re-bounded per source.**
Needs three framework changes: per-side scalar bounds, a side-aware `DynamicLabelFn` (both
existing users + `format_scalar_value` + overlay), a side-aware `load_transform` (or a post-load
re-clamp). Loses per-source memory (value 3 means a different dancer under every source ⇒ must reset
to RANDOM on every source change). Load order of `source` vs `model` values matters.

**B — one Source row + one Model row PER SOURCE (`ShowWhen::Equals { source, value }`).**
Needs one small framework change: a `texture_id` alias on `RegisterSpec` so every model row of a
kind renders the same `seop_item_background_dancer` label and `seop_image_background_dancer` chrome.
Each row has a fixed catalog slice, so today's `Dynamic` label, `clamp_load` and `choice_key`
machinery works unchanged per id. Per-source selections are remembered for free (each row persists).
Ids are leaked once per process (`Box::leak`, catalog is `OnceLock` already). Cost: N+1 registered
rows per kind, all but one hidden; N ≈ 8 today.

Both need the ordering family-adjacency rule (or config edits) from §3.

## 5. Behaviour to preserve

- Stock row values never move (stock block first, byte-sorted) — B keeps `background_dancer` =
  stock only; old cached values ≥ 27 clamp to RANDOM.
- RANDOM everywhere reproduces the plain random path under the same seed (`resolve_choice` contract).
- An explicitly chosen stage is never screen-filtered; RANDOM stage honours the screen rule, falls
  back to "all" when nothing matches (`StagePool::NoneLeft`, one WARN).
- `custom_content` OFF ⇒ no scan ⇒ today's two rows, stock only.
- Bot phantom side: `option_pick` already reads only entered sides' choices via `entered_side_list`.
