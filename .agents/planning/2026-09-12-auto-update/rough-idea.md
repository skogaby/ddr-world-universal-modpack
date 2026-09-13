# Rough Idea: Auto-Update

Captured 2026-09-12 from the maintainer's request.

The modpack should automatically update itself during bootup based on the latest
release from the project's GitHub releases page
(https://github.com/skogaby/ddr-world-universal-modpack/releases).

Requirements as stated:

- The update check happens **after** all mods are done installing and initializing,
  so nothing crucial is blocked — the vast majority of boots will not need an update.
- If an update is available, the user sees a **progress bar at the bottom of the
  screen** and a label reading "DDR World Universal Modpack - Update In Progress".
- Once the update has finished downloading and installing, the user is **prompted to
  restart the game**.
- The update process handles:
  - unzipping the release archive (always attached to the latest GitHub release,
    built by `scripts/build_release_archive.sh`),
  - installing/updating in place `data_mods/` and `ddr_world_hook.dll`,
  - `mod-config.json` updates.
- `mod-config.json` merge rule (simple for end users with custom configs): any key in
  the incoming config that is **not** present in the existing config is copied over;
  where a key exists in both, the **existing** value wins (preserve the user's
  configuration).
