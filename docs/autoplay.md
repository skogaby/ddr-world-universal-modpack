# Autoplay Mode Research

## Overview

DDR World uses a polymorphic input system for foot panel input during gameplay. During the attract loop (demo sequence), the game cycles through random songs with autoplay — automatically hitting every arrow with perfect timing. The same autoplay mechanism can be activated during normal player sessions by swapping the foot panel implementation at the right point in the game loop.

## IFootPanel Polymorphism

The game uses three classes for foot panel input (RTTI strings found in gamemdx.dll 20260324):

| Class | RTTI String | RTTI String Address (Ghidra) |
|-------|-------------|------------------------------|
| `IFootPanel` | `.?AVIFootPanel@input@@` | `0x180478a10` |
| `AutoFootPanel` | `.?AVAutoFootPanel@input@@` | `0x1804789b0` |
| `UserFootPanel` | `.?AVUserFootPanel@input@@` | `0x1804789e0` |

- `IFootPanel` — abstract base interface for foot panel input
- `UserFootPanel` — reads real input from the dance pad (normal gameplay)
- `AutoFootPanel` — generates automatic perfect inputs (demo/attract mode)

The `GamePlayActor` stores a pointer to an `IFootPanel` at **offset `+0x278`** from its base. During demo mode this points to an `AutoFootPanel` instance; during normal gameplay it points to a `UserFootPanel` instance.

## Vtable Addresses (Confirmed via RTTI Walk)

Found by walking MSVC x64 RTTI structures: TypeDescriptor → CompleteObjectLocator → vtable[-1] → vtable.

| Class | Vtable (Ghidra) | Vtable Offset from gamemdx.dll base |
|-------|-----------------|-------------------------------------|
| AutoFootPanel | `0x1803598D8` | `+0x3598D8` |
| UserFootPanel | `0x180359898` | `+0x359898` |

Both vtables have 7 entries (vtable[0] through vtable[6]).

### Vtable Layout Comparison

| Index | Offset | Purpose | AutoFootPanel (Ghidra) | UserFootPanel (Ghidra) |
|-------|--------|---------|------------------------|------------------------|
| [0] | +0x00 | destructor | `0x180022590` (shared) | `0x180022590` (shared) |
| [1] | +0x08 | **update** | `0x1800225C0` | `0x1800F3750` (no-op: `ret`) |
| [2] | +0x10 | wasJustPressed | `0x180022790` | `0x180022420` |
| [3] | +0x18 | isHeld | `0x1800227A0` | `0x180022460` |
| [4] | +0x20 | (unknown) | `0x1800227B0` | `0x1800224A0` |
| [5] | +0x28 | getPressTime | `0x1800227C0` | `0x1800224F0` |
| [6] | +0x30 | consumePress | `0x180022810` | `0x180022540` |

### Key Difference: update (vtable[1])

- **AutoFootPanel::update** (`0x1800225C0`) — Reads the note chart and auto-populates internal press/held flags. This is the core of autoplay.
- **UserFootPanel::update** (`0x1800F3750`) — A no-op (`ret 0000`). Does nothing because UserFootPanel reads real hardware input on demand.

## AutoFootPanel Internal Layout

```
+0x00: vtable pointer (8 bytes)
+0x08: byte[8] isHeld — one flag per panel direction
+0x10: byte[8] wasJustPressed — one flag per panel direction
+0x18: dword[8] pressTime — millisecond timestamp per panel (from timeGetSystemTime)
```

Total size needed: 0x38 bytes minimum (0x40 with alignment).

### AutoFootPanel Method Implementations

**wasJustPressed** (vtable[2]): `return this[0x10 + panelIndex]`
- Simple byte array read. Returns 1 if the panel was auto-pressed this frame.

**isHeld** (vtable[3]): `return this[0x08 + panelIndex]`
- Simple byte array read. Returns 1 if the panel is auto-held.

**getPressTime** (vtable[5]): Calls `timeGetSystemTime()` and returns current time.
- Always returns "now" — the timing difference from the note's music count is essentially zero, yielding perfect (Marvelous) judgement every time.

**consumePress** (vtable[6]): `this[0x18 + panelIndex * 4] = 0`
- Clears the press time for a panel after it's been judged.

### AutoFootPanel::update — The Autoplay Engine

Signature: `update(this, noteListPtr, noteCount, musicCount)`

The update method iterates the note chart and auto-generates inputs:

1. Clears isHeld (+0x08) and wasJustPressed (+0x10) arrays to zero
2. Iterates through the note list (same linked list structure used by judgeNotes)
3. For each unjudged note within timing range:
   - **Shock arrows** (all 4 directions = 1): Sets wasJustPressed to the INVERSE of the arrow pattern (avoids stepping on shock arrows — correct behavior)
   - **Normal arrows**: For each panel with an arrow (value 1 or 4), sets isHeld=1, wasJustPressed=1, and records `timeGetSystemTime().ms` in the pressTime array
4. Also handles freeze arrows by checking note values and setting held flags

### UserFootPanel Method Implementations

All UserFootPanel methods read from a **global input manager** at `DAT_1806ebc70`. They use `this+0x08` only as an index into that global structure. The actual input data comes from the hardware input system, not from the object itself.

This means UserFootPanel objects are small (~0x10 bytes: vtable + index int), while AutoFootPanel objects need at least 0x38 bytes for their internal arrays.

## GamePlayActor Details

- RTTI: `.?AVGamePlayActor@dance@sequence@@` at `0x18047cf40`
- Debug strings found:
  - `"GamePlayActor"` at `0x18035dad0`
  - `"..\\Game\\Sequence\\Dance\\GamePlayActor.cpp"` at `0x18035db90`
  - `"sequence::dance::GamePlayActor::judgeNotes"` at `0x18035dc68`
  - `"sequence::dance::GamePlayActor::onReceiveMessage"` at `0x18035dbd0`

## Game Loop Flow (Active Gameplay)

The game loop function `FUN_18005d2f0` (GamePlayActor's per-frame update) has a state machine. In state 4 (active gameplay):

```
1. footPanel->update(footPanel, &noteList, noteCount, musicCount)   // vtable[1]
2. FUN_18005f050(this)                                               // pre-judge setup
3. judgeNotes(this, musicCount)                                      // FUN_18005f270
```

The foot panel pointer is at `this[0x4F]` = `*(this + 0x278)`.

The update call parameters:
- `rcx` = footPanel pointer (from this+0x278)
- `rdx` = this + 0xB0 (pointer to note list start/end pair)
- `r8`  = *(this + 0x168) as int (note count or related value)
- `r9`  = musicCount (current music position)

## Note Judgement System (judgeNotes)

The main note judgement function is `FUN_18005f270` (GamePlayActor::judgeNotes).

**AOB Signature**: `4C 8B 45 20 41 3B 48 10` (function entry)

**Parameters:** `(GamePlayActor* this, uint musicCount)` — called each frame with the current music position.

**Panel count:** Checks `this->field_0x88` — if 1, uses 8 panels (doubles), otherwise 4 panels (singles).

**IFootPanel virtual methods called** (via `this->field_0x278`):

| Vtable Offset | Likely Purpose | Context |
|--------------|----------------|---------|
| `+0x10` (vtable[2]) | `wasJustPressed(int panelIndex)` | Called to check if a panel was just pressed this frame. Also called in the shock arrow auto-path. |
| `+0x18` (vtable[3]) | `isHeld(int panelIndex)` | Called in the initial loop to build `local_15c` bitmask of currently held panels. |
| `+0x28` (vtable[5]) | `getPressTime(int panelIndex)` | Called to get the exact timing of when the panel was pressed, for timing judgement calculation. |
| `+0x30` (vtable[6]) | `consumePress(int panelIndex)` | Called after a note is successfully judged, near the end of the function. |

### Note Iteration

The function iterates over a linked list of notes between `this->field_0xb0` (start) and `this->field_0xb8` (end). Each note entry is 64 bytes (8 qwords, `plVar19 += 8` per iteration). Each entry contains:
- `entry[0]` — pointer to note data struct
- `entry[1]` (low dword) — music count when note was hit
- `entry + 0xC` (byte) — judgement result (0xFF = unjudged)
- `entry + 0x11` (byte) — missed flag

**Note data struct** (pointed to by `entry[0]`):
- `+0x08` — note's music count (timing position)
- `+0x1C` through `+0x38` — 8 ints, one per panel direction. Value `1` = arrow present on that panel.

### Shock Arrow Detection

`FUN_18005b3f0` at `0x18005b3f0` checks if a note is a "shock arrow" (all 4 directions active simultaneously). It checks if all of `[+0x1C, +0x20, +0x24, +0x28]` are 1 (single) OR all of `[+0x2C, +0x30, +0x34, +0x38]` are 1 (double). Returns 1 if shock arrow, 0 otherwise.

### Judgement Flow Per Note

```
For each unjudged note (entry+0xC == 0xFF):
  1. Check if note is too old (musicCount > note.musicCount + 0xA0) → mark missed
  2. If note is too early (musicCount < note.musicCount - 0x104) → stop processing
  3. Call FUN_18005b3f0 to check if shock arrow
  4. If shock arrow AND note is in timing window:
       → Auto-check via vtable[2] (wasJustPressed) — shock arrows use a different path
  5. If NOT shock arrow:
       → Check local_15c bitmask (which panels user is pressing via vtable[3])
       → For each pressed panel matching a note direction:
           → Call vtable[5] (getPressTime) to get exact press timing
           → Calculate timing difference: abs(note.musicCount - pressTime)
           → Track best match across panels
  6. After checking all panels:
       → If enough panels matched → assign judgement grade based on timing
       → Judgement codes seen: 0x1028+grade (normal), 0x102D (miss), 0x1030 (shock miss), 0x1031 (shock hit), 0x1046 (some reset)
       → Call FUN_180060330 to submit the judgement result
```

### Timing Windows

Timing thresholds are loaded from a global data table at `_DAT_1803589d0` through `_DAT_180358918` (5 pairs of values — likely Marvelous/Perfect/Great/Good/Boo windows as min/max offsets from the note's music count).

### Key Field: `this->field_0x1e8`

A byte at `GamePlayActor + 0x1e8` is checked in several places:
- `*(char*)(param_1 + 0x1e8) == '\0'` — when this is 0, missed notes set `entry+0x11 = 1` (visible miss)
- When non-zero, misses are suppressed — this could be related to demo/autoplay mode

### Key Field: `this->field_0x1e9`

A byte at `GamePlayActor + 0x1e9` is checked before submitting judgement results:
- `if (*(char*)(param_1 + 0x1e9) == '\0')` → call `FUN_180060330` (submit judgement)
- When non-zero, judgement submission is skipped entirely

## DemoPlaySequence

- RTTI: `.?AVDemoPlaySequence@demo@sequence@@` at `0x18047dea8`
- String `"DemoPlaySequence"` at `0x180360688`
- String `"demo_root"` at `0x1803606a0`

This is the sequence class that manages the attract loop autoplay demo. It creates an `AutoFootPanel` and passes it to the `GamePlayActor`.

## Autoplay Approach: Hook judgeNotes + Swap Foot Panel

The cleanest approach to enabling autoplay is hooking `judgeNotes` and temporarily swapping the foot panel pointer.

### Why Not Just Swap the Vtable Pointer on UserFootPanel?

UserFootPanel objects are small (~0x10 bytes). AutoFootPanel::update writes to offsets +0x08 through +0x38. Overwriting the vtable pointer on a UserFootPanel would cause AutoFootPanel::update to write past the object's allocation — buffer overflow / heap corruption.

### Why Not a Global Flag?

Fields `+0x1e8` and `+0x1e9` on GamePlayActor suppress misses and judgement submission respectively, but they don't generate auto-inputs. Setting them would just make the game ignore all notes, not play them perfectly.

### The Approach

1. **Allocate** a persistent 0x40-byte buffer for a fake AutoFootPanel
2. **Write** the AutoFootPanel vtable pointer at offset 0 (resolved via RTTI vtable walk)
3. **Hook** `judgeNotes` (`FUN_18005f270`)
4. **On entry**:
   - Save the original foot panel pointer from `GamePlayActor+0x278`
   - Write the AutoFootPanel buffer address to `GamePlayActor+0x278`
   - Call `AutoFootPanel::update(ourPanel, this+0xB0, *(this+0x168), musicCount)` to populate auto-input data
5. **Let judgeNotes proceed** — it reads from the AutoFootPanel via vtable dispatch, getting perfect auto-inputs
6. **On exit**: Restore the original foot panel pointer (so other code that uses the foot panel for non-judgement purposes still works correctly)

### Signature

| Name | Pattern | Description |
|------|---------|-------------|
| `judge_notes` | `4C 8B 45 20 41 3B 48 10` | GamePlayActor::judgeNotes entry point |

### Derived Addresses (via RTTI)

| Name | Source | Description |
|------|--------|-------------|
| `auto_foot_panel_vtable` | RTTI walk for `.?AVAutoFootPanel@input@@` | AutoFootPanel vtable start |
| `auto_foot_panel_update` | `auto_foot_panel_vtable + 0x08` → read pointer | AutoFootPanel::update function |

## Related Strings

```
"DemoPlaySequence"                                    @ 0x180360688
"demo_root"                                           @ 0x1803606a0
"..\\Game\\Sequence\\Demo\\AdvertiseSequence.cpp"     @ 0x1803603a8
"data/arc/demo_advertise/demo_advertise_%02d_sd%s.arc" @ 0x1803603d8
"sequence::dance::GamePlayActor::judgeNotes"          @ 0x18035dc68
"sequence::dance::GamePlayActor::onReceiveMessage"    @ 0x18035dbd0
"GamePlayActor"                                       @ 0x18035dad0
"dance_judge"                                         @ 0x18035e8d0
"freeze_judge"                                        @ 0x18035ebd0
"dance_judge_for_freeze"                              @ 0x180360040
"gs_screencommand_judge"                              @ 0x1803599e0
"judge_timing"                                        @ 0x1803746f8
"judge_priority"                                      @ 0x180374718
"judge_position"                                      @ 0x180374728
```

## Build Version

All addresses in this document are from **gamemdx.dll build 20260324**.
