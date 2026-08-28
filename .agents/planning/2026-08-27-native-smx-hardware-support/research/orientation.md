# Orientation — Native SMX Hardware Support

My Step-2 blind-spot pass: how the idea fits the modpack, the natural integration
surfaces, and the unknowns worth escalating. Companion notes:
`research/spice2x-ddr-io.md`, `research/smx-sdk.md`.

## What the modpack already gives us (the porting surface)

The four SpiceManiaX feature areas map onto existing modpack machinery:

| SpiceManiaX does (today, via SpiceAPI) | Modpack native equivalent |
|---|---|
| Reads DDR lights over SpiceAPI (`lights_read`, `ddr_tapeled_get`) | **Hook the `arkmdxbio2.dll` light-output layer** (above spice2x). Surface TBD — see Unknown U1. |
| Writes stage/menu inputs via `buttons.write` (1000 Hz) | **`input_manager`** already detours `arkMDXGetStart/Up/Down/Left/Right` + `arkMDXGet10Key` on `arkmdxbio2.dll` and suppresses/passes them. Injection = make those getters return SMX-derived state to the game. |
| Keypad via `keypads.set`; card via `card.insert` | Keypad → the same `arkMDXGet10Key` layer. Card-in → TBD (Unknown U2). |
| Direct2D transparent topmost overlay window (WM_TOUCH) | Modpack renders overlays **natively in the game's own pipeline** (`overlay_draw` emits into the game command list; `mod_menu` is a full tabbed modal). No separate window; no touch input infra yet — see Decision D4. |
| Talks to SMX via `SMX.dll` (LoadLibrary) | Same `SMX.dll`, LoadLibrary'd from the cdylib (SDK research option A). |
| Own process, 4 media timers (1 ms inputs, 33 ms lights/overlay, etc.) | Modpack has render-thread frame callbacks (`input_manager::on_frame`, `poll` at native refresh) + background threads. The SDK owns its own HID threads. |

Key structural facts:
- **Mod system** (`src/mods/mod_trait.rs`): `Mod` trait (`id/name/description/
  required_signatures/init/enable/disable/is_active/early_apply`); registered +
  enabled in `src/lib.rs::init()`; toggleable live from the mod menu.
- **`input_manager` is the injection seam** (`src/services/input_manager.rs`). It
  already owns the `arkMDXGet*` detours, a render-thread `poll()`, per-frame
  callbacks, an exclusive-consumer channel, and game-side input suppression while an
  overlay is open. Panel (arrow) getters are NOT yet read here — only menu + 10-key.
- **Overlays are native** (`src/services/overlay_draw/`, `src/mods/mod_menu/`): the
  DLL draws into the game's screen command list (untextured quads, shader binds, VS
  consts) and via native widgets. **The codebase never touches the game HWND, D3D,
  or Windows touch APIs.**
- **cdylib** (`x86_64-pc-windows-msvc`, cargo-xwin), nightly, `windows` 0.58 +
  `retour`. Deployed by `scripts/deploy.sh`; validated by live cabinet runs.
- Loaded via **spice2x's `-k` flag** (spice2x is the loader today).

## The natural architecture

A `smx-hardware` mod that, on enable:
1. **Lights (read → drive SMX):** hook the game's `arkmdxbio2` light-output layer
   to capture tape-LED + named-light state; a worker (or the SDK's own threads)
   maps them to SMX stage/marquee/strip/spotlight payloads exactly as
   `SpiceManiaX/lights_utils.cpp` does; call the SDK setters.
2. **Inputs (SMX → inject):** the SDK's update callback caches the 9-bit panel mask
   per pad; `input_manager` (extended to also drive the panel/arrow getters) returns
   SMX-derived panel + menu + keypad state to the game.
3. **Overlay:** a native touch overlay (menu-nav / pinpad / card-in / visibility)
   rendered through the modpack's overlay path, with touch captured on the game
   window (Decision D4).
4. **SDK glue:** `LoadLibrary("SMX.dll")` FFI shim (SDK research option A), as an
   optional service with `is_available()`.

## Unknowns worth escalating (need research or a maintainer call)

- **U1 — lights-read hook point.** Confirm the `arkmdxbio2.dll` export(s) the game
  calls to *set* tape/corner/cabinet lights (the OUT analog of `arkMDXGet*`). If
  none, fall back to hooking `libacio` `ac_io_*` (double-hook risk vs. spice2x) or
  reading spice2x's `DDR_TAPELEDS` state. **Blocks the lights half of the design.**
- **U2 — card-in / keypad injection.** Confirm whether card insert can be driven at
  the `arkMDX*` layer, or needs the ICCA/eamuse path (which spice2x owns). Keypad is
  likely the `arkMDXGet10Key` layer we already hook.
- **U3 — panel/arrow input getter.** Which `arkmdxbio2` export the game reads arrow
  panels from (analog of `arkMDXGetStart`), for gameplay-input injection at 1000 Hz.
- **U4 — spice2x dependency.** Whether "spice2x not a strict requirement" means only
  "no SpiceManiaX/SpiceAPI" (spice2x still the loader/IO-emulator) or truly no
  spice2x (modpack must emulate the BIO2 board — very large). **Highest blast radius.**
- **U5 — touch input capture.** How touch reaches the DLL: subclass the game HWND +
  `RegisterTouchWindow`/WM_TOUCH, a message hook, raw input, or a separate D2D window
  like SpiceManiaX. No precedent in the codebase.
- **U6 — SMX.dll build + deploy.** Producing an x64 `SMX.dll` (fork is Win32-only in
  the vcxproj) and shipping it to the cabinet; or static-link (option B) / Rust port (C).
- **U7 — timing model.** SpiceManiaX polls inputs at 1000 Hz on a media timer; the
  modpack's input poll runs on the render thread (~display refresh). Whether the SDK's
  event-driven callback (immediate on HID interrupt) + render-thread injection is low
  enough latency, or a dedicated input thread is warranted.

## Proposed sequence

1. Ratify the decision register below (esp. D1 spice2x-scope, D2 SDK integration,
   D4 overlay/touch) — these gate everything.
2. Targeted research to close U1/U2/U3 (the `arkMDX*` light-out + arrow/card exports)
   — likely a Ghidra/exports pass on `arkmdxbio2.dll` (the maintainer has an SMX
   Ghidra project open) + a spice2x cross-check. U5 (touch) and U6 (SMX.dll build)
   in parallel.
3. Then detailed design + phased plan.

I'll begin the U1/U3 research in parallel with your review of the register unless you
redirect.
