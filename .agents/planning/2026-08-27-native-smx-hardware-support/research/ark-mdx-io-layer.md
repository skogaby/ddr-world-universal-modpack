# Research — the `arkMDXIO` export layer (confirmed hook points)

Confirmed by Ghidra on `arkmdxbio2_20260721.dll` (DDRWorld_Ghidra project). This
closes U1 (lights-out), U2 (card-in), U3 (arrow input). Every `arkMDX*` export is a
thin wrapper: `plVar1 = SingletonArkMDXIO::mdxIO(); (*plVar1->vtable[slot])(plVar1,
args...)`. The singleton accessor is `FUN_1800d2860` = `SingletonArkMDXIO::mdxIO`
(`programs\system\mdxIO\singletonArkMDXIO.cpp`, global `gInterfaceArkMDXIO_` @
`DAT_180c43658`) — the SAME pointer `input_manager::resolve_io_singleton_ptr`
already derives and gates polling on.

These wrappers sit **above** the `libacio` `ac_io_bi2a_*`/`ac_io_mdxf_*` functions
spice2x hooks — so detouring the `arkMDX*` exports never double-hooks spice2x, and
is **loader-agnostic** (works under bemanitools too, as long as the loader supplies
the IO emulation the vtable methods ultimately call). All exports are named + carry
stable ordinals; `input_manager` already resolves them via `GetProcAddress`.

## Input getters — DETOUR to INJECT (SMX → game)

Same `(i32 player, *out_a, *out_b)` shape family. `input_manager` already detours
Start/Up/Down/Left/Right + Get10Key (today only to *suppress* for game-side callers
while an overlay is open, via `IS_INPUT_SUPPRESSED` + the `IN_MODPACK_POLL`
re-entry flag). Injection = the same detour body, but ADD SMX-derived state to the
out-params for game-side callers instead of only zeroing.

| Export | vtable slot | Role | In modpack today |
|---|---|---|---|
| `arkMDXGetPanelUp/Down/Left/Right` | +0x310 (Up) … | **Arrow/stage panels** (gameplay) | not hooked yet — ADD |
| `arkMDXGetStart/Up/Down/Left/Right` | +0x2e0 (Start) … | Menu nav | detoured (suppress) |
| `arkMDXGet10Key(p, *buf1[12], *buf2[12])` | +0x308 | Keypad (10-key) | detoured (suppress) |
| `arkMDXGetEAPass(p, *out, *out)` | +0x2d8 | **e-Amuse card scan** (card-in) | not hooked — ADD |
| `arkMDXGetTestButton` | — | Test button | — |
| `arkMDXGetPanelCounterUp/...` | — | Panel press counters | — |

- ~~Panel getters share the `TriggerHoldFn = (i32, *u32, *u32)` shape~~ **WRONG —
  cabinet-caught boot crash 2026-08-27.** The panel getters take **five** args:
  `(player, *state_u8, *prev_state_u8, *sensors_a_u64, *sensors_b_u64)` — the
  wrapper prologue saves R9 and forwards a 5th stack arg, gamemdx's input poll
  (`FUN_180023830` @ 20260616) passes two u8 out-locals + two 8-byte sensor
  buffers, and the `MdxHWIO` impl (`FUN_1800c9a30`) writes through ALL FOUR
  pointers unconditionally. A 3-arg detour forwards garbage as the sensor
  out-pointers → wild write → EXCEPTION_ACCESS_VIOLATION at the first input
  poll. The MENU getters (`arkMDXGetStart/Up/Down/Left/Right`) really are
  3-arg. Out pair = (current, previous) state — gamemdx consumes only the
  current byte and derives edges downstream, so injection = OR the held level
  into out1's low byte.
- **Card-in (U2 closed):** detour `arkMDXGetEAPass` and write the configured card
  UID into its out-buffer when the overlay "Insert Card" button is pressed — a fully
  native path at this layer (no eamuse/ICCA/SpiceAPI needed).

## Light setters — DETOUR to READ (game → SMX)

Detour these, read the args (the light values), forward to the original. This is the
lights-capture surface (U1 closed).

| Export | vtable slot | Args (Ghidra) | Maps to (SpiceManiaX / spice2x) |
|---|---|---|---|
| `arkMDXChangeTapeled` | +0x3f0 | `(p1,p2,p3,p4,p5)` | Per-LED RGB tape — mirrors spice2x `ac_io_bi2a_control_tapeled_bright(off1,off2,r,g,b,bank)`. Feet (25 ea), top_panel/monitor (50 ea). → SMX stage arrow panels + marquee + vertical strips |
| `arkMDXSetLamp` | +0x370 / +0x378 | `(id, char on)` | Named on/off lamp (menu, woofer corner, card unit, title) → SMX corner/spotlight brightness |
| `arkMDXChangeDimlamp` | +0x3d8 | `(id, level)` | Dimmable lamp |
| `arkMDXChangeSatellite[Separate]` | +0x3c0 / +0x3d0? | `(p1..p5)` | Satellite/side light (likely stage-corner analog of `ac_io_mdxf_set_output_level`) |
| `arkMDXResetAllLamp` | — | — | reset |

**Design-time confirmation (not blocking):** decompile the vtable targets
(+0x3f0 tapeled, +0x370/+0x378 lamp, +0x3c0 satellite) and cross-reference spice2x's
`bi2a.cpp` `off1/off2`→device table + `mdxf.cpp` corner table to lock the exact
per-arg device/LED semantics before implementing the DDR→SMX map. The strong
inference (from the 5-arg shape + spice2x's per-LED path) is
`arkMDXChangeTapeled(off1, off2, r, g, b)` per LED.

## Consequences for the design

- **Lights read**: one detour on `arkMDXChangeTapeled` accumulating into an
  `[11][50][3]` buffer keyed by (off1,off2) exactly like spice2x's `DDR_TAPELEDS`,
  plus detours on `arkMDXSetLamp`/`ChangeDimlamp`/`ChangeSatellite` for the named
  scalar lights. Then map → SMX payloads (port `lights_utils.cpp` verbatim).
- **Input inject**: extend `input_manager` to detour `arkMDXGetPanel*` (arrows) and
  drive all getters (panels/menu/keypad/card) from SMX state for game-side callers —
  reusing the existing suppression/`IN_MODPACK_POLL` machinery (make it additive).
- **Shared-detour discipline**: `input_manager` already owns Start/Up/Down/Left/Right
  + Get10Key detours. The SMX mod must EXTEND that ownership (add panel/EAPass +
  injection), not install a second detour on the same targets (repo rule).
