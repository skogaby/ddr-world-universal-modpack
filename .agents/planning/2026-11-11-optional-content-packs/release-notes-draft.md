# Release notes draft — the first release with optional packs

For the maintainer to edit into the GitHub release body. Only the parts about packs and
the updater; the rest of the release's changes are not covered here.

## Dancers and stages from other DDR releases are now optional packs

The Background Dancers mod has gained dancers and stages ported from other DDR
releases — 777 models in 23 sources. Together they are about 1 GB to download and
3.4 GB on disk, so they no longer ride in the release zip. Each source is a **pack**
you choose:

- Every time the updater runs it now shows how many packs you have and waits 5 seconds:
  **press Enter** to open a checklist and pick packs; do nothing and the game starts.
- Double-click `ddr_world_hook_updater.exe` (or run it with `--packs`) to open the
  checklist directly.
- No keyboard on the cabinet? `ddr_world_hook_updater.exe --packs-install all` — or
  `none`, or a list of ids from `--packs-list`.
- Installing by hand? Every pack is a zip on the
  [packs page](https://github.com/skogaby/ddr-world-universal-modpack-assets/releases/tag/packs);
  unzip the ones you want into the game folder.

The updater keeps the packs you chose up to date and repairs them if files go missing.
Models you added yourself are never touched.

## Things to know when updating from v1.4

- **The update takes two launches to show the pack prompt.** The first launch installs
  this release (with the updater you already have); the prompt appears from the second.
- **The seven custom models v1.4 shipped moved.** Big Smoke, Carl Johnson, Hatsune Miku,
  Kasane Teto, Peter Griffin, Griffin House and Grove Street now live under
  `data_mods/custom_models/{dancers,stages}/Custom/` as single `.arc` files. The updater
  removes the old folders it installed.
- **If you edited one of those v1.4 model folders**, the updater keeps your folder, and
  the game will then find two models with the same name and use one of them (it logs a
  warning). Delete or rename your copy to choose.
- **Every launch is 5 seconds longer** when run from `gamestart.bat` on a machine with a
  console, because of the pack prompt.
- The first launch after choosing many packs waits for their download before the game
  starts. A download that is cut off resumes on the next launch.
- New option rows are placed in the section they belong to; if you re-ordered your
  options, your order is kept.

## For people who build from source

Ported models are now in a separate repository,
[ddr-world-universal-modpack-assets](https://github.com/skogaby/ddr-world-universal-modpack-assets).
See "For Developers" in the README.
