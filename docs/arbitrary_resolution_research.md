# Arbitrary Render Resolution — Feasibility Research

**Status:** research only (2026-09-02). No code. Primary build **20260616** (all
`FUN_`/`DAT_` names below are that build's Ghidra names, file-relative to
`0x180000000`); every load-bearing site was AOB-verified on **20260825** and
**20250805**, the back-buffer site also on **20260721** (§9). No live process was
available, so nothing here has runtime confirmation — every claim is static
disassembly plus the project's prior live observations; the explicit hypotheses
are collected in §10.

**Question asked:** what would it take to make DDR World render natively at
1080p / 4K (16:9 only) instead of its fixed 1280×720, and how much of that is
engine work vs. asset work?

**Short answer:** far less engine work than expected, because the AGCS renderer
already separates a *logical* 1280×720 canvas from the *physical* render
target — the canvas→NDC conversion divides by the canvas size, not the RT size
(§4.1), so almost all 2D and AFP content is already resolution-independent.
What is hard-coded is (a) the D3D9 back-buffer size (§2), (b) the sizes of the
offscreen surfaces and per-list viewports (§3), (c) the letterbox source rect
(§5.2) and (d) the scissor path, which copies canvas pixels straight into
`SetScissorRect` (§4.3). All of it is boot-time init or a single small walker
handler, all of it byte-stable across the four builds. The game even contains a
finished, tested "screen ≠ 1280×720" present path (letterboxed `StretchRect`,
§5) because SD cabinets run the same binary at 640×480.

The real cost is on the **asset** side: every bitmap is authored for 720p and
will be bilinear-upsampled (1.5× at 1080p — the awkward ratio; exactly 3× at 4K),
and a handful of engine constants (`sys_copy`-style present shaders, the arrow
AA shader's texel constants) assume 1:1 texel/pixel. §11 gives a tiered
estimate; §14 works through the hi-res asset pipeline (offline-vectorised art
rasterised in the DLL at an integer density factor and served through
LayeredFS) and the four engine sites that read texture pixel dimensions.

---

## Contents

1. Prior knowledge in this repo (what was already known)
2. Device / swap-chain path — where the back-buffer size comes from
3. Render-surface graph — the eleven surfaces, eight lists, six viewports
4. The 2D command-list pipeline is canvas-relative (and the one place it isn't)
5. The present chain — letterbox `StretchRect` + full-screen copy
6. The AFP/BM2D path — projection callback, render context, layer roots
7. Screen-pixel consumers that are NOT canvas-relative (audit of `DAT_1806f20d8`)
8. Key addresses + struct layouts
9. Cross-version verification (AOBs)
10. Open questions / hypotheses needing a live probe
11. What a mod would have to do — tiered effort estimate
12. Modpack-side impact inventory
13. Gotchas
14. Vector-source assets: rasterise in the DLL at the target scale (Tier C detail)

---

## 1. Prior knowledge in this repo

Nothing in `docs/` had touched the device/present layer. Relevant prior facts
(all re-confirmed here):

- `docs/overlay_draw_research.md` §"New layout facts": tag 0x07 (2D context)
  handler `FUN_180268c40`, payload `{f32 canvas_w, canvas_h, offset_x,
  offset_y}`, "rt dims at walker+0x144/+0x146". The game's own layer walk begins
  with a `07:14` record with canvas 1280.0 (live-observed).
- `docs/custom_arrow_renderer_research.md` §"Vertex format": CPU-side
  `ndc = (v*ctx.scale + ctx.offset)*{2,-2,1} + {-1,1,0}`; the stock 2D VS is a
  position passthrough (`mov o0,(v0.x,v0.y,0,1)`); tag map 0x00–0x1A.
- `docs/custom_shader_backgrounds_research.md` §5: the 8 named lists from
  `FUN_1801f5d10` (FRONT/MIDDLE/BACK 1280×720, SYSTEM = screen dims, OFFSCREEN0
  1280×720, OFFSCREEN1 1280×1280, DEBUG_DIALOG = screen dims, RENDER_CAPTURE
  1280×720) and the viewport graph `0x65 OFFSCREEN1 → 0x66 RENDER → 0x67 AFTER
  RENDER 3D → 0x68 RENDER_2D → 0x69 DISPLAY → 0x6a PRESENT`.
- `.agents/planning/20260627-fps-unlock/research/`: `Application::onBoot`
  builds a display struct on its stack; `+0x1C` = FPS target (patched by
  `fps_unlock`), `+0x14/+0x18` unidentified. Identified below (§2.1).
- `docs/playfield_styling_research.md`: the `720.0f` rodata constant
  `DAT_18038eb38` is shared by 14 functions and is the *logical* cull height —
  it must never be changed for a resolution mod (it defines the canvas, §4.1).

---

## 2. Device / swap-chain path

### 2.1 `Application::onBoot` → display struct

`FUN_1800020a0` (onBoot, 20260616; the `"Application::onBoot() end."` string
xref) builds a 0x20-byte display struct at `[RSP+0x50]` (`local_278`) and passes
it to `FUN_1801f1cf0` (graphics init). Field map, from the onBoot stores plus
the consumer `FUN_1801ef6d0`:

| off | value in onBoot | consumer | meaning |
|---|---|---|---|
| +0x00 | `WindowInfo` vfunc(+8) result | — | (window info) |
| +0x08 | `WindowInfo` vfunc(+0x10) result | `DAT_1806f0518` | **HWND** |
| +0x10 | `1` | `FUN_1801ef6d0` branch | request fullscreen (1) / windowed (0) |
| +0x11 | `0` | `FUN_1801ef6d0` R13B | allow fallback to a non-matching display mode |
| +0x12 | `machineType ∉ {0,1}` | `FUN_1801ef6d0` | **HD flag: 1 → 1280×720, 0 → 640×480** |
| +0x14 | `0` | `DAT_1806f0510` | extra `FUN_18024de80` calls at frame begin (count) |
| +0x18 | `0`, or `3` when `1 < pcType < 5` and HD | `DAT_1806f050c` | **AA config** 0/1/2/3 (§3.2) |
| +0x1C | `0x3C`, or `0x4B` when machineType == 1 | `DAT_1806f0508` | FPS / refresh target (the `fps_unlock` imm32) |

`(*DAT_1806f1330)` is the ark "get machine type" call (used twice), `(*DAT_1806f1338)` the "get PC type" call.

### 2.2 `FUN_1801ef6d0` — back-buffer size and display-mode selection

```
1801ef6ed  CMP  byte ptr [RCX+0x12],0          ; HD flag
1801ef6f6  MOV  dword ptr [DAT_1806f0524],0x500 ; screen_w = 1280
1801ef700  MOV  dword ptr [DAT_1806f0520],0x2d0 ; screen_h = 720
1801ef70c  MOV  dword ptr [DAT_1806f0524],0x280 ; else 640
1801ef716  MOV  dword ptr [DAT_1806f0520],0x1e0 ;      480
1801ef723  MOV  dword ptr [DAT_1806f0514],1     ; BackBufferCount
```

`DAT_1806f0524` / `DAT_1806f0520` = **the screen (back-buffer) width/height**.
They are the only origin of the physical resolution; everything downstream
reads them (or the per-display copy `DAT_1806f20d8[0..1]`, §2.4).

Fullscreen branch (`+0x10 != 0`): `Direct3DCreate9(0x20)` →
`GetAdapterModeCount(0, D3DFMT_X8R8G8B8)` (vtbl +0x30) → loop
`EnumAdapterModes(0, 0x16, i)` (vtbl +0x38) looking for an **exact**
`Width == screen_w && Height == screen_h` mode. Found → `DAT_1806f1d88 = 1`
(fullscreen). Not found → if `+0x11` (fallback allowed) the LAST enumerated
mode's W/H are written into `DAT_1806f0524/0520` and fullscreen is kept,
otherwise `DAT_1806f1d88 = 0` (windowed). Then `SetWindowLongW(hwnd, GWL_STYLE,
WS_POPUP|WS_VISIBLE)` and the window is sized to the desktop rect. The windowed
branch (`+0x10 == 0`) sizes the client area to `screen_w × screen_h` with
`SetWindowPos`.

**Consequence:** a 1920×1080 request is honoured verbatim as long as the
monitor exposes that mode (any 1080p/4K panel does).

### 2.3 `FUN_1801ef9e0` ("Renderer:initGs()") — the display descriptor

Builds a 0x30-byte display descriptor at `[RSP+0x20]` and a device-init header,
then calls `FUN_18026a7a0` (gd init → `FUN_1802473b0`):

| off | value | → `D3DPRESENT_PARAMETERS` field (via `FUN_180247c80`) |
|---|---|---|
| +0x00 | `DAT_1806f0524` | `BackBufferWidth` |
| +0x04 | `DAT_1806f0520` | `BackBufferHeight` |
| +0x08 | `0x16` | `BackBufferFormat` = `D3DFMT_X8R8G8B8` |
| +0x0C | `DAT_1806f0514` (1) | `BackBufferCount` |
| +0x10/+0x14 | `0, 0` | `MultiSampleType/Quality` |
| +0x18 | `fullscreen ? 2 : 1` | `SwapEffect` = FLIP / DISCARD |
| +0x1C | `0` | `EnableAutoDepthStencil` = FALSE (engine owns depth surfaces) |
| +0x20 | `0x50` | `AutoDepthStencilFormat` = `D3DFMT_D16` (unused) |
| +0x24 | `1` | `Flags` = `D3DPRESENTFLAG_LOCKABLE_BACKBUFFER` |
| +0x28 | `DAT_1806f0508` | `FullScreen_RefreshRateInHz` (fullscreen only) |
| +0x2C | `1` | `PresentationInterval` = `D3DPRESENT_INTERVAL_ONE` |

Header: `[RBP-0x40]=0`, `[RBP-0x3c]=1` (display count), `[RBP-0x38]=&desc`,
`[RBP-0x30]=&DAT_1806f0518` (HWND ptr), `[RBP-0x28]=DAT_1806f1d88`.

### 2.4 `FUN_1802473b0` — per-display info array

`DAT_1806f20c4` = display count (1); `DAT_1806f20d8` = `alloc(count << 6)` —
**0x40-stride per-display info**, first 0x30 bytes copied from the descriptor,
so `DAT_1806f20d8[0] = screen_w`, `[1] = screen_h`, `+0x30` = swap-chain ptr,
`+0x38` = back-buffer surface handle (filled by the swap-chain setup; read each
frame by `FUN_1801f24d0`, §5.3). ~40 readers across the binary (§7).

### 2.5 `FUN_18024aed0` — `IDirect3D9::CreateDevice`

`FUN_180247c80(pp, idx)` fills `D3DPRESENT_PARAMETERS` from the display info
(`Windowed = (DAT_1806f2208 == 0)`, `hDeviceWindow = DAT_1806f2200[idx]`), then
`CreateDevice(adapter=DAT_18047bf38, D3DDEVTYPE_HAL, hFocus=DAT_18047bf40,
flags=0x44 /*HW_VP|MULTITHREADED*/ (+0x200 ADAPTERGROUP when >1 display, |2
FPU_PRESERVE when desc+0x1c), pp, &DAT_1806f2110)`; on failure retries with
`0x20` (SW_VP) and then `D3DDEVTYPE_REF`. Device lost (`0x88760868/69`) is
handled in the gd executor's `Present` case (`FUN_18024c310` case 4 → sets
`DAT_1806f2118 = 1`, `DAT_1804607dc = 3`) — a Reset path exists but is
untested territory for this mod.

---

## 3. Render-surface graph

### 3.1 `FUN_1801f01a0` — surfaces, RT structs, viewports

Constructor of the 0x170-byte render-surface object `DAT_1806f1ef0`. Surfaces via
`FUN_18024f610(w, h, fmt[, msaa])`; the compiler hoisted **`R15D = 0x500` at
`1801f02b6` (`41 BF 00 05 00 00`) and `ESI = 0x2d0` at `1801f02da`
(`BE D0 02 00 00`)** and reuses them for every 1280/720 argument below.

| field | surface | notes |
|---|---|---|
| +0xdc | `(1280, 1280, 0x15)` | OFFSCREEN1 (`R15D,R15D`) |
| +0xb0 (`[0x16]`) | `(1280, 720, 0x4b)` | depth A (D24S8) |
| +0xb4 | `(1280, 720, 0x16)` | colour |
| +0xb8 (`[0x17]`) | `(1280, 720, 0x4b)` | depth B |
| +0xc0 (`[0x18]`) | `(1280, 720, 0x16)` | **`render_color`** — the 2D composite target |
| +0xc8 (`[0x19]`) | `(1280, 720, 0x4b)` | **`render_depth`** |
| +0xc4 | `(1280, 720, 0x16, msaa)` | RENDER (3D) colour, multisampled per AA config |
| +0xcc | `(1280, 720, 0x4b, msaa)` | RENDER depth, multisampled |
| +0xd0 (`[0x1a]`) | `(1280, 720, 0x16)` | resolve target / `render_back` when AA on |
| +0xd4 | `(1280, 720, 'D24R')` | readable-depth fourcc |
| **+0xec** | **`(DAT_1806f0524, DAT_1806f0520, 0x16)`** | **`display` — the only screen-sized surface** |

Texture views (`FUN_180249ba0`): +0x10c ← OFFSCREEN1 (registered by name
`"OFFSCREEN1"`), +0xf4 ← 'D24R', +0xf8 ← render_color, +0xfc/+0x100 ←
`"render_back"` (which physical surface depends on AA), **+0x104 ← display**.
Names `"display"`, `"render_color"`, `"render_depth"` are bound into the render
graph binder `DAT_1806f1f00` via `FUN_1801f34c0`.

**RT structs** (0x1c bytes, `{+8 colour id, +0x10 depth id, +0x14 u16 w, +0x16 u16 h, +0x18 u8 msaa}`) — dims are inline immediates:

| field | dims (imm) | colour / depth |
|---|---|---|
| `+0x80` `[0x10]` | `0x500` / `0x2d0` (u16 + u32 write) | **PRESENT** target: render_color (or display in AA-3) / render_depth. **Retargeted to the BACK-BUFFER every frame** (§5.3) but keeps these 1280×720 dims |
| `+0x90` `[0x12]` | `0x05000500` | OFFSCREEN1 |
| `+0x60` `[0xc]` | `0x02d00500` | render_color / 'D24R' (or display / render_depth in AA-3) |
| `+0x68` `[0xd]` | `0x500` / `0x2d0` | (no colour) / 'D24R' |
| `+0x58` `[0xb]` | `0x02d00500`, msaa | RENDER: +0xc4 / +0xcc (or display / render_depth in AA-3) |
| `+0x70` `[0xe]` | `0x02d00500` | RENDER_2D: render_color / render_depth |
| `+0x78` `[0xf]` | **`(short)DAT_1806f0524 / (short)DAT_1806f0520`** | **DISPLAY: display surface** (already screen-sized) |

The present quad vertex buffer `+0x124` (4 verts) is built with a half-pixel
offset computed from `DAT_1806f20d8` (screen dims) — already resolution-aware.

Viewports (`gs::Viewport`, 0x50) and their targets: `[1]` OFFSCREEN1 → rt[0x12];
`[5]` RENDER → rt[0xb]; `[6]` AFTER RENDER 3D → rt[0xe] (or [0xf] in AA-3);
`[7]` RENDER_2D → rt[0xe]; `[8]` DISPLAY → rt[0xf]; `[9]` PRESENT → rt[0x10].

### 3.2 AA config `DAT_1806f050c`

0 = none, 1 = 2× MSAA, 2 = 4× MSAA on the RENDER surfaces (`render_back` becomes
the resolve copy +0xd0), **3 = "direct" mode**: RENDER, AFTER RENDER 3D and the
PRESENT struct target the screen-sized `display` surface directly and the
COPYVIEWPORT blit is skipped (`FUN_1801f44d0` early-outs). Mode 3 is chosen
for `1 < pcType < 5` on HD cabinets (§2.1) — i.e. a performance shortcut that
only makes sense when `screen == 1280×720` (its depth buffer stays 720p). A
resolution mod must handle both `AA ∈ {0,1,2}` and `AA == 3` cabinets; forcing
`DAT_1806f050c = 0` at boot (write it before `FUN_1801f1cf0` runs) removes the
special case and any MSAA cost at 4K.

### 3.3 `FUN_1801f5d10` — the eight `ScreenCommandList` viewports

Stack table of `{name, w, h}` (imm32 writes at `[RBP+0x338..0x3ac]`):
FRONT/MIDDLE/BACK/OFFSCREEN0/RENDER_CAPTURE = `0x500 × 0x2d0`, OFFSCREEN1 =
`0x500 × 0x500`, **SYSTEM and DEBUG_DIALOG = `DAT_1806f20d8[0..1]` (screen
dims)**. Each viewport gets `+0x8 x=0, +0xc y=0, +0x10 w, +0x14 h, +0x18 minZ=0,
+0x1c maxZ=1.0`, identity view/proj (`DAT_18047bd60`), the list arena
`FUN_18026a720(0x400000)` and a fresh RT id `FUN_180251410(&DAT_1802e0990,3)`
(`DAT_1806f226c` keeps the last = default). The lists have **no own target**
(`vp+0xb8 = 0`) — they render into whatever RT their parent viewport bound.

Attachments (`FUN_1802662a0(parent, child, prio)` in `FUN_1801f1cf0`):
RENDER_2D ← BACK@0x65, MIDDLE@0x66, FRONT@0x67; RENDER ← OFFSCREEN0@0x65 (+ the
3D `Packet` scene viewports, `FUN_1801f5880`, which inherit dims); OFFSCREEN1 ←
list 5; **DISPLAY ← COPYVIEWPORT@0x65 then SYSTEM@0x66** — the SYSTEM list is
drawn *after* the upscale blit, directly into the screen-sized display surface.
DEBUG_DIALOG / RENDER_CAPTURE are not attached at init (no static consumer
found — §10).

### 3.4 Segment header — how a viewport becomes `SetViewport` + shader constants

`FUN_18026c9f0(walker, vp+8, …)` → `FUN_18026c4d0(walker, x, y, w, h, minZ,
maxZ)`: stores `walker+0x140..+0x146 = {x,y,w,h}` (u16), emits gd `0x17`
(→ `IDirect3DDevice9::SetViewport`, device vtbl +0x178), then
`FUN_18026c440` uploads `{x,y,w,h}` as floats to **VS c13 and PS c1**. So every
shader already receives the real viewport rect; the walker's `+0x144/+0x146`
(read by the tag-0x07 handler) are the *viewport* dims, not the surface's.

---

## 4. The 2D command-list pipeline is canvas-relative

### 4.1 Tag 0x07 handler `FUN_180268c40` — the decisive math

```
180268ca3  MOVSS XMM6,[1.0]            ; DIVSS XMM6,[RDI+8]  → 1/canvas_h
180268cb3  MOVSS XMM5,[1.0]            ; DIVSS XMM5,[RDI+4]  → 1/canvas_w
180268cbb  MOVZX EAX,word [RCX+0x146]  ; rt_h  (RCX = walker+8 object)
180268cc6  MOVZX EAX,word [RCX+0x144]  ; rt_w
180268cdf  DIVSS XMM2,XMM0             ; offset_y / rt_h
180268ce8  DIVSS XMM0,XMM1             ; offset_x / rt_w
180268d11  MOVAPS [ctx+0x00],{ox/rt_w, oy/rt_h, 0, 0}
180268d22  MOVAPS [ctx+0x10],{1/canvas_w, 1/canvas_h, 1, 0}
```

and every draw handler (0x01/0x02/0x03/0x04/0x05/0x06) converts with

```
ndc.x = (x * ctx.scale.x + ctx.offset.x) * 2 - 1        (FUN_1802689b0, tag 0x05)
      = (x / canvas_w + offset_x / rt_w) * 2 - 1
```

**The RT/viewport size only enters through the offset term.** A 1280-unit
canvas therefore always spans the full viewport whatever its pixel size. This
is why all screen-command content (arrows, guidelines, HUD quads, the modpack's
`overlay_draw`/SMX quads, theme backgrounds) is resolution-independent for free.
The stock 2D VS is a passthrough (`shader_replacement_research.md` §5), so the
GPU rasterises the same NDC geometry at whatever pixel density the viewport has.

### 4.2 Who emits the canvas

- `agcs::ScreenRoot::render` = `FUN_1802178d0` (vtable `0x1803887c8` slot 5):
  `canvas = (root+0x50 / scale_acc_x, root+0x54 / scale_acc_y)`,
  `offset = (acc_x, acc_y)`; `DAT_180cf2a54 = root+0x50 / 1280.0f`,
  `DAT_180cf2a50 = root+0x54 / 720.0f` (logical normalisation).
- `agcs::ScreenRoot` ctor `FUN_180217620`: `+0x50 = 1280.0f, +0x54 = 720.0f,
  +0x58/+0x5c = 1.0` (vtable slot 1 = `FUN_1802175f0(w,h)` set size).
- `FUN_180215410` / `FUN_180215480` reset the walk defaults
  `DAT_18046091c = 1280.0f`, `DAT_180464108 = 720.0f` (from `DAT_18038eb34/38`)
  before rendering the six stock roots `DAT_1806f07a8[0..5]`.

Keep all of these as they are — they define the *logical* space.

### 4.3 The exception: scissor (tag 0x0C) is copied raw

`FUN_180269080` (tag 0x0C) writes gd `0x18` as
`{u16 x, y, w, h}` **verbatim from the record** (`180269120..180269150`), and
the gd executor `FUN_18024c310` case `0x18` builds `RECT{x, y, x+w, y+h}` and
calls `SetScissorRect` (device vtbl +0x258) — **render-target pixels, no
scaling.** `ScreenRoot::render` emits scissors in canvas px
(`(short)(w*sx*w/1280)` etc.). Today canvas == RT so it is exact
(`overlay_draw_research.md` observed pixel-exact scissors). With a larger RT
every scissored layer (options-menu lists, song-wheel windows, any
`root+0x60 != 0` group, the modpack's own `set_scissor` emissions) would clip
to the top-left 2/3. This is the **one walker-level fix a resolution mod needs**:
scale by `rt/canvas` (both available: `walker+0x144/+0x146` and the current
`ctx.scale`) before writing the gd record — a small detour on `FUN_180269080`.

---

## 5. The present chain

Three `AfterRenderConditionImpl` sub-objects inside `DAT_1806f1f10`
(`FUN_1801f3de0`, 0x2b8 bytes): BEGINVIEWPORT (+8, in AFTER RENDER 3D),
COPYVIEWPORT (+0xc8, in DISPLAY @0x65), ENDVIEWPORT (+0x188, in PRESENT). A
shared parameter block at +0x248 (`FUN_1801f4060`: `sys_copy`,
`sys_copy_depth`, `sys_copy_aa` shader objects, display texture view, present
quad vb `DAT_1806f1ef0+0x124`).

### 5.1 COPYVIEWPORT = letterboxed `StretchRect`

`FUN_1801f4750 → FUN_1801f44d0` (skipped when AA == 3): emits gd `0x14` (clear
with the border colour `DAT_1806f7240 × (r,g,b,a)`) then
`FUN_1801f4790` → gd **`0x31` = `IDirect3DDevice9::StretchRect(src =
render_color (+0xc0), srcRect = this+0x28c.., dst = display (+0xec), dstRect =
this+0x29c.., filter = this+0x2ac)`** (executor case `0x31` → device vtbl
+0x110).

### 5.2 `FUN_1801f3f60(this, mode)` — the rect math (the engine's own scaler)

```
screen_w = DAT_1806f20d8[0]; screen_h = [1]
if screen_w == 0x500:               src = {0,0,1280,720}; dst = {0,0,screen_w,screen_h}; filter = 1 (POINT)
elif mode == 1:                     src = {0xa0,0,0x460,720}  (960-wide 4:3 centre crop, SD cabinets); filter = 2
else:                               scaled_h = screen_w / 1280.0f * 720.0f
                                    y = (screen_h - scaled_h) * 0.5
                                    src = {0,0,1280,720}; dst = {0, y, screen_w, y+scaled_h}; filter = 2 (LINEAR)
this+0x28c..0x298 = src {x0,y0,x1,y1}; +0x29c..0x2a8 = dst; +0x2ac = filter; +0x2b0 = mode
```

Immediates: `MOV EDX,0x500` at `1801f3f94` (src x1) and
`MOV dword [RBX+0x298],0x2d0` at `1801f403d` (src y1). Called from the ctor and
from 14 game-code sites (`FUN_18002de60`, `FUN_18002e7b0` ×13 — the
TEST-MENU/attract screen-mode switch). **This function is a complete,
shipping upscale path**: point a 1080p back-buffer at it and it letterboxes
(16:9→16:9 = full-screen) with a linear `StretchRect`, no code needed.

### 5.3 ENDVIEWPORT = full-screen copy into the back-buffer

Frame begin `FUN_1801f24d0` → `FUN_1802518e0(DAT_1806f1ef0+0x80 /*rt[0x10]*/,
DAT_1806f20d8+0x38 /*back-buffer surface*/)` — the PRESENT RT struct is
re-pointed at the back-buffer every frame. `FUN_1801f4770 → FUN_1801f45c0`
binds it, sets texture 0 = display view (+0x104), shader `sys_copy`, and draws
the 4-vertex quad (`FUN_1801f25c0(list, prim, count)`). The **PRESENT viewport
dims are the RT struct's hard-coded 1280×720 u16s** (§3.1) and its depth is the
720p `render_depth`. With a larger back-buffer the copy would land in the
top-left 1280×720 and bind an undersized depth buffer — both must be fixed even
for the cheapest tier.

---

## 6. The AFP / BM2D path

### 6.1 Callback table and remap

gamemdx registers `DAT_180388940` (copied to `DAT_1804607e0` in
`FUN_18021cc30`) via `afp_boot` → `afp_set_render_params` (libafp ordinal 1).
libafp `FUN_180160080` remaps it through `UNK_18020aea0` (34 `{dst,src}` u16
pairs) into `DAT_180245108`; gaps fall back to libafp defaults:

| libafp slot | gamemdx fn | role |
|---|---|---|
| 0 | `FUN_18021a290` | begin: `SetShader(bm2d_default,0)` (tag 0x13) |
| 5 | `FUN_18021a6d0` | set blend mode → ctx+0x1c |
| 6 | `FUN_18021a870` | set filter/clip |
| 7 | `FUN_18021afb0` | `draw_shape` by renderer-side handle — a **stub** (acquires the ctx lock, returns the ctx pointer); gamemdx never draws libafp shape handles |
| **8** | **`FUN_18021a960`** | **`draw_primitive`**: copies `{x,y,z,u,v,colour}` verbatim into a tag-0x05 record + c48/c49 colour mul/add |
| 9, 10 | *(none → libafp default)* | `load_matrix` (2×3) / `load_matrix44` — **libafp applies the layer matrix itself, CPU-side** |
| **11** | **`FUN_18021b040`** | `load_projection_matrix44(NULL or m)` → VS **c50–c53** via `FUN_18002af70(list, 2, m, 4)` |
| 12 | `FUN_18021aff0` | get screen rect → `{0, 0, screen_w, screen_h}` |
| 13 | `FUN_18021b030` | `(1.0, 10000.0)` depth range |
| 14 | `FUN_18021b480` | texture info by name → `{id, w, h, 0, 0, 2w, 2h}` (real texture dims) |

### 6.2 `FUN_18021b040` — the projection callback

NULL case: `D3DXMatrixOrthoOffCenterRH(l=0, r=(float)screen_w, b=(float)screen_h,
t=0, zn=0, zf=1)` (`18021b12b..18021b16f`, dims from `DAT_1806f20d8`), composed
with a half-pixel correction built from the BM2D render context rect
(`ctx+0x8 x, +0xc y, +0x10 w, +0x14 h` → `S(w/2, -h/2)`, `T(x-0.5, y-0.5)`),
uploaded to c50–c53. **This is the one AFP-side site that hard-wires the
physical screen size.** Because `draw_primitive` vertices go through the
walker's canvas→NDC conversion (§4.1) exactly like every other record, the
`gs_screencommand_bm2d_*` VS cannot be applying c50–c53 to positions (it would
double-transform) — so this matrix is most likely dormant/auxiliary in the 2D
path. **Hypothesis H1 (§10): confirm by dumping the bm2d VS.** If it *is*
consumed, the fix is still one site: feed the callback the logical 1280×720
instead of `DAT_1806f20d8`.

### 6.3 Render context and layer roots

- BM2D render context (0x88, `FUN_18021a1b0`, `*DAT_1806f1ff8`): `+0x8..+0x14`
  rect (defaults 0,0,1,1; reset to `0,0,screen_w,screen_h` by `FUN_18021ba00`),
  `+0x68/+0x70/+0x78` = `gs_screencommand_bm2d_default/multiply/hsv` shader
  objects, `+0x60` default shader.
- `agcs::BM2DGroup` (0x38; vtable `0x18035c148`): ctor in `FUN_18002b080` stores
  **`+0x24 = screen_w, +0x28 = screen_h`** (`18002b37f..18002b3a3`); render
  `FUN_180215880` copies them into ctx `+0x10/+0x14` then `afp_do_display(2,
  group_id)`. Set-size vfunc `FUN_180215860`.
- Layer table `DAT_1806f1d20` (11 entries × `{list_ptr, root, list_index}`,
  `FUN_18002b080`): list indices `{1,0,0,0,0,0,3,3,1,4,5}`. The set-size loop
  (`18002b21a..18002b287`) sizes roots **1, 3, 5, 6, 7 to SCREEN dims
  (`FUN_180247d30/d50`), 0/2/4/8/9 to 1280×720, 10 to 1280×1280**. Entry **7 =
  the modpack's widget/override layer → SYSTEM list**, which is drawn after the
  upscale blit (§3.3) — i.e. it is a genuine screen-pixel layer. On a stock
  cabinet screen == 1280×720 so nobody notices.
- The six `BM2DGroupWithPan` groups that host the game's scene AFP layers
  attach to roots 0, 8, 8, 8, 2 (1280×720 canvases) and one to root 7 (SYSTEM).

Net: game AFP content lives in 1280×720-canvas roots and is resolution-independent
via §4.1; only root 7 (SYSTEM) and the two "screen dims" lists are physical.

---

## 7. Screen-pixel consumers (`DAT_1806f20d8[0..1]` readers) — audit

| site | function | uses screen dims for | action for a resolution mod |
|---|---|---|---|
| `1801f5ed2`/`1801f5f56` | `FUN_1801f5d10` | SYSTEM / DEBUG_DIALOG viewport dims | none (already correct) |
| `1801f0468`/`1801f07c0` | `FUN_1801f01a0` | display surface + present-quad half-pixel | none |
| `1801f3f76` | `FUN_1801f3f60` | letterbox dst rect | none (§5.2) |
| `1801f256d` | `FUN_1801f24d0` | back-buffer surface handle | none |
| `18002b37f` | `FUN_18002b080` | BM2DGroup +0x24/+0x28 → BM2D ctx rect | only matters if H1 is true |
| `18021b131` | `FUN_18021b040` | AFP ortho extents | H1 |
| `18021aff6` | `FUN_18021aff0` | libafp "screen rect" callback | none (a larger rect only relaxes culling / hit-test bounds) |
| `18021ba0a` | `FUN_18021ba00` | BM2D ctx rect reset | H1 |
| `1800068a5..180008087` (≈12) | `FUN_180006800` family | debug/dev line & text drawing in **screen-percentage** units (`screen × pct / 100`) | none — inherently resolution-independent (TEST MENU family) |
| `1801fb6f7`/`1801fb757`/`1801fbeaf` | `FUN_1801fb6f0`/`750`/`bc40` | system-text overlay: `screen × x / ref_w` with `ref = *(DAT_1806f1f28+0x74/+0x78)` | none (scales by design) |
| `180247xxx`, `18024c49e` | gd device layer (`FUN_180247c80`, executor `Present`) | `D3DPRESENT_PARAMETERS`, swap-chain | none |
| `1800092f3` | (debug) | percentage drawing | none |

Also physical-pixel, not via the global: **the scissor path (§4.3)** and **the
PRESENT RT struct dims / depth (§5.3)**.

Not physical (must stay): `DAT_18038eb34/38` (1280.0f/720.0f rodata; 15/14
readers — canvas normalisation, cull bounds, `agcs::SolidFade` sprite size,
`FUN_1801f3f60`'s aspect math), `ScreenRoot` +0x50/+0x54 defaults,
`DAT_18046091c/DAT_180464108` walk defaults.

---

## 8. Key addresses + struct layouts (20260616)

| symbol | address | role |
|---|---|---|
| `Application::onBoot` | `FUN_1800020a0` | builds display struct at `[RSP+0x50]`; display-init call `FUN_1801f1cf0(&local_278)` |
| graphics init | `FUN_1801f1cf0` | `FUN_1801ef6d0` → `FUN_1801ef9e0` → `FUN_1801f01a0` → `FUN_1801f5d10` → attachments |
| back-buffer dims select + mode enum | `FUN_1801ef6d0` | `C7 05 … 00 05 00 00` at `1801ef6f6` (imm32 at `1801ef6fc`), `C7 05 … D0 02 00 00` at `1801ef700` (imm32 at `1801ef706`) → `DAT_1806f0524/0520` |
| display descriptor / `initGs` | `FUN_1801ef9e0` | 0x30 descriptor at `[RSP+0x20]` |
| gd init / per-display info | `FUN_1802473b0` | `DAT_1806f20d8` (0x40 stride), `DAT_1806f20c4` count |
| `D3DPRESENT_PARAMETERS` fill | `FUN_180247c80` | table §2.3 |
| `CreateDevice` | `FUN_18024aed0` | device → `DAT_1806f2110` |
| render surfaces + viewport graph | `FUN_1801f01a0` | `DAT_1806f1ef0` (0x170); hoists `R15D`@`1801f02b6`, `ESI`@`1801f02da`; RT-struct dim instructions `1801f0e8c` (`C7 40 16 D0 02 00 00`), `1801f0f34` (`C7 41 14 00 05 00 05`), `1801f0fb4`/`1801f1104`/`1801f11bc` (`C7 41 14 00 05 D0 02`), `1801f106e` (`C7 40 16 D0 02 00 00`); the two u16 width writes are `66 44 89 78 14` (R15W) |
| surface create / view / RT bind | `FUN_18024f610(w,h,fmt[,ms])` / `FUN_180249ba0(surf,flags)` / `FUN_1802518e0(rt,surf)` | |
| named lists + viewports | `FUN_1801f5d10` | lists `DAT_1806f0620[8]`, viewports `DAT_1806f0568 + i*0x18` (scene at −8, render at +8); imms `1801f5e6c..1801f5f87` |
| viewport attach | `FUN_1802662a0(parent, child, prio)` | |
| segment header / SetViewport / c13+PSc1 | `FUN_18026c9f0` / `FUN_18026c4d0` / `FUN_18026c440` | walker `+0x140..+0x146` |
| tag 0x07 (2D context) | `FUN_180268c40` | canvas → `ctx` (§4.1) |
| tag 0x05 (DrawVertices) | `FUN_1802689b0` | NDC formula (§4.1) |
| tag 0x0C (scissor) | `FUN_180269080` | raw copy → gd 0x18 (§4.3) |
| gd executor | `FUN_18024c310` | `0x17` SetViewport (+0x178), `0x18` SetScissorRect (+0x258), `0x31` StretchRect (+0x110), `4` Present |
| present chain object | `FUN_1801f3de0` (`DAT_1806f1f10`, 0x2b8) | BEGIN +8 / COPY +0xc8 / END +0x188; params +0x248; rects +0x28c..+0x2b0 |
| letterbox rect | `FUN_1801f3f60(this, mode)` | imms `1801f3f95` (0x500), `1801f4043` (0x2d0) |
| COPYVIEWPORT blit | `FUN_1801f4750` → `FUN_1801f44d0` → `FUN_1801f4790` (gd 0x31) | |
| ENDVIEWPORT copy | `FUN_1801f4770` → `FUN_1801f45c0` | `sys_copy` quad |
| frame begin (retarget PRESENT rt) | `FUN_1801f24d0` | |
| `agcs::ScreenRoot` | ctor `FUN_180217620`, vtable `0x1803887c8` (slot1 set-size `FUN_1802175f0`, slot5 render `FUN_1802178d0`) | `+0x48 x, +0x4c y, +0x50 w, +0x54 h, +0x58/+0x5c scale, +0x60 scissor flag` |
| layer table + BM2DGroups | `FUN_18002b080` | `DAT_1806f1d20`; set-size loop `18002b21a`; BM2DGroup ctor `18002b35b` |
| `agcs::BM2DGroup` | vtable `0x18035c148`; render `FUN_180215880`; set-size `FUN_180215860` | `+0x1c x, +0x20 y, +0x24 w, +0x28 h, +0x30 prio, +0x34 flag` |
| stock roots | `DAT_1806f07a8[0..6]` (`FUN_180215650`) | `FUN_180215410/480` render walks |
| AFP callback table | `DAT_180388940` → copy `DAT_1804607e0` (`FUN_18021cc30`) | libafp `DAT_180245108`, remap `UNK_18020aea0` |
| AFP projection callback | `FUN_18021b040` | ortho args `18021b141..18021b16f` |
| BM2D render ctx | `FUN_18021a1b0` (0x88), `*DAT_1806f1ff8`; reset `FUN_18021ba00` | |
| 1280.0f / 720.0f rodata | `DAT_18038eb34` / `DAT_18038eb38` | logical — never patch |
| screen dims globals | `DAT_1806f0524` (w), `DAT_1806f0520` (h), `DAT_1806f1d88` (fullscreen ok), `DAT_1806f050c` (AA), `DAT_1806f0508` (fps), `DAT_1806f0514` (bb count), `DAT_1806f0518` (HWND) | |

`gs::Viewport` (0xc0, `FUN_1801f5d10`): `+0x8 x, +0xc y, +0x10 w, +0x14 h,
+0x18 minZ, +0x1c maxZ, +0x20 name hash, +0x28 view[16], +0x68 proj[16], +0xb0
scene, +0xb8 list-render`. Segment header takes `vp+8`.

Walker 2D context (`*(walker)`, 0x20): `+0x00 {ox/rt_w, oy/rt_h, 0, 0}`,
`+0x10 {1/canvas_w, 1/canvas_h, 1, 0}`.

---

## 9. Cross-version verification

All patterns unique (exactly one hit) on every build listed. Addresses are the
**match** addresses (containing function on 20260825 in parentheses).

| site | AOB | 20260616 | 20260721 | 20260825 | 20250805 |
|---|---|---|---|---|---|
| back-buffer dims select (`FUN_1801ef6d0`) | `80 79 12 00 48 8B F1 74 16 C7 05 ?? ?? ?? ?? 00 05 00 00 C7 05 ?? ?? ?? ?? D0 02 00 00 EB 14 C7 05 ?? ?? ?? ?? 80 02 00 00 C7 05 ?? ?? ?? ?? E0 01 00 00` | `1801ef6ed` | `1801efe1d` | `1801f062d` (`FUN_1801f0610`) | `1801d812d` |
| tag-0x07 rt-dims read | `0F B7 81 46 01 00 00 66 0F 6E C0 0F B7 81 44 01 00 00 66 0F 6E C8` | `180268cbb` | — | `180268f1b` (`FUN_180268ea0`) | `18021fc8b` |
| tag-0x0C raw gd write | `C7 00 18 00 0C 00 66 89 48 04 66 89 50 06 66 44 89 40 08 66 44 89 50 0A 48 83 C0 0C` | `18026913d` | — | `18026939d` (`FUN_1802692e0`) | — |
| letterbox rect (`FUN_1801f3f60`) | `BA 00 05 00 00 8B F8 8B F0 44 8B C8 41 B8 01 00 00 00 44 3B D2 74 ?? 45 3B D8 75 ?? B8 A0 00 00 00 BA 60 04 00 00` | `1801f3f94` | — | `1801f5044` (`FUN_1801f5010`) | `1801dcba4` |
| `FUN_1801f01a0` hoisted 1280 | `45 33 C9 45 8D 41 15 41 BF 00 05 00 00 41 8B D7 41 8B CF E8` | `1801f02af` | — | `1801f11ef` (`FUN_1801f10e0`) | `1801d8cef` |
| list viewport dims (`FUN_1801f5d10`) | `C7 85 ?? ?? 00 00 00 05 00 00 C7 85 ?? ?? 00 00 D0 02 00 00 48 8D 05 ?? ?? ?? ?? 48 89 85 ?? ?? 00 00 C7 85 ?? ?? 00 00 00 05 00 00 C7 85 ?? ?? 00 00 D0 02 00 00` | `1801f5e6c` | — | `1801f6f1c`, `1801f6f3e` (`FUN_1801f6dc0`; two overlapping hits of one table) | — |
| layer set-size loop (`FUN_18002b080`) | `48 83 FB 01 74 ?? 48 83 FB 03 74 ?? 48 83 FB 04 76 ?? 48 83 FB 07 76 ?? 48 83 FB 0A 75` | `18002b21a` | — | `18002ac4a` (`FUN_18002aab0`) | `18002ac3a` |

"—" = not run this session (the 616/825/805 triple already brackets 721; the
first row was checked on all four). The function bodies around every hit are
byte-identical compiler output apart from RIP displacements — all of this is
init-time code that Konami has not touched across a year of builds.

---

## 10. Open questions / hypotheses (need a live probe)

- **H1 — bm2d VS and c50–c53.** Dump `gs_screencommand_bm2d_default` from
  `shader.arc` and check whether the VS reads c50–c53 for position. Argument
  that it does not: tag-0x05 vertices are CPU-converted to NDC by the walker
  (§4.1). If it does, `FUN_18021b040` (+`FUN_180215860`'s screen dims) must be
  fed the logical 1280×720.
- **H2 — depth-buffer size mismatch.** In every tier the PRESENT RT would bind
  a 720p depth to a larger back-buffer (§5.3) unless fixed. Retail D3D9 may
  render anyway; the debug runtime rejects the draw. Fix options: null the depth
  on rt[0x10], or make `render_depth` screen-sized (automatic in Tier B).
- **H3 — RENDER_CAPTURE / read-back consumers.** No static consumer of list 7
  or of `render_color` read-back was found; if a photo/upload feature reads a
  1280×720 buffer it would break in Tier B. Probe: log gd `0x32`
  (`GetRenderTargetData`-class copy) and any `LockRect` on render_color.
- **H4 — spice2x's D3D9 wrapper.** spice2x sits on `CreateDevice`/`Present`
  (`graphics::d3d9`) and implements `-w`; confirm it forwards a non-720p
  `BackBufferWidth/Height` unchanged and how its windowed mode sizes the window.
  The game's own request must simply be 1920×1080 when it reaches the wrapper.
- **H5 — AA config on real cabinets.** Which `DAT_1806f050c` value ships
  (depends on PC type); mode 3 changes which surfaces are targets (§3.2).
  Forcing 0 at boot sidesteps it.
- **H6 — 3D camera aspect.** No 1280/720 pixel constants were found in the 3D
  `Packet`/camera path (`FUN_1801f5880`, `FUN_1802204c0`); aspect is presumably
  viewport-derived or a 16:9 constant (`1.7777f` at `0x18035a7fc`). Irrelevant
  for 16:9 targets; would matter for ultrawide (out of scope).
- **H7 — half-pixel/texel alignment at 1.5×.** Bitmaps that are 1:1 today will
  be resampled; expect softness at 1080p (1.5×), much less at 4K (exact 3×).
  Any UI that relied on integer pixel snapping (thin 1-px AFP strokes) may
  shimmer at 1.5×.
- **H8 — u16 dims.** Viewport/RT/scissor dims are u16 everywhere: 4K fits, 8K
  does not.
- **H9 — CrossOver/D3DMetal.** 4K fill rate for the theme shaders and MSAA
  surfaces; the movie plane path is unaffected (texture-sampled).

---

## 11. What a mod would have to do — tiered estimate

Everything below is `early_apply`-style boot patching (the `fps_unlock`
precedent: the DLL's init provably completes before `onBoot`'s display init)
plus one or two small detours. Nothing per-frame except the scissor fix.

### Tier A — native *output* resolution (720p internal, engine-side upscale)

1. Patch the two imm32s in `FUN_1801ef6d0` (`0x500/0x2d0` → target W/H; AOB row 1).
   The fullscreen enumerator then requests the exact target mode; `display`
   (+0xec), the DISPLAY viewport, the SYSTEM/DEBUG_DIALOG lists, the letterbox
   dst rect and the present-quad half-pixel all follow automatically.
2. Post-original detour on `FUN_1801f01a0`: rewrite rt[0x10] (`+0x80`) dims to
   W/H and null its depth (H2).
3. Resize layer root 7 (SYSTEM) to 1280×720 via `ScreenRoot` vtable slot 1 so
   the modpack's widgets/mod-menu keep their canvas semantics (they would
   otherwise shrink to the top-left 2/3 — §6.3). Verify the SMX overlay/touch
   mapping (already canvas-relative) and toasts.
4. Optionally force `DAT_1806f050c = 0` (H5).

Result: the monitor runs at native 1080p/4K, the 720p composite is upscaled by
the game's own linear `StretchRect` (§5.2) instead of the panel scaler; the
mod-menu/SYSTEM overlays render at native resolution. Content sharpness
unchanged. **Effort: small — a few days including cabinet validation.**

**Tier A+ (cheap, high value):** swap the bilinear upscale for a quality
upscaler. The ENDVIEWPORT copy already runs a named shader (`sys_copy`) over a
full-screen quad with the display texture bound; the modpack's shader synthesis
(`shader_fixes`) can supply a sharper PS (bicubic / FSR-EASU-style) and the
COPYVIEWPORT `StretchRect` can be left POINT (or retargeted). This alone likely
addresses "unfortunate scaling on 1080p panels" without any re-layout.
**Effort: a few days more.**

### Tier B — native *internal* resolution (arbitrary 16:9)

Tier A plus:

5. `FUN_1801f01a0`: patch the two hoisted immediates (`R15D`, `ESI` — AOB row 5)
   so all ten 1280×720 / 1280×1280 surfaces are created at W/H (OFFSCREEN1 →
   W×W); in the same post-original detour rewrite the u16 dims of rt[0xb],
   [0xc], [0xd], [0xe], [0x10], [0x12] (leave [0xf]).
6. `FUN_1801f5d10`: post-original detour rewriting `vp+0x10/+0x14` of lists
   FRONT/MIDDLE/BACK/OFFSCREEN0/RENDER_CAPTURE → W/H and OFFSCREEN1 → W×W
   (or 12 imm patches — AOB row 6).
7. `FUN_1801f3f60`: patch the two src-rect imm32s (`0x500` at `1801f3f95`,
   `0x2d0` at `1801f4043`) to W/H so the `screen_w == render_w` branch is taken
   (1:1 POINT copy) — or bypass COPYVIEWPORT entirely by pointing RENDER_2D at
   `display` (what AA-3 does).
8. Scissor: detour `FUN_180269080` to scale `{x,y,w,h}` by
   `rt_dim × ctx.scale` before the gd write (§4.3).
9. H1: if the bm2d VS consumes c50–c53, feed `FUN_18021b040`/`FUN_180215860`
   the logical 1280×720.
10. Force `DAT_1806f050c = 0` (removes the AA-3 target juggling and MSAA at 4K).

Result: every vector/AFP shape, gradient, guideline, HUD quad, text edge drawn
from geometry, the 3D background pass, and the arrow AA/perspective shaders
rasterise at native resolution; bitmaps are still sampled from the untouched
stock 720p atlases — the "upsampling" here is ordinary GPU texture
magnification at draw time (bilinear between texels), not any offline
processing. Tiers A and B change no files on disk.
**Effort: medium — one to two weeks of implementation and cabinet iteration**
(scissored screens, results/photo path H3, both AA configs, CrossOver 4K
performance, `shader_fixes` AA at non-integer scale).

### Tier C — hi-res asset pass (open-ended; the only tier that touches assets)

Per-asset replacement of 720p bitmaps with higher-density art. Detail is in
**§14**, which works through the "vectorise offline, rasterise in the DLL at
the target scale, serve through LayeredFS" pipeline the maintainer proposed,
and the format/engine constraints it must satisfy. Headline: the existing
LayeredFS texture path already does everything except *scale* — it rejects a
PNG whose size differs from the texturelist `imgrect`, and the engine builds
the GPU atlas at the texturelist `<size>` — so the work is (a) a texturelist
rewriter that scales `<size>`/`imgrect`/`uvrect` by an integer factor while
keeping the normalised layout identical, (b) an SVG rasteriser in the DLL, and
(c) the art. Layout never changes: geometry is logical. **Effort: engine side
~1–2 weeks; art open-ended and incremental.**

---

## 12. Modpack-side impact inventory

Safe (all in the logical canvas or NDC): `cull_window` 720.0 (+ its "RIP target
reads 720.0f" verification — the rodata constant is untouched),
`playfield_styling` `RENDER_HEIGHT`/`X_SPLIT 640`, `player_perspective`
canvas-px constants and the persp VS's 640/360 NDC↔canvas reconstruction, theme
shaders' canvas reconstruction and `MODAL_*` rects, `overlay_draw`
`set_context_2d(1280,720)` (emits its own canvas), SMX overlay (own canvas
record; touch maps client/monitor px → 1280×720 model), `center_arrows_single`
`LANE_SHIFT`, `training_mode` strip/scrub geometry, `bg_preview_overlay`
`NATIVE_W/H` (logical scale of markers), `signatures.rs` derivations that
classify by `find_rip_f32_loads(720.0)`.

Needs attention:

- **Layer root 7 (SYSTEM) canvas** — every `widget_renderer` text/image widget,
  toast, and the mod menu lives there; it is screen-pixel today (§6.3). Tier A
  step 3 handles it.
- `shader_fixes` AA PS: its "collapses to stock at 1:1" identity no longer holds
  (by design it then does real filtering); `TEXEL` constants must become
  `(1/(768k), 1/(384k))` with hi-res sheets, and `render_notes`' 384.0/96.0
  cell constants need the k-redirect (§14.4).
- `overlay_draw`/mod-menu `set_scissor` emissions are canvas px — fixed by the
  same scissor detour as the game's.
- `preview_overlay`/`bg_preview_overlay`/`check_option_takeover.py` templates are
  1280×720 screenshots; authoring captures at 1080p must be downscaled (tooling
  note, not runtime).
- Mod-menu animated backgrounds at 4K under D3DMetal (H9): ~4–9× the pixels of
  the current 1160×600 modal.
- `fps_unlock` requests `FullScreen_RefreshRateInHz = preset`; the requested
  `(W, H, Hz)` triple must exist as a display mode (same risk as today, now with
  a larger W/H).

---

## 13. Gotchas

- Never change `DAT_18038eb34/38` (1280.0f/720.0f rodata), `ScreenRoot`
  +0x50/+0x54 defaults, or `DAT_18046091c/DAT_180464108` — they are the logical
  canvas; changing them shrinks/stretches all content instead of sharpening it.
- `walker+0x144/+0x146` are the **viewport** dims set by the segment header,
  not the surface size; a viewport smaller than its surface renders into a
  corner. Surfaces (§3.1) and viewports (§3.3) must be changed together.
- rt[0x10] (PRESENT) is retargeted at the back-buffer every frame by
  `FUN_1801f24d0` but keeps ctor-time dims and depth — patch the struct, not
  the surface.
- rt[0xf] (DISPLAY) already uses screen dims; touching it double-applies.
- `FUN_1801f3f60`'s `screen_w == 1280` comparison decides POINT vs LINEAR and
  whether the letterbox math runs; after Tier B the comparison should be against
  the internal render width (patch both src imms so the equality branch fires).
- AA config 3 re-wires targets (`display` becomes the RENDER/2D target) — the
  RT-struct rewrite must run *after* the ctor in both configurations.
- Scissor u16 fields, RT u16 dims, viewport u16 dims: 4K OK, 8K overflow.
- The layer set-size loop sizes roots 1/3/5/6 to screen dims too; nothing in
  the modpack uses them, but a 1080p run will reveal whether any stock content
  in those roots is positioned in physical pixels (debug/system overlays are,
  and they scale by percentage — §7).
- `D3DPRESENT_PARAMETERS.EnableAutoDepthStencil` is FALSE — the engine owns all
  depth surfaces; there is no automatic depth to lean on when the back-buffer
  grows (H2).

---

## 14. Vector-source assets: rasterise in the DLL at the target scale

The maintainer's proposal: vectorise the game's UI bitmaps **offline** (SVG),
ship the vectors, and have the DLL rasterise them **at texture-request time**
at whatever density the chosen resolution needs, serving the result through
LayeredFS exactly like today's PNG replacements. This section establishes what
the shipped data actually is, why the engine cannot rasterise vectors itself,
which engine paths care about a texture's pixel size, and the precise pipeline
changes that make hi-res serving work. Sources: libafp 2.13.7 disassembly,
`bemaniutils/bemani/format/afp/{geo,swf,render}.py`, and this repo's
`ifs_textures.rs` / `atlas_cloner.rs` / `texture_resolver.rs`.

### 14.1 What the shipped art is — there is nothing to vectorise *in the data*

- **GE2D shapes are pre-tessellated.** A geo file (`bemaniutils geo.py`) is
  `{vertices (x,y f32, pixels), tex_points (u,v f32), tex_colors (rgba), labels
  (texture-region names), DrawParams[]}` where each `DrawParams` = `{mode==4,
  flags, tex1 label idx, trianglecount, rgba, u16 triangle indices}`. Flag
  `0x2` = textured, `0x8` = solid fill colour, `0x40` = "normalise UVs by the
  region rect at draw time" (otherwise UVs are already 0..1 fractions of the
  atlas `<size>`). **No edge, curve, fill-style or line-style records exist**
  in GE2D or in the `AP2_SHAPE` tag (a 4-byte `{unknown, shape_id}` binding to
  `<name>_shape<id>`). Konami's converter (`afp 1.0.0 / converter 1.3.80`,
  per the IFS `version.xml`) baked Flash shapes into textured triangle meshes
  + raster atlases at authoring time. Solid-colour rectangles survive as
  untextured `DrawParams` and are already resolution-exact.
- **libafp does contain a runtime tessellator** — `afp-tesselate.c`
  (`FUN_1801a93a0`, "Unknown shape fill style type(%x) to check uv", SWF fill
  style ids `0x00` solid / `0x10,0x12,0x13` gradient / `0x40..0x42` bitmap,
  `afp_add_triangle_uv`, scanline sort via `FUN_180131f50`) plus
  `afp_shape_make_mesh_from_points` and `afp_shape_make_scaling_grid_shape_
  from_bitmap` (Scale9Grid). Its only statically reachable caller is the
  **MorphShape** advance path (`afp-morph.c` → `FUN_180154760` →
  `FUN_1801191b0`), i.e. `AP2_DEFINE_MORPH_SHAPE` (0x82) is the one tag that
  still carries edge data (start/end shapes interpolated per frame).
  bemaniutils parses it but renders a dummy; whether any DDR World AFP uses
  morph shapes is unknown (H10). `afp_set_shape_accuracy` writes
  `DAT_180245030` (default 0.5) which has **no reader** in this build — a dead
  knob. gamemdx's callback for libafp's `draw_shape`-by-handle (slot 7,
  `FUN_18021afb0`) is a stub that returns the render context — gamemdx never
  draws renderer-side shapes, everything reaches the GPU as `draw_primitive`
  triangle lists. **Conclusion:** the built-in vector machinery is a
  morph-shape-only leftover and is not a viable general path; the brute-force
  route (rasterise in the DLL, feed the engine bitmaps) is the right one.

### 14.2 How a texture's pixel size reaches the engine — and who uses it

The engine never reads dimensions from the pixel payload:

- IFS texture payloads are **headerless** (`argb8888rev` = raw BGRA;
  `dxt5` = raw word-swapped BC3 blocks; optional 8-byte AVSLZ prefix). Atlas
  dims come from `texturelist.xml <size>` and each image's rect from
  `<imgrect>`/`<uvrect>` (stored doubled; `uvrect` inset by 1 px for
  filtering). `ifs_textures.rs::cache_texture` L568-702, L886-898;
  `afp_texture_pipeline.md` L63-81.
- `get_bitmap_info` (libafp → gamemdx) returns `{bm2d tex ptr, u16 atlas W,
  u16 atlas H, u16 L,R,T,B (doubled)}`; loose ResourceManager textures return
  `{id, w, h, 0, 0, 2w, 2h}` via callback slot 14 (`FUN_18021b480`).
  `texture_resolver.rs` L193-205.
- The gs texture object (`gs::TextureData`) keeps `u16 w @+0x08, h @+0x0A`
  (bind id `@+0x04`); the 0xA0-stride registry entry at `DAT_1806f0a30` holds
  the sampler state block at `+0x34` (`FUN_18024b7c0`).

Who consumes those dims for **layout** (not just UV sampling):

| consumer | reads px dims? | effect of a hi-res atlas with *identical declared* `<size>`/rects |
|---|---|---|
| GE2D shapes (all AFP scene art) | no — vertices pre-baked, UVs normalised | none — the safe bulk case |
| `ImageWidget` / `agcs::Sprite` | no — quad from explicit w/h, UVs from bitmap info | none |
| `afp_mc_load_bitmap`-bound bitmaps (option rows, combo digits) | **yes** — "natural size" = imgrect px | none, *provided declared rects are unchanged* |
| `sequence::SpriteLayer` (BPM/LENGTH digit rows) | **yes** — advance = image width | same |
| KBF text | **yes** — fixed cell grid; UV divisor is the rodata `DAT_18036D284` | KBF is a separate format — needs its own hi-res KBFs (§14.5) |
| arrow sheets (`render_notes` `FUN_180026b00`) | **yes** — `u = col·384.0/texW`, `v = row·96.0/texH` from `tex+8/+0x0A` | **breaks** if texW/H change (cell px constants) — see §14.4 |
| `shader_fixes` AA PS | `TEXEL = (1/768, 1/384)` hardcoded | breaks — parametrise |
| `agcs::SolidFade` | no (1280/720 rodata) | none |

The table says the whole problem is *declared geometry vs. physical texels*:
if the engine keeps seeing the stock `<size>`/`imgrect`/`uvrect` numbers while
the GPU texture behind them has more texels, **nothing in the layout path
changes** — but D3D9 textures don't work that way (the surface *is* its texel
count), and the engine builds the GPU atlas from `<size>`. So the pipeline must
scale the declared numbers by an integer factor `k` and rely on every consumer
being either UV-normalised (GE2D, widgets) or a divider of declared dims by
declared dims (SpriteLayer/`mc_load_bitmap` "natural size" in *canvas* px —
which is where §14.4 bites).

### 14.3 What the current LayeredFS texture pipeline can and cannot do

- **Per-image replacement** (`handle_texture`, keyed `ifs/tex/md5(name)`):
  a PNG **smaller** than the imgrect is transparently padded; **any other size
  mismatch is rejected** (`"PNG {}x{} doesn't match texturelist {}x{},
  skipping"`, `ifs_textures.rs` L609-618). `<size>` is never rewritten. → A
  2× PNG for a stock name is silently dropped today.
- **Net-new textures** (`inject_new_textures`): one `ctex###` atlas per PNG,
  `<size>` = PNG dims, `imgrect/uvrect = 0 2w 0 2h`.
- **Donor/fresh atlas clones** (`atlas_cloner.rs`): emits its *own* `<size>`
  (can differ from the donor after growth), composites PNGs at donor rects
  (**clips** an oversized PNG to the donor rect, L490-491) or shelf-packs
  fresh; `MAX_ATLAS_SIDE = 4096` (`texture_packer.rs MAX_TEXTURE = 4096`).
- **Merged XML** (`xml_merger.rs`) appends `texturelist.merged.xml` fragments
  to the stock texturelist; `CacheHasher` (path+mtime MD5 sidecars) guards
  every generated artefact under `data_mods/_cache/`.
- Two IFS storage families: per-image blobs (`tex/md5(image)`) and whole-atlas
  blobs (`tex/md5(atlas)`); the serve path must handle both.

### 14.4 The pipeline that works — "scale factor k texturelist rewrite"

Choose an integer `k` per run (2 for 1080p — 1.5× would put doubled rects on
half-texels; 3 for 4K; k is a *texel-density* choice independent of the render
resolution, any k ≥ ceil(scale) is fine). Then, at texturelist parse time for
an IFS that has vector sources available:

1. **Rewrite the texturelist**: `<size> *= k`, every `imgrect *= k`, every
   `uvrect` = k·imgrect inset by 2 (1 px at the new density). Normalised UVs
   inside geo files are unchanged because both numerator and denominator
   scaled. This is the one missing primitive — a variant of the existing
   `write_merged_texturelist`/`xml_merger` path that transforms instead of
   appends. Atlases must stay ≤ the D3D9 cap (`MaxTextureWidth/Height`,
   8192 on D3D9-class hardware, 16384 typical today; the repo's own 4096 cap
   in `atlas_cloner`/`texture_packer` must be lifted). 2048×2048×k=3 = 6144² —
   fine on a real GPU, **check D3DMetal/CrossOver caps (H11)**.
2. **Rasterise** each image's SVG at `k × imgrect` px into a scratch RGBA
   buffer (pure-Rust `resvg`+`tiny-skia`, or `usvg`+`tiny-skia`; both
   `no_std`-free and known to build for `x86_64-pc-windows-msvc`), composite
   at `k × imgrect` origin into a `k×<size>` atlas buffer. Images that have
   **no** vector source are upsampled from the stock PNG (bilinear/Lanczos or
   an offline ESRGAN pass shipped as a k× PNG) so a partially vectorised IFS
   is still coherent — never leave a stock-density hole in a k× atlas (the
   engine would sample the wrong region).
3. **Encode** as `argb8888rev` (simplest; 4 B/px — 6144² = 144 MB VRAM per
   atlas, so prefer `dxt5` via `texpresso` for the big scene atlases; the
   engine already accepts both) and store under `data_mods/_cache/<ifs>/
   <md5(atlas)>` (whole-atlas family) or per-image (`md5(image)`), with a
   `CacheHasher` sidecar keyed on `{k, SVG path+mtime (or content hash),
   stock texturelist text, renderer version}` — the `shader_synthesis`
   fingerprint pattern. Rasterisation is **offline-from-the-game's-view**: at
   first boot it runs once per changed asset (like atlas clones today, with
   the existing "reboot once" splash), later boots serve the cache. It does
   NOT need to happen on the game's file-request thread.
4. **Serve** through the existing `handle_texture` path with the size check
   relaxed to "matches `k × stock imgrect`".

Engine-side follow-ups this exposes (all small, all already located):

- **Arrow sheets**: `render_notes` divides px cell constants (384.0 `DAT_
  180399338`, 96.0 `DAT_18035a710`) by the bound texture's `u16 w/h`
  (`180026c46..`, `FUN_180026b00`), and `FUN_180025900`/spot/judge paths
  share the 96.0 constant (11 readers). A k× sheet makes every cell UV 1/k
  too small. Fix options: (a) redirect the two rodata loads to mod-owned
  floats `= 384·k / 96·k` (the `cull_window` disp32-redirect pattern; both
  constants are shared, so use redirect not overwrite — 96.0 has 11 readers
  across the arrow/spot/judge/guideline family, each must be audited), or
  (b) keep the arrow sheets at stock dims and give them their own k (arrows
  are the highest-value target for crispness, so (a) is worth it).
  `shader_fixes` AA PS `TEXEL` becomes `(1/(768k), 1/(384k))` via the
  synthesiser (it already fingerprints on inputs).
- **`mc_load_bitmap` / `SpriteLayer` natural size**: these read the
  *declared* rect (canvas px) from `get_bitmap_info`, not the GPU texture —
  so a k× texturelist would make them k× too big. Two choices: keep the
  declared rects unscaled and instead make the engine's **atlas upload** use
  a k× surface — impossible without a texture-create detour that multiplies
  dims and a UV-scale everywhere — or, cheaper, detour the two `get_bitmap_
  info` call sites (`texture_resolver.rs` already parses the record) to
  divide the returned rect by k for the handful of consumers that do natural
  size. **Verify which path libafp's `afp_mc_load_bitmap` takes (H12)** —
  it may read `<size>`-relative UVs (safe) plus the doubled rect (needs /k).
- **KBF fonts** (§14.5) and the `scr_distancefont` SDF fonts (already scale;
  their atlas is a separate resource).
- The `get_bitmap_info` **atlas W/H** fields are consumed by `texture_
  resolver.rs` for UV normalisation — automatically correct with a k× atlas.

### 14.5 Text

- **KBF** (`kbf_font_format.md`): fixed cell grid, glyph metrics in the `.kbf`,
  pixels in `.N.dds`; UVs = px / rodata atlas size `DAT_18036D284`. A hi-res
  KBF = new `.kbf` with `cell_w/h`, glyph `width/height/bearing/advance` all
  ×k **plus** a patch of that rodata divisor (or a redirect) — the game
  otherwise samples the wrong cells. Source: the stock fonts are bitmap-only;
  the repo has `kbf_to_font.py` (KBF→TTF via sbix) but **no TTF→KBF
  builder** — that tool (Pillow/FreeType rasterising a chosen TTF at k× cell
  size into the grid + DXT1/luminance DDS) is a prerequisite. CJK coverage
  (7.5k glyphs) needs a real CJK face (the repo already ships Noto Sans
  JP/KR for the option labels).
- **AFP `DefineFont` text** is bitmap regions in the texture atlas — covered
  by the k× atlas path automatically.
- **Option-menu labels** (`gen_option_labels.py`) are already procedural
  (TTF via Pillow at fixed px sizes 176×16 / 352×16 / 132×24 / 368×172):
  emitting them at k× is a constant change, minus the few committed raster
  templates (`scripts/templates/*`), which need k× masters.

### 14.6 The art itself

- Inventory first: dump every `texturelist.xml` across `data/arc/bm2d/*.arc`
  (the repo's `arc_tool.py`/`unpack_arc.py`) → ~hundreds of atlases, thousands
  of images. Rank by on-screen frequency and upscale visibility (song-select,
  gameplay HUD, results, options, attract). Jackets/banners are photographic
  → ESRGAN-class offline upscale, not vectors. Flat UI chrome, icons, digit
  sheets, judgement words, arrow sheets → vectors (auto-trace as a first pass
  — `vtracer`/`potrace` — then hand-clean the ones that matter).
- The SVG must reproduce the stock image *inside the same imgrect* (same
  bounds, same anchor) — the geo vertices are baked to those bounds. A
  per-asset "SVG viewBox = imgrect" convention plus a validation script that
  rasterises at k=1 and diffs against the stock PNG (SSIM threshold) keeps
  the pack honest and gives a regression harness.
- Everything is incremental: an IFS with zero vector sources is still served
  at k× via upsampled stock PNGs, so the pack can be built up asset by asset
  behind one config switch (`textures.density_k`), and Tier B without Tier C
  stays a valid intermediate state.

### 14.7 Effort

- Engine/pipeline (k-rewrite, rasteriser, cache, arrow-constant redirect,
  bitmap-info /k, KBF builder + divisor patch, D3DMetal cap check): **~1–2
  weeks**, parallel to Tier B.
- Art: open-ended; a first visible win (gameplay HUD + arrows + song-select
  chrome + fonts) is a few weeks of tracing/cleanup; full coverage is months.

### 14.8 New hypotheses

- **H10** — do any DDR World AFPs use `AP2_DEFINE_MORPH_SHAPE`? (grep the
  `.afp` tag streams via `core/ap2`; if yes they render through the runtime
  tessellator and are already vector.)
- **H11** — D3DMetal (CrossOver) max texture dimension and BC3 support at
  6144²/8192²; VRAM budget for k=3 atlases (stock 2048² BGRA = 16 MB; k=3 =
  144 MB uncompressed, 36 MB DXT5).
- **H12** — `afp_mc_load_bitmap` sizing: confirm whether the "natural size"
  quad uses the doubled rect from `get_bitmap_info` (needs /k) or geo-relative
  UVs only.
- **H13** — the 96.0/384.0 constant readers (11 + 3 sites): which are arrow-
  sheet UV math (scale by k) vs. canvas-px geometry (leave alone) — must be
  audited per site before any redirect.
