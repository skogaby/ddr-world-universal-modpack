# Gameplay Overlay Elements (Combo / Judgement / Pacemaker) — Scale & Opacity Research

RE notes for a mod that exposes per-element **scale** and **opacity** settings for the
dynamic feedback elements drawn over the playfield during gameplay:

- **Combo counter** (`dance_combo_root{1,2,3}` clips, owned by `ComboActor`)
- **Judgement text** (MARVELOUS/PERFECT/… — `dance_judge`, owned by `NoteResultActor`)
- **Freeze judgements** (O.K./N.G. — `dance_judge_for_freeze` × 7/15)
- **FAST/SLOW display** (`dance_fast_slow`)
- **Pacemaker score tracker** (`dance_score_compare` — the ± score-diff readout; the
  same element PUS's `pacemaker_swap` patches)

**Explicitly out of scope** (maintainer decision): the receptor hit flashes
(`dance_effect` clips, also owned by `NoteResultActor`) are NOT to be modified.

All addresses are **file-relative to base `0x180000000`**, from **gamemdx 20260616**
(Ghidra program `gamemdx_20260616.dll`) unless marked otherwise. libafp addresses
are from **libafp-win64 20260324** (exports are name-stable; addresses
informational only). Cross-version status: **verified on both gamemdx 20260616 and
20260324** — all three AOBs, the vtable slot layout, and the sole-matrix-writer
invariant hold on both builds (see Cross-Version Notes for the 20260324 address
table and the twin-order flip caveat).

---

## 1. Overview — how these elements render

All five element groups are **BM2D CMovieClip pool wrappers**: game-side wrapper
objects around engine AFP layers (the same type-1 layer ids the modpack's
`bm2d_api` AFP-layer wrapper set drives for the animated background previews).
The actors create them at gameplay-scene build time from AFP templates inside the
`dance_common`/2D packages, position them once from a per-side layout registry, and
then drive them **event-wise** (not per-frame): play/frame-label/visibility ops on
each judgement or combo change. The engine's timeline animates the pop/fade
*inside* the layer; the layer's own transform and color-transform are static unless
game code writes them.

That gives two clean, independent injection points per element:

| Property | Mechanism | Field (libafp layer object) | Writers in gamemdx |
|---|---|---|---|
| **Scale** | `afp_layer_set_matrix(id, {sx,0,0,sy,0,0})` | matrix at `layerobj+0x100` | **exactly one**: CMovieClip vfunc `+0x40` (SetRotation) — never called on these elements |
| **Opacity** | `afp_layer_set_color(id, r,g,b,a)` (multiplicative CXFORM) | color block ptr at `layerobj+0x150`, mult RGBA at `+0x00..0x0C` | **exactly two**: CMovieClip vfuncs `+0x90` (float form) and `+0xB0` (int-percent form) |

Layer **position** is stored in the SAME 4×4 matrix as scale, NOT a disjoint
field: the translation lives in the matrix's translation row at
`layerobj+0x130/0x134`, and `afp_layer_set_position` writes ONLY those two
dwords (leaving the scale entries intact — libafp-verified, `0x1800135e0`).
Because `afp_layer_set_matrix` rewrites the WHOLE 4×4, a bare scale
`{s,0,0,s,0,0}` (tx=ty=0) **zeroes the translation and slams the element to the
screen origin** — the scale one-shot MUST carry the element's current position
as `{s,0,0,s,tx,ty}` (cabinet-confirmed 2026-07-12; see §9 and §10). The scale
survives later game repositions precisely because `afp_layer_set_position`
touches only the translation dwords, not the scale.

Because the elements were created with wrapper `SetAlign(3,3)` (center/center — the
alignment offset is applied at the **MC level inside the layer**, via
`afp_mc_set_param(root_mc, 0x101B, {ox,oy})`), the layer origin sits at the
element's visual center → a bare `{s,0,0,s,0,0}` matrix **scales about the visual
center**, exactly what a scale option wants.

---

## 2. Element inventory

### 2.1 ComboActor (`sequence::dance::ComboActor`, 0xA0 bytes)

Constructed inline in the per-player gameplay actor factory `FUN_18005be50`
(vtable written at `0x18005c2a9`; vtable = `0x180360438`).

| Offset | Type | Field | Evidence |
|---|---|---|---|
| +0x00 | ptr | vtable `0x180360438` | factory inline ctor |
| +0x58 | ptr | → side index (int) | `**(int**)(this+0x58)` in create/handler |
| +0x60 | u8 | is_double | factory `bVar22` |
| +0x64 | i32 | cached digit count | `FUN_1800667a0` |
| +0x68 | i32 | current combo | msg 0x1033 handler |
| +0x6C | i32 | worst-judgement tier this combo (0xFF sentinel) | handler; persisted on destroy to `PlayerWork+0x23C` and stage record `+0x5AC` |
| +0x70/78/80 | ptr ×3 | CMovieClip* `dance_combo_root3/2/1` | create loop |
| +0x94 | u8 | hide latch (msg 0x103C) | `FUN_1800664a0` |

ComboActor vtable (`0x180360438`): slot 4 = onCreate `FUN_1800660c0`, slot 5 =
onDestroy `FUN_180066520`, slot 6 = hide `FUN_1800664a0`, slot 8 = message handler
`FUN_1800665e0`.

- **onCreate `FUN_1800660c0`**: creates 3 pool wrappers from templates
  `dance_combo_root3/2/1` (package `dance_combo`), `layer_play(0)`,
  `set_attribute(1,1)` (visible), priority 3/2/1 (single/versus) or 12/11/10
  (double), view 0 (single/versus) or side+2 (double), **SetColor(a=0, r=g=b=1.0)**
  (starts fully transparent — alpha is the combo's visibility gate), `SetAlign(3,3)`,
  `SetPosition(layout["combo"])`, digit template `daco_combo%s_%d`, digit slots
  `combo_usr/number_usr/%d_usr`.
- **msg 0x1033 (combo changed) `FUN_1800665e0`**: combo > 3 → `SetColor(a=1,1,1,1)`
  on all three roots + frame-label ops + `FUN_1800667a0` digit refresh (which also
  `SetColor`s a per-judgement RGB tint on the roots via the **array-form** color
  vfunc `+0x98`, from an RGB table at `0x18047E...`/`auStack_74` with a = 1.0);
  combo ≤ 3 → `SetColor(a=0, 1,1,1)` — hidden.
- Why three roots: the templates differ per COMBO DISPLAY PRIORITY layer; the actor
  drives all three identically. For scale/opacity purposes treat them uniformly.

### 2.2 NoteResultActor (`sequence::dance::NoteResultActor`, 0x110 bytes)

Ctor `FUN_18007a050` (called from the same factory, alloc 0x110; vtable symbol
`sequence::dance::NoteResultActor::vftable`, RTTI `.?AVNoteResultActor@dance@sequence@@`
@ `0x180482d88`). Create = `FUN_18007a230`. Message handler = `FUN_18007af00`
(the function whose case `0x1036` contains the existing `pacemaker_render_input`
patch site at `0x18007b032`).

| Offset | Type | Field | Notes |
|---|---|---|---|
| +0x88 | ptr | layout ctx (`*(int*)` = side index) | passed to `FUN_18006ef50(layout, name)` lookups |
| +0x90 | i32 | is_double | |
| +0x94 | i32 | last judgement index (0=marv..5=miss, 6=OK, 7=NG) | |
| +0x98 | i32 | last fast/slow delta | |
| +0xA0 | ptr | CMovieClip* **`dance_judge`** (judgement text) | priority via `FUN_18007ad50`: view 0 pri 4 (single) / view side+2 pri 0xD (double) |
| +0xA8 | ptr | CMovieClip* **`dance_fast_slow`** | created only if option enabled (vfunc `+0x270` on player ctx) |
| +0xB0 | ptr | CMovieClip* **`dance_score_compare`** (pacemaker) | not created in mode `*(int*)(*DAT_1806f04f8+0x1C) == 10` |
| +0xB8 | ptr | score_compare package ptr | |
| +0xC8..D8 | vec | `vector<CMovieClip*>` **`dance_judge_for_freeze`** × (panels*2−1): 7 single / 15 double | half-panel-pitch x spacing from `layout["freeze_judge"]` |
| +0xE8..F8 | vec | `vector<CMovieClip*>` `dance_effect` (receptor flashes) | **EXCLUDED from mod scope** |
| +0x108/+0x10C | i32 | fast_slow x/y | y precomputed; x updated by msg 0x1035 |

Message handler `FUN_18007af00` cases (actor message codes, coincidentally in a
0x10xx range — not `mc_set_param` params):

| Case | Meaning | Ops on our elements |
|---|---|---|
| 0x1028–0x102F | judgement 0–7 fired | judge: `layer_play` + `set_attribute(1,…)` + frame-label `in_marvelous`…`in_ng`; fast_slow: show/hide + frame label + `SetPosition`; freeze clips: per-panel frame ops. **No color, no matrix.** |
| 0x1030/0x1031 | freeze O.K. / N.G. | judge clip frame label `in_ok`/`in_ng` |
| 0x1032 | freeze-hold tick | freeze clips `SetFrame` (mc_op 0xF08). No color/matrix. |
| 0x1035 | fast/slow x update | fast_slow `SetPosition(x, this+0x10C)` — position only |
| 0x1036 | pacemaker delta update | score_compare: digit textures (`dascco_plus/minus/plusminus`), and **`SetColor(a = delta>0 ? 1.0 : 0.5, …)`** via wrapper vfunc `+0x90` — the game dims a negative pacemaker to 50 % alpha |
| 0x103A | pacemaker outro | frame label `"out"` (`0x18035cba0`) |

### 2.3 Per-side layout registry

`FUN_18006bbb0` (gameplay layout builder) instantiates a throwaway `dance_root`
clip, resolves per-side placeholder sub-clips (`%s/judge_usr`, `%s/combo_usr`,
`%s/fast_slow_usr`, `%s/score_compare_usr`, `%s/freeze_judge_usr`, … with `%s` =
`%dp_lane_usr` / `double_lane_usr`), and stores each element's x/y/size into a
per-side registry at `layout+0x108+side*0x48` keyed by name (`"judge"`, `"combo"`,
`"fast_slow"`, `"score_compare"`, `"freeze_judge"`). The actors read positions from
here — this is why element positions are per-side data, not code constants.

---

## 3. The BM2D CMovieClip pool wrapper (gamemdx-side)

Pool: `DAT_1806f8b10` (20260616), 0x400 entries × stride **0x240** — the same pool
the modpack already derives via the `bm2d_pool_iter` AOB (`FF C3 48 81 C7 40 02 00
00 81 FB 00 04 00 00`). Wrappers are constructed by a CRT dynamic initializer
(`FUN_1802d1460` → vector-ctor-iterator with ctor `FUN_1802576e0`), which installs
the **CMovieClip vtable @ `0x18035d9c8`**.

Wrapper layout (fields proven by disassembly):

| Offset | Type | Field | Evidence |
|---|---|---|---|
| +0x00 | ptr | vtable `0x18035d9c8` | ctor `FUN_1802576e0` |
| +0x08 | u32 | type-1 AFP layer id (0 = free slot) | `Create` stores `afp_layer_create_with_property` result |
| +0x10 | struct | root-MC binding: `+0x110` = root type-4 MC id, `+0x114` = child-path buf (0x100 bytes, `"/"` for root) | `FUN_180258040(this+0x10, layer, "/")`: `afp_layer_mc_refer` result → `+0x100` rel = `this+0x110`; strcpy path → `this+0x114` |
| +0x220 | f32[3] | user params (raster scroll / HSV) via `afp_layer_set_user_params` | vfuncs +0x100/+0x108 |
| +0x238 | i32 | create mode flag (5th arg; 1 → `set_attribute(0x200,0x200)`) | `Create` tail |
| +0x23C | i32 | copy of package dword 0 | `Create` head |

`CMovieClip::Create` = **`FUN_180257770`** —
`Create(this, package*, name: *const c_char, priority: i32, mode: i32)`:
copies `package+0x314` (the afpu package id — same offset `bm2d_package` reads),
calls `afpu_get_afp_info_at_package(&info, pkg_id, name)` then
`afp_layer_create_with_property(info…)`, stores the layer id at `+0x08`, binds the
root MC, applies mode, then virtual `SetPriority(priority)`.
**The template name crosses this function as a plain C string in R8** — this is the
capture point for identifying element wrappers without any name-field guesswork.

CMovieClip vtable map (`0x18035d9c8`, slots relevant to this mod; full dump in §7):

| vfunc | Impl | Meaning | libafp call |
|---|---|---|---|
| +0x18 | `0x180257870` | Destroy (frees layer, zeroes +0x08) | `afp_layer_do_destroy` |
| +0x38 | `0x180258DE0` | SetPosition(int x, int y) | `afp_layer_set_position` (floats by bit pattern → `layerobj+0x130`) |
| +0x40 | `0x180258E90` | SetRotation(angle) — builds identity, rotates (`Ordinal_85`), **the only `afp_layer_set_matrix` caller in gamemdx** | `afp_layer_set_matrix` (IAT `0x1802d75d0`) |
| +0x48 | `0x180258FF0` | SetAlign(hmode, vmode) — mode 3 = center (extent × 0.5) → `afp_mc_set_param(root_mc, 0x101B, {ox,oy})` | mc-level, inside layer transform |
| +0x90 | **`0x18025E790`** | **SetColor(this, a, r, g, b)** — note **alpha is the FIRST float arg** (XMM1); forwards as `afp_layer_set_color(id, r, g, b, a)` | `afp_layer_set_color` (IAT `0x1802d76d8`) |
| +0x98 | `0x1802590F0` | SetColorArray(this, &{r,g,b,a}) — **virtual-dispatches to +0x90** (`CALL [RAX+0x90]`) | (via +0x90) |
| +0xA0 | `0x180259150` | SetAColor(this, a, r, g, b) — additive twin, byte-identical body | `afp_layer_set_acolor` (IAT `0x1802d76e0`) |
| +0xA8 | `0x180259120` | SetAColorArray → virtual +0xA0 | (via +0xA0) |
| +0xB0 | `0x180259180` | SetColorInt(this, alpha_pct: i32, r, g, b) — divides alpha by 100.0 (`DAT_18038ea80`) | `afp_layer_set_color` |
| +0xB8 | `0x1802591C0` | SetAColorInt (additive twin) | `afp_layer_set_acolor` |
| +0xE0 | `0x18025E7C0` | SetPriority | `afp_layer_set_priority` |
| +0xE8 | `0x1802592E0` | SetView/group | `afp_layer_set_group` |
| +0x138 | `0x180259450` | is-valid (slot in use) | |

**Complete gamemdx caller set for the color exports** (IAT xrefs, 20260616):
`afp_layer_set_color` ← only `0x18025E790` (+0x90) and `0x180259180` (+0xB0);
`afp_layer_set_acolor` ← only `0x180259150` (+0xA0) and `0x1802591C0` (+0xB8).
**All color writes to pool-wrapper layers flow through these four wrapper methods.**

---

## 4. libafp color/matrix internals (libafp-win64 20260324)

- **`afp_layer_set_color`** (`0x180013670`, Ordinal 49):
  `(id: u32, r: f32, g: f32, b: f32, a: f32)` — lazily allocates a 0x20-byte color
  block at `layerobj+0x150`, writes **multiplicative** RGBA to block `+0x00..0x0C`,
  maintains "non-identity" flag bit `0x2` in `layerobj+0x14` (identity test
  `FUN_18004d510`: any of r,g,b,a differs from 1.0 → flagged).
- **`afp_layer_set_acolor`** (`0x180013790`, Ordinal 50): same, **additive** RGBA at
  block `+0x10..0x1C`, flag bit `0x4` (non-zero test `FUN_18004d5d0`).
  Standard Flash CXFORM semantics: `result = value * mult + add` (per channel,
  clamped), composed down the display hierarchy — a layer-level mult-alpha scales
  every descendant's (timeline-animated) alpha proportionally.
- **`afp_layer_set_matrix`** (`0x180013030`, Ordinal 45): expands the 2×3 affine
  `{a,b,c,d,tx,ty}` to a 4×4 at `layerobj+0x100` (`afp_matrix44_from_matrix` @
  `0x18004ff00`).
- **`afp_layer_set_position`** (`0x1800135e0`, Ordinal 47): writes the two dwords at
  `layerobj+0x130/0x134` — these ARE the translation row of the layer's 4×4 matrix
  at `+0x100` (the 4th row: `m[12]=tx @ +0x130`, `m[13]=ty @ +0x134`), NOT a
  separate field. It rewrites ONLY those two dwords, so it does not disturb a
  mod-set scale. **Corollary (cabinet bug, 2026-07-12):** `afp_layer_set_matrix`
  rewrites the ENTIRE 4×4 including that translation row, so any scale written via
  set_matrix must include the current `(tx,ty)` or the element jumps to the origin.

All four are **named exports** — resolve like `bm2d_api::init_layer_api` does
(`afp_layer_set_matrix` / `afp_layer_set_position` are already resolved there;
`afp_layer_set_color` / `afp_layer_set_acolor` would be new additions).

---

## 5. Recommended hook design

Per-element settings: `scale` (e.g. 25–200 %) and `opacity` (0–100 %), for:
combo, judgement, freeze-judgement, fast/slow, pacemaker. (Whether the knobs are
per-player custom-options rows or cabinet-wide overlay rows is an implementation
decision; the capture mechanism below supports per-side application.)

1. **Capture detour on `CMovieClip::Create` (`FUN_180257770`)** — cold path (scene
   build only). Filter R8 name against: `dance_combo_root` (prefix),
   `dance_judge`, `dance_judge_for_freeze`, `dance_fast_slow`,
   `dance_score_compare` (exact). NOTE: `dance_judge` must be matched exactly, not
   as a prefix, or it would also capture `dance_judge_for_freeze`. On match, after
   calling the original: record `(wrapper_ptr, layer_id = wrapper+0x08, kind)` and
   apply the one-shots directly (we are on the game thread, the layer exists):
   - scale: `afp_layer_set_matrix(id, {s,0,0,s,0,0})`
   - opacity initial: `afp_layer_set_color(id, 1,1,1, op)` — survives untouched for
     judge/freeze/fast_slow (the game never colors them); for combo/pacemaker the
     game's later writes are handled by detour 2.
   Slot-reuse invalidation: any `Create` over a tracked wrapper ptr with a
   non-tracked name evicts it; also clear all on gameplay-scene exit
   (`scene_manager`).
2. **Opacity-compose detour on wrapper SetColor `FUN_18025E790` (+0x90)** — for
   tracked wrappers, multiply the incoming **first float arg** (alpha; the arg
   order is `(this, a, r, g, b)`) by the element's opacity before forwarding.
   This composes with the game's semantics instead of fighting them:
   - combo: alpha 0 (hidden, combo ≤ 3) stays 0; alpha 1 → `op`; RGB judgement
     tints pass through. The array-form (+0x98) dispatches virtually into +0x90,
     so it is covered.
   - pacemaker: game's 1.0/0.5 (negative-delta dim) → `op`/`0.5·op`.
   Optionally also detour the int-percent variant `0x180259180` (+0xB0) with the
   same filter — its caller set is unknown (virtual dispatch); hooking it closes
   the only other mult-color path. (The additive variants +0xA0/+0xB8 can be left
   alone; nothing suggests they are used on these elements.)
3. **Side attribution** (only needed if the options are per-player): `Create`
   doesn't see the player side. Options: (a) if only one side is active (single /
   double), all captures belong to that side; (b) for versus, bind on the first
   wrapper `SetPosition` (`FUN_180258DE0`, +0x38) after capture — the game
   positions each element immediately after `Create` from the per-side layout, so
   the x argument discriminates sides (threshold ≈ screen middle; validate the
   exact split on cabinet); (c) alternatively rely on creation order (factory runs
   per side), which is simpler but unproven against 2P-only starts.
4. **Live option changes mid-song**: re-apply matrix one-shots directly to tracked
   layer ids (cheap); opacity changes take effect on the next game color write for
   combo/pacemaker, or re-write `set_color` directly for the never-colored
   elements.

Rejected alternatives, for the record:
- *One-shot mult color for combo/pacemaker*: clobbered by the game per event, and
  overwriting would break the combo's alpha-0 visibility gating.
- *Additive alpha (`set_acolor` a=−δ) instead of detour 2*: zero-detour, but
  subtractive (non-proportional — distorts timeline fades and the pacemaker's
  0.5 dim) and clamps mid-fade values harshly.
- *Hooking `afp_layer_set_color` in libafp*: engine-wide hot path; the wrapper
  method detour has identical coverage for pool clips with far less traffic.

Threading: `Create`/`SetColor` fire on the game thread during scene build /
gameplay; all mod-side application happens inside those detours or via
`run_on_render_thread` — consistent with the "libafp only from the game thread"
rule.

---

## 6. Signatures

### 6.1 `cmovieclip_create` → `FUN_180257770`

```
48 89 5C 24 10 56 48 83 EC 40 41 8B F1 48 8B D9 48 85 D2 0F 84 ? ? ? ?
4D 85 C0 0F 84 ? ? ? ? 83 79 08 00 0F 85 ? ? ? ? 8B 02 89 81 3C 02 00 00
8B 92 14 03 00 00
```

- Match at function start. **Unique** on 20260616 (verified).
- Wildcards: the three internal `Jcc` disp32s (shift if the body is recompiled).
- Structural anchors kept fixed: prologue, `CMP [RCX+8],0` (layer-id-free check),
  `MOV [RCX+0x23C],EAX` (wrapper package-dword copy), `MOV EDX,[RDX+0x314]`
  (afpu package id — the same `+0x314` offset `bm2d_package`/`bm2d_api` already
  depend on, stable across both 2026 builds).

### 6.2 `cmovieclip_set_color` → `FUN_18025E790` (vfunc +0x90)

```
48 83 EC 38 8B 49 08 0F 28 C3 F3 0F 10 5C 24 60 0F 28 E2 F3 0F 11 4C 24 20
0F 28 D0 0F 28 CC FF 15 ? ? ? ? 48 83 C4 38 C3
```

- Complete function, prologue-to-ret. Wildcard: the `CALL [RIP+disp32]` IAT
  displacement only.
- **Matches exactly 2 addresses** on 20260616: `0x18025E790` (set_color) and
  `0x180259150` (set_acolor twin — byte-identical body, different IAT slot).
  **Disambiguate at runtime**: decode the RIP-relative operand at match+0x1F
  (via `scanner::decode_rip_relative`), read the IAT slot, and compare the target
  against `GetProcAddress("libafp-win64.dll", "afp_layer_set_color")`. Assert the
  other match resolves to `afp_layer_set_acolor` as a sanity check.

### 6.3 `cmovieclip_set_color_int` → `FUN_180259180` (vfunc +0xB0, optional)

```
48 83 EC 38 8B 49 08 0F 28 CB F3 0F 10 5C 24 60 0F 28 E2 66 0F 6E C2
0F 28 D1 0F 5B C0 0F 28 CC F3 0F 5E 05 ? ? ? ? F3 0F 11 44 24 20 FF 15
```

- Same twin situation with `+0xB8` (`0x1802591C0`); disambiguate via the trailing
  `FF 15` IAT target as above. Wildcards: the `DIVSS xmm0,[RIP+disp]` constant
  (100.0f @ `DAT_18038ea80`) displacement and the IAT displacement.

### 6.4 Named exports (no AOB)

`afp_layer_set_color`, `afp_layer_set_acolor` (new), `afp_layer_set_matrix`,
`afp_layer_set_position` (already resolved by `bm2d_api::init_layer_api`) — resolve
by name from `libafp-win64.dll`.

### 6.5 `cmovieclip_set_position` → `FUN_180258DE0` (vfunc +0x38)

Needed for versus side-binding (first-position x-discrimination):

```
48 83 EC 38 48 8B 05 ? ? ? ? 48 33 C4 48 89 44 24 28 8B 49 08 66 0F 6E C2
66 41 0F 6E C8 48 8D 54 24 20 0F 5B C0 0F 5B C9 F3 0F 11 44 24 20
F3 0F 11 4C 24 24 FF 15 ? ? ? ? 48 8B 4C 24 28 48 33 CC E8 ? ? ? ?
48 83 C4 38 C3
```

- Complete function. Wildcards: security-cookie load disp32, the
  `afp_layer_set_position` IAT disp32, the `__security_check_cookie` CALL
  rel32. All other bytes are identical across both builds (diffed byte-for-byte).
- **Unique match on both builds** (verified): `0x180258DE0` (20260616),
  `0x18021CD20` (20260324).
- Signature: `fn(this: *mut wrapper, x: i32, y: i32)` — converts to floats,
  forwards to `afp_layer_set_position(layer_id, &{x,y})`.

**Vtable derivation option**: instead of AOBs 6.2/6.3, resolve the CMovieClip
vtable once and take slots +0x90/+0xB0/+0x38 from it. The vtable address
(`0x18035d9c8`) is version-variant, but derivable: the dynamic initializer
(`FUN_1802d1460`-shaped: `LEA RAX,[dtor]; LEA R9,[ctor]; LEA RCX,[pool]; MOV
EDX,0x240; MOV R8D,0x400`) references the already-AOB-derivable pool base with the
distinctive `0x240`/`0x400` immediates; the ctor's `LEA R11,[vtable]; MOV
[RBX],R11` yields the vtable. Either route works — the direct AOBs are simpler,
the vtable route survives function-body recompiles better.

---

## 7. Key addresses (gamemdx 20260616)

| Symbol | Address | Notes |
|---|---|---|
| gameplay actor factory (per player) | `FUN_18005be50` | constructs ComboActor inline @ `0x18005c22f..0x18005c2d2`; NoteResultActor via `FUN_18007a050` |
| `ComboActor::vftable` | `0x180360438` | slots: 4=create `0x1800660C0`, 5=destroy `0x180066520`, 6=hide `0x1800664A0`, 8=msg `0x1800665E0` |
| ComboActor digit refresh | `FUN_1800667a0` | colors roots via array-form vfunc +0x98 |
| `NoteResultActor` ctor / create / msg | `0x18007a050` / `0x18007a230` / `0x18007af00` | msg case 0x1036 = pacemaker (existing `pacemaker_render_input` patch @ `0x18007b032`) |
| judge-cluster priority helper | `FUN_18007ad50` | view/priority only |
| gameplay layout builder | `FUN_18006bbb0` | per-side element positions registry |
| CMovieClip pool | `DAT_1806f8b10` | 0x400 × 0x240; derive via existing `bm2d_pool_iter` AOB |
| CMovieClip vtable | `0x18035d9c8` | installed by ctor `FUN_1802576e0` (dyn-init `FUN_1802d1460`) |
| `CMovieClip::Create` | `FUN_180257770` | name in R8 — capture point |
| wrapper SetColor (float) / (int) | `0x18025E790` / `0x180259180` | vfunc +0x90 / +0xB0 |
| wrapper SetAColor (float) / (int) | `0x180259150` / `0x1802591C0` | vfunc +0xA0 / +0xB8 |
| wrapper SetRotation (sole matrix writer) | `0x180258E90` | vfunc +0x40 |
| wrapper SetPosition / SetAlign | `0x180258DE0` / `0x180258FF0` | vfunc +0x38 / +0x48 |
| IAT: set_matrix / set_color / set_acolor | `0x1802d75d0` / `0x1802d76d8` / `0x1802d76e0` | libafp ordinals 45 / 49 / 50 |
| consts: 1.0f / 0.5f / 100.0f | `0x180358f64` / `0x18035a79c` / `0x18038ea80` | verified reads |
| template strings | `dance_combo` `0x180360368`, `dance_combo_root%d` `0x180360378`, `dance_judge` `0x180360970`, `dance_judge_for_freeze` `0x1803620e0`, `dance_fast_slow` / `dance_score_compare` (see `FUN_18007a230`) | |

CMovieClip vtable full slot dump (base `0x18035d9c8`): slots 0–41 =
`0x180257740, 0x180257860, 0x180258D50, 0x180257870, 0x18025E5A0, 0x180258DD0,
0x180258E30, 0x180258DE0, 0x180258E90, 0x180258FF0, 0x180258F90, 0x180258F20,
0x18025E660, 0x1802590C0, 0x18025E680, 0x18025E6A0, 0x18025E6C0, 0x1800214E0,
0x18025E790, 0x1802590F0, 0x180259150, 0x180259120, 0x180259180, 0x1802591C0,
0x180259210, 0x180259200, 0x1802592B0, 0x1802592A0, 0x18025E7C0, 0x1802592E0,
0x180259300, 0x180259320, 0x180259340, 0x1802593A0, 0x180259400, 0x180259410,
0x180257DE0, 0x180259430, 0x180259420, 0x180259450, 0x180259460, 0x1802594A0`.

---

## 8. Cross-Version Notes

Both currently-supported builds verified (20260616 + 20260324). Resolved addresses:

| Symbol | 20260616 | 20260324 | Basis |
|---|---|---|---|
| `CMovieClip::Create` (§6.1 AOB) | `0x180257770` | `0x18021B6A0` | **unique match on both** |
| wrapper SetColor float (§6.2 AOB, ord-49 twin) | `0x18025E790` | `0x180222800` | IAT target = ordinal 49 |
| wrapper SetAColor float (ord-50 twin) | `0x180259150` | `0x180222830` | IAT target = ordinal 50 |
| wrapper SetColorInt (§6.3 AOB, ord-49 twin) | `0x180259180` | `0x18021D140` | IAT target = ordinal 49 |
| wrapper SetAColorInt (ord-50 twin) | `0x1802591C0` | `0x18021D180` | IAT target = ordinal 50 |
| CMovieClip vtable | `0x18035d9c8` | `0x18035B988` | ctor `LEA R11` |
| CMovieClip ctor | `0x1802576E0` | `0x18021B610` | dyn-init `LEA R9` |
| pool dynamic initializer | `0x1802D1460` | `0x1802CE8F0` | `BA 40 02 00 00 41 B8 00 04 00 00` anchor |
| CMovieClip pool base | `0x1806f8b10` | `0x1806F2180` | existing `bm2d_pool_iter` AOB |
| IAT slot: set_color / set_acolor | `0x1802d76d8` / `0x1802d76e0` | `0x1802D56D8` / `0x1802D56D0` | ILT ordinal entries 49/50 |
| wrapper SetPosition (+0x38) | `0x180258DE0` | `0x18021CD20` | vtable slot |
| wrapper SetRotation (+0x40, sole matrix writer) | `0x180258E90` | `0x18021CDD0` | vtable slot; IAT ord-45 xref count = 1 on both |

- **Vtable slot layout is identical on both builds** (all 42 slots line up; every
  role-assigned slot cross-checked: +0x38/+0x40/+0x48/+0x90/+0x98/+0xA0/+0xB0/
  +0xB8/+0xE0/+0xE8/+0x138).
- **Twin order flips between builds.** On 20260616 the float set_color body sits
  at a *higher* address than its acolor twin; on 20260324 it sits *lower* (and the
  IAT slot order is likewise reversed). "Take the first match" would silently pick
  the wrong function on one of the two builds — the runtime IAT-target
  disambiguation in §6.2/§6.3 is **mandatory**, not defensive.
- The sole-`afp_layer_set_matrix`-caller invariant (§1) was re-proven on 20260324:
  the libafp ordinal-45 IAT slot (`0x1802D55F8`) has exactly one code xref —
  vtable+0x40 SetRotation. (The second ordinal-45 slot on each build belongs to a
  different DLL's import block — check the surrounding IAT run before xrefing.)
- `Create = ctor + 0x90` on both builds (incidental, do not rely on it).
- The dyn-init anchor pattern (`MOV EDX,0x240; MOV R8D,0x400`, §6 vtable-derivation
  option) matches exactly twice per build: the initializer and its atexit-registered
  pool *destructor* iterator. Both reference the pool base; only the initializer
  has the ctor in `LEA R9` preceded by two LEAs (dtor in RAX, ctor in R9). Verify
  against the `bm2d_pool_iter`-derived pool base and the `LEA R11,[vtable]` inside
  the ctor before trusting.
- Actor-side addresses in §2 (factory, ComboActor/NoteResultActor methods, message
  codes) were mapped on 20260616 only. They are documentation context, not hook
  targets — the design only detours §6 targets — so they were not re-mapped.
- libafp/libafputils are resolved **by export name** at runtime — their addresses
  in §4 are informational (from the 20260324 DLLs) and don't need version pinning.
- The CMovieClip vtable layout (slot meanings) is load-bearing for the vtable-
  derivation option; re-dump it on any new build before use.

## 9. Gotchas

- **Alpha is the FIRST float arg** of wrapper SetColor (`(this, a, r, g, b)`), not
  the last — the shim reorders into `afp_layer_set_color(id, r, g, b, a)`. Get
  this wrong in the detour and you'll be scaling red instead of alpha.
- **Do not one-shot-overwrite combo/pacemaker mult color** — alpha there is game
  state (combo visibility gate 0/1; pacemaker negative-dim 0.5), not styling.
  Compose multiplicatively in the SetColor detour instead.
- The **set_color/set_acolor wrapper twins are byte-identical**; any AOB for one
  matches both. Always disambiguate via the IAT target (§6.2). Same for the
  int-percent twins.
- **Pool slots are recycled.** `Destroy` (vfunc +0x18) frees the layer and the slot
  becomes reusable; a stale tracked wrapper ptr can alias an unrelated later clip.
  Evict on Create-over-tracked-ptr and on gameplay-scene exit.
- The three combo roots (`dance_combo_root1/2/3`) are all driven identically by
  ComboActor — apply settings to all three, don't try to pick "the visible one".
- `dance_fast_slow` and `dance_score_compare` are **conditionally created**
  (options/mode-gated) — the capture set varies per song; treat every element as
  optional.
- Freeze-judge count differs single (7) vs double (15); capture is per-wrapper so
  this is automatic, but don't assume a fixed count.
- **Receptor hit flashes (`dance_effect`) are out of scope** by explicit decision —
  the capture filter must not match them (they're created in the same
  `FUN_18007a230` loop).
- The judgement "pop" animation is MC-timeline-driven *inside* the layer; the
  layer matrix (our scale) composes above it. Residual risk that some engine path
  writes the layer matrix independently — no gamemdx writer exists besides
  SetRotation, but cabinet-validate that a scale sticks across a whole song.
- **The layer matrix translation row is at `+0x130/0x134` — the scale one-shot MUST
  preserve it.** `afp_layer_set_matrix` overwrites the whole 4×4; a bare
  `{s,0,0,s,0,0}` zeroes translation → element renders scaled at the screen origin
  (upper-left). Confirmed live on a frozen cabinet: our 12 scoped clips all read
  `m[0]=0.30, alpha=0.35` (writes landed) but translation `(0,0)` while an untouched
  sibling read translation `(641,663)`. Fix: `{s,0,0,s,tx,ty}` with the SetPosition
  x/y. (`docs`/design updated 2026-07-12.)
- Actor message codes (0x1028–0x103C) look like `afp_mc_set_param` param ids but
  are an unrelated actor-message namespace — don't conflate.

## 10. Implementation status

Shipped as the **`overlay-element-styling`** mod (`src/mods/overlay_element_styling/`;
signatures `cmovieclip_create` / `cmovieclip_set_position` + the
`derive_cmovieclip_color_twins` IAT resolver in `src/core/signatures.rs`; raw
game-owned-layer setters `layer_set_scale_raw` / `layer_set_color_raw` /
`layer_color_available` in `src/services/bm2d_api.rs`). Design + plan under
`.agents/planning/20260712-overlay-element-styling/`.

Two research items in this doc are **pending cabinet validation** (the code ships a
defensible default for each; update this doc once the cabinet confirms):

- **`X_SPLIT` versus threshold** — hard-coded to `640` (playfield midline) in
  `capture.rs`. The SetPosition bind path logs `bind kind=… x=… side=…` at debug so
  the exact split can be confirmed from one versus play; adjust the constant if the
  logged x values straddle a different boundary.
- **+0xB0 int-color path coverage** — the int-percent compose detour logs once
  (`+0xB0 int color fired on a tracked clip …`) on its first tracked hit. If that
  line never appears across combo/judge/pacemaker play, §5's assumption that all
  scoped color writes flow through +0x90 is confirmed and +0xB0 can be treated as
  dead-path insurance.
