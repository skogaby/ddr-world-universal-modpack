# Idea honing — custom dancer / stage sources

Decision register. Status ∈ Proposed | Accepted | Overridden | Assumed | Open.

Register accepted by the maintainer 2026-09-30 (D3, D14 overridden as recorded below).

Readiness Confirmed 2026-09-30 — design may proceed (no Open decisions; source-row coarse step settled in the design).

| ID | Decision | Why it matters | Recommendation | Status |
|---|---|---|---|---|
| D1 | Row shape | Drives framework changes, persistence, UX | **B**: one SOURCE row per kind + one MODEL row PER SOURCE, children via `ShowWhen::Equals` | Accepted |
| D2 | What makes a folder a SOURCE | Folder-layout contract; backwards compat | A non-model dir directly under `dancers/`/`stages/` that holds ≥1 friendly folder. Models directly inside a source (no friendly wrapper) accepted with key-rule labels | Accepted |
| D3 | Where legacy placements go | 115 folders + every existing user install use `<kind>/<Friendly>/` | Implicit source **CUSTOM** (a real folder named `Custom` merges into it) | Overridden |
| D4 | Source row values | Persisted ints; UX | 0 = RANDOM, 1 = STOCK, 2… = sources sorted by label; row registered only when ≥1 custom source of that kind exists, else today's single stock row | Accepted |
| D5 | Model row ids | Persistence keys; texture names | `background_dancer` = STOCK pool (unchanged id); `background_dancer_<slug>` per source, slug from the folder name; same-slug folders MERGE (design-time refinement, see below); reserved slug `source` / unprintable name ⇒ WARN + folder skipped | Accepted |
| D6 | Visibility | The "hide when RANDOM" requirement | Model rows `Equals { source_row, v }`; source RANDOM ⇒ no model row visible | Accepted |
| D7 | Random semantics | Core behaviour | Source RANDOM ⇒ uniform over everything (today's draws, seed-identical). Source S + model RANDOM ⇒ uniform within S (STOCK is a source) | Accepted |
| D8 | Stage screen rule within a source | The movie/monitor requirement | Pool = S ∩ screen filter; empty ⇒ all of S, one WARN naming S | Accepted |
| D9 | Preview when a SOURCE row is focused | UX | Show the side's effective choice: the model row for that source if non-RANDOM, else the RANDOM badge | Accepted |
| D10 | Menu ordering of the new rows | Shipped + every existing config LISTS `background_dancer`/`background_stage`; unlisted ids append at the END | Framework: unlisted rows in a `ShowWhen` family sit adjacent to the family's listed anchor; also list the two source ids in the shipped `mod-config.json` | Accepted |
| D11 | Shared textures | Every model row must render the "BACKGROUND DANCER" label + the dancer chrome | Framework: `RegisterSpec` label / preview texture aliases (`seop_item_<alias>`, `seop_image_<alias>`) | Accepted |
| D12 | Persistence + mirroring | Cabinet behaviour | All rows `Local`, per-row `clamp_load`; source rows default RANDOM; stage source + every stage model row versus-mirrored | Accepted |
| D13 | Existing cached values | One-time UX blip | `background_dancer` values > 26 (old custom entries) clamp to RANDOM at load (existing clamp) — accepted loss | Accepted |
| D14 | Repo content reorganisation | 2489 tracked files; cache arcs re-pack once (source path is in the cache name/hash) | Maintainer moves the content himself once the code lands — NOT part of the plan | Overridden |
| D15 | Source labels | 15-byte SSO budget | `label_from_folder` (upper, `_`→space, ≤15) — `DDR STRIKE` fits; cut ⇒ WARN as today | Assumed |
| D16 | New row label art | Blank labels otherwise | `DANCER SOURCE` / `STAGE SOURCE` via `scripts/option_strings.py` → `gen_option_labels.py` (en/ja/ko) | Assumed |
| D17 | No new config key | `custom_content` (next launch) already gates the scan | Unchanged | Assumed |
| D18 | Pure logic stays harness-testable | Repo rule | Source classification, slugging, grouped catalog, pool resolution in `crate::`-free files; `validate_background_dancers.sh` extended | Assumed |
| D19 | Docs | Layout contract lives in three places | Update `custom_content.rs` `//!`, `docs/background_dancers_research.md` §6, README | Assumed |

## D1 — Row shape

**Question.** Two rows per kind with one re-bounded model row (A), or one model row per source (B)?

**Recommendation: B.** The framework stores scalar bounds per OPTION (not per side), and both
`DynamicLabelFn` and `load_transform` are `fn(id, value)` with no side. A therefore needs three
framework signature changes (per-side bounds, side-aware labels touching `ddr_selection` too,
side-aware load clamp), and still has to reset the model row to RANDOM on every source change because
value 3 means a different dancer under every source. B needs one small framework addition (texture
aliases, D11), reuses today's per-id label/clamp/choice machinery unchanged, and remembers the last
pick per source for free. Cost: N+1 registered rows per kind (N ≈ 8 today), all but one hidden by
`ShowWhen`; hidden rows are allocated but filtered — negligible.

Rejected: encoding source×model into one value with a stride (breaks the stepper/marker; still
per-side bounds).

## D2 — Source classification

A directory directly under `dancers/` (or `stages/`) is:
- a **model folder** if its name classifies as `pl_*` / `mapset_*` (unchanged);
- a **source folder** if it is not a model folder AND contains ≥1 *friendly folder* — a non-model
  subdirectory that itself contains ≥1 model folder or body/stage `.arc`;
- otherwise a **friendly folder** (today's 2-level layout) → implicit source (D3).

Inside a source folder: friendly folders → label from the friendly name; model folders / `.arc`s
directly inside → key-rule label (mirrors today's root behaviour); sidecars at either level apply to
their own directory (unchanged rule: rows for keys not in the same directory are ignored). A stray
non-model subdirectory with no models (an author's `textures_src/`) does not promote its parent to a
source. Deterministic from the listing alone.

## D3 — Implicit source

Legacy placements need a home or the feature breaks every existing install and the README contract.
Maintainer (2026-09-30): label it **CUSTOM** — future non-DDR entries will live under a `Custom/`
source folder, so the implicit bucket and that folder are the same pool (same slug `custom` ⇒ they
merge), which keeps already-released dancers backwards compatible. The implicit source is omitted when
nothing lands in it. Model row id: `background_dancer_custom` / `background_stage_custom`.

## D4 / D5 — Values and ids

Source row: `0 RANDOM`, `1 STOCK`, `2..=N+1` custom sources sorted by label (byte order of the
upper-cased label). Model rows: `0 RANDOM`, `1..=count` entries of that source, stock byte-sorted by
key (today's block, unchanged values), custom sorted by label then key (today's rule).

Design-time refinement (2026-09-30): two folders with the same slug also derive the same 15-byte
label, so two rows would be indistinguishable — they MERGE into one source (the rule D3 already
needs for `Custom/`), with one INFO naming both spellings. Only the reserved slug `source` (it would
collide with the source row id) and an unprintable name are refused (WARN + content skipped).

Ids are persistence keys and must be stable across folder additions, so they derive from the folder
name, not the sort position: `background_dancer_<slug>`, slug = ASCII-lowercased folder name,
`[^a-z0-9]` runs → `_`, trimmed, capped (kbin-valid). `background_dancer` keeps meaning "the stock
pool" so today's stock values survive. `CUSTOM` ⇒ `background_dancer_custom`.

## D7 / D8 — Random

`resolve_choice` today takes `Option<&str>` per element. It grows a per-element choice:
`Key(k)` | `Within(pool)` | `Any`. With every element `Any` the draw sequence is byte-identical to
today (seed reproducibility, existing tests keep passing). For a stage `Within(S)`: rows of S kept by
the screen filter; none ⇒ all rows of S (mirrors `StagePool::NoneLeft`) with one WARN naming S.
A chosen stage key is never filtered (today's rule).

## D9 — Preview on the source row

`on_preview_request(side, id)` receives the focused row. For a source row, the effective identity is
"what will play": the side's model row for the selected source (non-RANDOM ⇒ that model; RANDOM or
source RANDOM ⇒ badge). Model rows preview their own value exactly as today.

## D10 — Ordering

`compute_order` appends unlisted ids after the listed ones. Every existing `mod-config.json` lists
`background_dancer` / `background_stage`, so the (new, unlisted) source rows would land at the bottom
of the menu, separated from the model rows they control. Rule: an unlisted option that is the
`ShowWhen` parent of a listed option is placed immediately before its first listed child; unlisted
children of a placed parent follow it in registration order. Pure permutation logic with tests. The
shipped config also gains the two source ids so fresh installs need no rule.

## D11 — Texture aliases

`RegisteredOption::label_texture_name()` / `preview_image_base_name()` are `format!("…_{id}")`.
Add `RegisterSpec::label_texture_like(id)` and `preview_texture_like(id)` (independent: a source row
keeps its own `seop_item_background_dancer_source` label but borrows the dancer chrome so the 3D
preview lands in the same marker box). `asset_gen` already dedups names.

## D14 — Content move

Not code and not in the plan: the maintainer moves the tracked content once the code lands. For the
record: moving a model folder changes its cache-arc name (`<name>-<fnv1a32(source path)>.arc`) and
fingerprint ⇒ every moved folder re-packs once at the next boot (115 folders, one-time). Dropping the
`3rdMIX ` prefix from friendly names is optional and frees label bytes.
