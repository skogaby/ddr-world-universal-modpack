# Research — spice2x DDR IO (how DDR exposes lights / consumes inputs)

Source: `spice2x.github.io` (spice2x source). Paths below are relative to that
repo's `src/spice2x/`. This is the reference for what a native in-process port
must read/replace to reproduce SpiceManiaX without SpiceAPI.

## Board/cabinet variants

spice2x picks a DDR code path from the game DLL name (`launcher/launcher.cpp`):

| Game DLL | Cabinet | Light/Input mechanism |
|---|---|---|
| `arkmdxbio2.dll` | **DDR World GOLD & WHITE** (BIO2 board) | **ACIO function hooks** `ac_io_bi2a_*` + `ac_io_mdxf_*` (in `libacio.dll`) + a `COM1` ACIO device for the card reader |
| `arkmdxp4.dll` | GOLD/WHITE (P4IO board) | `\\.\P4IO` device hook + `ac_io_mdxf_*` |
| `arkmdxp3.dll`/`gamemdx.dll` (older) | SD/HD (P3IO) | `\\.\P3IO`, `COM1/COM2`, USBMEM device hooks + HDXS ACIO |

**Our target = DDR World GOLD = `arkmdxbio2.dll` (BIO2 + MDXF), lights via `libacio.dll` exports.**
Everything below is the BIO2/GOLD path.

## Lights OUT (game → IO), three hooked `libacio.dll` exports

1. `ac_io_bi2a_control_led_bright(size_t index, uint8_t brightness)` —
   `acio/bi2a/bi2a.cpp` (MDX block). GOLD menu/card-unit/title/**woofer-corner**
   lights. `index`→light table; value = `brightness/127`.
2. `ac_io_bi2a_control_tapeled_bright(off1, off2, r, g, b, bank)` —
   `acio/bi2a/bi2a.cpp`. Per-LED RGB for the **11 tape devices**; also derives the
   scalar `GOLD_*_AVG_*` lights. Stored into `games::ddr::DDR_TAPELEDS[11][50][3]`
   (`games/ddr/ddr.cpp`). `off1/off2`→device/LED; values 0–255 raw.
3. `ac_io_mdxf_set_output_level(a1, a2, value)` — `acio/mdxf/mdxf.cpp`. GOLD
   **stage-corner** lights. `a1`=17(P1)/18(P2), `a2`=0–3 corner; value = `value/128`.

### Named lights (match SpiceManiaX's SpiceAPI names)
`games/ddr/io.h` (enum) + `games/ddr/io.cpp` (`get_lights()` → `{category,name}`):
`GOLD P1/P2 Woofer Corner`, `GOLD P1/P2 Stage Corner {Up,Down}-{Left,Right}`, menu,
title, card-unit RGB, and the derived `*_AVG_*` scalars. **There is NO DDR "Marquee"
named light** — the marquee is driven from the `top_panel` tape device. `lights_read`
returns each light's `last_state` (`cfg/api.cpp`, `cfg/light.h`).

### Tape LED devices (`ddr_tapeled_get`, `api/modules/ddr.cpp`)
Index → name → LED count (flat `[r,g,b,...]`):
```
0 p1_foot_up      1 p1_foot_right   2 p1_foot_left   3 p1_foot_down   (25 LEDs each)
4 p2_foot_up      5 p2_foot_right   6 p2_foot_left   7 p2_foot_down   (25)
8 top_panel       9 monitor_left   10 monitor_right                   (50 each)
```
Note the foot order is **up/right/left/down**. Backing store `DDR_TAPELEDS[11][50][3]`.

## Inputs IN (you → game)

All emulated reads honor a per-object override: `GameAPI::Buttons::getState` returns
`override_state` when `override_enabled` (`cfg/api.cpp`). SpiceAPI `buttons.write`
sets that override (`api/modules/buttons.cpp`) then calls `mdxf_poll(true)`.

- **Arrow panels** (GOLD/WHITE): the **MDXF ring buffer** — `ac_io_mdxf_update_control_status_buffer`
  / `ac_io_mdxf_get_control_status_buffer` (`acio/mdxf/mdxf.cpp`). node 17/25=P1, 18/26=P2;
  sensor nibbles at entry bytes 4/5, timestamp (needs `arkGetTickTime64`) at 0x18.
- **Menu/Start/Service/Test/Coin**: `ac_io_bi2a_update_control_status_buffer` +
  `ac_io_bi2a_get_control_status_buffer` (272-byte buffer, `acio/bi2a/bi2a.cpp`).
- **Keypad (10-key)**: emulated inside the **ICCA** card reader device
  (`acioemu/icca.cpp` `update_keypad`); SpiceAPI `keypads.set` →
  `KEYPAD_STATE_OVERRIDES` in `misc/eamuse.cpp`. Bit order `EAM_IO_KEYPAD_*` in
  `misc/eamuse.h`. (spice2x uses a 150 ms dwell for DDR keypad pulses.)
- **Card insert**: `card.insert` → `eamuse_card_insert` (`misc/eamuse.cpp`) →
  ICCA `parse_msg`/`update_status` reports the UID on next poll.

## KEY finding for a native port — the `arkMDX*` layer sits ABOVE spice2x

`arkmdxbio2.dll` exports `arkMDXGet*` input getters and (to be confirmed)
`arkMDXSet*`-style light-output wrappers. The game calls these; they internally
call the `libacio` `ac_io_*` functions that spice2x IAT-hooks. **The modpack's
`input_manager` already detours `arkMDXGetStart/Up/Down/Left/Right` and
`arkMDXGet10Key`** to read and suppress input. So the modpack naturally sits *above*
spice2x's ACIO emulation — reading light output and injecting input at the
`arkMDX*` layer avoids double-hooking the `libacio` functions spice2x owns.

**Open research item:** identify the `arkmdxbio2.dll` light-OUTPUT export(s) the game
calls to set tape/corner/cabinet lights (the OUT analog of `arkMDXGet*`), OR confirm
whether lights must instead be read at the `libacio` layer / spice2x state / game
memory. This determines the lights-read hook point.

## Coexistence caveat

The modpack is loaded via spice2x's `-k` flag → **spice2x is present and already
hooks all `ac_io_*` / device paths.** Per repo rule "one detour per target
function," the modpack must NOT double-hook those. Hooking at the `arkMDX*` export
layer (above spice2x) is the clean seam. If spice2x's IO is *absent*, the game's
`arkMDX*`/`ac_io_*` calls hit real (missing) hardware and the ark IO singleton never
inits — so **spice2x-as-IO-emulator/loader is still required unless the modpack also
emulates the BIO2 board itself** (large, out of the likely scope).
