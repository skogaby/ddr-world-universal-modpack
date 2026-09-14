# Rough Idea: Auto-Updater

Captured 2026-09-13 from the maintainer's request, verbatim in substance.

A **standalone application** that downloads and installs the newest release of the
modpack, based on the latest version published at
https://github.com/skogaby/ddr-world-universal-modpack/releases.

## Invocation

The updater is invoked from the user's `gamestart.bat` in such a way that spice2x /
DDR **won't start until the updater program runs and finishes**.

## What the update covers

The release archive is always attached to the latest GitHub release and is built by
`scripts/build_release_archive.sh`. The updater must:

- Download the archive and unzip it.
- Install / update **in place**: `data_mods/` and `ddr_world_hook.dll`.
- Handle `mod-config.json` updates.
- Handle `judgement_offsets.csv` updates.

## `mod-config.json` merge rules

Keep it simple for end users who may already have custom configurations:

- Any key in the incoming (release) `mod-config.json` that is **not present** in the
  user's existing file gets **copied over** from the update.
- If a key exists in **both**, **prefer the existing (user) value**, preserving the
  user's configuration.
- `option_menu_settings` needs special handling: do not disturb the user's custom
  option ordering, but if the release adds a **new option** the user's local config
  lacks, insert it **under whatever header it sits under in the release copy** of
  `mod-config.json`.

## `judgement_offsets.csv` merge rules

- Only **insert songs that are missing** from the user's local file.
- Never override user preferences for songs they have set their own offsets for.
