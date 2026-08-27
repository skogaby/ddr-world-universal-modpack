# Mod Menu Input Gating During Gameplay (Scene 28)

## Overview

The bulk-hack-porting feature (Q1, idea-honing.md) drops the mod menu's
`scene < 17` open-gate so the menu is reachable on any screen, including
gameplay (scene 28). The user's concern is whether the four numpad buttons
the menu uses for navigation — `NUM_2` (down), `NUM_4` (toggle off),
`NUM_6` (toggle on), `NUM_8` (up) — bleed through to game-side handlers
when the menu is open during gameplay.

The mod menu blocks input via `services::input_manager::set_exclusive_consumer`,
which intercepts events at the *modpack's* dispatcher. It does **not** patch
or reroute the underlying `arkmdxbio2.dll` exports. So if the game also reads
those exports (or reads from a downstream input struct that arkmdxbio2 still
populates), the exclusive-consumer is no defense.

The conclusion below is good news: the game *does* poll all input exports
including `arkMDXGet10Key` every frame, but **scene 28's main update loop
reads only the START button** (and the four foot-panel arrow bits). It never
inspects the numpad bits. No suppression hook is required.

## Existing input_manager Behavior

- `services::input_manager::poll()` is invoked from `wrapper_render_hook` —
  the modpack's hook on `agcs::BmpString::vtable[5]`. That vtable slot
  fires every render frame, **regardless of scene**, so polling and
  triple-press gesture detection work during gameplay (proof: autoplay's
  judge_hook subscriber already runs during scene 28; the render hook
  uses an even more universal entry point).
- `set_exclusive_consumer` swaps in a callback that consumes events
  *before* normal subscribers see them. The flag is per-event, not
  per-export.
- The modpack reads input via `GetProcAddress` on the same arkmdxbio2
  exports (`arkMDXGetStart`, `arkMDXGetUp/Down/Left/Right`,
  `arkMDXGet10Key`). Calling these from our render hook does **not**
  consume them — they are pure read-side accessors that return cached
  state from arkmdxbio2's I/O singleton.

So the mod menu's exclusive-consumer is correctly sufficient for the
*modpack's* internal subscribers (autoplay, future Quick Restart, etc.).
The remaining question is what the game itself does with the same
input data.

## Game-Side Pinpad Input Path Analysis

### arkmdxbio2 export → gamemdx export-pointer table

`gamemdx.dll` does not link to arkmdxbio2 at static-link time. Instead,
function `FUN_180004390` (`gamemdx_20260421.dll`; `FUN_180004310` on
20250805) walks a name table and `GetProcAddress`-resolves all 0x144
arkMDX/ark* exports into an internal pointer table. The relevant slot:

| Symbol | 20260421 | 20250805 |
|---|---|---|
| `arkMDXGet10Key` ptr | `DAT_1806ed408` | `DAT_1806b5098` |
| `arkMDXGetStart` ptr | `DAT_1806ed3b8` | `DAT_1806b5048` |
| `arkMDXGetUp` ptr | `DAT_1806ed3a8` | `DAT_1806b5038` |
| `arkMDXGetDown` ptr | `DAT_1806ed3b0` | `DAT_1806b5040` |
| `arkMDXGetLeft` ptr | `DAT_1806ed398` | `DAT_1806b5028` |
| `arkMDXGetRight` ptr | `DAT_1806ed3a0` | `DAT_1806b5030` |

Each pointer has exactly **one read xref** in gamemdx — from the input
aggregation function `FUN_180023130` (20260421) / `FUN_180022f80`
(20250805). No other gamemdx code calls `arkMDXGet10Key` directly.

### Input aggregation: every frame, regardless of scene

`FUN_180023130` is the per-frame input poller. It is called
unconditionally from `FUN_180003040` (20260421) — the master per-frame
tick (alongside `arkMDXDraw` etc.). The disassembly shows a
two-iteration outer loop over players (`uVar10 = 0..1`) and 0x1d-button
inner loop. For each player it merges every export's output into a
29-byte `local_260[]` buffer:

```
local_260[0]    |= arkMDXGetStart.trigger    // bit index 0 -> button START
local_260[1]    |= arkMDXGetUp.trigger       // bit index 1 -> MENU_UP
local_260[2]    |= arkMDXGetDown.trigger     // bit index 2 -> MENU_DOWN
local_260[3]    |= arkMDXGetLeft.trigger     // bit index 3 -> MENU_LEFT
local_260[4]    |= arkMDXGetRight.trigger    // bit index 4 -> MENU_RIGHT
local_260[5..8] |= panel arrows (Left/Down/Up/Right)
local_260[9..0x14] |= arkMDXGet10Key buf[0..11]   // numpad NUM_0..NUM_HASH
local_260[0x15..0x1c] = miscellaneous (eapass etc.)
```

It then calls `FUN_180022de0(player_idx, button_idx, active_byte, ...)`
0x1d times per player, which sets bit `(1 << button_idx)` in the
**per-player input bitmap** at `DAT_1806ede10 + player_idx * 0x498`
(20260421) / `DAT_1806b5a80 + player_idx * 0x498` (20250805). The
bitmap layout matches the existing `docs/input_system_research.md`
"Per-Sub-Player Bitmask" — bit 5..8 are panel arrows, **bits 9..20 are
the numpad keys NUM_0..NUM_HASH**. So *both* exports the modpack reads
and the game's own bitmap end up populated from the same arkmdxbio2
calls every frame.

Conclusion for question 1: **Yes**, the game polls the same arkmdxbio2
exports the modpack polls. They share the I/O driver. Hooking the
exports would affect both. The exclusive-consumer at the modpack's
input_manager level does not block game-side reads.

## Gameplay Scene Pinpad Handling

The game's gameplay scene (scene 28) is implemented by
`sequence::dance::GamePlayActor` (RTTI string at
`gamemdx_20260421!0x18047ef50`). Its update method is
`FUN_1800bc2e0` (20260421) / `FUN_1800b3a80` (20250805) — a large
per-frame state-machine that reads the per-player bitmap to advance
song timing, dispatch judge events, etc.

GamePlayActor consults the input bitmap through
`FUN_180023560(byte button_index)` (20260421) /
`FUN_1800233b0(byte button_index)` (20250805). This helper is a generic
"is-button-N-held-on-either-player?" accessor — it tests
`(1 << param_1) & (player_bitmap_p1 | player_bitmap_p2)`.

Disassembly of GamePlayActor's update around the two CALL sites
(20260421 addresses; 20250805 byte-identical apart from displacements):

```
; first call site -- 1800beab1
1800beaa6: CMP dword ptr [RAX + 0xb8],R12D     ; some pause/menu state check
1800beaad: JZ  0x1800beac7
1800beaaf: XOR ECX,ECX                         ; button index = 0 (START)
1800beab1: CALL 0x180023560                    ; is_button_held(0)
1800beab6: TEST AL,AL
1800beab8: JNZ 0x1800beac7                     ; START held -> skip branch

; second call site -- 1800befa2
1800bef88: JBE 0x1800bf161
1800bef8e: MOV EDX,0x1d
1800bef93: MOV RCX,R15
1800bef96: CALL 0x18020f180                    ; advance some sub-state
1800bef9b: JMP 0x1800bf161
1800befa0: XOR ECX,ECX                         ; button index = 0 (START)
1800befa2: CALL 0x180023560                    ; is_button_held(0)
1800befa7: ...                                 ; consume return
```

Both call sites use `XOR ECX, ECX` immediately before the call — the
button index passed in is **always zero**, which maps to the START
button (bit 0). GamePlayActor never queries any other bit through this
helper.

The other gameplay-relevant bit-readers in gamemdx — `FUN_18000d520`,
`FUN_18000d6a0`, `FUN_18000d790`, `FUN_180010bb0`, the lamp-driver
`FUN_18000e510`, the per-player bit accessors `FUN_180022820..940` —
all hard-code a `+5` shift bias and read **only bits 5..8** (the four
foot-panel arrows). None of them touch bits 9..20.

The numpad bits (9..20) are consumed in exactly one place reachable
from a normal scene update: `selectmusic::sequence::TenkeyPanel` (RTTI
string at `gamemdx_20260421!0x1804b9bb0`). That class lives in the
`selectmusic` namespace — i.e., **scene 25 (song select)**. There is
no `gameplay::TenkeyPanel`, `dance::TenkeyPanel`, or equivalent. A
broader string search for `tenkey` / `numericKey` / `10key` /
`Numeric` returns only the song-select assets
(`se_select_music_numerickey_window_in`, `musi_tenkey_*`,
`side_%dp_usr/tenkey%dp_usr/...`). All scene-25 namespaced.

The other notable numpad reader, `FUN_180021920` (called from
`FUN_18004cbc0`), iterates all 0x1d button bits but reads from
sub-player slot index `iVar1+2` — the doubles-mirror slot used by the
operator/I/O test screen (scene 1's child) — not the live gameplay
struct.

Conclusion for question 2: **The gameplay scene 28 update loop does
NOT read pinpad/numpad input.** The numpad bits land in the input
struct every frame, but no scene-28 code path inspects bits 9..20.
The user's concern is moot — there is no "bleed-through" to suppress.

## Suppression Strategy Recommendation

Given the analysis above:

> **Recommendation: do not install any pinpad-suppression hook.**

Reasons:

1. **No bleed-through exists.** Scene 28's update path queries only
   bit 0 (START) via `FUN_180023560` and bits 5..8 (panel arrows) via
   the `+5`-biased accessors. Numpad bits 9..20 are written to the
   input bitmap every frame but never read by any gameplay-active
   consumer.
2. **A hook would add risk for no benefit.** The two candidate
   suppression strategies the prompt enumerated both have downsides:
   - **Hook arkmdxbio2 `arkMDXGet10Key` export.** Would zero numpad
     state for *all* callers. The modpack itself depends on this
     export; it would need a "passthrough when menu closed, zero when
     menu open" variant, plus the modpack's own input_manager would
     need to track real state independently (the exclusive consumer
     already handles that, but bypassing the export entirely loses
     trigger/edge information for the modpack subscribers themselves).
     Crosses a process boundary (different DLL).
   - **Hook the gamemdx pinpad-read function.** No such function
     exists for scene 28 — there is nothing to hook. The only readers
     are scene-25 (`TenkeyPanel`, intentional) and operator-mode
     (intentional). Hooking either would regress legitimate
     functionality.
3. **Defensive note.** If a future game version adds a pinpad reader
   to GamePlayActor (unlikely — the design intent for arcade DDR is
   that pinpad is "operator panel only"), the simplest fix is the
   shared dispatcher pattern: hook `FUN_180023560`/its successor and
   return 0 for indices 9..20 while the menu is open. That is a
   5-byte detour on a 0x24-byte function — trivial to add later if
   needed. Document this fallback in the task notes; do not implement
   it preemptively.

The Q1 answer ((b) "Drop both gates AND ensure exclusive input
consumption holds during gameplay") is satisfied **by the existing
input_manager exclusive-consumer alone**. Implementation of Q1 is just:
delete the `current_scene() > ATTRACT_SCENE_MAX` guard in `open()` and
the auto-close callback in `enable()`. No new hook needed.

### Verification plan post-deploy

When the menu-open-on-any-scene change is deployed, confirm during
gameplay (scene 28):

1. Open menu via triple-5 during a song. Menu should appear.
2. Press NUM_2 / NUM_4 / NUM_6 / NUM_8 to navigate. Menu cursor
   should move; **gameplay should not react** (no Quick Restart
   trigger from triple-1, no song-select panel response).
3. Confirm panel arrow inputs (the foot panels) still drive judgments
   — those are bits 5..8 and were never under exclusive-consumer
   control. (They feed `judgeNotes` directly via the `IFootPanel`
   actor path, independent of arkmdxbio2 menu exports.)

If step 2 shows visible game reaction to numpad presses, revisit — a
new code path is reading those bits and the static analysis missed it.
That is the *only* condition under which a suppression hook becomes
necessary.

## AOB Anchors (if new hooks needed)

Keeping these on file in case the post-deploy verification surfaces a
problem and we need to install a fallback suppressor on the shared
helper.

### `is_button_held(byte index)` — `FUN_180023560` / `FUN_1800233b0`

Tiny function, 0x24 bytes. The most stable anchor is its prologue + the
`(P1 | P2) >> bit` combine.

Disassembly (20260421, identical structurally on 20250805):

```
180023560: B8 01 00 00 00         MOV EAX, 1
180023565: 8B C9                  MOV ECX, ECX
180023567: D3 E0                  SHL EAX, CL
180023569: 48 8B 0D ?? ?? ?? ??   MOV RCX, [RIP+disp]   ; -> DAT_1806ede10
180023570: 85 81 34 09 00 00      TEST [RCX + 0x934], EAX
180023576: 75 11                  JNZ +0x11
180023578: 85 81 CC 0D 00 00      TEST [RCX + 0xDCC], EAX
18002357E: 74 0D                  JZ +0x0D
180023580: B8 01 00 00 00         MOV EAX, 1
180023585: ...                    ret-with-1 / ret-with-0 epilogue
```

Proposed AOB pattern (wildcards = the RIP-relative disp32 to the input
struct global):

```
B8 01 00 00 00 8B C9 D3 E0 48 8B 0D ?? ?? ?? ?? 85 81 34 09 00 00 75 ?? 85 81 CC 0D 00 00
```

Verified-unique check: search the immediate prologue
`B8 01 00 00 00 8B C9 D3 E0`:
- 20260421: matches at `0x180023560` and 16 other unrelated sites that all
  happen to share the `MOV EAX,1; MOV ECX,ECX; SHL EAX,CL` head. The full
  pattern (with the two `TEST` instructions and their structural offsets
  `0x934` and `0xDCC`) is the disambiguating part — the constants
  `0x934 = 2 * 0x498 + 4` and `0xDCC = 3 * 0x498 + 4` encode the
  per-sub-player stride into the input bitmap. Both are structurally
  invariant across versions (the input struct layout has not changed).
- 20250805: same byte sequence resolves at `0x1800233b0`.

Hook semantics if/when needed: install a retour detour on the matched
function. Implementation:

```rust
unsafe extern "C" fn is_button_held_hook(button_index: u8) -> u64 {
    if button_index >= 9 && button_index <= 20 {
        // Numpad bits — return 0 while the mod menu is open.
        if mod_menu::MOD_MENU_STATE.lock().unwrap().is_open {
            return 0;
        }
    }
    ORIGINAL_DETOUR.call(button_index)
}
```

This is a single-byte-range hook on a single function. No struct-layout
discovery required. Total complexity: ~10 lines of new code if the
fallback is ever needed.

### Numpad-only export hook (alternative, NOT RECOMMENDED)

For completeness — if we ever needed to suppress numpad at the export
boundary (e.g., to also block `selectmusic::TenkeyPanel` during a
hypothetical "menu open during song select" scenario, which the user
has not requested), the hook target is the resolved function pointer
in `arkmdxbio2.dll`'s export table. No AOB scan needed —
`GetProcAddress(arkmdxbio2, "arkMDXGet10Key")` returns the address
directly. Wrap the export with a detour that zeroes the output buffer
when `mod_menu::is_open()` returns true.

Trade-off vs. the helper-function hook: the export hook affects
everyone (game + modpack), so the modpack's own subscribers would
miss the menu-navigation events too — the menu would self-suppress
its own navigation. Not viable without splitting the modpack's input
read path away from the export. Recommend AGAINST this approach.

## Cross-Version Notes

All structural facts in this document were verified on **both**
`gamemdx_20260421.dll` (current default) and
`gamemdx_20250805_MODIFIED.dll`:

| Fact | 20260421 anchor | 20250805 anchor |
|---|---|---|
| Input aggregation function | `FUN_180023130` | `FUN_180022f80` |
| `arkMDXGet10Key` ptr global | `DAT_1806ed408` | `DAT_1806b5098` |
| Per-button update | `FUN_180022de0` | `FUN_180022c40` |
| Input bitmap base | `DAT_1806ede10` | `DAT_1806b5a80` |
| `is_button_held(idx)` helper | `FUN_180023560` | `FUN_1800233b0` |
| GamePlayActor update | `FUN_1800bc2e0` | `FUN_1800b3a80` |
| GamePlayActor START-test pattern | `XOR ECX,ECX; CALL is_button_held` (x2) | identical |
| Per-player bitmap stride | `0x498` | `0x498` |

The structural invariants — single arkMDXGet10Key call site, identical
input-aggregation skeleton, only-START-tested in GamePlayActor, no
numpad readers in scene 28 — hold on both versions. The input pipeline
is one of the more stable subsystems across DDR World builds; the same
0x498-byte struct stride and 5..8 panel-arrow bit numbering have
appeared in every version inspected.

If a future build relocates the I/O bitmap or changes the helper's
calling convention, the anchors above would re-resolve via:

1. AOB-scan the `arkMDXGet10Key` string. Walk the string's data xref
   chain to recover the function-pointer slot.
2. Walk the slot's read xrefs — should still be one call site (the
   input aggregator).
3. Walk the input bitmap global's read xrefs and look for the
   helper's signature shape (`MOV EAX,1; SHL EAX,CL; TEST [reg+0x934];
   TEST [reg+0xDCC]`).

These are all derivable at runtime without any hardcoded offsets.

## Gotchas

- The input aggregation reads `arkMDXGet10Key` regardless of scene.
  Even in attract demo mode the numpad bits are flowing into the
  bitmap. They are simply not consumed by any active actor outside
  scene 25. Hooking the aggregation function would NOT save CPU
  (it is already O(1) per export) and would risk breaking song
  select.
- `services::input_manager::poll()` runs on the render thread (per
  `wrapper_render` hook). It does not run on a background timer.
  The poll latency is bounded by the game's render rate, which
  remains active during scene 28. Confirmed by inspection of
  `widget_renderer.rs:94` — same hook drains `pending_updates` and
  calls `input_manager::poll()`.
- The exclusive-consumer callback returns `bool` indicating whether
  the event was consumed. Returning `true` blocks downstream
  modpack subscribers but **never** affects the game itself. This
  is correct behavior — confirmed via `input_manager.rs:228-237`.
- The modpack's input_manager polls the menu-button exports
  (`arkMDXGetStart/Up/Down/Left/Right`), not just the 10-key. Pinpad
  inputs that map to MENU_UP/DOWN/LEFT/RIGHT (per the test mode
  reading) would also reach the modpack subscribers. Per the existing
  `mod_menu.rs:251-266`, the menu treats `MENU_UP` and `NUM_8` (etc.)
  as equivalent navigation; the exclusive-consumer absorbs both.
  Game-side, MENU_UP/DOWN/LEFT/RIGHT live at bits 1..4 of the bitmap;
  the same audit applies — only START (bit 0) and panel arrows
  (bits 5..8) are read by GamePlayActor.
