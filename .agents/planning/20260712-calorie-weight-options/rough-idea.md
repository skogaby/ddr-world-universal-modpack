# Rough Idea

Extend the **WebUI Options** mod to add two in-game options that Konami's web UI
normally owns exclusively:

- **player WEIGHT** — the body-weight value fed into the in-game calorie calc.
- **DISPLAY BURNED CALORIES** (`is_disp_weight`) — the on/off toggle for showing
  burned calories in-game.

Mirror the existing cosmetic-customize pattern:

- Register both as `custom_options` (`PersistMode::SaveOnly`).
- Write them straight into `PlayerWork` on change — `+0x24` weight (s32),
  `+0x28` is_disp_weight (u8/bool).
- Seed the menu by reading those offsets at SONG_SELECT (scene 25) entry.
- Inject them on `playerdata_save` so a backend (bemani-buddy) maps them to its
  native `common.weight` / `common.is_disp_weight` columns.

Load stays game-native: the server's `<common>` block → `ReflectPlayerWork` →
PlayerWork, exactly as today. The DLL adds only the save direction the game lacks.

## Reference

Full RE findings (memory offsets, wire format, reflect evidence, calorie formula,
cross-version notes, signature basis) are documented in
[`docs/calorie_weight_profile_research.md`](../../../docs/calorie_weight_profile_research.md).
