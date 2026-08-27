# Rough Idea — Center Arrows for Single Player

A new mod in the DDR World hook DLL that centers the single-player playfield, ported
from the 32-bit "center arrows" hex hack (fully reverse-engineered in
`docs/hex_edit_porting.md`, Hack 2). Target: the 64-bit builds (20260324 primary,
20260526 verified).

## What the hack does (32-bit original)

`FUN_1005a180` (64-bit `FUN_18006c230`) is the gameplay HUD/lane layout builder. It
iterates both player sides and positions every HUD element by name via a named-layout
setter `FUN_1005bcd0` / 64-bit `FUN_18006f5d0(parent, name, &coord6)`, where the coord
payload's `dword[0]` = X, `dword[1]` = Y. The original 6-patch hack centers single-player
play by:

1. Forcing the centered ("double") lane geometry for 1P instead of the side-offset 1P
   geometry (`double_lane_usr` instead of `%dp_lane_usr`).
2. Forcing the `lane_%s_%s` selector to `"double"` (centered lane skin).
3. Hard-setting the X coordinate of the `arrow`/`arrow_raw` and `freeze_judge` elements
   to 495 (screen center) via code caves.

## Recommended 64-bit approach (from the RE doc)

Rather than byte patches + code caves, implement as a Rust hook:
- **Post-hook `FUN_18006f5d0`** (the named-layout setter, AOB-resolved). When the current
  context is single-player and `name ∈ {arrow_raw, arrow, freeze_judge}` (and optionally
  judge/combo/etc. for full HUD centering), rewrite `coord[0]` (X) to the centered value.
- **Force the `double_lane_usr` lane branch** + the `lane_%s_%s` → `"double"` selector for
  1P, so the lane skin renders centered.

The 32-bit centered X was 495; DDR World's playfield logical coordinate space is believed
consistent across the 32→64 transition, but the value should be confirmed against what
`double_lane_usr` itself resolves to rather than hardcoded blindly.

## Reference

- `docs/hex_edit_porting.md` → "Hack 2 — Center arrows for single player" (full 32-bit
  patch table, mechanism, and 64-bit anatomy with `FUN_18006c230` / `FUN_18006f5d0`).
- Existing mod patterns: `src/mods/mod_trait.rs`, and binary-patch/hook mods like
  `timer_freeze`, `real_speed_fix`, plus shared-dispatcher discipline in CLAUDE.md.
