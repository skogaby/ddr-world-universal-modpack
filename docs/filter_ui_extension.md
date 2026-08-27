# Filter UI Extension

Reverse engineering document for DDR World's VERSION filter UI — how filter entries are rendered, what textures they use, and how to inject new ones.

**Game binary**: `gamemdx.dll` (MDX-003_20260324)
**Ghidra base**: `0x180000000`
**Prerequisite**: [series_filter_internals.md](series_filter_internals.md)

---

## Summary

The filter UI uses Konami's **BM2D MovieClip** system (Flash-like animation framework). Each filter entry is a MovieClip instance created from a `"filter_item"` template. The entry's label texture is set on a child element named `"item_usr"` using the naming convention `sefi_item_{key}`, where `{key}` is the entry's internal key string (e.g., `"world"`, `"a20plus"`).

Textures are loaded from ARC files in `data/arc/bm2d/`. Custom ARCs containing new textures can be piggyback-loaded through the game's asset loader.

---

## BM2D MovieClip Rendering System

The filter UI is built entirely with BM2D MovieClips — not native text/image widgets. MovieClips are the game's own UI framework, managed by `libafp-win64.dll`.

### Key BM2D Ordinals (from libafp-win64.dll)

| Ordinal | Purpose | Used For |
|---------|---------|----------|
| `Ordinal_43` | Set color/tint | Filter item styling |
| `Ordinal_56` | Set visibility | Show/hide filter items |
| `Ordinal_103` | Find child by name | Find `"item_usr"`, `"choices_usr"` children |
| `Ordinal_106` | Get next sibling | Iterate child elements |
| `Ordinal_112` | Set texture/bitmap | Apply `sefi_item_{key}` texture to child |
| `Ordinal_114` | Set frame label | Animation states: `"in"`, `"loop_select"`, `"loop_switch"` |

### Filter Item Rendering (`FUN_180132750`)

**Ghidra address**: `0x180132750`

Each FilterButton instance calls this function to render. The flow:

1. Finds a free slot in the BM2D object pool (`DAT_1806f2180`, stride 0x48, max 0x400 entries)
2. Instantiates a `"filter_item"` MovieClip template from the loaded BM2D assets
3. Positions the MovieClip based on the entry's grid coordinates
4. Constructs the texture name: `sprintf(buf, "sefi_item_%s", entry_key)`
5. Finds all `"item_usr"` children in the MovieClip via `Ordinal_103`
6. Sets the texture on each child via `Ordinal_112`
7. Sets the animation frame label (`"in"`, `"loop_select"`, or `"loop_switch"`)

### Filter Category Panel (`FUN_1801343a0`)

**Ghidra address**: `0x1801343a0`

Each filter category (VERSION, DIFFICULTY, etc.) uses this function:

1. Creates a `"filter_switch_base%02d"` MovieClip (where `%02d` is the category index)
2. Sets the category label texture: `sprintf(buf, "sefi_%s", category_key)` on `"choices_usr"` children
3. Iterates the category's filter entries (from a vector at object+0x1b0) and calls each entry's render function

---

## Texture Naming Convention

Filter textures use the `sefi_` prefix (short for "select filter"). There are two layers:

### Outer Category Buttons (in lang_eng IFS)
Format: `sefi_item_%s` where `%s` is the category key. These are the top-level filter menu labels.
- `sefi_item_version`, `sefi_item_difficulty`, `sefi_item_genre`, `sefi_item_bpm`, etc.
- Located in `select_music_option_lang_eng_v3.arc/.ifs`

### Inner Version Filter Entries (in main IFS)
Format: `sefi_version_%s` where `%s` is the entry's internal key from the static table.
- `sefi_version_world`, `sefi_version_a20plus`, `sefi_version_1th5th`, etc.
- Located in `select_music_option_v3.arc/.ifs`
- These are the green text labels with transparent background shown in the VERSION filter panel

### Group Tab Labels (in main IFS)
- `sefi_version_classic`, `sefi_version_white`, `sefi_version_gold`
- Same IFS as the version entries

### Mark Textures (generic, in main IFS)
- `sefi_mark_on` — shown when a filter entry is selected
- `sefi_mark_off` — shown when deselected
- These are NOT per-series — the same on/off textures are used for all entries

### Other Format Strings in Code
| Format String | Purpose | Example |
|--------------|---------|---------|
| `sefi_item_%s` | Outer category button label | `sefi_item_version` |
| `sefi_mark_%s` | Generic mark (on/off only) | `sefi_mark_on` |
| `sefi_%s` | Category panel header | `sefi_version` |
| `sefi_switch_mark_%s` | Group tab indicator | `sefi_switch_mark_gold` |

**Note**: The `sefi_version_%s` format for inner entries is NOT a format string in the game code — it is constructed by the BM2D MovieClip template (`filter_item` in the `afp/` directory). The template's child elements reference textures using the `sefi_version_` prefix combined with the entry key.

---

## Version Filter Entry Table (Complete)

**Base address (Ghidra)**: `0x180CEF1F0`
**Entry stride**: `0x88` bytes
**Entry count**: 10 (9 real + 1 sentinel)

### Entry Structure (0x88 bytes)

| Offset | Type | Field | Description |
|--------|------|-------|-------------|
| +0x00 | u32 | `group_index` | Cabinet generation group (0=CLASSIC, 1=CLASSIC, ..., 6=GOLD, 7=GOLD, 8=GOLD) |
| +0x08 | std::string | `key` | Internal key, used for texture names (`"world"`, `"a20plus"`) |
| +0x30 | u32 | `series_start` | First series value in this entry's range (inclusive) |
| +0x38 | std::string | `code` | Short display code (`"WORLD"`, `"A20"`) |
| +0x60 | std::string | `display` | Full display name (`"WORLD"`, `"A20+"`) |

### Complete Entry Data

| Idx | Key | Series | Code | Display | Group | Cabinet |
|-----|-----|--------|------|---------|-------|---------|
| 0 | `1th5th` | 1 | 1st | 5th | 0 | CLASSIC |
| 1 | `maxex` | 6 | MAX | EXTREME | 1 | CLASSIC |
| 2 | `novanova2` | 9 | SuperNOVA | SuperNOVA2 | 2 | CLASSIC |
| 3 | `x` | 11 | X | X3 VS 2ndMIX | 3 | CLASSIC |
| 4 | `1314` | 14 | 2013 | 2014 | 4 | WHITE |
| 5 | `a` | 17 | A | A | 5 | WHITE |
| 6 | `a20plus` | 18 | A20 | A20+ | 6 | GOLD |
| 7 | `a3` | 20 | A3 | A3 | 7 | GOLD |
| 8 | `world` | 21 | WORLD | WORLD | 8 | GOLD |
| 9 | `-` | 22 | *(empty)* | *(empty)* | 9 | *(sentinel)* |

### UI Display Format

The filter UI shows entries as:
- `"code – display"` when code ≠ display (e.g., "A20 – A20 PLUS", "MAX – EXTREME")
- `"code"` alone when code == display (e.g., "WORLD", "A3")

The actual rendered text comes from the `sefi_item_{key}` texture, not from the code/display strings directly. The strings are used for other purposes (sorting, logging, accessibility).

---

## Cabinet Generation Groups

**Base address (Ghidra)**: `0x180CECDC0`
**Entry stride**: `0x30` bytes
**Entry count**: 4 (3 real + 1 sentinel)

| Index | Key | Series Start | Filter Entries | Tab Label |
|-------|-----|-------------|---------------|-----------|
| 0 | `classic` | 0 | 0–3 | GROUP CLASSIC |
| 1 | `white` | 4 | 4–5 | GROUP WHITE |
| 2 | `gold` | 6 | 6–8 | GROUP GOLD |
| 3 | `-` | 9 | *(sentinel)* | — |

The GROUP tabs filter which version entries are visible. Selecting GROUP GOLD shows only entries 6–8 (A20–A20 PLUS, A3, WORLD).

Custom series entries assigned to the GOLD group (group index 8, continuing the pattern) appear alongside WORLD. This is the simplest approach and makes sense semantically — custom content is "current generation."

---

## Filter Entry Registration Flow

During filter init (`FUN_18011eda0`), version filter entries are registered through this call chain:

```
FUN_180127ca0(filter_manager + 0x1F8, &filter_index)  → get/create filter slot
FUN_180128230(slot, &entry_data)                        → populate entry with data
```

The filter manager at `param_1 + 0x1F8` holds a collection (likely `std::map<int, FilterEntry>`) keyed by filter index. Each entry contains the key string, series range, code/display strings, and group index.

The UI reads this collection to create FilterButton instances. The collection is built once during filter init and not modified afterward.

---

## Texture Asset Requirements for Custom Series

To add a custom series filter entry (e.g., "WORLD PLUS" with key `worldplus`):

1. **`sefi_version_worldplus`** — The filter entry label texture (required)
   - This is what appears in the VERSION filter panel
   - Should match the visual style of existing entries (green text on transparent background)
   - Located in `select_music_option_v3.arc/.ifs` for vanilla entries

2. **No per-series mark texture needed** — The game uses generic `sefi_mark_on` / `sefi_mark_off` for all entries

3. **Package as ARC**: Textures should be packaged into an ARC file containing an IFS with the texture in DXT5 format

4. **Load via the game's ARC loader** at runtime

### Verified Texture Names (from `select_music_option_v3.arc/.ifs`)

| Entry | Key | Texture Name |
|-------|-----|-------------|
| 1st – 5thMIX | `1th5th` | `sefi_version_1th5th` |
| MAX – EXTREME | `maxex` | `sefi_version_maxex` |
| SuperNOVA – SuperNOVA2 | `novanova2` | `sefi_version_novanova2` |
| X – X3 VS 2ndMIX | `x` | `sefi_version_x` |
| 2013 – 2014 | `1314` | `sefi_version_1314` |
| A | `a` | `sefi_version_a` |
| A20 – A20 PLUS | `a20plus` | `sefi_version_a20plus` |
| A3 | `a3` | `sefi_version_a3` |
| WORLD | `world` | `sefi_version_world` |
| GROUP CLASSIC | `classic` | `sefi_version_classic` |
| GROUP WHITE | `white` | `sefi_version_white` |
| GROUP GOLD | `gold` | `sefi_version_gold` |

---

## Addresses Quick Reference

| Symbol | Ghidra Address | Offset | Description |
|--------|---------------|--------|-------------|
| `filter_item_render` | `0x180132750` | `+0x132750` | FilterButton render/update function |
| `filter_mark_render` | `0x180133219` | `+0x133219` | LEA for `sefi_mark_%s` |
| `filter_category_render` | `0x1801343A0` | `+0x1343A0` | Filter category panel builder |
| `filter_init` | `0x18011EDA0` | `+0x11EDA0` | Filter system initialization (hook target) |
| `filter_register_slot` | `0x180127CA0` | `+0x127CA0` | Get/create filter entry slot |
| `filter_populate_version` | `0x180128230` | `+0x128230` | Populate version filter entry |
| `version_filter_entries` | `0x180CEF1F0` | `+0xCEF1F0` | Static version filter entry array |
| `cabinet_group_entries` | `0x180CECDC0` | `+0xCECDC0` | Static cabinet generation group array |
| `bm2d_object_pool` | `0x1806F2180` | `+0x6F2180` | BM2D MovieClip object pool |

### Format Strings

| Address (Ghidra) | String | Used By |
|-------------------|--------|---------|
| `0x18036F5C0` | `sefi_item_%s` | FilterButton render |
| `0x18036F5E0` | `sefi_mark_%s` | Filter mark render |
| `0x18036F648` | `sefi_%s` | Filter category render |
| `0x18036F680` | `sefi_switch_mark_%s` | Group tab render |
| `0x18036F628` | `filter_switch_base%02d` | Filter category panel template |

---

## Version Filter Predicate — Full Disassembly (`FUN_1801235e0`)

**Ghidra address**: `0x1801235e0`

This function determines whether a song matches the currently selected version filter entries. It is the **only function that reads series ranges from the static table at runtime**.

```asm
; Function prologue
1801235e0: MOV  [RSP+0x10], RBX
1801235e5: PUSH RDI
1801235e6: SUB  RSP, 0x20
1801235ea: MOV  RBX, RCX              ; RBX = param_1 (filter context)
1801235ed: MOV  RCX, RDX              ; RCX = param_2 (song metadata)
1801235f0: CALL 0x1800ff7b0           ; series_mapper(song) → mapped series in EAX
1801235f5: MOV  EDX, [RBX+0x8]       ; EDX = filter category sub-index
1801235f8: MOV  RCX, [RBX]           ; RCX = filter manager base
1801235fb: MOV  [RSP+0x30], EDX
1801235ff: LEA  RDX, [RSP+0x30]
180123604: ADD  RCX, 0x378            ; RCX = filter_manager + 0x378 (selected entries collection)
18012360b: MOV  EDI, EAX             ; EDI = mapped series value
18012360d: CALL 0x1801d3f50           ; get selected entries linked list for this category
180123612: MOV  RDX, [RAX+0x8]       ; RDX = list sentinel node
180123616: MOV  RCX, [RDX]           ; RCX = first node
180123619: CMP  RCX, RDX             ; empty list check
18012361c: JZ   0x180123654           ; if empty → return false

; *** THE KEY INSTRUCTION — loads table base into R8 ***
18012361e: LEA  R8, [0x180cef1f0]    ; R8 = version_filter_entries table base

; Loop: iterate selected entries
180123630: MOVSXD RAX, [RCX+0x10]    ; RAX = entry index (from linked list node)
180123634: IMUL RAX, RAX, 0x88       ; RAX = index * stride
18012363b: CMP  [RAX+R8+0x30], EDI   ; entry[i].series_start <= mapped_series?
180123640: JG   0x18012364c           ; if start > mapped → skip
180123642: CMP  EDI, [RAX+R8+0xB8]   ; mapped_series < entry[i+1].series_start?
18012364a: JL   0x180123661           ; if yes → MATCH (return true)
18012364c: MOV  RCX, [RCX]           ; next node
18012364f: CMP  RCX, RDX             ; end of list?
180123652: JNZ  0x180123630           ; continue loop

; No match
180123654: XOR  AL, AL               ; return false
180123660: RET

; Match found
180123661: MOV  AL, 0x1              ; return true
18012366d: RET
```

### Critical Insight: Single Table Base Reference

The `LEA R8, [0x180cef1f0]` at `0x18012361e` is the **only place** the predicate references the table base. All subsequent accesses use `[RAX + R8 + offset]`. This means:

**Patching the RIP-relative displacement in this single LEA instruction redirects ALL table lookups to a new table.**

The instruction encoding is `4C 8D 05 <disp32>` (7 bytes). The displacement is at bytes 3-6 of the instruction (address `0x180123621`). Current displacement: `0x00BCBBCB` (points to `0x180CEF1F0`).

To redirect to a new table at address `NEW_TABLE`:
```
new_disp = NEW_TABLE - 0x180123625  (next instruction address)
```

### Range Check Logic

The predicate checks: `entry[i].series_start <= mapped_series < entry[i+1].series_start`

- `[RAX + R8 + 0x30]` = `table_base + index*0x88 + 0x30` = `entry[index].series_start`
- `[RAX + R8 + 0xB8]` = `table_base + index*0x88 + 0x88 + 0x30` = `entry[index+1].series_start`

So `0xB8 = 0x88 (next entry stride) + 0x30 (series_start offset)`.

### AOB Signature for the LEA Instruction

```
4C 8D 05 ? ? ? ? 0F 1F 80 00 00 00 00 48 63 41 10 48 69 C0 88 00 00 00
```

This captures the `LEA R8, [rip+disp]` followed by the `NOP` padding and the `MOVSXD; IMUL` sequence. Offset +0 is the LEA instruction; displacement is at offset +3.

---

## Version Filter Entry Structure — Confirmed Layout

Confirmed from decompilation of the one-time init block in `FUN_18011eda0`:

```
Entry (0x88 bytes):
+0x00  u32    group_index      Cabinet generation group (0-8 for vanilla, 9 = sentinel)
+0x04  [4]    padding
+0x08  string key              Internal key for texture lookup ("world", "a20plus")
       +0x08  [16] buf/ptr     SSO buffer or heap pointer
       +0x18  u64  length
       +0x20  u64  capacity    (0xF for SSO)
+0x28  [8]    padding
+0x30  u32    series_start     First series value in range (inclusive)
+0x34  [4]    padding
+0x38  string code             Short display code ("WORLD", "A20")
       +0x38  [16] buf/ptr
       +0x48  u64  length
       +0x50  u64  capacity
+0x58  [8]    padding
+0x60  string display          Full display name ("WORLD", "A20 PLUS")
       +0x60  [16] buf/ptr
       +0x70  u64  length
       +0x78  u64  capacity
+0x80  [8]    padding/alignment to 0x88
```

### MSVC std::string SSO (Small String Optimization)

For strings ≤ 15 bytes (which all filter keys are):
- Bytes 0-15: string data (null-terminated in remaining bytes)
- Offset +0x10: length (u64)
- Offset +0x18: capacity = 0x0F (u64, signals SSO mode)

For strings > 15 bytes:
- Bytes 0-7: heap pointer
- Offset +0x10: length (u64)
- Offset +0x18: capacity (u64, actual heap allocation size)

### Memory Layout After Version Filter Table

The version filter table (10 entries × 0x88 = 0x550 bytes) ends at `0x180CEF740`. The **title filter entries** (`line_a`, `line_ka`, etc.) begin immediately at `0x180CEF740`. There is **no free space** after the table — in-place extension is not possible. A new table must be allocated.

---

## Registration API — Complexity Assessment

The registration functions (`FUN_180127ca0` + `FUN_180128230`) operate on a `std::map<int, FilterCategoryData>` at `param_1 + 0x1F8`. The data structure passed to `FUN_180128230` is NOT a simple entry descriptor — it contains:

- Pointers to the filter manager context
- `std::tr1::function` wrappers around predicate lambdas (with vtable pointers)
- Multiple `std::string` copies of the category name
- Type/mode indicators
- Data copied via `FUN_1801279c0` (a structure copy function)

Calling these functions from outside `FUN_18011eda0` would require reconstructing the full internal state, including lambda vtable pointers for `sequence::selectmusic` anonymous classes. This is **not practical** for external injection.

Each filter category (title=0, version=1, genre=2, bpm=3, event=4, difficulty=0xB, level=5, flare=6, rank=7, clear_status=0xC, skill_target=8) has its own populate function (`FUN_180128230`, `FUN_180128450`, `FUN_180128730`, etc.) with category-specific parameter formats.

---

## Version Filter Handler Object

The version filter category is managed by a 0x20-byte handler object created by `FUN_180128100`:

```
Handler object (0x20 bytes):
+0x00  ptr    vtable           → 0x18036e338
+0x08  ptr    filter_manager   (R12 in filter_init)
+0x10  u64    category_params  (from filter_init stack)
+0x18  u64    more_params
```

**Factory**: `FUN_180128100` allocates 0x20 bytes via `operator new`, sets vtable, copies 24 bytes of parameters from the caller's stack.

**Vtable at `0x18036e338`**:
- `vtable[0]` → `FUN_18012cc30` — clone/copy constructor (allocates new 0x20 object, copies data)
- Additional entries TBD — likely include the method that builds FilterButton objects from the static table

**Clone function** (`FUN_18012cc30`): Allocates 0x20 bytes, sets vtable to `0x18036e338`, copies `+0x08`, `+0x10`, `+0x18` from source.

### How Entries Reach the Panel

The chain from registration to UI rendering:

```
filter_init
  → FUN_180128100 creates handler (vtable 0x18036e338)
  → FUN_180127ca0 creates std::map slot for category 1
  → FUN_180128230 stores handler + metadata in the slot
  ...later, when song select screen opens...
  → Panel builder (FUN_1801343a0) reads from the std::map
  → Iterates entries vector at [panel + 0x1B0] (16-byte elements = shared_ptr<FilterButton>)
  → Calls vtable[0] on each FilterButton → FUN_180132750 (render)
```

The entries vector is populated between registration and panel rendering — likely by the handler object's methods when the filter panel is first created.

### FilterButton Object Layout (Partial)

From disassembly of `FUN_180132750` (FilterButton render):

```
FilterButton object:
+0x00   ptr    vtable          (vtable[0] = FUN_180132750 = render)
+0x28   ptr    some_object     (has vtable, called at +0x1329ca)
+0x30   u8     selection_state (checked for animation label selection)
+0x60   ptr    parent_ref      (optional, checked for position offset)
+0x88   f64    base_x
+0x90   f64    base_y
+0x98   f64    base_z
+0xA0   f64    offset_x
+0xA8   f64    offset_y
+0xB0   f64    offset_z
+0xC8   u8     flag            (determines animation label: "in" vs "loop_select" vs "loop_switch")
+0xD0   string key             (std::string, used for sefi_item_%s texture name)
+0xF0   u32    category_index  (used in panel builder for filter_switch_base%02d)
+0x138  ptr    bm2d_movieclip  (shared_ptr to the "filter_item" MovieClip instance)
```

---

## UI Injection Strategy — Four Patches + Extended Table

The function `FUN_1801239c0` builds FilterButton objects from the static table. It loads the table base AND a hardcoded entry count. The game will create FilterButton objects for ANY entries in the table, as long as the count and base are correct. The game handles rendering, click/selection, textures — everything.

### Required Patches (4 total)

| # | What | Patch | Purpose |
|---|------|-------|---------|
| 1 | Series mapper default case | `xor eax,eax` → `mov eax,esi` | Custom series values pass through instead of being clamped to 0 |
| 2 | Predicate `LEA R8` at `0x18012361e` | Change disp32 → new table | Predicate reads series ranges from extended table |
| 3 | UI loop `LEA RBX` at `0x180123c0e` | Change disp32 → last custom entry key | FilterButton creation loop starts from last custom entry |
| 4 | UI loop `MOV ESI,8` at `0x180123c09` | Change imm32 → 8+N | Loop creates FilterButtons for all entries (counts down to 0) |

Note: The predicate path (`FUN_180123670` with `LEA RCX` and `MOV EDX,9`) and the UI path (`FUN_1801239c0` with `LEA RBX` and `MOV ESI,8`) are **separate code paths** that both reference the version filter table. Both must be patched.

### AOB Signatures

**Version entry loop (UI FilterButton creation)** — `FUN_1801239c0`:
```
BE 08 00 00 00 48 8D 1D
```
`MOV ESI,8` + `LEA RBX,[last_entry_key]`. Count at offset 1, LEA at offset 5. Unique in gamemdx.dll.

**Predicate table base** — `FUN_1801235e0`:
```
48 8B 50 08 48 8B 0A 48 3B CA 74 ? 4C 8D 05
```
`MOV+MOV+CMP+JZ` before `LEA R8,[table_base]`. LEA at offset 12. Unique in gamemdx.dll.

### New Table Layout

Allocate `(9 + N_custom + 1) * 0x88` bytes:

```
Index 0:   1th5th     (copied from vanilla)
Index 1:   maxex      (copied from vanilla)
...
Index 8:   world      (copied from vanilla)
Index 9:   {custom_0} (from config: series_value, key, code, display, group=8)
Index 10:  {custom_1} (from config)
...
Index 9+N: sentinel   (series_start = max custom + 1, group = 9+N)
```

Each custom entry needs:
- `group_index`: 8 (GOLD group — same as WORLD)
- `key`: from config `texture_name` (used for `sefi_item_{key}`)
- `series_start`: from config `series_value`
- `code`: from config `label` (short display)
- `display`: from config `label` (full display)

### Cabinet Group Sentinel Update

The cabinet group table at `0x180CECDC0` has a sentinel at entry 3 with `group_start = 9`. This must be updated to `9 + N_custom` so custom entries (indices 9+) fall within the GOLD group range.

This is a simple in-place memory write — the sentinel is at a fixed address (`0x180CECE50`).

### What the Game Handles Automatically

With these 4 patches + extended table + custom ARC:
- FilterButton objects created for custom entries
- `sefi_version_{key}` textures loaded from custom ARC and applied
- Click/selection handling (same as vanilla entries)
- Filter count updates correctly
- Custom entries appear in the VERSION filter panel alongside vanilla entries
- Predicate checks correct series ranges
- Combines with other filters (difficulty, BPM, etc.)
- Selecting a custom filter with 0 matching songs shows empty list (no crash)

---

## Builder Entry Filter Check — Why Custom Entries Were Initially Invisible

### Discovery

The entry builder (`FUN_180122d50`, called from `FUN_180123670`) has a per-entry filter check before creating each FilterButton:

```asm
; Inside FUN_180122d50 loop:
180122e30: MOV RCX,[R15 + 0x18]    ; load filter functor
180122e3d: MOV RAX,[RCX]           ; vtable
180122e40: MOV EDX,EDI             ; entry index
180122e42: CALL [RAX + 0x8]        ; vtable[1] = FUN_180130620 — filter check
180122e45: TEST AL,AL
180122e47: JZ skip_entry            ; if false → skip, no FilterButton created
```

The filter check (`FUN_180130620`, vtable at `0x18036ece0`) queries the filter_manager's registered entries collection at `+0x378`:

```asm
; FUN_180130620 — entry registration check
18013062a: MOV EAX,[RCX + 0x10]     ; category sub-index from functor
18013062d: MOV RDI,[RCX + 0x8]      ; filter_manager pointer from functor
18013063a: LEA RCX,[RDI + 0x378]    ; registered entries collection
180130645: CALL 0x1801d3f50          ; look up category in collection
18013066c: CALL 0x18004f400          ; search for entry index in category's set
18013067b: SETNZ AL                  ; return true if found
```

### Root Cause

The `+0x378` collection is populated as a side effect of `FUN_180128230` (the registration function). Only indices 0-8 are registered because only vanilla entries go through the registration chain. Custom entries (indices 9+) are not in the collection, so the filter check returns false and the builder skips them.

### Why Registration Cannot Be Called Externally

The `+0x378` collection is populated inside the registration chain: `FUN_180128230` → `FUN_180129850` → `FUN_18012a300` → `FUN_18012b3e0`. The input to this chain is a complex stack-built structure containing:
- Filter manager pointers (`+0x358`, `+0x378`, `+0x3d8`)
- `std::tr1::function` wrappers around predicate lambdas (with vtable pointers to `sequence::selectmusic` anonymous classes)
- Category name strings, type indicators, nested structure copies

Reconstructing this externally would require replicating the exact stack layout of `FUN_18011eda0`'s local variables, including lambda vtable pointers that are specific to the game binary version.

### Solution: Hook the Filter Check

Hook `FUN_180130620` to return true for custom entry indices. The hook calls the original first (preserving vanilla behavior), then returns true for indices ≥ 9 (custom entries). This is functionally equivalent to having registered the entries in the `+0x378` collection.

**Function signature**: `bool filter_check(void* functor, u32 entry_index)`

**AOB for FUN_180130620**:
```
8B 41 10 48 8B 79 08 89 54 24 ? 48 8D 54 24 ? 48 8D 8F 78 03 00 00
```
This is: `MOV EAX,[RCX+0x10]; MOV RDI,[RCX+0x8]; MOV [RSP+?],EDX; LEA RDX,[RSP+?]; LEA RCX,[RDI+0x378]`

---

## CORRECTION: Actual FilterButton Creation Path

### Previous Analysis Was Wrong

The function `FUN_180123670` (with the `LEA RCX,[table_base]` and `MOV EDX,9`) builds the **predicate's internal entry list**, NOT the FilterButton objects. It is never called during filter panel creation. The entry filter check (`FUN_180130620`) is also part of the predicate path, not the UI path.

### Actual FilterButton Creation Chain

Found by tracing xrefs to the FilterButton vtable at `0x18036FA48`:

```
handler vtable[8] (0x18036e378)
  → FUN_18012cd90
    → FUN_1801239c0  ← THIS is the real UI entry builder
      → FUN_1801237a0  ← creates one FilterButton (allocates 0x1D0 bytes)
        → FUN_180133ae0  ← FilterButton constructor (writes vtable)
```

### FUN_1801239c0 — The Real Entry Builder

This function has TWO loops:

**Loop 1 (lines 180123a00-180123b27):** Creates 3 GROUP tab entries (CLASSIC, WHITE, GOLD), iterating the cabinet group table backwards:
```asm
180123a05: LEA RBX,[0x180cece28]    ; cabinet group table entry 2 (GOLD) key field
180123a00: MOV ESI,0x2              ; start index = 2
; loop: create FilterButton, SUB RBX,0x30, DEC ESI, JNS loop
```

**Loop 2 (lines 180123c0e-180123d44):** Creates 9 VERSION filter entries, iterating the version filter table backwards:
```asm
180123c0e: LEA RBX,[0x180cef638]    ; version table entry 8 (WORLD) key field (+0x08)
180123c09: MOV ESI,0x8              ; start index = 8 (WORLD, counting down to 0)
; loop body:
180123c20: MOV RCX,[R14 + 0x18]    ; functor
180123c30: MOV EDX,ESI             ; entry index
180123c32: CALL [RAX + 0x8]        ; vtable[1] → creates FilterButton via FUN_1801237a0
; ...
180123d3b: SUB RBX,0x88            ; previous entry (stride)
180123d42: DEC ESI                  ; decrement index
180123d44: JNS 0x180123c20         ; loop while >= 0
```

### What Needs Patching (REVISED)

The version filter entry loop in `FUN_1801239c0` has:
1. **`LEA RBX,[0x180cef638]`** at `0x180123c0e` — points to the LAST entry's key field (entry 8 = WORLD, at table_base + 8*0x88 + 0x08). Must point to the last custom entry's key field instead.
2. **`MOV ESI,0x8`** at `0x180123c09` — start index (8 = WORLD). Must be 8 + N_custom (e.g., 10 for 2 custom entries).

The loop counts DOWN from ESI to 0, creating one FilterButton per iteration. It reads the key string from `[RBX]` (which is the entry's key field at +0x08 in the table).

### AOB Signatures Needed

**Version entry loop start** (unique because of the specific initial count and stride):
```asm
180123c09: MOV ESI,0x8              ; BE 08 00 00 00
180123c0e: LEA RBX,[0x180cef638]    ; 48 8D 1D ...
```

Pattern: `BE 08 00 00 00 48 8D 1D`
- Count at offset 1 (4 bytes, little-endian)
- LEA RBX at offset 5, displacement at offset 8

The LEA target must point to `new_table + (8 + N_custom) * 0x88 + 0x08` (the last custom entry's key field).
