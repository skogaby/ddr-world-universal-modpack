# Assist Tick — Feasibility: a mod-owned audio output path

**Date:** 2026-07-25
**Scope:** Can the hook DLL open its own audio output client inside the DDR World process, and what
breaks if it does? Covers device exclusivity, latency/jitter, Rust/`windows`-crate cost, the decode
pipeline, and graceful degradation. **Does not** cover the trigger side (where the arrow-hit
timestamps come from) — that is separate research.

**Evidence labels used throughout:**
`[VERIFIED]` = observed directly in this session (Ghidra on the shipped DLL, files on disk, the
cabinet's own `log.txt`, the vendored crate source, or an authoritative public doc, all cited).
`[INFERENCE]` = reasoned from verified facts, not itself observed.
`[RECALL]` = from prior knowledge, unverified — treat as a hypothesis to test.

---

## 1. Overview

### Verdict summary

| Question | Answer | Confidence |
|---|---|---|
| Does the game hold the audio device exclusively? | **No.** DDR World plays audio through **XACT 2.10 → DirectSound**, which on Vista+ is a shared-mode client of the WASAPI engine. Multiple streams are explicitly supported. | **High** — multiple independent verified sources |
| Can a mod-owned XAudio2 (shared WASAPI) client coexist? | **Yes**, on a stock cabinet with the launch flags this project actually uses. | **High** |
| Is sub-frame (<16 ms) *placement* achievable? | **Yes** — but only with an **always-fed** source voice whose sample clock we anchor to QPC. The naive "start a voice per tick" approach quantises to the XAudio2 processing pass (~10 ms) *plus* the game-frame quantum, i.e. ±20 ms jitter. | **Medium-High** |
| Is sub-frame *end-to-end latency* achievable? | **No, and it does not matter.** Expect ~20–35 ms output latency on a stock Win10 shared-mode path. Latency is a **constant** and is calibrated away with an offset knob (the project already ships `timing_offsets.sound_offset`, default `87`, README.md:197). Jitter is what must be small. | **High** |
| Do we need an Ogg decoder? | **No.** Transcode to raw 16-bit PCM offline; `include_bytes!` an 18,846-byte blob. Verified by running the transcode. | **High** |
| Biggest risk | The mod-owned stream lands on a **different endpoint / different mixer path** than the cabinet's speakers, or an operator uses a non-default spice2x audio configuration (ASIO). Silent failure, not a crash. | — |

### The one-line recommendation

Use **XAudio2** (2.9 → 2.8, resolved at runtime via `LoadLibrary`/`GetProcAddress`, never as a
load-time import), a **single mastering + single source voice kept permanently fed** with a
continuously-submitted silence carrier into which we additively mix the clap PCM at exact sample
offsets, with **DirectSound as the documented fallback** for hosts where XAudio2 is absent.

---

## 2. What the game and spice2x do for audio

### 2.1 `gamemdx.dll` itself contains no audio API at all

`[VERIFIED]` The full import table of `gamemdx_20260721.dll` (Ghidra `list_imports`, 327 entries,
image base `0x180000000`) contains **no** XAudio2, DirectSound, WASAPI, `waveOut*`, or ASIO symbol.
The only audio-adjacent imports are the `winmm` timer family (`timeGetTime`, `timeSetEvent`,
`timeBeginPeriod`, …) — timing, not sound — plus `CoInitialize` / `CoInitializeEx` /
`CoCreateInstance` (COM, used for the DirectShow movie player and for XACT).

`[VERIFIED]` The same is true of `ess.dll` (Ghidra `list_imports`): no audio APIs; its imports are
filesystem, `GetAdaptersAddresses`, `DeviceIoControl`, `MessageBoxA` — i.e. it is not the sound
module despite the suggestive name.

`[VERIFIED]` A scan of every module in
`…/CrossOver/Bottles/bemani/drive_c/ddr_world/contents/modules/` for audio-API strings returns
audio-stack names only in `gamemdx.dll` (`audioses`, `avrt.dll`, `mmdevapi`, `winmm.dll`) and bare
`WINMM.dll` in `arkmdx*.dll` / `libavs-win64.dll`. No module imports XAudio2 or ASIO.

### 2.2 The audio engine is **XACT 2.10**, and XACT 2.x on Windows outputs via **DirectSound**

`[VERIFIED]` Strings in `gamemdx_20260721.dll`:

| Ghidra address | String |
|---|---|
| `0x1802de0a0` | `xactengine2_10.dll` |
| `0x1802de3c8` | `audiokse.dll` (Konami sound-engine wrapper) |
| `0x1802ddc20` | `data/arc/soundbanks.arc` |
| `0x18035d2e0` | `data/sound/win/voice.xwb` |
| `0x18035d390` / `0x18035d388` | `/%s.xsb` / `/%s.xwb` |
| `0x180380088` | `Software\Microsoft\XACT` |

Plus MSVC RTTI names `.?AVXsbFileCallback@audio@@` and `.?AVXwbFileCallback@audio@@` (`strings` on
the shipped `gamemdx.dll`). `.xsb` = XACT sound bank, `.xwb` = XACT wave bank.

`[VERIFIED]` Those banks exist on disk as real game data:
`…/ddr_world/contents/data/sound/win/{voice.xwb, voice_n.xwb, bgm_menu.xwb}` and per-song
`data/sound/win/dance/<4-char>.{xsb,xwb}`.

`[VERIFIED]` The engine DLL ships with the game at
`…/ddr_world/contents/com/xactengine2_10.dll` (404,120 bytes) and **spice2x registers it as a COM
server at boot**, from the cabinet's own log (`…/ddr_world/contents/log.txt`):

```
log.txt:22  I:ddr: found DLL: xactengine2_10.dll, size: 404120 bytes
log.txt:23  I:ddr: `regsvr32.exe /s "C:\ddr_world\contents\modules\..\com\xactengine2_10.dll"` returned 0
```

`[VERIFIED]` `xactengine2_10.dll`'s own string table contains `DirectSound`,
`DirectSoundEnumerateW`, `ole32.dll`, `WINMM.dll` — and **no** WASAPI / XAudio2 / ASIO names. So the
XACT 2.10 engine renders through **DirectSound** and enumerates devices with
`DirectSoundEnumerateW`.

`[VERIFIED]` Corroboration from a second angle: `gamemdx` carries a null-terminated table of
module-name string pointers at `0x180465e20`… whose strings (`0x1802de3a0`+) are
`kmd.dll, kld.dll, kuc.dll, ksuser.dll, audiokse.dll, audioses.dll, propsys.dll, mmdevapi.dll,
clbcatq.dll, wintrust.dll, avrt.dll, msacm32.dll, …` — and a second table at `0x180465fc0` pointing
at `k-clvsd.dll`, `xactengine2_10.dll` and neighbours.
`[INFERENCE]` `ksuser.dll + audioses.dll + propsys.dll + mmdevapi.dll + avrt.dll + msacm32.dll` is
precisely the transitive dependency closure of **`dsound.dll` on Vista+**. That is why those WASAPI
names appear in `gamemdx` at all: the game never calls WASAPI itself, it just knows the DLL set its
DirectSound stack drags in. `[INFERENCE]` The table is a preload/expectation list, not an enforced
allowlist — no "illegal module" style strings exist in the binary (Ghidra string search for
`illegal|unknown module|invalid module|not allowed|unauthoriz` returns only
`"Called with illegal stage. stage=%d"` and `"draw_primitive ) Illegal param"`), and this project
already injects its own DLL successfully.

### 2.3 spice2x: DDR is explicitly a DirectSound game

`[VERIFIED]` spice2x's own documentation, *Audio modes demystified*
(<https://github.com/spice2x/spice2x.github.io/wiki/Audio-modes-demystified>), section
**DirectSound (DSound)**:

> "**DDR**, popn, etc, including many older versions of games (IIDX24 and below, gfdm XG3 and
> below, older SDVX…). Highest latency, but compatible with basically everything. **Allows for
> multiple audio streams.** Audio can be captured."

Contrast the same page's WASAPI-exclusive section ("it only allows one audio stream at a time (i.e.
the game takes over all of audio, preventing other games and applications from using it)") which
lists IIDX 25-30, SDVX nemsys, Gitadora — **not** DDR.

`[VERIFIED]` The same page's *Optimizing for latency* ranking and the `-lowlatencysharedaudio`
section:

> "Use `Low Latency Shared Audio` option (`-lowlatencysharedaudio`) to reduce latency. If your audio
> device supports it, it can lower the round-trip latency from a typical 10ms down to 2-3ms…
> Requires Windows 10 or higher. **Works for DirectSound and shared mode WASAPI**, but does not work
> for exclusive WASAPI or ASIO."

`[VERIFIED]` spice2x wraps both layers in-process. Strings in the shipped
`…/ddr_world/contents/spice64.exe` include the mangled/RTTI names
`WrappedIDirectSound8`, `WrappedIDirectSoundBuffer`, `WrappedIAudioClient`, `WrappedIMMDevice`,
`WrappedIMMDeviceEnumerator`, `DummyIAudioClient`, `AsioBackend`, plus log strings
`DirectSoundCreate8 hook hit`, `CoCreateInstance(CLSID_MMDeviceEnumerator)`,
`IMMDevice::Activate(IID_IAudioClient3...)`, `audio::dsound`, and the option names
`audiobackend, audiodummy, audiohookdisable, asioconvert, asiodriverid, asioforceunload, iidxasio,
lowlatencysharedaudio`.

`[VERIFIED]` The help text for the ASIO conversion option is explicit that it is a no-op for DDR:

> "Selects the audio backend to use when spice audio hook is enabled, **overriding exclusive
> WASAPI. Does nothing for games that do not output to exclusive WASAPI.**"
> "Converts **WASAPI Exclusive** audio output to ASIO."

### 2.4 The launch configuration actually in use has no audio flags

`[VERIFIED]` `…/ddr_world/contents/gamestart-bemanibuddy.bat` (and the `-logger` variant):

```
.\spice64.exe -modules modules -ddr -w -p <PCBID> -url http://127.0.0.1:5720 \
    -api 1337 -apipass lolhax -icmphook -K ddr_world_hook.dll
```

No `-audiobackend`, `-asioconvert`, `-audiodummy`, `-audiohookdisable`, or
`-lowlatencysharedaudio`. So: stock DirectSound path, spice's default `IAudioClient`/DirectSound
wrappers installed, default endpoint, default (10 ms) shared-mode buffer.

`[VERIFIED]` The audio hook does initialise at boot: `log.txt:275 I:audio: initializing`.

### 2.5 Development host (CrossOver/Wine) note

`[VERIFIED]` The bottle contains 64-bit `windows/system32/{dsound.dll, mmdevapi.dll,
xaudio2_0…xaudio2_9.dll, xactengine2_*/3_*.dll}`. So both a DirectSound and an XAudio2 client have a
provider on the dev host.
`[RECALL]` Wine's `xaudio2_*` is implemented on top of **FAudio**, which is actively maintained and
far more robust than Wine's `quartz` (the DirectShow component that forced
`src/mods/non_native_os_support.rs` to exist). I would expect XAudio2 to work under CrossOver, but
this is unverified and must be smoke-tested on the dev host.

---

## 3. Exclusivity verdict and residual risk

### Verdict

**A second, mod-owned shared-mode output client is viable.** DDR World does not take exclusive
control of the endpoint: it uses DirectSound, which on every OS this project targets is a *client*
of the WASAPI shared mixer, and spice2x's own docs state DirectSound "allows for multiple audio
streams". Nothing in the game, in `xactengine2_10.dll`, or in the launch line asks for
`AUDCLNT_SHAREMODE_EXCLUSIVE` or an ASIO driver. **Confidence: high.**

Bonus: `[VERIFIED, mechanism from MS docs]` if an operator *does* enable
`-lowlatencysharedaudio`, our stream benefits for free — Microsoft documents that "if one
application requests the usage of small buffers, then the audio engine will start transferring audio
using that particular buffer size. In that case, **all applications that use the same endpoint and
mode will automatically switch to that small buffer size**"
(<https://learn.microsoft.com/en-us/windows-hardware/drivers/audio/low-latency-audio>, FAQ). spice's
own log text confirms that is exactly how the option is implemented ("this is NOT used to output
sound, but rather to reduce buffer sizes when the game requests an audio client at a later point").

### Residual risks (ordered by likelihood × impact)

| # | Risk | Why | Mitigation |
|---|---|---|---|
| R1 | **Our stream reaches a different output than the speakers.** XACT enumerates renderers with `DirectSoundEnumerateW` and *may* select a non-default renderer by index/name; spice2x additionally wraps `IMMDeviceEnumerator`/`IMMDeviceCollection` and can even synthesise a fake device entry ("`WrappedIMMDeviceCollection::Item[{}] -> synthetic fake Realtek render device`"). An XAudio2 mastering voice created with `szDeviceId = NULL` goes to the **default render endpoint**. If those differ, the tick is inaudible with no error. | `[VERIFIED]` strings/imports as above; `[INFERENCE]` for the divergence scenario | Log the chosen endpoint's friendly name at init; expose an optional `device` substring in `mod-config.json` (out of scope for v1); accept "silence with a clean log line" as the failure mode |
| R2 | **Operator uses ASIO** (`-iidxasio`-style forcing, or an Xonar card auto-detected). ASIO takes the hardware exclusively and mutes other streams (spice wiki: "On most audio hardware, running ASIO will be an exclusive operation (mutes all other audio streams)"). | `[VERIFIED]` spice wiki + the `SOUND_OUTPUT_DEVICE`/`-iidxasio` strings in `spice64.exe` | Not applicable to DDR by default (`-asioconvert` "does nothing for games that do not output to exclusive WASAPI"). Document as unsupported; fail silent |
| R3 | **XAudio2 provider absent** — Windows 7 has no inbox `XAudio2_8/2_9.dll`. `[VERIFIED]` <https://learn.microsoft.com/en-us/windows/win32/xaudio2/xaudio2-versions>: 2.9 ships in Windows 10 (redist available for 7 SP1/8/8.1), 2.8 is inbox in Windows 8; 2.7 and earlier were DirectX-SDK redistributables. This repo maintains a real Win7 build path (`build_win7.sh:1-14`). | `[VERIFIED]` | **Never** take a load-time import on `xaudio2_*.dll` (see §5.1). Resolve at runtime; if absent, either fall back to DirectSound or self-disable the mod |
| R4 | **Volume mismatch.** spice hooks the volume APIs to stop games slamming the endpoint to 100% (`Default: off (prevent games from changing audio volume by hooking IAudioEndpointVolume)`), and the game's own in-game volume applies only to its own XACT categories. Our tick is on a separate session — it will not track the game's music volume. | `[VERIFIED]` spice2x strings/wiki | Expose a tick-volume scalar row (the mod-option framework already supports scalar rows — README.md:35) and pick a conservative default |
| R5 | **Non-native OS (CrossOver).** Untested XAudio2/FAudio path. | `[RECALL]` | Smoke-test on the dev bottle before cabinet deploy; the mod must fail-open exactly like `non_native_os_support` does |

---

## 4. Latency and jitter analysis

### 4.1 Where the time goes (shared-mode output)

| Stage | Windows 10/11 | Windows 7/8.x | Source |
|---|---|---|---|
| Our scheduling slack (lookahead we choose) | design choice, 20–80 ms | same | — |
| XAudio2 processing quantum | 10 ms (`XAUDIO2_QUANTUM_NUMERATOR/DENOMINATOR = 1/100`) | 10 ms | `[VERIFIED]` windows-0.58.0 `…/Media/Audio/XAudio2/mod.rs:705-706` |
| WASAPI audio-engine processing | **1.3 ms** | ~12 ms (float) / ~6 ms (int) | `[VERIFIED]` MS low-latency-audio, "Windows audio stack" items 4-5 |
| Shared-mode endpoint buffer | 10 ms default; as low as **2.67 ms** (128 frames @48 kHz) with the inbox HDAudio driver | always ~10 ms | `[VERIFIED]` same doc, items 7-8 + "Measurement tools" |
| Driver + codec + amp | device-specific, typically 1–10 ms | same | `[RECALL]` |

**Realistic totals** (`[INFERENCE]` from the cited components, excluding our own lookahead):

* Stock Win10 cabinet, no spice audio flags: **~20–25 ms**, plausibly up to ~35 ms with a
  vendor-driver + amp.
* Win10 + `-lowlatencysharedaudio` on a cooperative driver: **~10–15 ms**.
* Windows 7: **~30–50 ms**.

**This does not need to be small.** It is a *constant*, and the mod ships an offset knob; the repo
already normalises this exact class of problem with `timing_offsets.sound_offset` (README.md:197,
default `87` ms). What must be small is **jitter** — a tick that wanders ±15 ms around the beat is
perceived as flamming and actively harms a timing aid.

### 4.2 XAudio2 has no absolute-time scheduling

`[VERIFIED]` The only scheduling primitives are:

* **Buffer queueing.** `SubmitSourceBuffer` appends to a per-voice FIFO (max
  `XAUDIO2_MAX_QUEUED_BUFFERS = 64`, windows-0.58.0 `…/XAudio2/mod.rs:696`) and queued buffers play
  **back-to-back, gaplessly**. `XAUDIO2_BUFFER` exposes `PlayBegin`/`PlayLength` so one static blob
  can be sliced without copying (struct fields verified in the vendored crate source).
* **Operation sets.** `Start`/`Stop`/`SetVolume`/… take an `OperationSet`; `CommitChanges` applies a
  group atomically and "is guaranteed to be sample-accurate. For example, voices will start in
  sync." But: "All other methods that take an *OperationSet* argument only take effect **on the next
  processing pass** after the method is called"
  (<https://learn.microsoft.com/en-us/windows/win32/xaudio2/xaudio2-operation-sets>).
  → Operation sets give **relative** sample accuracy between voices, **not** absolute placement.
  A `Start()` lands on a processing-pass boundary, i.e. quantised to the ~10 ms quantum.

There is **no** "play at time T" API. Absolute placement must be built from buffer content.

### 4.3 What the voice's own clock gives us

`[VERIFIED]` `XAUDIO2_VOICE_STATE { pCurrentBufferContext, BuffersQueued, SamplesPlayed: u64 }`
(vendored source). `GetState` docs
(<https://learn.microsoft.com/en-us/windows/win32/api/xaudio2/nf-xaudio2-ixaudio2sourcevoice-getstate>):
`SamplesPlayed` is the voice's cursor; `XAUDIO2_VOICE_NOSAMPLESPLAYED` skips it (≈3× faster call);
and "If a client needs to get the correlated positions of several voices … it must make `GetState`
calls in an XAudio2 engine callback. This ensures that none of the voices advance while the calls
are being made."

`[VERIFIED]` from the struct docs, `SamplesPlayed` = "total number of samples processed by this
voice since it last started, or since the last audio stream ended (as marked with the
`XAUDIO2_END_OF_STREAM` flag)".

**Two consequences** `[INFERENCE]`:

1. `SamplesPlayed` counts samples the voice *processed* (submitted into the mix), not samples
   *heard*. The offset between the two is the pipeline latency of §4.1 — unknown but constant.
   → Calibrate with a config offset; do not try to measure it.
2. `SamplesPlayed` **does not advance while the voice is starved** (nothing is being processed).
   So a voice that is allowed to run dry loses its clock: the mapping
   `SamplesPlayed ↔ QueryPerformanceCounter` silently gains an unknown offset after every gap.
   → **Sample-accurate absolute placement requires a voice that is never starved.**

### 4.4 Evaluating the pre-roll-silence technique

The proposal — "submit a buffer whose leading N samples are silence so the clap lands
sample-accurately relative to the voice's own sample clock" — is **correct in principle and cheap to
implement**, with one important caveat and one nice optimisation.

**It works because** queued buffers are gapless, so the clap's onset is exactly `N` samples after
whatever the voice was already going to play. No copying is needed: submit the silence as a
`PlayLength`-limited slice of a single static zero blob, then submit the clap blob — two
`SubmitSourceBuffer` calls, zero allocation per tick:

```
queue: [ zeros: PlayBegin=0, PlayLength=N ] , [ clap: full ]
```

`[VERIFIED]` `PlayBegin`/`PlayLength` exist on `XAUDIO2_BUFFER`; `[INFERENCE]` gapless concatenation
across queued buffers is the documented behaviour of the source-voice FIFO.

**The caveat:** the technique is only *sample*-accurate if the voice's clock is continuous, i.e.
per §4.3(2) the voice must be running and fed. Two design shapes follow:

#### Design A — one-shot voices with pre-roll (simple, ±10–20 ms jitter)

Keep a small pool of voices; per tick pick a free one, queue `[zeros(N)][clap]`, `Start()`.
* Pro: ~40 lines, no mixing, no feeder.
* Con: `Start()` lands on the next processing pass → up to one quantum (~10 ms) of *error*, and the
  error is not constant (it depends on the phase of the call inside the quantum) → **jitter**. Add
  the game-frame quantum if the trigger comes from a per-frame hook (16.7 ms at 60 Hz) and you are
  at roughly **±10 ms typical / ±20 ms worst**. `[INFERENCE]` from §4.2.
* Con: a single voice serialises its queue, so overlapping claps need separate voices. At 16th notes
  and 150 BPM the inter-onset interval is 100 ms — shorter than the 213 ms clap — so overlap is
  routine and a pool (≥4, ideally 8) is mandatory.

#### Design C — one always-fed voice, we mix (recommended, sub-ms jitter)

One mastering voice + **one** source voice, `Start()`ed once at init and never stopped. We own a
small ring of i16 mono samples; every game frame we top the queue up so it never runs dry
(e.g. keep 60–80 ms queued, submit 16–20 ms chunks). Scheduling a tick = additively mixing (with
saturation) the clap PCM into the ring at an exact sample offset derived from
`anchor_samples_played ↔ anchor_qpc`.

* Pro: **placement error is a single sample**, jitter ≈ 0. Overlapping claps are free (additive
  mix). No voice pool. No queue-depth games. `SamplesPlayed` stays a valid monotonic clock because
  the voice never starves.
* Pro: no extra thread needed — the repo already dispatches per-frame work (e.g.
  `src/mods/power_user_statistics/calorie_feed.rs` detours a per-frame actor tick; AGENTS.md
  documents `services/render_notes_hook` as a per-frame dispatcher). Topping up 16 ms of audio per
  16.7 ms frame is trivially affordable.
* Con: we write ~80 lines of trivial mixing; a frame hitch longer than the queued lookahead causes
  a starvation click. Mitigate with a generous 60–100 ms queue and by re-anchoring the clock (and
  logging) whenever `BuffersQueued == 0` is observed.

**Recommendation: Design C.** It is the only shape that delivers on "sample-accurate relative to
the voice's own sample clock", and it is barely more code than A once A's voice-pool bookkeeping is
counted. Design A is a reasonable stepping stone for a first smoke test ("does any sound come out
of this process at all").

### 4.5 Jitter budget for Design C

| Contributor | Magnitude | Note |
|---|---|---|
| Sample placement in the ring | ±1 sample (±0.023 ms) | exact arithmetic |
| QPC ↔ `SamplesPlayed` anchor drift | device clock vs. QPC drift, ~10⁻⁴–10⁻⁵ | re-anchor every second; irrelevant over a song if re-anchored |
| Trigger-side timestamp quality | **dominant unknown** | out of scope — depends on whether the arrow-hit time is available as a song-time value or only as a per-frame observation. If the trigger is per-frame-only, the tick inherits ±8 ms and Design C's precision is wasted |
| Output pipeline | constant (§4.1) | calibrated away |

`[INFERENCE]` **The binding constraint on this feature is the trigger side, not the audio side.**
Whatever the audio design, the tick can only be as precise as the hit-time source. That should be
stated in the design doc as a dependency.

---

## 5. Rust / `windows`-crate implementation sketch

*(No repo code was changed. These are sketches for the design phase.)*

### 5.1 The one real trap: do **not** use `windows`' `XAudio2CreateWithVersionInfo`

`[VERIFIED]` windows-0.58.0 exposes exactly one XAudio2 factory function, and it links against the
**Windows 8** DLL:

```rust
// …/windows-0.58.0/src/Windows/Win32/Media/Audio/XAudio2/mod.rs:25-27
pub unsafe fn XAudio2CreateWithVersionInfo(...) -> Result<()> {
    windows_targets::link!("xaudio2_8.dll" "system" fn XAudio2CreateWithVersionInfo(...) -> HRESULT);
    ...
}
```

There is **no** `XAudio2Create` binding (grep confirms only the `WithVersionInfo` variant exists).
`[VERIFIED]` `windows_targets::link!` resolves either to
`#[link(name = <dll>, kind = "raw-dylib", modifiers = "+verbatim")]` (when `windows_raw_dylib` is
configured) or to the bundled umbrella import library
`windows_x86_64_msvc-0.52.6/lib/windows.0.52.0.lib` (5.2 MB, present in the vendored crate). Both
paths produce a **load-time import**.

`[INFERENCE]` Consequences:
* A load-time import on `xaudio2_8.dll` means the hook DLL **fails to load entirely** on any host
  without that DLL — exactly the class of bug `build_win7.sh` was written to fix
  (`build_win7.sh:2-7`: `ProcessPrng` from `bcryptprimitives.dll` "doesn't exist on Win7 and causes
  the loader to reject the DLL"). Unacceptable.
* `XAudio2CreateWithVersionInfo` was added with XAudio 2.9 / RS5, so a `xaudio2_8.dll`-named import
  of it is doubly fragile.
* Enabling the `Win32_Media_Audio_XAudio2` **feature** is nonetheless safe, because the linker only
  emits an import for symbols that are actually referenced — and we will never reference that
  function. The feature is worth enabling purely for the COM interface types (`IXAudio2`,
  `IXAudio2SourceVoice`, `XAUDIO2_BUFFER`, `XAUDIO2_VOICE_STATE`, the constants), which are pure
  vtable definitions with no imports.

**Do this instead** (runtime binding, fail-open, matches the project's existing hook conventions):

```rust
// Cargo.toml additions (feature names verified in windows-0.58.0/Cargo.toml:479-483)
//   "Win32_Media_Audio",            // WAVEFORMATEX, WAVE_FORMAT_PCM
//   "Win32_Media_Audio_XAudio2",    // IXAudio2 & friends (types only)
//   "Win32_System_Com",             // CoInitializeEx (only if we own a thread)
// (Win32_System_LibraryLoader / Win32_Foundation are already present: Cargo.toml:16-24)

use windows::Win32::Media::Audio::XAudio2::*;
use windows::Win32::Media::Audio::{WAVEFORMATEX, WAVE_FORMAT_PCM};
use windows::core::{HRESULT, Interface};

type XAudio2CreateFn = unsafe extern "system" fn(
    ppxaudio2: *mut *mut core::ffi::c_void,
    flags: u32,
    processor: u32,
) -> HRESULT;

/// Resolve XAudio2 without taking a load-time dependency. 2.9 first (inbox on Win10+,
/// and what the CrossOver bottle ships), then 2.8 (inbox on Win8.x).
unsafe fn create_engine() -> Option<IXAudio2> {
    for dll in ["XAudio2_9.dll", "XAudio2_8.dll"] {
        let h = LoadLibraryA(PCSTR(format!("{dll}\0").as_ptr())).ok()?; // (real code: cached CString)
        let p = GetProcAddress(h, s!("XAudio2Create"));
        let Some(p) = p else { continue };
        let f: XAudio2CreateFn = core::mem::transmute(p);
        let mut raw = core::ptr::null_mut();
        if f(&mut raw, 0, XAUDIO2_DEFAULT_PROCESSOR).is_ok() && !raw.is_null() {
            return Some(IXAudio2::from_raw(raw));
        }
    }
    None // → mod logs a warning and self-disables (see non_native_os_support precedent)
}
```

`[VERIFIED]` `XAUDIO2_DEFAULT_PROCESSOR = 1` (`…/XAudio2/mod.rs:667`);
`IXAudio2::from_raw` is available via `windows_core::Interface`.

### 5.2 COM / threading

`[VERIFIED]` MS: from XAudio 2.8 onward, "`XAudio2Create` is a flat Win32 API call and no longer
creates an XAudio2 CLSID. Support for instantiating XAudio2 by `CoCreateInstance` has been removed."
(<https://learn.microsoft.com/en-us/windows/win32/xaudio2/xaudio2-versions>). So **we do not need an
apartment for creation**, and we do not need `CoCreateInstance`.

`[VERIFIED]` The game does call `CoInitialize`/`CoInitializeEx` (import table) — COM is already
initialised on its threads, apartment model unknown.
`[INFERENCE]` Practical guidance:
* Create the engine **lazily on a game thread at mod-enable / scene-entry time**, never in
  `DllMain` (loader lock — the project already builds everything this way).
* Do **not** call `CoUninitialize`, and treat `RPC_E_CHANGED_MODE` from any `CoInitializeEx` of ours
  as benign. Safest is to not call `CoInitializeEx` at all on a thread the game owns.
* XAudio2 spins up its **own** internal audio thread, so Design C needs no thread of ours — the
  per-frame top-up runs on whatever thread the existing frame hook runs on. If a dedicated thread is
  ever wanted, `CoInitializeEx(None, COINIT_MULTITHREADED)` on that thread and keep it alive for the
  engine's lifetime.
* `SubmitSourceBuffer` is documented as callable from any thread; still, funnel all voice calls
  through one thread (the frame hook) to avoid needing a lock. `[RECALL]` on the thread-safety
  claim — verify before relying on cross-thread submits.

### 5.3 The format and buffer structs (hand-constructed, no header needed)

`[VERIFIED]` `WAVEFORMATEX` and `WAVE_FORMAT_PCM = 1` live in `Win32::Media::Audio`
(`…/Media/Audio/mod.rs:6745-6753`, `:4119`). A bare PCM blob is all XAudio2 wants —
`XAUDIO2_BUFFER` is `{ Flags, AudioBytes, pAudioData, PlayBegin, PlayLength, LoopBegin, LoopLength,
LoopCount, pContext }`. **No RIFF/WAV header, no `.wav` parsing.**

```rust
const SR: u32 = 44_100;
let wfx = WAVEFORMATEX {
    wFormatTag: WAVE_FORMAT_PCM as u16, // 1
    nChannels: 1,
    nSamplesPerSec: SR,
    nAvgBytesPerSec: SR * 2,            // 88_200
    nBlockAlign: 2,                     // 1 ch * 16 bit / 8
    wBitsPerSample: 16,
    cbSize: 0,
};
```

`[INFERENCE]` The device will almost certainly be running at 48 kHz. A source voice created at
44.1 kHz is resampled by XAudio2's per-voice SRC automatically (that is what
`maxFrequencyRatio` / `XAUDIO2_DEFAULT_FREQ_RATIO = 2.0` is about — verified constant at
`…/XAudio2/mod.rs:666`). For mono this is negligible CPU and introduces no jitter, only a fixed
group delay. Simplest correct choice: keep everything at 44.1 kHz and let XAudio2 resample. (If the
mix rate ever matters, `IXAudio2MasteringVoice` can be created with an explicit rate, or we ship a
second 48 kHz blob — not worth it for v1.)

### 5.4 Lighter alternatives, ranked

| Option | Header-free / cross-compiles? | Latency | Sample-accurate placement? | Verdict |
|---|---|---|---|---|
| **XAudio2** (runtime-bound) | Yes — `windows` 0.58 feature `Win32_Media_Audio_XAudio2` `[VERIFIED]`; pure Rust after `GetProcAddress` | ~20–35 ms stock | Yes, via always-fed voice + `SamplesPlayed` | **Recommended** |
| **DirectSound** (`Win32_Media_Audio_DirectSound`; `DirectSoundCreate8`, `IDirectSoundBuffer8::Lock/Play/GetCurrentPosition` all present `[VERIFIED]` at `…/DirectSound/mod.rs:36,396,399,449`) | Yes; `dsound.dll` is inbox on **every** Windows incl. 7 and present in the Wine bottle `[VERIFIED]`, so even a load-time import is safe | same order as the game's own audio (it *is* the game's layer), and `-lowlatencysharedaudio` covers it `[VERIFIED]` | Placement into a looping buffer is exact, **but** the wall-clock anchor comes from `GetCurrentPosition`, whose accuracy/granularity is driver-dependent and documented as approximate `[RECALL]` | **Best fallback**; also the most "same speakers as the game" option |
| `waveOutWrite` (winmm — already loaded by the game `[VERIFIED]`) | Yes | Highest; MME sits on top of the same shared engine plus its own buffering `[RECALL]` | Queued `WAVEHDR`s are gapless, so the pre-roll trick works; the clock (`waveOutGetPosition`) is coarse `[RECALL]` | Only if both above fail |
| Pure-Rust crates (`cpal`, `rodio`, `kira`, `tinyaudio`) | `cpal` on Windows is `windows-sys`-based and header-free `[RECALL]`, but pulls a dependency tree and its own stream/callback thread model | shared WASAPI | callback-driven; you'd implement Design C inside its callback anyway | **Rejected** on dependency-weight grounds (project policy, AGENTS.md) — and they'd add a *second* audio abstraction over the same 30 lines of XAudio2 we need |
| Route through the **game's own XACT mixer** | n/a | best possible (same clock as the music) | in principle perfect | Out of scope for this doc; if achievable it dominates every option here and should be researched first |

---

## 6. Decode / asset pipeline

**Confirmed: no Ogg decoder dependency is needed.** Transcode offline and embed raw PCM.

`[VERIFIED]` The source asset:

```
$ ffprobe clap.ogg
Input #0, ogg: Duration: 00:00:00.21, bitrate: 400 kb/s
Stream #0:0: Audio: vorbis, 44100 Hz, mono, fltp, 239 kb/s
$ ls -l clap.ogg          → 10,704 bytes
```

`[VERIFIED]` The transcode and its exact size (run in this session, wrote nothing to disk):

```
$ ffmpeg -v error -i clap.ogg -f s16le -ac 1 -ar 44100 - | wc -c
   18846
```

**Size math checks out.** 18,846 bytes = 9,423 frames × 2 bytes = 0.21367 s at 44.1 kHz — matching
the 9,423 samples recorded in `rough-idea.md`. The prompt's "~19 KB" estimate is correct
(18.4 KiB). Negligible in a DLL of this size.

Proposed pipeline (mirrors the existing `scripts/build_ddr_package/` pattern of "offline asset
conversion + committed artifact"):

```bash
ffmpeg -v error -i clap.ogg -f s16le -acodec pcm_s16le -ac 1 -ar 44100 \
       assets/assist_tick_44100_mono_s16le.pcm     # 18,846 bytes, committed
```

```rust
static CLAP_PCM: &[u8] = include_bytes!("../../assets/assist_tick_44100_mono_s16le.pcm");
```

Format notes / gotchas:

* `[VERIFIED]` XAudio2 needs the `WAVEFORMATEX` of §5.3 plus `pAudioData`/`AudioBytes`. **A bare PCM
  blob is fine** — do not wrap it in RIFF, do not write a WAV parser.
* `[INFERENCE]` If we implement Design C (we mix ourselves) we must read the blob as `i16`.
  `include_bytes!` yields a `&[u8]` with **1-byte alignment**, so `as *const i16` is UB-adjacent.
  Either use `i16::from_le_bytes(...)` / `<[u8]>::align_to` at load time into a
  `Vec<i16>`/`Box<[i16]>` (one 18 KB allocation at enable time — fine), or embed as
  `static CLAP: [i16; 9423] = [...]` generated by a script. The `from_le_bytes` copy is simplest and
  endianness-explicit.
* `[INFERENCE]` Trim/normalise decisions worth making offline while we're already transcoding:
  the 213 ms tail is long relative to 16th-note spacing. With Design C's additive mixing the overlap
  is harmless, but a shorter (~80–120 ms) and slightly peak-limited clap will read cleaner under the
  music bed and reduce the chance of cumulative clipping. Keep the untrimmed blob as the source of
  truth.
* Licensing: the clap is StepMania's asset. `[RECALL]` StepMania is MIT-licensed, but its bundled
  media assets are not uniformly covered by that licence — worth a one-line check before shipping a
  binary blob in a public repo.

---

## 7. Failure modes and graceful-degradation plan

The mod must follow the house convention: **fail-open, self-disable, one clear log line, never crash
the game** (as `non_native_os_support` and the shader/perspective mods do — AGENTS.md).

| Failure | Detection | Response |
|---|---|---|
| No `XAudio2_9/8.dll`, or no `XAudio2Create` export (Win7, stripped image, odd Wine build) | `LoadLibrary`/`GetProcAddress` returns null | `log_warn!` once; try DirectSound fallback if implemented, else disable the option row (or grey it out) and never retry |
| `XAudio2Create` / `CreateMasteringVoice` / `CreateSourceVoice` returns an error HRESULT (endpoint busy, no default device, ASIO holding the card) | HRESULT | `log_warn!` with the HRESULT; disable for this boot |
| Engine created but **inaudible** (wrong endpoint, muted session, ASIO) | **Not detectable in-process** | Log the endpoint friendly name + mastering-voice channel/rate at init so an operator can diagnose from `log.txt`; document the `-audiohookdisable` / default-device checklist in README |
| Starvation click (frame hitch > queued lookahead) | `GetState().BuffersQueued == 0` on top-up | Re-anchor the QPC↔`SamplesPlayed` mapping, `log_debug!` a counter; keep 60–100 ms queued to make it rare |
| `OnCriticalError` / device removed mid-song (`IXAudio2EngineCallback::OnCriticalError` exists in the crate, `…/XAudio2/mod.rs:213`) | callback (needs a hand-rolled vtable — see below) or repeated submit failures | Tear down and disable for the rest of the session; do not attempt a hot re-open mid-song |
| Clap audible but consistently early/late | operator/player report | The offset knob is the answer; ship it from day one (scalar row, cabinet-wide, same shape as `timing_offsets`) |
| Two players on different charts | by design | `rough-idea.md` already resolves this: tick follows P1 |
| Score integrity | — | **Open question (§8).** An assist tick is a play aid; the repo has `services/score_guard.rs` for taint. Decide deliberately rather than by omission |

`[VERIFIED]` Implementation note on callbacks: `IXAudio2VoiceCallback` / `IXAudio2EngineCallback` in
XAudio2 are **not** `IUnknown`-derived (raw vtables). windows-rs models them as interfaces, but
`#[implement]` cannot produce a non-`IUnknown` COM object, so implementing them from Rust means
hand-building a `#[repr(C)]` vtable struct.
`[INFERENCE]` **Avoid callbacks entirely in v1** — `CreateSourceVoice`'s `pcallback` parameter is
`Option`-shaped (`P0: Param<IXAudio2VoiceCallback>`, verified at `…/XAudio2/mod.rs:160-164`), so
pass `None` and poll `GetState()` from the frame hook instead. That removes the whole class of
"game audio thread calls into Rust" hazards.

---

## 8. Open questions

1. **Can we ride the game's own XACT mixer instead?** (Highest-value follow-up.) If `audiokse` /
   XACT can be handed an extra cue, the tick inherits the music's exact clock and the entire §3/§4
   risk surface disappears. `[VERIFIED]` facts to start from: the engine is `xactengine2_10.dll`
   registered as COM (`log.txt:22-23`); the game builds `%s.xsb`/`%s.xwb` paths per song
   (`0x180363d40`/`0x180363d50`) and has `data/arc/soundbanks.arc` (`0x1802ddc20`); RTTI shows
   `audio::XsbFileCallback` / `audio::XwbFileCallback` and `sequence::AudioPlayer` /
   `sequence::AudioLoader`. LayeredFS could plausibly inject an extra wave into a bank. **Unknown:**
   whether a cue can be triggered at an arbitrary time with sub-frame precision, and whether the
   per-song banks are even writable by us.
2. **Where do arrow-hit timestamps come from, and with what precision?** Design C's sub-ms placement
   is wasted if the trigger is only observable per-frame. Does a song-time / sample-position value
   exist in the gameplay state that we can read (analogous to how `judge_hook` and the SSQ parsing
   already expose chart data)?
3. **Does the tick need to fire on *upcoming* arrows (schedule-ahead) or on judged arrows?** An
   assist tick must sound *at* the beat, i.e. it must be scheduled from the chart **before** the
   player steps — it cannot be driven by `judge_hook` (which fires when the player hits, or at the
   miss boundary). This likely means reading the parsed SSQ note list, not the judge path.
4. **Which OS does the target cabinet actually run?** `build_win7.sh` exists but no doc names a Win7
   deployment. If Win7 is a real target, DirectSound becomes the primary rather than the fallback.
5. **Does our XAudio2 stream reach the cabinet speakers?** Needs a one-off live test: play a tick in
   the attract loop and listen. Also worth logging what endpoint XACT itself selected (spice's
   `WrappedIMMDeviceEnumerator::GetDefaultAudioEndpoint: using {}` line already prints one).
6. **Does XAudio2 work under CrossOver/Wine (FAudio)?** Smoke test on the dev bottle; the bottle
   does ship 64-bit `xaudio2_9.dll` `[VERIFIED]`.
7. **Should assist tick taint score submission** (`services/score_guard.rs`)?
8. **Asset licensing** for redistributing StepMania's clap in this repo.

---

## Appendix — evidence index

| Claim | Source |
|---|---|
| `gamemdx` has no audio imports | Ghidra `list_imports` on `gamemdx_20260721.dll` (327 entries) |
| `ess.dll` is not the sound module | Ghidra `list_imports` on `ess.dll` |
| XACT strings & addresses | Ghidra `search_strings`: `0x1802de0a0`, `0x1802de3c8`, `0x1802ddc20`, `0x18035d2e0`, `0x18035d388/390`, `0x180363d40/50`, `0x180380088` |
| dsound dependency-closure name table | Ghidra `read_memory 0x180465e20` (ptr array) + `inspect_memory_content 0x1802de3a0` (strings) |
| `xactengine2_10.dll` uses DirectSound | `strings` on `…/contents/com/xactengine2_10.dll` → `DirectSound`, `DirectSoundEnumerateW` |
| XACT COM registration at boot | `…/ddr_world/contents/log.txt:22-23` |
| spice audio hook active | `…/log.txt:275` |
| Launch flags in use | `…/ddr_world/contents/gamestart-bemanibuddy.bat` |
| spice2x hooks DSound + WASAPI; option names | `strings` on `…/contents/spice64.exe` |
| DDR = DirectSound; multi-stream; `-lowlatencysharedaudio` | <https://github.com/spice2x/spice2x.github.io/wiki/Audio-modes-demystified> |
| Shared-mode latency numbers; small-buffer propagation to all apps | <https://learn.microsoft.com/en-us/windows-hardware/drivers/audio/low-latency-audio> |
| Operation sets are next-pass, relative-only | <https://learn.microsoft.com/en-us/windows/win32/xaudio2/xaudio2-operation-sets> |
| `GetState` / `SamplesPlayed` semantics | <https://learn.microsoft.com/en-us/windows/win32/api/xaudio2/nf-xaudio2-ixaudio2sourcevoice-getstate> |
| XAudio2 version availability; flat `XAudio2Create` since 2.8 | <https://learn.microsoft.com/en-us/windows/win32/xaudio2/xaudio2-versions> |
| Source-voice graph basics | <https://learn.microsoft.com/en-us/windows/win32/xaudio2/how-to--build-a-basic-audio-processing-graph> |
| Crate feature names | `windows-0.58.0/Cargo.toml:479-483` |
| Only `XAudio2CreateWithVersionInfo`, linked to `xaudio2_8.dll` | `windows-0.58.0/src/Windows/Win32/Media/Audio/XAudio2/mod.rs:25-27` |
| `link!` → raw-dylib or umbrella lib | `windows-targets-0.52.6/src/lib.rs:8-45`; `windows_x86_64_msvc-0.52.6/lib/windows.0.52.0.lib` |
| XAudio2 constants/structs | `…/XAudio2/mod.rs:658,666,667,696,705-706,711`; `XAUDIO2_BUFFER`, `XAUDIO2_VOICE_STATE` defs |
| `WAVEFORMATEX`, `WAVE_FORMAT_PCM` | `…/Media/Audio/mod.rs:6745-6753`, `:4119` |
| DirectSound bindings present | `…/Media/Audio/DirectSound/mod.rs:36,396,399,449` |
| Win7 build path exists | `build_win7.sh:1-14` |
| Existing sound-offset precedent | `README.md:197`; AGENTS.md (`timing_offsets`) |
| Asset facts + PCM size | `ffprobe`/`ffmpeg` on `clap.ogg`; `rough-idea.md` |
