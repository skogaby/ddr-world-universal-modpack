# Rough Idea — Native Customize Persistence (single source of truth)

Make the game's native `<customize>` profile fields the single source of truth
for the WebUI customization options, eliminating the duplicative
`mod_customize_*` round-trip.

- The DLL keeps **sending** `mod_customize_*` children on profile save (the
  game has no native save path for these selections — stock setups only set
  them via Konami's web portal).
- The DLL stops **loading** them back: the server write-throughs the saved
  values into the native `cust_*` profile fields, the game's own `<customize>`
  load path applies them to the `ddr::player::Customize` object, and the DLL
  seeds its in-game options menu state by reading that `Customize` object at
  SONG_SELECT (scene 25) entry — the earliest point the options modal can be
  summoned.
- JSON (offline) persistence is dropped for the WebUI options entirely. Other
  custom options (autoplay, premium-free, power-user-statistics, …) keep their
  existing full round-trip (network echo + JSON) — they have no native game
  fields to piggyback on.
- bemani-buddy side: rename the opaque `cust_<cat>_<pat>` columns to semantic
  names, drop the inert `cust_3_0`, drop the now-redundant
  `opt_mod_customize_*` columns, their protocol fields, and their load echo
  (keeping `opt_mod_autoplay` and the other mod options' round-trip untouched).
  Save path writes the incoming `mod_customize_*` values directly into the
  renamed native columns.
- No backward-compatibility constraints — both repos are closed-testing and
  co-maintained by the same person. Other private-server operators will be
  helped to adopt the same mapping (the RE research doc's "Server-Side
  Persistence Mapping" section documents the contract).

## Why

The current design has the server echo `mod_customize_*` fields on load and
the DLL overwrite the game's `Customize` object in memory — a kludge that
creates two writers for the same data. Servers with real web UIs legitimately
drive the native `cust_*` fields, and the DLL's overwrite fights them. The RE
work (see `docs/player_customization_system_research.md`) fully decoded the
`(category, key, pattern)` wire mapping, so the native path can now carry the
values end-to-end and the DLL only needs to fill the one genuine gap: the
game-to-server save direction.

## Division of labor

- DLL-side changes: implemented directly in this session's repo
  (ddr-world-universal-modpack).
- bemani-buddy changes: documented as an implementation brief in that repo's
  `doc/` (superseding/updating `doc/ddr_world_customize_column_rename.md`),
  then delegated to a Fable 5 xhigh subagent, overseen from this session.
  A local MySQL dev DB is available (`config.toml`) for running migrations and
  regenerating the sqlx offline cache.
