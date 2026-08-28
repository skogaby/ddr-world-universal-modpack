# Implementation Plan — Native SMX Hardware Support

Decomposition of `design/detailed-design.md` (Status: Approved 2026-08-27). Each step
leaves something demonstrable on the cabinet, builds on the last, and lands core value
(playing DDR on SMX pads, pads lit in sync) in the first phase. Validation is
**on-device / manual** throughout — this is a near-verbatim port of mature code
(SpiceManiaX + the SMX SDK), so there is no host-test harness (see the design's
Testing Strategy). Ship diagnostics first, then observe on the cabinet.

Status: Approved 2026-08-27

## Checklist

- [x] Step 1: SMX foundation — HID transport, stage input injection, and stage lights
      (implemented + builds clean 2026-08-27; cabinet demo pending — see progress.md)
- [x] Step 2: Cabinet lights (marquee, vertical strips, spotlights)
      (COMPLETE — cabinet-validated 2026-08-27 through deploys #13–#15: parity
      port, improved marquee resampler + probe-discovered slot fix, strip
      linear interpolation — see progress.md)
- [x] Step 3: Touchscreen overlay + menu / keypad / card injection
      (COMPLETE — cabinet-validated 2026-08-28 through deploys #16–#20:
      touch capture (mouse under CrossOver), override-word menu injection
      incl. the IO test menu, momentary pinpad pulses + modpack gestures,
      card-in both sides, textured Gold-cab-styled TOPMOST overlay with
      lamp-lit menu buttons, corner-anchored scale, mod-menu SMX HARDWARE
      section, Gold/Platinum pad style — see progress.md)
- [x] Step 4: Lifecycle, config, diagnostics, docs, validation harness
      (COMPLETE 2026-08-28 — disable/enable reversal audited (+ card-episode
      cancel fix), config schema final, diagnostics shipped organically
      through deploys #1–#20, docs landed: `docs/smx_hardware_research.md`,
      AGENTS.md Key Entry Points row, README operator section incl. the
      Wine EnableHidraw prereq. Deviations from this step's original text:
      no `DDR_SMX_FAULT` env and no validation harness — every degradation
      path was exercised on hardware across 20 deploys, and D12 already
      waived host tests; dead dev scaffolding skipped.)

---

### Step 1: SMX foundation — HID transport, stage input injection, and stage lights

**Objective.** Full stage-level SMX integration, end to end and both directions: the
modpack discovers a connected SMX device over raw HID, reads its panel sensors and
injects them into the game as arrows, and mirrors DDR's Gold-Cab **stage** lighting
back onto the SMX pads. One demo proves the whole approach (transport + input +
stage lights) at once.

**Implementation guidance.**

*Mod + transport skeleton:*
- `mods/smx_hardware/mod.rs`: `Mod` impl (`id="smx-hardware"`, default OFF via the
  config `mods` map, `is_active()` self-disable if the transport can't start),
  registered in `src/lib.rs` (steps 2c/7). `enable()` starts the transport, installs
  the light-read detours, and turns on input injection; `disable()` reverses.
- `mods/smx_hardware/config.rs` + `SmxHardwareConfig` in `mods::config`
  (`p1card`, `p2card`, `overlay_opacity`, `overlay_enabled`, `output_lights`).

*HID transport (`services/smx/`):*
- `protocol.rs` (pure): `parse_input_report(&[u8;64]) -> Option<u16>` (report id 3 →
  `(buf[2]<<8)|buf[1]`); `frame_serial_command(&[u8]) -> Vec<[u8;64]>` (report-id-5
  chunking, START/END flags).
- `device.rs`: HID enumeration via the `windows` crate (`SetupAPI` + `HidD_*`), VID
  `0x2341`/PID `0x8037`, product-string filter (`StepManiaX`=stage /
  `SMXArcade`=cabinet), open `FILE_FLAG_OVERLAPPED`. Add the needed `windows` features
  (`Win32_Devices_HumanInterfaceDevice`, `Win32_Devices_DeviceAndDriverInstallation`).
- `transport.rs`: a dedicated thread (`ABOVE_NORMAL`) that polls discovery ~250 ms
  (connect/hot-plug), keeps a permanent overlapped `ReadFile` per device (→ `AtomicU16`
  per pad), and runs the ~30 Hz lights drain (below). API: `init`, `shutdown`,
  `is_available`, `input_mask(pad)`, `submit_lights(frame)` (latest-wins),
  `cabinet_model()`.

*Stage input injection:*
- `services/smx/input_map.rs` (pure): SMX 9-bit mask → DDR panels (Up←bit1, Down←bit7,
  Left←bit3, Right←bit5).
- Extend `services/input_manager.rs`: add detours for
  `arkMDXGetPanelUp/Down/Left/Right` (same `TriggerHoldFn` shape), and switch the
  getter detour bodies from *suppress-only* to *additive* (OR injected trigger/hold
  bits for game-side callers), all gated behind a new `set_injection_active(bool)`
  (default off) so behavior is unchanged unless the SMX mod turns it on. Preserve the
  `IN_MODPACK_POLL` re-entry contract.
- `mods/smx_hardware/input_inject.rs`: on enable, `set_injection_active(true)` +
  feed panel state from `transport::input_mask`; on disable, deactivate.

*Stage lights (read → map → output):*
- **Ghidra confirmation first:** decompile the `arkMDXIO` vtable targets (+0x3f0
  tapeled, +0x370/+0x378 lamp, +0x3c0 satellite) and cross-reference spice2x's
  `bi2a.cpp` off1/off2 device table + `mdxf.cpp` corner table to lock the exact
  `arkMDXChangeTapeled` arg→(device,LED) decode and the corner-light source.
- `mods/smx_hardware/lights_read.rs`: detour `arkMDXChangeTapeled` (accumulate into a
  `DdrLightFrame` `[11][50][3]` tape buffer keyed by device/LED) + the corner-light
  source export (per the Ghidra finding). Bodies forward-to-original then store
  (hot-path tight).
- `services/smx/light_map.rs` (pure): `map_stage(&DdrLightFrame) -> [StagePad; 2]`
  (arrows 25:1 from foot tape; corners = L-shape flags lit by the corner value over
  static gold `0xBB,0xBB,0x00`; center = gold).
- `services/smx/protocol.rs`: `encode_stage_lights` — the 3 tagged serial commands
  (`4`/`2`/`3`), 16-outer+9-inner ordering, ×0.6666 scale, V3 1/60 s gap vs V4 queue
  metadata.
- Wire the transport's 30 Hz drain to map+encode+write stage lights, gated on
  `output_lights` + `is_available`.

**Validation (on-device).** Fold into the Step 1 demo below; ship the diagnostics
(device connect + resolved model + input mask + first stage-light frame) first so the
port can be observed and reasoned about against the SpiceManiaX/SDK source. Keep
`protocol` / `input_map` / `light_map::map_stage` as isolated pure functions for easy
inspection.

**Integration.** Mod registers/enables through the existing registry; no existing
behavior changes when OFF. Input injection *extends* `input_manager`'s detour set (no
second detour on any owned target). Light-read detours are new targets. `cargo check
--target x86_64-pc-windows-msvc`, `cargo fmt`, `./build.sh` clean.

**Demo (single, end-to-end).** On the cabinet with the mod ON: play a full song on
the SMX pads **while** the pad arrows + corner L-shapes light in sync with DDR;
unplug/replug reconnects and resumes.

---

### Step 2: Cabinet lights (marquee, vertical strips, spotlights)

**Objective.** SMX marquee, side strips, and spotlights mirror DDR's cabinet lighting
— full lights parity with SpiceManiaX.

**Implementation guidance.**
- `services/smx/light_map.rs`: `map_marquee(top_panel 40 → 24)` (prefer-lit
  conflict-average), `map_strip(monitor 25 → 28)` (`MapValue` interpolate),
  `map_spotlights(woofer_brightness → 8 white)`.
- `services/smx/protocol.rs`: `encode_cabinet_light(dev, model, &[Rgb])` — the
  model-dependent `L`/`Q` command with per-device channel reorder (BRG/RBG), reverse
  (model-3 strips), and zero-pad to 32|8 triplets.
- `services/smx/transport.rs`: perform the `"I"` handshake on the cabinet device at
  connect → cache `cabinet_model()`; extend the 30 Hz drain to emit marquee + both
  strips + both spotlights.
- `mods/smx_hardware/lights_read.rs`: ensure the woofer-corner + top-panel + monitor
  sources are captured into `DdrLightFrame` (extend from Step 1 as needed).

**Validation (on-device).** Verify each cabinet device against DDR on the cabinet
(marquee sweep, strip gradients, spotlight brightness). Log the resolved cabinet
model + first cabinet-light frame per device.

**Integration.** Reuses the Step 1 drain + transport; the cabinet device is the 3rd
HID slot.

**Demo.** On the cabinet: marquee/strips/spotlights animate with DDR (title sweeps,
gameplay reactive lighting), stage lights + inputs from Step 1 unaffected.

---

### Step 3: Touchscreen overlay + menu / keypad / card injection

**Objective.** A native, in-render-path touchscreen overlay (menu nav, pinpad,
insert-card, per-player visibility toggle) whose presses drive menu/keypad/card
injection — working in exclusive fullscreen.

**Implementation guidance.**
- `mods/smx_hardware/overlay_model.rs` (pure): build the per-player button set +
  layout (ported from `overlay_utils.cpp`/`overlay_button.h`), and
  `hit_test(point) -> Option<button_id>` (point-in-(rotated-)rect).
- `mods/smx_hardware/overlay.rs`: render the buttons through the modpack overlay path
  (`overlay_draw` command-list quads + `widget_renderer` text), honoring
  `overlay_opacity` and per-player visibility.
- `mods/smx_hardware/touch.rs`: locate the game HWND; subclass its WndProc
  (`SetWindowLongPtrW` `GWLP_WNDPROC`, chaining original) + `RegisterTouchWindow`;
  handle WM_TOUCH (0.01 mm→px) + WM_LBUTTONDOWN/UP (debug) → hit-test → atomic button
  states. Restore the original WndProc + `UnregisterTouchWindow` on disable.
- Extend `input_inject.rs`: menu getters (additive from button states),
  `arkMDXGet10Key` (write `buf1` from pinpad state), `arkMDXGetEAPass` (write the
  configured card UID on Insert-Card). Visibility toggle is local (no injection).

**Validation (on-device).** On the cabinet in fullscreen: confirm touch coordinates
hit the right buttons (esp. the rotated menu buttons + pinpad grid), and that each
button drives the right injection. Keep `overlay_model` isolated for direct
inspection.

**Integration.** Overlay uses the same native render path as `mod_menu`; injection
reuses the Step 1 machinery; the touch subclass is the only net-new OS surface —
validate WM_TOUCH delivery in fullscreen early in the step.

**Demo.** On the cabinet in fullscreen: touching the overlay navigates menus, the
pinpad enters a PIN, Insert-Card logs a player in, and the visibility toggle
shows/hides per player.

---

### Step 4: Lifecycle, config, diagnostics, docs, validation harness

**Objective.** Production-harden: clean enable/disable reversal, full config wiring,
dev fault injection, diagnostics, and documentation.

**Implementation guidance.**
- `disable()` fully reverses: stop transport threads, remove light-read detours,
  `set_injection_active(false)`, restore the original WndProc + unregister touch, hide
  the overlay — so the game returns to stock input/lights and the mod toggles live
  from the mod menu.
- Finalize the `smx_hardware` config schema + defaults; fixed fields read once at
  enable (next-launch semantics); card ids/opacity apply as designed.
- `DDR_SMX_FAULT` (developer-mode-gated) env: `no-device` / `drop-lights` /
  `model=N` to exercise degradation without hardware.
- Diagnostics: INFO on device connect + resolved model + first light frame + first
  injected input; one-shot WARN per fallback class (missing export, no device, report
  anomalies).
- Docs: add the "Key Entry Points" row + Custom Instructions note in `AGENTS.md`; a
  README operator section (prereqs: Gold-Cab/BIO2, Dedicated Cabinet, use instead of
  spice2x SMX mapping).

**Validation (on-device).** Toggle the mod on/off repeatedly from the mod menu with
the cabinet attached and confirm clean reversal (no residual detours/subclass, stock
behavior when off); exercise `DDR_SMX_FAULT` paths to confirm graceful degradation.

**Integration.** Live mod-menu toggle on/off with no residual detours or window
subclass after disable; `cargo fmt` + `cargo check` + `./build.sh` clean.

**Demo.** Toggle `smx-hardware` on/off repeatedly from the in-game mod menu with the
cabinet attached: lights, inputs, and overlay come and go cleanly; the game is
stock-behaving when off; config edits take effect on next launch.
