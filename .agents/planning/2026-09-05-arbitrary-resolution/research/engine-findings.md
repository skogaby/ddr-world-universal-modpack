# Engine findings — shader dump, spice2x source, AOB sweep

2026-09-05. Detail lives in `prototypes/shader_dump/REPORT.md` (full
disassembly, CTABs, extracted blobs) and `prototypes/aob_sweep/REPORT.md`
(per-build addresses, byte dumps, `sweep.py`). This file carries the findings
that change decisions.

## 1. H1 is REFUTED — the bm2d VS consumes c50–c53

`gs_screencommand_bm2d_default` VS (vs_3_0, 10 instr):
`o0 = c50·v0.x + c51·v0.y + c52·v0.z + c53` — CTAB names the block
`flash_parameter { color_mul c48; color_add c49; float4x4 viewProjection c50..c53 }`.
`gs_screencommand_default`'s VS is the documented passthrough `o0 = (x, y, 0, 1)`.
The bm2d PS reads only s0 (c48/c49 arrive as interpolants). Confidence: certain
(bytecode + CTAB + independent token decode).

So the projection callback `FUN_18021b040` (ortho over `DAT_1806f20d8` screen dims
∘ half-pixel correction from the BM2D ctx rect = `BM2DGroup +0x24/+0x28`, also
screen dims) is live, and for stock rendering to be correct the product must be
≈identity in NDC plus a half-**screen**-pixel offset. Consequences:

- **render == output (the default):** screen dims and the 2D RT move together —
  correct with no extra work.
- **render ≠ output (SD 640×480 from a 720p render, or supersampling):** the
  half-pixel term is half an *output* pixel expressed in NDC, i.e. a
  `0.5·render/output` render-px UV bias on every AFP bitmap. Sub-pixel, but
  visible as slight softening/shift of all AFP content. Exactness needs the
  callback + BM2DGroup sizing fed the RENDER dims (research doc §11 step 9 is
  now *required for exactness*, not hypothetical). Fail-open polish item.
- Diagnostic: AFP/BM2D content scaling or drifting while screen-command content
  (arrows, HUD quads) stays put ⇒ ctx rect and `DAT_1806f20d8` disagree.

## 2. `sys_copy` is a minimal copy; `sys_copy_aa` proves PS c1 = viewport rect

- `sys_copy` VS: 2 instr, **zero constants**, one POSITION float4 `{x_ndc, y_ndc,
  u, v}` → `o0 = (x, y, 0, 1)`, `o1.xy = zw`. PS: `texld_pp oC0, v0, s0`. Filter
  = engine sampler state. Nothing carries texel size.
- `sys_copy_aa` shares the VS byte-for-byte; its PS (57 instr, 9 texld) is a
  texkill-gated edge-directed blur reading **PS `c1` = `_gs_ps_parameter_ViewportSize
  {x,y,w,h}`** — hard evidence for research §3.4 (segment header uploads the
  viewport rect to VS c13 / PS c1). It is not an MSAA resolve.
- A replacement PS drops into `sys_copy` with the identical interface (`v0.xy`
  + `s0`) and can read the DESTINATION viewport from c1; the SOURCE texel size is
  not supplied (bake a `def` at synthesis, or upload from a small detour).
- **Which pass scales:** today the letterboxed upscale/downscale is the
  COPYVIEWPORT fixed-function `StretchRect` (render_color → display, filter
  POINT at 1:1 / LINEAR otherwise); ENDVIEWPORT's `sys_copy` quad is a 1:1
  display → back-buffer copy. A shader-based scaler therefore means re-routing:
  either bind `render_color`'s view (+0xF8) instead of the display view (+0x104)
  in ENDVIEWPORT and skip COPYVIEWPORT (the AA-3 early-out shape), or run the
  new PS in a COPYVIEWPORT-replacement draw. Moderately invasive → Phase 2.
- StretchRect LINEAR at exactly 2:1 (1280→640) is a 2×2 box — adequate for SD.

## 3. spice2x (`~/Desktop/Projects/spice2x.github.io`, `hooks/graphics/backends/d3d9/d3d9_backend.cpp`)

- `IDirect3D9::CreateDevice` hook: logs the game's `D3DPRESENT_PARAMETERS`
  verbatim, then rewrites `BackBufferWidth/Height` ONLY for `-forceres`
  (`GRAPHICS_FS_CUSTOM_RESOLUTION`, fullscreen), `-forceresswap`, or in `-w`
  mode when `-windowresize`/screenresize (`GRAPHICS_WINDOW_BACKBUFFER_SCALE` +
  `GRAPHICS_WINDOW_SIZE` / `cfg::SCREENRESIZE`) are set. Otherwise the game's
  dims pass through unchanged (H4 closed). `-w` forces `Windowed = TRUE` and
  `FullScreen_RefreshRateInHz = 0`.
- Window sizing under `-w` is the game's own `SetWindowPos(client = screen_w ×
  screen_h)` branch (research §2.2) — a 3840×2160 window on a smaller desktop
  is the operator's problem (same as any game).
- `-o` "DDR 4:3 Mode" (`games::ddr::SDMODE`): fakes SetupAPI monitor
  "Generic Television" (the same string gamemdx carries at `18035bb90` in a
  5-entry monitor-name table) and P3IO cmd `0x27` cabinet type `0x00`. No bio2
  effect. CreateDevice failure text explicitly suggests "run in SD mode" only
  for pop'n.
- Conflict rule for the design: the mod owns dims; document that `-forceres` /
  `-windowresize` / screenresize must not be combined (or detect: read back the
  created device's back-buffer desc and WARN once).

## 4. AOB sweep — every research §9 pattern verified on all four builds + live

`prototypes/aob_sweep/REPORT.md`: 6 patterns × exactly 1 hit and the list-
viewport table × 2 overlapping hits on 20250805 / 20260224 / 20260721 /
20260825 (the live install = 20260825 by MD5). 20260224 (previously "—" in the
doc) now has addresses for all seven. Patcher-facing bytes identical everywhere:

| site | offsets (from match) | values |
|---|---|---|
| back-buffer select | +15 / +25 (HD) ; +37 / +47 (SD) | `0x500 / 0x2d0 ; 0x280 / 0x1e0` — 4 `C7 05` stores alternating two RIP targets (w, h globals) |
| letterbox rect | +1 ; `[RBX+0x298]=0x2d0` at **+0xA9** | `0x500` |
| hoisted 1280 (`FUN_1801f01a0`) | `MOV R15D,0x500` at +7 ; `MOV ESI,0x2d0` at **+0x2B** | |
| HD-flag store (NEW) | `85 D2 78 ?? 83 FA 01 7F 04 32 C0 EB 02 B0 01 88 44 24 ??` | unique on all builds; `B0 01` at +12 (`MOV AL,1` = HD) — forcing `AL` for both branches gives a machine-type-independent flag |
| AA-config store (NEW) | `C7 44 24 68 03 00 00 00` at **fps_target_imm32 + 0x69** | the bare shape has 18–40 hits — must be anchored to the existing `fps_target_imm32` signature |

The guessed `CMP EAX,1; SETA` shapes had zero hits — the real HD-flag code is
`TEST/JS/CMP/JG` + `XOR AL,AL / MOV AL,1` (see above and `research/sd-cabinet-path.md`).
