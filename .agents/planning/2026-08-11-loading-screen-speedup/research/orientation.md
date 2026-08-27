# Orientation — findings before requirements

Repo-relative paths throughout. Game addresses file-relative to `gamemdx.dll`
`0x180000000` (build `gamemdx_20260721`).

## What scenes 18/21 actually do (from the diagnosis session)

- Both are **asset-preload** screens. The engine `request_load`s a batch of BM2D
  `.arc` packages; `bm2d::…::Manager::Update` (`FUN_1801acd40`) creates each
  package once its arc finishes loading; package creation reads every inner
  `/tex,/geo,/afp` member through AVS.
- The `FileManager` pump (`FUN_1801fdbf0`) runs **once per frame** on the main
  thread; new async opens are budgeted at `FileManager+0x70` = **4/pump**. This
  gates only the arc-level loads (~10–13 arcs), which is a negligible slice.
- The inner-member opens are **synchronous throughput-bound work** (AVSLZ decode +
  D3D9 texture create under CrossOver), bursting 300–870/s — NOT a frame-paced
  1-per-frame drip. So there is **no Fast-Bootup-style pacing hack** available for
  these screens; the lever is per-open cost, which is ours.

## Why `select_music_option_lang_eng_v3` is the slow package (~4 s of ~7 s)

Its texture opens are ~half mod-injected (`custom_options`), served from
`data_mods/_cache/…` real disk files via the LayeredFS→Wine redirect (~9 ms/open)
vs stock ramfs textures (300–700/s). Cabinet inventory:

- `select_music_option_lang_eng_v3_ifs` cache dir: **234 files, 8.4 MB**.
- Injected source PNGs (`data_mods/custom_options/…lang_eng…/tex`): **229**, of
  which **150 are `seop_op_item_001..150`** ("ITEM #001".."ITEM #150" ribbons).

## Change 2 — enum→scalar (removes the 150 item ribbons)

The custom-options framework (`src/services/custom_options/`) has two row kinds
(`api.rs::UiKind`):

- `Enum` — left/right cycles a labeled value list; each value needs a
  `seop_op_<key>` ribbon texture. The WebUI cosmetics use a runtime-count variant.
- `Scalar` — renders the value as digits via the game's **native `seop_num_*`
  sprites — no per-value texture**. Two step sizes (fine/coarse), `ScalarFormat`.

The 9 cosmetic categories in `src/mods/webui_options/discovery.rs` use
`RenderMode::EnumIndexed`; `mod.rs::build_indexed_enum_values` builds one value
per asset labeled `seop_op_item_<NNN>` (1-based). `RenderMode::Scalar` **already
exists** and is described as the value-model-identical default:

> "All modes use the same index-based value model (`value` = index into
> discovered `asset_ids`), so persistence/apply are identical; only the UI
> presentation differs." — `discovery.rs`

The Scalar registration path already exists in `mod.rs`:
`RegisterSpec::scalar(option_id, 0, count-1, 1, ScalarFormat::Integer)`.

**Preview box + live-art overlay survive the switch:** `install_ioptionelement_vtable`
(`rows.rs:1026`) is called by **both** the enum (`rows.rs:706`) and scalar
(`rows.rs:827`) row builders, installing the `preview_image_name_trampoline` that
binds `seop_image_<id>` for the focused row and fires `fire_preview_request` to
drive the WebUI overlay. `preview_image_name_for_value` returns the base
`seop_image_<id>` for non-enum kinds. So a scalar cosmetic row still shows its
chrome + live art.

**The one wiring dependency:** `preview_gen::generate_chrome(option_id)` (builds
the `seop_image_<id>` base chrome from the category `_TEMPLATE`) is currently
called **only in the `EnumIndexed` arm** of `mod.rs` (`mod.rs:188`). After the
switch, the (now scalar) cosmetic categories must still get `generate_chrome`, or
their preview box renders blank. Fix: call it in the scalar arm too (idempotent).

Value-display semantics: scalar range `0..=count-1` renders "0".."N-1" (0-based),
vs the old "ITEM #001" (1-based). Internal value model is 0-based index already
(save_transform, `seed_registry_from_game`, overlay all 0-based), so 0-based
display is the low-risk match; 1-based parity would need a small `ScalarFormat`
display-offset addition and is optional.

Persistence unchanged: still `PersistMode::SaveOnly` + `persist_save_transform`
(index→asset id). `RenderMode::EnumIndexed` + `build_indexed_enum_values` become
dead code after the switch.

## Change 3 — `scripts/gen_option_labels.py`

`ITEM_RIBBON_COUNT = 150` (line 134) and the `RIBBONS += [(f"item_{i:03}", …)]`
comprehension (lines 142–144) generate the `seop_op_item_<NNN>.png` set. Removing
those two stops generation. The 150 already-generated PNGs under
`data_mods/custom_options/…/tex/` must be deleted from the repo to actually shrink
the injected set (the script only writes, never prunes).

## Change 1 — RAM-preload the `_cache/` (option 2)

Current serve path (`src/services/avs_layeredfs/file_hooks.rs`):
`hook_avs_fs_open` → `fs_open_body` → `find_mod_replacement` →
`ifs_textures::handle_texture` returns a `./data_mods/_cache/<ifs>/<md5>` path →
`original.call(cpath,…)` gives the game a **real AVS handle on the cache file**;
the game later calls `avs_fs_read(handle,…)` (hook at
`file_hooks.rs:253`, currently pass-through) and `avs_fs_close`.

There is already an in-memory **cache index** (`ifs_textures.rs::CACHE_INDEX`,
built by `build_cache_index()` walking `_cache/` at init) used to replace a
per-open `exists()` stat — so the enumeration/boot-walk infrastructure exists.

The per-file Wine cost is `open + lstat + read + close`, each a wineserver
round-trip; for the larger atlases (up to ~2 MB) the `read` transfer dominates
when the OS cache is cold, the round-trips dominate when warm (this is the 90/10
nondeterminism the June note recorded).

Candidate mechanisms (materially different; central design decision):

- **A. Warm the OS/Wine file cache at boot** — read every `_cache/` file once so
  the game's later reads hit Wine's warm cache. Tiny change, but keeps every
  per-file `open/read/close` wineserver round-trip; only removes cold-disk latency.
- **B. Intercept `avs_fs_read`, serve bytes from an in-process RAM map** — preload
  `_cache/` files into `HashMap<path, Arc<[u8]>>` at boot; on a cache-file open,
  keep the real `original.call` (so the handle, `lstat`/`fstat`, and `close` stay
  valid) but record `handle → (buf, cursor)`; the read hook `memcpy`s from RAM and
  advances the cursor, skipping the real AVS read. Eliminates the biggest
  round-trip (the data transfer) and the disk I/O; keeps a cheap real open.
  Needs an `avs_fs_close` hook (not currently installed) to retire handle→buf
  entries and avoid handle-reuse mis-serves. This is the "serve from memory"
  reading of option 2.
- **C. Full RAM serve (synthetic handle or AVS ramfs mount)** — never touch the
  real FS: either fabricate a handle our hooks fully service (risky: must emulate
  read/lstat/fstat/close and not collide with real AVS handles) or mount the bytes
  as an AVS `ramfs` entry the way the engine mounts `image.bin`. Biggest win
  (zero Wine FS calls) but the most RE + risk.

Open unknown that decides A-vs-B-vs-C payoff: the **open-vs-read cost split** for a
cache file under this CrossOver bottle. Not measured yet. A quick instrumented
build (time `original.call` open vs read for cache paths) would settle it.

## Interaction / sequencing

Change 2 removes 150 of ~229 injected textures in the hot package (~65%) at low
risk — likely the larger CAUTION win on its own. Change 1 (RAM cache) then speeds
the *remaining* injected textures (labels, previews) **and every other mod's
cache files** (bg_preview, folder/series atlases, etc.), so it's the broader play
but higher-risk. Recommend landing change 2 first (measure), then change 1.

## Post-design empirical reconciliation (2026-08-11, log_verbose.txt run)

Matched the CAUTION-window opened `tex/<md5>` names against the merged
`texturelist.xml` (438 `<image>` entries across 26 canvases):

- The game opens **per-image** md5 files (417 unique matched image names; 0
  canvas-name matches), so per-open attribution is exact.
- CAUTION window, `select_music_option_lang_eng_v3` texture opens: **208 served
  from `_cache/` (slow disk path)** vs 209 from the stock ramfs. Of the 208:
  **138 `seop_op_item_*`**, 38 `seop_image_*`, 26 `seop_item_*`, 4 bespoke
  `seop_op_*`, 2 other.
- **Phase 1 therefore removes ~138/208 ≈ 66% of the slow-path opens**; ~70
  remain (previews + labels + bespoke ribbons) as Phase 2's target population.
- **Scene 18 carries no injected `custom_options` textures** (its packages are
  scene_eamusement/payment/mode-select). Phase 1 improves **CAUTION only**;
  scene 18 is already at stock-ish cost now that dev mode is off. Set
  expectations accordingly.
- Host-test note: `scripts/validate_song_playback_speed.sh` builds a temp-dir
  `#[path]` harness compiling `custom_options/{api.rs, registry.rs,
  availability_tests.rs}` — the new `ScalarFormat` variant must keep that
  harness compiling; the formatter arm in `rows.rs` is outside any host harness
  (cabinet-validated, per repo convention).
