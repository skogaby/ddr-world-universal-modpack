# Research — StepManiaX SDK (cabinet-IO fork) integration

Source: `stepmaniax-sdk` (`github.com/skogaby/stepmaniax-sdk`), fork HEAD `5a47908`.
All SDK C++ in `sdk/Windows/`; single public header `sdk/SMX.h`. Builds `SMX.dll`.

## Public C API (all `extern "C"`, cdecl, undecorated x64 names)

Used by SpiceManiaX / needed for our port:
- `void SMX_Start(SMXUpdateCallback, void* pUser)` / `void SMX_Stop()`
- `void SMX_SetLogCallback(SMXLogCallback)`
- `uint16_t SMX_GetInputState(int pad)` — 9-bit panel bitmask (see below)
- `void SMX_SetLights2(const char* data, int size)` — stage panels
- `void SMX_SetDedicatedCabinetLights(SMXDedicatedCabinetLights dev, const char* data, int size)` — **fork addition**
- plus `SMX_GetInfo/GetConfig/SetConfig/SetPanelTestMode/Version/...` (not needed for the bridge)

Enums: `SMXDedicatedCabinetLights { MARQUEE=0, LEFT_STRIP=1, LEFT_SPOTLIGHTS=2, RIGHT_STRIP=3, RIGHT_SPOTLIGHTS=4 }`;
`SMXUpdateCallbackReason { Updated, FactoryResetCommandComplete }`.
Callbacks: `typedef void SMXUpdateCallback(int pad, SMXUpdateCallbackReason, void*)`,
`typedef void SMXLogCallback(const char*)`.

**Lifetime hazard:** only `SMX_Start`/`SMX_Stop` null-check the singleton; every other
export dereferences it unconditionally → call nothing before `SMX_Start` / after `SMX_Stop`.
`SMX_Stop` must NOT be called from inside the update callback.

## Input model
`SMX_GetInputState(pad)` → `uint16_t`, bits 0–8 = 9 panels (reading order
`012/345/678`): bit1=Up, bit3=Left, bit4=Center, bit5=Right, bit7=Down (corners 0/2/6/8).
Event-driven: I/O thread wakes on HID interrupt; `SMXUpdateCallback` fires on change
(from a dedicated user-callback thread — receiver must be thread-safe; callback says
*something* changed, re-read what you care about). Cabinet device (pad 2) input = 0.

## Lights formats (byte layouts the SDK expects)
- **`SMX_SetLights2`** stage: **1350 bytes** = 2 pads × 9 panels × 25 LEDs × 3 (RGB).
  Order: `[pad0][pad1]`, 9 panels reading-order, per panel 16 outer (4×4) then 9 inner (3×3).
  (864-byte legacy = 16-LED-only also accepted.)
- **`SMX_SetDedicatedCabinetLights`** input sizes: MARQUEE 24×3=72, L/R STRIP 28×3=84,
  L/R SPOTLIGHTS 8×3=24. SDK reorders channels / reverses / zero-pads to the wire
  format per detected lights-controller model (`"I"` handshake). Client always sends RGB.
  (Marquee "24 vs 12": 12 was a reverted experiment; current API count is 24 logical.)

These sizes exactly match `SpiceManiaX/lights_utils.*` (kSmxArrowLedCount=25,
kSmxMarqueeLogicalLedCount=24, kSmxVerticalStripLedCount=28, spotlight=8).

## Transport / threads
Raw Windows HID via `setupapi` + `hid.dll` (VID `0x2341`, PID `0x8037`, product-string
filter `"StepManiaX"`=stage / `"SMXArcade"`=cabinet). Overlapped `ReadFile`/`WriteFile`,
64-byte HID reports (report id 3=input, 5=host→dev, 6=dev→host). **Three SDK-owned
threads** (I/O + user-callback, both `THREAD_PRIORITY_HIGHEST`; + 250 ms device-search).
Global singleton `SMXManager::g_pSMX`. **No COM, no window, no message loop** → safe to
embed; does not need the game's UI thread. Default logging is `printf` — set a log
callback before `SMX_Start`. Internal C++ exceptions only on API misuse/crypto failure;
public C API never propagates them across the boundary. Links `hid.lib setupapi.lib
advapi32.lib` (rest of the vcxproj libs are VS boilerplate, unused).

## Build
`sdk/Windows/SMX.vcxproj` → `DynamicLibrary`, v143, **Win32-only configs checked in**
(x64 was added then reverted). C++14 (no C++17 features used). Pre-build `.bat` writes a
git-ignored `SMXBuildVersion.h`. **No prebuilt `SMX.dll` in the repo — must be built.**
~10 core `.cpp` (+3 for GIF animation, reachable only via `SMX_Stop`'s
`SMX_LightsAnimation_SetAuto(false)` and the unused `SMX_LightsAnimation_*` exports).

## Integration options (Rust MSVC cdylib, cargo-xwin)

- **(A) `LoadLibrary("SMX.dll")` + `GetProcAddress`** — exactly SpiceManiaX's
  `smx/smx_wrapper.cpp` pattern. Cleanest C-ABI seam; SDK's threads/exceptions/`printf`
  stay behind a module boundary; swap DLL without rebuilding. **Cost:** must build an
  **x64** `SMX.dll` once and ship it beside the modpack DLL; hand-write ~10 extern
  signatures. **Effort: low–moderate. Lowest risk.** Fits repo's "optional service +
  `is_available()` + graceful degradation."
- **(B) Static-link the SDK `.cpp` via `cc`/`build.rs`** — one self-contained artifact,
  no extra DLL. **Cost:** pulls the MSVC C++ runtime/STL into the cdylib; must generate
  `SMXBuildVersion.h` from `build.rs`; clang-cl warning wrangling; SDK C++ exceptions now
  share the cdylib's unwind. **Effort: moderate.**
- **(C) Full Rust port of the transport** — no C++ at all; only port input + stage +
  cabinet lights (drop GIF/config/sensor-test). **Cost:** faithfully reproduce HID
  framing + the model-dependent cabinet wire format + stage light pacing (30 FPS,
  ×0.6666 scale, V3/V4 timing) — the subtle bits recent fork commits fixed; hardware-gated
  validation. **Effort: high, highest protocol risk.**

**Leaning A** (matches the proven consumer, lowest risk, keeps the SDK's HIGHEST-priority
threads + C++ runtime isolated) unless a single-artifact deploy is a hard requirement.
