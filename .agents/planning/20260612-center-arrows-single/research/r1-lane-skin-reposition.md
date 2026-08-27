# R1 — Single lane-skin reposition feasibility (FEASIBLE — preferred path viable)

## Question (from Q4)
Can we keep the **single** lane skin and reposition it to center, instead of forcing the
game's centered **double** lane skin?

## How the lane skin is laid out (builder `FUN_18006c230`)

The lane skin is bound differently from the HUD elements:

- **Lane skin:** resolved via `FUN_18021bae0(parent, "%dp_lane_usr")` → a layer/handle,
  then `FUN_18021c170(laneLayer, sideObj, &laneNameStr)` loads the AFP MovieClip into the
  lane layer (`afp_mc_load_movie` + `afp_mc_set_param(iVar1, 0x101e, 1)`). The lane name
  string is `lane_%s_%s` (`pcVar11` = `"single"`/`"double"`, + `normal`/`reverse`).
- **HUD elements** (arrow/judge/combo/…): the builder *reads* a position from the same AFP
  layer via `FUN_18021c460` (`afp_mc_get_param(layer, 0x1008, &xy)`), optionally adjusts it,
  then *stores* it via `FUN_18006f5d0(perSideParent, name, &coord)`.

Key point: the element coords that `FUN_18006f5d0` stores are **derived from the lane
layer's AFP position** (param `0x1008`). The lane layer's transform is the upstream source of
truth for "where the lane is."

## Feasibility: YES — the lane is an AFP layer with a settable position

`afp_mc_get_param(layer, 0x1008, ...)` reads the layer's position; the engine has the
matching setter. **The codebase already wraps these** (`src/services/bm2d_api.rs`):
- `bm2d_api::set_position(layer_id, x, y)` → `afp_layer_set_position`
- `bm2d_api::mc_set_param(mc_id, param, value)` → `afp_mc_set_param`
- `bm2d_api::mc_op(mc_id, op, value)` → `afp_mc_op`

So we can set the single lane layer's X to the centered value directly, keeping the
`"single"` lane art. This makes the **preferred** Q4 approach viable.

## Two coherent strategies (decide in design; settle A↔B by one cabinet test)

**Strategy A — element-only rewrite via the `FUN_18006f5d0` hook (cheapest).**
Rewrite the element X-coords (arrow_raw/arrow/freeze_judge/judge/combo/fast_slow/filter/
score_compare) in the setter hook. IF the visible lane backdrop's position is driven by
those same stored coords, this centers everything including the lane, no separate AFP write.
**Unknown:** whether the lane backdrop tracks stored coords or the raw AFP layer transform —
**verify on a diagnostic deploy** (center elements only; see if the lane art follows).

**Strategy B — element rewrite + explicit lane-layer reposition (robust; preferred default).**
Also set the single lane AFP layer's X to the same center via `bm2d_api::set_position`. The
lane layer handle is obtainable at builder time (the `FUN_18021bae0("%dp_lane_usr")` result);
capturing it may require reading the per-side lane layer id (e.g. a small hook on the lane
bind, or reading it off the per-side object). Guarantees single lane art + receptors are
co-located regardless of how the backdrop is driven.

**Fallback (Q4 fallback) — force double lane.** Intercept the lane-name selection so 1P uses
`double_lane_usr` + `lane_..._double` (centered), plus the element X rewrite. Known-good (the
32-bit hack shipped it) but uses the double-lane graphic instead of the single.

## Static trace of the READ side (UPDATE — Strategy A confirmed by static RE)

Traced who consumes the named coord map the setter (`FUN_18006f5d0` → `FUN_18006fb40`) writes
into, at `perSideParent + 0x28` (a std::map keyed by name string; reader = `FUN_18006f290` /
value-getter `FUN_18006f6b0`). Every per-element renderer follows the **same pattern**: read
the element's coord by name, then push `coord[0]/coord[1]` into the element's AFP layer via the
layer vtable `setPositionXY` at slot **+0x38** (and scale via +0xC0). Confirmed on three
representative renderers:

- **`FUN_180065f10`** (`dance_bpm`): `puVar5 = getCoord("bpm"); layer.setPositionXY(puVar5[0],
  puVar5[1]); layer.setScale(puVar5[4],puVar5[5])`.
- **`FUN_18006a980`** (`filter`): reads `perSideParent("filter")`, `layer.setPositionXY(coord[0],
  coord[1])`.
- **`FUN_180078b40`** (shock-lane / arrow overlay): `piVar11 = getCoord("arrow");
  Xanchor = arrow.coord[0] + const;` then each shock-lane/panel layer is positioned at
  `setPositionXY(perPanelOffset + Xanchor, Y)`. The lane-overlay geometry is anchored to the
  stored `"arrow"` X.

**Conclusion:** the stored named coords ARE the render-time source of truth for element/receptor
position — the renderers copy `coord[0]` into the AFP transform every build. So rewriting
`coord[0]` in the setter hook (**Strategy A**) moves the receptors and all lane-relative
elements. No AFP-layer poke needed for those.

### Residual caveat (narrowed)
The one layer NOT observed re-reading a coord is the **static lane backdrop frame** itself
(`%dp_lane_usr` MovieClip bound via `FUN_18021c170`), which is template-bound. Two reasons this
is low-risk: (1) the original 32-bit hack centered via these same element/`arrow_raw` X writes
and shipped looking correct, implying the backdrop either tracks the receptors or its residual
offset is visually acceptable; (2) if the static frame looks off-center at deploy, Strategy B
(reposition the lane AFP layer via `bm2d_api::set_position`) remains available as a targeted
add-on. The deploy check is now narrowed from "does anything center?" to "does the static lane
frame graphic look right?" — a cosmetic confirmation, not a mechanism unknown.

## Recommendation (UPDATED)
**Ship Strategy A** (pure `FUN_18006f5d0` X-rewrite) — static RE confirms it drives the rendered
positions of the receptors and all lane-relative elements. Treat the static lane-backdrop frame
as a **cosmetic deploy check only**; if it reads off-center, add the targeted **Strategy B**
lane-layer reposition. **Force-double** remains the last-resort fallback. The Step 8 A↔B
"decision point" is downgraded to a cosmetic confirmation.

## Reference
- `src/services/bm2d_api.rs` (`set_position`, `mc_set_param`, `mc_op`, vtable accessors).
- `src/services/series_filter_scroll.rs` (precedent: hooking BM2D `set_position` vtable slot
  0x30 to inject a position offset — working pattern for nudging AFP layer positions).
- `docs/hex_edit_porting.md` → Hack 2.
