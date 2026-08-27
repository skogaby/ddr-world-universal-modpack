# Idea Honing — decision register

Project: loading-screen speedup (RAM cache preload + enum→scalar option slimming).
Statuses: Proposed / Accepted / Overridden / Assumed / Open.

**Readiness Confirmed 2026-08-11** — all recommendations accepted by the user.

## Phasing (user direction)

- **Phase 1 (do first):** change 2 (enum→scalar, remove the 150 `seop_op_item_*`
  textures) + change 3 (script). Self-contained, shippable, and expected to be
  the larger CAUTION win. Decisions D7–D15.
- **Phase 2 (deferred):** change 1 (RAM cache preload, mechanism B). Only pursued
  if Phase 1's measured win is insufficient. Decisions D1–D6. Designed at sketch
  fidelity now; fully specified when picked up (footprint should be re-measured
  against the post-Phase-1 injected-texture set first).

| ID | Decision | Why it matters | Recommendation | Status |
|----|----------|----------------|----------------|--------|
| D1 | RAM-cache serve mechanism | Determines the whole shape + risk of change 1 | **B**: preload `_cache/` into an in-process RAM map, keep the real open, intercept `avs_fs_read` to memcpy from RAM | Accepted |
| D2 | Which cache files to preload | Memory footprint vs coverage | Texture-family cache files served during scene loads + all small (<256 KB) cache files; skip the big bg_preview/arc repacks (already RAM/ramfs-mounted by the engine) | Accepted |
| D3 | Memory budget + fallback | Bytes stay resident; must bound | Total cap (default ~24 MB) + per-file skip >2 MB; any miss/over-cap falls through to today's disk redirect | Accepted |
| D4 | `avs_fs_close` hook for handle retirement | Correctness of B (handle reuse) | Add a close hook that drops `handle→buf` entries; fail-open (pass through on panic), same catch_unwind discipline as the others | Accepted |
| D5 | Config gate + kill switch | Operability | `layeredfs.preload_cache` bool, default **true**; force-off when `developer_mode` is on (dev mode = no cache) | Accepted |
| D6 | Measure open-vs-read split first? | De-risks D1 | Skip a separate spike — B dominates A and is lower-risk than C regardless of the split; add a one-line timing log behind `diagnostics.profiling` instead | Accepted |
| D7 | Convert all 9 `EnumIndexed` cosmetics to `Scalar` | The core of change 2 | Yes — flip `render:` for appeal_board, background ×2, character ×2, lane ×2, lanecover ×2 | Accepted |
| D8 | Remove `RenderMode::EnumIndexed` + `build_indexed_enum_values` | Dead code after D7 | Remove both (request says "remove entirely"); keep `EnumFixed` (VIDEO SIZE) and `Scalar` | Accepted |
| D9 | Scalar value display base | UX — the number shown in the selector | **1-based** ("1".."N") via a new display-only `ScalarFormat::OffsetInteger { display_offset }` variant; internal value stays the 0-based index | Overridden |
| D10 | Keep the preview chrome for the now-scalar cosmetics | Blank preview box otherwise | Call `preview_gen::generate_chrome(option_id)` in the scalar arm of `mod.rs` (idempotent; only runs for webui cosmetic categories) | Accepted |
| D11 | Delete the 150 generated `seop_op_item_*.png` from the repo | Script stops *generating* but never prunes; stale PNGs would still be injected | Delete `data_mods/custom_options/…lang_eng…/tex/seop_op_item_*.png` | Accepted |
| D12 | Update `scripts/gen_option_labels.py` | Change 3 | Remove `ITEM_RIBBON_COUNT` + the `item_<NNN>` comprehension + the now-stale docstring/comments | Accepted |
| D13 | `customize_movie_size` (VIDEO SIZE) scope | It's `EnumFixed`, not indexed | Out of scope — keeps its authored `seop_op_fullscreen/on/off` ribbons | Assumed |
| D14 | Backend / save wire format | Could break saved cosmetics | Unaffected — index→asset-id `save_transform` + `SaveOnly` are unchanged by enum→scalar | Assumed |
| D15 | Docs | Keep AGENTS.md/README honest | Update the WebUI-options + LayeredFS notes to reflect scalar rows and the RAM cache; add a `docs/` research note | Accepted |

## Questions behind the flagged decisions

**D1 — mechanism.** A (warm OS cache) is a few lines but leaves every per-file
wineserver `open/read/close` round-trip in place. C (synthetic handle or AVS
ramfs mount) removes all FS calls but needs handle emulation or ramfs-ABI RE and
is the riskiest in a live FFI callback path. B keeps a cheap real open (so
`lstat`/`fstat`/`close` and the handle stay genuine) and kills the expensive part
— the data-transfer read — by memcpy from an in-process buffer. Recommend B.

**D2/D3 — footprint (likely not yet considered).** `_cache/` today is ~8.4 MB for
the option IFS alone, but the tree also holds `bg_preview` (~35 MB) and `arc`
repacks. Preloading *everything* would pin tens of MB for process life. The
engine already serves bg_preview/arc via its own RAM/ramfs mounts, so those don't
benefit from our RAM map. Recommend scoping the preload to the texture-family
caches (the ones on the scene-load hot path) with a total cap + per-file cap and
disk fallback, so footprint stays ~10–15 MB and predictable.

**D6 — skip the spike.** We could ship an instrumented build first to measure the
open-vs-read split, but B is the right call for either outcome, so I'd fold a
cheap timing log behind the existing `diagnostics.profiling` flag rather than gate
the work on a measurement round-trip.

**D9 — 1-based display (user override, 2026-08-11).** The old ribbon showed
"ITEM #001" (1-based); the user wants the scalar selector to read "1".."N" for
parity. Implemented as a **display-only** formatter variant
(`ScalarFormat::OffsetInteger { display_offset: i32 }`, rendered as
`value + display_offset`) in `format_scalar_value` — a pure function with one
call site (`rows.rs::push_scalar_value_text`). The internal value model stays the
0-based asset index everywhere (registry, save_transform, seed, overlays), so
nothing else changes.

## Assumptions I settled myself

- D13, D14 above.
- The enable-time ordering in `webui_options::enable` (discover → register →
  seed-on-scene-25 → overlay init) is preserved; only the per-category
  `RegisterSpec` construction and the `generate_chrome` call site change.
- No change to `custom_options` persistence, `seed_registry_from_game`, or the
  preview/bg overlays beyond what D10 requires.
