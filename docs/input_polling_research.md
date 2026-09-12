# Input Polling Research — MDXF Pad Path, Poll Cadence, Press Timestamps

Static analysis (Ghidra) of how DDR World actually samples the stage panels:
which module polls the MDXF pad board, how often, where the per-press
timestamp that `judgeNotes` consumes is generated, and what that means for the
community "the game only polls at 125/250 Hz" / "a faster pad IO board gives
better timing" claims. Written 2026-09-11 to answer those two questions and to
seed the diagnostics for a future higher-rate input-polling mod.

Builds analyzed (all addresses file-relative, module base `0x180000000`):

| Module | Build | Role in the chain |
|---|---|---|
| `libacio2.dll` | 20260825 | The ACIO stack `arkmdxbio2` actually loads (by name, `libacio2.dll`; the sibling `libacio.dll` is NOT used by the BIO2 ark). Owns the MDXF driver, the serial transport, all timestamps. |
| `libacio.dll` | 20260825 | Older stack, ships alongside; same MDXF driver at different device indices — see Cross-Version Notes. |
| `arkmdxbio2.dll` | 20260721 | I/O driver (Gold cabinet / BIO2): brings the ACIO buses up, converts the MDXF sample ring into per-panel press/release timestamps, exposes them through the `arkMDXGetPanel*` vtable impls. |
| `arkmdxp4.dll` | 20260825 | I/O driver (White cabinet / P4IO — the privately-run fleet). **Same pad path**: its backend's per-frame IO fn `FUN_18008ec10` (`"acio(foot) boot start"`) issues the identical `ac_io_begin(2, "1.31.16", …, 0x60000, 0, 115200)` / `ac_io_begin_get_status(0x60000, 0)` / per-frame `FUN_18008f130` (twin of `FUN_180091670`) + `ac_io_update(0)`, and calls the SAME `FUN_180091ae0` ring walk with the same press/release slot assignment. Only the cabinet-IO half differs (P4IO `DeviceIoControl` vs BIO2 ACIO bus 1). Imports the six `ac_io_*` from `libacio2.dll` by name at the same IAT slots as arkmdxbio2 (20250805 + 20260721 binaries checked). Everything in §3–§6 applies to both arks. |
| `gamemdx.dll` | 20260825 | Reads the ark once per frame, stores per-button timestamps, judges against them. |
| `libavs-win64.dll` | 20250805 | Clock (ordinal 45 `XCnbrep700002c`), threads, semaphores, events. |

Companion notes: `docs/audio_clock_research.md` (the `T`/`mc` timing contract
and the "judgement does not depend on the poll phase" derivation),
`docs/input_system_research.md` (export inventory — its panel-getter
signature is superseded by §4 below), `docs/smx_hardware_research.md`
(arkMDXIO vtable / MdxHWIO field map).

## 1. TL;DR

1. **The MDXF board neither timestamps nor buffers presses.** libacio2 polls it
   with ACIO command `0x10`; the reply is a 3-byte snapshot of the current
   sensor state (4 panels × 4 sensors = 16 bits, + 8 misc bits). The reply is
   timestamped **on the host, when libacio2 parses it**, with libavs ordinal 45
   — the same clock gamemdx uses for its frame tick `T`.
2. **The game side works the way the community describes:** gamemdx reads the
   ark once per frame, gets a per-panel *press timestamp* `P`, and `judgeNotes`
   places the step at `mc − (T − P)`. `T` cancels; judgement is placed at the
   poll-parse time, not at the 60 Hz frame. (Requires `HIGH_PRECISION_INPUT`,
   the shipped default.)
3. **The input cadence and the timestamp resolution are therefore set by
   libacio2's threads, not by the pad board.** Polling is lockstep (one request
   in flight per node), replies are read by a polled serial thread that wakes
   every ~2–3 ms, and both pads share one 115200 8N1 link. Static estimate:
   ~2–5 ms per sample per pad (roughly 200–500 Hz-class, jittery), timestamp
   quantization ≈ the serial thread's wake period (~2–3 ms).
4. **A replacement pad IO board cannot raise the game's input resolution under
   the stock Konami bootstrap** — the ceiling is libacio2 + the serial link. It
   can change fixed response latency (absorbed by JUDGEMENT OFFSET calibration)
   and sub-poll-period press latching (a real "dropped light taps" difference,
   but not a rate). **Launcher-dependent** (§7a): spice2x in *bootstrap mode*
   (DLL named explicitly — how stock-hardware cabinets run it, incl. with this
   modpack) leaves libacio2 live, so the above holds there too; spice2x in
   *emulated-IO mode* (auto-detected) stubs the whole ACIO layer, never polls
   a stock MDXF board, and stamps presses at HID-event arrival — there a
   1 kHz HID board genuinely sets the resolution, and the board swap is
   required, not optional.
5. The true cadence is **measurable in software**: every sample in libacio2's
   ring carries a request-send and a response-receive timestamp (§8).
6. **Field evidence so far (§7c):** one stock White-cabinet session confirms
   115200 8N1, two MDXF nodes on firmware **1.0.0 (Sep 2012)**, zero
   transport timeouts in 20 min, and ~14 unsolicited `0x10` status packets
   from the board. **The board's own firmware may be sitting on the cabinet's
   disk** as `contents\newfootio100.dat` (R8C/35A image; §9a) — a
   disassembly would answer the sensor-scan-rate question outright.

## 2. Bus bring-up (`arkmdxbio2!FUN_180090e40`, the ark's ACIO state machine)

Called once per ark IO frame from `FUN_180088900` (the per-frame ark update).
Two ACIO buses:

| Bus | `ac_io_begin(port, "1.31.16", &status, device_mask, bus, baud, flag)` | Nodes | Follow-up |
|---|---|---|---|
| 0 — pads | `ac_io_begin(2, …, 0x60000, 0, 0x1C200 /*115200*/, 0)` — `"acio(mdxf) boot start"` | device indices `0x11` (P1 pad) and `0x12` (P2 pad) | on `ac_io_is_active2(2)` success → `ac_io_begin_get_status(0x60000, 0)` (starts libacio2's auto-update thread for bus 0) |
| 1 — cabinet | `ac_io_begin(6, …, 0x4000000, 1, 0x1C200, 1)` — `"acio(bio2) boot start"` | `0x1A` (BI2A cabinet IO: buttons, coin, lamps) | `ac_io_bi2a_init(0xEC)` → tape-LED init → `ac_io_begin_get_status(0x4000000, 1)` |

The three locals the decompiler shows next to each call (`local_198/190/188`)
are the trailing `bus / baud / flag` args. `libacio2!ac_io_begin`
(`0x1800016b0`) forwards `baud` into `FUN_18000ab60(port, baud, 8 /*data bits*/,
0, 0, 0, 0x1000 /*serial-thread stack*/, 0, 3 /*serial-thread wait ms*/, bus,
flag)`; the COM port is opened as `\\.\COMn` in `FUN_18000e280`.

Steady state (`DAT_180c43310 == 2`): every ark frame calls
`FUN_180091670(&status, &has_new)` (§4) and then `ac_io_update(0)` — the game
thread kicks the bus once per frame in addition to the auto-update thread.

The libacio2 import descriptor is invisible to Ghidra's import parser (the
IAT sits at `0x1800d8930..0x1800d8a00`; `ac_io_update = 0x1800d89d0`,
`ac_io_begin_get_status = 0x1800d89e0`,
`ac_io_mdxf_update_control_status_buffer = 0x1800d89f0`,
`ac_io_mdxf_get_control_status_buffer = 0x1800d8a00`) — parse the PE import
table directly if Ghidra shows no `ac_io_*` externals.

## 3. libacio2 MDXF driver

All MDXF functions gate on `param_1 - 0x11 < 2` (nodes `0x11`/`0x12`);
`FUN_180002430(node, DAT_1800f9d24)` maps node → per-node slot 0/1. Per-node
state is `0x260` bytes at `DAT_1800f9840 + slot*0x260`.

### 3.1 Commands

| ACIO cmd | Sender | Payload | Reply handler | Meaning |
|---|---|---|---|---|
| `0x00` | `FUN_180022430` | — | `FUN_1800223f0` — logs `"initialized:%d"` | init / status |
| `0x16` | `FUN_180022750` (P1 node only) | 2 bytes `80 02` (`0x18004f15c`) | `FUN_180022700` — logs `"I/O auto get start"`, sets driver state 2 | "auto get start". Semantics of `80 02` unknown — see §9 |
| `0x10` | `FUN_1800228f0` | — | `FUN_1800227a0` (requires 3-byte reply) | **the poll**: 3-byte sensor snapshot |
| `0x13` | `FUN_180022980` | 10 bytes (2 × 5 output levels) | none | stage-corner LED output levels (`ac_io_mdxf_set_output_level`) |
| `0x41` | `FUN_1800224e0` | — | `FUN_180022470` | framing-error counter query, rate-limited by `ac_io_mdxf_set_framing_err_packet_send_interval` |

### 3.2 The poll reply handler — where every pad timestamp is born

`FUN_1800227a0(node_state, payload, len)`, `len == 3`:

```
lock(node+0x24c)
node+0x08  += 1                          // sample sequence counter
node+0x18   = XCnbrep700002c()           // libavs ord 45 — RECEIVE time (host)
memcpy(node+0x00, payload, 3)            // latest raw status
idx = (node+0x240 < 15) ? node+0x240+1 : 0   // 16-entry ring index
node+0x240 = idx
entry = node + 0x40 + idx*0x20:
   +0x00  3-byte status
   +0x08  sequence counter
   +0x10  = node+0x10 (REQUEST send time, ord 45, written by FUN_1800228f0)
   +0x18  = node+0x18 (RECEIVE time)
if (node+0x258 /*queued polls*/ < 6) FUN_1800228f0(node)   // keep the queue fed
else node+0x258 -= 1
unlock; node+0x241 += 1                  // "new data" tick
```

There is no timestamp, sequence number or event list in the 3-byte payload.
The board answers "what do I read right now"; the host stamps arrival.

`FUN_1800229c0(node)` (the per-update device tick) tops the node's request
queue up to **6 outstanding `0x10` polls** (`node+0x258`), and on the P1 node
flushes pending output-level changes (`0x13`).

### 3.3 Reader API (used by arkmdxbio2)

- `ac_io_mdxf_update_control_status_buffer(node)` (`0x1800222c0`): if the
  sequence counter changed since the last call, copies the latest sample into
  the "previous" slot (`+0x20`) and returns 1 — i.e. "new data since you last
  looked".
- `ac_io_mdxf_get_control_status_buffer(node, out, back, cur)` (`0x1800221b0`):
  `cur == 0xFF` ⇒ current write index; returns ring entry
  `(cur − back) mod 16` into `out`:
  `out+0x00` counter, `out+0x04..0x06` 3-byte status, `out+0x08` sequence,
  `out+0x10` request-send time, `out+0x18` receive time. Returns `cur` so the
  caller can walk `back = 0..15` newest→oldest against a fixed head.
- `ac_io_mdxf_control_reset`, `ac_io_mdxf_req_get_control_status` (forces a
  `0x41` framing-error query), `ac_io_mdxf_set_output_level(node, corner, 0..128)`.

## 4. arkmdxbio2 — ring → per-panel press/release timestamps

### 4.1 Per-frame status (`FUN_180091670(&status_bytes, &has_new)`)

For each node: `update_control_status_buffer`, then `get(…, back=0, 0xFF)` =
newest sample. The 3-byte status is decoded as **one nibble per panel**:
byte1 hi nibble → status bit `0x20`, byte1 lo → `0x40`, byte2 hi → `0x80`,
byte2 lo → the 4th panel (P1: `status[2] & 1`, P2: `status[2] & 0x10`). Each
nibble = 4 sensors; `DAT_180c43304` (1..4) selects one sensor for the I/O-check
screen's per-sensor view, 0 ⇒ OR of all four. The same nibble map (all-four OR)
is applied to older ring entries in §4.2. `has_new` is only used as a
"there is pad data this frame" gate.

### 4.2 Press/release derivation (`FUN_180090e40`, tail, + `FUN_180091ae0`)

```
FUN_180091ae0(status[16][0x10-stride], p1_ts[8], p2_ts[8], 16→clamped 7)
   for i in 0..7: entry = ring[cur − i]      // i=0 newest
      status[i] |= nibble-map(entry.status)   // 0x20/0x40/0x80/(0x01|0x10)
      p1_ts[i] = entry.receive_time; p2_ts[i] = …
   returns 7
```

then, per player, with `press[13]` / `release[13]` u64 arrays (globals:
P1 press `DAT_180c3edb8`, P2 press `DAT_180c3ee20`, P1 release
`DAT_180c3ee88`, P2 release `DAT_180c3eef0`; slots 0..8 = non-panel buttons,
slots 9..12 = the four panels):

```
press[0..12] = release[0..12] = ts[0]           // default: newest sample time
for i in 1..count:                              // older samples
   for panel in 9..12:
      if status[i] has panel bit: press[panel]   = ts[i]
      else:                       release[panel] = ts[i]
```

Because later iterations overwrite, `press[panel]` ends as the **receive time
of the oldest sample in the 7-sample window in which the panel read active**
— i.e. the first poll that saw the press (the press edge), provided the edge
lies inside the window (~7 polls ≈ 15–35 ms at the cadence in §6, which covers
a 16.7 ms frame). `release[panel]` symmetrically = oldest inactive sample.
(Corner case: a release + re-press inside one window makes `press` point at
the earlier press's tail — rare, ≤ one window.)

Accessors `0x180089350` (`press_time(player, idx)`) and `0x180089370`
(`release_time(player, idx)`) read those arrays (they are slots 18/19 of the
BIO2 backend function table `PTR_FUN_180145de8 + backend*0x1f*8`,
`DAT_180c47ef0 == 2`); the ark's per-frame update copies panel slots 9..12
into the `MdxHWIO` object, and the four panel-getter vtable impls return them:

```
u64 FUN_1800c9a30(this, player, u8* state, u8* trigger, u64* press_ts, u64* release_ts)   // Up, slot +0x310
   player 0: *state = this+0x6b0; *trigger = this+0x626; *press_ts = this+0x630; *release_ts = this+0x670
   player 1: *state = this+0x6b4; *trigger = this+0x62a; *press_ts = this+0x650; *release_ts = this+0x690
   player 4..11 (debug keyboard rows): state from DAT_180bd59xx, others 0
```

(Down/Left/Right = `+0x318/+0x320/+0x328`, same shape, next fields.)
**Correction to earlier notes:** the two u64 out-args are the press and
release timestamps (ord-45 domain), not "4×u16 sensor level blobs".

## 5. gamemdx consumption (20260825)

`FUN_180023440` (input aggregator, called from the main tick right before the
`0x102` actor broadcast — `docs/audio_clock_research.md` §6.1):

- per player, two `u64[0x1d]` arrays (press `local_118`, release
  `local_208`) are pre-filled with `Ordinal_45()` (now);
- the four panel getters are called as
  `getter(player, &state, &trigger, &press[btn], &release[btn])` for buttons
  5..8, overwriting the panel slots with the ark's ring-derived times;
- `FUN_1800230f0(player, button, held, press_ts, release_ts)` records each
  button into the `0x498`-stride sub-player block at `DAT_1806f2cf8`:
  press edge (`held-count == 0`) ⇒ `wasJustPressed |= bit`, record `+0x20` =
  `press_ts` — **unless** `HIGH_PRECISION_INPUT` (`state+0x1261`) is off, in
  which case `Ordinal_45()` (now) is stored instead; release edge symmetric
  with `release_ts` into `+0x30`; `+0x28` = "now" on every held frame;
- finally `FUN_1800231f0` stores `T = Ordinal_45()` at `state+0x1268`.

`judgeNotes` → `UserFootPanel::getPressTime` (vtable `+0x28`) →
`age = T − P` (low 32) → `event = mc − age = P − A − S + J`. Same clock on both
sides, so the judged instant is exactly the libacio2 receive stamp of the
first poll that saw the panel down (`docs/audio_clock_research.md` §7 has the
algebra and the "poll phase cancels" proof). Judgement is integer ms.

## 6. Transport threads and the resulting cadence

### 6.1 Threads (libacio2, per bus unless noted)

| Thread | Entry | Loop | Wake cadence |
|---|---|---|---|
| auto-update (one per bus, started by `ac_io_begin_get_status`) | `FUN_180002e20` | `FUN_180002480(bus)` = per-device ticks (MDXF: `FUN_1800229c0` tops the poll queue to 6) → `FUN_18000a9e0` → **signal the serial thread's event**; then `avs_thread_delay(2)` = `Sleep(2)` | ~2 ms (libavs calls `timeBeginPeriod(1)` at boot; expect 2–3 ms) |
| serial "MicomMain" (one per bus) | `FUN_180005490` | `FUN_1800033e0` → state `queue_loop` `FUN_180009360` → `FUN_180005a40`: **receive pass** `FUN_1800055d0` (parse one frame from the RX buffer, match to a pending slot, run its reply callback — this is where `0x1800227a0` stamps time) then **send pass** `FUN_180004ad0` (drain the send queue into ≤16 pending slots, `WriteFile`); then `event_wait(this+0x49a0, timeout = this+0x4968 = 3 ms)` | every ≤3 ms, or sooner when kicked (auto-update thread ~2 ms; game frame `ac_io_update`) |
| node access (one per ACIO node, `nodeacs.c`) | `FUN_18000a680` | run node state; idle `avs_thread_delay(this+0x1c54 = 1 ms)` unless a state transition is pending | signal-driven (see 6.2) |
| game thread | `FUN_180090e40` → `ac_io_update(0)` | kicks the bus 0 serial thread | once per frame |

COM port config (`FUN_18000e280`): `ReadIntervalTimeout = MAXDWORD`, all other
timeouts 0 ⇒ **non-blocking reads**; `SetCommMask(EV_RXCHAR)` is set but
`WaitCommEvent` is never used. Replies sit in the driver's RX buffer until the
serial thread next wakes.

### 6.2 Lockstep per node

`FUN_18000a1b0` (node wait state):
`sem_wait(node+0x1c4c)` (counting semaphore, max 0x16 = the node request-queue
depth; posted by `FUN_180009ae0` on every enqueue) → `sem_wait(node+0x1c50)`
(binary, initial 1; posted by the reply path `FUN_180009e10`) → state
`FUN_18000a120`, which submits the queue head to the serial layer
(`FUN_180009fe0`, records it as the single in-flight request at `node+0x360`)
and returns to the wait state. Semaphores created in `FUN_180009830`.

⇒ **Exactly one `0x10` poll is on the wire per pad at any time; the next one
is submitted only after the previous reply has been parsed.** The "6
outstanding" in §3.2 is queue depth, not wire pipelining — it guarantees the
node thread has a request ready the instant a reply lands.

### 6.3 Wire budget

ACIO frame (`FUN_180004c30` parser): `AA` sync + node + cmd(2) + seq + len +
payload + checksum = `7 + len` bytes, `FF`-escaped. Poll request `len 0` =
7 bytes, reply `len 3` = 10 bytes. At 115200 8N1 (86.8 µs/byte):
request 0.61 ms, reply 0.87 ms, minimum lockstep round trip ≈ 1.5 ms + board
firmware turnaround. Both pads share the RX channel: 20 bytes per pair of
replies ⇒ ≤ ~575 reply pairs/s. **Hard ceilings on this link, ignoring the
host entirely: ≈ 675 polls/s for a single pad, ≈ 575 polls/s per pad with two
pads.** A true 1 kHz per-pad poll is not reachable at 115200 with this frame
format.

### 6.4 Putting it together (estimate, not a measurement)

One poll cycle for a pad: reply bytes arrive → wait for the serial thread's
next wake (≤3 ms; typically the ~2 ms auto-update kick) → parse + stamp →
node thread wakes on the semaphore and enqueues the next request → it is
written either in the same serial pass (if the node thread won the race
against the send pass) or on the next wake (~2 ms later) → ~1.5 ms of wire
time → repeat.

- Sample period per pad ≈ 2–5 ms, **jittery**, dominated by `Sleep(2)` /
  3 ms event-timeout scheduling — ~200–500 Hz-class.
- Timestamp quantization ≈ the serial thread's wake period (~2–3 ms), because
  the stamp is taken at parse time, not at byte arrival. Several replies
  parsed in one wake (two nodes, or a late wake) share nearly identical stamps.
- Fixed latency (sensor → stamp) ≈ wire 0.9 ms + board turnaround + up to one
  wake period; constant part is absorbed by JUDGEMENT OFFSET, the jitter is
  what limits timing precision.
- USB-serial adapters (FTDI latency timer, default 16 ms) on COM2 would
  coarsen all of this further; a real UART would not. Cabinet-specific.

This is the same ballpark as the community's "125/250 Hz" figure, so whoever
measured that was probably measuring the real thing — but the cause is
host-side ACIO software cadence, not a pad-board limit.

## 7. Implications

- **"Faster pad IO board ⇒ higher input resolution":** not on stock DDR World.
  The board only answers polls; sampling instants and timestamps are decided
  by libacio2. What a board CAN legitimately change: (a) firmware turnaround
  (a constant, calibrated away); (b) whether a tap shorter than one poll
  period is latched until the next poll or missed — a genuine reliability
  difference for very light/short taps, unrelated to "polling rate";
  (c) sensor thresholds/debounce.
- **A "1000 Hz input polling" mod** would have to live in the host: libacio2
  threading (event-driven RX via overlapped `ReadFile`/`WaitCommEvent` or a
  spinning reader, stamp at byte arrival, submit the next poll from the reply
  path without a thread hop) and possibly the link (baud is a host parameter
  in `ac_io_begin`; whether MDXF firmware accepts anything but 115200 is
  unknown). Realistic targets on the stock link: ~500 Hz/pad sampling and
  ~0.1 ms stamp jitter — a large improvement over today's ~2–3 ms jitter, but
  not literally 1 kHz. See §9 for the `0x16` "auto get" lead.
- **spice2x is a different world:** it hooks `ac_io_mdxf_*` and fills the ring
  from its own IO thread with `arkGetTickTime64` stamps
  (`.agents/planning/2026-08-27-native-smx-hardware-support/research/spice2x-ddr-io.md`);
  none of §6 applies there. See §7a.

## 7a. Under spice2x (read from the spice2x source, 2026-09-11)

Everything in §2–§6 describes the stock Konami bootstrap. spice2x has TWO
modes for DDR, and which one you are in decides whether libacio2 runs at all:

**Emulated-IO mode (game auto-detected — the usual PC setup).** With no
`-exec`/positional DLL, `launcher.cpp` auto-detects `arkmdxbio2.dll` OR
`arkmdxp4.dll` → `attach_io = true` → `acio::attach()` + `DDRGame::attach()`:

- On 64-bit the ACIO hook mode is IAT (`acio.cpp`), and
  `detour::iat_try(name, stub, module = nullptr)` walks EVERY loaded module's
  import table and patches any import matching the function NAME
  (`detour.cpp::iat_find`, `iid_name == nullptr` — the providing DLL is not
  checked). Both arks import `ac_io_begin` / `ac_io_begin_get_status` /
  `ac_io_update` / `ac_io_mdxf_*` from `libacio2.dll` by name ⇒ all
  redirected. `-devicehookdisable` ("device passthrough") only clears
  `hooks::device::ENABLE`, i.e. the `CreateFile`/COM1/P4IO device hooks; the
  `ac_io_*` IAT patches stay — so on a P4IO cabinet that flag gives real
  cabinet IO while the MDXF pads are STILL never polled.
- Stubs: `ac_io_begin` → return 1 (no COM port, no libacio2 threads at all);
  `ac_io_begin_get_status` → 1; `ac_io_update` → rawinput output flush;
  `ac_io_mdxf_update/get_control_status_buffer` → spice2x's own ring
  (`acio/mdxf/mdxf.cpp`: 16 × 32-byte entries per player, state at `+4/+5`,
  timestamp at `+0x18` — matching libacio2's layout; `+0x08` sequence and
  `+0x10` request-time are never written, so they read 0).
- Ring fill: `mdxf_poll(true)` from `rawinput.cpp`'s WndProc on every HID event
  (stamp = `arkGetTickTime64()`, which on 64-bit falls back to `timeGetTime()`),
  plus padding — a 125 Hz thread (`THREAD_REFRESH_RATE_HZ`) when the game runs
  < 120 Hz, else 4 ms backfill (`BACKFILL_INTERVAL_MS`) at each per-frame
  `update` call. So in this mode the press stamp resolution IS the HID board's
  report rate (≈1 ms for a 1 kHz board, quantized by `timeGetTime`) — and a
  stock MDXF pad is dead, which is why emulated-IO users run HID pad boards.

**Bootstrap mode (DLL named explicitly — how stock-hardware cabinets run
spice2x).** `spice64 -modules modules arkmdxp4.dll -K ddr_world_hook.dll …`
sets `avs::game::DLL_NAME` at `launcher.cpp:1210`, so the auto-detect block
at `:1813` (`if (avs::game::DLL_NAME.empty())`) is SKIPPED — and every
`attach_io = true` / `attach_ddr = true` lives inside it. No `acio::attach()`,
no `DDRGame::attach()`, no device hooks: spice2x is a pure bootstrap
replacement, the REAL libacio2 opens the real COM2, the real P4IO/BIO2 is
used, and `-K` still loads this modpack. Everything in §3–§6 is the live
runtime of a stock-hardware White cabinet launched this way. (The
`-iohookdisable` seen in field `gamestart.bat`s is not an option in the
2026-07 source — unknown `-` args are dropped silently at
`options.cpp:3554` — the explicit DLL name is what does the work.)

Provenance of the community numbers: spice2x PR #446 (2025-12, "Decoupled
Input Polling Rate From Game Refresh Rate") states that arkmdx "expects the
ring buffers to be updated … inside libacio2.dll at ~250Hz per player" — an
independent RE of libacio2 that agrees with §6.4 — and 125 Hz is spice2x's
emulated-IO padding thread. Neither is a pad-board property.

Consequences:

- A "faster input polling" mod has ONE seam that works in both modes: the
  ark's `ac_io_mdxf_*` IAT slots. In bootstrap mode they point at libacio2
  (the mod may take over the MDXF transport — own COM2 handling with
  RX-event-driven stamping, back-to-back polls, any baud); in emulated-IO mode
  they already point at spice2x (slot ≠ libacio2 export ⇒ the mod stands
  down). Never fight spice2x for the slots.
- Local testability. Emulated-IO mode runs the real gamemdx + ark code
  (aggregator, recorder, judge, the ark's ring→press-time derivation, getter
  impls) but NOT libacio2 — good for the diagnostic plumbing, the judge-age
  cross-check, consumer-side changes, and a 1 kHz end-to-end simulation via a
  spice2x build with `THREAD_REFRESH_RATE_HZ`/`BACKFILL_INTERVAL_MS` at 1 ms.
  **The real transport is ALSO exercisable locally**: launch bootstrap-style
  with the positional `arkmdxp4.dll` plus `-ddr` but WITHOUT `-io`/`-acio` —
  `-ddr` (`LoadDDRModule`, `launcher.cpp:732`) attaches only `DDRGame`, whose
  `devicehook_add(new DDRP4IOHandle())` fakes the P4IO board so the ark boots
  on a PC, while ACIO stays unstubbed so the real libacio2 opens `\\.\COM2`;
  under CrossOver map `dosdevices/com2` → a pty and run an MDXF board
  emulator on the other end (ACIO enumeration + `0x10` → 3-byte status;
  bemanitools `src/main/acioemu/{emu,addr,pipe}.c` + `src/main/acio/mdxf.h`
  are the reference — MDXF POLL `0x0110`, AUTO_GET_START `0x0116`, LIGHT
  `0x0113`). That runs real libacio2 + real ark + real game + this modpack
  against a fake board; Wine's serial layer on a pty is the one thing needing
  a smoke test. Only firmware behaviour (baud acceptance, `0x16` streaming,
  turnaround, sensor scan rate) and final validation need a real board — and
  a stock-hardware cabinet in bootstrap mode CAN produce true stock-cadence
  numbers with the §8 diagnostic.

## 7b. Jitter budget — "Windows serial isn't realtime" vs. what libacio2 adds (2026-09-11)

The objection "Windows serial is designed for buffers, not this level of
timing" is correct about the OS and irrelevant as an argument against
replacing libacio2's polling: today's path uses that same serial stack AND
adds its own `Sleep`-grid waits on top. Two separable layers:

**The driver floor (cannot be removed; medium-dependent, currently UNKNOWN
for White cabinets — ask what Device Manager says COM2 is):**
- Real 16550 UART on `serial.sys`: interrupt-driven; a 10-byte reply is below
  the default 14-byte RX FIFO trigger, so it is delivered by the FIFO
  character-timeout interrupt (4 char-times ≈ 0.35 ms at 115200) → DPC →
  completion → scheduler wake. ≈ 0.4–0.6 ms after the last byte, ~0.1 ms
  spread, DPC-latency tail under load. `RxFIFO` registry value lowers it.
- USB-bridged serial (CDC-ACM / FTDI / possibly the P4IO board's own SCI
  ports tunneled over its USB link — bemanitools `p4io/cmd.h` documents
  `P4IO_CMD_SCI_MNG_OPEN/SCI_UPDATE/SCI_MNG_BREAK` but knows no game using
  them): floor = USB poll interval (1 ms FS frames / 125 µs HS microframes)
  + device buffering + driver timers. An FTDI default 16 ms latency timer
  would already show today as 16 ms-quantized ring stamps.
- Event wakes (overlapped `ReadFile` / `WaitCommEvent` completion) are NOT
  tied to the 1 ms timer resolution; `Sleep` and wait TIMEOUTS are. That is
  the precise point the "buffers" objection misses.

**What libacio2 adds above the floor (removable — §6):** non-blocking reads
polled on a `Sleep(2)`-kicked 3 ms event timeout with the stamp taken at
PARSE time (+0–3 ms uniform on the stamp), plus a node-thread semaphore hop
and the serial thread's next send pass before the following poll leaves
(+0–2 ms on the period), all quantized to the timer grid.

**Estimate (statics — the §8 diagnostic replaces them with measurements):**
press-time error = sampling delay (uniform over the poll period P) +
delivery delay; the mean is calibrated away by JUDGEMENT OFFSET, only the
spread matters.

| | Today | RX-event-driven, next poll from the reply path |
|---|---|---|
| P per pad | ~3–5 ms on the Sleep grid | ~1.6–2 ms, wire-bound (0.6 ms req + 0.9 ms reply + turnaround; two pads share RX) |
| host delivery + parse | wire + 0–3 ms uniform | wire + driver floor (~0.4 ± 0.1 ms UART; ~1 ms-quantized USB) |
| σ(press-time error) | ≈ √((3.5² + 3²)/12) ≈ 1.3 ms | ≈ √(1.8²/12 + 0.1²) ≈ 0.5 ms (UART) |

≈ 2.5× tighter spread, ~350–500 polls/s per pad, stamps off the 2–3 ms grid.
NOT 1 kHz (the 115200 bus cannot, §6.3), and both σ values sit well inside
even the tightest judgement window (the judge is integer ms) — the gain is
tail consistency, not a felt difference for most players. Decision test: if
the diagnostic's per-pad poll-period histogram clusters on 2/3/4/5 ms, the
software waits dominate and the rewrite pays; if it is already a tight
~1.6 ms, the bus is the ceiling. The pty rig (§7a) exercises the transport's
LOGIC only — a pty has no driver floor to measure.

## 7c. First stock-hardware trace (White cabinet, spice2x bootstrap mode, 2026-09-06 log)

A tester's `log.txt` from a stock White cabinet (`-exec arkmdxp4.dll -k
ddr_world_hook.dll -modules modules`, Windows 7 SP1 build 7601, AMD RX-421BD
embedded, ~20 min session). What it pins down:

- **Mode:** spice2x itself logs `WARNING - user specified -exec option …
  disables all game-specific hooks` — bootstrap mode (§7a), real libacio2.
- **Cabinet IO:** `libp4io 0.1.0`, driver `p4io.sys 1.05 … Aug 27 2012`, board
  `type 0B000000 ver 1.1.2 prod BMPU date Jun 12 2013`. One boot-time
  `W:libp4io: jamma not updated!, 2` (unrelated to pads).
- **Pad bus:** `acio(foot) boot start` → `MiCmd: NodeAcs Task Start` ×2 (one
  node thread per pad) → `BoudRate : 115200, ByteSize : 8, Parity : 0,
  StopBit : 0` (confirms §2/§6.3). `MiCmd_Sub: Serial Error: / CE_BREAK` once
  during `init_start` — the comm-error flag for a break condition, i.e. the
  ACIO bus reset; harmless. The COM number is only logged on failure
  (`COM Port %d Open Error`); `ac_io_begin(2, …)` ⇒ `\\.\COM2` (§2). Device
  Manager still needed to learn what COM2 physically is (§7b).
- **Enumeration:** `init` → `set_id` → `get_version`: node 1 and node 2 both
  report `type 0x09 0x07 0x00 0x00, flag 0, version 1.0.0, product_code MDXF,
  date Sep 28 2012 13:58:48` — **both pads run MDXF firmware 1.0.0 (2012)**.
  libacio2 then logs `No Need Firmware Update. Node N MDXF` (see §9a), then
  `enable_io` → `Node 2 Enable IO`, `Node 1 Enable IO` → `wait_node` →
  `queue_onece_init` → `queue_loop_start` → `acio(foot) boot success.`
- **MDXF driver:** `I/O initialize start. deviceIndex:18` → `I/O initialize
  request failed.` (expected: `FUN_1800229c0` only issues the `0x16` from the
  `0x11` node), `deviceIndex:17` → `initialized:0` ×2 → `I/O auto get start`.
- **Transport health:** ZERO `MiCmdRetry(Err:Timeout)`, ZERO watchdog lines in
  ~20 min — no request ever exceeded libacio2's ACIO timeout.
- **`Garbage Packet CMD:0x10` × 14:** two immediately after `I/O auto get
  start`, then twelve sporadic ones (roughly once per 1–3 min: 17:25:23,
  17:27:43, 17:29:59, 17:30:38, 17:30:45, 17:33:31, 17:33:47, 17:37:14,
  17:37:38, …), not correlated with scene loads. The unsolicited-packet
  handler (§3.1) fires only when a received `0x10` packet matches NO pending
  request (cmd + seq; a node mismatch is counted silently). With no timeouts
  or retries in the log, these cannot be late replies — **the board itself
  occasionally emits a `0x10` status packet without being asked**, i.e. the
  `0x16` "auto get" (§9) does enable SOME board-initiated status traffic; what
  triggers it (state change? keepalive? threshold recalibration?) needs a
  byte-level capture. At ~14 per 20 min it is not per-step streaming.

## 8. Measuring the real cadence (diagnostic design)

No hardware needed — the ring already holds both ends of every poll:

- Resolve `ac_io_mdxf_get_control_status_buffer` /
  `ac_io_mdxf_update_control_status_buffer` through **arkmdxbio2's IAT slots
  for `libacio2.dll`** (parse its import descriptor; §2 lists the 20260721
  slot addresses), NOT `GetProcAddress` on libacio2 — under spice2x the export
  still points at libacio2's never-filled ring while the IAT slot points at
  spice2x's live one; on a stock cabinet both agree. Off the game thread, once
  per ~5 ms, walk `back = 0..15`
  against a fixed `cur`, de-duplicate on the sequence counter (`out+0x08`),
  and log per node: Δ(receive time) between consecutive sequences (poll
  period), `receive − request` (round trip incl. host wake; N/A when the
  request stamp reads 0 — spice2x never writes it), and the count of
  distinct stamps per 16 samples (stamp clustering). The ring is written
  under `node+0x24c`'s mutex but read lock-free by the API — a torn read shows
  up as a non-monotonic sequence; drop it.
- Alternatively detour `0x1800227a0`-equivalent by content (it's not exported;
  AOB on the `cmp len,3` / ring-index `0x0F` shape) to stamp with QPC at parse
  time for sub-ms resolution — the ring stamps themselves are integer ms.
- Cross-check against gamemdx: the per-button record's `+0x20` press time vs
  `state+0x1268` (`T`) gives the age distribution the judge actually sees.

Bounded, off-thread, no game-state writes — safe as a first-pass diagnostic
inside a future input-polling mod.

## 9. Open questions

- **`0x16` "I/O auto get start" with payload `80 02`.** libacio v1
  (`libacio.dll`) installs an unsolicited-packet handler (`FUN_180020730`) that
  parses unsolicited status packets (dispatcher class 9, cmd byte `0x2F`, 3-byte
  payload) straight into the same ring, i.e. the MDXF firmware could *stream*
  status autonomously; libacio2's handler
  (`FUN_1800228b0`) only logs `"Garbage Packet CMD:0x%02x"`. The stock-cabinet
  log (§7c) shows the board DOES emit unsolicited `0x10` packets after `0x16`
  — but only ~14 in 20 minutes (two right after the enable, the rest
  sporadic), so it is not per-poll or per-step streaming with this firmware
  (1.0.0) and this payload. What `80 02` encodes (interval? threshold? mask?)
  and what triggers the sporadic packets needs a serial capture on COM2 or a
  hook logging the unsolicited packets' bytes. If the board can stream at a
  configurable rate, that is the cleanest path to a board-driven cadence —
  still bounded by the 115200 link (~575 status/s for two pads).
- Board firmware turnaround time and internal scan rate: not observable from
  the host; scope or firmware dump only. Irrelevant once ≥ the poll rate.
  **A firmware dump may be on disk — see §9a.**
- Whether the MDXF UART accepts a higher baud.
- Panel direction ↔ nibble mapping (Up/Down/Left/Right ↔ status bit
  `0x20/0x40/0x80/(0x01|0x10)`): inferred from getter/slot order, not verified
  on hardware.

## 9a. The MDXF firmware image libacio2 can flash (lead for the scan-rate question)

libacio2 carries an ACIO firmware-update path (`Source\micmd_firm.c`:
`EncodeFirmPacket`/`DecodeFirmPacket`, states `EraseFlashStart` →
`WriteFlashRAMStart` → `WriteFlashRAMReceiveFirst/Second` → `UpdateFirmwareEnd
Node %d`) and, per device driver, the expected firmware version + the file
names to flash. For MDXF, `FUN_180022cb0` registers the node with
`FUN_18000b2f0(FUN_180022ba0, FUN_180022130, "MDXF" @0x18004f158, type 1, 0,
0, "r8cdl-35a32k13.dat" @0x1800413d0, "newfootio100.dat" @0x180041aa0,
&DAT_1800f9d18, 0x1000, 1, 1, …)`. After `get_version`, `FUN_18000a2a0`
compares the board's type/major/minor against those registration values and
logs `No Need Firmware Update. Node %d %c%c%c%c` (the tester's boards, §7c) or
`Need Update Firmware!! Node %d` → `FUN_180004020(node, name1, name2, buf)` →
the update state machine, whose `WriteFlashRAMStart` (`FUN_180006720`) does
`avs_fs_open(node+0x4948 /* "newfootio100.dat" */, 1 /*read*/, 0644)` and
streams the file to the board in 0x40-byte chunks.

- **What the two files are:** `r8cdl-35a32k13.dat` = the Renesas **R8C/35A**
  serial flash-download loader ("r8cdl", 35A group, 32K flash) sent into the
  MCU's RAM first; `newfootio100.dat` = the MDXF ("new foot IO") application
  firmware **v1.00** — matching the `version 1.0.0` both pads reported. If it
  exists on a cabinet's disk it is the exact image running on those boards,
  and the sensor scan period is readable from its timer setup — the direct
  answer to "how often does the board sample" with no oscilloscope. Ghidra has
  no stock R8C/M16C processor module (IDA does; community SLEIGH specs exist
  and would need vetting). `.dat` is either Motorola S-record or a raw image.
- **Expected on-disk location — derived, not yet confirmed:** the open uses a
  bare relative name. libavs `avs_fs_open` (`XCnbrep700004e`) normalizes
  through `FUN_180055200`, which prepends the AVS cwd `DAT_1800fc400` unless
  the path starts with `/`; the cwd is initialized to `"/"`
  (`FUN_180056220`) and only `avs_fs_chdir` (`XCnbrep7000059`, ordinal 90)
  changes it — **no module in the install imports ordinal 90** (gamemdx,
  arkmdxp4, libacio2, ess, libafp, libavs-ea3 all checked). So
  `"newfootio100.dat"` → `/newfootio100.dat`. The tester's boot log has
  `fs/root/device : "."`, and the field `gamestart.bat` does `cd /d %~dp0`
  before `start spice64.exe`, so `.` = the directory holding
  `spice64.exe`/`bootstrap.exe`, `modules\`, `data\`, `prop\` (Konami's
  `contents\`). The LayeredFS verbose log shows exactly this mechanism live:
  the game opens `./prop/share-config.xml` and `/prop/ark-config.xml`
  interchangeably and both land in `contents\prop\`. Therefore:

  ```
  contents\newfootio100.dat      (MDXF application firmware v1.00)
  contents\r8cdl-35a32k13.dat    (R8C/35A flash-loader stub)
  ```

  NOT in `modules\` or `data\`. A local install built from `data\`/`modules\`/
  `prop\` alone (this repo's CrossOver bottle) does not have them; a factory
  image should, next to the other ACIO firmware files libacio2 names
  (`epistle-*.dat` for BI2A-family boards, `gpio10x.dat`, `ledio100.dat`,
  `ledbar102.dat`, `strctrl211.dat`, `hdxio100.dat`, `klpmainio100.dat`,
  `ps2io101.dat`, `cellicrw151.dat`, `fffio.dat`, `hbhio.dat`, `i36io.dat`,
  `r8cdl-2432k01.dat`). If not found there, a whole-drive search for
  `*footio*.dat` / `r8cdl*.dat` is cheap. Not all firmware files need be
  present: the update runs only when a board reports an older version, so
  Konami may ship them only with releases that carry a firmware bump — absence
  is possible even on a pristine image.

## 10. Cross-version notes

| Item | libacio.dll (v1) | libacio2.dll |
|---|---|---|
| MDXF device indices | `0x19` / `0x1A` | `0x11` / `0x12` |
| Per-node state | `DAT_1800a9330 + slot*0x250` | `DAT_1800f9840 + slot*0x260` |
| Poll reply handler | `FUN_180020630` | `FUN_1800227a0` |
| Unsolicited status | parsed (`FUN_180020730`, cmd `0x2F`) | logged as garbage |
| Auto-update thread | `FUN_180002ec0`, `Sleep(2)` | `FUN_180002e20`, `Sleep(2)` |
| Loaded by the 2026 arks | no | **yes** — `arkmdxbio2` 20260721 AND `arkmdxp4` 20250805/20260721/20260825 (`"libacio2.dll"` import descriptor, same six `ac_io_*` names) |

arkmdxbio2 20260721 rebases `has_new`'s stamp with `+ ord45 − ord44` while
libacio2 20260825 already stamps in ord 45 — a version-pair mismatch in the
analyzed set (the value is only used as a nonzero gate, so harmless). The
per-entry timestamps used for press/release are taken raw.

gamemdx 20260825 aggregator `FUN_180023440` / recorder `FUN_1800230f0` /
`T` store `FUN_1800231f0`; earlier builds: `FUN_180023830` (20260616),
`FUN_180023130` (20260421), `FUN_180022f80` (20250805) per the existing notes.

## 11. Gotchas

- **`input_manager.rs` SMX injection used to write `0x00C800C800C800C8` into
  the two u64 out-args when they read zero, believing they were sensor
  levels.** They are press/release timestamps: had that fill ever fired for a
  real press (it could not while the ark or spice2x supplied nonzero stamps,
  which is why the SMX cabinet validated fine), gamemdx would have recorded a
  press ~3.6 h off and the step would never judge. Fixed 2026-09-11: a zero
  stamp is backfilled with the current ord-45 time (fail-open if libavs
  ordinal 45 is unresolvable), nonzero left alone; the modpack's own
  `poll_player` also passed a `u8`/`u32` where the impl writes two `u64`s — a
  latent stack clobber, now `u64` locals.
- `docs/input_system_research.md` §"Foot Panel Arrow Exports" describes the
  panel getter as `(player, bool* trigger, bool* hold, bool* release,
  uint* counter)`; the real impl is
  `(this, player, u8* state, u8* trigger, u64* press_ts, u64* release_ts)`.
- `HIGH_PRECISION_INPUT` off collapses every press to the frame clock — any
  measurement of judgement resolution must confirm it is on
  (`docs/hex_edit_porting.md`).
- Ghidra does not list libacio2's imports for `arkmdxbio2.dll`; the IAT
  addresses in §2 came from parsing the PE import directory.
