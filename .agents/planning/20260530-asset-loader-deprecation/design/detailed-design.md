# Detailed Design — Asset Loader Deprecation

## Background: how the two texture-delivery mechanisms differ

`asset_loader::register_arc` → `arc_load` is an **active** mechanism: it tells the
game to open and register a container, after which every texture inside resolves
by bare name through BM2D's `get_bitmap_info` callback (the same callback
`texture_resolver::resolve` walks).

LayeredFS is a **reactive** mechanism: it hooks `avs_fs_open`/`read`/`mount` and
only acts on files the *game itself* opens. For net-new textures, the pathway is:
the game opens an IFS's `tex/texturelist.xml`; LayeredFS intercepts that read,
runs `ifs_textures::parse_texturelist` → `inject_new_textures`, packs each extra
PNG into a 1:1 atlas, appends `<texture>` entries to a cached texturelist, and
registers MD5→atlas mappings so the subsequent texture reads are served from
`data_mods/_cache/`. The new names then resolve through BM2D exactly like stock
textures — **but only because they ride an IFS the game already opens.**

Consequence: a texture name can only become BM2D-resolvable via LayeredFS if it is
injected into a host IFS the game opens on its own. Free-floating textures with no
host require either an `arc_load` (the thing we're removing) or a runtime IFS+kbin
writer (which does not exist in this codebase — we have a kbin *decoder* and IFS
*reader* only).

## Requirements

- **R1.** Remove all production calls to `asset_loader::register_arc`.
- **R2.** Remove the `asset_loader` service and the `arc_load` signature (plus its
  derived `arc_file_open`, if unused elsewhere) once R1 holds.
- **R3.** series_expansion custom labels render via LayeredFS injection into
  `select_music_option_v3.ifs`, configured by dropping PNGs in `data_mods/`.
- **R4.** folder_expansion is unaffected at runtime (its ARC branch was dead).
- **R5.** hello_world's text-bounce demo still runs; its image widgets are removed.
- **R6.** `image_widget` + `widget_renderer::create_image_widget` remain compilable
  and functional for future callers. The image-widget texture-resolution loop must
  no longer depend on `asset_loader::is_loaded()`.
- **R7.** No new game signatures, no new LayeredFS service code.

## Component changes

### `src/services/asset_loader.rs` — DELETE
Remove the file, the `pub mod asset_loader;` line in `src/services/mod.rs`, and its
doc bullet in the `services/mod.rs` header comment.

### `src/core/signatures.rs` — remove `arc_load`
- Remove the `arc_load` `SignatureDefinition` (line ~29).
- Remove the derived-address block that decodes `arc_file_open` from `arc_load`
  (lines ~956–975) **after confirming `arc_file_open` has no other consumer**.
  (Grep shows asset_loader was the only `arc_load` consumer; `arc_file_open` is a
  legacy derivation that, per current grep, is not read anywhere else. Verify
  during implementation; if something consumes it, keep the derivation but source
  its landmark differently or leave a TODO.)

### `src/lib.rs` — remove registration + init
- Delete `asset_loader::register_arc("data/arc/bm2d/custom_mod.arc");` (line ~166).
- Delete `asset_loader::init(&signatures);` + its `profiling::tick("asset_loader")`
  (lines ~167–168).
- Remove `asset_loader` from the `use crate::services::{…}` import (line ~26).

### `src/services/widget_renderer.rs` — drop asset_loader coupling
- Remove the `asset_loader::load_arcs();` call in `render_function_hook` (line ~80)
  and the surrounding `drop(r)` that exists only to release the lock before that
  call. (Font capture itself stays.)
- In `create_image_widget`'s background resolver thread (lines ~395–401), replace
  the `loop { if asset_loader::is_loaded() { break } sleep }` readiness gate with a
  wait on `texture_resolver::is_available()`. Rationale: the original gate meant
  "custom ARC textures are now registered"; the correct general signal for "the
  texture system can resolve names" is the resolver being available. Stock and
  LayeredFS-injected textures don't need an ARC load at all.
- Remove the now-unused `asset_loader` from the `use crate::services::{…}` there.

### `src/mods/folder_expansion.rs` — delete dead ARC branch
- Remove the `if let Some(ref arc_path) = config.arc_path { register_arc … }` block
  in `enable()` (lines ~1424–1428).
- Remove the `pub arc_path: Option<String>` field from `FolderConfig` (line ~137).
- Remove `use crate::services::asset_loader;` (line ~14).

### `src/mods/series_expansion.rs` — migrate to LayeredFS
- Remove the `register_arc` block in `enable()` and the
  `use crate::services::asset_loader;` import.
- Remove `pub arc_path: Option<String>` from `SeriesConfig`.
- In `enable()`, call a new `generate_label_atlases(config)` helper, then
  `mod_paths::init_mod_paths()` to rescan.

**CORRECTION (post first cabinet test).** The original plan assumed raw
`sefi_version_{key}.png` PNGs dropped into the IFS mod folder would be served by
the auto-inject path (`inject_new_textures`). A verbose deploy disproved this:
the PNGs *were* injected ("injected 2 new textures" in the log), but
`inject_new_textures` builds each PNG as its own 1:1 atlas with full-coverage UV
(0,0)–(1,1). The `filter_item` BM2D MovieClip applies the label by name and
expects it at a specific atlas UV slot, so the full-coverage texture renders
wrong/invisible.

The fix mirrors what `custom_options` and `folder_expansion` already do for this
exact IFS (`select_music_option_v3.ifs`): use `atlas_cloner::generate_cloned_atlases`
to clone a donor `sefi_version_*` slot (donor = `sefi_version_world`) for each
custom label, compositing the author's PNG at the donor's pixel rect and emitting
a `texturelist.merged.xml`. LayeredFS's `merge_xmls` combines all mods'
`.merged.xml` files for the IFS, so series and custom_options coexist — **provided
each uses a distinct `custom_atlas_prefix`** (series uses `cser_version`,
custom_options uses `copt_mods`). The cache filename is `md5(prefix_NNN)`, so a
shared prefix would overwrite the other mod's atlas blob.

Author workflow is unchanged from the README: drop `sefi_version_{texture_name}.png`
into `data_mods/custom_series/select_music_option_v3_ifs/tex/`. The mod reads it
from there at enable time and composites it into the cloned atlas.

### `src/mods/hello_world.rs` — remove image loading
- Remove the two `create_image_widget` calls and any logo/banner state fields,
  animation, and the `LOGO_TEXTURE`/`BANNER_TEXTURE` consts.
- Keep the text widget(s) and bounce animation.
- Do **not** touch `widgets/image_widget.rs` or `create_image_widget`.

### Docs + assets
- `README.md`: rewrite the "Custom Series" setup to the `data_mods` PNG workflow
  (drop ARC packaging steps + `arc_path`). Remove/replace the "Custom Textures"
  section that describes `custom_mod.arc` + the ARC loader. Note `build_ddr_package`
  is now optional/legacy.
- `mod-config.json`: remove the `series_expansion.arc_path` key.
- `scripts/build_ddr_package/README.md`: add a note that ARC packs are no longer
  required for textures — LayeredFS `data_mods` is the supported path; this tool
  remains for optional build-time pre-baking.
- Delete the checked-in `custom_mod.arc` asset reference if present under `assets/`.

## Risk areas

- **R-A (series IFS-in-ARC injection).** `select_music_option_v3.ifs` lives inside
  an ARC, so the texturelist read rides the recently added arc-inner-IFS demangler
  path. Highest-uncertainty item: must verify on cabinet that the texturelist open
  is actually intercepted and injection fires. If it doesn't, the fallback is the
  folder-style approach (explicit atlas handling via `atlas_cloner`), which is
  known to work for `select_music_folder_v3`. Diagnostic: enable `layeredfs.verbose`
  and watch for `parse_texturelist` + `inject` log lines for the option IFS.
- **R-B (`arc_file_open` derivation).** Confirm nothing reads it before deleting.
- **R-C (image-widget readiness gate).** Switching the gate from
  `asset_loader::is_loaded()` to `texture_resolver::is_available()` changes timing.
  Low risk since hello_world's images are being removed (the only current caller),
  but the loop must still terminate cleanly and not spin forever — keep the
  existing `is_available()` early-return that already exists below the gate.

## Success criteria

1. `cargo check --target x86_64-pc-windows-msvc` clean (no unused-import / dead
   warnings from the removals).
2. On cabinet: custom VERSION filter labels (`WORLD RUBY`, `WORLD SAPPHIRE`) render
   from `data_mods` with no ARC in `data/arc/bm2d/`.
3. Custom folders still render (folder_expansion unaffected).
4. hello_world (when enabled) shows the bouncing text demo, no image, no crash.
5. No `arc_load not resolved` / `arc_file_open` warnings in the log; init completes.
