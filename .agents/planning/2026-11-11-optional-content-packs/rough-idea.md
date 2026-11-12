# Rough idea: optional content packs + release / distribution / updater restructuring

Source: maintainer request, 2026-11-11.

Today a release is a single archive (`scripts/build_release_archive.sh`) posted to the
newest GitHub release (https://github.com/skogaby/ddr-world-universal-modpack/releases),
and `ddr_world_hook_updater.exe` pulls that archive.

Since v1.4 (the last public release) `data_mods/` has grown by a little over 3 GB of
custom dancers and custom stages — ports of essentially every model and animation from
every DDR release. Not every user wants all of it; many will not even enable background
dancers.

Goals:

1. **Make the extra assets optional.** The updater lets the user choose whether to
   download them at all, and ideally *which* ones — e.g. the dancers for 3rd–5thMIX and
   the stages for SuperNOVA 1/2, but not WINX Club or Hottest Party.
2. This touches **release archive management, distribution, and the updater** itself.
3. **Never clobber local custom models.** Some users will create their own models with
   the Blender add-on, or install models distributed outside the modpack's official
   channels. The rewritten updater must leave those alone.
4. **Option rows the user lacks are inserted where the canonical `mod-config.json` puts
   them.** The header rows define sections. Example: the dancer / stage and dancer source /
   stage source rows belong at the end of *Playfield Styling Options*. A user's own
   re-ordering is never clobbered (re-ordering is an explicit feature); only rows missing
   entirely are inserted, by canonical section and row order — so new options are not
   dumped at the end of the options screen where users miss them.
