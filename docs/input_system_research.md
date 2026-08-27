# Input System Research — DDR World

## Overview

DDR World handles button input through a set of named exports in `arkmdxbio2.dll`. These exports read from spice2x's IO abstraction layer and return per-button state including edge detection (trigger/release). No hooking is required — the exports can be called directly.

## arkmdxbio2.dll Input Exports

### Foot Panel Arrow Exports

Signature: `(int playerIndex, bool* trigger, bool* hold, bool* release, uint* counter) -> void`

| Export | Description |
|--------|-------------|
| `arkMDXGetPanelUp` | Up arrow |
| `arkMDXGetPanelDown` | Down arrow |
| `arkMDXGetPanelLeft` | Left arrow |
| `arkMDXGetPanelRight` | Right arrow |

- `playerIndex`: 0 = P1, 1 = P2
- `trigger`: true for one frame when button goes down (press edge)
- `hold`: true while button is held
- `release`: true for one frame when button goes up (release edge)
- `counter`: cumulative press count

### 10-Key Numpad Export

Signature: `(int playerIndex, bool[12] outKeys) -> void`

| Export | Description |
|--------|-------------|
| `arkMDXGet10Key` | Numpad state for one player |

- `playerIndex`: 0 = P1, 1 = P2 (each player has their own numpad)
- `outKeys`: 12-element bool array, one per key:
  - Index 0-9: digit keys 0-9
  - Index 10: `*` (star) key
  - Index 11: `#` (hash) key
- Each element is true if that key was pressed this frame (trigger)
- The game's I/O test screen reads this with `vtable[0x308/8]` on the MDX object

**Mod menu use case**: The 10-key numpad is ideal for mod menu navigation because it has minimal in-game effect outside of specific scenes (e.g., PIN entry). During attract mode and most gameplay scenes, numpad presses are ignored by the game, making them safe for mod menu control without interfering with normal operation.

### Menu/Navigation Button Exports

Signature: `(int playerIndex, bool* trigger, bool* hold) -> void`

| Export | Description |
|--------|-------------|
| `arkMDXGetStart` | Start button |
| `arkMDXGetUp` | Menu up (distinct from panel up) |
| `arkMDXGetDown` | Menu down |
| `arkMDXGetLeft` | Menu left / select left |
| `arkMDXGetRight` | Menu right / select right |

### Operator Button Exports

| Export | Signature | Description |
|--------|-----------|-------------|
| `arkGetIOCoinTrigger` | `(uint* out) -> uint` | Coin inserted (trigger) |
| `arkGetIOServiceTrigger` | `(uint* out) -> uint` | Service button (trigger) |
| `arkMDXGetTestButton` | `(bool* test, bool* service) -> uint` | Test + service buttons |

### Panel Counter Exports

Signature: `(int playerIndex, uint* counter) -> void`

| Export | Description |
|--------|-------------|
| `arkMDXGetPanelCounterUp` | Cumulative up press count |
| `arkMDXGetPanelCounterDown` | Cumulative down press count |
| `arkMDXGetPanelCounterLeft` | Cumulative left press count |
| `arkMDXGetPanelCounterRight` | Cumulative right press count |

### Other Exports

| Export | Description |
|--------|-------------|
| `arkMDXGetEAPass` | e-amusement pass reader |
| `arkGetIOStartTrigger` | Start trigger for all 16 possible players (bool[16]) |
| `arkGetIOEAmusementTrigger` | e-amusement button trigger |
| `arkGetIOEAmusementHold` | e-amusement button hold |
| `arkGetIOSecretPressed` | Secret button combo |

## Internal Architecture

All `arkMDX*` functions follow the same pattern:
1. Get the MDX singleton via an internal getter (`FUN_1800d2860`)
2. Call a vtable method on it (each button type has its own vtable slot)
3. Write results to output parameters

The `arkGetIO*` functions use a separate IO singleton (`DAT_180d4cb20` or fallback `DAT_180d4c580`) with its own vtable.

Both are thread-safe reads from spice2x's IO layer.

## gamemdx.dll Input Manager (DAT_1806ebc70)

The game also maintains its own higher-level input state structure at global `DAT_1806ebc70`. This is what `UserFootPanel` reads from.

### Structure Layout

Total allocation: 0x1270 bytes. Contains 4 sub-player entries of 0x498 bytes each, plus a 0x10-byte footer.

```
Offset 0x0000: P1 left side  (sub-player 0, 0x498 bytes)
Offset 0x0498: P1 right side (sub-player 1, 0x498 bytes, doubles only)
Offset 0x0930: P2 left side  (sub-player 2, 0x498 bytes)
Offset 0x0DC8: P2 right side (sub-player 3, 0x498 bytes, doubles only)
Offset 0x1260: uint16 config (0x100)
Offset 0x1268: uint64 timestamp (from arkAnalyzeMsgHashAsInt / Ordinal_45)
```

### Per-Sub-Player Bitmask (offset 0x00 within each 0x498 entry)

```
Bit 5: Left arrow
Bit 6: Down arrow
Bit 7: Up arrow
Bit 8: Right arrow
```

- Offset +0x00: uint32 "wasJustPressed" bitmask
- Offset +0x04: uint32 "isHeld" bitmask

UserFootPanel methods index into this structure using `this+0x08` as the player base index and `panelIndex / 4` as the sub-player offset.

### Initialization

`FUN_180022850` allocates and initializes the structure. It calls `FUN_180022950` for each of the 2 player halves (stride 0x930 = 2 sub-players). References `"HIGH_PRECISION_INPUT"` config key.

## Polling Strategy

The `trigger` and `release` outputs from the ark exports are edge-detected — they are only true for **one game frame**. Polling at a lower rate (e.g., 10Hz) would miss rapid presses. Polling once per frame from the render hook (which fires once per frame on the main thread) is the ideal approach.

### Why Exports Over Hooking

| Approach | Pros | Cons |
|----------|------|------|
| **ark exports** | No hooking complexity, edge detection built-in, covers all button types, thread-safe | Cannot suppress input from reaching game |
| Hook gamemdx.dll input manager | Can suppress input (zeroing bitmask) | Complex bitmask parsing, only covers foot panel, fragile to updates |
| Hook per-consumer (UserFootPanel) | Per-consumer control | Misses non-gameplay scenes, multiple hook points needed |

### Input Consumption Limitation

Since the exports are read-only (not intercepting writes), input **cannot be suppressed from reaching the game** via this approach. Game-level input suppression would require hooking the gamemdx.dll input manager's write path.

## Address Resolution

No AOB signatures are needed. All addresses are resolved via named exports on `arkmdxbio2.dll`, which are stable across game updates (export names don't change).

## Button Inventory

### Player Buttons (per P1/P2)

| Button | Export | Has Release Edge |
|--------|--------|-----------------|
| Panel Up | `arkMDXGetPanelUp` | Yes |
| Panel Down | `arkMDXGetPanelDown` | Yes |
| Panel Left | `arkMDXGetPanelLeft` | Yes |
| Panel Right | `arkMDXGetPanelRight` | Yes |
| Start | `arkMDXGetStart` | No (trigger + hold only) |
| Menu Up | `arkMDXGetUp` | No |
| Menu Down | `arkMDXGetDown` | No |
| Menu Left | `arkMDXGetLeft` | No |
| Menu Right | `arkMDXGetRight` | No |

### Operator Buttons (global, not per-player)

| Button | Export |
|--------|--------|
| Coin | `arkGetIOCoinTrigger` |
| Service | `arkGetIOServiceTrigger` |
| Test | `arkMDXGetTestButton` |

### 10-Key Numpad (per P1/P2)

| Button | Index in outKeys array |
|--------|----------------------|
| 0-9 | 0-9 |
| * (star) | 10 |
| # (hash) | 11 |

Export: `arkMDXGet10Key(playerIndex, bool[12] outKeys)`. Numpad inputs are ignored by the game during attract mode and most gameplay scenes.

### Release Edge for Menu Buttons

Menu button exports only provide `trigger` and `hold` — no `release` output. Release events must be synthesized by tracking hold state: when `hold` transitions from true to false, a release event is implied.

## Build Version

All Ghidra addresses from **arkmdxbio2.dll** and **gamemdx.dll** build 20260324.
