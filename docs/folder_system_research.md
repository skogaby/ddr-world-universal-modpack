# Folder System Research (Folder Expansion Mod)

Reverse engineering document for DDR World's genre folder system — how folders are defined, how songs are assigned to them, how difficulty restrictions work, how the folder card UI renders, and what's needed to add custom folders.

**Game binary**: `gamemdx.dll` (MDX-003_20260324)
**Ghidra base**: `0x180000000`
**Tools used**: Ghidra (static analysis), Python (musicdb.xml analysis), bemaniutils (AFP parsing)

---

## Summary

The genre folder system has three layers:

```
musicdb.xml <property>N</property>     (bitmask, bits 0-9)
    ↓
FolderProperty objects                  (created in FUN_180141050, one per folder)
    +0x00: folder_type_id              (1-7 for genre folders)
    +0xc0: max_difficulty              (0=Beginner only, 4=all)
    functor stores property bit index  (0-5, passed as EDX to FUN_180144040)
    ↓
Song list measurement                   (FUN_1801b33b0, tests property bitmask per song)
    per-folder song counts at [0x1806ebcf8 + 0xd0 + bit_index*4]
```

**Bits 0-5** of `<property>` map directly to folder_type_id 1-6. **Bits 6-9** have songs tagged but no corresponding folder in the current UI — these are candidates for custom folder expansion.

---

## Property Bitmask (musicdb.xml)

The `<property>` field in musicdb.xml is a **bitmask** controlling genre folder membership. A song can belong to multiple folders by having multiple bits set.

| Bit | Mask | Folder | Songs | Folder Type ID |
|-----|------|--------|-------|----------------|
| 0 | 1 | FIRST STEP | 24 | 1 |
| 1 | 2 | FOR MUSIC GAMERS | 81 | 2 |
| 2 | 4 | POP MUSIC | 20 | 3 |
| 3 | 8 | VIRTUAL POP | 51 | 4 |
| 4 | 16 | ANIME & GAME | 17 | 5 |
| 5 | 32 | TOUHOU Project | 47 | 6 |
| 6 | 64 | *(no folder)* — Tokimeki Idol songs | 12 | — |
| 7 | 128 | *(no folder)* — hinabita♪ / BEMANI songs | 55 | — |
| 8 | 256 | *(no folder)* — variety / licensed songs | 38 | — |
| 9 | 512 | *(no folder)* — DDR World originals | 40 | — |

**Statistics** (from MDX-003_20260324 musicdb.xml, 1413 songs total):
- 303 songs have a `<property>` tag
- 1110 songs have no `<property>` tag (appear only in ALL MUSIC)
- 145 songs have property but bits 0-5 all clear (also ALL MUSIC only)
- 61 songs belong to multiple genre folders (multiple bits set)
- Bit 0 (FIRST STEP) never appears alone — those songs always have other folder bits too

### Property Bitmask Test — Supports 32 Bits Natively

The actual song-to-folder filter is in `FUN_1801444f0`. It reads the property as a **u32** from `[song_object + 0x178]` (with fallback to `+0x174`) and tests: `(property >> bit_index) & 1`. The `SHL EDX, CL` instruction natively supports indices 0-31 via x86 shift masking. **The bitmask test has no 10-folder limit.**

There's a special case for bit index 8: it also matches if bit 6 of the property byte is set (`TEST R8B, 0x40`). This may be a legacy compatibility quirk.

### Property Field Width

The property value is read as a **u32** from `[song_object + 0x178]`. In musicdb.xml, the `<property>` tag is parsed as an integer. Current max value is 512 (bit 9), but the u32 accessor supports values up to 2^32-1, giving 32 possible folder bits. To use bits 10+ in musicdb.xml, simply set `<property>` to values with those bits set (e.g., `<property>1024</property>` for bit 10).

### Song Count Arrays — 10 Slots

The per-folder song count arrays are **10 dwords each** (confirmed from zeroing loops in `FUN_1801b33b0`):
- Single mode: `[ListManager + 0xd0]` through `[ListManager + 0xf8)` — 10 × u32
- Duo mode: `[ListManager + 0xf8]` through `[ListManager + 0x120)` — 10 × u32

For bit indices ≥ 10, the has-songs predicate reads out-of-bounds. The predicate hook bypasses this for configured custom folders.

---

## Difficulty Restriction Per Folder

Each genre folder has a **maximum difficulty index** at offset `+0xc0` in its FolderProperty object.

| Folder | +0xc0 | Max Difficulty | Selectable |
|--------|-------|----------------|------------|
| FIRST STEP | 0 | Beginner | Beginner |
| FOR MUSIC GAMERS | 1 | Basic | Beginner, Basic |
| POP MUSIC | 1 | Basic | Beginner, Basic |
| VIRTUAL POP | 1 | Basic | Beginner, Basic |
| ANIME & GAME | 1 | Basic | Beginner, Basic |
| TOUHOU Project | 1 | Basic | Beginner, Basic |
| ALL MUSIC | 4 *(default)* | Challenge | All |

The constructor (`FUN_180140b60`) initializes `+0xc0` to 4 (all difficulties). The folder init function (`FUN_180141050`) overwrites it to 0 or 1 for genre folders. ALL MUSIC keeps the default.

---

## Key Functions

### `FUN_180141050` — Folder Init

**Ghidra address**: `0x180141050`

Creates all FolderProperty objects. The full per-folder creation sequence (verified from disassembly):

1. Builds voice-over string (e.g., `"vo_select_folder_firststep"`)
2. Builds folder key string (e.g., `"firststep"`)
3. Calls `FUN_1801448a0` — lookup in some registry → returns 16-byte shared_ptr-like result
4. Calls `FUN_180144040(stack_buf, EDX=bit_index)` — creates property-bit functor → RAX saved in RBX
5. Calls `FUN_180143ff0(stack_buf2, EDX=bit_index)` — creates filter functor → RAX saved in RSI
6. `operator new(0x208)` → `FUN_180140b60(ptr)` (constructor) → RDI = folder_property
7. Sets `+0xc0` (max_difficulty), `+0x00` (folder_type_id), `+0x1f8` (mode flag = 3)
8. Copies key string to `[RDI+0x48]`, voice string to `[RDI+0x70]` via string assign
9. `FUN_18017cf00(folder_property + 0x1d8, RSI)` — wires filter functor via shared_ptr move
10. `FUN_18017cf00(folder_property + 0x8, RBX)` — wires property functor via shared_ptr move
11. `FUN_180140ce0(folder_property, lookup_result)` — stores shared_ptr at +0x1a8, returns folder_property
12. `FUN_180143db0(context_ptr, folder_property)` — pushes into folder list

**Critical detail**: The functor constructors (steps 4-5) are called BEFORE the FolderProperty is allocated (step 6). The functors are created into stack-allocated buffers (0x40 bytes each), then wired into the FolderProperty via shared_ptr move (steps 9-10).

**EDX (property bit index) per folder**:

| Folder | EDX | Bit Mask | Disasm Address |
|--------|-----|----------|----------------|
| firststep | 0 | 1 | `0x18014114c: XOR EDX,EDX` |
| popmusic | 2 | 4 | `0x180141373: MOV EDX,0x2` |
| virtualpop | 3 | 8 | `0x1801415a2: MOV EDX,0x3` |
| animegame | 4 | 16 | `0x1801417d1: MOV EDX,0x4` |
| touhou | 5 | 32 | `0x180141a00: MOV EDX,0x5` |
| musicgamers | 1 | 2 | `0x180141c2f: MOV EDX,0x1` |
| allmusic | *(special)* | *(all)* | *(no functor — shows all songs)* |

### `FUN_180144040` — Folder Functor Constructor

**Ghidra address**: `0x180144040`

Creates a small functor object. Signature: `__fastcall(RCX=out_buffer, EDX=bit_index) → *mut u8`

```
Functor layout:
+0x00: ptr   vtable → 0x180370fc0
+0x08: u32   property_bit_index (from EDX)
+0x18: ptr   self-pointer
```

The vtable has these methods:
- `vtable[0]` → `FUN_180145e10` — clone
- `vtable[1]` → `FUN_180145e80` — **predicate**: checks if folder has songs
- `vtable[2]` → `FUN_180145ec0` — RTTI info
- `vtable[3]` → `FUN_180186720` — destructor

### `FUN_180143ff0` — Filter Functor Constructor

**Ghidra address**: `0x180143ff0`

Creates the filter functor. Same signature as functor_ctor: `__fastcall(RCX=out_buffer, EDX=bit_index) → *mut u8`. Wired into FolderProperty at +0x1d8 via `FUN_18017cf00`.

### `FUN_18017cf00` — Shared Ptr Move

**Ghidra address**: `0x18017cf00`

Moves a shared_ptr into a destination slot. Signature: `__fastcall(RCX=dest_slot, RDX=source)`. Called twice per folder:
- `(folder_property + 0x1d8, filter_functor_return)` — wire filter functor
- `(folder_property + 0x8, functor_return)` — wire property functor

### `FUN_180140ce0` — Store Shared Ptr at +0x1a8

**Ghidra address**: `0x180140ce0`

Signature: `__fastcall(RCX=folder_property, RDX=shared_ptr_buffer) → *mut u8` (returns folder_property).

Calls `FUN_18016def0(folder_property + 0x1a8)` internally, then decrements refcount on the old shared_ptr from RDX. The second parameter is the 16-byte result from the `FUN_1801448a0` lookup.

### `FUN_180143db0` — Folder Register

**Ghidra address**: `0x180143db0`

Pushes a FolderProperty into the folder list. Signature: `__fastcall(RCX=context_ptr, RDX=folder_property)`.

The context_ptr is a pointer to a stack-local structure in folder_init (at `[RSP+0x50]`). RDX is the return value of `FUN_180140ce0` (which returns the folder_property pointer).

### `FUN_180145e80` — Folder Has-Songs Predicate

**Ghidra address**: `0x180145e80`

Checks whether a folder has any songs by reading a pre-computed count:

```asm
MOV RAX,[0x1806ea478]           ; game state global
MOVSXD RDX,[RCX + 0x8]          ; RDX = property bit index (from functor)
MOV RCX,[RAX]                   ; game state object
MOV RAX,[0x1806ebcf8]           ; ListManager global
MOV R8D,[RCX + 0x4]             ; R8D = play mode (1=duo, other=single)
CMP R8D,0x1
JNZ use_single
  MOV EDX,[RAX + RDX*4 + 0xf8]  ; duo mode: count at +0xf8 + index*4
  JMP check
use_single:
  MOV EDX,[RAX + RDX*4 + 0xd0]  ; single mode: count at +0xd0 + index*4
check:
TEST EDX,EDX                    ; if count == 0 → folder empty → return false
```

### `FUN_180140b60` — FolderProperty Constructor

**Ghidra address**: `0x180140b60`

Initializes a 0x208-byte FolderProperty object. Key defaults:
```asm
MOV dword ptr [RCX + 0xc0], 0x4      ; max_difficulty = 4 (all)
MOV dword ptr [RBX + 0x1fc], 0x1010101  ; per-difficulty flags
MOV word ptr [RBX + 0x200], 0x101
MOV byte ptr [RBX + 0x202], 0x1
```

---

## FolderProperty Object Layout

The struct layout differs between game versions. All field offsets are detected dynamically at runtime by analyzing `folder_init` and `folder_property_ctor`.

### 20260324 Layout (0x208 bytes)

```
+0x00   u32    folder_type_id    1-7 for genre, 8-0xa for brave, 0x63 for special
+0x04   u32    sub_type          7 for brave folders, 0 for genre
+0x08   ...    shared_ptr (property functor, from FUN_18017cf00)
+0x48   string key               SSO, e.g., "firststep", "popmusic"
+0x70   string voice_key         SSO, e.g., "vo_select_folder_firststep"
+0x98   string extra_string      SSO
+0xc0   u32    max_difficulty    0=Beginner, 1=Basic, ..., 4=Challenge (all)
+0xc4   u8     flag_c4           0 for genre, 1 for brave/special
+0xc8   ...    collection (from FUN_1801401c0)
+0x150  ...    collection (from FUN_1801445a0) — brave folders add entries here
+0x1a8  ptr    shared_ptr (from FUN_180140ce0)
+0x1d8  ...    shared_ptr (filter functor, from FUN_18017cf00)
+0x1f8  u32    mode_flag         3 for genre folders, 0 for allmusic
+0x1fc  u8[7]  ui_axis_flags     Per-folder UI/input-axis enables (NOT difficulty — see below)
```

### 20250805 Layout (0x1D0 bytes)

```
+0x00   u32    folder_type_id
+0x08   string key               SSO
+0x58   string voice_key         SSO
+0x100  u32    max_difficulty    0=Beginner, ..., 4=all
+0x104  u8[3]  diff_restrict_flags  Per-difficulty restriction flags (ctor default: all 0s = unrestricted)
+0x118  ...    shared_ptr (filter functor)
+0x138  u32    mode_flag         3 for genre folders, 0 for allmusic
+0x140  ...    shared_ptr (property functor)
+0x1a8  ptr    shared_ptr (folder data, from store_ptr)
```

### Difficulty Unlock — `+0xc0` Only

Difficulty selectability is governed **solely** by the `max_difficulty` field (`+0xc0`
on 20260324/20260526, `+0x100` on 20250805). The constructor seeds it to 4 (all
difficulties); `folder_init` overwrites it to 0/1 for restricted genre folders.
Writing 4 back is the complete difficulty unlock — nothing else needs to change.

### The `+0x1fc` Cluster Is View/Input-Axis State, NOT Difficulty Flags

> **Correction (verified on 20260526):** Earlier revisions of this doc and the
> Folder Expansion mod treated the 7-byte cluster at `+0x1fc..+0x202` as
> "per-difficulty enable flags" and wrote `1`s across it during difficulty unlock.
> That was wrong. The cluster is a set of independent per-folder booleans that the
> select-music screen reads to decide **which input axes and widgets to activate**.
> Writing it corrupts a folder's native layout. This is what caused the Dan Ranking
> 2D-scroll bug (see below).

Constructor (`folder_property_ctor`) defaults the whole cluster to `1`:
`+0x1fc = 0x01010101`, `+0x200 = 0x0101`, `+0x202 = 0x01` → `{1,1,1,1,1,1,1}`.

Per-folder `folder_init` then customizes it:
- **Genre folders** clear `+0x1fc` and `+0x1fe` → `{0,1,0,1,1,1,1}` (no 2D grid).
- **ALL MUSIC** keeps the all-1s default (full 2D grid of songs).
- **Dan Ranking** (type_id 10) keeps `+0x1fc=1` but clears `+0x1fd..+0x202` →
  `{1,0,0,0,0,0,0}` (single vertical course list, no horizontal axis).

Verified readers (each byte gates a distinct behavior):

| Byte | Reader (20260526) | Behavior gated |
|------|-------------------|----------------|
| `+0x1fc` | `FUN_18010e730` | Registers the 2D scroll-axis input handlers (`FUN_18004fc20(…,3,4)` and `(…,1,2)`) — both up/down and left/right selection axes |
| `+0x1fd` | `FUN_18010e730` (tail) | If nonzero, registers extra input handler id `0x10` (an additional nav axis) |
| `+0x1fe` | `FUN_1801a2080` | If nonzero, registers selection callbacks ids `0xd` and `0x0a` |
| `+0x1ff` | `FUN_18010c4a0` | Passed to `FUN_180045640(widget@+0x410, flag)` — toggles a widget's active state |
| `+0x200` | `FUN_1801610d0` | Passed to `FUN_180045640(widget@+0xc8, flag)` — toggles another widget |
| `+0x201`, `+0x202` | (same family) | Seeded by ctor; consumed alongside the above (no distinct reader isolated) |

**Dan Ranking bug mechanism:** the blanket unlock wrote `1` to all 7 bytes, flipping
Dan Ranking's `+0x1fd`/`+0x1fe` (and others) from 0 back to 1. That re-registered the
horizontal-axis and extra-selection handlers, turning the single-axis course list
into a 2D grid — letting the player scroll left/right into phantom items. Fix: the
mod now writes only `+0xc0` and never touches this cluster.

### 20250805 Note

On 20250805 the genre-folder difficulty field is `+0x100` and is the only field
needed for unlock. Dan Ranking did not exist before 20260324, so there is no
type-10 folder and no `+0x1fc`-style corruption to worry about on that build.

### FolderProperty Ownership

`folder_register` creates a `shared_ptr<FolderProperty>` with a control block (vtable `0x180370CF8` on 20260324) that stores the FolderProperty pointer. When the folder manager is destroyed, the control block's destructor calls the game's `free()` on the FolderProperty. **Custom FolderProperty objects must be allocated with the game's CRT malloc** (`operator new`, found via AOB scan) — using `VirtualAlloc` or Rust's allocator causes a heap mismatch crash in `RtlFreeHeap`.

---

## Folder Card UI Rendering (AFP + BM2D)

This section documents how the game renders the visual folder card in the carousel. This is critical for custom folder support — without proper AFP handling, custom folders appear in the carousel but render as invisible cards.

### Asset Structure (from unpacked `select_music_folder_v3.arc`)

The folder UI assets are in an IFS container with three directories:

```
afp/                    AFP animation templates (binary)
afp/bsi/                AFP byte-swap info (descrambling data)
geo/                    Shape/geometry files (contain texture name references)
tex/                    Texture PNGs
```

### Per-Folder AFP Templates

Each genre folder has its own AFP template named `folder_{key}`:

| AFP Template | Size | Geo Count |
|-------------|------|-----------|
| `folder_firststep` | 29720 bytes | 18 |
| `folder_musicgamers` | 29720 bytes | 18 |
| `folder_popmusic` | 29716 bytes | 18 |
| `folder_virtualpop` | 29720 bytes | 18 |
| `folder_animegame` | 29720 bytes | 18 |
| `folder_touhou` | 29716 bytes | 18 |
| `folder_allmusic` | 29832 bytes | 18 |

**All 6 genre folder AFPs have identical tag structure** (verified via bemaniutils AFP parser):
- 289 tags, 300 frames, 60 FPS
- Identical labels: `{in: 60, in_end: 76, in_current: 120, in_current_end: 136, out: 180, out_end: 190}`
- Identical exported tags (except self-name): `{aep_mask_dummy: 6, aeplibset: 3, current_effect: 27, folder_button: 37, folder_{key}: 77}`
- 18 shape references with identical IDs: `[5, 8, 9, 12, 15, 18, 21, 29, 30, 41, 44, 47, 50, 53, 56, 59, 62, 63]`

**The ONLY difference between folder AFPs is one string in the string table**: the `exported_name` (first entry, e.g., `"folder_firststep"` vs `"folder_popmusic"`). All other strings (20 of 21) are identical.

### Per-Folder Textures

Each genre folder requires **6 textures** (4 confirmed required, 2 subtitle variants):

| Texture Name Pattern | Purpose |
|---------------------|---------|
| `mufo_folder_back_{key}_on` | Background (selected/highlighted) |
| `mufo_folder_back_{key}_off` | Background (not selected) |
| `mufo_txt_folder_title_{key}_on` | Title text (selected) |
| `mufo_txt_folder_title_{key}_off` | Title text (not selected) |
| `mufo_txt_folder_subtitle_{key}_on` | Subtitle text (selected) |
| `mufo_txt_folder_subtitle_{key}_off` | Subtitle text (not selected) |

### Per-Folder Geo Files (Shape Data)

Each AFP references 18 geo files named `folder_{key}_shape{N}`. Of these:

**6 folder-specific shapes** (contain the folder key in texture references):

| Shape ID | Texture Reference |
|----------|------------------|
| 41 | `mufo_txt_folder_title_{key}_on` |
| 44 | `mufo_txt_folder_title_{key}_off` |
| 47 | `mufo_txt_folder_subtitle_{key}_on` |
| 50 | `mufo_txt_folder_subtitle_{key}_off` |
| 53 | `mufo_folder_back_{key}_on` |
| 56 | `mufo_folder_back_{key}_off` |

**12 shared shapes** (identical across all folders):

| Shape ID | Texture Reference |
|----------|------------------|
| 5 | *(empty — mask/placeholder)* |
| 8 | *(empty)* |
| 9 | *(empty)* |
| 12 | `reference_folder` |
| 15 | `mufo_txt_folder_decide` |
| 18 | `mufo_button_key_decide` |
| 21 | `mufo_button_base_ef` |
| 29 | *(empty)* |
| 30 | *(empty)* |
| 59 | `mufo_folder_bd_current` |
| 62 | `mufo_folder_bd` |
| 63 | *(empty)* |

Texture names are embedded in the GE2D binary geo data at byte offset 56 as null-terminated ASCII strings (prefixed with a length byte).

### Texture Resolution Chain

```
Game code: sprintf("folder_%s", key)
  → AFP runtime loads "folder_{key}" from IFS container
    → AFP shape tags reference shapes by numeric ID (5, 8, 9, 12...)
      → Shapes loaded from same IFS container's geo/ folder
        → Geo files contain embedded texture name strings
          → BM2D resolves texture names to atlas regions
```

The AFP's `exported_name` determines the geo file naming prefix. Shape tag ID 41 in `folder_firststep` maps to geo file `folder_firststep_shape41`, which contains the texture name `mufo_txt_folder_title_firststep_on`.

### Folder Card Setup Function (`FUN_18013f750`)

This function creates and configures the AFP layer for a single folder card. Called once per folder during carousel setup.

**Annotated flow** (libafp API functions identified via ordinal export table from libafp-win64.dll):

```
Step 1: sprintf("folder_%s", folder_key)  →  e.g., "folder_firststep"

Step 2: afp_layer_create_with_property(slot, afp_package, template_name, flags)
        [IAT: 0x1802d56C0, Ordinal 31]
        - Internally finds the AFP stream by name in the loaded IFS package
        - If stream not found → returns NULL → all rendering skipped
        - If found → creates AFP layer, loads shapes+textures from package
        - Stores layer handle at [folder_obj + 0x108]

Step 3: afp_layer_play(layer_handle, 1, 1)
        [IAT: 0x1802d56E0, Ordinal 43]

Step 4: sprintf("mufo_folder_base_%s", folder_key)  →  e.g., "mufo_folder_base_firststep"

Step 5: afp_layer_mc_refer(layer_handle, "mufo_folder_base_firststep")
        [IAT: 0x1802d5660, Ordinal 103]
        - Finds a MovieClip child by name WITHIN the AFP layer
        - Returns MovieClip ID (≥0) or -1 if not found

Step 6: Loop over MovieClip and siblings:
        afp_mc_load_bitmap(mc_id, "mufo_folder_base_firststep")
        [IAT: 0x1802d56B0, Ordinal 112]
        - Loads/binds a texture by name into the MovieClip
        
        afp_mc_traversal(mc_id, 6)
        [IAT: 0x1802d5638, Ordinal 106]
        - Gets next sibling MovieClip, repeat until -1
```

**Key insight**: The game constructs BM2D element names using the **folder's key** (from FolderProperty +0x48), NOT the AFP's internal exported_name. Steps 4-6 use `folder_key` to build texture/MovieClip names. The AFP template provides the layout/animation structure, while the folder key drives the texture binding.

### libafp IAT Mapping (gamemdx.dll)

Function pointers resolved from libafp-win64.dll ordinal imports:

| IAT Address | Ordinal | Export Name | Purpose |
|-------------|---------|-------------|---------|
| `0x1802d5600` | 15 | `afp_do_render` | Render frame |
| `0x1802d5608` | 47 | `afp_layer_set_position` | Set layer position |
| `0x1802d5610` | 45 | `afp_layer_set_matrix` | Set transform matrix |
| `0x1802d5618` | 3 | `afp_boot` | Initialize AFP system |
| `0x1802d5620` | 41 | `afp_layer_set_params` | Set layer parameters |
| `0x1802d5628` | 52 | `afp_layer_set_priority` | Set render priority |
| `0x1802d5630` | 33 | `afp_layer_do_destroy` | Destroy layer |
| `0x1802d5638` | 106 | `afp_mc_traversal` | Traverse MovieClip tree |
| `0x1802d5640` | 114 | `afp_mc_op` | MovieClip operation |
| `0x1802d5660` | 103 | `afp_layer_mc_refer` | Find MovieClip by name in layer |
| `0x1802d5668` | 122 | `afp_mc_mc_list` | List MovieClips |
| `0x1802d5670` | 22 | `afp_id_is_valid` | Check if ID is valid |
| `0x1802d5678` | 56 | `afp_layer_set_attribute` | Set layer attribute |
| `0x1802d5698` | 104 | `afp_mc_refer` | Find MovieClip by name (global) |
| `0x1802d56A0` | 111 | `afp_mc_load_movie` | Load movie into MovieClip |
| `0x1802d56A8` | 113 | `afp_mc_load_bitmap_from_info` | Load bitmap from info struct |
| `0x1802d56B0` | 112 | `afp_mc_load_bitmap` | Load bitmap by name into MovieClip |
| `0x1802d56B8` | 32 | `afp_layer_get_name` | Get layer name |
| `0x1802d56C0` | 31 | `afp_layer_create_with_property` | Create layer from AFP stream |
| `0x1802d56C8` | 116 | `afp_mc_set_param` | Set MovieClip parameter |
| `0x1802d56E0` | 43 | `afp_layer_play` | Play layer animation |

### Custom Folder AFP Strategy — Runtime Hooking

Since all genre folder AFPs are structurally identical (differing only in the exported_name string), custom folders can reuse `folder_firststep`'s AFP template at runtime via two hooks. This avoids requiring mod creators to understand or produce AFP/geo assets.

**Hook 1: `afp_layer_create_with_property`** (Ordinal 31)

When the game creates a layer for `folder_{custom_key}`:
- The stream doesn't exist in the IFS package → would return NULL
- Intercept: redirect the stream name to `folder_firststep`
- The layer is created using firststep's AFP template and geo shapes
- Track that this layer belongs to custom folder `{custom_key}`

**Hook 2: `afp_layer_mc_refer`** (Ordinal 103)

After layer creation, the game looks for MovieClips by name using the folder's key:
- Game calls `afp_layer_mc_refer(layer, "mufo_folder_base_{custom_key}")`
- But the layer was created from firststep's AFP, so the MovieClip is named `mufo_folder_base_firststep`
- Intercept: for tracked custom folder layers, rewrite `{custom_key}` → `firststep` in the lookup name
- The MovieClip is found successfully

**No hook needed for texture loading**: After `afp_layer_mc_refer` finds the MovieClip, the game calls `afp_mc_load_bitmap(mc_id, "mufo_folder_base_{custom_key}")`. This uses the folder's key (not firststep's), so it loads the custom texture from the mod's ARC directly. The BM2D texture system resolves the name against all loaded IFS/ARC containers.

**Mod creator requirements**: Only provide textures in the custom ARC:
- `mufo_folder_back_{key}_on` / `_off`
- `mufo_txt_folder_title_{key}_on` / `_off`
- `mufo_txt_folder_subtitle_{key}_on` / `_off` (optional — may fall back gracefully)

---

## Folder Key Strings and BM2D Elements

| Folder | Key | Voice Key | Type ID |
|--------|-----|-----------|---------|
| FIRST STEP | `firststep` | `vo_select_folder_firststep` | 1 |
| FOR MUSIC GAMERS | `musicgamers` | `vo_select_folder_gamer` | 2 |
| POP MUSIC | `popmusic` | `vo_select_folder_popmusic` | 3 |
| VIRTUAL POP | `virtualpop` | `vo_select_folder_virtualpop` | 4 |
| ANIME & GAME | `animegame` | `vo_select_folder_animegame` | 5 |
| TOUHOU Project | *(at 0x18036dc8c)* | `vo_select_folder_touhou` | 6 |
| ALL MUSIC | `allmusic` | `vo_select_folder_all` | 7 |

**BM2D format strings** (used by game code to construct element names from folder key):

| Format String | Address | Purpose |
|--------------|---------|---------|
| `folder_%s` | `0x180370350` | AFP template name |
| `mufo_folder_base_%s` | `0x180370378` | Folder base MovieClip lookup |
| `mufo_txt_folder_info_%s` | `0x180370308` | Folder info text |
| `muca_txt_folder_name_%s` | `0x18036c940` | Folder name text |
| `folder_layout_root` | `0x180370260` | Root layout (shared) |
| `folder_information` | `0x180370298` | Info panel (shared) |
| `folder_button_usr` | `0x1803703a0` | Button child element (shared) |
| `difficulty_limit` | `0x1803702c0` | Difficulty restriction display (shared) |

---

## Addresses Quick Reference

### 20260324

| Symbol | Ghidra Address | Description |
|--------|---------------|-------------|
| `folder_init` | `0x180141050` | Creates all FolderProperty objects |
| `folder_property_ctor` | `0x180140b60` | FolderProperty constructor (0x208 bytes) |
| `folder_functor_ctor` | `0x180144040` | Creates property-bit functor |
| `folder_filter_functor_ctor` | `0x180143ff0` | Creates filter functor |
| `folder_has_songs` | `0x180145e80` | Predicate: folder has songs? |
| `folder_functor_vtable` | `0x180370fc0` | Functor vtable |
| `folder_register` | `0x180143db0` | Push folder into list |
| `folder_store_ptr` | `0x180140ce0` | Store shared_ptr at +0x1a8 |
| `shared_ptr_move` | `0x18017cf00` | Move shared_ptr into destination slot |
| `folder_card_setup` | `0x18013f750` | Creates AFP layer for folder card UI |
| `afp_layer_create_wrapper` | `0x18021b4d0` | Wrapper: find free slot + create layer |
| `afp_layer_init` | `0x18021b6a0` | Init layer from AFP package stream |
| `afp_slot_finder` | `0x18021b590` | Find free slot in AFP layer pool |
| `property_bitmask_test` | `0x1801444f0` | Tests song.property & (1 << bit_index) |
| `song_measurement` | `0x1801b33b0` | Song list measurement |
| `list_manager_global` | `0x1806ebcf8` | ListManager global pointer |
| `game_state_global` | `0x1806ea478` | Game state global pointer |
| `game_malloc` | `0x180276a34` | MSVC operator new (CRT HeapAlloc wrapper) |
| `game_free` | `0x18027678c` | MSVC operator delete (CRT HeapFree wrapper) |
| `game_heap` | `[0x1804bdb50]` | CRT heap handle |

### 20250805

| Symbol | Ghidra Address | Description |
|--------|---------------|-------------|
| `folder_init` | `0x180134d20` | Creates all FolderProperty objects |
| `folder_property_ctor` | `0x180134970` | FolderProperty constructor (0x1D0 bytes) |
| `folder_register` | `0x180136ea0` | Push folder into list |
| `game_malloc` | `0x18025e444` | MSVC operator new (CRT HeapAlloc wrapper) |

---

## Open Questions

1. **~~Are the count arrays sized for more than 6?~~** YES — confirmed 10 slots each, matching property bits 0-9.

2. **What determines folder display order?** Folders are created in a specific order in `FUN_180141050`. The UI carousel order matches creation order. Custom folders injected before ALL MUSIC (via folder_register hook) appear after vanilla genre folders.

3. **Can the folder carousel scroll?** Currently 7 folders fit the UI. Adding more may need scroll support (similar to the series filter scroll work).

4. **What are the brave folders (type 8/9/0xa)?** These have a `+0x150` collection with entries added via `FUN_180144650`. They appear to be event/special folders with different song assignment logic. On 20260526 the registration order is: genre folders (1-6), ALL MUSIC (7), **Dan Ranking (10)**, extrasavior/brave (8), galaxybrave/brave (9), then a final type-99 folder.

5. **~~What is folder type 0x63?~~ / What is folder type 10?** Type **10** is the **Dan Ranking / Dan Course** folder (key `"danrank"`, voice `"danrank"`, added in 20260324). It is built via a dedicated path in `folder_init` — not the genre-folder path — and configures the `+0x1fc` UI-axis cluster to `{1,0,0,0,0,0,0}` for a single vertical course list. Writing difficulty-unlock 1s into that cluster re-enabled a phantom horizontal axis (see "The `+0x1fc` Cluster" above). Type **0x63** is the final folder created in `folder_init` (special constructor path); its purpose is still unconfirmed.

6. **Are subtitle textures required?** The geo files reference `mufo_txt_folder_subtitle_{key}_on/off` but these may fall back gracefully if missing. Needs testing.

7. **Does `afp_mc_load_bitmap` fail gracefully for missing textures?** If the custom ARC doesn't include a texture, does the MovieClip render blank or crash? Needs testing.
