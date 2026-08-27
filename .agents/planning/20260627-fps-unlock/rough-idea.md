# Rough Idea — FPS Unlock

## Concept

Port / implement the **FPS unlock** hack (a.k.a. "Fullscreen FPS Target") into the
DDR World hook DLL. This is **Hack 5** in `docs/hex_edit_porting.md`.

The original 32-bit `patches.js` exposed a union/numeric patch that overrides the
game's fullscreen frame-rate target (default `0x3C` = 60, cabinet-selected to 75 on
`MachineType == 1`) with one of: **60 / 120 / 144 / 165 / 240 / 360** FPS. The value
is a single imm32 in the main app-init function.

## Why it's wanted

Raising the display target makes gameplay arrow scroll smooth/correct at higher
refresh rates (the engine is fundamentally delta-time based: the global frame delta
`DAT_1806ea714` is clamped and read by ~120 functions, so dt-scaled motion stays
correct). The desirable case is **smooth high-FPS gameplay**.

## The known catch (from prior research)

The engine's **logic tick is driven 1:1 by the render loop**, and some animation
paths advance **per-tick by a fixed step** (frame-counted) rather than multiplying by
`DAT_1806ea714` — menu scrollers, some AFP timeline advances, etc. Raising the global
display target therefore makes those frame-counted animations run **too fast** in
menus/selection/attract. A pure byte patch can't fix this because the value is global.

Prior research's recommended design: a **hook-DLL mod that varies the display target
by scene** via the existing `scene_manager` — high target in gameplay scenes, drop
back to 60 in menu/selection/attract scenes — rather than a single static global value.

## Scope for THIS effort

- **Focus strictly on the FPS unlock (Hack 5).** Do NOT touch the other hacks
  (announcer mute, center arrows, hide bottom text, timing offsets, preset select).
- Center-arrows and timing-offsets were already shipped from the same research
  effort (`.agents/planning/20260612-center-arrows-single`,
  `.agents/planning/20260626-timing-offsets`).

## Re-verification mandate

The original `docs/hex_edit_porting.md` research was produced in an earlier session
with an **older model** and a **broader scope** (all of Hacks 1–6 at once), so some
findings may be imprecise, incomplete, or have missed details specific to the FPS
path. This session MUST **re-verify the RE findings fresh** against the binary
(Ghidra / live memory) before designing on top of them — specifically:

- The app-init function (`FUN_1800020f0` per prior notes) and the imm32 site
  (`C7 44 24 ?? 3C 00 00 00 75 08 C7 44 24 ?? 4B 00 00 00`).
- Where the chosen display target actually flows (struct field `+0x14` →
  `FUN_1801f0030` → present/refresh path) and whether it is re-read each frame (the
  precondition for a live per-scene re-write vs. a boot-time-only static value).
- The delta-time clamp `CAP = DAT_18045f114 / 59.94` and the global delta
  `DAT_1806ea714`, to confirm the side-effect analysis.
- Anything the prior pass may have missed that affects whether per-scene switching is
  feasible as designed.

## Open design questions (to resolve in idea-honing)

- Static global value vs. per-scene auto-switch vs. both (config-selectable)?
- Which scenes count as "high FPS" vs. "force 60"?
- Configurable target value(s) and where (mod-config.json `fps_unlock` section,
  mod-overlay scalar/enum row, or both)?
- Master on/off + graceful degradation behavior, consistent with the other mods.

> Sourced from `docs/hex_edit_porting.md` → "Hack 5 — Fullscreen FPS Target".
