# Progress — Native SMX Hardware Support

Updated: 2026-08-27
Status: Step 2 of 4 — **COMPLETE & cabinet-validated** (deploys #13–#15: parity
port → improved marquee resampler + slot fix → strip linear interpolation, all
confirmed on hardware; marquee/strips/spotlights at their physical resolution
ceiling). Step 1 also COMPLETE & cabinet-validated. Uncommitted (maintainer
commits manually).
NEXT ACTION: none required for Step 2. When ready, move to Step 3 (touchscreen
overlay: menu nav / pinpad / card-in — see `implementation/plan.md`). Known
cosmetic gap carried to Step 4: the test-menu "FOOT PANEL CHECK" per-SENSOR
screen reads sensors through a different path and shows nothing (gameplay + I/O
input are unaffected). Resume protocol: read the deploy #13–#15 + probe entries
below + `implementation/plan.md` Step 3.

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

- Nothing — Step 2 code complete, awaiting cabinet deploy #13. Readiness gates all
  green 2026-08-27: `cargo check --target x86_64-pc-windows-msvc` clean →
  `cargo fmt` (whole crate) → `./build.sh` clean
  (`target/x86_64-pc-windows-msvc/release/ddr_world_hook.dll`).

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
