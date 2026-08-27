# Player Customization System Research

Reverse engineering document covering the **player customization subsystem** in DDR World — the mechanism by which the game applies cosmetic selections (appeal boards, backgrounds, characters, lanes, lane covers, BGM) that are normally set via the web UI.

**Game binaries**: `gamemdx.dll` (20250805 and 20260324), `ess.dll` (both versions)
**Ghidra base**: `0x180000000`
**Tools used**: Ghidra (static analysis)
**Related**: [`custom_player_options_research.md`](./custom_player_options_research.md) (custom *option* injection — a different system)

---

## Overview

Player customization is the set of cosmetic selections (visual skins) that the player configures via the DDR World web portal. When a player scans their card, the server sends a `<customize>` block in the `playerdata_load` response containing an array of `{category, key, pattern}` tuples. The game stores these in a shared memory buffer managed by `ess.dll`, then `gamemdx.dll` reads the buffer and routes each entry through a category-dispatch switch to populate fields in the `ddr::player::Customize` object.

The customization options are **not sent back to the server on save** — the game treats them as read-only values set by the web UI. This means if we modify the values in memory, they persist visually for the current session but don't automatically persist to the server. However, the server could add support for saving these values through the existing `playerdata_save` pathway.

---

## Wire Format

### Server → Game (playerdata_load response)

The `<customize>` block is an array of up to 64 entries:

```xml
<customize>
  <category __type="s32">1</category>
  <key __type="s32">5</key>
  <pattern __type="s32">0</pattern>
</customize>
<customize>
  <category __type="s32">2</category>
  <key __type="s32">3</key>
  <pattern __type="s32">1</pattern>
</customize>
...
```

Each entry is a triplet:
- **category**: Which customization *group* (1–8). Selects the target setter via an 8-entry jump table (index = `category − 1`). `category ≤ 0` terminates the scan; `category > 8` is ignored.
- **key**: The **value written** — the selected asset id (e.g. `appeal_board_0005` → `key = 5`). This is the value argument to every setter. Out-of-range keys are silently dropped (the field keeps its previous/zero value), not clamped-to-max.
- **pattern**: A per-category **sub-selector**. It is meaningful for **only two categories** — category 2 (character player-side) and category 3 (background context). For every other category it is **ignored**.

### Default entries (from server code) — corrected

The `→` target is the **verified** logical field (see the dispatch decode below). Earlier revisions of this doc mislabeled several of these; the corrected mapping is:

```
(cat=1, pat=0, key=1)  → appeal_board       = 1
(cat=2, pat=1, key=1)  → character_p1        = 1   (pat = player-side sub-index: 1=P1)
(cat=2, pat=2, key=2)  → character_p2        = 2   (pat = player-side sub-index: 2=P2)
(cat=3, pat=0, key=1)  → IGNORED (pattern 0 hits neither branch — written by stock server, dropped by game)
(cat=3, pat=1, key=1)  → background          = 1   (pat=1 → result/special-scene context, +0x10)
(cat=3, pat=2, key=1)  → background_gameplay = 1   (pat=2 → normal gameplay context, +0x14)
(cat=4, pat=1, key=1)  → lane_single         = 1   (pattern ignored)
(cat=5, pat=1, key=1)  → lane_double         = 1   (pattern ignored)
(cat=6, pat=1, key=1)  → lane_cover_single   = 1   (pattern ignored)
(cat=7, pat=1, key=1)  → lane_cover_double   = 1   (pattern ignored)
(cat=8, pat=0, key=1)  → movie_size          = 1   (pattern ignored)
```

> The stock server sends `pattern = 1` (or `0`) for categories 1/4/5/6/7/8 out of habit; the game ignores it there, so it is harmless. The `(cat=3, pat=0)` tuple the stock server sends is a genuine dead entry — the game's cat-3 branch only handles `pattern == 1` and `pattern == 2`.

---

## Shared Memory Buffer (ess.dll)

### Parsing (sys_playerdata_load_receiver)

`ess.dll` receives the `playerdata_load` response and stores the customize entries in a per-player shared memory region.

**Offset from player data base** (both 20250805 and 20260324): `+0x5491B0`

**Entry layout** (12 bytes each, confirmed on both versions):

| Offset | Size | Type | Field |
|--------|------|------|-------|
| +0x00 | 4 | i32 | category |
| +0x04 | 4 | i32 | key |
| +0x08 | 4 | i32 | pattern |

**Array capacity**: 64 entries (0x40), total size = 768 bytes (0x300)

**Evidence** (20260324 ess.dll, `FUN_180025d70`):
```asm
; Loop counter check
1800283b3: CMP EBP,0x40
1800283b6: JGE 0x180028491

; Category at base + index*12
1800283c3: MOV dword ptr [RSP + 0x28],0x4
1800283cb: LEA RCX,[R12 + R12*0x2 + 0x15246c]   ; R12=index, computes index*3 + 0x15246c
1800283e4: LEA RDX,[RAX + RCX*0x4]               ; base + (index*3 + 0x15246c)*4 = base + index*12 + 0x5491B0

; Key at base + index*12 + 4
18002840d: LEA RCX,[RAX + R12*0x4 + 0x5491b4]    ; R12 = index*3

; Pattern at base + index*12 + 8
18002844e: LEA RCX,[RAX + R12*0x4 + 0x5491b8]    ; R12 = index*3
```

The `brave` section immediately follows at offset `+0x5494B0` = `0x5491B0 + 64*12`.

---

## ddr::player::Customize Object (gamemdx.dll)

### Class Hierarchy

```
ddr::player::Customize (RTTI confirmed on both versions)
  - 20260324: vtable at 0x180383E98, TypeDescriptor at 0x1804B8890
  - 20250805: vtable at 0x180364CA8, TypeDescriptor at 0x180482720
```

### Location within PlayerWork

The `Customize` object is inlined within `ddr::player::Work` (the per-player state):

| Version | Customize offset in PlayerWork | Option offset in PlayerWork |
|---------|-------------------------------|----------------------------|
| 20260324 | **+0x1790** | +0xE0 |
| 20250805 | **+0x1770** | +0xF0 |

**The PlayerWork struct layout shifted between versions.** The Customize object's offset is NOT stable across versions. It must be found at runtime via RTTI vtable walk on the `ddr::player::Customize` TypeDescriptor, or derived from a version-stable anchor.

### Customize Object Internal Layout

The fields within the Customize object itself ARE stable across versions (same vtable slot functions, same relative offsets):

| Offset | Size | Field | Category | Pattern | Max Value | Asset Naming |
|--------|------|-------|----------|---------|-----------|--------------|
| +0x00 | 8 | vtable pointer | — | — | — | — |
| +0x08 | 4 | (padding/reserved) | — | — | — | — |
| +0x0C | 4 | appeal_board | 1 | 0 | 103 (+ 100001–100007) | `appeal_board_%04d` |
| +0x10 | 4 | background | 3 | 1 | 52 | `background_%04d` (result/special scene context) |
| +0x14 | 4 | background_alt | 3 | 2 | 52 | `background_%04d` (normal gameplay context) |
| +0x18 | 4 | character_1p | 2 | 1 | 59 | `character_%04d_1p` |
| +0x1C | 4 | character_2p | 2 | 2 | 59 | `character_%04d_2p` |
| +0x20 | 4 | lane_single | 4 | (1) | 75 | `lane_single_%04d` |
| +0x24 | 4 | lane_double | 5 | (1) | 91 | `lane_double_%04d` |
| +0x28 | 4 | lane_cover_single | 6 | (1) | 88 | `lane_cover_single_%04d` |
| +0x2C | 4 | lane_cover_double | 7 | (1) | 106 | `lane_cover_double_%04d` |
| +0x30 | 4 | movie_size | 8 | 0 | 3 | N/A (selects Flash layer: 0–1=fullscreen, 2–3=sized) |
| +0x34 | 4 | (dormant custom_bgm_id) | — | — | 10 | Getter stubbed to always return 1 |

Each setter enforces a per-field **accept ceiling** (reject-and-skip, not saturate): a `key` above the ceiling is dropped and the field is left unchanged. These ceilings **grow with almost every game update** (each release ships more customizer assets), so the exact numbers are deliberately **not pinned** here and should not be hardcoded in the DLL or a server — treat them as "whatever the current build allows". The `Max Value` column above is illustrative only. (Appeal board additionally accepts the special range `100001`–`100007`.)

### Getter/Setter Vtable

Each field has a paired getter and setter in the vtable. Getters return the stored value, substituting `1` when it is `0` (so "no selection" reads as item 1; the character P2 getter defaults to `2`). The offset each **setter** writes was verified by disassembly (20260324); this is the authoritative slot → offset → field map:

| Vtable Offset | Slot | Setter (20260324) | Writes offset | Field | Params |
|---------------|------|-------------------|---------------|-------|--------|
| +0x18 | 3 | `FUN_1801ddf90` | **+0x0C** | appeal_board | (this, val) |
| +0x28 | 5 | `FUN_1801ddfc0` | **+0x10** | background (result/special) | (this, val) |
| +0x38 | 7 | `FUN_1801ddfe0` | **+0x14** | background_gameplay (normal) | (this, val) |
| +0x48 | 9 | `FUN_1801de010` | **+0x18** (sub 1) / **+0x1C** (sub 2) | character_p1 / character_p2 | (this, sub_index, val) |
| +0x58 | 11 | `FUN_1801de040` | **+0x20** | lane_single | (this, val) |
| +0x68 | 13 | `FUN_1801de060` | **+0x24** | lane_double | (this, val) |
| +0x78 | 15 | `FUN_1801de080` | **+0x28** | lane_cover_single | (this, val) |
| +0x88 | 17 | `FUN_1801de0a0` | **+0x2C** | lane_cover_double | (this, val) |
| +0x98 | 19 | `FUN_1801de0c0` | **+0x30** | movie_size | (this, val) |
| +0xa8 | 21 | `FUN_1801de0d0` | **+0x34** | (dormant bgm) | (this, val) |

Verified getters (paired):

| Vtable Offset | Slot | Getter (20260324) | Reads offset | Notes |
|---------------|------|-------------------|--------------|-------|
| +0x10 | 2 | `FUN_1801ddf80` | +0x0C | appeal_board; returns 1 if 0 |
| +0x20 | 4 | `FUN_1801ddfb0` | +0x10 | background; returns 1 if 0 |
| +0x40 | 8 | `FUN_1801ddff0` | +0x18 / +0x1C | character; 2-param (sub_index); defaults P1→1, P2→2 |
| +0x90 | 18 | `FUN_1801de0b0` | +0x30 | movie_size; returns 1 if 0 |
| +0xa0 | 20 | `FUN_18025db80` | — | bgm getter is **stubbed** (`MOV EAX,1; RET`) — confirms +0x34 is dormant |

Disassembly evidence:

```asm
; +0x18 appeal_board -> +0x0C  (FUN_1801ddf90)
CMP EDX,0x66 ; JBE .w ; LEA EAX,[RDX-0x186A1] ; CMP EAX,0x6 ; JA .ret
.w: MOV [RCX+0xC],EDX          ; +0x0C

; +0x28 background -> +0x10    ; +0x38 background_gameplay -> +0x14
CMP EDX,<max> ; JA .ret ; MOV [RCX+0x10],EDX
CMP EDX,<max> ; JA .ret ; MOV [RCX+0x14],EDX

; +0x48 character (2-param) -> +0x18 / +0x1C  (FUN_1801de010)
CMP EDX,1 ; JNZ .a ; XOR EAX,EAX ; JMP .b       ; sub_index 1 -> idx 0 (+0x18)
.a: CMP EDX,2 ; JNZ .ret ; LEA EAX,[RDX-1]      ; sub_index 2 -> idx 1 (+0x1C)
.b: CMP R8D,<max> ; JA .ret ; CDQE ; MOV [RCX+RAX*4+0x18],R8D
```

**Correction to earlier revisions of this doc:** the slot labels were shifted by one field. Slot +0x38 (`FUN_1801ddfe0`) is **background_gameplay**, not "character"; the real character setter is the 2-param slot **+0x48**. Slots +0x58/+0x68 are the **lanes** (single/double), and +0x78/+0x88 are the **lane covers** — the earlier "unknown_6/unknown_7" were lane covers all along. Slot +0x98 is **movie_size**, not "bgm"; the true bgm field (+0x34) is unreachable (no category maps to it and its getter is stubbed).

(Addresses are for 20260324; other versions have equivalent functions at different addresses but identical logic. The DLL never uses these vtable slots — it writes the struct offsets directly — so this table is documentation, not a hook dependency.)

---

## Category Dispatch (gamemdx.dll)

### Dispatch Function

`FUN_180013C80` (20260324) is the large `ark::network::ReflectPlayerWork` player-init routine; the customize apply is an **inlined section** within it:

- **switch head** @ `0x180016680`
- **jump table** (8 × int32 RVA, added to image base) @ `0x180016D94`
- **loop tail** @ `0x1800167E0`

The setters are only reached through the `ddr::player::Customize` vtable (`PlayerWork + 0x1790`), so there is no direct xref to them — the dispatch was located by byte-searching for the distinctive 2-param character vcall `FF 50 48` (`call [rax+0x48]`), which lands inside this region.

### Dispatch Logic (verified, 20260324)

The loop reads customize entries (stride `0xC`, up to `0x40`), stops when `category ≤ 0`, and switches on `category` (jump-table index = `category − 1`, `category > 8` ignored):

```c
// entry+0 = category, entry+4 = key, entry+8 = pattern
Customize *c = PlayerWork + 0x1790;
switch (category) {
  case 1:  c->vtable[+0x18](c, key);                 break;  // appeal_board   -> +0x0C
  case 2:  c->vtable[+0x48](c, /*sub=*/pattern, key); break;  // character p1/p2 (pattern=1->+0x18, 2->+0x1C)
  case 3:                                                     // background, context by pattern
           if      (pattern == 1) c->vtable[+0x28](c, key);   //   background          -> +0x10
           else if (pattern == 2) c->vtable[+0x38](c, key);   //   background_gameplay -> +0x14
           //  pattern 0/other -> no write
           break;
  case 4:  c->vtable[+0x58](c, key);                 break;  // lane_single       -> +0x20  (pattern ignored)
  case 5:  c->vtable[+0x68](c, key);                 break;  // lane_double       -> +0x24  (pattern ignored)
  case 6:  c->vtable[+0x78](c, key);                 break;  // lane_cover_single -> +0x28  (pattern ignored)
  case 7:  c->vtable[+0x88](c, key);                 break;  // lane_cover_double -> +0x2C  (pattern ignored)
  case 8:  c->vtable[+0x98](c, key);                 break;  // movie_size        -> +0x30  (pattern ignored)
}
```

Raw bytes of the cat-2 / cat-3 cases (`0x1800166D8`), confirming the argument wiring:

```asm
; case 2 (character):
48 8B 0B              MOV  RCX,[RBX]              ; PlayerWork
...
48 81 C1 90 17 00 00  ADD  RCX,0x1790            ; RCX = Customize this
48 8B 01              MOV  RAX,[RCX]              ; vtable
44 8B C2              MOV  R8D,EDX                ; R8D = key
8B D7                 MOV  EDX,EDI                ; EDX = pattern  (sub_index)
FF 50 48              CALL [RAX+0x48]             ; character(this, sub=pattern, val=key)
; case 3 (background):
83 FF 01              CMP  EDI,1                  ; pattern == 1 ?
75 20                 JNZ  .try2
...
FF 50 28              CALL [RAX+0x28]             ; background          (+0x10)
.try2:
83 FF 02              CMP  EDI,2                  ; pattern == 2 ?
0F 85 ..              JNZ  .default               ; else skip
...
FF 50 38              CALL [RAX+0x38]             ; background_gameplay (+0x14)
```

---

## Asset Structure

Assets live under `data/arc/custom/<type>/` in the game's file system:

| Category | Path Format | Notes |
|----------|-------------|-------|
| appeal_board | `data/arc/custom/appeal_board/appeal_board_%04d.arc` | Also `_result` variant |
| background | IFS directories: `background_%04d_ifs/` | Contains animation data |
| character | `data/arc/custom/%s/%s_%04d_%s.arc` | `%s` = suffix `1p`/`2p`, also `_result` variants |
| lane_single | `data/arc/custom/%s/%s_%04d.arc` | General format |
| lane_double | `data/arc/custom/%s/%s_%04d.arc` | General format |
| lane_cover | `lane_cover_%s` → `single` or `double` | Used as AFP MovieClip name for bitmap load |
| bgm | `data/custom/bgm/custom_bgm_%04d` | Audio |

### Asset ID Ranges (from unpacked assets)

| Type | Regular Range | Special IDs | Total Assets |
|------|--------------|-------------|-------------|
| appeal_board | 0001–0103 | 100001–100007 | 111 textures × 2 (normal + result) |
| background | 0001–0051 | — | 52 IFS dirs |
| character | 0001–0058 | 9000–9001 | 60 chars × 4 (1p/2p × normal/result) |
| lane_single | 0001–0074 | 9000–9001 | 76 textures |
| lane_double | 0001–0090 | 9000–9001 | 92 textures |
| lane_cover_single | 0001–0087 | 9000–9001 | 89 textures |
| lane_cover_double | 0001–0105 | 9000–9001 | 107 textures |

IDs 9000+ appear to be reserved/special items (possibly subscriber exclusives or event rewards).

---

## Accessing Customize at Runtime (Modding Strategy)

### Finding the Customize object

The existing hook infrastructure already resolves `player_work_table` (via `player_work_table_anchor` signature). The pointer chain is:

```
player_work_table[playSide]  →  wrapper*
wrapper[+0x00]               →  PlayerWork*
PlayerWork[+CUSTOMIZE_OFFSET] =  Customize (inlined)
```

**CUSTOMIZE_OFFSET varies by version:**
- 20260324: `0x1790`
- 20250805: `0x1770`

### Runtime version detection

The offset can be determined at init time by:
1. **RTTI walk**: Scan for `ddr::player::Customize` TypeDescriptor → find vtable → find constructor that writes the vtable → extract the offset from the LEA/MOV that writes to `param_1 + offset`.
2. **Signature on the dispatch function**: AOB-scan the category switch in `FUN_180013c80` (20260324) / equivalent in 20250805, decode the `ADD` or `LEA` instruction that computes `PlayerWork + customize_offset`.

### Writing values

To change a customization at runtime:
1. Get `PlayerWork*` from the player_work_table chain
2. Add the version-detected Customize offset to get the Customize base
3. Write the desired ID to the appropriate field offset (`+0x0C` for appeal_board, etc.)

The game reads these values on demand (when rendering the corresponding UI element), so a write takes effect on the next frame/scene that references the field.

### Persistence

The game **never sends customize data back** on `playerdata_save` — stock setups have no in-game way to change these values, so the game treats the load-time `<customize>` block as authoritative and read-only.

This modpack changes selections in-game (WebUI Options). The **native profile fields are the single source of truth**; the DLL adds only the direction the game lacks (game → server on save):

1. **In-memory apply (user edit only)** — when the player changes a value in the options menu, `webui_options` writes the chosen asset id straight into the `Customize` struct offsets (bypassing the category dispatch entirely). This is the *only* writer of `Customize` besides the game's own load; there is no scene-entry re-apply.
2. **Save injection** — on save, the DLL appends `<mod_customize_*>` s32 children (named by logical option id, one per row) to the `playerdata_save` `<option>` block. The value sent is the **stable asset id** (index → id via `persist_save_transform`), i.e. the same number the game's `key` field carries. The server writes these into its native customize columns.
3. **Load (game-native)** — the DLL does **not** read customize values back from the server, and does not JSON-cache them. The server emits the stored values in the stock `<customize>` block, the game applies them to the `Customize` object itself on card-in, and the DLL **seeds its options-menu state by reading the `Customize` object** at every SONG_SELECT (scene 25) entry — a strictly read-only, silent (no on-change dispatch) reverse lookup of asset id → menu index. An asset id not present on the cabinet displays as item 1 but is never written back, so the server's value survives.

For a server to round-trip these properly (so they come back down in the stock `<customize>` block on the next card-in), it must map each injected `mod_customize_*` field to the correct `(category, pattern)` tuple. **See [Server-Side Persistence Mapping](#server-side-persistence-mapping) below** — this is the canonical mapping table and is the required integration for any private server, whether or not it also has a web UI.

---

## Server-Side Persistence Mapping

This is the practical payoff of the dispatch decode: how a server should translate between the DLL's injected `mod_customize_*` save fields and the stock `<customize>` load block.

### The wire values line up 1:1

The DLL sends the **asset id** as the `mod_customize_*` value, and the game's `<customize>` `key` field **is** that same asset id (the value written into the `Customize` field). So **no numeric transform is needed** — the injected value is used verbatim as `key`. The only thing the server must supply is the correct `(category, pattern)` pair for each logical option.

### Injected field → `(category, pattern, key)` map

| DLL save field (`<option>` child) | category | pattern | `key` = value | Customize offset |
|-----------------------------------|:--------:|:-------:|---------------|:----------------:|
| `mod_customize_appeal_board`       | 1 | 0 (ignored) | asset id | +0x0C |
| `mod_customize_character_p1`       | 2 | **1** | asset id | +0x18 |
| `mod_customize_character_p2`       | 2 | **2** | asset id | +0x1C |
| `mod_customize_background`         | 3 | **1** | asset id | +0x10 |
| `mod_customize_background_gameplay`| 3 | **2** | asset id | +0x14 |
| `mod_customize_lane_single`        | 4 | 1 (ignored) | asset id | +0x20 |
| `mod_customize_lane_double`        | 5 | 1 (ignored) | asset id | +0x24 |
| `mod_customize_lanecover_single`   | 6 | 1 (ignored) | asset id | +0x28 |
| `mod_customize_lanecover_double`   | 7 | 1 (ignored) | asset id | +0x2C |
| `mod_customize_movie_size`         | 8 | 0 (ignored) | value 1–3 | +0x30 |

Notes for server authors:
- **`pattern` matters only for categories 2 and 3.** For 2 it is the player side (`1`=P1, `2`=P2); for 3 it is the background context (`1`=result/special, `2`=normal gameplay). For every other category the game ignores `pattern`, so send `0` or `1` — either is fine.
- **`movie_size`** is a small enum, not a discovered asset id: the DLL sends `1`=fullscreen, `2`=on, `3`=off (its in-game rows are index 0/1/2, mapped to `key` 1/2/3 before sending). Pass the value through as `key` for category 8.
- **Emit all applicable `<customize>` entries on load**, keyed by `(category, pattern)`. The stock game also sends a `(cat=3, pat=0)` entry — it is a no-op (the cat-3 branch only handles `pattern` 1 and 2); you may keep it for parity or drop it (bemani-buddy drops it).
- **Do not clamp on the server.** Accept ceilings live in the game and grow every update; let the game reject anything it can't handle. Storing an id the current build rejects simply leaves that field at its default until a build that supports it.
- **Do not echo `mod_customize_*` back in the `<option>` block on load.** The DLL never reads them — it seeds its in-game menu from the game's own `Customize` object after the native `<customize>` load has applied. An echo is at best dead weight and at worst a second writer.

### Required server behavior (single source of truth)

Store a single canonical per-option value — the native customize store your load path already emits — and wire the save path into it:

- **On `playerdata_save`**: if a `mod_customize_<opt>` child is present under `<option>`, write its value into the canonical store for `<opt>`. (These arrive only from a cabinet running this mod; the "only when present" guard means an un-hooked play or a web-UI edit made between hooked sessions never clobbers the store.)
- **On `playerdata_load`**: build the `<customize>` array from the canonical store using the `(category, pattern)` map above. Nothing mod-specific appears in the load response.

A web UI (if the server has one) edits the same canonical store, so both sources converge by construction — there is no second copy to reconcile.

### Reference: bemani-buddy column scheme

`bemani-buddy` stores the canonical values in semantically-named `cust_*` columns on the profile row (renamed from the historical `cust_<category>_<pattern>` scheme in migration `010`) and rebuilds the load block from them via a fixed `(category, pattern)` key table. The injected fields map onto those columns as:

| `mod_customize_*` field | → column | (cat, pat) emitted on load |
|-------------------------|----------|:--------------------------:|
| `appeal_board`          | `cust_appeal_board` | (1, 0) |
| `character_p1`          | `cust_character_p1` | (2, 1) |
| `character_p2`          | `cust_character_p2` | (2, 2) |
| `background`            | `cust_background` | (3, 1) |
| `background_gameplay`   | `cust_background_gameplay` | (3, 2) |
| `lane_single`           | `cust_lane_single` | (4, 1) |
| `lane_double`           | `cust_lane_double` | (5, 1) |
| `lanecover_single`      | `cust_lanecover_single` | (6, 1) |
| `lanecover_double`      | `cust_lanecover_double` | (7, 1) |
| `movie_size`            | `cust_movie_size` | (8, 0) |

Writing the injected value into the matching `cust_*` column on save is all that's required — the existing load path already emits those columns as the `<customize>` block, so the selection round-trips through the game's own profile load. (The inert `(3, 0)` tuple and the interim `opt_mod_customize_*` echo columns were dropped in the same migration.)

---

## Cross-Version Notes

| Aspect | 20250805 | 20260324 | Stable? |
|--------|----------|----------|---------|
| Customize RTTI string | Same | Same | ✅ Yes |
| Customize field layout (+0x0C..+0x34) | Same | Same | ✅ Yes |
| Customize vtable slot assignments | Same | Same | ✅ Yes |
| PlayerWork→Customize offset | 0x1770 | 0x1790 | ❌ No — must detect |
| ess.dll shared buffer offset | 0x5491B0 | 0x5491B0 | ✅ Yes |
| Category dispatch logic (category/pattern → field) | Same | Same | ✅ Yes |
| Wire protocol (`category`/`key`/`pattern` semantics) | Same | Same | ✅ Yes |
| Setter accept ceilings | grow per release | grow per release | ⚠️ Version-specific — do not pin |

> **20260526 (live-validated, not fully RE'd):** the runtime offset detection
> resolved `customize_offset = 0x1790` (same as 20260324) and the full chain —
> server `<customize>` load → category dispatch → `Customize` fields → the
> mod's read-back — was confirmed working on-cabinet for all 10 categories
> (2026-07-11 native-persistence test, values seeded from the DB and displayed
> correctly in-game).

---

## Open Questions (resolved)

Previously-open items, now answered by the dispatch decode:

1. ~~**Categories 6 and 7**~~ — **Resolved:** cat 6 = `lane_cover_single` (+0x28), cat 7 = `lane_cover_double` (+0x2C). They were mislabeled "unknown" only because the earlier vtable-slot table was shifted.
2. ~~**Slot 21 / +0x34**~~ — **Resolved:** this is the dormant `bgm` field. Its setter (slot +0xa8) exists but **no category in the switch reaches it**, and its getter (slot +0xa0) is stubbed to always return `1`. It is doubly dormant — the server can't drive it and its value is ignored.
3. ~~**Category 3, pattern 0**~~ — **Resolved:** silently ignored. The cat-3 branch only handles `pattern == 1` (background) and `pattern == 2` (background_gameplay); `pattern 0` falls through with no write.
4. **Accept-ceiling patching** — Still open, but out of scope for persistence: to *select* an id above the current build's ceiling you'd NOP/patch the setter bound check. Not needed for the server-mapping goal, and ceilings grow each release anyway.
5. **ARC loading trigger** — Still open: exactly when the game loads the ARC for a written field (immediate vs deferred to the referencing scene). The mod writes `Customize` only on a user edit in the options menu (song select), which is sufficient in practice.

---

## Gotchas

- **Version-dependent PlayerWork offset**: Never hardcode `0x1790` or `0x1770`. Detect at runtime.
- **Getter substitution**: Getters return `1` when the stored value is `0` (character P2 returns `2`). "No selection" displays as item 1, so setting `0` is equivalent to setting `1`.
- **The 2-param setter is `character`, not `lane`**: The character setter (vtable **+0x48**) takes `(sub_index, value)` where `sub_index` 1=P1, 2=P2. The lanes are separate single-param setters (+0x58 single, +0x68 double). An earlier revision of this doc had these swapped.
- **`category` selects the field group; `pattern` sub-selects only for cat 2 (player side) and cat 3 (background context)**; `key` is always the value written.
- **Accept ceilings grow every update**: setters reject (don't saturate) out-of-range keys. Don't pin these numbers anywhere — in the DLL or a server.
- **Shared buffer is read-only from gamemdx's perspective**: customize data flows one-way (ess.dll shared buffer → gamemdx Customize object) via the dispatch. To change values at runtime, write the `Customize` struct offsets directly (what the mod does); the dispatch only re-runs on a fresh load.
