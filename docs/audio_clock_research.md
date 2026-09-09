# Audio/Visual Clock Research — Stock Timing Jitter, the XACT 2.10 Mixer, and a Deterministic Audio Clock

Consolidated reverse-engineering record from the 2026-09 timing investigation
(`.agents/scratchpad/2026-09-08-frame-timing/`). It answers two field reports
from a ~1 ms-sensitive player on a stock Windows 7 cabinet: (a) the same song
feels earlier or later from play to play, with the *visual* and *aural* offsets
moving together; (b) timing drifts slightly within a song.

Status (2026-09-09): the fix this record supports ships as the
`gameplay-timing-fixes` mod (default OFF) and is live-validated on CrossOver —
§7.1 has the measured numbers; §8 lists what only the Win7 tester can answer.

Conventions: engine RVAs are relative to `xactengine2_10.dll` image base
`0x400000` (PE32+ **AMD64**, TimeDateStamp `0x471C7720`, SizeOfImage `0x69000`,
404120 bytes — the copy in `contents/com/`); game RVAs are relative to
`gamemdx.dll` base `0x180000000`, build **20260825** unless noted; libavs RVAs
are relative to `libavs-win64.dll` base `0x180000000` (20250805). Nothing in
shipped code hard-codes any of these — resolution is by AOB + attestation.

Symbols used throughout:

| Symbol | Meaning |
|---|---|
| `T` | the game's frame tick, libavs `XCnbrep700002c` (ordinal 45), cached at `input_state+0x1268` once per frame |
| `A` | GamePlayActor timing anchor `+0x160` (the value of `T` broadcast by the `0x1044` message at song start) |
| `S`, `J` | SOUND_OFFSET (`+0x16C`) and the per-player JUDGEMENT offset (option vcall `+0x248`) |
| `mc` | the authoritative music count computed once per frame in `GamePlayActor::onUpdate`: `mc = T − A − S + J` (`LEA R14D,[RAX+RBX]`, the `song_rate_clock_patch` site) |
| `W` | frames the mixer has WRITTEN into the DirectSound ring since device start (`DS+0xC0 / blockalign`) |
| `Wc` | accumulated DirectSound WRITE-cursor frames (`DS+0xC8 / blockalign`) |
| `P` | accumulated DirectSound PLAY-cursor frames (`DS+0xD0 / blockalign`) — the DAC position as the driver reports it |
| `F0` | value of `W` at the start of the mix pass that consumed sample 0 of a given source voice |
| `t_k` | QPC at mix pass `k` |

---

## 1. Summary of findings

1. **The stock game has ~10 ms of structural audio-onset jitter per play.** The
   engine mixes in fixed 10 ms / 441-frame passes on its own thread. A voice
   `Start` only *posts a command*; the song's first samples are mixed at the
   next pass, which is unsynchronised with the game thread that anchors the
   clock (`A`) ~0.1 ms after `Start` returns. Audible onset relative to `A`
   therefore varies uniformly over one pass period from play to play (§3).
   Because `mc` drives BOTH arrow placement and judgement, the visual and aural
   offsets move together — exactly the tester's observation.
2. **The exact onset is knowable.** Sample 0 of a voice lands at the first frame
   of the 441-frame block written by the first pass in which the voice node
   produces; `F0 = W` read inside that pass is exact and deterministic (§3.5).
3. **The in-song drift has an identified mechanism.** The game tick is RDTSC
   with a frequency libavs calibrates against QPC at boot (§4). Under CrossOver
   that calibration (`freq=999984252/s` for a 1 GHz emulated TSC) makes the
   game clock run **+15.75 ppm** fast — matching the +15.9…+16.0 ppm measured
   in the v2 capture. On a native cabinet the game tick is QPC-locked to a few
   ppm, so the remaining game-vs-audio drift is the sound device's crystal
   error (typically ±20…100 ppm ⇒ 2–10 ms over a 100 s song). It is only
   observable through the DirectSound play cursor.
4. **The DAC position is observable but coarse.** The engine reads
   `GetCurrentPosition` once per pass; under CrossOver the play cursor is a
   512-frame (11.6 ms) staircase (§5). Win7's dsound-on-WASAPI emulation is
   expected to be a 10 ms-class staircase as well (unmeasured).
5. **The visual pipeline is vblank-phase-locked; it adds no independent
   play-to-play jitter** (§6). Frame N's input poll (`T`) runs right after the
   GPU executor thread returned from `Present(N−2)`, so `T` is sampled at a
   fixed phase after vblank and each frame is scanned out a constant number of
   vblanks later. Judgement is independent of when `T` is sampled (the term
   cancels, §6.3). The only visual-side sources of variation are dropped frames
   and driver flip-queue depth changes, which are rare and not play-to-play.

Consequence: a clock that (i) anchors on `F0` and (ii) advances with the DAC
cursor removes both reported effects with one mechanism (§7).

---

## 2. Engine architecture: XACT layer → internal XAudio2-shaped core → DirectSound

The XACT engine wraps an internal audio core with an XAudio2-shaped object model
(source voices, format buses, master output, render thread). It is NOT the
public XAudio2 DLL — vtable slot numbering differs from any released header —
but the semantics map cleanly.

### 2.1 AudioSystem (vtable `0x405560`, 29 slots)

| Slot | RVA | Meaning |
|---:|---|---|
| 3 (`+0x18`) | `0x431EE0` | `Initialize(framesPerPass, rate, flags, callback)`; validates `framesPerPass*1000/rate ∈ [10,500]` ms; creates the format-bus table (`this+0xC8`), the command-queue owner (`this+0xD8`, `0x431C70`) and the render-thread object (`this+0xD0`, `0x438CE0`) |
| 7 (`+0x38`) | `0x4321E0` | `CreateOutputDevice(guidStr, flags, fmt, &out)`; backend type `this+0x490`: **1 = DirectSound** (`0x435EB0`/`0x4366B0`), 2 = other (`0x439F90`) |
| 9 (`+0x48`) | `0x42FCF0` | `CreateFormatBus(fmt, &bus)`; requires `(fmt.Hz*framesPerPass) % rate == 0` |
| 14 (`+0x70`) | `0x4306A0` | `CreateSourceVoice(fmt, ctx, effect, 0, 1, &bus, cb, &out)` → `0x434DA0`; returns the INTERFACE at `wrapper+0x30` (0x40-byte wrapper, vtable `0x4056E0`). **The source node is `*(iface+8)`** |
| 23 (`+0xB8`) | `0x431470` | `Start()` — starts registered outputs then the render thread (`0x438910`) |

The XACT engine calls `Initialize(10, 1000, 0, this)` at `0x41D440`: the render
period is expressed in **milliseconds — one pass = 10 ms**; a 44.1 kHz bus mixes
`44100*10/1000 = 441` frames per pass (bus `+0x208` = bytes per pass).

### 2.2 Render thread (object at `AudioSystem+0xD0`; run loop `0x438A80`)

`CreateThread` at priority **15 (TIME_CRITICAL)**, 64 KiB stack (`0x438760`).
Offsets below are relative to the thread sub-object `T = render+0x18`:

```
outer: FUN_00437680(cmdQueue)                      // apply posted voice commands
  inner do {
      callback->vt[0]()      // XACT pre-pass callback 0x41D150 = SetEvent(notify-thread wake)
      FUN_00437680(cmdQueue) // apply again
      for dev in T+0x220: dev->vt[0]()                 // pre-render
      lock(T+0x20)
      for dev: err    = dev->vt[1]()                   // RENDER ONE PASS (master node 0x43D350)
               waitMs = max(waitMs, dev->vt[5]())      // ms until next pass
      unlock
      T+0x240 += framesPerPass*1e7/rate               // += 100000 (100 ns units) = 10 ms per pass
      callback->vt[1]()      // no-op
  } while (waitMs == 0)
  timeBeginPeriod(1); WaitForMultipleObjects(2, T+0x10 /*wake, quit*/, FALSE, waitMs); timeEndPeriod(1)
```

The wake is a **computed sleep**, not a DirectSound position notification (the
buffer is created without `DSBCAPS_CTRLPOSITIONNOTIFY`; the module imports no
`timeSetEvent`/waitable timers). The XACT notify thread (`0x412AE0`) waits on
the event set by the pre-pass callback and runs `Engine_NotifyThreadPump`
(`0x411850`) — the "~10 ms packet" pump already described in
`docs/xact_audio_research.md`.

### 2.3 Master output pass — `0x43D350` (once per pass)

`N` = master mixer node (vtable `0x405D98`; `0x43CF50`/`0x43D0A0`).
`N+0x3B8` = DS backend secondary interface (`DS+8`); `N+0x3B0` = 441;
`N+0x39C` = **DesiredOffset, default 0x1E = 30 ms** (registry override
`HKCU\Software\Microsoft\Multimedia\LEAP\Timing\DesiredOffset`); `N+0x398` =
computed wait.

```
lead = DS->vt[9]()                    // 0x436090 → 0x435A50: frames queued ahead of the DS WRITE cursor
if lead == -1: return E
if DS->vt[8]() < lead + 441: return E_FULL       // 300 ms ring has no room — never in steady state
ptr = DS->vt[5](441)                  // GetWriteBuffer
bus->vt[3](1, &in, 1, &out)           // MIX 441 frames from every source node into ptr
DS->vt[6](441, silence)               // Commit: DS+0xC0 += 1764 bytes
lead_ms  = round(lead*1000/Hz)
N+0x398  = lead_ms > DesiredOffset ? lead_ms − DesiredOffset : 0
```

**Every pass writes exactly one block**; pacing comes only from the computed
wait. Steady state: lead-before-write ≈ 40 ms, one pass per 10 ms, lead
oscillating 40→50 ms ahead of the DS write cursor. Each pass instant is
re-derived from the DS write cursor, so `W` is DAC-locked in rate over the long
run, while each `t_k` carries the cursor's reporting error plus wake latency.

### 2.4 DirectSound backend object (`0x435EB0` ctor, `0x4366B0` init, 0xE0 bytes)

| Offset | Meaning |
|---|---|
| `+0x00` / `+0x08` | vtables `0x405890` (main) / `0x405840` (secondary — the mixer-facing interface) |
| `+0x20..0x48`, `+0x50..0x78` | two embedded position-source objects (vtable `0x4058C0`; member-fn thunks → `0x435B50` = accumulated WRITE-cursor bytes, `0x435B90` = accumulated PLAY-cursor bytes; `+0x48`/`+0x78` = Hz) |
| `+0x80` | output `WAVEFORMATEX*` (44100 Hz, 16-bit, channels from speaker config; Hz at `fmt+4`, blockalign at `fmt+0xC`) |
| `+0x88` / `+0x90` | `IDirectSound8*` / `IDirectSoundBuffer8*` (secondary buffer, `DSBCAPS_GLOBALFOCUS` only) |
| `+0x98` / `+0xA0` | locked ring pointer (locked once at init, pointer retained) / wrap staging buffer |
| `+0xA8` | ring bytes = `OutputBufferSize` ms (default **300**, registry) × Hz × blockalign |
| `+0xAC` / `+0xB0` | last raw play / write cursor (bytes) |
| `+0xB4` / `+0xB8` | pending bytes / ring write offset |
| **`+0xC0`** | **u64 accumulated bytes written (`W`)** |
| **`+0xC8`** | **u64 accumulated DS write-cursor bytes (`Wc`)** |
| **`+0xD0`** | **u64 accumulated DS play-cursor bytes (`P`)** |
| `+0xD8` | preroll silence frames (default 30 ms; registry `PrerollSilence`; ≤ half the ring) |

Secondary interface (`this+8`, vtable `0x405840`): 1/2 = position objects,
3 = Start (`0x435900`: preroll silence + `Play(LOOPING)`), 4 = Stop (`0x435990`,
zeros the accumulators), 5 = `GetWriteBuffer` (`0x436560`), 6 = `Commit`
(`0x4365B0`), 7 = format, 8 = ring frames, 9 = lead frames (`0x436090`).

**`0x435A50`** = `IDirectSoundBuffer::GetCurrentPosition(&play,&write)` +
wrap-accumulation into `+0xD0`/`+0xC8` + underrun clamp (`Wc > W` ⇒ `Wc := W`,
returns 0). It is the ONLY place the engine samples the DAC position, called
once per pass through slot 9. The v2 diagnostics already detour it passively.

### 2.5 Source voice node (0x658 bytes, vtable `0x405D20`; ctor `0x43C360`, init `0x43C710`)

Interface vtable `0x4056E0` (at `wrapper+0x30`; node = `*(iface+8)`):

| Slot | RVA | Meaning |
|---:|---|---|
| 5 (`+0x28`) | `0x433F30` | QI on the node's `+0x620` sub-object (the "secondary object" the XACT wrapper keeps at `V+0x30`) |
| 7 (`+0x38`) | `0x434FC0` | **Start** — with the render thread running it POSTS `{flag=(arg>>12)&1, op=1, wrapper, 0}` to the lock-free queue (`0x436DA0`→`0x4380B0`); applied at the next pass drain via `wrapper->vt[0]` → **`0x43B250`: `node+0x640 = 1`, `node+0x5F8 = 0`** |
| 8 (`+0x40`) | `0x434260` | Stop (posted) |
| 9 (`+0x48`) | `0x434540` | SubmitSourceBuffer |
| 10 (`+0x50`) | `0x434710` | op 4 → `0x43C040` FlushSourceBuffers |
| 11 (`+0x58`) | `0x434820` | op 5 → `0x43B270` ExitLoop-like |
| 12 (`+0x60`) | `0x434930` | op 6 → `0x43B290` Discontinuity |
| 13 (`+0x68`) | `0x434170` | GetState → `0x43B3B0`: `{ +0x560, +0x5F0 − +0x560, +0x5F8 }` |

Node fields (render-thread owned): `+0x18` decoder/effect chain (present when
`+0x618 != 0`; for format tag 2 = MS-ADPCM — every DDR bank — an ADPCM decoder
from `0x43E3C0` whose `vt[4](outFrames)` converts output frames → source bytes);
`+0x540/+0x548/+0x550` queued-buffer list; `+0x558` current buffer (NULL ⇒ the
pass emits SILENCE via `0x43B9E0`); `+0x560/+0x568` buffer data/len; `+0x5F0`
read pointer; **`+0x5F8` cumulative SOURCE BYTES consumed since Start** (reset
by `0x43B250`, `+= bytes` at the end of `0x43CAC0`); `+0x628` downstream bus;
`+0x640` playing flag.

Per-pass processing (vtable slot 1 = `0x43CE20`): if playing → advance buffers
(`0x43C150`) → if a buffer is current → **`0x43CAC0` produce**: pull exactly the
source bytes the decoder needs for 441 output frames, decode, submit to the bus,
`+0x5F8 += bytes`. No buffer ⇒ silence descriptor, `+0x5F8` untouched.

### 2.6 The XACT-side wrapper (previous session's findings, confirmed)

Voice wrapper `V` (0x98 bytes, vtable `0x3CF0`; `0x1F9A0`/`0x1F190`): `V+0x20` =
AudioSystem, `V+0x28` = source-voice interface (created via AudioSystem slot
14), `V+0x30` = the QI'd sub-object, `V+0x68` = format, `V+0x80` = play/stop
STATE (not a counter), `V+0x88` = queued-buffer count. Wrapper `Start`
(`0x1E1F0`) forwards to iface slot 7. Reciprocal ownership chain
`cue → sound → track → event → wave W → V` is implemented in
`src/services/audio_sync_diag/xact*.rs`.

---

## 3. Stock onset timing, derived

### 3.1 Sequence

```
game thread:   Sound_Play → … → streaming submission 0x25ED0 → V->Start → iface Start POSTS cmd → returns
game thread:   DPS reads T, broadcasts 0x1044 → GamePlayActor+0x160 = A            (≈ +0.1 ms; v2 capture)
render thread: next pass k0 (t_k0 ∈ (A, A+10 ms]): drain applies Start → node playing
               master pass: lead read → mix (node produces sample 0 into block at W_k0 = F0) → Commit
DAC:           plays frame F0 at  t_k0 + (F0 − P_true(t_k0))/Hz  ≈ t_k0 + lead_before(≈40 ms) + margin
```

### 3.2 Onset latency relative to the game's anchor

`onset − A = phase + lead_before + margin (+ device latency beyond the cursor)`
where `phase = t_k0 − A ~ U(0, 10 ms]` because the game thread's `Start` is
unsynchronised with the render loop, `lead_before ∈ [40, 50) ms` (steady-state
sawtooth, plus cursor-granularity wobble), `margin = Wc − P` (10 ms exactly
under Wine; platform-constant elsewhere). Everything but `phase` is
platform-constant and absorbed by the operator's SOUND_OFFSET calibration;
`phase` is a **±5 ms uniform play-to-play error the player cannot calibrate
away**. A deferred start (the wave started from the notify-thread pump because
it was scheduled in the future or its stream was still pending — the v2
capture's third play) lands near the far end of the window.

### 3.3 Why visual and aural move together

`mc` is computed once per frame from `T − A`; the same value positions the
arrows and (with the native press ages, §6.3) judges the steps. A wrong `A`
shifts both by the same amount. Nothing else in the pipeline shifts per play.

### 3.4 The in-song term

`mc` advances at the game tick's rate; the music advances at the DAC's rate. The
difference (§4) accumulates linearly through the song and is the "progressively
changes during a song" pattern.

### 3.5 What is exactly knowable

`F0` — the output-frame index of the song's sample 0 — is exact: read
`DS+0xC0/blockalign` inside the first `0x43CAC0` call for the song's node whose
`+0x5F8` goes 0 → >0 (the master's `Commit` has not yet run for that pass).
Everything about "when did sample 0 reach the DAC" then reduces to mapping
output frames to wall time through the play cursor `P` (§5).

**Ordering within a pass (cabinet-observed 2026-09-09, first arm of the
shipped clock):** source nodes produce in the render loop's pre-render step
(`dev->vt[0]()`, §2.2), BEFORE the master pass `0x43D350` that reads the
cursor (`0x435A50`), pulls the bus and commits the block. At produce time the
newest cursor sample is therefore the PREVIOUS pass's, and
`F0 = W_prev + 441` exactly (`1446480 − 1446039`). Both readings are
consistent with the model — `F0` is the block the coming master pass writes;
that pass's `lead`/`margin` are the inputs the latency constant averages.

---

## 4. The game tick is a boot-calibrated RDTSC

`libavs!XCnbrep700002c` (ordinal 45; `0x18000DFC0`) → `FUN_180053600` →
`(*DAT_1800FC388)()`. The pointer is chosen at boot by `FUN_180087B50`:

- "tick": `timeGetTime()` @1000 Hz by default, **RDTSC when the CPU has an
  invariant TSC** (`FUN_180078260() == 0`, logged as "Windows Vista/7/8
  synchronizes RDTSC between CPU cores on boot / CPU has Invariant TSC");
- "high precision timer": QPC, or RDTSC under the same condition;
- the RDTSC frequency is **measured against QPC over 5 × 100 ms**
  (`FUN_180078280`: spin until QPC advances by `freq/10`, count TSC, ×2).

The AVS boot log prints the choice and the calibrated rate:

```
I:boot: time: use RDTSC as tick
I:boot: time: use RDTSC as high precision timer
I:boot: time: tick: bits=64, freq=999984252/s, no wrap        ← CrossOver / Apple M3 Pro
```

Under Rosetta the emulated TSC runs at exactly 1 GHz; `999984252` therefore
makes every game millisecond `1e9/999984252 − 1 = +15.75 ppm` short — the game
clock runs fast by that amount, which is precisely the +15.9…+16.0 ppm game-vs-QPC
slope the v2 capture measured. On native hardware the 500 ms QPC-referenced
calibration should be good to a few ppm, and a CPU without an invariant TSC
falls back to `timeGetTime()` (whose rate is the system-timer crystal). **The
tester's `log.txt` `time:` lines identify which source their cabinet uses.**

The frame delta-time (`FUN_180210E30` → `FUN_180220240`) is a separate,
QPC-based clock used only for animation.

---

## 5. What the DAC cursor looks like (CrossOver, from the existing v2 capture)

From the 2532 `output_cursor` rows of `audio-sync-diagnostics-v2.csv`
(sha256 `51c492fe…`): the play-cursor deltas have gcd **2048 bytes = 512 frames
= 11.6 ms** — Wine's DirectSound play cursor is a 512-frame staircase; the write
cursor is always `play + 1764 bytes` (= 10 ms). This is why the v2 report's raw
cursor fits show 4.3–4.6 ms residual SD, and why a single cursor read cannot
anchor anything at 1 ms precision on this platform. The DAC rate is nevertheless
fully present in the staircase's long-run slope.

Win7's dsound-on-WASAPI emulation is expected to be a 10 ms-class staircase
too (its mixer runs on the WASAPI engine period); this is unmeasured. The v2
diagnostics build already records the cursor at 4 Hz — one tester run gives the
answer (gcd of `accumulated played bytes` deltas, or residual SD).

---

## 6. The visual pipeline (game side)

### 6.1 Frame structure (`FUN_180003000`, the per-frame application tick)

```
FUN_1801F3410()        // frames ≥2: BEGIN — drain the GPU executor (wait until idle), then submit last frame's streams
FUN_180210E30()        // frame dt (QPC)
…                      // timers, sound DoWork, per-frame subsystems
FUN_180023440()        // INPUT: for each player read the ark IO exports (panel exports write the
                       //        device timestamps into the per-button slots; other buttons get "now"),
                       //        FUN_1800230F0 records each button; LAST: FUN_1800231F0 → T = Ordinal_45(); state+0x1268 = T
broadcast 0x102        // actor update (GamePlayActor::onUpdate → mc, judge, render state)
layer dispatch         // 2D draw → gd command stream (the overlay_draw detour lives here)
broadcast 0x103
FUN_18002AF60(); FUN_1801F0C40()   // END — finalize the frame's command stream (its tail words 0x4000F/0x40002/0x4003B
                                   //        are written by FUN_180272560; Present and the per-frame fence query are stream
                                   //        commands executed by the executor — exact opcode→case mapping not decoded)
```

### 6.2 Executor thread and pacing

`FUN_18024EC80` is the **GPU executor thread**: it dequeues command streams from
a 16-slot ring (`DAT_1806F3180`, semaphore `DAT_1806F31D0`), executes them
(`FUN_18024D650`, whose Present case calls `IDirect3DDevice9::Present` /
`IDirect3DSwapChain9::Present` at `0x18024D7C9`/`0x18024D803`) and releases them
(`FUN_18024F980`). The device is D3D9 (not Ex), fullscreen `SwapEffect=FLIP`,
`BackBufferCount=1`, `PresentationInterval=D3DPRESENT_INTERVAL_ONE`,
`FullScreen_RefreshRateInHz = 60` (75 on machineType 1) — see
`docs/arbitrary_resolution_research.md` §2.

The main thread throttles itself with **`FUN_18024F260`: a full drain** — poll
`DAT_1806F31C0 == 0 && DAT_1806F31C4 == 0` (nothing queued, nothing executing)
with `Sleep(1)` — called from `FUN_1801F3410` at the START of every frame (via
`FUN_18026AEA0`) before the previous frame's stream is submitted. So:

```
frame N start:  wait until Present(N−2) has RETURNED  →  submit stream N−1  →  poll input (T_N) → update → build N
executor:       execute N−1 … Present(N−1) (blocks on the driver's flip queue / vblank)
```

With vsync the executor's `Present` blocks until a flip completes, so `T_N` is
sampled at `vblank + ε` where ε = main-thread wake-up (`Sleep(1)` granularity;
libavs calls `timeBeginPeriod(1)` at boot) — a **fixed phase**. Frame N is
scanned out a constant number of vblanks later (2 + the driver's queued-flip
depth). The game also creates `D3DQUERYTYPE_EVENT` queries (`FUN_1802572A0`)
and issues one per frame from the executor (`FUN_180257630`, polled by a
separate query thread) — resource-recycling fences, not a latency limiter.

### 6.3 What this means

- **Visual latency is constant in steady state.** There is no beat between the
  game loop and the refresh (the loop is paced by `Present` return), and no
  timer-driven frame loop that could drift against vblank.
- **Judgement does not depend on the input-poll phase.** For a press stamped
  `Pₛ` (IO-layer timestamp, T domain): `age = T − Pₛ`,
  `event = mc − age = (T − A − S + J) − (T − Pₛ) = Pₛ − A − S + J` — `T` cancels.
  Only the arrows' drawn position depends on when `T` was sampled, and that
  phase is fixed per §6.2 (≤1 ms `Sleep(1)` wobble ⇒ ≤ ~0.6 px at typical scroll).
- **Per-play visual offset = per-play `A` error (§3).** Fixing the anchor fixes
  the visual and the aural report together. Residual visual-only effects are
  dropped frames (a 16.7 ms hitch, an *outlier* pattern) and driver flip-queue
  depth changes (a constant-latency step, rare). A per-frame GPU fence could pin
  the queue depth at the cost of GPU parallelism; not recommended without
  evidence from the affected hardware.
- The main-thread `T` store is the last instruction of `FUN_1800231F0`; a
  post-original detour reading `+0x1268` plus QPC pairs the game tick with QPC
  at ~100 ns skew — the only extra game-side hook a cursor-authority clock needs.

---

## 7. Deterministic audio clock — shipped as `gameplay-timing-fixes` (2026-09-09)

Design: `.agents/scratchpad/2026-09-08-frame-timing/clock-rate-correction/design.md`;
code map: `AGENTS.md` → "Gameplay Timing Fixes"; deploy log: the same
scratchpad's `progress.md`.
In one paragraph: observe every mix pass (`t_k, P_k, Wc_k, W_k` from the
existing `0x435A50` seam, now a shared dispatcher); latch `F0` at the song
node's first produce (`0x43CAC0`, §3.5); publish a DAC frame↔QPC line
reconstructed from the cursor staircase over the last ~10 s (a fixed-lag
smoother — needed only because the cursor is coarse; the extrapolation horizon
is ONE pass, so rate precision is irrelevant and seconds of history suffice,
not minutes); replace `T − A` in the clock-patch stub with
`(P̂(t_frame) − F0)/Hz + C + origin`, where `t_frame` is the QPC paired with
this frame's `T` and `C = 5 ms + mean(lead_before) + mean(margin)` keeps
existing SOUND_OFFSET calibrations valid on average. Native press ages need no
transformation (rate mismatch × ≤250 ms age = microseconds).

### 7.1 Live confirmation (CrossOver, 2026-09-09, five launches)

- Every song, quick restart and training scrub armed on the FIRST frame after
  `F0` (`waited=0`), `|E − (T−A)|` well inside the 50 ms gate; clean disarms
  at cue stop / scene exit; zero REFUSED / Diverged events.
- **Stock onset error measured directly** (`delta_vs_stock` per arm):
  −11.1 … +3.5 ms over ~19 plays on one cabinet, uniform-looking — the
  posted-`Start` model of §3.2 (0..10 ms per play) is what the field reports
  were feeling. Mean ≈ −2.7 ms (small n): if the stock mean phase really sits
  later than the 5 ms the model assumes, `C` is a few ms early on average;
  `latency_bias_ms` and the auto-calibration absorb it — the model is NOT to
  be changed on CrossOver samples alone.
- **In-song drift measured directly** (per-frame CSV, stock `T−A−S+J`
  recomputed vs the judged `mc`): the applied correction is a flat per-song
  offset drifting −16…−22 ppm = RDTSC(+16 ppm, §4) − DAC(−4…−6 ppm vs QPC,
  §5). The analyzer's game-clock-vs-QPC slope went from +16 ppm to −2…−7 ppm
  with 0.4–0.7 ms residual SD.
- Seeks: the game re-anchors BEFORE `song_reset` republishes the content
  origin; the in-between frame drops to Gating (passthrough) and re-arms on
  the next — this is what makes scrubs safe, not a special case.
- CrossOver cursor granularity: fit residual SD 4.2–4.6 ms (the 512-frame
  staircase, §5). Win7 unmeasured until the tester's capture.

### 7.2 Assist-tick alignment (design §10, implemented as ONE post-onset re-lay)

The tick track is a separate cue (§8 of the earlier draft listed it as a
follow-up); its voice `Start` is posted like the song's, so `F0_tick − F0_song
= 441·n` exactly (`+15876 = 36 passes`, `+441 = 1 pass` observed). With both
onsets exact, `skip* = (E_tick0 − S + J) − wall(m0) − C` is the shift the
commit SHOULD have served from (design §10 omitted the `−C`; derivation in
`tick_align.rs`); the claps are re-laid at SAMPLE positions for the observed
`F0_tick`, encoded, and copied over the mod-owned bank from
`consumed_bytes(node) + 8 KiB` — landing ~340 ms into the track, still inside
the READY dwell. Two facts learned live:

- **The tick voice's identity lands ASYNCHRONOUSLY.** `SoundBank::Play`
  returns before the engine's in-memory submission (`0x419DB0`) runs on the
  notify pump, so a generation read "synchronously after Play" can be stale
  by one voice. The alignment therefore never decides at commit: it waits for
  the first aux onset with `generation ≥ commit-time floor`, the song's frame
  epoch, and `F0_tick ≥ F0_song`.
- **The commit's own cost is a measurable onset error.** The first commit of
  a process registers the 28.9 MB tick bank and every commit memcpys the full
  segment; on the run that exercised it the game thread stalled ~19 ms between
  the frame's `T` and the `Play`, and the alignment measured exactly that
  (`correction +18.60 ms`) — larger than the ±5 ms pass phase, and real.
  Stock ticks would have been that late for the whole song.

Also folded in: `synthesize_track_at_samples` now mixes/encodes only through
the last clap and pads with the canonical silence block (byte-identical; MS-ADPCM
blocks are self-contained), cutting per-song synthesis from ~2.9 s to ~0.3 s —
which incidentally moved the stock-shaped tick commit from ~2.5 s INTO the
chart to before chart time 0.

---

## 8. Open / unmeasured

- Win7 DS cursor granularity and the stock onset distribution on the tester's
  cabinet (the first tester run of the shipped build answers both via the
  `armed … fit(sd=…)` lines / `onset` CSV rows).
- Whether the tester's CPU takes the RDTSC or the `timeGetTime()` tick path
  (their `log.txt`).
- Whether the mean stock onset phase is 5 ms (the model) or a few ms later
  (the CrossOver mean `delta_vs_stock` ≈ −2.7 ms hints so, n ≈ 19).
- Whether anything other than the master pass calls `0x435A50` (the two
  position thunks are reachable via secondary slots 1/2; no static caller found).
- Whether the song voice routes through an intermediate submix node adding one
  pass of constant delay (irrelevant to consistency).
- Assist-tick pre-swap window: the ≤ ~340 ms of track served before the re-lay
  lands keeps the commit's alignment. Registering the tick bank at song build
  (not inside the first commit) and extent-limiting `rewrite_tick_wave` would
  shrink that window's error; not needed for correctness.
