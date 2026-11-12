# Idea honing — decision register

Status legend: `Proposed` (awaiting user), `Accepted`, `Overridden`, `Assumed` (settled by the
agent, auditable), `Open`. ★ = likely not yet considered. Evidence: `research/orientation.md`,
`research/builder-and-tabs.md`, `research/predicate-and-persistence.md`,
`research/label-pipeline.md`, `research/label-prototype.md`.

| ID | Decision | Recommendation | Status |
|---|---|---|---|
| D1 ★ | Enhanced-mode architecture | Detour the VERSION builder and the group-tab press body; keep the stock predicate with a per-row end at `+0x34` (one disp32 patch); legacy path untouched. Research: feasible, instruction-identical on all five builds | Accepted |
| D2 | Config schema | `num_columns` + `filters[]` (`label`, `series_start`, `series_end`?, `texture`, `group`?) | Overridden (key `filters`) |
| D3 ★ | Precedence + parse safety | Enhanced key present ⇒ enhanced mode, `custom_series` ignored; `custom_series` optional; enhanced block validated by the mod, never fails the global parse | Accepted |
| D4 ★ | Series values | Raw musicdb values, inclusive; 16 is treated as 15 (both DDR 2014, as the game's mapper does) — bounds of 16 are normalised to 15 | Accepted |
| D5 ★ | Group-tab membership | Optional per-row `group`; default = group whose span contains `series_start` (CLASSIC 1–13, WHITE 14–17, GOLD 18–21) | Accepted |
| D6 ★ | Selection index + persistence | Index = position in `filters` = saved bit; hard cap 64 (u64 field; SHL/ROL alias above 63) | Accepted |
| D7 ★ | Shipping / visibility | Enhanced block never in committed `mod-config.json` (updater would add it for everyone); `--emit-config` prints it; README / menu / WebUI untouched; internal docs yes | Accepted |
| D8 ★ | Names for custom series ≥ 22 | `label` of a row covering exactly that one value; else WORLD fallback | Accepted |
| D9 ★ | Label texture pipeline | **Revised by research:** sources in `series_labels/`, normalised to W×20 and pre-converted into per-image `_cache` blobs (this IFS serves textures per image); cached texturelist batch, no reboot splash | Accepted |
| D10 ★ | Thumbnail loop bound | **Revised by research:** highest N in 22–127 whose `jacket_thumbnails_<rgn>_<N>.arc` exists, else stock; never from row ranges | Accepted |
| D11 | Generator script scope | Canonical table + `--from-config` + `--emit-config` + `--preview` | Accepted |
| D12 | Label rendering | FOT-TsukuGo Pro B (user-supplied, `scripts/fonts/FOT-TSUKUGOPRO-B.OTF`) at 15 px, base scale 0.90, no faux-bold; prototype fit rules | Overridden |
| D13 | Canonical rows + order | 20 rows (2014 = 15–16), newest first | Accepted |
| D14 | `num_columns` handling | Integer 1–5 = template index; else WARN, use 2 | Assumed |
| D15 | Row validation | start > end, value > 255, unknown `group`, `texture` outside `[a-z0-9_]`, non-ASCII `label` ⇒ skip row (one WARN each) | Assumed |
| D16 | `FilterHeader` | Omit (explained below; confirmed by research) | Accepted |
| D17 | Empty group | Tab still shown; press clears the VERSION selection | Assumed |
| D18 | Scroll integration | `series_filter_scroll`: builder registers entry buttons by pointer (no template test), layer ids refreshed on every CreateVisual, `columns = num_columns`, 9 visible rows | Assumed |
| D19 | Texture names | Real names: texture `sefi_version_<texture>_<N>col` (= PNG stem); label key written with the game's `std::string::assign` | Overridden |
| D20 | Chip text | Stock `"DDR "` + runs of `label`s merged by adjacent index (`DDR WORLD～A20`); adjacency follows config order, not ranges | Assumed |
| D21 | Build coverage | New sites AOB/derived, green in `validate_signatures.sh` + `shape_diff.py`; any miss ⇒ enhanced mode off (one WARN), legacy fallback if configured | Assumed |
| D22 | Plan ordering | Plan Step 1 = `gen_series_labels.py` + `--preview`; throwaway prototype now | Accepted |
| D23 | `label` length | No limit in enhanced mode (mod-owned table strings; game only copies them) | Assumed |
| D24 ★ | Grid-row limit | **New (research):** buttons laid out below the on-screen band never get a visual; cap cells at `24 × num_columns` (and 64), limit verified on the first cabinet step | Accepted |
| D25 ★ | Legacy thumbnail hang | **New (research):** clamp the legacy bound to 127 too (a `series_value` ≥ 128 hangs boot today) | Accepted |

## D1 — Enhanced-mode architecture

- **Builder detour** (`version_filter_builder` AOB, unique on all builds). Enhanced: three tabs via
  the stock tab factory (builder+0x55), `SetTemplate(3)`, stock labels; one entry per cell via the
  factory `std::function` it receives (selection index = cell index), `SetTemplate(num_columns)`,
  label via the game's `assign(const char*, n)`; destroy the factory exactly once, as stock does.
  Not enhanced: call the original.
- **Predicate:** stock code. Table LEA → enhanced table (existing patch) and second compare
  `+0xB8 → +0x34` (identical bytes at `match+0x34` on all builds; `+0x34` is never read or written
  elsewhere). Rows store `start = s`, `end = e + 1` (signed compares).
- **Group-tab press detour** (`version_group_press` AOB): clear category, set each member of tab
  `g` (set-one from the lambda93 AOB), tail to notify. Keyed on `g` only (the tab factory hardcodes
  the stock group table).
- Table holds `max(N, 9) + 1` rows: N cells, a sentinel (the chip loop runs `0..=count`), inert
  padding (`start = i32::MAX`). Never freed.
- Shared with legacy: mapper default patch, flare exclusion, count detour (N), chip builder (N),
  per-song name table, AFP scroll children, scroll service.
- Rejected: extending the stock loop (forces reverse index order and ≥ 1 entry); a full predicate
  detour (needs the build-dependent FilterManager offset); group-contiguous indices + group-table
  patch (ties saved bits to group order).

## D3 — Precedence and parse safety

`custom_series_enhanced` present and an object ⇒ enhanced mode; `custom_series` ignored entirely.
`filters: []` ⇒ tabs only, mod still active. `SeriesConfig.custom_series` gets `#[serde(default)]`;
the enhanced block deserialises as `serde_json::Value` and the mod validates it, so nothing in it
can fail the global config parse (today any type error there drops every mod to defaults). Not an
object ⇒ one WARN, legacy if `custom_series` is non-empty, else mod not registered.

## D4 — Series values

Raw musicdb `<series>` values, inclusive, 0–255; 1stMIX = 1, 0 = no series. The predicate sees
the game's mapped value, where raw 16 is folded into 15 (the game names both "DDR 2014"). Bounds of
16 are normalised to 15, so a cell covering either covers both. Rejected: a 4-byte jump-table patch
un-merging 16 (also splits the VERSION sort's group headers into two "2014" groups).

## D5 — Group-tab membership

Tabs always present (stock art, template 3). A press replaces the VERSION selection with the tab's
members: explicit `group`, else the group whose span contains `series_start`; 0 or ≥ 22 ⇒ none.
Rejected: containment (spanning cells join no group) and overlap (a cell joins several groups).

## D6 — Selection index and persistence

Cell index = saved bit in `filtersort/version`. Hard cap 64 (load `SHL` and save `ROL` alias
beyond 63). Appending cells keeps saved filters; inserting, reordering or switching modes remaps
them. No migration.

## D7 — Shipping and visibility

The committed `mod-config.json` never carries the enhanced block. `gen_series_labels.py
--emit-config` prints the canonical block for pasting into a tester's cabinet config. Label PNGs
ship in `data_mods/custom_series/series_labels/` (≈ 92 KB), inert unless referenced. README, mod
menu, WebUI: nothing. Module `//!` docs, a `docs/` note and `.agents/` describe it as experimental.

## D8 — Names for custom series

256-entry "Version / %s" table: stock 0–21; each raw value ≥ 22 takes the `label` of the first cell
whose range is exactly that value; the rest keep the WORLD fallback.

## D9 — Label texture pipeline (revised)

Research: `select_music_option_v3.ifs` serves textures **per image name**; the cloned atlas blob is
never opened, and the legacy mode works only because its PNGs sit at the per-image serving path
(`custom_series/select_music_option_v3_ifs/tex/`). Sources only in `series_labels/` would render
blank.

Recommendation, at `enable()`:
1. Per unique `texture`: source `series_labels/sefi_version_<texture>_<N>col.png`, fallback
   `series_labels/sefi_version_<texture>.png`, else blank + WARN. Normalise to exactly W×20
   (crop/pad top-left; WARN on mismatch — the server rejects oversized images).
2. Write each per-image blob straight into `_cache/select_music_option_v3_ifs/` with a small
   LayeredFS helper that reuses `cache_texture`'s encoding (nothing staged under an `_ifs` folder
   ⇒ no auto-inject, no stale PNGs).
3. Texturelist entries via `generate_cloned_atlases_cached` (fresh mode, prefix `cser_enh`);
   after a cache hit verify every stem is in the merged xml (mode-switch staleness), write an empty
   merged texturelist when no labels resolve, rescan only on a miss.
4. No "REBOOT" splash for this batch: labels mount at the CAUTION preload, after `enable()`, so a
   rebuild is picked up the same boot (the latch would be a false alarm on every `num_columns`
   change).
Declare only the active width (each label ≈ 9 ms of CAUTION load on Wine).
Alternative (rejected): stage PNGs into the `_ifs/tex` folder each boot (s_marvelous pattern) —
needs stale-copy cleanup and cache purges. Fresh vs donor atlas mode needs one cabinet check;
donor mode is the fallback.

## D10 — Thumbnail loop bound (revised)

Research: `CMP RSI,imm8` sign-extends before an unsigned `JBE`, so any bound ≥ 128 never ends and
exhausts the resource pool (the real cause of the old 255 crash); missing arcs are harmless; only
`jacket_thumbnails_ja_0..21.arc` ship. Bound = highest N in 22–127 for which
`jacket_thumbnails_ja_<N>.arc` or `_ua_<N>.arc` exists (install `data/` or a LayeredFS mod
folder), else leave `0x15`. Never derived from cell ranges.

## D11 — Generator script

`scripts/gen_series_labels.py`: canonical table (key, text, raw range, per-width overrides);
renders all five widths into `data_mods/custom_series/series_labels/`; `--from-config PATH` renders
every cell of a config's enhanced block from its `label`; `--emit-config` prints the canonical
block; `--preview` writes the contact sheet (stock tab art used when the local extraction is
present, placeholders otherwise).

## D12 — Label rendering (override)

**Override (user, 2026-09-27):** font FOT-TsukuGo Pro B (`scripts/fonts/FOT-TSUKUGOPRO-B.OTF`,
copied from the maintainer's file) instead of Inclusive Sans — closer to the stock face, plain
zeros. Metrics tuned against stock: 15 px (12-px caps), baseline y = 16, base horizontal scale 0.90,
no faux-bold (tried 0.375 px; dropped by the user 2026-09-27), fill `#00B68C`, canvases 220/104/64/44/32 × 20,
left-aligned. Fit to stock ink limits (216/98/60/42/30). Condense to 0.70, then a two-line stack
(10.5 px, second line right-aligned) only at a space or case/digit break; single words condense.
Per-width overrides in the table. Final prototype: `prototypes/out/contact_sheet.png`.

## D13 — Canonical rows

WORLD(21), A3(20), A20 PLUS(19), A20(18), A(17), 2014(15–16), 2013(14), X3 VS 2ndMIX(13), X2(12),
X(11), SuperNOVA2(10), SuperNOVA(9), EXTREME(8), MAX2(7), MAX(6), 5thMIX(5), 4thMIX(4), 3rdMIX(3),
2ndMIX(2), 1stMIX(1). Newest first (stock convention; newest series visible without scrolling).

## D16 — `FilterHeader`

An invisible, non-focusable, full-width 1-px component between the tabs and the entries. It
forces a line break, but here the break happens anyway: three 72-px tabs fill the 216-px line, so
the next cell of any width wraps. Its only effect is moving the cells down 1 px. Recreating it needs
five more derived sites (operator new, Component ctor, two vtables, vector push). Omit; revisit if
the 1-px shift is noticeable.

## D24 — Grid-row limit (new)

`FilterButton::CreateVisual` runs lazily, only for buttons whose **layout** rect is inside the
virtual screen band (y < 864 in 1280×720). The scroll service moves BM2D layers, not layout, so a
cell laid out below the band never gets a visual, even when scrolled to. Estimate ≈ 24 grid rows
below the tab row. Cap cells at `min(64, 24 × num_columns)` (extras dropped, one WARN) — affects
only 1 column (> 24 cells) and 2 columns (> 48); measure the real limit on the first cabinet
deploy. Rejected for now: Component-level scrolling (GridPanel scroll offset) — more RE, and it
would also scroll the tabs.

## D25 — Legacy thumbnail hang (new)

The legacy mode writes the highest `series_value` into the imm8 unchecked; ≥ 128 hangs boot.
Clamp it to 127 (one WARN). Otherwise legacy behaviour is unchanged.

## Acceptance

All decisions accepted by the user on 2026-09-27 (D2, D12, D19 overridden as recorded; D22
accepted explicitly; the rest accepted as recommended, including the research-revised D9, D10 and
the new D24, D25). No decision is `Proposed` or `Open`.

Readiness Confirmed 2026-09-27
