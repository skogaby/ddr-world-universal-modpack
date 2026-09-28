# Config-defined VERSION (series) filter layout — Implementation Plan

Status: Approved 2026-09-27

Design: `.agents/planning/2026-09-27-series-filter-layout/design/detailed-design.md` (approved
2026-09-27). Section (§) and requirement (R) references point there; this plan does not restate
them.

**Every step's gate:**

- `cargo check --target x86_64-pc-windows-msvc` clean (Rust steps);
- `cargo fmt` (whole crate);
- `./build.sh` clean;
- `scripts/validate_series_expansion.sh` green (from Step 2 on);
- when `src/core/signatures.rs` or a `match + N` reader changes: `./scripts/validate_signatures.sh
  <supported-builds folder>` green and a `scripts/sig_harness/shape_diff.py` review;
- a cabinet deploy (`./scripts/deploy.sh`, plus `data_mods/custom_series/` when art changed) with
  the step's Demo observed in spice2x `log.txt`;
- `progress.md` (this feature directory) updated.

The maintainer commits. The committed `mod-config.json` never gains `custom_series_enhanced`
(R28); enhanced configs live only in the cabinet's config.

## Checklist

- [x] Step 1: Label generator, canonical label art and the preview contact sheet — done 2026-09-27 (uncommitted)
- [x] Step 2: Config schema, module split and the pure enhanced model — done 2026-09-27 (uncommitted; cabinet log check folded into Step 4)
- [x] Step 3: Enhanced sites — signatures, derivations and shape checks — done 2026-09-27 (uncommitted; sweep ALL GREEN)
- [x] Step 4: Enhanced menu core — table, predicate, builder and group-press detours — cabinet-proven 2026-09-27 (all five layouts; probe 20/20; remaining demo checks carried into the Step 8 matrix; uncommitted)
- [x] Step 5: Per-width label pipeline — cabinet-proven 2026-09-27 (uncommitted)
- [x] Step 6: Scrolling for registered cells — cabinet-proven 2026-09-27 (uncommitted)
- [x] Step 7: Thumbnail bound policy and the legacy clamp — stock-bound path cabinet-proven 2026-09-27; custom-arc and legacy-clamp checks carried into the Step 8 matrix (uncommitted)
- [ ] Step 8: Documentation, release integration and the cabinet matrix

---

Step 1: Label generator, canonical label art and the preview contact sheet

- **Objective.** Ship the label art and the tool first (the maintainer wants to visualise layouts
  up front): R25–R27, §4.9.
- **Guidance.**
  - Create `progress.md` in this feature directory (AGENTS.md "PDD feature progress tracking").
  - `scripts/gen_series_labels.py`, written fresh (the planning prototype is reference only; carry
    over its measured constants, not its code): canonical table (R26) with per-width overrides,
    rendering per R27, default run into `data_mods/custom_series/series_labels/`,
    `--from-config`, `--emit-config`, `--preview` (default `target/series_labels_preview.png`).
  - Uses `scripts/fonts/FOT-TSUKUGOPRO-B.OTF` (already copied); clear error when missing. Whether
    the font file is committed is the maintainer's call (commercial font) — the script must not
    depend on it being tracked.
  - Stock tab / check-mark art for the preview comes from the untracked local extraction when
    present, placeholders otherwise; never stage that extraction.
  - Paths printed repo-relative.
- **Tests.** `scripts/test_gen_series_labels.py` (stdlib `unittest`, like
  `scripts/test_analyze_audio_sync.py`): every output has its exact canvas size; ink stays within
  the width's limit; the fitting rule never splits a single word; deterministic re-render
  (byte-identical); `--emit-config` output is valid JSON with 20 cells, newest first, 2014 = 15–16.
- **Integration.** Standalone; no DLL change. The 100 PNGs are inert until Step 5 consumes them.
- **Demo.** `python3 scripts/gen_series_labels.py && python3 scripts/gen_series_labels.py
  --preview` writes 100 labels and a contact sheet matching the approved prototype;
  `--emit-config` prints the block the maintainer will paste into the cabinet config in Step 4.

Step 2: Config schema, module split and the pure enhanced model

- **Objective.** Parse and validate the enhanced block safely (R1–R8, R11 normalisation, R14 names,
  §4.2, §4.3, §5.1) with no game-side change yet.
- **Guidance.**
  - Move `src/mods/series_expansion.rs` to `src/mods/series_expansion/mod.rs` verbatim first
    (pure move), then add `enhanced/{mod.rs, model.rs}` stubs; `hooks.rs` and `labels.rs` arrive
    in Steps 4 and 5.
  - `SeriesConfig` (§4.2): `custom_series` `#[serde(default)]`, `custom_series_enhanced:
    Option<serde_json::Value>`.
  - `model.rs` as §4.3, `crate::`-free. `table_rows`, `members`, `custom_names`, `label_key`,
    `label_stems`, `canvas_width` all land here now.
  - `init` mode selection: enhanced key is an object ⇒ `parse`, log every warning and a one-line
    plan summary (columns, cell count, per-group members), then — enhanced engine not yet
    present — fall back per R20 (legacy if `custom_series` non-empty, else unregistered) with one
    INFO saying so. Not an object ⇒ R4.
  - New `scripts/validate_series_expansion.sh` (pattern: `scripts/validate_ddr_selection.sh`, with a
    `serde_json` dependency like `scripts/validate_background_dancers.sh`).
- **Tests.** Harness cases per §7 "Host harness" for everything in `model.rs` except the thumbnail
  helpers (Step 7). Fixture: the Step 1 `--emit-config` output parses with zero warnings.
- **Integration.** Legacy path untouched; a config without the enhanced key boots exactly as before.
- **Demo.** Harness green. Cabinet with the canonical block pasted in: `log.txt` shows the parsed
  plan (20 cells, groups GOLD 4 / WHITE 3 / CLASSIC 13) and the fallback INFO; the VERSION menu is
  the legacy/stock one. A deliberately broken cell logs one WARN and the rest of `mod-config.json`
  still loads (other mods keep their settings).

Step 3: Enhanced sites — signatures, derivations and shape checks

- **Objective.** Resolve every enhanced address on all five builds, fail-closed (§4.4, R20,
  Appendix A).
- **Guidance.**
  - Add `version_filter_builder`, `version_group_press`, `filter_toggle_one_body`,
    `version_predicate_range` to `src/core/signatures.rs` (patterns in Appendix A).
  - `enhanced/mod.rs::resolve_sites(ctx) -> Option<EnhancedSites>`: derive tab factory,
    SetTemplate (cross-check `filter_button_panel_config`), `assign`, clear-category, notify,
    set-one (cross-check notify), predicate range site (cross-check the first
    `version_predicate_lea` match, mapper CALL anchor, 58-byte shape with the LEA disp32 masked),
    builder shape checks, `ui_entry_loop` = builder + 0x249. Use the `core/scanner.rs` decode
    primitives; check each opcode byte before decoding.
  - Each failure logs the site name; `resolve_sites` returns `None`; `init` falls back per R20.
    Still no patch or detour.
- **Tests.** `validate_signatures.sh` green with the new names (unique on every build);
  `shape_diff.py` identical on every build for builder `+0x55/+0x65/+0x8B/+0x1D0/+0x260/+0x278/
  +0x27F`, press `+0x19/+0xD9`, lambda93 `+0x18/+0x26` and the predicate window.
- **Integration.** Called from the Step 2 enhanced branch; on success it logs every resolved
  address and still falls back (engine lands in Step 4).
- **Demo.** Cabinet boot with the enhanced block: one INFO line per resolved enhanced site with
  file-relative addresses matching the design's 20260915 reference list; the fallback INFO; stock
  menu behaviour.

Step 4: Enhanced menu core — table, predicate, builder and group-press detours

- **Objective.** The config-driven menu works end to end with the riskiest pieces first: builder
  and press detours, the `+0x34` predicate patch, count, chip and names (R9–R14, R22, R23,
  §4.5, §4.6, §5.2, §6).
- **Guidance.**
  - `enhanced/mod.rs`: build the near-allocated table (§5.2; long labels point at leaked mod
    memory), the 256-entry name table with R14 labels, and the flare tables (shared code, factored
    out of the legacy path without behaviour change).
  - `enhanced/hooks.rs`: builder and group-press detours exactly per §4.5 (catch_unwind; factory
    destroyed exactly once, outside the fallible section; tabs `g = 2,1,0`; labels via the game's
    `assign`).
  - `enable`/`disable` ordering per §4.6: detours first (pass-through), then byte patches
    (mapper default, predicate LEA + disp32 together, chip LEA + count, name LEA, flare), then the
    count detour, then `ENHANCED_ACTIVE`. Remove the Step 2/3 fallback INFO.
  - Scroll service is not yet configured for enhanced mode (Step 6); labels are not yet declared
    (Step 5), so cells render without label art.
  - **Row-limit probe (design assumption, R8):** one INFO, one frame after the menu opens (a
    `run_on_render_thread` follow-up scheduled from the builder detour), listing how many cells have
    a non-null `FilterButton+0x178` and the highest such index. Keep it as a one-shot diagnostic.
- **Tests.** Harness additions for anything newly pure (e.g. table-row values for long labels,
  sentinel/padding count for N = 0, 1, 9, 20, 64). Engine code: cabinet only.
- **Integration.** Replaces the Step 2/3 fallback. Legacy mode and stock behaviour unchanged when
  the key is absent; the mod toggled off in the overlay menu restores the stock menu.
- **Demo.** Canonical block, `num_columns` 3 then 1:
  - three GROUP tabs, then 20 cells in config order, 3 (resp. 1) per row (labels blank);
  - selecting a cell filters correctly — Filtering Results match the cabinet musicdb's per-series
    counts (stock 20260915: WORLD 289, 2014 107, 1stMIX 3); multi-
    select ORs; the chip reads `DDR WORLD～A20` for the first four;
  - GROUP GOLD / WHITE / CLASSIC select 4 / 3 / 13 cells; a `group` override moves a cell;
  - selection survives a credit;
  - `filters: []` shows tabs only;
  - `log.txt`: the row-limit probe at `num_columns` 1 (record the numbers in `progress.md`; if fewer
    than 20 cells got visuals, stop and revisit D24 before Step 6).

Step 5: Per-width label pipeline

- **Objective.** Cells show their art at every column count, with no reboot splash (R16–R19,
  §4.7, §5.3).
- **Guidance.**
  - `ifs_textures::prebuild_texture` (§4.7 step 2): a thin public wrapper that builds the
    descriptor and calls the existing `cache_texture` — no encoder change.
  - `atlas_cloner::generate_cloned_atlases_cached_with` + `BatchOptions { sidecar_key,
    latch_reboot }`; the existing function delegates with `("atlasbatch", true)` so current callers
    are byte-for-byte unchanged.
  - `enhanced/labels.rs::prepare`: resolve (R16), normalise (R17) into
    `data_mods/_cache/custom_series_labels/`, prebuild blobs, cached texturelist batch (fresh mode,
    prefix `cser_enh`), the stale-cache and empty-set guards, conditional rescan. Called first in
    enhanced `enable`.
  - If fresh-mode entries don't bind on the cabinet, switch the set to `fresh: false` (donor mode)
    and record it in `progress.md`.
- **Tests.** Harness: `label_stems` / source-candidate ordering and `canvas_width` (already in
  model) extended with the donor-per-width table if it lives there. LayeredFS additions: cabinet
  (existing custom-options / s_marvelous / legacy series labels must still render — regression).
- **Integration.** Consumes Step 1's art and Step 4's label keys.
- **Demo.**
  - Each `num_columns` 1–5 shows the matching art (compare against the Step 1 preview); a
    missing texture logs one WARN and leaves that cell blank; a mis-sized PNG logs one WARN and
    renders cropped;
  - changing `num_columns` between boots: correct art the next boot, no red "REBOOT" splash;
  - legacy → enhanced → legacy → enhanced across four boots: correct labels every time (stale-
    cache guard);
  - legacy custom labels (WORLD RUBY / SAPPHIRE) and custom-options labels unchanged.

Step 6: Scrolling for registered cells

- **Objective.** Layouts taller than nine grid rows scroll; the tabs stay put (R15, §4.8).
- **Guidance.** `ScrollConfig.tracking` (`Template2` for legacy, `Registered` for enhanced);
  `begin_build` / `register_entry` called from the builder detour; the CreateVisual detour
  records/refreshes layer ids for registered pointers and activates when all have one. Enhanced
  `enable` calls `configure` per §4.6. Remove Step 4's row-limit probe once D24's constant is
  confirmed (keep one INFO on activation).
- **Tests.** No new pure logic expected; if row/scroll math is extracted, harness it.
- **Integration.** Legacy scroll path unchanged (`Template2`).
- **Demo.** `num_columns` 1 (20 grid rows) and 2 (10): cursor down past row 9 scrolls, cells
  outside the window are masked, tabs never move; closing and reopening the menu repeatedly, and
  switching categories, never crashes (dtor path); legacy mode with custom series still scrolls.

Step 7: Thumbnail bound policy and the legacy clamp

- **Objective.** R21 and R24.
- **Guidance.** `model::thumbnail_bound` / `legacy_thumbnail_bound` (pure); enhanced `enable`
  probes `data/arc/thumbnail/jacket_thumbnails_{ja,ua}_<N>.arc` in the install and via LayeredFS
  (`find_first_modfile`) for N = 22..=127 and patches the imm8 only when a bound exists; legacy
  clamps to 127 with one WARN.
- **Tests.** Harness: no arcs ⇒ `None`; 22 only; gaps (22, 30) ⇒ 30; 127 cap; legacy clamp
  (21, 30, 127, 200 ⇒ 127).
- **Integration.** Shares the existing thumbnail patch site and `SavedPatch` restore.
- **Demo.** No custom arcs: log says the loop is left stock. A copied test arc
  `jacket_thumbnails_ja_30.arc` in a mod folder: log shows bound 30 and a series-30 test song shows
  its jacket in the wheel. Legacy config with `series_value` 200: boot completes, one WARN.

Step 8: Documentation, release integration and the cabinet matrix

- **Objective.** Finish the feature: internal docs, release hygiene, full regression (R28, R29,
  §7).
- **Guidance.**
  - `src/mods/series_expansion/mod.rs` `//!` header: an "Enhanced mode (experimental,
    undocumented)" section — mechanism summary, hook ownership (builder body, group-press body),
    file ownership (§5.3).
  - `docs/filter_menu_system_research.md`: add the corrections found in research (selection sets
    are `map<int, hash_set<int>>`; per-image texture serving in this IFS; lazy on-screen
    CreateVisual; chip loop `0..=count`; thumbnail `CMP RSI,imm8` sign extension) and the enhanced
    mode as a consumer.
  - `.agents/learnings/learnings.md`: the per-image IFS serving trap and the lazy CreateVisual trap.
  - Confirm README, mod menu and WebUI untouched; `mod-config.json` has no enhanced key; `git grep`
    for absolute paths clean; the untracked stock extraction not staged.
  - Readiness gate (AGENTS.md) and `./scripts/build_release_archive.sh` includes
    `data_mods/custom_series/series_labels/`.
- **Tests.** The full §7 cabinet list, items 1–9, on the cabinet build; `validate_signatures.sh`
  once more across all builds.
- **Integration.** Nothing new; this step closes the feature.
- **Demo.** The §7 matrix passes and is recorded in `progress.md`; a release archive built from
  the tree installs on a clean config without enabling enhanced mode.
