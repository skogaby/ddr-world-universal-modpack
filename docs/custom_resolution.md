# Custom Resolution — implementation record and RE facts

Mod: `src/mods/custom_resolution/` (id `custom-resolution`, default OFF). Feature
planning: `.agents/planning/2026-09-05-arbitrary-resolution/` (register
`idea-honing.md`, design `design/detailed-design.md`, cabinet history
`progress.md`). Engine research this rests on: `arbitrary_resolution_research.md`
(20260616 `FUN_` names; this file uses **20260825**, the live build, where it
names functions). Addresses are file-relative to `0x180000000`.

Shipped 2026-09-07 after four cabinet checkpoints (SD 640×480 crop/letterbox;
Tier A 1080p output / 720p render; Tier B native 1080p + 4K; perf mode 4K output
/ 1080p and 75 % render). **Revised 2026-09-09 to ONE knob** (§1a): the render ≠
output "perf mode" and the MSAA setting were removed after a 1080p-stutter report
traced to the present chain they forced (§3a). Everything is boot-time byte
patching of immediates the game reads once plus four detours; settings apply at
the NEXT launch.

## 1. Model

- **output** = D3D9 back-buffer / display surface / DISPLAY viewport / SYSTEM lists
  (what the panel receives). **render** = the size of the game's internal
  "1280×720" surfaces and the six content list viewports (what geometry
  rasterises at). The **logical 1280×720 canvas** never changes: every 2D draw
  handler converts `ndc = (x / canvas_w + origin / vp_w)·2 − 1`, so content is
  resolution-independent once the surfaces are bigger.
- `plan.rs` (pure, host-tested via `scripts/validate_custom_resolution.sh`):
  `Inert` iff output is stock; 16:9 (`|9w − 16h| ≤ 16`) or 4:3 (`|3w − 4h| ≤ 12`),
  else `Rejected`; `h ≥ 360`, sides ≤ 8192 (u16 dims everywhere), even dims.
  **render is derived** (`render_for`): 16:9 ⇒ render == output, 4:3 ⇒ the stock
  1280×720. `present_policy`: `Stock` (16:9 — the engine's own `screen_w ==
  render_w` 1:1 POINT branch, or direct mode with no copy at all) or
  `Sd(Crop|Letterbox)`. `aa`: `Stock` for 16:9 (the game's choice is kept),
  `ForceOff` for 4:3. The render always covers the output, so the PRESENT depth
  is never touched (stock SD cabinets bind a 720p depth to a 480p colour target).

## 1a. Why one knob (2026-09-09)

A 1080p stutter report on mid-range cabinet hardware that runs other native-1080p
Bemani titles cleanly prompted a cost audit. The mod adds no per-frame CPU (the
scissor detour is dormant on stock content, the letterbox detour fires per scene,
everything else is boot-time), so the delta had to be GPU-side — and one piece of
it was ours, not the resolution's: `msaa: "off"` (the former default) forced the
game's AA config 3 → 0 on every pcType-2..4 HD cabinet (and spice2x), which
kicks the engine out of its "direct" present chain into the offscreen-composite
one (§3a: two extra `StretchRect`s, a full-screen clear, a depth-copy quad and
two extra RT switches per frame, at output resolution). That policy only existed
to make render ≠ output work (direct mode has no scaler). Neither render ≠
output nor user-selectable MSAA had a use case the maintainer wanted to keep, so
both knobs are gone: 16:9 keeps the game's own AA/present chain, 4:3 (the only
render ≠ output case left, hard-coded) forces 0 because the SD crop/letterbox
scaler lives in the mode-0 chain — and stock SD cabinets run 0 anyway (onBoot
picks 3 only when the HD flag is set). Removed with them: `ForceLetterbox`,
`PresentDepth::CreateOutputSized` + the `surface_create` / `present_depth_*`
derivations, the `GATES`, the RENDER SCALE and MSAA rows, and the `render` /
`msaa` config keys (stale keys are ignored — the config has no
`deny_unknown_fields`).

## 2. Boot flow (`mod.rs::early_apply`, before `Application::onBoot` reaches display init)

1. config → `plan::compute` → too-late check (`screen_w_global == 0`) →
   `display_modes::validate` (fullscreen fail-safe via `EnumDisplaySettingsW`;
   SKIPPED under spice2x `-w`, detected from the process command line — the Mac's
   desktop enumerates no 4K mode).
2. **OUTPUT set** (`patches::apply_output_set`, 6 writes at 16:9 / 7 at SD): the
   four back-buffer selector imms (`display_backbuffer_dims` — HD AND SD branch,
   so the machine type stops mattering), the window-descriptor client size
   (`window_client_size` — `main` builds the window BEFORE display init) and, for
   4:3 ONLY, the AA config imm (`aa_config_imm` 3 → 0; the graphics_init detour's
   struct write covers the other onBoot branches). 16:9 never touches AA.
3. **RENDER set** (`apply_render_set`, 22 writes, empty when render is stock):
   group 3 surface ctor hoisted `MOV R15D,0x500` / `MOV ESI,0x2d0` + the six
   RT-struct dim stores (`C7 41 14 00 05 D0 02` ×3 → `w|h<<16`, `C7 41 14 00 05 00
   05` ×1 OFFSCREEN1 → `w|w<<16`, `C7 40 16 D0 02 00 00` ×2 → `h`, FIRST = PRESENT);
   group 4 the list-viewport stack table (`list_viewport_table`: 5 wide pairs + 1
   square pair of `C7 85 disp32 imm32`); group 5 letterbox source rect
   (`letterbox_rect_fn` +0x35 `MOV EDX,0x500` — also the equality comparand — and
   `[RBX+0x298] = 0x2d0` at +0xE3). Site finders are pure (`sites.rs`), every imm
   stock-verified before the first write.
4. **Scissor detour** (`scissor.rs`, render ≠ stock only) — see §4.
5. **All-or-nothing:** any RENDER-set / scissor / graphics-init failure rolls
   back BOTH sets ⇒ byte-identical stock boot + one WARN. A letterbox-detour miss
   only degrades SD LETTERBOX to the stock crop (WARN, no rollback).
6. `present::install` — `graphics_init` detour (§3); `letterbox::install` —
   present-mode policy detour (`Stock`/`Sd(Crop)` install nothing);
   `logical_screen::install` (§5).
7. `enable()` re-states everything as one `boot state -- …` INFO because spice2x's
   `debughook` often attaches AFTER `early_apply` has logged (see learnings
   2026-09-07) — that line is the cabinet-triage anchor. It is followed by the
   one-shot **`present chain -- aa_config=N (onBoot chose M) -> <shape>`** INFO
   (`plan::present_chain_shape`; deferred to the first scene change when
   `enable` beats `graphics_init`, which it does on some boots) — the
   PERFORMANCE-triage anchor: a 16:9 plan on a pcType-2..4 cabinet must read
   `aa_config=3 … -> direct (3)`; `0` there means the machine is paying for the
   offscreen-composite chain (§3a).

## 3. PRESENT fixup (`present.rs`, post-`graphics_init`)

Pre-original: for the 4:3 plan write AA 0 into the display struct `+0x18`
(covers every onBoot branch); for 16:9 leave it. Record onBoot's value and the
effective value (`present::aa_config()`) for the §2 step-7 line, and log the
struct (`hd_flag`, `aa_config`, `fps`) + `present_chain_shape` right there too.

Post-original: PRESENT rt = `*(render_surfaces_global + 0x80)`; RT struct
`{+0x08 colour id, +0x10 depth id, +0x14 u16 w, +0x16 u16 h, +0x18 u8 msaa}`
(0x1C). The ctor sizes it with the (patched) render imms; the present chain
re-points its colour at the output-sized back-buffer every frame, so
`+0x14/+0x16` become the OUTPUT dims here (gate: they must read the render dims
first; a no-op for 16:9 where render == output). The depth stays the stock
`render_depth` — the render now always covers the output.

Retained RE (the removed render < output depth swap, cabinet-proven 2026-09-07,
in case it is ever needed again): D3D9 needs a depth ≥ the colour target, and the
ctor's own idiom (20260825 `FUN_1801f10e0` @ +0xD13, binding `render_depth`) is
`new = surface_create(out.w, out.h, 0x4b, msaa 0, &{u32 0, u8 0})` (**5 args** —
the 5th is a pointer to a zeroed options block the ctor passes for every surface;
`FUN_180250950(u16 w, u16 h, u32 fmt, u32 msaa, opts*) -> u32 id`; it is the
`E8` at `render_surface_hoist`+0x13), `if old { slot = 0; release(old) }`,
`slot = new; addref(new)` (`release` = `FUN_180250be0(id)`, `addref` =
`FUN_180250b30(id)` — the 2nd/3rd CALL after the ctor's first `C7 40 16` PRESENT
dim store). Cabinet: `depth replaced: id 0xA02D2 -> 0x160F72 (3840x2160)`.

## 3a. Per-frame present chain by AA config (20260825 RE, 2026-09-09)

The game's AA config (`display struct +0x18` → `DAT_1806f150c` on 20260825) is
0 on SD / non-pcType-2..4 machines and **3 ("direct") on every pcType-2..4 HD
cabinet and under spice2x** (onBoot: `1 < pcType < 5 && HD`). From the surface
ctor `FUN_1801f10e0` and the three `AfterRenderConditionImpl` vfuncs
(`FUN_1801f5250` BEGINVIEWPORT / `FUN_1801f5580` COPYVIEWPORT / `FUN_1801f5670`
ENDVIEWPORT; the vftables at `0x180388428/440/458`), the fixed per-frame work
BEYOND content, per mode:

| | mode 3 direct (stock HD) | mode 0 offscreen composite (forced by the old `msaa: off`) |
|---|---|---|
| RENDER (3D) + RENDER_2D target | `display` (screen-sized) directly | `+0xc4` RENDER colour, then `render_color` |
| AFTER RENDER 3D (BEGINVIEWPORT) | one in-place `sys_copy_aa` quad (ps 57 instr / 9 texld) | gd `0x31` **StretchRect** `+0xc4 → render_color` + `sys_copy_depth` quad sampling the readable-depth `D24R` surface (colour-write 0) |
| DISPLAY (COPYVIEWPORT) | **skipped** (`FUN_1801f5580` early-outs on 3) | gd `0x14` **clear** `display` + gd `0x31` **StretchRect** `render_color → display` |
| PRESENT (ENDVIEWPORT) | `sys_copy` quad → back-buffer | same |
| Viewport clears (walker `FUN_180272600`, `vp+0x40 & 2`) | OFFSCREEN1 colour, RENDER c+d+s, RENDER_2D depth | same |
| RT switches / frame | ~3 | ~5 |
| Readable-depth `D24R` in use | no | yes (`rt[0xc]`/`rt[0xd]` depth) |
| Full-screen fixed ops | 2 | 5 (modes 1/2 add a resolve `StretchRect` on multisampled surfaces: 6) |

At 1080p each extra op is ~2.07 Mpx (≈ 8.3 MB read + 8.3 MB write per blit).
Mode 0's extra cost is bandwidth (+2 RT switches + the depth-decompress path
that `D24R` reads imply on many GPUs) rather than ALU; mode 3's `sys_copy_aa` is
57 instructions but a single pass. With `BackBufferCount=1` +
`D3DPRESENT_INTERVAL_ONE` any frame over budget is a whole missed vsync, so a
few ms of fixed overhead is exactly the difference between hitching and smooth
on a near-budget iGPU. Also audited and ruled out as resolution-specific:
`shader_fixes`' arrow/judge AA pixel shaders (30/27 instr vs stock 7/6 — a ~4×
per-pixel tax on lane pixels only, the same ratio at every resolution), the
scissor/letterbox/logical-screen detours (no per-frame work), VRAM (~105 MB of
RTs at 1080p). Untested candidates if a cabinet still hitches on mode 3:
OFFSCREEN1 at `render.w²` is cleared every frame (3.7 Mpx at 1080p) — a stock
1280² OFFSCREEN1 would be a two-imm config knob pending a probe of what entry
10 renders; `D3DPRESENTFLAG_LOCKABLE_BACKBUFFER` (initGs `MOV [RSP+0x44],1`)
is a documented driver perf cost and nothing obviously locks the back-buffer.

Also here: `fit_window` — spice2x's `CreateWindowExW` hook (MDX, `-w`) REPLACES
the requested client size with 1280×720 (800×600 under its `-o`) and its
`SetWindowPos` IAT hook swallows the game's later resizes, so the client is
resized through `user32!SetWindowPos` resolved by `GetProcAddress`. And the R14
check: screen globals ≠ output after init ⇒ WARN (mode fallback / `-forceres`).

## 4. Scissor (`scissor.rs`)

Tag-0x0C handler `(walker_ctx**, record*)` (20260825 `FUN_1802692e0`): record
`+4` u16 enable, `+6/+8/+A/+C` u16 x,y,w,h copied VERBATIM into gd 0x18 →
`SetScissorRect` (RT pixels). The detour rescales with the draw handlers' own
math: `ctx = *walker` (`+0x00` offset `{ox/vp_w, oy/vp_h}`, `+0x10` scale
`{1/canvas_w, 1/canvas_h}`, written by the tag-0x07 handler `FUN_180268ea0`),
`gd = walker[1]` (`+0x144/+0x146` u16 viewport w/h from the segment header);
`plan::scissor_scale` → write rect, call original, RESTORE the 8 bytes (lists are
re-walked). Passthrough on disabled records / zero viewport / non-positive scale.

**Observed:** four 4K cabinet runs through title, attract, song select WITH the
options modals, gameplay (movie), results never dispatched a scissor record —
`ScreenRoot::render` (`FUN_1802180a0`) emits tag 0x0C only when the root's
`+0x60` flag is set (sole writer: vtable slot 9 `FUN_180217de0`; ctor default 0),
and the options modal is AFP content (clipping is AFP-side). The detour is a
correctly installed safety net; a one-shot `scissor handler first dispatch` INFO
will say if anything ever reaches it.

## 5. Logical screen (`logical_screen.rs`) — D5 v3

The game keeps a per-display-info block (w/h) behind a POINTER global; 38
`MOV r64,[RIP+disp32]` loads of it exist. Classified by content + anchor and the
disp32 redirected to mod-owned pointer slots (near-alloc): **design (4)** — the
two screen w/h getters the layer set-size loop uses for roots 1/3/5/6/7 + the
footer text setup + the loading text — read a constant `{1280,720}` so every
screen-sized layer root is a 1280×720 canvas the walker scales to the viewport;
**render (4)** — the AFP projection callback, BM2DGroup ctor/ctx reset — read
`{render}` (the bm2d VS DOES consume c50–c53, H1 refuted); **physical (30; 29 on
20250805)** — surface ctor, letterbox, list viewports, frame-begin bind, gd
device layer, the ark draw-callback API (`screen × pct / 100`, RIP-relative
`100.0f` within [−0x60,+0x80)) and the system font (`DIVSS [r+0x74/0x78]`) — stay
on the real block. The install REFUSES on any other family shape. This replaced
two failed attempts (re-canvasing root 7 alone; scaling the DLL's widgets) —
`progress.md` checkpoint #2 runs 1–4 record why.

## 5a. Debug-UI scale (`debug_ui.rs`) — TEST menu / hardware check / error screens

The ark drives these through gamemdx's draw-callback table (20260825
`FUN_1800067f0..FUN_180008170`). Positions, lines and fill rects are in screen
PERCENTAGES (`screen × pct / 100`, physical family in §5 — they follow the
output for free). Two things are fixed PIXEL sizes and never consult the
back-buffer: `createFont` (`FUN_1800069c0`) calls the scale chooser
`FUN_1800066d0(class, &sx, &sy)`, which asks `arkMDXGetMachineType`
(`DAT_1806f2330` on 20260825; `DAT_1806f2338` is `arkMDXGetPCType`) and picks a
480-line table (machine 0/1 = SD cabinet: class 0 → 0.95, 1 → 0.8, 2 →
0.75×0.7) or a 720-line table (1.5 / 1.2 / 1.0; other classes 1.0), stored into
the agcs text object's `params+0x58/+0x5C`; `createSprite` (`FUN_180007250`)
stores 0.8 (SD) / 1.0 (HD; `screenCheck` always 1.0) into `sprite+0x08`, and
`drawSprite` sizes the quad `texture_px × scale`. Hence 1.5× text at 640×480
and third-size text at 4K.

Fix: post-original detours on both entries (signatures `debug_font_scale`,
`debug_sprite_create` — entry AOBs, unique on all four builds, byte-identical
bodies on 20250805) multiply the game's value by `plan::debug_ui_scale` =
`output_h / ref_h × test_menu_scale`, `ref_h` = 480 when the SAME export
reports machine 0/1, else 720 (resolved lazily on the first call — the ark is
the caller, so it is loaded; unresolvable ⇒ 720 + one WARN). Identity at 720p
⇒ nothing installed. Config `resolution.test_menu_scale` (0.25..=4, default 1)
is an operator multiplier on top. HD machine at 640×480 lands at 1.0 (Konami's
SD table was 0.95); an SD machine at 4K at 4.5. Cosmetic — a miss never rolls
the plan back. Boot line: `debug-UI scale detours installed (output N lines, …)`
and one `debug font class … scale a×b -> c×d (factor f)` INFO per boot.

## 6. Letterbox / present policy (`letterbox.rs`)

`letterbox_rect_fn(this, int mode)` (20260825 `FUN_1801f5010`): `screen_w ==
<imm>` ⇒ 1:1 POINT copy; mode 1 ⇒ 960-px centre crop (SD cabinets shipped with
it; re-selected at EVERY scene transition by `FUN_18002e7b0`-class callers ×13);
else width-fit LINEAR letterbox (full-screen for matching aspect; the TEST menu
asks for 0). The detour maps the mode per `plan::present_mode` on every call and
is installed for `Sd(Letterbox)` only (16:9 never enters this function's scaling
branches: render == output takes the equality branch, and direct mode skips the
COPYVIEWPORT entirely).

## 7. Signatures (all unique + byte-identical bodies on 20250805 / 20260224 / 20260721 / 20260825)

`display_backbuffer_dims`, `window_client_size`, `render_surface_hoist`,
`list_viewport_table`, `letterbox_rect_fn`, `scissor_handler`,
`debug_font_scale`, `debug_sprite_create`; derived
(`derive_custom_resolution`): `aa_config_imm`, `graphics_init`,
`render_surfaces_global`, `screen_w/h_global` (`C7 05 disp32 imm32`: RIP is
AFTER the imm — `decode_rip_relative(disp) + 4`). `CustomResolutionAnchors`
re-derives the subset `early_apply` needs before `resolve_derived` runs. (The
`surface_create` / `present_depth_release/addref` derivations went with the
depth swap — §3 keeps the shape.)

## 8. Hypothesis outcomes (research §10)

- H1 REFUTED offline (bm2d VS reads c50–c53) → the render family of §5.
- H2 CONFIRMED benign on CrossOver/D3DMetal (720p depth under a 1080p/4K colour
  target rendered), fixed anyway for real D3D9 by §3.
- H3 no read-back consumer surfaced through gameplay → results at 4K (native and
  1080p-render); photo/upload path not separately exercised.
- H4 spice2x forwards the back-buffer size unchanged; its `-w` window pinning is
  the reason for `window_client_size` + `fit_window`.
- H5 the live cabinet (CrossOver) boots with AA config 3. Through 2026-09-08 the
  mod forced it to 0 (`… aa_config=0 (onBoot chose 3)`); since 2026-09-09 16:9
  plans keep the 3 and only the 4:3 plan forces 0 (§1a, §3a). The `msaa` knob
  (2×/4×, never cabinet-tested) is gone.
- H7 1.5× softness was visible in Tier A (LINEAR `StretchRect`); Tier B is crisp
  — one of the reasons Tier A / perf mode was dropped.
- H8/H9 hold (u16 dims; 4K on D3DMetal playable).

## 9. Phase-2 shader scaler — decision

Dropped. Tier B (render == output) is the only 16:9 configuration now; the
render < output perf mode it would have served no longer exists (§1a).

## 10. Gotchas

- Never touch the `1280.0f/720.0f` rodata, `ScreenRoot` defaults or
  `DAT_18046091c/DAT_180464108` — the logical canvas (`cull_window` verifies them).
- `C7 05 disp32 imm32` stores: RIP is AFTER the imm32.
- `surface_create` is FIVE args; NULL for the 5th substitutes an unverified
  global default block — pass the zeroed `{u32,u8}` the ctor passes.
- The RENDER set runs before `resolve_derived`: any derivation anchored on a
  stock immediate the set rewrites must accept the patched value (the
  `aa_config_imm` 3-or-0 precedent).
- Never reintroduce a policy that writes AA 0 for a 16:9 plan — that is the
  present-chain regression §3a exists to document. Direct mode is safe at any
  output size because the RENDER set makes `render_depth` output-sized.
- The DLL's early_apply INFO/WARN lines may be absent from `log.txt` (debughook
  attach race) — read the `boot state` line and the `debughook: attached` line
  number before concluding a step did not run.
- Do not combine with spice2x `-forceres` / `-windowresize`.
