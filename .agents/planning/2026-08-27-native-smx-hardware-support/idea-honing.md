# Idea Honing — Native SMX Hardware Support (decision register)

Status: **D1–D5 accepted (2026-08-27); D6–D11 recorded as assumptions.** Research
U1/U2/U3 closed (see `research/ark-mdx-io-layer.md`). Ordered by blast radius.

Readiness Confirmed 2026-08-27 — requirements settled (D1–D5 accepted, D6–D11
assumptions), research U1/U2/U3 closed via Ghidra (`research/ark-mdx-io-layer.md`) +
the SDK/spice2x notes. Cleared to write the detailed design. Open design-time items
(non-blocking): U5 touch capture (locate game HWND), U7 timing (cabinet-measure),
exact `arkMDXChangeTapeled` param decode (design/impl-time Ghidra confirmation).

Ratification log:
- 2026-08-27: D12 accepted — **no host-test harness**; validation is on-device/manual
  (near-verbatim port of mature SpiceManiaX + SDK code; a host harness would churn
  more cycles than it returns, and matches the repo norm of live-cabinet validation).
  Design amended + re-dated accordingly; plan's per-step Tests reframed as on-device
  validation.
- 2026-08-27: D1 accepted (spice2x/bemanitools stays loader+IO-emulator; only
  SpiceManiaX+SpiceAPI is replaced; mod is loader-agnostic — hooks the game's own
  `arkMDX*` layer). D2 accepted as **Option C (full Rust port)** — macOS-compileable,
  single artifact, no C++ toolchain. D3 accepted (hook the `arkMDX*` light-out
  layer; U1 confirmed). D4 accepted (fully native render, no window, exclusive-
  fullscreen-correct; touch via game-HWND subclass). D5 accepted.

| ID | Decision | Why it matters | Recommendation | Status |
|----|----------|----------------|----------------|--------|
| D1 | Meaning of "spice2x not a strict requirement" | Determines whether this is a mid-size port or a reimplementation of spice2x's whole DDR IO/DRM/board-emulation stack | Scope = drop **SpiceManiaX + SpiceAPI** only; spice2x (or bemanitools) stays the loader/IO-emulator (`-k`). Mod hooks the game's own `arkMDX*` layer ⇒ loader-agnostic. | **Accepted** |
| D2 | SMX SDK integration method | C++ SDK vs Rust cdylib boundary; deploy shape; risk; **macOS-buildable** | **(C) Full Rust port** of the used SDK subset (input + stage lights + cabinet lights + connect) via `windows`-crate raw HID. No `SMX.dll`, no C++ toolchain. Pure wire/mapping layers host-tested. | **Accepted** |
| D3 | Lights-READ hook point | Where we capture the game's Gold-Cab light output | Detour the `arkMDX*` light-OUT exports (above spice2x): `arkMDXChangeTapeled` (per-LED RGB) + `arkMDXSetLamp`/`ChangeDimlamp`/`ChangeSatellite`. Confirmed. | **Accepted** |
| D4 | Touch overlay rendering + touch capture | Biggest net-new surface; no precedent in codebase | Render natively through the modpack overlay path (no extra window; exclusive-fullscreen-correct) + capture touch by subclassing the game HWND (`RegisterTouchWindow`/WM_TOUCH). | **Accepted** |
| D5 | Input INJECTION seam | Reuse vs new machinery | Extend `input_manager` to detour `arkMDXGetPanel{Up,Down,Left,Right}` (arrows) + `arkMDXGetEAPass` (card) and drive all getters (panels/menu/keypad/card) from SMX state for game-side callers, reusing the existing suppression/`IN_MODPACK_POLL` machinery (made additive). | **Accepted** |
| D6 | Lights mapping fidelity | Correctness of the DDR→SMX light map | Port `SpiceManiaX/lights_utils.cpp` verbatim (arrow 25:1, corner L-shapes, marquee 40→24 conflict-avg, strips 25→28, spotlights, static-gold fills) | Assumed |
| D7 | Input timing / threading | Latency vs the current 1000 Hz media-timer model | SDK event-driven callback caches the 9-bit panel mask; inject at the getter layer (latency = game's own read cadence). Measure; add a dedicated input thread only if needed | Assumed |
| D8 | `SMX.dll` build + deploy | Fork vcxproj is Win32-only; cabinet needs x64 | Build an x64 `SMX.dll` from the fork; ship it beside the modpack DLL via `scripts/deploy.sh` | Proposed (follows D2=A) |
| D9 | Mod identity / config / default | Config schema + boot default | id `smx-hardware`; default **OFF** (hardware-specific); `smx_hardware` config: `p1card`/`p2card`, `overlay_opacity`, mapping toggles | Assumed |
| D10 | Cabinet-model + Gold-Cab scope | What hardware/game mode is supported | Match SpiceManiaX: SMX **Dedicated Cabinet** only; DDR in **Gold Cabinet (BIO2)** mode (already the modpack's `arkmdxbio2` target) | Assumed |
| D11 | Coexistence with spice2x's own SMX mapping | Double-driving the hardware | Use this mod INSTEAD of spice2x `smxdedicab`/spicecfg SMX light mapping; document as a usage constraint | Assumed |

## Detail / rationale

### D1 — spice2x dependency scope (HIGHEST blast radius; likely under-considered)
Two readings:
- **(a) Recommended:** the mod replaces the **SpiceManiaX process + the SpiceAPI TCP
  hop** with in-process IO, and drives SMX natively. spice2x remains the loader
  (`-k`) and the DDR IO/board/DRM/network emulator. The concrete goal — "bypass
  spiceapi for top-tier performance" — is fully met, and the mod stays a bounded port.
- **(b) Not recommended now:** truly remove spice2x → the modpack must emulate the
  BIO2/MDXF board + ICCA card reader + eamuse + network/DRM the game needs to boot.
  That is a large fraction of spice2x re-implemented in Rust; high risk; out of scope
  for a first version. Could be a later phase if desired.

### D2 — SDK integration (A/B/C)
A keeps the SDK's HIGHEST-priority HID threads and C++ runtime/exceptions behind a
clean C boundary (repo rule: no exceptions across FFI), matches the proven
SpiceManiaX/C# consumer, and lets the SDK be an optional service. B (static-link)
gives one artifact but drags the MSVC C++ runtime into the cdylib. C (Rust port) is
highest-effort/highest-protocol-risk. Details: `research/smx-sdk.md`.

### D3 — lights-read (research-gated)
Preferred seam is the `arkmdxbio2` light-output exports, mirroring how
`input_manager` already hooks `arkMDXGet*` *above* spice2x — this avoids double-hooking
the `libacio ac_io_*` functions spice2x owns (repo rule: one detour per target).
Confirming that export (U1) is the first research task.

### D4 — overlay + touch (close call)
Leaning native-render because everything else in the modpack renders through the
game's own pipeline, it composites correctly even if the game ever runs exclusive
fullscreen, and it avoids z-order/focus fights a separate topmost window can have.
The cost is net-new **touch capture** on the game window (no precedent). The
lower-effort alternative is porting SpiceManiaX's self-contained transparent D2D
window 1:1 (proven, but a new window pattern + assumes borderless-windowed game).
Worth an explicit call.

### D7 — timing
The getter-injection latency is bounded by *when the game reads input*, not by a
poll rate, so render-thread injection should match or beat the 1000 Hz SpiceAPI push
in practice. Confirm on hardware (U7).

## Research items opened (see orientation.md)
U1 lights-out export · U2 card-in path · U3 arrow-panel getter · U4 = D1 ·
U5 touch capture · U6 SMX.dll x64 build · U7 timing.
