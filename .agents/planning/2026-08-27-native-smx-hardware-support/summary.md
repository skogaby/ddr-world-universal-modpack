# Summary — Native SMX Hardware Support (PDD pass complete)

Date: 2026-08-27 · Status: planning complete, ready for implementation.

## What this is

A new top-level mod, `smx-hardware`, that bakes StepManiaX Dedicated Cabinet IO
directly into the modpack DLL — reading the game's Gold-Cab light output and driving
the SMX cabinet's lights, injecting the SMX stage panels as game input, and rendering
a native touchscreen overlay — **without SpiceManiaX or the SpiceAPI TCP hop**. It's a
port of the maintainer's SpiceManiaX app into the DLL, for lowest-latency in-process
IO. spice2x (or bemanitools) stays the loader/IO-emulator; the mod hooks the game's
own `arkMDX*` layer, so it's loader-agnostic.

## Artifacts created (`.agents/planning/2026-08-27-native-smx-hardware-support/`)

- `rough-idea.md` — the initial concept + scope.
- `idea-honing.md` — the decision register (D1–D12 accepted/assumed) + ratification log.
- `research/orientation.md` — modpack integration surface + unknowns.
- `research/spice2x-ddr-io.md` — how DDR exposes lights / consumes inputs (spice2x).
- `research/smx-sdk.md` — the SMX SDK API, transport, wire formats, integration options.
- `research/ark-mdx-io-layer.md` — the confirmed `arkMDXIO` hook-point map (Ghidra).
- `design/detailed-design.md` — the approved detailed design (Status: Approved, amended).
- `implementation/plan.md` — the approved 4-step plan (Status: Approved).
- this `summary.md`.

## Design in brief

Two layers:
- **`services/smx/`** — game-agnostic SMX transport (pure Rust, `windows`-crate raw
  HID; no `SMX.dll`, no C++): `protocol` (HID framing + stage/cabinet wire encoders),
  `light_map` (DDR→SMX mapping), `device` + `transport` (discovery, overlapped IO,
  input cache, 30 Hz lights drain, `"I"` model handshake, hot-plug).
- **`mods/smx_hardware/`** — game integration: `lights_read` (detour the `arkMDX*`
  light-out exports), `input_inject` (extend `input_manager` to detour
  `arkMDXGetPanel*` + drive all getters additively), `overlay` + `touch` +
  `overlay_model` (native render via the modpack overlay path + game-HWND WndProc
  subclass for WM_TOUCH).

Key decisions: D1 keep spice2x as loader, replace only SpiceManiaX+SpiceAPI
(loader-agnostic); D2 full Rust port (macOS-buildable, single artifact); D3 hook the
`arkMDX*` light-out layer above spice2x; D4 native render + game-HWND touch
(exclusive-fullscreen-correct); D5 extend `input_manager` for injection; D12 on-device/
manual validation only (no host-test harness).

## Plan in brief (4 steps, each a cabinet demo)

1. **SMX foundation** — HID transport + stage input injection + stage lights (one
   end-to-end demo: play a song on the SMX pads with the pads lit in sync).
2. **Cabinet lights** — marquee, vertical strips, spotlights.
3. **Touchscreen overlay** — menu nav / pinpad / insert-card / visibility toggle +
   menu/keypad/card injection.
4. **Lifecycle, config, diagnostics, docs** — clean live toggle, degradation, docs.

## Next steps

1. Run the **code-task-generator** sop against `implementation/plan.md` — Step 1 first
   (it processes one PDD step at a time, so lessons from Step 1 shape Step 2's tasks).
2. Run the **code-assist** sop on each generated task in order.
3. Deploy Step 1 to the cabinet (`scripts/deploy.sh`) and run its demo before starting
   Step 2. Per repo policy, commits are the maintainer's to make — I'll leave work
   staged/unstaged and report, not commit.

## Assumptions / things to confirm during implementation

- Exact `arkMDXChangeTapeled` arg→(device,LED) decode + the corner-light source export
  — the first task inside Step 1 (Ghidra vtable pass, cross-referenced to spice2x's
  device/corner tables).
- Locating + subclassing the game HWND and WM_TOUCH delivery under exclusive
  fullscreen — prove out early in Step 3.
- Getter-injection latency vs. the 1000 Hz SpiceAPI baseline — cabinet-measure in
  Step 1; add a dedicated input thread only if needed (D7).
- Overlay default visibility (per-player hidden until toggled, per SpiceManiaX) and the
  `smx_hardware` config defaults — settle at implementation, easy to adjust.
