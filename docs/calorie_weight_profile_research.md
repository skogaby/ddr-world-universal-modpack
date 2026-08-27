# Calorie / Weight Profile Field Research

Reverse-engineering document covering the two **workout profile fields** in DDR
World that are normally settable only through Konami's web UI:

- **`weight`** — the player's body weight (kg), fed into the in-game calorie
  calculation.
- **`is_disp_weight`** — the "display burned calories in-game" toggle.

The goal is to extend the **WebUI Options** mod (`src/mods/webui_options/`) to set
these in-game, using the same pattern the cosmetic customizations already use:
write the game's own field in memory, seed the menu by reading it back, and inject
the value on `playerdata_save` for a server to round-trip.

**Game binaries**: `gamemdx.dll` (20260324, 20260616), `ess.dll` (20260324)
**Ghidra base**: `0x180000000` (all addresses file-relative unless noted)
**Tools used**: Ghidra (static analysis)
**Related**: [`player_customization_system_research.md`](./player_customization_system_research.md)
— the cosmetic `<customize>` subsystem this mirrors. **These two fields are NOT part
of the `ddr::player::Customize` object** (which ends at the dormant bgm field +0x34);
they live in the `ddr::player::Work` (PlayerWork) header.

---

## TL;DR (the facts the mod needs)

| Value | gamemdx `PlayerWork` offset | Type | Wire (kbin) | ess `common` offset |
|-------|:--------------------------:|------|-------------|:-------------------:|
| **weight** | **+0x24** | s32 (kg; `0` = unset → game assumes 60) | `<common><weight>` s32 | +0x64 |
| **is_disp_weight** | **+0x28** | u8 / bool (0/1) | `<common><is_disp_weight>` bool | +0x70 |
| today_cal *(context only)* | +0x30 | u64 | `<common><today_cal>` u64 | +0x68 |

Both are reachable via the **exact chain the mod already resolves** for customize —
just at PlayerWork-header offsets instead of `+customize_offset`:

```
player_work_table[playSide]  → wrapper*
*wrapper                     = PlayerWork      // == the mod's `player_work` local
PlayerWork + 0x24            = weight   (s32)
PlayerWork + 0x28            = is_disp_weight (u8)
```

There is a **load path only** — the stock game never saves these (they change only
via the web UI). This is the same one-directional situation the WebUI Options
cosmetics already solve.

---

## 1. Wire format — server → game (`playerdata_load`)

`ess.dll` parses the profile-load response. Both fields are children of the
**`<common>`** node, handled by `sys_playerdata_load_receiver`
(`ess.dll` 20260324 `FUN_180025d70`).

Tag strings (ess.dll 20260324):
- `"weight"` @ `0x1800640b0`
- `"is_disp_weight"` @ `0x180064148`

Each field is read with `XCnbrep70000af(node, tag, kbin_type, dest, size)`, where
`dest = *(base + 0x18) + <offset>` (the ess player-data staging region). Verified
excerpt (decompiled, `common` block):

```c
//                              tag             kbinType  dest                 size
XCnbrep70000af(uVar7, lVar8, "playcount",       6, *(base+0x18) + 0x60, 4);  // s32
XCnbrep70000af(uVar7, lVar8, "weight",          6, *(base+0x18) + 0x64, 4);  // s32   <-- weight
XCnbrep70000af(uVar7, lVar8, "today_cal",       9, *(base+0x18) + 0x68, 8);  // u64
XCnbrep70000af(uVar7, lVar8, "is_disp_weight", 0x34,*(base+0x18) + 0x70, 1); // bool  <-- toggle
XCnbrep70000af(uVar7, lVar8, "is_takeover",    0x34,*(base+0x18) + 0x71, 1); // bool
XCnbrep70000af(uVar7, lVar8, "pre_playable_num",6, *(base+0x18) + 0x74, 4);  // s32
XCnbrep70000af(uVar7, lVar8, "is_subscribed",  0x34,*(base+0x18) + 0x78, 1); // bool
```

kbin type codes seen here: **6 = s32**, **9 = u64**, **0x34 = bool**. So on the
wire `weight` is an `s32` and `is_disp_weight` is a `bool`.

> The `common` block also carries `ddrcode`, `dancername`, `is_new`,
> `is_registering`, `area`, `extrastar`, `playcount`, `today_cal`, the
> subscription flags, etc. Only `weight` and `is_disp_weight` are of interest here.

---

## 2. ess staging buffer → gamemdx PlayerWork (the reflect)

The ess `common` values are copied into `ddr::player::Work` by
`ark::network::ReflectPlayerWork`:

| Version | Reflect fn | Player table global | Staging base | Per-side stride |
|---------|-----------|---------------------|--------------|:---------------:|
| 20260616 | `FUN_180014850` (0x180014850–0x180017780) | `DAT_1806f1ee0` | `DAT_1804e7cf8` | `0x101bc8` |
| 20260324 | `FUN_180013c80` | `DAT_1806ebe50` | `DAT_1804e1c78` | `0x101bc8` |

The routine reads the per-side staging record (`staging_base + playSide*0x101bc8`)
and writes each field into `PlayerWork` (`*plVar4`, where
`plVar4 = player_work_table[playSide]`, i.e. `*plVar4` = the same PlayerWork base
the mod uses). Verified copies — **identical on both versions**:

```c
// 20260616 (FUN_180014850); 20260324 (FUN_180013c80) is byte-for-byte equivalent
*(u32*)(*plVar4 + 0x24) = *(u32*)(staging + 0xd54);   // weight          (s32)
*(u64*)(*plVar4 + 0x30) = *(u64*)(staging + 0xd58);   // today_cal       (u64)
*(u8 *)(*plVar4 + 0x28) = *(u8 *)(staging + 0xd60);   // is_disp_weight  (bool)
*(u32*)(*plVar4 + 0x1708)= *(u32*)(staging + 0xd64);  // pre_playable_num
// ... isSubscribed -> +0x1d, popup platinum flags -> +0x1e/+0x1f ...
```
*(20260324 staging labels are `DAT_1804e1cd4`/`…cd8`/`…ce0` — same relative layout.)*

**Field identification** rests on two independent lines of evidence:

1. The copy **order and sizes** align exactly with the ess `common` layout —
   `weight`(s32) → `today_cal`(u64) → `is_disp_weight`(bool), i.e. ess
   `0x64 → 0x68 → 0x70`, staging `d54 → d58 → d60`.
2. `PlayerWork+0x24` is **independently proven** to be `weight` by the calorie calc
   (§3), and `PlayerWork+0x30` matches the `today_cal` accumulator read by the
   result-screen CalorieTab (§4).

So: **weight → `PlayerWork+0x24` (s32)**, **is_disp_weight → `PlayerWork+0x28`
(u8)**. `is_takeover` (ess +0x71) is not reflected to a PlayerWork header slot.

---

## 3. Consumer / proof — the gameplay calorie calc

`FUN_180053430` (20260616) is the calorie-actor update. It reads weight directly
from `PlayerWork+0x24`:

```asm
180053430: MOVSXD RAX,[RCX + 0x58]           ; RCX = actor; +0x58 = playSide
180053434: LEA    RDX,[0x1806f1ee0]          ; player_work_table  (RIP-relative)
18005343b: MOV    RAX,[RDX + RAX*8]          ; wrapper = player_work_table[side]
18005343f: MOV    RDX,[RAX]                  ; PlayerWork = *wrapper
180053442: MOVD   XMM0,dword ptr [RDX + 0x24]; weight  <-- +0x24
180053447: CVTDQ2PS XMM0,XMM0                ; (int -> float)  => weight is an int
18005344a: COMISS XMM0,dword ptr [0x180359698]  ; threshold = 0.0
180053451: JBE    0x180053464
180053453: DIVSS  XMM0,dword ptr [0x18038ea80]   ; divisor  = 100.0
18005345b: MOVSS  [RCX + 0xcc],XMM0              ; actor.factor = weight/100.0
180053463: RET
180053464: MOV    dword ptr [RCX + 0xcc],0x42700000 ; else factor = 60.0f (unset default)
18005346e: RET
```

Constant values (read from memory):
- `_DAT_180359698` = **0.0f** (the "weight is set" threshold; weight must be `> 0`)
- `DAT_18038ea80` = **100.0f** (divisor)
- unset default `0x42700000` = **60.0f**

⇒ **`weight` is an integer body weight in kg; `0` means "unset" and the game
assumes 60 kg.** Writing `PlayerWork+0x24` changes the calorie computation.

This block also doubles as an alternate landmark for `player_work_table` (the
`LEA RDX,[0x1806f1ee0]` at `0x180053434`).

### Class family (context)
RTTI strings (gamemdx): `.?AVCalcCalorieActorBase@dance@sequence@@`,
`…Single…`, `…Double…`. Base ctor `FUN_180053340` (sets the `agcs::Actor`
vtable + the `"CalcCalorieActor"` name label @ `0x18035f438`); the actor is
constructed in the gameplay-sequence builder `FUN_18005be50` — Single subtype
size `0xd8`, Double `0xe0` — with `param_1+0x84` = playSide and
`param_1+0x88 == 1` selecting Double.

### 3.1 Full calorie-accumulation formula (discovered)

The `CalcCalorieActor` has a 10-slot vtable — `Base` @ `0x18035f458`,
`Single` @ `0x18035f4b8`, `Double` @ `0x18035f518` (0x60 apart). Roles of the
CalcCalorie-specific slots:

| Slot | vtable off | Single | Double | Role |
|:----:|:----------:|--------|--------|------|
| 4 | +0x20 | `FUN_180053430` | (same) | compute weight factor (§3) |
| 5 | +0x28 | `FUN_1800534a0` | (same) | onFinalize — commit stage kcal to profile |
| 6 | +0x30 | `FUN_180053470` | (same) | per-frame tick — accumulate |
| 8 | +0x40 | `FUN_1800534e0` | (same) | message handler (`0x1043`/`0x1045`) |
| 9 | +0x48 | `FUN_180053560` | `FUN_180053bb0` | per-window kcal increment (mode-specific) |

Actor state fields used by the calc:
- `+0x58` playSide · `+0x60` step/judge source (shared_ptr chain)
- `+0x94` **running kcal accumulator** for the current stage
- `+0xC8` resolved **step-pattern class** (0..7 single / 0..9 double)
- `+0xCC` **weight factor F** (written by slot 4)
- `+0xD0` **chart intensity** value (written by message `0x1045`, payload word [3])
- `+0x68..` five "measurement window" entries (stride 8); `+0x90` count (=5);
  `+0x92` current window index

**Per-window increment** — the value returned by slot 9:
```
inc = (int)( scoreWeight[class] * F / 60.0 )        // DAT_18035a2b8 = 60.0f
    + (int)( bonusWeight[class] * intensity ) / 20   // /0x14
```
where
- `F` = `actor+0xCC` = `(weight > 0) ? weight/100.0 : 60.0`  (§3; `DAT_18038ea80`
  = 100.0f, unset default `0x42700000` = 60.0f)
- `intensity` = `actor+0xD0` (set from message `0x1045`)
- class-weight tables (indexed by `actor+0xC8`):
  - **Single** (inline constants): `scoreWeight[0..7] = {31,26,19,23,120,116,124,112}`,
    `bonusWeight[0..7] = {1,1,4,3,2,4,0,6}`
  - **Double** (`DAT_180464ad0`, interleaved `score,bonus` byte pairs):
    `score[0..9] = {31,26,19,23,120,116,124,112,36,74}`,
    `bonus[0..9] = {1,1,4,3,2,4,0,6,2,6}`
  - Higher classes (single 4–7, the `~112–124` weights) correspond to
    jumps/crossovers — i.e. the class is a "MET-like" intensity of the step
    pattern just resolved.

**Accumulate** (slot 6, each frame while the current window is flagged closed,
`*(int*)(actor + 0x68 + idx*8) == 1`): `actor+0x94 += inc`.

**Commit to profile** (slot 5, at stage finalize):
```
PlayerWork[ +0x5dc + dayIndex*0x2b8 ] = actor+0x94    // dayIndex = *DAT_1806f04f8 + 0xC
```
This is the per-stage kcal slot the result-screen CalorieTab (§4) sums together
with `today_cal` (+0x30) for the displayed total.

**End to end:**
```
kcal(stage) = Σ_windows [ scoreWeight[class]·(weight/100)/60  +  bonusWeight[class]·intensity/20 ]
```
`weight` is the **only** player-profile input to the formula; everything else is
chart/step-derived.

**⚠ Weight unit / unset-default anomaly (needs runtime confirmation).**
With `weight` interpreted as **kg**, `F = weight/100 ≈ 0.6` at 60 kg, and the
first term `scoreWeight·0.6/60` yields sane per-window kcal (≈1 kcal for a big
pattern). But the *unset* branch hardcodes `F = 60.0f` (~100× larger), which would
massively inflate calories for a profile with `weight == 0`. Either the stored
`weight` is pre-scaled (×100) or the unset default is a Konami quirk. This does
**not** block the mod — the round-trip is unit-agnostic (it reads `+0x24`, lets the
user adjust, writes the same integer back, and sends that integer on save; the
server echoes the same units) — but the option's **display range/label should be
calibrated from an observed value** (set a known weight via the web UI, read
`+0x24` at runtime) before shipping. Confirming this is a good Cheat-Engine task.

---

## 4. `today_cal` and the result-screen CalorieTab (context only)

`sequence::result::CalorieTab` (ctor `FUN_1800e9980`, update `FUN_1800e9d90`)
renders the post-play calorie tab. It reads the same per-player struct
(`player_work_table[side]` → `*wrapper`):

- `*(int*)(PlayerWork + 0x24)` — weight (used to gate a "set your weight" prompt
  layer: `_DAT_180359698 < weight`).
- `*(u64*)(PlayerWork + 0x30)` — `today_cal` base accumulator.
- per-stage kcal at `PlayerWork + 0x5dc + stage*0x2b8`.

> ⚠️ The literal strings `"weight"` (@ `0x180367608`) and `"calorie"`
> (@ `0x18036bdc0`) **inside gamemdx** are **AFP layer names** used by this tab
> (`FUN_180257a60(clip,"weight",…)`), *not* the profile storage or the kbin tags.
> The kbin tags live in `ess.dll` (§1). Don't confuse them.

`today_cal`/CalorieTab are documented only for orientation — the mod should not
write them.

---

## 5. Modding strategy (extend WebUI Options)

This is structurally the same problem the cosmetic customizations already solved,
so the extension mirrors `src/mods/webui_options/mod.rs` closely. The only new RE
inputs are the two PlayerWork offsets and the wire tags.

**Chain (same as `seed_registry_from_game` / `try_apply_all`):**
```rust
let wrapper = *player_work_table.add(side);   // player_work_table[side]
let player_work = *(wrapper as *const *const u8);
// weight:         (player_work + 0x24) as *mut i32
// is_disp_weight: (player_work + 0x28) as *mut u8
```

**Register two options** (`custom_options`, `PersistMode::SaveOnly`, like the
cosmetics):
- `weight` → `RegisterSpec::scalar` (integer kg; choose a sane clamp, e.g.
  20–200). `0` is a valid "unset" but the menu will more naturally offer a real
  range; treat display of a read-back `0` however the UX prefers (the game shows
  60 kg behaviour for `0`).
- `is_disp_weight` → `RegisterSpec::enum_values` OFF/ON. The stock ribbon
  textures `seop_op_off` / `seop_op_on` already exist, so no new art is needed.

**Apply on change** (new `try_apply_all`-style writer): write `player_work+0x24`
(as `i32`) and `player_work+0x28` (as `u8`, `0`/`1`). Null-guard the wrapper /
player_work chain exactly like the existing code.

**Seed on SONG_SELECT** (extend `seed_registry_from_game`): read the two offsets
back and `set_value_silent` — the game's own `<common>` load has populated them by
card-in, so the menu reflects the server/web-UI value.

**Save injection** (`PersistMode::SaveOnly`, like `mod_customize_*`): append the
two values to the `playerdata_save` `<option>` block via the custom-options
save path (`custom_options_persistence` `save_sender`), e.g. `mod_weight` (s32)
and `mod_is_disp_weight` (s32, `0`/`1`). No load-side DLL handling — the menu is
seeded from PlayerWork after the game's native `<common>` load applies.

**When changes take effect**: weight is read by the calorie actor at gameplay
start (and per-update), so a song-select edit lands on the next play — consistent
with how the customize writes behave. `is_disp_weight` at `+0x28` is the reflect
target for the toggle; writing it updates the game's stored flag (its display
consumer was not separately traced — see §7).

---

## 6. Server-side persistence mapping

The DLL has a load path already (server → ess → reflect → PlayerWork). To
round-trip an in-game edit, a server needs a **save path**, mapping the injected
save field back to its native `common` column:

| DLL save field (`<option>` child) | native load location | kbin type | PlayerWork |
|-----------------------------------|----------------------|:---------:|:----------:|
| `mod_weight`          | `<common><weight>`         | s32  | +0x24 |
| `mod_is_disp_weight`  | `<common><is_disp_weight>` | bool | +0x28 |

Notes for server authors:
- Values line up **1:1** — the injected number is the same value the game reads
  from `<common>` on the next load. `weight` is a raw kg integer; `is_disp_weight`
  is `0`/`1` (send as bool on load).
- Emit both under `<common>` on `playerdata_load`; the game's own reflect applies
  them (no mod-specific load handling).
- Only write the column when the `mod_*` field is present on save (a stock/un-hooked
  play or a web-UI edit between hooked sessions must not clobber the store), exactly
  as the cosmetic `mod_customize_*` fields already do.
- A web UI, if present, edits the same canonical column, so both sources converge.

*(Exact injected field names are an implementation choice; match whatever the
backend is written to detect. The `mod_<opt>` convention mirrors the existing
`mod_customize_*` scheme.)*

---

## 7. Cross-version notes

| Aspect | 20260324 | 20260616 | Stable? |
|--------|----------|----------|---------|
| ess `common` tag names / offsets (`weight`+0x64, `is_disp_weight`+0x70) | same | same¹ | ✅ |
| PlayerWork `weight` offset | +0x24 | +0x24 | ✅ |
| PlayerWork `is_disp_weight` offset | +0x28 | +0x28 | ✅ |
| PlayerWork `today_cal` offset | +0x30 | +0x30 | ✅ |
| Reflect fn | `FUN_180013c80` | `FUN_180014850` | address shifts |
| Player-work table global | `DAT_1806ebe50` | `DAT_1806f1ee0` | address shifts (mod derives at runtime) |
| Reflect per-side staging stride | `0x101bc8` | `0x101bc8` | ✅ |
| Calorie calc consumer | equivalent | `FUN_180053430` | address shifts |
| `customize_offset` (for reference) | 0x1790 | 0x1790 | ✅ (unchanged) |

¹ ess.dll 20260616 was not in the project; the ess `common` layout was verified on
20260324 and is consistent with the gamemdx-side reflect on 20260616. The ess
customize offset was previously shown stable across 20250805/20260324, so the
`common` layout is expected stable too — re-verify if an ess.dll update lands.

The PlayerWork header offsets `+0x24/+0x28/+0x30` are verified identical on both
gamemdx builds (same as the customize *internal* fields being stable while the
PlayerWork→Customize base offset moves).

### Deriving the offset at runtime (recommended over hardcoding)
`+0x24` is a fixed header offset, but to be update-resilient it can be derived
from the calorie calc `FUN_180053430`, whose weight read is structurally distinct:

```
48 63 41 58              MOVSXD RAX,[RCX+0x58]
48 8D 15 ?? ?? ?? ??     LEA    RDX,[player_work_table]   ; RIP-rel (wildcard)
48 8B 04 C2              MOV    RAX,[RDX+RAX*8]
48 8B 10                 MOV    RDX,[RAX]
66 0F 6E 42 24           MOVD   XMM0,[RDX+0x24]           ; <-- weight displacement (last byte)
0F 5B C0                 CVTDQ2PS XMM0,XMM0
0F 2F 05 ?? ?? ?? ??     COMISS XMM0,[threshold]          ; RIP-rel (wildcard)
```

Anchoring on this sequence lets the DLL decode the `+0x24` displacement (and, as a
bonus, resolve `player_work_table` from the `LEA`). `is_disp_weight` is then
`weight_offset + 4`. Given the confirmed cross-version stability, hardcoding
`+0x24`/`+0x28` is also acceptable — the PE/SDE can decide during design.

---

## 8. Gotchas

- **Not a Customize field.** These are PlayerWork-header fields; do **not** route
  them through `customize_offset` or the category-dispatch setters. Write the raw
  offsets.
- **In-gamemdx `"weight"`/`"calorie"` strings are AFP layer names**, not storage
  and not the kbin tags. The real kbin tags are in `ess.dll`'s `<common>` parser.
- **`weight == 0` means "unset"** (game assumes 60 kg); it is not a literal 0 kg.
  Decide how the option UI presents a read-back `0`.
- **`is_disp_weight` is a single byte** at `+0x28`; write `0`/`1`, not a 4-byte int.
- **`today_cal` (+0x30, u64) is a live accumulator** — leave it alone.
- **Display consumer of `is_disp_weight` not separately traced.** Its identity is
  confirmed by the ess tag + the reflect target; if an in-session toggle of the
  on-screen calorie display proves not to update live, the value still round-trips
  (write → save → server → next card-in load applies it), matching the cosmetics'
  behaviour.
