# Scene 18/21 Loading-Screen Analysis — asset preload path + injected-texture cost

**Date:** 2026-08-11
**Build:** `gamemdx_20260721.dll` (image base `0x180000000`; all addresses
file-relative). Log evidence from a CrossOver/Wine cabinet at 120 fps.
**Feature context:** the Phase-1 enum→scalar slimming of the WebUI cosmetic
pickers (`.agents/planning/2026-08-11-loading-screen-speedup/`), plus the
`layeredfs.developer_mode` regression diagnosis that preceded it.

## What scenes 18 and 21 are

Scene 18 (LANGUAGE_TO_MODE_INTERSTITIAL) and scene 21 (CAUTION) are bulk
**asset-preload** screens:

- Scene 18 preloads the mode-select flow's BM2D packages:
  `scene_select_mode`, `scene_caution`, `scene_payment_window`,
  `scene_eamusement_window` (+ `_lang_eng` variants, `common_operation_guide`)
  — ~2,200 inner-file opens.
- Scene 21 preloads the song-select package set: `select_music_side`,
  `select_music_option`, `select_music_card`, `select_music_folder`
  (+ `_lang_eng` variants) — ~3,700–4,100 inner-file opens.

## The load mechanism (why there is no Fast-Bootup-style hack here)

- Arc-level loads go through the engine's `FileManager` (singleton
  `DAT_1806f2f48`). Its pump `FUN_1801fdbf0` runs **once per frame** from the
  main engine update (`FUN_180003020`) and drives each record through the async
  open/read state machine (same 0x40-stride, status-at-+0x20 table the
  fast-bootup mod's SSQ actor walks).
- New async opens per pump are budgeted by `FileManager+0x70` = **4** (set from
  the ctor params, `FUN_1801fd350` / `FUN_1801fd590`). This caps only the
  arc-level fetches (~10–13 per scene) — a negligible slice of the load time.
  (Synchronous drain loops exist but are boot-only: `FUN_1801fe380` for
  startup.arc/soundbanks.arc, the shader.arc drain in `FUN_1801f2420`.)
- Package creation is gated by `bm2d::data::…::Manager::Update`
  (`FUN_1801acd40`, pumped per frame): once ALL queued arcs report finished, the
  packages are created — and creation **synchronously** opens every inner
  `/tex`, `/geo`, `/afp` member through `avs_fs_open`/`avs_fs_read` (AVSLZ
  decode + D3D9 texture upload per file).
- Measured open rates in those windows burst to 300–870/s — far above the
  4/pump × frame-rate ceiling — confirming the inner opens are **not**
  frame-paced. They are throughput-bound real work. Unlike the SSQ boot preload
  (which idled a full frame per item and made Fast Bootup possible), there is no
  idle to reclaim: **per-open cost is the only lever.**

## Per-open cost: stock vs mod-injected

Stock inner files are served from the arc's in-RAM ramfs image (mounted
`image.bin`) — cheap. LayeredFS-served files (our injected textures) redirect
the open to a real `data_mods/_cache/<ifs>/<md5>` file on disk, paying Wine
round-trips for open/read/close (~9 ms/open observed on the reference cabinet;
OS-file-cache-warmth dependent).

Measured (verbose log, CAUTION window, healthy config): the
`select_music_option_lang_eng_v3` package alone took ~4 s of the ~7–11 s screen,
with **208 of its 417 texture opens on the slow disk path**. Attribution (the
game opens per-image `tex/<md5(name)>` files, so matching MD5s against the
merged `texturelist.xml` is exact):

| Injected family | Slow-path opens |
|---|---|
| `seop_op_item_*` (ITEM #NNN value ribbons) | **138** |
| `seop_image_*` (preview panels) | 38 |
| `seop_item_*` (row labels) | 26 |
| bespoke `seop_op_*` ribbons + misc | 6 |

Scene 18's packages carry **no** injected `custom_options` textures — its
loading cost is stock. (Its 2026-08 slowness was entirely the
`layeredfs.developer_mode: true` regression, below.)

## The two config-side costs (regression diagnosis, 2026-08-11)

- **`layeredfs.developer_mode: true`** — bypasses the in-memory mod-file index:
  every `find_first_modfile` re-scans `data_mods/` (read_dir + metadata) and
  stats each mod folder, 2–3× per open through the `.ifs→_ifs` expansion —
  ~30–60 Wine filesystem calls per game file open. Effect: CAUTION 20 s
  (vs 7 s), scene 18 9 s (vs 2 s). This is the same per-open-stat cost class the
  June 2026 CAUTION fix (`ifs_textures::CACHE_INDEX`) eliminated for non-dev
  mode. Dev mode is *designed* to trade speed for live file checks — just don't
  leave it on.
- **`layeredfs.verbose: true`** — one `OutputDebugStringA` per open; ~+4 s on
  CAUTION. Diagnostic only.

## Phase 1 fix: cosmetic pickers → scalar rows (shipped with this note)

The 138 `seop_op_item_*` opens existed only to label the `EnumIndexed` WebUI
cosmetic pickers' value ribbons. Those categories are now
`RenderMode::Scalar`: the row renders its value through the game's native digit
text path — no per-value texture at all — displaying the **1-based** position
("1".."N") via the display-only `ScalarFormat::OffsetInteger { display_offset: 1 }`
(`src/services/custom_options/api.rs`); the internal value stays the 0-based
asset index, so persistence (`SaveOnly` + index→asset-id `save_transform`),
card-in seeding, and the preview overlays are untouched. The preview box works
as before: both row builders install the `IOptionElement` slot-0 override that
binds the base `seop_image_<id>` chrome (`generate_chrome` now runs on the
scalar registration path), and `preview_overlay`/`bg_preview_overlay` draw the
live art on top.

Retired with the conversion: `RenderMode::EnumIndexed`,
`build_indexed_enum_values`, the `ITEM_RIBBON_COUNT` block in
`scripts/gen_option_labels.py`, and the 150 committed `seop_op_item_*.png`.
The lang_eng atlas rebuild self-invalidates (`atlasbatch.md5` hashes the
registered spec list), so the merged texturelist drops the 150 entries on the
next boot and the game never requests those MD5s again.

**Cabinet note:** the 150 PNGs must also be removed from the cabinet's
`data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/` — LayeredFS
texture *injection* packs any unmatched PNG found there into the served
texturelist (`inject_new_textures`), which would resurrect the opens with no
option referencing them. Stale `_cache/` files are harmless (never requested).

Expected effect: CAUTION slow-path opens drop ~208 → ~70 (the remaining
previews/labels/bespoke ribbons). Scene 18: no change (nothing injected there).

## Phase 2 (designed as a sketch, currently shelved)

If the remaining ~70 slow-path opens still matter: preload eligible `_cache/`
texture files into an in-process RAM map at boot and serve `avs_fs_read` from
memory (keep the cheap real open; add an `avs_fs_close` hook to retire
handle→buffer entries; `layeredfs.preload_cache` gate, off under dev mode).
See `.agents/planning/2026-08-11-loading-screen-speedup/design/detailed-design.md`.

## Cross-version notes

- The `FileManager` pump/budget addresses above are 20260721; the singleton and
  record-table layout match what `fast_bootup.rs` already tracks via
  `step_data_global_table` (stride 0x40, status +0x20), so re-derivation on a
  new build can anchor there.
- The per-image `tex/<md5(name)>` open convention and the merged-texturelist
  attribution method are engine-stable (IFS format), not build-specific.
