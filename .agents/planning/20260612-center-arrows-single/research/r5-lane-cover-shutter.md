# R5 — Lane cover (ShutterActor) centering

## Status: RESOLVED — no separate hook needed (superseded)

Initial fear (below) was that the lane cover and the end-of-song FullcomboActor needed their
own AFP-layer hooks. Live testing + RE showed otherwise: **both position via coords in the
LayoutActor named-coord map** — the same map the `hud_layout_setter` hook writes. The lane
cover centered automatically once the lane elements shifted. The `FullcomboActor`
(onCreate `FUN_1800690f0`) reads the `"fullcombo"` coord via `FUN_18006e300` → `setPositionXY`,
so adding `"fullcombo"` to `TARGET_KEYS` centered it through the existing shift. No
`ShutterActor`/AFP-path hook was required.

Lesson: a `cover_usr`/`fullcombo_usr` `afp_layer_mc_refer` lookup in an actor's onCreate does
NOT mean the actor's *screen position* is set there — these actors pull their position from
the HUD coord map (`FUN_18006e300`) and only use the AFP refer to bind the clip. Check for a
trailing `getCoord(name) → setPositionXY` before assuming a separate positioning path.

---

## Original (now-superseded) analysis

## Why the current hook misses it

The lane cover is a **`ShutterActor`** (`sequence::common::shutter`), not part of the
gameplay HUD `LayoutActor`'s named-coord map. Its layout function is **`FUN_180067020`**
(20260526), which positions cover layers by:

```c
// for each cover sub-layer ("%s_%04d"):
for (iVar1 = afp_layer_mc_refer(layer, "cover_usr"); iVar1 >= 0;
     iVar1 = afp_mc_traversal(iVar1, 6)) {
    afp_mc_load_movie(iVar1, ...);     // Ordinal_112
}
```

i.e. it resolves the `cover_usr` AFP layer and manipulates it directly via libafp ordinals —
it never calls the coord-map setter (`FUN_18006e220`) that our mod hooks. So our `coord[0]`
shift never reaches the cover.

Related strings/anchors (20260526):
- `cover_usr` @ `0x1803604e8` (xref'd from `FUN_180067020`)
- `lane_cover_%s` @ `0x1803604b8`, `hidden_cover_%s` @ `0x180360498`, `sudden_cover_%s` @ `0x1803604a8`
- `ShutterActor` @ `0x18035cf00`; `ddr::player::Option::SetLaneCover` @ `0x1803866d8`
- Layout fn: `FUN_180067020`; sibling `FUN_180066a40` (refs `lane_cover_%s`)

## What a fix needs (when picked up)

1. Determine how `cover_usr`'s AFP layer X is set, and whether it's per-side (the function
   loops over two side-collections at `*root+0xC8` and `*root+0xE8`).
2. Grab the cover's live X for P1 and P2 in a CE-frozen 2P demo (same method as the main
   elements — confirm the spacing matches the 719 the HUD elements use, or capture the
   cover's own delta).
3. Hook the ShutterActor layout (or its AFP `set_position`) and apply the same signed
   per-side shift (`±LANE_SHIFT`, or the cover's own derived delta), gated identically
   (`single_player && active_side && option_enabled[side]`).
4. Reuse the same player-array detection + option already in `center_arrows_single.rs`.

## Note
If the cover's P1↔P2 spacing equals the HUD elements' 719, the existing `LANE_SHIFT=360`
constant applies directly and this is purely "add a second hook on the cover path." Verify
before assuming.
