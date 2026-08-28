# Rough Idea — Native StepManiaX Hardware Support

Add native **StepManiaX (SMX)** hardware support as a new top-level mod in the
DDR World Universal Modpack DLL. When enabled, the mod talks directly to a
StepManiaX cabinet — driving its lights, reading its panel sensors, and
rendering a touchscreen overlay — from inside the game process, bypassing
`spiceapi` entirely for top-tier performance.

This is largely a **porting effort** from three existing repos the maintainer owns:

- **`stepmaniax-sdk`** (fork of the official SMX SDK, `github.com/skogaby/stepmaniax-sdk`):
  adds *dedicated cabinet IO* (marquee, strips, spotlights) on top of stock stage
  pad IO. Builds `SMX.dll` with a flat `extern "C"` API.
- **`SpiceManiaX`** (`github.com/skogaby/SpiceManiaX`): a standalone Windows app the
  maintainer wrote that middlemans between a running spice2x DDR (via SpiceAPI over
  loopback TCP) and SMX hardware (via the SDK). Provides: full stage + cabinet
  lighting, 1000 Hz stage inputs, a Direct2D touchscreen overlay (menu nav, pinpad,
  card-in, per-player visibility toggle), and automatic IO mapping.
- **`spice2x.github.io`**: the spice2x source, showing what it hooks for DDR IO
  (Gold Cabinet / BIO2 lights + tape LEDs, panel/menu/keypad/card inputs) and how
  SpiceManiaX interfaces with DDR through SpiceAPI today.

## Scope (maintainer-stated)

1. **Cabinet + stage lights**: read the game's Gold-Cabinet light outputs (named
   lights + RGB tape LEDs) and drive SMX stage panels, marquee, vertical strips,
   and spotlights — the full mapping SpiceManiaX does today.
2. **Inputs**: read SMX panel sensors (1000 Hz) and inject them into the game as
   stage inputs; also menu / keypad / card-in.
3. **Touchscreen overlay** (explicitly in scope): the SpiceManiaX Direct2D overlay —
   per-player menu-nav buttons, a 10-key pinpad, an insert-card button, and a
   per-player overlay visibility toggle.
4. Bake all IO into the modpack DLL so **`spiceapi` is bypassed** and **spice2x is
   not a strict requirement** for the SMX bridging.

## Motivation

Native, in-process IO removes the SpiceManiaX process + the SpiceAPI TCP round-trip
(JSON-RPC over loopback), for the lowest possible input/light latency, and collapses
"DDR + spice2x + SpiceManiaX + SMX.dll" into "DDR + modpack DLL (+ SMX.dll)".
