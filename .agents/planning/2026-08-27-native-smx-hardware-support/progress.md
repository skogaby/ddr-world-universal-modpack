# Progress — Native SMX Hardware Support

Updated: 2026-08-28
Status: **FEATURE COMPLETE — all 4 steps done, cabinet-validated through
deploy #20** (uncommitted — maintainer commits manually).
Step 3 (touchscreen overlay) validated across deploys #16–#20; Step 4
(close-out) done 2026-08-28: lifecycle audit (+ card-episode cancel on
disable), docs (`docs/smx_hardware_research.md`, AGENTS.md row, README
"StepManiaX Cabinet Support" section incl. the Wine EnableHidraw prereq),
plan checklist closed. Known cosmetic gap (accepted): the test-menu "FOOT
PANEL CHECK" per-SENSOR screen reads sensors through a path we don't feed
(gameplay + I/O input unaffected). Skipped by design: `DDR_SMX_FAULT` env +
validation harness (every degradation path was hardware-exercised across
20 deploys; D12 waived host tests).
NEXT ACTION: none — maintainer commit when ready.

## Done

- PDD planning pass (rough-idea → design → plan), all Approved 2026-08-27.
- **Ghidra arg-decode confirmation (Step 1's first sub-task) — CLOSED 2026-08-27.**
  All on `arkmdxbio2_20260721.dll` (+ gamemdx_20260616 call sites), addresses
  file-relative to `0x180000000`:
  - `SingletonArkMDXIO::mdxIO` = `FUN_1800d2860`, singleton ptr @ `DAT_180c43658`.
    Concrete impl class `MdxHWIO` (ctor `FUN_1800cdd80`, vftable @ `0x1800F7C88`).
    gamemdx resolves every `arkMDX*` export via `GetProcAddress` at boot
    (`FUN_1800042d0`), exactly like our `input_manager`.
  - **`arkMDXChangeTapeled(off1, off2, r, g, b)`** (vtable +0x3f0 → `FUN_1800ca5d0`):
    writes RGB into `this + 0x153C + (off1*50 + off2)*12` (r@+0, g@+4, b@+8, u32 each,
    values 0..255, >0xFF = "leave channel"). Index space = spice2x's exactly:
    off1 0..3 = foot pairs (off2 < 25 → up/left half; ≥ 25 → right/down half),
    off1 5..7 = top_panel / monitor_left / monitor_right (off2 0..49).
    Device table (spice2x `DDR_TAPELEDS[11]`):
    `0 p1_up 1 p1_right 2 p1_left 3 p1_down 4 p2_up 5 p2_right 6 p2_left 7 p2_down
     8 top_panel 9 monitor_left 10 monitor_right`.
  - **STAGE CORNERS = `arkMDXChangeDimlamp(id, value)`** (vtable +0x3d8 →
    `FUN_1800ca6e0`, writes `this+0x14C8 + id*4`). gamemdx drives 29 dimlamps through
    a fade table (`FUN_180012ac0` @ gamemdx `DAT_1804c4ad0`). The ark flush
    (`MdxHWIO::stepUpdate` = `FUN_1800ce320`, machine-type-4/GOLD branch) stages all
    29 via table `1800f7a60` ({group,idx,id} triples) into `DAT_180c3dd10`
    (32 slots × 3B), then `FUN_180085bc0` maps staging slots
    `(side, idx 12..15)` → the 8 MDXF stage-corner outputs (`DAT_180c42e31`), i.e.
    `ac_io_mdxf_set_output_level(17/18, 0..3, v)`.
    **Dimlamp id → corner: P1 = 21,22,23,24 and P2 = 25,26,27,28, order per side =
    mdxf a2 0..3 = [UP_RIGHT, DOWN_LEFT, UP_LEFT, DOWN_RIGHT]** (spice2x `mdxf.cpp`
    mapping table). Values 0..255 at the arkMDX layer (flush halves to mdxf 0..128).
    Also useful: **woofer corners** ride the same dimlamp path — staging slots
    (0,8)/(0,9)… map through the 21-entry `DAT_180115c90` table to the BI2A LED
    index space where 31/32 = GOLD P1/P2 Woofer Corner (Step 2 concern; capture the
    whole 29-id dimlamp array now so Step 2 needs no new hook).
  - **`arkMDXGetPanelUp/Down/Left/Right(player, *trigger, *hold)`** — vtable
    +0x310/+0x318/+0x320/+0x328; identical `TriggerHoldFn` wrapper shape to
    `arkMDXGetStart` (+0x2e0). `arkMDXGetEAPass(p,*out,*out)` = +0x2d8 (Step 3).
  - `arkMDXChangeSatellite(dev, r, g, b, -1)` = whole-tape-device fill (11 devices,
    `FUN_1800ca8c0`); `arkMDXChangeSatelliteSeparate(dev, led, val)` = per-LED variant
    — **never called by gamemdx** (only the resolver writes its slot). Neither is the
    corner source; both write the same tape state the tapeled path writes. We do NOT
    need to hook them for Step 1 (tapeled alone carries the foot/top/monitor tape).
  - `arkMDXSetLamp(id, on)` = binary lamp array `this+0x1440` (+0x370 off / +0x378 on)
    — not needed for stage lights.

## In flight

- Step 3 code complete, readiness gates all green 2026-08-28
  (`cargo check --target x86_64-pc-windows-msvc` → `cargo fmt` → `./build.sh`).
  Awaiting cabinet deploy #16.

### Step 3 implementation summary (what was built)

- **Ghidra RE (all on `arkmdxbio2_20260721`, file-relative to 0x180000000;
  the injection design DEVIATES from the design doc's table where RE proved
  it wrong):**
  - **`arkMDXGetEAPass` card plan was WRONG.** gamemdx resolves the export
    (slot `DAT_1806f2270`) but NEVER calls it; its impl (vtable +0x2d8 →
    `FUN_1800c9d10`) is a per-player trigger/hold BYTE getter, not a UID
    reader. The whole card flow is ark-internal (ENTRYFLOW scenes) and every
    consumer reads the MdxHWIO object's card fields, written each frame by
    `MdxHWIO::stepUpdate` (`FUN_1800ce320`)'s reader state machine.
    `MdxHWIO::getEAPassCardID` (vtable +0xF0 → `FUN_1800cd460`) formats the
    stored UID for the entry flow's network login.
  - **Card field map (verified against stepUpdate's physical-card path +
    the acio decoder `FUN_18007f250`):** per-player card block base
    `+0x5BC` (P1) / `+0x5D4` (P2), stride 0x18:
    `{uid[8] @+0, type_bool @+8, presence @+9, type_int @+0xC,
    debounce_count @+0x14}`; card trigger `+0x60B/+0x60C` (set on a NEW
    uid), card hold `+0x624/+0x625` (held while the card sits on the
    reader) — both zeroed at stepUpdate's top each frame; scan-enabled
    gate `+0x6F8/+0x6F9` (set by `MdxHWIO::setEAPassReadStart` — the entry
    flow arms the reader on its card-wait screens). Card type rule (the
    decoder's own): `uid[0]==0xE0 ⇒ type 1` (ISO15693) else 2 (FeliCa).
  - **Menu getters** (vtable +0x2E0 Start / +0x2E8 Up / +0x2F0 Down /
    +0x2F8 Left / +0x300 Right) are 4-arg byte getters
    `(this, player, *trigger_u8, *hold_u8)` — NOT the panels' 6-arg shape —
    over plain object fields stepUpdate rewrites per frame from the raw
    digest: trigger P1 `0x61A/0x61B/0x61C/0x61D/0x61E` =
    Start/Left/Right/Up/Down (P2 = +5 each), hold P1 `0x60D..0x611`
    (P2 = +5). Internal ark consumers (entry-flow scenes) read the BYTES,
    so export-level injection is insufficient (deploy #4's lesson again).
  - **10-key impl** (vtable +0x308 → `FUN_1800c9420`,
    `(this, player, *buf1[12], *buf2[12])`, one-hot both buffers) is the
    single keypad funnel: its keycode source `FUN_18007ecd0`
    (`DAT_180bd59ec + player*0x84`, values 0..0xB, else 0xC=none) has NO
    other caller — one impl detour covers the export AND the ark's own
    PIN scenes. Buffer index = key: 0..9 digits, 10 = "00", 11 = decimal.
  - **IO dispatcher** = vtable +0x28 → `FUN_1800d07d0` (per-frame state
    machine; state 4 calls stepUpdate). Called ONLY through the vtable —
    the perfect post-original injection point right after the game's own
    field writes.
- **`src/services/input_manager.rs` extensions:** `inject_slot` grew
  `PINPAD_BASE..PINPAD_BASE+12` (COUNT 21). Two NEW detours, resolved from
  the live vtable inside the existing lazy `install_panel_impl_hooks`
  (aliasing check now spans all 6 targets; each overlay target degrades
  independently with one WARN): **(1) IO-dispatcher detour** (+0x28) —
  PRE-original it publishes the ark's per-player digest OVERRIDE WORDS
  (`DAT_180c47f50`, resolved by the ark module's first AOB — see the
  deploy #17 entry) with the provider-served MENU_* state (mask bits
  Start 0x01/Left 0x02/Right 0x04/Up 0x08/Down 0x10, zeros when idle), so
  the test menu's direct digest reads, stepUpdate's level-byte copies
  (+0x61A..) and the panel counters all see injected presses through the
  ark's own front door; POST-original it synthesizes one rising-edge
  pulse into the EDGE bytes (+0x60D..0x611 / +0x612..0x616 — the raw
  edge derivation never sees the override) and drives **card episodes**:
  `request_card_scan(player, uid8)` arms a ~120-frame episode; each
  dispatcher frame with the reader ARMED (`+0x6F8+p`) replicates
  stepUpdate's physical-card writes (uid block + type + presence +
  count + hold, trigger once per episode). Episodes always drain; a
  press on a non-card screen warns once and does nothing.
  **(2) 10-key impl detour** (+0x308) — ORs PINPAD_* one-hot into both
  buffers, EXCLUDED from the modpack's own poll (`IN_MODPACK_POLL`) so
  touch pinpad presses reach the game's PIN entry, not the mod-menu
  gesture machinery. The export-level `arkMDXGet10Key` suppression runs
  after the impl's injection, so an open mod menu still wins for
  game-side export callers. The export menu detours no longer inject
  anything (suppression only) — injection flows through the override
  words upstream.
- **`src/mods/smx_hardware/overlay_model.rs` (pure):** SpiceManiaX button
  set + layout (same 1280×720 coordinates incl. its int truncations:
  menu-up cx 100/1072 cy 575, L/R/Start row cy 618, down 662, Start
  offset +162; pinpad first key (35|1165, 85), 30 px keys / 10 px gaps;
  toggle (80|1200, 35); card (210|1070, 35)), rotated-rect corner math +
  inverse-rotation `contains`, `hit_test` (visibility-aware: hidden
  overlay = only the toggle responds — fixes SpiceManiaX pressing
  invisible buttons), `parse_card_id` (16 hex chars → 8 UID bytes).
- **`src/services/overlay_draw/` aux anchor:** a second, independent
  emission anchor (`set_aux_anchor(wrapper, dirty, emitter)` /
  `clear_aux_anchor`) with unconditional dirty re-arm, plus
  `emit_overlay_quads(&[Quad])` — gate ladder (default shader, active
  list, bump invariant, soft cap) → `set_context_2d` + stock program-0
  bind + one untextured-quad batch.
- **`src/mods/smx_hardware/overlay.rs`:** shared state (per-player HELD
  bitmask + VISIBLE + per-button press timestamps + alpha from
  `overlay_opacity`) and the native render: aux-anchor text widget FIRST,
  then label TextWidgets (labels z-above quads); per-frame emitter builds
  border+fill quad pairs per visible button (pressed = red border +
  warm-accent fill, ≥150 ms flash so sub-frame taps read; UI improvement
  over SpiceManiaX's flat polygons), reconciles label visibility on
  transitions (toggle label flips HIDE/SHOW OVERLAY — new). Lazy widget
  allocation from the mod's `on_frame` tick once `widget_renderer` is
  ready (pool-headroom check like mod_menu's chrome).
- **`src/mods/smx_hardware/touch.rs`:** paced game-window discovery
  (EnumWindows, same-PID largest visible client area, ≥320×240) →
  `SetWindowLongPtrW(GWLP_WNDPROC)` subclass + `RegisterTouchWindow`.
  Handles WM_TOUCH (1/100-screen-px → client), WM_POINTERDOWN/UP (screen
  px), WM_LBUTTONDOWN/UP (client px) — each with a one-shot
  `SmxTouch: delivery -- …` INFO so deploy #16 doubles as the CrossOver
  touch-delivery probe. Presses tracked per CONTACT (release releases the
  pressed button regardless of lift position — fixes SpiceManiaX's
  stuck-press on drag-off). WndProc panic-contained; consumes ONLY
  WM_TOUCH (exists solely because we registered); everything else
  forwarded via `CallWindowProcW`. Disable restores the original proc +
  unregisters touch.
- **`input_inject.rs`:** provider now serves MENU_* + PINPAD_* from the
  overlay bitmask (PANEL_* unchanged); `on_card_button(player)` fires
  `request_card_scan` with the parsed config UID.
- **`mod.rs`:** enable parses `p1card`/`p2card` (bad hex ⇒ WARN + that
  button absent), activates overlay (when `overlay_enabled`) + touch +
  injection, registers ONE `input_manager::on_frame` tick
  (overlay widget alloc + window-subclass pacing); disable reverses
  (widgets stay hidden — render-list nodes are permanently consumed).
- **Cargo.toml:** + `Win32_UI_WindowsAndMessaging`, `Win32_UI_Input_Touch`,
  `Win32_Graphics_Gdi`.

### Step 2 implementation summary (what was built)

- **Woofer-corner RE (CLOSED 2026-08-27).** The GOLD P1/P2 woofer corners are
  **`arkMDXChangeDimlamp` ids 19 (P1) and 20 (P2)** — already captured by the
  Step 1 dimlamp poll, zero new hooks. Chain (all `arkmdxbio2_20260721`,
  file-relative to `0x180000000`): the flush's machine-type-4 branch walks the
  29-triple table `0x1800f7a60` (`{group, idx, dimlamp_id}`) staging
  `(dimlamp[id]+1)/2` via `FUN_18007efa0(group, idx, …)`; triples `{0,11,19}` /
  `{1,11,20}` land in staging slots (0,11)/(1,11); `FUN_180085bc0` maps those
  through the 21-pair table `0x180115c90` to brightness slots 19/20
  (`DAT_180c42e1c`); the BI2A emitter `FUN_180091da0` sends slot i to LED index
  `DAT_180117150[i]` = `[8..23, 28..32]`, so slots 19/20 → **BI2A LEDs 31/32** =
  spice2x's `GOLD P1/P2 Woofer Corner`. Cross-check: the same triple table has
  corners `{0,12..15}→21..24` / `{1,12..15}→25..28`, matching the
  cabinet-validated Step 1 corner ids. Bonus finding: spice2x normalizes the two
  woofer lamps with `max=0` (`brightness/0 = inf` → clamped 1.0), so
  SpiceManiaX's spotlights were effectively BINARY; our raw 0..255 dimlamp read
  gives true proportional brightness (strict improvement, same visual language).
- **Dedicated-cabinet-lights wire protocol RE (CLOSED).** From
  `SMXManager::SetDedicatedCabinetLights` + `SMXDevice::CheckActive` /
  `HandleCabinetInfoResponse`: commands go to the **cabinet ("SMXArcade")
  device** as ordinary framed serial commands, shape
  `<'L'|'Q'> <device 0..4> <padded triplet count> <colors>` — **no trailing
  newline, no ×0.6666 scale** (stage-only). Device indices: 0 marquee, 1/3
  left/right strip, 2/4 left/right spotlights. Models 0–2: `'L'` everywhere,
  marquee 24 / strips 28 / spotlights 8 wire triplets; model 3: `'Q'` for
  marquee (20) and strips (23, physically reversed), `'L'` spotlights (6).
  Padded counts: 32 (marquee/strips) / 8 (spotlights), zero-filled. Channel
  order on the wire: marquee B,R,G; strips R,B,G (B,R,G on model 1); spotlights
  R,G,B. The model comes from the cabinet's `"I\n"` handshake (serial `'I'` +
  u16 LE version + u8 model when version ≥ 2, else model 0), sent after the
  standard flags-0x80 device-info response (the SDK requests device info on ALL
  devices at Open; CheckActive then sends `"I\n"` for cabinet kind). Flow
  control is the shared HOST_CMD_FINISHED mechanism Step 1 already implements.
- **`services/smx/cabinet_map.rs`** (new, pure): verbatim port of SpiceManiaX's
  three cabinet handlers fed by `&DdrLightFrame` —
  `map_marquee(&tape[8])` (40→12 via `MapValue(ddr_i,0,40,12,0)`, prefer-lit +
  channel-average on collision, 24-triplet payload with only mapped positions
  written; the map REVERSES onto SMX indices 12..=1 — intentional SpiceManiaX
  behavior), `map_strip(&tape[9]/[10])` (28 SMX LEDs each repeating
  `MapValue(smx_i,0,28,25,0)` — see Deviations for the 25-vs-26 note),
  `map_spotlights(dimlamps[19]/[20])` (8 white LEDs at the woofer brightness;
  P1→left spots, P2→right). `map_value` reproduces C++ trunc-toward-zero
  division (Rust i32 `/` matches).
- **`services/smx/protocol.rs`**: `CabinetLightDevice` enum,
  `cabinet_info_command()` (`"I\n"`), `parse_cabinet_info` (short packets
  zero-padded like the SDK's `resize(4,0)`), and `encode_cabinet_light(device,
  model, rgb)` — the byte-for-byte `SetDedicatedCabinetLights` port
  (cmd/wire/padded/reverse/channel-order per device+model, `wire =
  min(wire, data/3)`, zero-fill to the fixed size).
- **`services/smx/transport.rs`**: cabinet devices now get the device-info
  handshake at connect; its response (cabinet kind) queues `"I\n"`; the `'I'`
  serial response parses into `Device.cabinet_info` (+ INFO with
  version/model). `drain_lights` split by kind: Stage unchanged; Cabinet
  (gated on the new `OUTPUT_CABINET_LIGHTS` atomic AND the existing
  `OUTPUT_LIGHTS`, and on `cabinet_info` being resolved) stages a 5-command
  set per 30 Hz tick in SpiceManiaX's order (marquee, L strip, R strip, L
  spots, R spots) — same staged/active latest-wins model, one command per
  HOST_CMD_FINISHED, serialized per-packet writes. One-shot INFO on the first
  cabinet-light frame. `pump_writes`' timeout label now distinguishes
  device-info / lights / handshake.
- **Config**: `output_cabinet_lights` (default true) in
  `SmxHardwareConfig` (mods/config.rs) + `smx_hardware/config.rs` +
  `transport::init(output_lights, output_cabinet_lights)` +
  `set_output_cabinet_lights()` runtime setter; enable log line shows it.

### Step 1 implementation summary (what was built)

- **`src/services/smx/`** (new service; started by the mod's enable, NOT lib.rs init):
  - `protocol.rs` (pure): report parsing (id 3 input / id 6 serial), report-id-5
    command framing (START/END flags, ≤61 B/packet), device-info request/response,
    and `encode_stage_commands` — the '4'/'2'/'3' stage-lights commands with the
    SDK's exact panel-major order + ×0.6666 scale + '\n' terminators.
  - `input_map.rs` (pure): SMX 9-bit mask bits → `PanelDir` (Up=b1, Down=b7,
    Left=b3, Right=b5).
  - `light_map.rs` (pure): `DdrLightFrame` (11×50 tape RGB + 29 dimlamps) →
    2×`PadLights` — arrows 25:1 from foot tape, corner L-shapes from dimlamp ids
    21–28 over static gold `0xBB,0xBB,0x00`, center gold. SpiceManiaX flag grids
    verbatim.
  - `device.rs`: SetupAPI HID-class walk + VID 0x2341/PID 0x8037 +
    product-string filter ("StepManiaX"/"SMXArcade"), `FILE_FLAG_OVERLAPPED`
    open, `HidD_SetNumInputBuffers(512)`.
  - `transport.rs`: the `smx-transport` thread (ABOVE_NORMAL): 250 ms
    discovery/hot-plug, permanent overlapped read per device (input reports →
    per-pad `AtomicU16`), device-info handshake → player slot + firmware version,
    flow-controlled command queue (one in flight, gated on HOST_CMD_FINISHED;
    2 s timeout ⇒ fail-and-reopen), 33 ms lights drain (V4 queues '4'+'2'+'3';
    V3 skips '4' and spaces '2'/'3' 1/60 s), replace-unsent-lights semantics,
    latest-wins `DdrLightFrame` accumulator (`write_tape_led`/`write_dimlamp`/
    `fill_tape_device`).
- **`src/services/input_manager.rs` extension**: `ArkExports.get_panels`
  (Optional — a miss degrades injection only), four new panel-getter detours
  (forward + inject, no suppression), the additive injection seam
  (`inject_slot` indices ×9, `set_injection_provider` fn-pointer registry,
  `set_injection_active` default-off gate, per-(player,slot) rising-edge
  trigger synthesis), menu bodies now inject BEFORE suppression (overlay
  wins over injected input). `IN_MODPACK_POLL` contract preserved.
- **`src/mods/smx_hardware/`**: `mod.rs` (Mod impl, id `smx-hardware`, default
  OFF via `DEFAULT_OFF_MODS` in mod_trait.rs, `is_active` self-disable),
  `lights_read.rs` (Tapeled + Dimlamp detours [load-bearing] + Satellite
  whole-fill [best-effort]; capture gated on an atomic so disable = passthrough;
  detours never uninstalled), `input_inject.rs` (provider + activate/deactivate),
  `config.rs` (settings load + defaults).
- **Config**: `SmxHardwareConfig` in `mods/config.rs` (`p1card`, `p2card`,
  `overlay_opacity`, `overlay_enabled`, `output_lights`) + `smx_hardware` field
  in `ConfigFile` (+ both fallback literals). Card/overlay fields parse now,
  consumed in Step 3.
- **Registration**: constructed in lib.rs step-2c vec (registered step 7).
  Default-OFF mechanism: `DEFAULT_OFF_MODS` const consulted by
  `enable_with_config` (absent from `mods` map ⇒ OFF; explicit value wins).
- **Cargo.toml**: added windows features `Win32_Devices_DeviceAndDriverInstallation`,
  `Win32_Devices_HumanInterfaceDevice`, `Win32_Security` (CreateFileW's gate),
  `Win32_Storage_FileSystem`, `Win32_System_IO`.
- **Diagnostics shipped** (the cabinet-demo observables): INFO on device connect,
  "stage device ready — P{n}, firmware v{v}", first input per pad (with mask),
  first stage-light frame queued, transport start/stop, capture-detours installed;
  one-shot WARN: no devices found, device disconnected (reap), command timeout,
  missing exports (panel getters / light-out), thread-spawn failure.

## Deploy & test log

- **2026-08-28 Step 4 close-out — FEATURE COMPLETE.** Lifecycle audit:
  `disable()` fully reverses (rows removed, WndProc restored + touch
  unregistered, topmost emitter cleared, injection off, capture/force/
  poll-ark gated off, transport threads joined); found + fixed one gap —
  a mid-flight card episode froze at disable and would fire on re-enable,
  now cancelled via `input_manager::clear_card_scans()` from
  `input_inject::deactivate`. Docs: `docs/smx_hardware_research.md` (the
  consolidated RE record — ark IO maps, the three-consumer-layer input
  lesson, override words, card machine, lamp id decode, topmost append,
  atlas pipeline, Wine facts), AGENTS.md Key Entry Points row, README
  "StepManiaX Cabinet Support" operator section + the `smx_hardware`
  config-table row + the Wine `EnableHidraw` prereq. plan.md Steps 3+4
  ticked. Gates green.
- **2026-08-28 deploy #20 — VALIDATED: everything looks perfect
  (maintainer).** Layout (card between toggle and pinpad, column-aligned),
  the mod-menu SMX HARDWARE section (opacity / scale / pad lights /
  cabinet lights / pad style), corner-anchored live scale, decoupled
  light toggles, and Gold/Platinum pad style all confirmed on the
  cabinet. → feature close-out.
- **2026-08-28 deploy #20 (superseded by validation above) — layout polish + mod-menu SMX HARDWARE
  section + overlay scale + decoupled light toggles + Pad Style.** Deploy
  #19c feedback (aesthetic settled; card-in validated on both sides):
  - **Pad Style (Gold / Platinum)**: new `smx_hardware.pad_style` config
    key ("gold" default | "platinum"; unknown ⇒ WARN + gold) selecting the
    static accent for the un-driven pad regions — `light_map::PadStyle`
    (Gold = SpiceManiaX's 0xBB,0xBB,0x00; Platinum = cool silver/chrome
    0x8C,0x96,0xA8, tune on hardware) threaded through `map_stage`/
    `corner_panel`; live via `transport::set_pad_platinum` + a fifth
    mod-menu row ("Pad Style", GOLD/PLATINUM).
  - **Top-cluster layout** (`overlay_model.rs`): Insert-Card moved from
    beside the toggle to BELOW it (between toggle and pinpad); toggle,
    card, and pinpad all center on the pinpad's middle column
    (`COLUMN_CX` = 75 | 1205). Stack: toggle cy 35 → card cy 75 → pinpad
    rows 115/155/195/235 (pinpad position fixed whether or not a card
    button exists).
  - **Overlay scale, corner-anchored** (maintainer design): every button
    carries a cluster `anchor` — pinpad/utility stacks anchor at the TOP
    screen corners, menu-nav clusters at the BOTTOM corners — and both
    `corners()` (render) and `contains()` (hit-test, inverse-mapped)
    apply `p' = anchor + (p − anchor)·s`, so clusters grow toward screen
    center / shrink into their corners and touch targets always track
    the visuals. Range 50–150 %, default 100, live.
  - **Mod-menu section**: four contributed rows under
    `parent_row_key = "smx-hardware"` (the GLOBAL SETTINGS tab groups
    them under the mod's own header while it's enabled, hides them when
    disabled): Touch Overlay Opacity (10–100 %, fine 5 / coarse 25),
    Touch Overlay Scale (50–150 %), Pad Lights (ON/OFF), Cabinet Lights
    (ON/OFF). All live-applied; every change persists the WHOLE
    `smx_hardware` config section (`config::persist`, quick_restart
    pattern; cards/gold/overlay_enabled carried from the enable-time
    snapshot; opacity percent is the source of truth — the ALPHA-byte
    round-trip drifted 25→24).
  - **Light toggles decoupled** (`transport.rs`): `output_lights` was the
    master gate on the whole 30 Hz drain (cabinet lights required it);
    now `OUTPUT_LIGHTS` gates only the stage-pad staging and
    `OUTPUT_CABINET_LIGHTS` only the cabinet devices — two honest,
    independent toggles. Config docs updated; `overlay_scale` added to
    `SmxHardwareConfig` + the repo example config.
  Watch-items: (1) column alignment reads right at 100 %; (2) scale
  50 %/150 % — clusters stay glued to their corners, diamonds/glow scale
  proportionally, touch matches visuals at every scale; (3) the SMX
  HARDWARE section appears/disappears with the mod toggle; (4) Pad
  Lights OFF leaves cabinet lights running and vice versa; (5) row edits
  land in mod-config.json and load next boot; (6) card buttons still
  work at the new position.
- **2026-08-28 deploy #19b → #19c — textures VALIDATED; lit-face color
  iterations.** #19b confirmed: occlusion fix works, textures load and
  look right. Lit-face feedback loop: v1 warm gold = too yellow → v2 pure
  white = too subtle at overlay opacity during gameplay → v3 (#19c):
  slight warm hue (255,252,238 → 238,226,188) **plus a bloom halo** — a
  new `menu_glow` atlas cell (2× the button footprint, warm-white radial
  alpha falloff) drawn as an extra quad inflated by half the button size
  per side, underneath the face, alpha-crossfaded by the lamp value like
  the lit face. Reads clearly lit through the overlay opacity. Rebuild
  required (UV table gained MENU_GLOW); atlas re-copied to the bottle.
- **2026-08-28 deploy #19 → #19b — occlusion fix VALIDATED; textures didn't
  load (stem/basename mismatch — fixed).** Deploy #19 confirmed the topmost
  emission works (overlay renders above the mod menu) but the atlas never
  resolved (`textured=false`, no `atlas texture resolved` line, load issued
  and polled forever). Root cause: the engine's PngFileCallback registers a
  loose PNG under its **BARE FILENAME STEM**, and `asset_loader::resolve`
  hashes the caller's stem — the file was `overlay_atlas.png` but the code
  polled `smx_overlay_atlas`. Fix (#19b): the PNG is now
  `smx_overlay_atlas.png`; the generator derives BOTH constants from the
  filename so they can never diverge, and the overlay tick gained a
  self-diagnosing timeout WARN (600 unresolved polls ⇒ name the path +
  stem rule). Rebuilt + atlas re-copied to the bottle (the stale
  `overlay_atlas.png` removed).
- **2026-08-28 deploy #19 (what shipped) — textured topmost overlay + lamp-lit
  menu buttons (the mod-menu occlusion fix + presentation pass).**
  Deploy #18 feedback: the mod menu (widget-based, registered later ⇒
  higher z) occluded the touch overlay; maintainer requested pre-rendered
  Gold-cab-styled textures instead of flat quads + labels, plus the menu
  buttons lighting with the game's cabinet lamp output.
  - **Topmost emission** (`overlay_draw`): the layer-dispatcher detour
    (installed since the overlay-menu rewrite as a passthrough) now runs a
    registered TOPMOST EMITTER post-original — appends into the WIDGET
    layer's private CommandList (the layer-table override entry whose
    layer object == `widget_renderer::render_list_manager()`, walk-flag
    gated; table global deref confirmed against the dispatcher decompile)
    AFTER the dispatcher recorded everything ⇒ our records draw LAST ⇒
    above the mod menu, loading art, all game UI. Appends happen before
    the orchestrator's consumer kick (same call stack) — same-frame-safe.
    `with_topmost_writer(closure)` wraps the gate ladder + arena append;
    `topmost_ready()` = the dispatcher-hook availability.
  - **Textured buttons**: `scripts/gen_smx_overlay_atlas.py` (PIL + the
    repo font) generates `data_mods/smx_hardware/overlay_atlas.png` + the
    UV table `src/mods/smx_hardware/overlay_atlas.rs` — silver convex
    menu diffusers with near-black rounded bevels (drawn square; the 45°
    quad rotation makes the diamond, matching the real cab's rotated
    buttons), a warm LIT variant, Kokushin-style charcoal keycaps with
    baked legends (blank bottom-right), INSERT CARD / HIDE / SHOW
    utility buttons, and per-shape translucent-grey pressed overlays.
    Loaded via `asset_loader` (chrome_loader's loose-PNG pattern) from
    the overlay `tick()`; `encode.rs` gained `TexQuad` + `quads_textured`
    (tag 0x04, count × 0x34 `{corners, uv rect, color}`) and `blend`
    (tag 0x08; the emitter binds the engine's own standard-alpha bits +
    stock program 0 for deterministic state mid-append). 21 host tests.
  - **Lamp-lit menu buttons — zero new hooks**: menu-button lamps are
    dimlamps `player*8 + button` (P1 Start/Up/Down/Left/Right = 0..4,
    P2 = 8..12) — decoded from the ark's 29-triple staging table
    (0x1800f7a60: staging (0,i)/(1,i) for i 0..7) + the 21-pair slot map
    (0x180115c90: slots 0..15) + the BI2A LED table
    (`DAT_180117150` = [8..23, 28..32]) + spice2x's GOLD LED names
    (LEDs 8..12/16..20 = P1/P2 menu), cross-validated against the
    woofer ids 19/20 Step 2 proved on hardware. The emitter reads them
    off the live MdxHWIO object (`+0x14C8 + id*4`, resolved once per
    frame) and crossfades the LIT cell by lamp value — proportional
    brightness, like the spotlights.
  - **TextWidgets deleted** from the overlay (labels are baked into the
    art): no more widget-pool consumption, no aux anchor, no label
    centering guess. The `overlay_draw` aux-anchor API remains (unused —
    kept as service surface). Flat-quad fallback (no legends) when the
    atlas fails to resolve; one WARN when topmost is unavailable.
  - **DEPLOY NOTE:** needs the DLL **and**
    `data_mods/smx_hardware/overlay_atlas.png` (already copied into the
    local bottle's contents/data_mods — regenerate with
    `python3 scripts/gen_smx_overlay_atlas.py` after art edits).
  Watch-items: (1) overlay visible + usable ABOVE the open mod menu;
  (2) textured look (diamonds/keycaps/legends; rotated diamond art reads
  correctly); (3) menu lamps track the game (P1/P2 sides not swapped —
  if swapped, the dimlamp base ids flip); (4) pressed grey highlight on
  all shapes; (5) alpha blending clean (no opaque black boxes — would
  mean the blend/shader state records misbehave); (6) HIDE/SHOW cell
  swaps on toggle; (7) if `atlas texture resolved` never appears the
  overlay falls back to flat quads (check the PNG path); (8) no
  performance dip (≈90 textured quads + 10 lamp reads per frame);
  (9) if some game content STILL draws above the overlay, layer-table
  entries 8..10 compose above entry 7 — fallback plan: append to the
  LAST walked override entry instead (one-line change in
  `resolve_widget_layer_list`).
- **2026-08-28 deploy #18 — VALIDATED: touch pinpad gestures (mod menu
  0-0-0, quick restart/fail/logout), blank decimal key, and X-CLOSE FIXED
  (the game shuts down from the window close button).** New feedback →
  deploy #19: draw the touch overlay as the very top layer (the mod menu
  occluded it), and move to pre-rendered Gold-cab-style button textures +
  lamp-lit menu buttons.

- **2026-08-28 deploy #18 (what shipped) — touch-pinpad modpack gestures +
  blank decimal key + X-close ownership.** Deploy #17 validated the
  override-word menu nav (test menu ✓, in-game ✓) and the pinpad pulse
  (momentary in the test menu ✓). Remaining items:
  - **Touch pinpad didn't drive the modpack's gestures** (mod-menu 0-0-0,
    quick restart 1 / fail 3, logout 9-9-9): the 10-key impl detour
    deliberately excluded the modpack's own poll via `IN_MODPACK_POLL` —
    maintainer wants cabinet parity instead. Exclusion removed: injected
    pinpad pulses now reach the modpack poll AND the game alike.
  - **Decimal key label blanked** (Konami cabinet pinpads have a blank
    key there).
  - **X-close STILL hangs — and the deploy #17 log exonerated the HID
    reader threads**: the failing run had NO SMX devices attached, and
    the X-click produced NO log reaction at all (no WM_CLOSE at our
    subclassed proc — which sits FIRST in the chain — and no spice2x
    shutdown initiation; the visible teardown lines were the later
    ctrl-C). New approach: take OWNERSHIP of the close in the subclass —
    on `WM_SYSCOMMAND/SC_CLOSE` or `WM_CLOSE` (one-shot): log which
    message fired, stop the SMX transport, forward, and force-exit via
    `TerminateProcess` after 1.5 s if the game is still alive
    (deliberately NOT `process::exit` — CRT teardown is the thing that
    wedges; spice2x's own "force shutdown" ends the same way).
    `WM_DESTROY` still triggers a transport stop for teardowns that
    bypass close messages. **Decision tree for the log:** `close
    requested (msg=0x112)` = Mac close button arrives as SC_CLOSE ✓ fixed;
    `msg=0x10` = arrives as WM_CLOSE ✓ fixed; NO line at all = the close
    request never enters the window proc chain under CrossOver — our DLL
    cannot see it, move the investigation to spice2x (its window hook /
    `-windowed` handling) or accept ctrl-C as the close path.
  Watch-items: (1) touch 0-0-0 opens the mod menu; touch menu-nav then
  navigates it (exclusive consumer) while the game underneath stays
  suppressed; (2) quick restart / fail / logout gestures fire from touch;
  (3) decimal key blank; (4) X-close per the decision tree above; (5) no
  regression in PIN entry (the pulse now also feeds the modpack poll —
  harmless, it only consumes digits during its own UI flows).
- **2026-08-28 deploy #17 — test-menu menu nav + pinpad pulse VALIDATED;
  X-close still hangs (reader threads exonerated — see deploy #18).**
  Override words + edge-byte synthesis work on hardware: menu nav
  registers in the cabinet IO test menu and in-game; held pinpad keys
  read as one momentary press. New findings → #18: touch pinpad didn't
  drive modpack gestures (IN_MODPACK_POLL exclusion — intentional, but
  maintainer wants cabinet parity), decimal key should be blank, X-close
  produced zero log reaction (not even spice2x shutdown initiation).

- **2026-08-28 deploy #17 (what shipped) — test-menu menu nav (override
  words) + pinpad pulse + first X-close attempt.** Deploy #16 findings and
  their fixes:
  - **Menu nav worked in-game but NOT in the cabinet IO test menu** (the
    deploy-#4 lesson, one layer deeper): the test menu reads the raw
    digest LEVEL through `FUN_18007e910`, UPSTREAM of the object bytes the
    first implementation wrote. RE follow-up settled the whole level/edge
    architecture: `FUN_18007e910` (raw digest LEVEL) ORs a dormant
    per-player OVERRIDE WORD `DAT_180c47f50[player]` into every read
    (single reader, ZERO writers — the ark's own dev injection surface,
    first spotted in deploy #5); `FUN_180084850` derives the EDGE bytes
    (`~prev & cur`) from the RAW digest with NO override. stepUpdate
    copies the override'd level reads into `+0x61A..` and the raw edges
    into `+0x60D..` — so the first implementation ALSO had level/edge
    swapped (it navigated in-game by acting as auto-repeat on the edge
    byte). **Fix:** the dispatcher detour now (pre-original) publishes the
    override words (digest mask bits Start 0x01 / Left 0x02 / Right 0x04 /
    Up 0x08 / Down 0x10; zeros when idle) — covering the test menu, the
    level bytes, and the panel counters through the ark's own front door —
    and (post-original) synthesizes ONE rising-edge pulse into the edge
    bytes `+0x60D..0x611`/`+0x612..0x616` per press. The override base is
    the ark module's first AOB
    (`E8 ?? ?? ?? ?? 85 B4 BD ?? ?? ?? ?? 48 8B 5C 24 30`, disp32 at +8 is
    MODULE-BASE-relative — RBP holds the image base; exactly-one-match +
    bounds-validated, miss ⇒ WARN + menu injection off). The export menu
    detours' `inject_state_byte` path was REMOVED (it double-applied
    level-as-trigger on top of the dispatcher injection).
  - **A held touch pinpad key stayed "pressed" in the test menu.** Not
    event flooding — the injection was level-based, so the one-hot 10-key
    getter faithfully reported the key down while the finger was down;
    real pinpads are momentary. **Fix:** each touch converts to one fixed
    ~120 ms pulse (`overlay::pinpad_pulse_active` — press-edge timestamp,
    re-press requires lifting); the visual pressed state still tracks the
    finger.
  - **Clicking X on the game window didn't shut the game down** (predates
    Step 3 — present since the SMX mod landed). Prime suspect: the
    per-device reader threads blocked in overlapped hidraw reads wedge
    Wine's process teardown. **Fix:** the (now-owned) WndProc subclass
    catches WM_CLOSE/WM_DESTROY one-shot and runs `transport::shutdown()`
    (CancelIoEx + joins all threads, idempotent) before forwarding.
  Watch-items: (1) test-menu button check shows touch Up/Down/Left/Right/
  Start; (2) in-game nav unchanged (single steps per tap — no auto-repeat
  regression from the edge rework; holding should repeat only if the game
  itself repeats on level); (3) held pinpad key = one press in test menu;
  (4) X-close exits cleanly; (5)
  `InputManager: digest override words resolved` appears; (6) pads/lights/
  card-in/visibility unaffected.
- **2026-08-28 deploy #16 — OVERLAY VALIDATED (mouse delivery under
  CrossOver): quads + labels render in-game, all buttons work — menu nav
  navigates, pinpad enters digits (test menu shows keys), card-in and
  visibility toggle behave.** Touch delivery on this rig = MOUSE events
  (as predicted for Wine). Two findings → deploy #17 (above): menu nav
  invisible to the IO test menu; held touch pinpad keys read as held
  (should be momentary). Also raised: the long-standing X-close hang
  (since Step 1) — fix folded into #17.
- **2026-08-27 deploy #1 — BOOT CRASH (no SMX hardware attached, mod OFF).**
  EXCEPTION_ACCESS_VIOLATION ~2 s after `io_Start` (first game input poll),
  stack rooted spice64 → arkmdxbio2 thread-entry (+0x1F81/+0x2EEC of base
  0x1024e0000) → gamemdx ark-IO glue. Root cause: the research note's claim
  that the panel getters share the menu getters' 3-arg `TriggerHoldFn` shape
  was WRONG — `arkMDXGetPanelUp/Down/Left/Right` take FIVE args
  `(player, *state_u8, *prev_state_u8, *sensors_a_u64, *sensors_b_u64)`
  (wrapper prologue saves R9 + forwards a 5th stack arg; gamemdx's poll
  `FUN_180023830` passes 2 u8 out-locals + 2 sensor buffers; the MdxHWIO impl
  `FUN_1800c9a30` writes through ALL FOUR pointers unconditionally). Our
  3-arg detour trampoline forwarded garbage R9/stack-arg as the sensor
  out-pointers → wild write on the first poll. The detours install
  unconditionally at input_manager init — that's why boot crashed with the
  mod OFF and no hardware.
  **Fix (same day):** `PanelGetterFn` typed with the real 5-arg shape;
  injection rewritten as `inject_state_byte` — OR the held level into out1's
  LOW BYTE only (game-side callers pass u8 locals; the out pair is
  current/previous state and gamemdx derives edges downstream, so the
  rising-edge synthesis machinery was deleted as unnecessary). Menu-getter
  injection writes the same single byte. Research note corrected. Rebuilt
  clean — **awaiting deploy #2**.
- **2026-08-27 deploy #2 — crash fix VERIFIED; devices not found under
  Wine/CrossOver.** Clean boot with the mod ON: capture detours installed,
  transport started, graceful `SMX: no SMX devices found` WARN. With the
  pads attached: macOS saw all 3 devices (2× StepManiaX + SMXArcade,
  VID 0x2341/PID 0x8037), spice2x's rawinput scan saw them INSIDE the
  bottle, but our enumeration matched none. A standalone probe
  (`smx_probe.exe`, our exact device.rs code with verbose output, left in
  the game dir) proved SetupAPI enumeration + open + attributes all work
  under Wine — the failure was the product-string filter: **Wine's hidclass
  composes the product string with manufacturer fragments** ("Revolution
  StepManiaX" / "Step Re SMXArcade" instead of Windows' exact "StepManiaX" /
  "SMXArcade"). The SDK's exact match rejected everything.
  **Fix (same day):** substring match (`contains`) behind the VID/PID gate,
  plus a WARN naming any unrecognized product string. Rebuilt clean —
  **awaiting deploy #3** (expect: `SMX: stage device connected` ×2 +
  cabinet controller line at boot).
- **2026-08-27 deploy #3 — Wine HID backend diagnosed; bottle fix applied.**
  With the substring fix all 3 devices connected, but the stage devices
  fail-looped (connect → first write → reap): probe v2 showed Wine's SDL
  backend had claimed the joystick-usage SMX devices and synthesized a
  generic gamepad descriptor (`input_len=10/11, output_len=9` instead of
  the real 64/64) — the report-id-5 write fails ERROR_INVALID_PARAMETER
  against that fake descriptor. `Enable SDL=0` kills enumeration entirely
  (no fallback); the fix is winebus's per-device raw passthrough allowlist:
  **`HKLM\System\CurrentControlSet\Services\winebus\EnableHidraw =
  REG_MULTI_SZ "2341:8037"`** (+ wineserver restart). Probe then shows real
  64-byte descriptors, exact product strings, and a complete device-info
  round-trip: P1+P2 pads, firmware v5 (V4+ lights path). Note Wine reports
  63-of-64 bytes written (IOKit drops the report-id byte from the count) —
  harmless, the transport doesn't check write counts.
  **This registry key is a Wine/CrossOver install prerequisite** — document
  in the README operator section in Step 4 (real Windows needs nothing).
  `smx_probe.exe` (v2, handshake diagnostics) left in the game dir; source
  in the opencode temp dir (rebuildable from device.rs if lost).
- **2026-08-27 deploy #4 — LIGHTS WORK; input injection layer was wrong.**
  With the hidraw fix: both pads ready (P1/P2, fw v5), first light frame
  queued, pads showed the static-gold non-arrow panels — the whole
  transport + wire-encode + lights chain is cabinet-proven. Input masks
  ALSO proven (`first input from pad 0 (mask=0x002)` on an Up step). But
  the game (test menu) saw no inputs: injecting at the `arkMDXGetPanel*`
  EXPORTS misses most consumers — the ark layer's own update loop reads
  the panel getters through the IO singleton's VTABLE directly (panel
  counters, test/I-O state), and the export path (`FUN_180023830` in
  gamemdx, its only export consumer) also forwards per-sensor out-args we
  weren't filling. Ghidra: vtable slots +0x310/318/320/328 (Up/Down/Left/
  Right) → four distinct impls, shared shape
  `u64 impl(this, player, *state_u8, *trigger_u8, *sens_a_u64, *sens_b_u64)`
  reading the digested state (+0x6bX level, +0x62X press-edge trigger —
  the counter bookkeeping increments on the trigger byte; player indices
  4..=11 are debug-keyboard rows).
  **Fix:** moved injection to the four VTABLE impls — the single funnel
  every consumer (exports, ark counters, test menu) goes through. Detours
  resolved from the LIVE object's vtable (no AOB, build-independent),
  installed lazily from `poll()`'s first tick with the singleton live and
  a provider registered; bounds-checked against the ark module; alias
  check; injection ORs the held level into `state`, synthesizes a
  rising-edge `trigger` (per-(player,dir) latch, first-reader-wins), and
  fills zeroed sensor blobs (4×u16 = 200) while held so the I/O-check
  screen displays the press. Export-level panel detours REMOVED (the
  exports call the detoured impls). - **2026-08-27 deploy #5 — vtable injection installed cleanly; lights froze
  after the first frame; inputs still dead in test menu.** Log: impl detours
  installed at exactly the Ghidra-confirmed addresses, both pads' masks
  arriving. Two findings:
  **(a) Lights freeze root-caused (code bug, fixed):** the 33 ms drain
  REPLACED still-queued lights commands every tick. Under Wine a lights
  command takes > 11 ms to complete, so every set's tail ('3', often '2')
  was evicted before sending — the pads never received a complete
  '4'/'2'/'3' set after the first and froze on that frame (they only apply
  an update when the set completes; the SDK's "always finish a started
  update" invariant). Fix: per-device `lights_active` (a STARTED set always
  transmits fully, one command per HOST_CMD_FINISHED, V3 gap honored) +
  `lights_staged` (latest-wins replacement of the UN-started next set only).
  **(b) Input:** injection layer confirmed installed but the test menu
  still saw nothing — added two one-shot diagnostics to discriminate:
  `panel getter impl consulted (…)` (does ANYTHING call these impls?) and
  `first injected panel press (…)` (did injection fire?). Next run tells us
  whether to move to the deeper injection point: `FUN_18007e910` (the root
  BI2A digest reader) contains a built-in per-player OVERRIDE WORD
  (`DAT_180c47f50[player]` OR'd into every raw read) — the ark layer's own
  input-injection mechanism, likely what test/dev builds use, and the
  natural "be the IO layer" fallback (per-build AOB needed).
  Rebuilt clean — **awaiting deploy #6**.
- **2026-08-27 deploy #6 — INPUT WORKS (vtable injection validated);
  lights garbled by packet interleaving (fixed).** Maintainer confirmed:
  inputs correct in-game AND in the test menu's "input check" screen —
  the vtable-impl injection is the right layer (the `FUN_18007e910`
  override-word fallback stays documented but unneeded). Known gap: the
  "foot panel check" per-SENSOR screen still shows nothing — it reads
  sensors through yet another path (likely the PanelCounter interface or
  the raw MDXF ring); cosmetic diagnostic, deferred (noted for Step 4).
  Lights: static gold correct but arrow tape garbled/partial/wrong-colored
  and slow; lamp check inert. Root cause: `pump_writes` issued all 4–5 HID
  packets of a command back-to-back on one OVERLAPPED (the stock SDK's
  behavior — safe on Windows where the HID class driver orders write IRPs,
  but Wine's hidraw path interleaves in-flight writes → out-of-order
  61-byte chunks → scrambled LED data + broken START/END framing + slow
  master acks). Fix: serialized per-packet writes (fresh OVERLAPPED +
  `GetOverlappedResultEx` 500 ms bounded wait per packet; timeout ⇒
  fail-and-reopen). Correct on both platforms.   Rebuilt clean —
  **awaiting deploy #7** (expect: fluid arrow tape + corner L-shapes,
  working test-menu lamp/LED cycling).
- **2026-08-27 deploy #7 — lights still garbled; SATELLITE CONTAMINATION
  root-caused (fixed).** Serialized writes didn't change symptoms.
  Cross-validation per the maintainer's suggestion settled it:
  - Tapeled decode CONFIRMED CORRECT: gamemdx's tapeled callers
    (`FUN_180010780` @ 20260616) pass the packed `(group, led)` encoding —
    per-group LED-count table `DAT_18035a8b8 = [50,50,50,50, 0, 40, 26,
    26]` = exactly spice2x's off1 groups (foot pairs split at 25; top 40;
    monitors 26). Channel order r,g,b confirmed straight-through
    (`FUN_180084640` staging copy).
  - THE BUG: capturing `arkMDXChangeSatellite` as tape fills. Satellite is
    the SD-cab (P3IO `sate.cpp`) pod-light path; the GOLD flush IGNORES
    satellite state, but the game still calls it constantly (attract/test
    patterns, lamp-check whole-device white fills — `FUN_18000cea0` etc.).
    Our capture wrote those phantom fills into the tape frame over the
    legit tapeled data → garbled/partial/wrong-colored arrows; the real
    GOLD hardware never displays them (spice2x's GOLD tape source is
    exclusively `ac_io_bi2a_control_tapeled_bright`). Fix: satellite
    capture REMOVED (`fill_tape_device` deleted); capture = Tapeled +
    Dimlamp only, mirroring spice2x's GOLD surface.
  - Also from this run: inputs remain good; "foot panel check" per-sensor
    screen still a known cosmetic gap. Corner L-shapes reportedly never
    lit — dimlamp 21–28 corner mapping still unverified visually; watch
    for white L-pulses over the gold this run before suspecting the id
    table. Rebuilt clean — **awaiting deploy #8**.
- **2026-08-27 deploy #8 — satellite removal killed ALL lights: the game
  drives tape through SATELLITE, not Tapeled (capture rebuilt with mask
  semantics + traffic diagnostics).** With satellite gone, `first
  stage-light frame queued` never appeared — ZERO tapeled/dimlamp writes
  ever arrive in-game on this build; every light byte we ever saw came
  through `arkMDXChangeSatellite`. Re-read of the call shape:
  **`Satellite(device 0..10, r, g, b, led_mask_u64)`** — the 5th arg is a
  per-LED BITMASK (bit N = LED N; -1 = whole-device fill; gamemdx's
  effect drivers build masks per frame). The deploy-#7 garble was the old
  capture misreading the mask as an LED INDEX (mask ≥ 50 dropped, huge
  masks treated as fills, small masks as one wrong LED). Deploy #7's
  "spice2x only reads tapeled_bright" reasoning conflated layers: that's
  the LIBACIO surface the ark flush emits — at the arkMDX EXPORT layer the
  game uses satellite for tape. Fix: `fill_tape_device_masked` capture
  (per-channel ≥0x100 skip preserved) + first-6-calls INFO dumps on ALL
  FOUR light exports (satellite/tapeled/dimlamp/setlamp — `SMX diag:`
  lines) so the next cabinet log empirically maps the GOLD light traffic,
  incl. which path the corner lamps and the lamp-check screen really use.
  Tapeled+Dimlamp remain load-bearing installs (harmless if silent).
  Rebuilt clean — **awaiting deploy #9**.
- **2026-08-27 deploy #9 → root cause: cabinet MIS-DETECTED as non-GOLD; the
  satellite work was a wrong turn. GOLD-mode force implemented (awaiting #10).**
  Deploy #9 symptom (only P2-down arrow correct, every other arrow shows
  cabinet-light colors, no corners, test-menu light screens dead) drove a full
  decode of the light pipeline in Ghidra (gamemdx 20260721 + arkmdxbio2 20260721):
  - **gamemdx's per-frame light dispatcher `FUN_18000fcf0`** branches on a
    cabinet-family classifier `FUN_1800135e0(machineType, pcType)`. It takes the
    **GOLD light state machine `FUN_180012720`** (per-LED arrows via
    `arkMDXChangeTapeled` + corners via `arkMDXChangeDimlamp`) ONLY when
    **`arkMDXGetMachineType`==4 AND `arkMDXGetPCType`∈{2,3,4}**. Otherwise it
    falls to the **satellite state machine `FUN_18000e9f0`**
    (`arkMDXChangeSatellite`, cabinet-light effects) — all we ever captured.
  - **Satellite device space ≠ tape table.** `arkMDXChangeSatellite` writes
    per-device fill+mask blocks at `MdxHWIO + dev*0x100 + 0x940/0x950` (11
    devices) for the SD-cab (P3IO "sate", serial `0xAA 0x70`) model; the GOLD
    flush ignores them. Our `pad*4+dir → tape[]` map was pointing at
    cabinet-light devices (hence "cabinet colors on arrows"). `light_map.rs`
    was never wrong for GOLD — it was fed the wrong source.
  - **Why gamemdx is on satellite:** the ark's internal flush
    (`MdxHWIO::stepUpdate FUN_1800ce320`) uses the raw backend table
    (`DAT_180c47ef0`==2=BIO2, forced by `io_Start FUN_1800cdc60`) and ALREADY
    runs the machine-type-4 GOLD branch — reads `+0x1544` tape + `+0x14C8`
    dimlamp and emits `ac_io_bi2a_control_tapeled_bright` (exactly what
    SpiceManiaX consumed via SpiceAPI `ddr.tapeled_get`→`DDR_TAPELEDS` + mdxf
    `set_output_level` corners). But gamemdx's EXPORT view is overridden:
    `arkMDXGetMachineType` impl `FUN_1800c9320` returns **1** when `MdxHWIO+0x5ee`
    (the "force SD" flag, set from `arkMDXInitialize`'s param block) is set,
    and/or the backend getter downgrades 4→3 via `DAT_180c47f69`. The internal
    flush ignores the override; gamemdx doesn't → gamemdx drives satellite and
    never fills the tape/dimlamp buffers (in-game `SMX diag: tapeled/dimlamp`
    never fire, deploys #8–#9).
  - **`<io>bio2</io>` is NOT a fix — cabinet boot-death confirmed 2026-08-27.**
    spice2x only fakes the BIO2 USB probe (`FUN_1800d0dd0`, `PID_804C`/`8050`)
    when ea3 `spec` starts with `'I'`; DDR World's spec is `F`, so the probe
    fails and `dll_entry_init` raises a `specification.i` boot error
    (`DAT_180c4364c=0x44d`). Boot log confirms: `MDX:J:F:A`, `<io>p4io</io>`,
    p3io device-init fails → `io_Start` forces BIO2 backend → "acio(bio2) boot
    success" + "BI2A TapeLED init is finished" (backend IS type 4; gamemdx just
    can't see it).
  - **FIX (this session):** new `mods/smx_hardware/cabinet_force.rs` detours
    **`arkMDXGetMachineType`→4** and **`arkMDXGetPCType`→max(pc,2)** (export
    detours: forward-to-original then patch the out-param; gated by an atomic;
    installed once). gamemdx's classifier then picks the GOLD path → drives
    Tapeled + Dimlamp, which our existing capture + `light_map.rs` (arrows
    `tape[pad*4+dir]`, corners dimlamp 21–28) already decode. Aligns gamemdx
    with the ark's already-GOLD internal flush (a repair on a Gold/Universal
    cab, not a spoof). Satellite→tape capture suppressed while forcing
    (`cabinet_force::is_forcing()`) so boot-clear satellite fills can't wipe the
    tape frame. Config `smx_hardware.force_gold_cabinet` (default true; off-switch
    for genuine SD/HD cabs). Kept `<io>p4io</io>` (boots fine). Gates green:
    `cargo check` → `cargo fmt` → `./build.sh`. **Awaiting deploy #10.**
    Watch-item: `Application::onBoot FUN_1800020b0` reads machine type once for a
    refresh/window value (0x3c vs 0x4b); if boot render looks off, the force may
    install after onBoot — fallback = patch `MdxHWIO+0x5ee`→0 on the singleton,
    or install the force earlier. Lights re-query every frame so gameplay is
    unaffected regardless.
- **2026-08-27 deploy #10 — GOLD FORCE VALIDATED: gameplay lights fully correct.**
  Maintainer confirmed: during a song, pad arrows show per-arrow tape colors AND
  corner L-shapes light correctly — the whole GOLD `arkMDXChangeTapeled` +
  `arkMDXChangeDimlamp` capture → `light_map` → pad chain works. No boot-render
  regression observed (the `onBoot` 0x3c/0x4b watch-item was a non-issue). The
  satellite work (#7–#9) is fully retired as the wrong path. Remaining gap:
  operator test-menu **LAMP CHECK** still doesn't drive the pads (worked under
  SpiceManiaX). → deploy #11.
- **2026-08-27 deploy #11 — LAMP CHECK via ark buffer polling: VALIDATED.**
  RE of the test menu: `LAMP CHECK` is **ark-owned** — the string `lampCheck`
  lives only in `arkmdxbio2` (test-menu node id 8; the UI layout is
  `arkdata/ark/xml/testmodelayout*.xml`, lamp list incl. FOOT U/D/L/R, STAGE
  CORNER ×4, TOP/MONITOR/TITLE, WOOFER, CARD RGB, menu buttons). In the operator
  test menu **gamemdx's light dispatcher isn't running** — the ark drives the
  lamps itself, writing its **internal** light buffers directly, so it never
  calls the `arkMDX*` exports our detours capture (same class as the deploy-#4→#6
  input bug: internal callers use the vtable/buffers, not the export). SpiceManiaX
  caught it because it reads the **post-emission** `DDR_TAPELEDS` + Lights at the
  libacio layer over SpiceAPI.
  - **Fix:** poll the ark's internal GOLD output buffers directly, read-only,
    from the transport drain — the exact memory the export impls write and the
    machine-type-4 flush emits to BI2A every frame in ALL scenes (the source
    SpiceManiaX mirrored). Ghidra-confirmed layout (`arkmdxbio2_20260721`,
    relative to the `MdxHWIO` singleton, impls `FUN_1800ca5d0`/`FUN_1800ca6e0`
    at vtable +0x3f0/+0x3d8): **tape** `this+0x153C+(off1*50+off2)*0xC` = r(u32),
    +4 g, +8 b (off1 0..7, off2 0..49; off1→device via the spice2x foot-split
    map); **dimlamp** `this+0x14C8+id*4` = value(u32), id 0..28. New
    `input_manager::io_object_addr()` exposes the live singleton object
    (reused from the panel-injection landmark); `transport::poll_ark_light_buffers`
    reads both into a `DdrLightFrame`. Gated by `transport::set_poll_ark(true)`,
    set by the mod's enable **only when `force_gold_cabinet`** (in SD mode the
    buffers stay clear and the satellite→tape detour path is used instead). The
    drain now sources the frame via `acquire_light_frame()`: poll when forcing
    (latched on first lit LED so we never drive black at boot), else fall back to
    the export-detour-fed `DDR_FRAME`. Poll fully subsumes the tapeled/dimlamp
    detours in GOLD mode; the detours stay installed as the fallback + diagnostics.
    Read-only cross-thread reads of a stable heap object (input_manager reads the
    same pointer every render frame); torn RGB reads are cosmetically negligible
    at 30 Hz. Gates green: `cargo check` → `cargo fmt` → `./build.sh`.
    **Cabinet-VALIDATED 2026-08-27:** gameplay arrows/corners AND the operator
    test-menu LAMP CHECK all drive the pads correctly, no regressions.
- **2026-08-27 deploy #12 (pending) — input-latency split (dedicated reader
  thread).** Maintainer goal: preserve/maximize input timing (native HID over
  SpiceAPI is largely about input latency; target ≈1000 Hz-fresh input). Audit
  of the single-thread transport found input freshness was capped by two things
  on the shared worker thread: the `sleep(1ms)` (real granularity is OS-timer
  dependent — up to ~15 ms without `timeBeginPeriod`) and, worse, the **blocking
  serialized lights writes** (`GetOverlappedResultEx` up to 500 ms/packet; a full
  `'4'/'2'/'3'` set >11 ms under Wine) which starved input reads every 30 Hz
  lights frame.
  - **Fix:** split the transport into a dedicated **per-device reader thread**
    (`reader_thread`) doing event-driven blocking reads (`ReadFile` overlapped +
    `WaitForSingleObject(event, 200ms)` — wakes the instant a report lands, 200 ms
    only as a `stop`-poll backstop). Input reports (id 3) update `INPUT_MASKS`
    directly (≈0 latency, lock-free); serial reports (id 6) are forwarded to the
    worker over an `mpsc` channel. The worker keeps discovery + the device-info
    handshake + HOST_CMD_FINISHED-gated lights + the 30 Hz drain — it never touches
    the read path, so a blocking lights write can no longer stall input. Slot
    routing: worker publishes the assigned pad slot to the reader via a shared
    `AtomicI32` (-1 until the device-info handshake completes). Teardown order
    (reap/shutdown): set `stop` → `CancelIoEx(handle, None)` (unblocks the reader's
    wait) → join the reader → `close_device` → clear the mask; the reader owns its
    own OVERLAPPED + auto/manual-reset event and never closes the file handle.
    Read/write on one handle with distinct OVERLAPPEDs is concurrency-safe.
    Result: input freshness ≈ the pad's USB report interval (~1 ms) + an atomic
    store, fully decoupled from lights. Gates green: `cargo check` → `cargo fmt`
    → `./build.sh`. **Awaiting deploy #12.** Watch-items: (1) no input regression
    in-game or test menu (edges, holds, 2P slot routing); (2) hot-plug/unplug still
    reconnects cleanly (reader thread joins on reap); (3) lights unaffected.
    The game's own getter call-rate is the only remaining ceiling and isn't
    something the transport can change (DDR judges on timestamps, so freshness is
    what matters).
  - **Cabinet-VALIDATED 2026-08-27:** input timing confirmed responsive on
    hardware, no regressions in-game or in the test menu, lights unaffected. The
    reader-thread split is the shipping input path. **Step 1 complete.**
- **2026-08-27 deploy #13 (pending) — Step 2: cabinet lights (marquee / monitor
  strips / spotlights).** Zero new game-side hooks: the Step 1 frame already
  carries every source (tape 8/9/10 = top panel + monitors; dimlamps 19/20 =
  the woofer corners, Ghidra-traced this session through the flush staging
  table `0x1800f7a60` → brightness slots 19/20 → BI2A LEDs 31/32). New:
  `cabinet_map.rs` (verbatim SpiceManiaX ports), the
  `SetDedicatedCabinetLights` wire encoder + `"I\n"` version/model handshake in
  `protocol.rs`, and the transport's cabinet drain branch (5 commands per
  30 Hz tick, SpiceManiaX order, same staged/active + HOST_CMD_FINISHED flow
  control + serialized per-packet writes as the pads). Config knob
  `smx_hardware.output_cabinet_lights` (default true). Gates green.
  **Cabinet-VALIDATED 2026-08-27:** maintainer confirmed full SpiceManiaX
  parity — marquee, monitor strips, and spotlights all track DDR's cabinet
  lighting; stage lights/input unaffected. Closes the deviation watch-items:
  strip map constant 25 is correct as shipped, proportional spotlights look
  right, and the marquee's reversed 12..1 placement matches the hardware.
- **2026-08-27 deploy #14 (pending) — marquee resampler improvement
  (maintainer-requested after #13 parity).** SpiceManiaX's many→few marquee
  blend was rudimentary by its author's own assessment; with the verbatim
  baseline validated, `map_marquee` was replaced by a **prefer-lit,
  coverage-scaled box resampler**: in source-LED units (source `s` covers
  `[s, s+1)`, marquee LED `m` covers `[m·R, (m+1)·R)`, `R = 40/12`),
  `out[m] = Σ(w·rgb, lit sources) / max(Σ(w, lit sources), 1.0)`. Fixes vs
  the original: (1) order-independence — the old iterative pairwise average
  weighted a bin's last-arriving source at 50% and earlier ones exponentially
  less; (2) smooth sweeps — sub-LED lit coverage scales brightness linearly,
  so a sweeping pixel cross-fades between adjacent marquee LEDs instead of
  stepping every 3–4 source positions; (3) prefer-lit kept — dark sources
  contribute nothing, and ≥ 1 source-LED of lit coverage renders the
  full-brightness weighted mean (uniform fills and collisions behave exactly
  like before). Payload placement initially kept bit-identical to the
  validated baseline (indices 12..1) — then corrected to slots 11..=0 by the
  probe session below. Strips + spotlights untouched (still verbatim).
  Unused `average()` helper removed. Gates green.
- **2026-08-27 marquee address-space probe — RESULTS (hardware session, no DLL
  deploy).** Standalone `smx_marquee_probe.exe` (source `/tmp/smx_marquee_probe/`,
  exe also copied to the game contents dir; same discovery/handshake/framing as
  the transport) walked a single white triplet across all 32 marquee payload
  slots, then lit all 32. Maintainer-observed: **slots 0..=11 each drive a
  physical LED — slot 0 = RIGHT edge, slot 11 = LEFT edge; slots 12..=31 drive
  nothing.** Conclusions: (1) the marquee has exactly 12 physical LEDs — the
  24-LED resolution-upgrade hypothesis is CLOSED (the SDK's 24-triplet payload
  is just address space); (2) **SpiceManiaX had a genuine off-by-one**: its
  `MapValue(…, 12, 0)` wrote payload slots 1..=12, so the right-edge LED
  (slot 0) never lit and the DDR-start bin landed on the void slot 12 — masked
  for years because 11-of-12 LEDs lighting looks fine on a cabinet. Anomaly
  noted, not chased: Phase C (all 32 slots white, 10 s) appeared to light
  nothing — possibly firmware rejecting/limiting a payload with content past
  the wire-lights region (the SDK never sends nonzero past slot 23), a power
  limit at 12× full white, or simply missed; irrelevant to the game path,
  whose payload only ever has nonzero in slots 0..=11 (deploy #13 already
  validated that shape end-to-end). If attract-mode full-marquee fills look
  right on deploy #14, the anomaly is moot.
  **Fix folded into pending deploy #14:** `map_marquee` placement changed from
  bins 0..=11 → slots 12..=1 (SpiceManiaX parity) to bins 0..=11 → slots
  **11..=0** — same visual direction (DDR start → left edge), all 12 physical
  LEDs now live, no dead writes to slot 12. Gates re-run green.
  **Awaiting deploy #14.** Watch-items: (1) marquee sweeps glide instead of
  step; (2) ALL 12 marquee LEDs participate — the right-edge LED lights for
  the first time, content no longer shifted one LED left; (3) direction
  unchanged (DDR start → left edge); (4) full-marquee fills (attract) look
  right — also retires the probe's Phase C anomaly; (5) strips / spotlights /
  stage unchanged.
  **Cabinet-VALIDATED 2026-08-27:** maintainer confirmed everything looks
  perfect — smooth marquee sweeps, all 12 LEDs live, no regressions. The
  Phase C anomaly is retired (full fills render correctly through the real
  payload shape).
- **2026-08-27 deploy #15 (pending) — strip upsampler smoothing (maintainer-
  requested symmetry with the marquee).** `map_strip` rewritten from
  SpiceManiaX's nearest-neighbor `MapValue` repeat to **linear
  interpolation**: each SMX LED's center sits at its fractional position
  along the reversed source strip and blends its two neighboring DDR LEDs.
  Fixes: (1) uneven duplication banding — the truncating integer map gave
  some DDR LEDs two SMX LEDs and others one, in an irregular pattern;
  (2) sweep motion — cross-fades instead of irregular 1-or-2-LED jumps;
  (3) the skipped DDR LED 0 — SpiceManiaX's `25` map constant (vs the 26
  physical monitor LEDs, ark LED-count table `[…, 26, 26]`) meant DDR LED 0
  was never displayed; the continuous map spans all 26. Prefer-lit
  deliberately NOT applied (few→many: ≤ 2 sources per output — blending
  toward a dark neighbor IS the cross-fade). Direction preserved (SMX LED 0
  ↔ DDR strip end). Uniform fills reproduce exactly, so mostly-solid
  content is visually identical to #14. `map_value` + `DDR_STRIP_MAP_MAX`
  removed (last callers gone); `DDR_STRIP_LEDS = 26` added. Marquee /
  spotlights / wire layer untouched. Gates green. **Awaiting deploy #15.**
  Watch-items: (1) strip gradients smoother, no banding; (2) strip sweeps
  cross-fade; (3) the SMX-LED-27 end of each strip now shows DDR LED 0's
  content; (4) solid fills look identical to #14; (5) marquee / spotlights /
  stage unchanged.
  **Cabinet-VALIDATED 2026-08-27:** maintainer confirmed both the marquee
  and the monitor-side strips look great — no regressions. **Step 2
  complete:** all three cabinet-light devices (marquee / strips /
  spotlights) drive at their physical resolution ceiling with smooth
  resampling; stage lights + input from Step 1 unaffected throughout.

## Deviations & open questions

- **Step 3 deviates from the design doc's injection table (RE-driven,
  2026-08-28):** (1) `arkMDXGetEAPass` injection is DEAD — gamemdx never
  calls it; card-in instead writes the MdxHWIO object's card block from a
  post-original detour on the vtable +0x28 IO dispatcher, replicating the
  physical reader's writes (see the Step 3 summary for the field map).
  (2) Menu-button injection lives in the ark's per-player digest OVERRIDE
  WORDS (`DAT_180c47f50` — the ark's own dormant dev injection surface;
  written pre-original in the dispatcher detour) plus a synthesized
  rising-edge pulse into the object EDGE bytes post-original — the export
  detours' `inject_state_byte` path was removed outright (deploys #16→#17;
  internal ark consumers and the TEST MENU read the digest/bytes, not the
  exports). (3) `arkMDXGet10Key` injection sits at the vtable impl
  (+0x308), not the export, covering the ark's own PIN scenes; presses
  are momentary ~120 ms pulses, not levels (deploy #16 finding).
- **Touch menu presses reach the mod menu while it is open** (the
  object-byte injection is upstream of the modpack's poll): touch
  Up/Down/Left/Right/Start navigates the mod menu exactly like cabinet
  buttons, and the menu's suppression still shields the game underneath.
  Deliberate cabinet-button parity.
- **Touch pinpad presses feed the modpack's poll too** (deploy #18 —
  reversing the original IN_MODPACK_POLL exclusion at maintainer request):
  touch 0-0-0 opens the mod menu, and quick restart / fail / logout
  gestures fire from touch, exactly like the cabinet pinpad.
- **WM_TOUCH is consumed** (not forwarded) — it only exists because we
  RegisterTouchWindow'd; mouse/pointer messages are always forwarded to the
  original proc (spice2x's own hooks may want them; the game ignores mouse).
- **Pinpad "00"/decimal buffer indices (10/11) are unverified** — digits
  0..9 are cabinet-proven (mod-menu gesture); the last two are the natural
  reading of the one-hot impl. Trivial swap if deploy shows them reversed.
- **Label vertical centering is a first-deploy guess** (`cy − 20·scale`,
  BmpString anchors at glyph top; no text metrics queryable) — tune by eye
  like the mod menu's layout.
- **Marquee AND strips deviate from D6 (verbatim SpiceManiaX) — all
  maintainer-approved 2026-08-27, each landed only after a cabinet deploy
  validated what it replaced.** (1) `map_marquee` is the prefer-lit,
  coverage-scaled box resampler (deploy #14); (2) its placement is bins
  0..=11 → slots 11..=0, fixing SpiceManiaX's hardware-probe-confirmed
  off-by-one (it wrote slots 1..=12; slot 0 = the right-edge LED never lit,
  slot 12 is void); (3) `map_strip` is a linear-interpolation upsampler
  over all 26 monitor LEDs (deploy #15), replacing the nearest-neighbor
  repeat whose `25` constant never displayed DDR LED 0. The verbatim
  reference remains in `~/Desktop/Projects/SpiceManiaX/lights_utils.cpp`
  if a comparison is ever needed. Spotlights remain a verbatim port
  (modulo proportional brightness, below).
- **Strip map constant 25-vs-26 (Step 2) — SUPERSEDED by the deploy #15
  interpolation rewrite.** The Step 2 handoff said 26, SpiceManiaX shipped
  25 (skipping DDR LED 0); the verbatim port used 25 per D6 and deploy #13
  validated it. The interpolating `map_strip` made the question moot: the
  continuous map spans all 26 physical LEDs with no phantom index.
- **Spotlights are proportional, SpiceManiaX's were binary (Step 2) —
  validated by deploy #13.** spice2x normalizes the woofer-corner lamps with
  `max=0` → `inf` → clamp 1.0, so the SpiceAPI value SpiceManiaX read was
  0-or-1; our raw dimlamp read is 0..255. Deliberate improvement (true
  fades), confirmed looking right on hardware.
- **Corner-light source deviation from the design table:** design guessed
  `arkMDXSetLamp`/`ChangeSatellite`; RE proved corners ride `arkMDXChangeDimlamp`
  ids 21–28. lights_read.rs therefore detours **Tapeled + Dimlamp** (+ Satellite
  best-effort for whole-device fills; SetLamp left un-hooked — binary lamps are
  not stage lights). Dimlamp capture already covers Step 2's woofer corners.
- **P1/P2 corner-output order is inferred** (staging group 0 → mdxf node 17 = P1).
  If the demo shows corners lighting on the wrong pad, swap
  `CORNER_DIMLAMP_BASE` in `light_map.rs` (2-line fix).
- **Command-timeout recovery = fail-and-reopen** (not the SDK's in-place retry):
  simpler, avoids racing a canceled OVERLAPPED, same healthy end state.
- **Pad-slot conflict (two pads configured as the same player)** is not
  auto-swapped like the SDK's CorrectDeviceOrder; misconfigured pads collide on
  one slot. Fine for a correctly-configured cabinet; revisit in Step 4 if needed.
- Per D12: no host tests, no validate script. Pure modules kept isolated for reading.
- Task files: skipped the code-task-generator artifacts (agent judgment call — full
  context was already assembled in-session; plan.md Step 1 is the task spec). The
  progress file + plan checklist remain the resume points.

## Key facts for a cold resume

- **Step 3 ark IO injection map (arkmdxbio2_20260721, MdxHWIO vftable @
  0x1800F7C88, singleton ptr @ DAT_180c43658, all offsets object-relative):**
  IO dispatcher vtable +0x28 (`FUN_1800d07d0`, state 4 → stepUpdate
  `FUN_1800ce320`); menu getters +0x2E0/E8/F0/F8/300 =
  Start/Up/Down/Left/Right, 4-arg `(this, player, *level_u8, *edge_u8)`;
  10-key impl +0x308 (`FUN_1800c9420`, one-hot, sole reader of keycode
  source `FUN_18007ecd0`); menu LEVEL bytes P1
  0x61A(S)/0x61B(L)/0x61C(R)/0x61D(U)/0x61E(D) (P2 = +5) fed by
  stepUpdate from `FUN_18007e910` = raw digest OR the per-player
  OVERRIDE WORD `DAT_180c47f50[player]` (single reader, zero writers —
  our injection surface; digest mask bits S 0x01/L 0x02/R 0x04/U 0x08/
  D 0x10; AOB `E8 ?? ?? ?? ?? 85 B4 BD ?? ?? ?? ?? 48 8B 5C 24 30`,
  disp32 at +8 is MODULE-BASE-relative); menu EDGE bytes 0x60D..0x611
  (P2 = +5) fed from the RAW digest only (`FUN_180084850`, ~prev & cur —
  the override never reaches them; we synthesize pulses); card block
  +0x5BC/+0x5D4 `{uid[8], type_bool@8, presence@9, type_int@C,
  count@14}`, card trigger 0x60B/0x60C, hold 0x624/0x625, scan-armed
  gate 0x6F8/0x6F9 (armed by the entry flow's card-wait screens); card
  type: uid[0]==0xE0 ⇒ 1 else 2. `arkMDXGetEAPass` is resolved by
  gamemdx but never called — do not inject there.
- **Overlay layout (1280×720, SpiceManiaX-exact):** menu diamonds
  anchored at (100|1072, 575); pinpad 4×3 rows from (35|1165, 85);
  toggle (80|1200, 35); card (210|1070, 35). Shared state bits:
  0..4 menu, 5..16 pinpad (5+bufidx), 17 card, 18 toggle.
- **Menu-button LAMPS:** dimlamp id = `player*8 + button` (MenuButton
  order Start/Up/Down/Left/Right ⇒ P1 = 0..4, P2 = 8..12; woofers 19/20,
  stage corners 21..28 — all in the same `MdxHWIO+0x14C8 + id*4` u32
  array the LAMP CHECK poll reads). Chain: staging triples 0x1800f7a60 →
  slot map 0x180115c90 → BI2A LEDs `DAT_180117150` = [8..23, 28..32] →
  spice2x GOLD names (8..12/16..20 = P1/P2 menu).
- **Topmost overlay rendering:** post-dispatcher append to the widget
  layer's private CommandList (layer-table override entry matched by
  `render_list_manager()` identity) — records drawn last = above ALL
  widget content incl. the mod menu. Atlas: regenerate with
  `python3 scripts/gen_smx_overlay_atlas.py` (writes the PNG + the UV
  table `overlay_atlas.rs`); the PNG must ship to
  `contents/data_mods/smx_hardware/`.
- SMX HID: VID 0x2341 PID 0x8037; product string "StepManiaX" (stage) / "SMXArcade"
  (cabinet). 64-byte reports: id 3 = input (`mask = buf[2]<<8 | buf[1]`, 9 bits,
  bit1=Up bit3=Left bit4=Center bit5=Right bit7=Down), id 5 = host→device
  (flags @1: START 0x04 / END 0x01; len @2; ≤61 payload/packet), id 6 = device→host
  serial (flags: END 0x01 / HOST_CMD_FINISHED 0x02 / START 0x04 / DEVICE_INFO 0x80).
- Device-info handshake: report-id-5 packet with flags=0x80, len=0 → id-6 response
  with flag 0x80: payload = 'I', size, player ('0'/'1'), pad, serial[16], fw_version
  u16 LE. Stage lights: 3 serial commands per update, '4' = inner 3×3, '2' = top 4×2,
  '3' = bottom 4×2, each panel-major (9 panels), colors ×0.6666 scale, '\n'
  terminator. fw ≥ 4 ⇒ queue all three at once; fw < 4 ⇒ skip '4' and space '2'/'3'
  1/60s apart. Cap updates ~30 Hz (SDK uses min-interval 1/30s from send time).
  One command in flight per device; next command only after HOST_CMD_FINISHED.
- SMX panel LED order per panel: 16 outer (4×4) then 9 inner (3×3) = 25; SetLights2
  buffer = [pad0][pad1] × 9 panels reading-order × 25 × RGB = 1350 bytes.
- **Cabinet lights (Step 2):** the SAME HID device family (VID 0x2341/PID 0x8037)
  but the "SMXArcade" product string (`DeviceKind::Cabinet`). Handshake: standard
  flags-0x80 device-info first, then serial `"I\n"` → `'I' <version u16 LE>
  <model u8 if version ≥ 2>`; the model picks the wire protocol. Command shape
  `<'L'|'Q'> <device 0..4> <padded count> <colors>`, NO trailing newline, NO
  brightness scale; padded 32 (marquee/strips) / 8 (spotlights); channel order
  marquee BRG, strips RBG (model 1: BRG; model 3: 'Q' + reversed), spotlights
  RGB. Five commands per lights update: marquee, L/R strips, L/R spotlights.
- **DDR cabinet-light sources:** marquee ← `tape[8]` (top panel, 40 real LEDs);
  strips ← `tape[9]/[10]` (monitors, 26 real LEDs); spotlights ← dimlamps
  **19 (P1 woofer → left) / 20 (P2 woofer → right)** — all already in the
  Step 1 `DdrLightFrame`, no new hooks.
- **Marquee physical layout (hardware-probed 2026-08-27):** exactly 12 LEDs
  at payload slots 0..=11; slot 0 = RIGHT edge, slot 11 = LEFT edge; slots
  12..=31 drive nothing. `map_marquee` writes bins 0..=11 → slots 11..=0
  (DDR start → left edge). Probe tool: `/tmp/smx_marquee_probe/` (exe also
  in the game contents dir; rebuild with `cargo xwin build --release
  --target x86_64-pc-windows-msvc`).
- SpiceManiaX stage mapping (lights_utils.cpp): arrows = foot tape 25:1; corners =
  L-shape 4×4 flag grids lit by corner value over static gold (0xBB,0xBB,0x00),
  inner 3×3 static gold; center panel = static gold.
- SMX→DDR input map (input_utils.h): Up←bit1, Down←bit7, Left←bit3, Right←bit5.
- Repo rules bite here: no panics across FFI (catch_unwind in detours), transport
  thread ABOVE_NORMAL not HIGHEST, one detour per target (input_manager extension,
  not second detours), no `scripts/validate_smx.sh` (D12), never commit (maintainer).
