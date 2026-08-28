# Native SMX Hardware Support — RE research notes

Reverse-engineering record for the `smx-hardware` mod (`src/services/smx/` +
`src/mods/smx_hardware/`): native StepManiaX Dedicated Cabinet support —
pads as stage input, DDR lights mirrored to the pads/cabinet, and a native
touchscreen overlay (menu nav / pinpad / card-in) — replacing the external
SpiceManiaX app + SpiceAPI loopback.

Addresses are file-relative to `0x180000000`. Primary targets:
`arkmdxbio2_20260721.dll` (the ark IO layer) and `gamemdx_20260721.dll`.
The deploy-by-deploy trail (deploys #1–#20) lives in
`.agents/planning/2026-08-27-native-smx-hardware-support/progress.md`.

## 1. The ark IO layer (`arkmdxbio2`)

### 1.1 Singleton + vtable

- `SingletonArkMDXIO::mdxIO` = `FUN_1800d2860`; singleton pointer global
  `DAT_180c43658`. Concrete impl class `MdxHWIO` (ctor `FUN_1800cdd80`,
  vftable @ `0x1800F7C88`).
- gamemdx resolves every `arkMDX*` export via `GetProcAddress` at boot
  (`FUN_1800042f0` — a ~0x144-entry name/slot table). Each export is a thin
  wrapper: `mdxIO()` → vtable slot call.
- Runtime resolution in the DLL is loader-agnostic: the singleton pointer is
  derived by walking `arkMDXGetStart` → its single `CALL rel32`
  (`get_io_state`) → the `MOV RAX,[RIP+disp32]` inside it
  (`input_manager::resolve_io_singleton_ptr`). Vtable impls are read from
  the LIVE object's vtable (no AOBs, build-independent), bounds-checked
  against the ark module.

### 1.2 Input: three consumer layers (the recurring lesson)

Every input surface has (a) export callers (gamemdx), (b) ark-internal
vtable callers (counters, entry-flow scenes), and (c) the operator TEST
MENU reading the RAW DIGEST upstream of everything. Injecting at the wrong
layer looks correct in-game and silently misses the others (deploys #4,
#16).

| Surface | Vtable slot → impl | Shape / notes |
|---|---|---|
| Panel getters Up/Down/Left/Right | +0x310/318/320/328 → `FUN_1800c9a30/9900/97d0/96a0` | `u64 impl(this, player, *state_u8, *trigger_u8, *sens_a_u64, *sens_b_u64)` — FIVE out-args (deploy #1 boot crash came from assuming 3). Players 4..=11 = debug-keyboard rows. Injection: OR level into `state`, latch-synthesized rising edge into `trigger`, plausible sensor blobs (4×u16=200) while held. |
| Menu getters Start/Up/Down/Left/Right | +0x2E0/2E8/2F0/2F8/300 → `FUN_1800c9b60/9cd0/9c90/9c50/9c10` | `(this, player, *LEVEL_u8, *EDGE_u8)`. Level bytes P1 `0x61A(S)/0x61B(L)/0x61C(R)/0x61D(U)/0x61E(D)`, P2 = +5; edge bytes P1 `0x60D..0x611`, P2 = +5. |
| 10-key pinpad | +0x308 → `FUN_1800c9420` | `(this, player, *buf1[12], *buf2[12])`, one-hot both buffers from `FUN_18007ecd0` (its ONLY caller) reading keycode global `DAT_180bd59ec + player*0x84` (0..0xB, else 0xC=none). Buffer index = key: 0..9 digits, 10 = "00", 11 = decimal (blank cap). |
| EAPass trigger/hold | +0x2D8 → `FUN_1800c9d10` | `(this, player, *out1, *out2)` — byte getter over `+0x60B/+0x624` (P1). **The export `arkMDXGetEAPass` is resolved by gamemdx but NEVER called** (its slot `DAT_1806f2270` has no readers) — do not inject there. |
| IO dispatcher | +0x28 → `FUN_1800d07d0` | Per-frame state machine; state 4 calls `MdxHWIO::stepUpdate` (`FUN_1800ce320`). Called only through the vtable — the post-original injection point for menu/card. |

### 1.3 The digest override words (menu injection, test-menu-visible)

- `FUN_18007e910(player, mask)` — the raw-digest LEVEL reader every level
  byte and the TEST MENU flow through. It ORs in a per-player **override
  word** `DAT_180c47f50[player]` (u32×2) — single reader, ZERO writers in
  the shipped binary: the ark's dormant dev-injection surface, adopted as
  the modpack's menu-button injection point.
- Digest mask bits: Start `0x01`, Left `0x02`, Right `0x04`, Up `0x08`,
  Down `0x10` (from stepUpdate's `FUN_18007e910(p, mask)` → level-byte
  copy pairs).
- AOB for the override base (the ark module's only AOB in this feature):
  `E8 ?? ?? ?? ?? 85 B4 BD ?? ?? ?? ?? 48 8B 5C 24 30` — the
  `TEST [RBP+RDI*4+disp32], ESI` inside `FUN_18007e910`; **disp32 at
  match+8 is MODULE-BASE-relative** (RBP holds the image base), not
  RIP-relative.
- `FUN_180084850` — the raw-digest EDGE reader (`~prev & cur`); it does
  NOT see the override word, so injected presses must synthesize the edge
  bytes (+0x60D..) separately (done post-dispatcher, one pulse per press).
- The dispatcher detour publishes the override words PRE-original (so the
  same frame's stepUpdate and any direct digest read see them; zeros when
  idle) and writes edge pulses + card state POST-original.

### 1.4 Card-in (the ark owns the whole flow)

`MdxHWIO::stepUpdate` runs the reader state machine; every consumer reads
the object's card fields. The ark's ENTRYFLOW scenes (`ArkEntryFlowScene*`,
factory `FUN_18000b400`) drive login; `MdxHWIO::getEAPassCardID`
(vtable +0xF0 → `FUN_1800cd460`) formats the stored UID for the network
login. Injection = replicate the physical-scan writes post-dispatcher:

- Card block, per player: base `+0x5BC` (P1) / `+0x5D4` (P2), stride 0x18:
  `{uid[8] @+0, type_bool @+8, presence @+9, type_int @+0xC,
  debounce_count @+0x14}`.
- Card trigger `+0x60B/+0x60C` (stock sets it only on a NEW uid), hold
  `+0x624/+0x625` (asserted while the card sits on the reader). Both are
  zeroed at stepUpdate's top each frame (u16 writes covering the pairs).
- Scan-enabled gates `+0x6F8/+0x6F9` — set by `MdxHWIO::setEAPassReadStart`
  (`FUN_1800cd9f0`, vtable-driven from the entry flow's card-wait screens).
  Injection only fires while armed; episodes (~120 dispatcher frames)
  always drain so a press on the wrong screen can't fire later.
- Card type rule (from the acio decoder `FUN_18007f250`): `uid[0] == 0xE0`
  ⇒ type 1 (ISO15693 e-amusement pass), else type 2 (FeliCa);
  `type_bool = type_int - 1`.
- Config card ids are the same 16-hex-char UIDs spice2x card files use;
  memory byte k = hex pair k.

### 1.5 Lights (capture + lamp sources)

- `arkMDXChangeTapeled(off1, off2, r, g, b)` (vtable +0x3F0 →
  `FUN_1800ca5d0`): writes `this + 0x153C + (off1*50 + off2)*12`
  (r/g/b u32 each, >0xFF = leave channel). Device space = spice2x
  `DDR_TAPELEDS[11]`: 0..=3 P1 foot up/right/left/down, 4..=7 P2 foot,
  8 top panel, 9/10 monitor left/right.
- `arkMDXChangeDimlamp(id, value)` (vtable +0x3D8 → `FUN_1800ca6e0`):
  writes `this + 0x14C8 + id*4` (u32, 0..255), ids 0..=28.
- **Dimlamp id map** (via the 29-triple staging table `0x1800f7a60`
  `{group, idx, dimlamp_id}` → the 21-pair slot map `0x180115c90` → BI2A
  LED indices `DAT_180117150` = `[8..23, 28..32]` → spice2x's GOLD LED
  names):
  - `player*8 + button`: P1 menu Start/Up/Down/Left/Right = **0..4**,
    P2 = **8..12** (drives the touch overlay's lamp-lit buttons).
  - 5..7 / 13..15 = P1/P2 card unit R/G/B; 16..18 = title panel L/C/R.
  - **19/20** = P1/P2 woofer corner (cabinet spotlights).
  - **21..24 / 25..28** = P1/P2 stage corners, order per side =
    `[UP_RIGHT, DOWN_LEFT, UP_LEFT, DOWN_RIGHT]` (mdxf a2 table).
- The game only emits Tapeled/Dimlamp on the GOLD light path: gamemdx's
  per-frame dispatcher `FUN_18000fcf0` takes the GOLD state machine
  (`FUN_180012720`) iff `arkMDXGetMachineType()==4 &&
  arkMDXGetPCType()∈{2,3,4}`, else the SD satellite machine
  (`FUN_18000e9f0` → `arkMDXChangeSatellite`, cabinet-light effects — NOT
  pad tape). On this cabinet the export view is overridden
  (`FUN_1800c9320` returns 1 when `MdxHWIO+0x5EE` is set) even though the
  ark's internal flush already runs the GOLD branch — `cabinet_force.rs`
  detours the two exports to report GOLD (4 / ≥2).
- The operator test menu's LAMP CHECK is ark-driven and bypasses the
  exports entirely: the transport polls the internal buffers
  (`+0x153C` tape / `+0x14C8` dimlamps) off the live singleton instead
  (`transport::poll_ark_light_buffers`).

## 2. gamemdx integration points

- **Layer dispatcher** `FUN_18002af10` (20260721; `FUN_18002b530` on
  20260616), called once per frame from the render orchestrator
  `FUN_180003020`. Iterates the 11-entry layer table (pointer global
  `DAT_1806f2d18`), stride 0x18 `{override_ptr, layer_object, list_index}`;
  a layer is walked iff `byte[layer+0x10]==0 && byte[layer+0x12]!=0`.
  Entries 7–10 are OVERRIDE entries whose `override_ptr` is the layer's
  private CommandList; **entry 7's layer object is the render-list manager
  the DLL's widgets register into** (`widget_renderer::render_list_manager`
  = `*(scene_mgr)+0xB0`). The touch overlay's TOPMOST emission appends to
  that list POST-original in the dispatcher detour (records drawn last =
  above the mod menu / loading art / all widget content; the append
  happens before the orchestrator's consumer kick — same-frame-safe).
  Full spike record: `docs/overlay_draw_research.md`.
- **Command-list records** used by the overlay (walker tag map authority:
  `docs/custom_arrow_renderer_research.md` §3): tag 0x04 textured quads
  (count × 0x34 `{x0,y0..x3,y3, u0,v0,u1,v1, color}` — UVs assigned from
  the rect min/max per corner), tag 0x08 blend (`{1, blendBits}`; the
  engine's standard-alpha prefix bits are `0x01220625`), plus the shared
  0x03/0x07/0x11/0x13 emitters in `overlay_draw::encode`.
- **Loose-PNG textures**: `asset_loader` (FileManager→ResourceManager);
  the engine's PngFileCallback registers the texture under the file's
  **bare basename stem** — the load stem MUST equal the PNG basename
  (deploy #19 failure mode: a mismatched stem polls forever).

## 3. SMX hardware (HID) — summary

Full wire facts in `services/smx/protocol.rs` docs and the feature
progress.md "Key facts" section. Highlights: VID 0x2341 / PID 0x8037,
product strings "StepManiaX" (stage) / "SMXArcade" (cabinet controller);
64-byte reports (id 3 input mask, id 5 host→device framed serial, id 6
device→host serial with HOST_CMD_FINISHED flow control); stage lights =
'4'/'2'/'3' panel-major commands ×0.6666 scale; dedicated cabinet lights =
`<'L'|'Q'> <device 0..4> <padded count> <colors>` per the `"I\n"`
version/model handshake. The marquee has exactly 12 physical LEDs at
payload slots 0..=11 (hardware-probed; SpiceManiaX had an off-by-one that
never lit slot 0).

## 4. Wine / CrossOver deployment facts

- **`HKLM\System\CurrentControlSet\Services\winebus\EnableHidraw =
  REG_MULTI_SZ "2341:8037"`** (+ wineserver restart) is a hard
  prerequisite: without it Wine's SDL backend claims the joystick-usage
  SMX devices and synthesizes a fake gamepad descriptor (report-id-5
  writes fail `ERROR_INVALID_PARAMETER`). Real Windows needs nothing.
- Wine's hidclass composes product strings with manufacturer fragments
  ("Revolution StepManiaX") — device matching must be substring, not
  exact.
- Wine's hidraw path interleaves in-flight overlapped writes: multi-packet
  HID commands must be serialized per packet (fresh OVERLAPPED +
  bounded `GetOverlappedResultEx` wait each).
- Touchscreen input arrives as MOUSE events under CrossOver (WM_TOUCH /
  WM_POINTER never fire); the WndProc handles all three paths.
- Clicking the window's close button can produce NO window message at all;
  the subclass owns the close (SC_CLOSE/WM_CLOSE → transport shutdown →
  1.5 s grace → `TerminateProcess`, spice2x's own force-shutdown endgame —
  a graceful CRT exit is exactly what wedges).

## 5. Touch overlay architecture

- Geometry model space is 1280×720 (`overlay_model.rs`, pure): SpiceManiaX
  button layout (menu diamonds = squares rotated 45°), per-cluster
  screen-corner anchors for the overlay-scale transform
  (`p' = anchor + (p − anchor)·s`, applied identically to rendering and
  the inverse-mapped hit-test), per-contact press tracking (release by
  contact id, never by lift position).
- Injection slot map (`input_manager::inject_slot`): 0..4 menu, 5..8
  panels, 9..20 pinpad (9 + 10-key buffer index). Pinpad presses are
  ~120 ms pulses (real pinpads are momentary — a held level reads as a
  stuck key in the test menu); menu presses are level+edge.
- Art: `scripts/gen_smx_overlay_atlas.py` → committed
  `data_mods/smx_hardware/smx_overlay_atlas.png` + generated UV table
  `overlay_atlas.rs`. Lamp-lit menu faces crossfade by dimlamp value with
  a bloom-halo quad (2× footprint) behind the face.
