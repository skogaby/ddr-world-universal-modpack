# Research — Arrow / Receptor Render Path (Ghidra RE)

All addresses file-relative to `gamemdx.dll` base `0x180000000`.
Primary build: **20260616**; cross-checked on **20260324** where noted.
Verified live in Ghidra (project `DDRWorld_Ghidra`) on 2026-07-16.

## 1. Renderer class family (RTTI, 20260616)

| Class (RTTI string) | Addr of RTTI name | Role |
|---|---|---|
| `screen::ArrowSprite` | 0x18047e070 | Shared sprite base for lane renderers. vbptr at +0x80 (virtual-base reverse-flag lookup). |
| `screen::ArrowRenderer` | 0x18047df18 | Scrolling notes (the repo's `render_notes` owner). |
| `screen::SpotRenderer` | 0x18047dfc8 | Stationary receptor row. |
| `screen::JudgeEffectRenderer` | 0x18047dee0 | Receptor hit flash (sprite-based, distinct from the BM2D `dance_effect` clips). |
| `screen::GuidelineRenderer` | 0x18047df98 | Measure guideline. Does **not** draw through the shared quad fill (§2). |

Each active side owns its own renderer set (two of each in versus; doubles =
one ArrowRenderer/SpotRenderer spanning 8 panels). The renderer objects carry
**no play-side field** — side is baked in externally via position (posX) and
per-side option feeds.

## 2. Function map — the quad fill is a real call, not inlined

**KEY FINDING:** `render_sprite_final` (existing repo signature, 20260616 @
`0x180025900`) is reached by **real `CALL`s from every lane renderer**, not
just used inline. One detour there can observe/adjust every arrow, freeze,
shock, receptor, and hit-flash quad:

| Caller | 20260616 addr | Identity (from decompile) |
|---|---|---|
| `FUN_180026b00` = `render_notes` | call sites 0x180026ef5, 0x1800274f3 | Normal-arrow pass + shock "electric" overlay quad. (Existing AOB; unique on 20260324 too @ 0x180026050.) |
| `FUN_1800275b0` | 0x180027823 | Shock-arrow pass (per collected note; UV row selected by panel grouping). Called from `render_notes`. |
| `FUN_1800278a0` | 0x180027bb5, 0x180027d5b | Freeze-arrow pass: head/tail quads via fill; body quads via `FUN_180025860` (a tail-call wrapper that jumps straight into the fill). Reads `note+0x3C` length array, calls `get_offset_y` for tail offsets, reads per-note alphas from the collected record (+0x18/+0x1C/+0x20). |
| `FUN_180025e30` | 0x18002604d | **`SpotRenderer` draw** — receptor row. `numSpots = (mode@+0x98 == 1) ? 8 : 4`; per panel: `set_direction(i)` then fill. Shader ptr at spot+0xA0, atlas at +0x20. |
| `FUN_180029290` | 0x18002955e | **`JudgeEffectRenderer` draw** — expanding hit flash. Decompile shows the center-preserving grow math: `size2 = (size − 96) * 0.5; x = 96*dir − size2` (0.5 = `DAT_18035a79c`). |
| `FUN_180025860` | 0x1800258f1 | Thin wrapper → fill (used by freeze bodies). |
| `FUN_180026a50` | calls `render_notes` @ 0x180026aaf | `ArrowRenderer::onDraw` (state bracket + `set_render_state` + renderNotes). |

### Fill semantics (decompiled, 0x180025900)

```
fill(this /*ArrowSprite*/, out_quad /*0x34-byte ROTATESPRITE*/,
     x, y, w, h /*floats, XMM*/, uv /*float[4]*/, twist /*float*/,
     color /*COLOR4B*, 4 bytes RGBA*/)
```

- `x`, `y` are **lane-relative** (x = 96·dir; y = scroll offset from the
  receptor row). The fill adds `posX@this+0x30` / `posY@this+0x34` AFTER any
  reverse mirroring — so scaling `(x, y, w, h)` before the original runs
  scales the lane about its origin without touching screen placement.
- Appearance alpha (HIDDEN/SUDDEN/STEALTH): piecewise lerp on the incoming
  lane-relative `y`, fields at `this+0x6C` (startHeight), `+0x70` (endHeight),
  `+0x74` (startVal), `+0x78` (endVal).
- Reverse scroll: flag read via vbptr chain `*(*(this+0x80)+4) + this + 0x80`
  → negates `y` and `h` (sign-bit XOR with `DAT_18038eb10` = 0x80000000).
- Rotation: if `twist != 0` (compare vs `_DAT_180359698`), corners rotate
  about the **quad center** `(x + w/2 + posX, y + h/2 + posY)`.
- Final color: `quad.color.a = color.a * appearance_alpha` (byte × float),
  RGB copied through. **A per-side opacity multiplier composes naturally on
  the `color` argument's alpha byte.**
- The whole-lane tint lives at `this+0x64` (RGBA; alpha byte 3). The game
  itself animates that alpha (shock-damage flash, game-over fade), so
  composing at the fill is robust where overwriting +0x64 is not.

## 3. The note collector and its culling window

`render_notes`'s **first CALL target** is the per-pass note collector:
20260616 `FUN_180024b40`, 20260324 `FUN_1800240c0`. Called twice per frame
(shock pass, then normal pass). Iterates the judge Results vector and emits
0x28-stride records: `{dir i32@0, y1 f32@4, y2 f32@8, result*@0x10,
alpha1@0x18, alpha2@0x1C, alpha3@0x20}` (the three alphas drive the
fade-after-judge effects consumed by the freeze/normal passes).

### Culling (the load-bearing facts for scaled-down playfields)

- **Top cull (loop break):** at **collector+0xA6 on BOTH builds** —
  `MOVSS XMM15, dword ptr [RIP+disp32]` loading **720.0f**, then the
  per-note loop breaks when `get_offset_y(...) > 720.0`.
  - 20260616: insn @ 0x180024be6 (`F3 44 0F 10 3D 49 9F 36 00`), target
    `DAT_18038eb38` = 720.0f.
  - 20260324: insn @ 0x180024166 (`F3 44 0F 10 3D B9 83 36 00`), target
    0x18038c528 = 720.0f.
  - The 720.0 constant is **shared by 14 functions** — do NOT patch the
    constant. Patch the **instruction's disp32** to point at a mod-owned
    float (must be RIP-reachable, i.e. allocated within ±2 GB of the module —
    same class of byte-patch as `power_user_statistics::pacemaker_swap`).
- **Bottom cull (per direction):** `0.0 <= offsetY_param + 96.0 + bottomY`
  where `offsetY_param` is the screen-space receptor offset passed in by
  `render_notes` and 96.0 = `DAT_18035a710` (ARROW_SIZE). Uses raw
  (unscaled) offsets.
- `render_notes` itself also loads 720.0 once (20260616 @ 0x180026b7a) for
  the reverse-scroll `offsetY = reverse ? 720 − posY : posY` computation —
  unrelated to culling; leave untouched.

### Scale-vs-cull analysis

Let `s` = per-side scale factor, lane origin posY ≈ receptor screen Y.

- **s < 1 (shrink):** notes become visible at lane offset `y = (720 −
  posY)/s > 720` — the stock top cull stops collecting before that, so
  arrows would **pop into existence** at screen y ≈ `720·s + posY`. The top
  window must extend to ~`720/s` (min scale 25% → 2880.0). The bottom cull
  self-solves for s < 1 (scaled notes leave the visible region *before* the
  stock cull drops them).
- **s > 1 (grow, ≤150%):** both bounds are already conservative — no cull
  change needed.
- **Density/cost:** an extended window collects more notes — but the worst
  case (window ×4) is equivalent in note count to the stock 0.25× speed mod
  at window ×1, which the game supports natively. Compounded low-speed +
  small-scale is the theoretical worst case; validate CommandList arena
  headroom on cabinet.

## 4. Constants (20260616)

| Global | Value | Used as |
|---|---|---|
| `DAT_18038eb38` | 720.0f | Render height / top-cull bound (shared, 14 readers) |
| `DAT_18035a710` | 96.0f | ARROW_SIZE (quad size, per-panel pitch, cull margin) |
| `DAT_18035a79c` | 0.5f | half (center math) |
| `DAT_18038eb10` | 0x80000000 | float sign-bit mask (reverse mirroring) |

## 5. Side attribution options for renderer instances

1. **posX split** (proven pattern from `overlay_element_styling`): read
   `this+0x30` at fill time; single-active-side via `player_array_anchor`
   presence read; versus → `posX < 640` = P1. Doubles: `mode@+0xB0` (Arrow)
   / `+0x98` (Spot) == 1 → treat as side 0.
2. GamePlayActor walk via judge_hook (actor+0x84 = side) — provides the
   side↔option values, but not a renderer-pointer binding without further RE
   of GamePlayActor member offsets. Not needed if (1) is used.

## 6. Dead ends verified

- The HUD named-layout coord map (`arrow`/`arrow_raw` entries) carries
  scaleX/scaleY fields at [4]/[5], but the lane renderers consume only
  x/y from it — writing scale there has no effect on arrows/receptors.
- AFP-layer scaling (the `overlay_element_styling` mechanism) does not apply:
  the lane renderers are CommandList sprite emitters, not CMovieClips.
- `GuidelineRenderer` does not draw via the shared fill — it has its own
  plain-sprite batch path, RE'd in §8 (load-bearing per requirements A6).

## 7. Hookability summary

| Target | Status | Use for this feature |
|---|---|---|
| `render_sprite_final` (0x180025900) | **Un-detoured; real CALLs from all lane renderers** | Primary injection point: scale `(x,y,w,h)` + compose opacity into `color.a`, per side, per renderer class (classify via vtable / this-identity). |
| `render_notes` (0x180026b00) | Already detoured by `mine_render` | Extend into a shared dispatcher if a per-frame pre-pass is needed (e.g. setting the per-side cull-window float before the collector runs). |
| Collector top-cull insn (collector+0xA6) | Byte-patchable (disp32 redirect) | Extend the top window to 720/s for shrink scales. |
| `set_direction` (existing AOB) | Un-detoured | Not needed (twist handled inside fill). |
| Guideline draw + its bulk emitter `FUN_18000c7b0` (§8) | Un-detoured; emitter has a single caller | Guideline scale/opacity: capture detour (Y-base pre-scale) + record transform in the emitter detour; cull via the same disp32-redirect. |
| Mine quads (`mine_render`) | Mod-owned code | Apply the same per-side transform + extend its hardcoded 720/margin culls. |
## 8. GuidelineRenderer draw path (added after A2/A6 made it load-bearing)

`FUN_180026210` (20260616) is the guideline (measure-line) draw. Identified
via `get_offset_y` xrefs: its callers are exactly {collector ×2, freeze pass,
guideline draw} on BOTH builds (20260324: `FUN_1800240c0` ×2 + `FUN_180025760`
+ `FUN_180026df0`). The guideline is distinguishable cross-version as the
`get_offset_y` caller that emits **plain sprites** (tag 0x01) via a bulk-copy
helper instead of calling the rotate-quad fill.

- Its prologue AOB (`48 8B C4 55 41 54 41 55 41 56 41 57 48 8D 68 98 48 81 EC
  40 01 00 00`) matches **3 functions** on 20260616 — derive from
  `get_offset_y` xrefs (callee-set classification) instead of a raw AOB.

### Object layout (from decompile; NOT ArrowSprite-based)

| Offset | Field |
|---|---|
| +0x18 | measures-vector ref (ptr → {begin,end}; 0x18-byte entries {beat_count i32, numerator i32, denominator i32}) |
| +0x20 | vbptr (reverse flag via `*(*(this+0x20)+4) + 0x20 + this`) |
| +0x28 / +0x2C | speed×100 i32 / boost enum |
| +0x34..+0x40 | appearance fade fields (start/end height, start/end value) |
| +0x44 / +0x48 | beat_count / music_count |
| +0x4C + idx*8 | guideline enable gate (must == 1 to draw) |
| +0x50 + idx*8 | fade factor float |
| +0x76 / +0x78 / +0x7C | index u16 / mode (1 = double → 8 panels) / type (2 = off; 0 → +0x30 y-adjust) |
| **+0x80 / +0x84** | **X base (lane left, f32) / Y base (receptor screen Y, f32)** |
| +0x88..+0x8B | color RGB + alpha bytes |

### Emission — a private, hookable bulk emitter

Lines are accumulated into a temp vector of **0x14-byte records**
`{x f32 = this+0x80, y f32 = screen Y, w f32 = numPanels·96, h f32 = 3.0
(DAT_18038fc78), color u32 (alpha pre-composed: base·brightness(0xFF major /
0x3F minor)·appearance·fade)}` and submitted in ONE call to
`FUN_18000c7b0(cmdlist, count, records*)`, which writes a tag-0x01
DRAWSPRITES command and memcpys the records into the arena.
**`FUN_18000c7b0` has exactly one caller (the guideline draw)** — detouring it
is a de-facto private hook receiving the whole line batch pre-submission,
where records can be transformed in place (scale x about lane center =
`x + w/2` from the record itself; scale y/w/h; multiply color alpha).

### Guideline culling

- Normal scroll: loop breaks when screen y > **the same shared 720.0f**
  (`MOVSS XMM9, [RIP+disp]` @ 0x180026448, bytes `F3 44 0F 10 0D E7 86 36
  00` → DAT_18038eb38). Patchable with the same disp32-redirect technique /
  same mod-owned float.
- Reverse scroll: bound is a literal 0.0 compare (not a patchable constant).
  Workaround: the guideline's screen y is `±(offset + adj) + Ybase@+0x84`; a
  capture detour on the guideline draw can pre-scale `+0x84` to `Y/s` before
  the original runs (restore after), making the emitter-side transform
  `y' = s·y` exact for BOTH scroll directions while both cull bounds cover
  the extended window.

