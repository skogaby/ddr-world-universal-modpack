# Task: Remove Music Wheel Song Length X/Y overlay rows + STOCK_RIBBONS fix

## Description
Delete the Music Wheel Song Length mod's two live-tuning overlay rows and
their save-on-change wiring (the config keys remain honored at enable), and
add the two stock ribbon textures missing from asset_gen's STOCK_RIBBONS list
(kills a pre-existing atlas-rebuild WARN ×6).

## Background
The `mwsl-offset-x`/`mwsl-offset-y` rows were dev-tuning aids (maintainer
decision, plan Step 9): `register_offset_rows()` at
`src/mods/music_wheel_song_length.rs:316–349` registers them via
`mod_menu::register_scalar_row` with `set_offset()` (lines ~358–391) poking
the live SpriteLayer and persisting the whole section via
`save_json_key("music_wheel_song_length", …)`. The config READS live in
`init()` (~lines 255–276) and must stay — operators keep tuning via
mod-config.json. Separately, `asset_gen.rs:130` `STOCK_RIBBONS = ["seop_op_on",
"seop_op_off"]` omits `seop_op_left`/`seop_op_right`, so atlas-REBUILD boots
log 6 `get_bitmap_info … can not find`-class WARNs for training_mode's
progress-position values (survey + Step 5 note).

## Technical Requirements
1. Remove `register_offset_rows()` and its call from `enable()`; remove the
   `set_offset` change-callback machinery and the `save_json_key` wiring; if
   the mod's `disable()` removes rows via `remove_rows_for`, drop the now-dead
   keys from that list.
2. KEEP: the `init()`-time config reads (`offset_x`, `offset_y`, `spacing`,
   `scale` with defaults), the Runtime fields, and the per-frame layout that
   consumes them — offsets from mod-config.json must still apply.
3. Remove any now-unused imports/helpers in music_wheel_song_length.rs
   (compile-clean, no orphaned code).
4. Add `"seop_op_left"` and `"seop_op_right"` to
   `asset_gen::STOCK_RIBBONS` (they are stock atlas members — the
   conservative-list doc comment's criteria are met; this stops the redundant
   donor clones and the rebuild WARNs).
5. Update the mod's module doc comment (it documents the "live-tunable from
   the mod-menu overlay scalar rows" behavior — now config-only).

## Dependencies
- task-01 may touch neighboring files but not these sites — order-independent.

## Implementation Approach
1. Excise the rows + callback; verify `cargo check` finds all dead code.
2. STOCK_RIBBONS one-line addition.
3. Cabinet validation: boot with an atlas REBUILD (bump a texture or clear
   the atlas cache) → the 6 seop_op_left/right WARNs are gone; the overlay's
   GLOBAL SETTINGS tab no longer lists Length X/Y rows; the LENGTH readout
   still honors config offsets at song select.

## Acceptance Criteria

1. **Rows gone from the overlay**
   - Given the overlay menu's GLOBAL SETTINGS tab with the mod enabled
   - When browsing the Music Wheel Song Length group
   - Then no Length X/Y Offset rows appear (the mod's toggle remains)

2. **Config offsets still honored**
   - Given `music_wheel_song_length.offset_x/offset_y` set in mod-config.json
   - When the game boots to song select
   - Then the LENGTH readout renders at the configured offsets

3. **Rebuild WARNs gone**
   - Given a boot that rebuilds the options atlas
   - When the log is harvested
   - Then zero seop_op_left/seop_op_right lookup WARNs appear

4. **No orphaned code**
   - Given the final diff
   - When `cargo check` runs and the file is reviewed
   - Then no unused fns/imports remain and the module docs match behavior

5. **Gates green**
   - Given the readiness gates
   - When check → fmt → build.sh + the three harnesses run
   - Then all pass

## Metadata
- **Complexity**: Low
- **Labels**: rust, mod-menu, cleanup, textures
- **Required Skills**: repo conventions (AGENTS.md)
