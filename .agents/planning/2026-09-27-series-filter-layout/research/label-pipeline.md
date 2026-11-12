# Research: label texture pipeline for the enhanced VERSION layout

Read-only research for the enhanced (`custom_series_enhanced`) mode's per-width filter labels.
Sources: `src/services/avs_layeredfs/{atlas_cloner,ifs_textures,mod_paths,xml_merger,mod,cache_hasher,file_hooks}.rs`,
`src/mods/series_expansion.rs`, `src/services/custom_options/asset_gen.rs`,
`src/mods/folder_expansion.rs`, `src/mods/s_marvelous/{assets,lamp_badge}.rs`,
`src/mods/music_wheel_song_length.rs`, `src/lib.rs`, `src/mods/mod_trait.rs`,
`scripts/build_release_archive.sh`, `updater/src/plan.rs`, `docs/scene_load_analysis.md`,
`docs/filter_menu_system_research.md`, `.agents/learnings/learnings.md`; the local stock
extraction `select_music_option_v3_ifs/tex/texturelist.xml`; and the raw IFS manifest of
`$DDR_WORLD_INSTALL/data/arc/bm2d/select_music_option_v3.arc` (unpacked to a temp dir with
`scripts/arc_tool.py`, manifest read with `ifstools`).

## 0. Headline finding — this IFS serves textures PER IMAGE, not per atlas

The stock `select_music_option_v3.ifs` manifest's `tex/` folder holds **204 entries: 203 are
`md5(<image name>)`, 1 is `texturelist.xml`, and 0 are `md5(<atlas name>)`** (checked for
`tex000`/`tex001`). The package loader opens one `tex/md5(image)` file per `<image>` and builds
the atlas itself (same finding for the lang IFS: `docs/scene_load_analysis.md:56-57`;
"per-image family" rule: `.agents/learnings/learnings.md:643-645`).

Consequences:

- The atlas blob the cloner writes (`_cache/<ifs>/md5(<prefix>_NNN)`,
  `atlas_cloner.rs:521-535`) is **never opened** for this IFS. Only the merged
  `<texture>/<image>` entries matter: they give the name, `<size>`, format and imgrect.
- Each cloned image is served by `ifs_textures::handle_texture` (`file_hooks.rs:416`,
  `ifs_textures.rs:361-396`): cache hit → `_cache/select_music_option_v3_ifs/md5(name)`;
  otherwise it looks for **`<ifs_mod_path>/<name>.png` or `<ifs_mod_path>/tex/<name>.png` in a
  mod folder** (`ifs_textures.rs:377-384`) and converts it (`cache_texture`, `:568-711`).
- The legacy mode works only because its source PNGs already sit at that serving path
  (`data_mods/custom_series/select_music_option_v3_ifs/tex/sefi_version_<key>.png`,
  `series_expansion.rs:250`).
- **This contradicts D9 as written.** If the PNGs live only in
  `data_mods/custom_series/series_labels/`, the merged texturelist declares the names but
  nothing can serve them: `handle_texture` → `None`, and the game opens a `tex/md5` that isn't
  in the IFS, so the label is blank. The enhanced mode must also **stage** each active PNG at
  the serving path (the s_marvelous pattern: `lamp_badge.rs:287-321`, `assets.rs:231-246`,
  `:795-830`), or **pre-convert** per-image blobs into `_cache` (§7, option B).

## 1. Q1 — one call, mixed donors, packing, growth, sizing

- **Mixed parent atlases: yes.** Specs are grouped by the donor's parent atlas (a `BTreeMap`
  keyed by atlas name, `atlas_cloner.rs:239-261`). Each group emits one cloned atlas, named
  `<prefix>_000`, `_001`, … in atlas-name order (`:275-329`). The donor lookup takes the
  first atlas containing the name (`:242-247`). Moot here: **all five donors are in `tex001`**,
  so any mix yields one group and one atlas.
- **Packing (donor mode, `fresh=false`).** The first spec *per distinct donor rect* takes the
  donor's exact slot (`claimed_donors`, `:373-389`). Every other spec is shelf-packed by
  `ShelfPacker` (`:736-826`) with **every stock `tex001` rect as a blocker** (`:293-311`) and a
  2 px pad (`:83`). The `allocate_packed_rect` named in the `NewTextureSpec` doc (`:56`) no
  longer exists (stale doc).
- **Growth.** Donor atlases start at the donor atlas size (2048×2048), then double height
  (while h ≤ w), then width, up to `MAX_ATLAS_SIDE` = 4096 (`:68`, `:816-823`). A spec that
  still won't fit is skipped with a WARN (`:433-439`). Fresh mode: fixed 2048 wide, starts
  256 tall, grows height only, spills into further atlases (`:75-79`, `:397-432`).
- **64 labels (simulated).** I ported `ShelfPacker` to Python and ran it against the real
  `tex001` rects:

  | Width | Donor mode, 1 slot + 63 packed | Fresh mode, 64 packed |
  |---|---|---|
  | 220 | all fit in 2048², no growth, max y 1064 | 2048×256 |
  | 104, 64, 44, 32 | all fit in 2048², max y ≤ 976 | 2048×256 |

  In donor mode the first extra label lands at (1782, 846). There are about 0.4 M
  blocker-nudge steps × 199 blockers, which is cheap in Rust. **64 × 220×20 is fine** in both
  modes.
- **Crop / scale — never scales.**
  - Donor-slot spec: its rect is the donor rect. The composite copies `min(png, rect)` from
    the top-left (`:489-503`): a larger PNG is cropped, a smaller one leaves transparent
    space. The emitted imgrect is the donor size regardless.
  - Every other spec: its rect is the **PNG's own size** (`image::image_dimensions`,
    `:391-393`), so a mis-sized PNG yields an entry of a different size than the slot.
  - The served per-image data (the part that renders) comes from `cache_texture`: a PNG
    smaller than the imgrect is padded top-left; **a larger one is rejected** and the label
    renders blank (`ifs_textures.rs:601-621`).
  - **So the D9 fallback "WARN, it is cropped" only holds if the enhanced code crops the PNG
    itself** before staging or conversion. Normalise every PNG to exactly W×20.
- **uvrect.** The cloner always emits a 1 px inset (`:833-848`). Stock donors inset 3 px on
  the right for league, world and gold (uv widths 216 / 100 / 60) and 1 px for title_other and
  level_00 (42 / 30). Cloned 220/104/64 labels therefore sample 2 px more than stock. That is
  harmless if the art keeps ink within the stock uv width, which equals the prototype's
  measured ink limits (`research/label-prototype.md`).

## 2. Q2 — donors (stock `select_music_option_v3.ifs`)

Root is `<texturelist compress="avslz">`. There are 203 unique image names (no duplicates),
no rotate or other extra attributes, and child order is `uvrect` then `imgrect`. `tex000`
(512², argb8888rev, 4 images) holds only logos; **`tex001` is 2048², `format="dxt5"`, 199
images**. All five donors exist, and each has a per-image blob in the IFS manifest. None of
them is in the lang IFS (`select_music_option_lang_eng_v3`).

| num_columns / template | Donor | Atlas | imgrect px (x0–x1, y0–y1) | Size | Stock uvrect width |
|---|---|---|---|---|---|
| 1 (`filter_switch_base01`) | `sefi_event_league` | tex001 | 1724–1944, 112–132 | 220×20 | 216 |
| 2 (`…base02`) | `sefi_version_world` | tex001 | 1944–2048, 112–132 | 104×20 | 100 |
| 3 (`…base03`) | `sefi_version_gold` | tex001 | 888–952, 756–776 | 64×20 | 60 |
| 4 (`…base04`) | `sefi_title_other` | tex001 | 1652–1696, 780–800 | 44×20 | 42 |
| 5 (`…base05`) | `sefi_level_00` | tex001 | 596–628, 844–864 | 32×20 | 30 |

The template ↔ label-canvas mapping matches `docs/filter_menu_system_research.md:189-195`.

Gotchas:

- The donor atlas is dxt5, but the clone is emitted as `argb8888rev` (`atlas_cloner.rs:547-552`).
  This is fine: the legacy path already does it.
- `sefi_version_world` touches the atlas's right edge (x1 = 2048).
- In donor mode the cloned atlas declares `<size>` 2048×2048, i.e. about 16 MiB of ARGB VRAM if
  the engine allocates at `<size>` (likely, not verified). Fresh mode is 2048×256, about 2 MiB.
- The local extraction is plain XML; the game copy is kbin, which `load_stock_texturelist`
  decodes (`atlas_cloner.rs:973-985`).

## 3. Q3 — auto-injection scope and path mapping

- `list_extra_pngs` scans, for every active mod folder, exactly `<mod>/<ifs_mod_path>` and
  `<mod>/<ifs_mod_path>/tex`, non-recursively (`ifs_textures.rs:726-743`).
  **`data_mods/custom_series/series_labels/*.png` is never auto-injected.**
- **`known_names` includes merged names.** When a merged xml exists, `parse_texturelist` runs
  on the *merged* output (`file_hooks.rs:361-366`), and `known_names` is every `TEXTURE_MAP`
  entry for that IFS, lowercased (`ifs_textures.rs:260-264`). A PNG in `tex/` whose stem
  equals a cloned image name is therefore not double-injected.
- Caveat 1: if the merge didn't happen, known_names lacks the cloned names. That occurs when
  the merged xml was written after the last mod-path scan, because `merge_xmls` uses the
  in-memory index (`xml_merger.rs:22-28`). Staged PNGs are then auto-injected as `ctex###`
  1:1 atlases (the known-bad path, learnings §"LayeredFS textures").
- Caveat 2: **stale staged PNGs whose names are no longer in the merged xml are auto-injected
  on every texturelist parse.** This covers a num_columns change, a removed row, a mode switch,
  or the mod being off. Each is decoded, AVSLZ-compressed and written on the game thread at
  scene 21 (`:749-874`). In the merge branch the injected texturelist is not served, so this
  is wasted work rather than a visual bug. Without a merged xml, `_cache/<ifs>/texturelist.xml`
  is served (`file_hooks.rs:385-398`), and that file is never deleted, so it can go stale.
- **`series_labels/…` is harmless as a path.** The mod index stores every file under a mod
  folder as a lowercase relative path (`mod_paths.rs:247-297`), so these become
  `series_labels/<stem>.png`. A game path only matches after the `data/` prefix is stripped
  (`mod_paths.rs:72-108`), and the game never opens `data/series_labels/…`. The `.ifs→_ifs`
  expansion can't produce it either (`file_hooks.rs:327-336`). Cost is about 100 extra index
  entries per rescan.
- `parse_texturelist` only registers anything if some mod has a `select_music_option_v3_ifs/`
  folder (`ifs_textures.rs:157-163`). `custom_series` (legacy PNGs) and `custom_options`
  (tab icon) both have one today.

## 4. Q4 — cached vs uncached, same-boot pickup, recommendation

**Today (legacy)**

- `series_expansion` calls **uncached** `generate_cloned_atlases` from `enable()`
  (`series_expansion.rs:839-844`, `:264-271`).
- It regenerates **every boot**: stock arc load, PNG decodes, a 2048² RGBA composite, a BGRA
  swap, AVSLZ of 16 MiB (with a 16 M-entry `prev` chain, `avslz.rs:31-32`), a blob write, and a
  merged-xml rewrite.
- It then runs an **unconditional full `init_mod_paths()` rescan** (`:279`).
- The merged xml's mtime changes every boot, which busts `xml_merger`'s path+mtime hash
  (`xml_merger.rs:37-47`, `cache_hasher.rs:41-52`). `custom_options::generate_static_tab_assets`
  already does the same to this IFS every boot (`asset_gen.rs:191-217`), so the re-merge
  happens regardless.
- The uncached path never sets `ATLASES_REBUILT_THIS_BOOT`.

**Same-boot pickup: yes.**

- Mods enable at `lib.rs` step 8 (`:622-628`), on the init thread, long before
  `select_music_option_v3` mounts.
- The mount happens at the scene-21 (CAUTION) song-select preload, after the player has
  started a credit. Package creation opens every `tex/geo/afp` member
  (`docs/scene_load_analysis.md:19-21, 35-39`).
- The rescan puts the merged xml and staged PNGs into the index; blobs written later are
  added to `CACHE_INDEX` by the cloner and `cache_texture` (`atlas_cloner.rs:535`,
  `ifs_textures.rs:706`).

**`_cached`** (`atlas_cloner.rs:643-722`)

- Hashes the texlist, then `atlas_prefix`, `fresh`, and each spec's name, donor and
  `png_path`+mtime (seconds).
- It skips only if the hash matches **and** `<mod_root>/<ifs>/tex/texturelist.merged.xml`
  exists (`:668`). On a rebuild it sets the reboot latch (`:720`).
- The sidecar is **one fixed file per IFS**: `_cache/select_music_option_v3_ifs/atlasbatch.md5`
  (`:651`; the doc at `:639` claims a per-key name, which the code doesn't do). No other mod
  uses `_cached` on this IFS today, so the enhanced mode would own it. A second `_cached` user
  of this IFS later would thrash it every boot.

**Recommendation: `generate_cloned_atlases_cached` with one `AtlasSet`, `fresh: true`.**

- Warm boots then skip everything except the arc read (400 KB) and about 64 `metadata()`
  calls.
- Use a conditional rescan (probe merged xml plus every staged name with `find_first_modfile`;
  rescan only on a miss or after deleting files) as in `lamp_badge.rs:343-354` and
  `music_wheel_song_length.rs:339-350`. Never rescan unconditionally.
- Why fresh:
  - 2048×256 instead of a 2048² build and VRAM footprint.
  - In a per-image IFS, fresh and donor entries differ only in atlas `<size>` and imgrect
    origin; the name and imgrect size conventions are identical.
  - Fresh mode already serves name-bound per-image textures elsewhere (s_marvelous,
    music_wheel_song_length, custom_options previews).
  - **Not yet proven on the `filter_item`/`choices_usr` binding.** One cabinet check is
    needed; the fallback is `fresh: false`, the legacy-proven mode, at the cost of the 2048²
    build on cache misses.
- **Splash side effect.** Every rebuild (config edit, num_columns change, art change, and every
  updater install, §7) shows "REBOOT THE GAME AT LEAST ONCE" (`lib.rs:708-734`). For this IFS
  that warning is a false positive, because the rescan makes the labels appear the same boot.
  Either accept it, or add a non-latching variant or flag to the batch API. Collaborators who
  flip `num_columns` will see it on every change.

## 5. Q5 — mode switches, num_columns changes, zero textures

- **Shared merged path.** Both modes writing `MOD_ROOT=./data_mods/custom_series` share
  `custom_series/select_music_option_v3_ifs/tex/texturelist.merged.xml`. `write_merged_texturelist`
  replaces it whole (`atlas_cloner.rs:557-584`), so one mode per boot is consistent, **except
  for a stale-Cached bug**:
  1. Boot A, enhanced: Rebuilt, sidecar H.
  2. Boot B, legacy: the uncached writer overwrites the merged xml with legacy entries; the
     sidecar still says H.
  3. Boot C, enhanced with identical inputs: the hash matches and the merged xml exists →
     `Cached` → **the enhanced labels are missing**.

  Guard options: after `Cached`, check that the merged xml contains `name="<stem>"` for every
  spec (else delete the sidecar and rerun), or have legacy delete `atlasbatch.md5` when it
  writes.
- **num_columns change.** New stems and a new donor change the hash → rebuild → the merged xml
  now lists only the new width.
  - Old per-image cache blobs `_cache/<ifs>/md5(*_Ncol)` stay behind. They are harmless:
    never requested.
  - Old *staged* PNGs stay in `_ifs/tex` and trigger the auto-inject waste (§3 caveat 2).
    Unstage by pattern: delete `sefi_version_*_[1-5]col.png` not in the current set. This
    pattern cannot hit legacy release PNGs, which have no suffix.
- **Prefix.** Keep a prefix distinct from legacy `cser_version` (e.g. `cser_enh`). With one
  shared merged file it only avoids blob overwrite (irrelevant for per-image serving). If the
  modes ever write separate merged files, it is **required**: a stale legacy file plus an
  enhanced file would otherwise both declare a `<texture name="cser_version_000">`.
- **Zero textures** (`filters: []`, or every PNG missing).
  - `_cached` returns `Nothing` and **leaves the old merged xml in place** (`:711-713`).
  - Its stale entries stay declared. The labels still load wherever old per-image blobs exist
    in `_cache`, and cost one slow-path open each (~9 ms on Wine,
    `docs/scene_load_analysis.md:47-52`).
  - Fix: write an empty merged texturelist (`write_merged_texturelist(.., "")` gives
    `<texturelist>\n</texturelist>`, which merges nothing), or delete it and rescan, and
    unstage.
- **Mod off or unregistered.** `init()` runs at registration even when the mod is toggled off
  (`mod_trait.rs:167-197`); `enable()` does not. If `init()` returns early (no or empty
  config, `series_expansion.rs:571-578`) or a required signature is missing, nothing cleans
  up. The last merged xml and any staged PNGs persist. This is pre-existing for legacy; with
  staging, the auto-inject waste repeats every boot. Put cleanup where it always runs, before
  the early returns in `init()`, or use option B (§7).
- **Content change under the same name.** `handle_texture` serves any indexed `_cache` blob
  without an mtime check (`ifs_textures.rs:371-374`). Editing or updating
  `sefi_version_x_3col.png` keeps the old art until a `purge_texture_replacement`
  (`:407-429`) or a `_cache` wipe. Staging must byte-compare, write, then purge
  (`s_marvelous::serve_image`, `assets.rs:237-246`). Legacy has the same latent staleness.

## 6. Q6 — name constraints

- **No length limit** in the cloner, texturelist parse, `CacheHasher` or LayeredFS lookup. The
  game-side stock names reach 34 chars in this IFS (`sefi_filter_change_selection_on_bd`) and
  44 in the lang IFS. The longest prototype stem is 28 (`sefi_version_supernova2_5col`).
- AVS only ever sees the fixed-length `_cache/<ifs>/<md5>` path (fine against the 128-byte
  `GetLongPathNameA` issue, `file_hooks.rs:430-436`). Staged and source PNG paths are read with
  `std::fs` only.
- **Characters: restrict to `[a-z0-9_]`.**
  - Names go raw into XML attributes with no escaping (`atlas_cloner.rs:836`, `:548-550`), and
    parsers stop at the next `"` (`:988-994`).
  - Serving is keyed by `md5(exact bytes)` (`ifs_textures.rs:235`), so the texturelist name
    must byte-equal what the game requests: `sefi_` + the label key (`FilterButton+0xC8`,
    `docs/filter_menu_system_research.md:594-597`). That is case-sensitive.
  - Mod-file lookup is lowercase-insensitive (`mod_paths.rs:113`, `:287`, `:292`).
- The legacy 15-byte SSO `assert!` (`series_expansion.rs:360-363`) must not be reused for
  enhanced keys like `version_supernova2_5col` (23 bytes); D19's heap strings avoid it.

## 7. Q7 and other shipping notes

- **Release.** `build_release_archive.sh:150-159` rsyncs the `data_mods/` *working tree*
  (minus `.DS_Store`). `series_labels/` ships automatically: the 100 prototype PNGs total about
  92 KB.
- **Updater.** Release files are rewritten unconditionally each install (`updater/src/plan.rs:98-111`),
  and I found no mtime preservation in `updater/src`. So every update bumps the source mtimes
  → one `_cached` rebuild plus a reboot splash after each update (custom_options already
  does this).
  - Dropped PNGs are pruned only if unmodified (`:113-129`).
  - DLL-staged copies and the merged xml are in no manifest, so the updater never touches
    them; the DLL owns their lifecycle.
  - A collaborator PNG in `series_labels/` whose name a later release also ships is
    overwritten by the release.
- **Allowlist.** Keeping all output inside `custom_series/` (not a new generated mod folder
  like `data_mods/bg_preview`) keeps it served under an operator `layeredfs.allowlist`
  (`mod_paths.rs:226-230`).
- **Load cost.** Each declared label is one slow-path `_cache` open at scene 21 (~9 ms on
  Wine). Declare only the textures the configured rows use: 20 rows cost about 0.2 s and 64
  about 0.6 s of CAUTION time. Never declare all five widths.
- Generated PNGs must be regenerated, not hand-edited (AGENTS.md).

## 8. Recommended pipeline (enhanced mode, at `enable()`)

1. **Resolve.** `num_columns` N (1–5) → width W (220/104/64/44/32) and donor (§2). For each
   *unique* `texture` in `filters`:
   - stem `sefi_version_<texture>_<N>col`;
   - source `data_mods/custom_series/series_labels/<stem>.png`, falling back to
     `sefi_version_<texture>.png` per D9;
   - validate `[a-z0-9_]`.
2. **Normalise** each source to exactly W×20 (crop or pad top-left, one WARN on mismatch).
3. **Serve per image.** Pick one option:
   - **A — stage (precedent, no LayeredFS change).** Write the normalised PNG to
     `data_mods/custom_series/select_music_option_v3_ifs/tex/<stem>.png` only when the bytes
     differ, then `purge_texture_replacement`. Delete `sefi_version_*_[1-5]col.png` not in the
     set. Also run the unstage in legacy mode and in the always-run part of `init()`.
   - **B — pre-convert (cleaner).** Add a small `ifs_textures` helper that writes the
     per-image blob (argb8888rev + AVSLZ, W×20, exactly `cache_texture`'s encoding) to
     `_cache/select_music_option_v3_ifs/md5(<stem>)` and calls `cache_index_insert`. Nothing
     is ever placed under an `_ifs` folder, so there are no auto-inject or stale-PNG hazards,
     and stale blobs are inert. Rewrite the blobs whenever the batch is `Rebuilt` or a blob is
     missing.
4. **Texturelist.** Call `generate_cloned_atlases_cached(texlist, "select_music_option_v3_ifs",
   "./data_mods/_cache", "./data_mods/custom_series", [AtlasSet { prefix "cser_enh", specs
   (new_name = stem, donor = the width's donor, png_path = the normalised file), fresh: true }])`.
5. **Guards.**
   - On `Cached`, verify every stem is in the merged xml, else delete the sidecar and rerun.
   - On `Nothing` or an empty set, write an empty merged texturelist and unstage.
6. **Rescan** only if the merged xml or a staged file is missing from the index, or files were
   deleted.
7. **Splash.** Decide whether the rebuild latch should fire for this caller (§4).

## 9. Risks and unknowns

- **Fresh vs donor mode** for `choices_usr` (Ordinal_112) is unverified on the cabinet. It is
  expected to work, since per-image serving makes the atlas origin irrelevant. The fallback is
  `fresh: false`.
- Whether the engine allocates the full `<size>` VRAM for a cloned atlas in a per-image IFS is
  assumed, not verified.
- The cloner's 1 px uvrect inset versus the stock 3 px right inset (220/104/64) gives a 2 px
  wider sample. It is invisible if the art respects the stock ink limits.
- The fixed per-IFS `atlasbatch.md5` makes `_cached` single-owner per IFS: a future second
  user of `select_music_option_v3_ifs` would thrash it.
- `_cache/<ifs>/texturelist.xml` from auto-inject is never deleted. It would be served only if
  this IFS ever had no merged xml at all; today custom_options always writes one.
- Legacy's pre-existing issues (not in scope, but they interact with a mode switch):
  - It regenerates a 2048² atlas and rescans every boot.
  - It never cleans its merged xml when disabled.
  - Edited PNGs stay stale in `_cache`.
