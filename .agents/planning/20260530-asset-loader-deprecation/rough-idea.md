# Rough Idea — Deprecate `asset_loader::register_arc`, move textures to LayeredFS

## Motivation

Early in the project, three mods shipped custom textures by:

1. Building a Konami `.arc` file (containing an IFS of textures) at build time
   via `scripts/build_ddr_package`.
2. Dropping it into `data/arc/bm2d/` next to the game's own archives.
3. Calling `asset_loader::register_arc(path)`, which calls the game's `arc_load`
   from the render thread to register the ARC's textures into the BM2D system so
   they resolve by bare name.

This works but is clunky for both mod authors (must run the Python packer, repack
on every texture change) and end users (must place an ARC inside the game's data
tree). It also predates LayeredFS.

Later, LayeredFS gave us transparent file replacement / net-new texture injection
out of `data_mods/` — no ARC packing, no touching `data/`. Authors drop PNGs into
a `_ifs/tex/` folder and they're converted, atlased, and served at runtime.

## Goal

Retire `asset_loader::register_arc` (and the `arc_load` signature) entirely. Move
the two real consumers onto LayeredFS texture injection, and drop the demo mod's
image loading.

## The three callers

- **folder_expansion** — already fully on LayeredFS (atlas_cloner pipeline). Its
  `register_arc`/`arc_path` is vestigial dead code (config defaults to `None`,
  not in README or `mod-config.json`). → Delete the dead branch.
- **series_expansion** — labels are `sefi_version_{key}`, which belong in the
  stock `select_music_option_v3.arc/.ifs` (same ARC/IFS family folder_expansion
  injects into). The game already opens that IFS, so LayeredFS net-new texture
  injection makes them BM2D-resolvable with no new service code. → Migrate.
- **hello_world** — demo mod, off by default. Its `paseli_logo`/`build_test_banner`
  textures correspond to no stock asset, so there's no host IFS to ride. Per
  maintainer decision: drop the image loading for now, keep the text-bounce demo.
  (A truly free-floating "virtual ARC" would require a runtime IFS + kbin writer,
  which we don't have and won't build for a demo.)

## Key constraint discovered during exploration

LayeredFS is purely **reactive** — it only acts when the *game* opens a file.
Net-new texture names enter BM2D one of two ways:
  1. Ride a host IFS the game already opens (folder/series — no writer needed).
  2. Actively `arc_load` a real container (today's asset_loader — needs real
     on-disk IFS bytes, or a runtime IFS writer to fake them).
The goal is to kill path #2. Series rides path #1; hello_world's demo textures
have no host IFS so they're simply dropped.

## Scope decisions (maintainer)

- Keep `image_widget.rs` and `widget_renderer::create_image_widget` intact even
  though hello_world is the only current caller.
- Keep `scripts/build_ddr_package` as an optional/legacy build-time tool; note in
  its README that ARC packs are no longer required.
- Capture this as a short PDD planning doc before implementing.
