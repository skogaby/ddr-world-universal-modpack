# Implementation Plan — Asset Loader Deprecation

Each step ends in `cargo check --target x86_64-pc-windows-msvc`. The final
acceptance is a cabinet deploy (no unit-test harness). Steps are ordered so the
tree compiles after each one.

## Step 1 — Remove dead ARC branch from folder_expansion
- Delete the `arc_path` `register_arc` block in `enable()`.
- Delete `FolderConfig::arc_path`.
- Remove `use crate::services::asset_loader;`.
- **Check:** compiles; folder_expansion logic otherwise untouched.

## Step 2 — Migrate series_expansion to LayeredFS
- Delete the `register_arc` block in `enable()` and the `asset_loader` import.
- Delete `SeriesExpansionConfig::arc_path`.
- Add `mod_paths::init_mod_paths()` call at the end of `enable()`'s asset setup.
- **Check:** compiles. (Runtime verification deferred to Step 7.)

## Step 3 — Remove image loading from hello_world
- Strip the two `create_image_widget` calls, image state, animation, and the
  `LOGO_TEXTURE` / `BANNER_TEXTURE` consts.
- Keep text-bounce demo.
- **Check:** compiles; no references to removed consts remain.

## Step 4 — Decouple widget_renderer from asset_loader
- Remove `asset_loader::load_arcs()` call in `render_function_hook` (keep font
  capture).
- Replace the `asset_loader::is_loaded()` readiness gate in the
  `create_image_widget` resolver thread with `texture_resolver::is_available()`.
- Remove the `asset_loader` import in that scope.
- **Check:** compiles; `create_image_widget` still present and self-contained.

## Step 5 — Delete the asset_loader service
- Delete `src/services/asset_loader.rs`.
- Remove `pub mod asset_loader;` and its header doc bullet in `services/mod.rs`.
- **Check:** compiles; grep confirms zero `asset_loader` references remain in src.

## Step 6 — Remove the arc_load signature + derivation
- Confirm via grep that `arc_file_open` has no remaining consumer.
- Remove the `arc_load` `SignatureDefinition` and the `arc_file_open` derivation
  block in `signatures.rs`.
- Remove the `asset_loader::register_arc/init` calls + import + profiling tick in
  `lib.rs`.
- **Check:** compiles; grep confirms zero `arc_load` references remain in src.

## Step 7 — Docs, config, assets
- `mod-config.json`: drop `series_expansion.arc_path`.
- `README.md`: rewrite Custom Series to the `data_mods` workflow; remove/replace
  Custom Textures (ARC loader) section; note `build_ddr_package` is optional/legacy.
- `scripts/build_ddr_package/README.md`: add the "no longer required" note.
- Remove checked-in `custom_mod.arc` reference under `assets/` if present.
- Author a sample `data_mods` layout for the two configured custom series so the
  maintainer can drop PNGs in.
- **Check:** N/A (docs); re-grep for `custom_mod.arc` / `arc_path` across repo.

## Step 8 — Cabinet acceptance test
- Deploy. Enable `layeredfs.verbose`.
- Verify: custom series labels render from `data_mods`; folders still render;
  hello_world text demo runs; no `arc_load`/`arc_file_open` warnings.
- **Highest-risk watch (R-A):** confirm `parse_texturelist` + injection fire for
  `select_music_option_v3.ifs` (IFS-in-ARC). If not, fall back to the folder-style
  `atlas_cloner` injection path for series labels and re-test.

## Notes
- Steps 1–6 are each independently revertable.
- Step 2's runtime correctness is only proven at Step 8 — the migration is
  config/asset-shaped, so `cargo check` can't validate texture resolution.
