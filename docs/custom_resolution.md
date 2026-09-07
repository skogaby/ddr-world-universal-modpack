# Custom Resolution — implementation record and RE facts

Mod: `src/mods/custom_resolution/` (id `custom-resolution`, default OFF). Feature
planning: `.agents/planning/2026-09-05-arbitrary-resolution/` (register
`idea-honing.md`, design `design/detailed-design.md`, cabinet history
`progress.md`). Engine research this rests on: `arbitrary_resolution_research.md`
(20260616 `FUN_` names; this file uses **20260825**, the live build, where it
names functions). Addresses are file-relative to `0x180000000`.

Shipped 2026-09-07 after four cabinet checkpoints (SD 640×480 crop/letterbox;
Tier A 1080p output / 720p render; Tier B native 1080p + 4K; perf mode 4K output
/ 1080p and 75 % render). Everything is boot-time byte patching of immediates the
game reads once plus four detours; settings apply at the NEXT launch.

## 1. Model

- **output** = D3D9 back-buffer / display surface / DISPLAY viewport / SYSTEM lists
  (what the panel receives). **render** = the size of the game's internal
  "1280×720" surfaces and the six content list viewports (what geometry
  rasterises at). The **logical 1280×720 canvas** never changes: every 2D draw
  handler converts `ndc = (x / canvas_w + origin / vp_w)·2 − 1`, so content is
  resolution-independent once the surfaces are bigger.
- `plan.rs` (pure, host-tested via `scripts/validate_custom_resolution.sh`):
  `Inert` iff both stock; 16:9 (`|9w − 16h| ≤ 16`) or 4:3 (`|3w − 4h| ≤ 12`), else
  `Rejected`; `h ≥ 360`, sides ≤ 8192 (u16 dims everywhere), even dims; 4:3
  coerces render → 1280×720 (INFO). `present_policy`: `Stock` (render == output
  ⇒ the engine's own `screen_w == render_w` 1:1 POINT branch), `ForceLetterbox`
  (16:9, render ≠ output), `Sd(Crop|Letterbox)`. `present_depth`:
  `CreateOutputSized` iff the render does not cover the output.
  `GATES { letterbox_policy, native_render }` both `true` since Step 6 — flip one
  to cut a build back to a known-good subset.

## 2. Boot flow (`mod.rs::early_apply`, before `Application::onBoot` reaches display init)

1. config → `plan::compute` → too-late check (`screen_w_global == 0`) →
   `display_modes::validate` (fullscreen fail-safe via `EnumDisplaySettingsW`;
   SKIPPED under spice2x `-w`, detected from the process command line — the Mac's
   desktop enumerates no 4K mode).
2. **OUTPUT set** (`patches::apply_output_set`, 7 writes at 16:9 / 5 at SD): the
   four back-buffer selector imms (`display_backbuffer_dims` — HD AND SD branch,
   so the machine type stops mattering), the window-descriptor client size
   (`window_client_size` — `main` builds the window BEFORE display init), the AA
   config imm (`aa_config_imm` 3 → 0 unless `msaa: "stock"`).
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
5. **All-or-nothing:** any RENDER-set / scissor / graphics-init / letterbox
   failure rolls back BOTH sets ⇒ byte-identical stock boot + one WARN. (A Tier-A
   degrade was rejected: the present policy was computed for the failed plan.)
6. `present::install` — `graphics_init` detour (§3); `letterbox::install` —
   present-mode policy detour (`Stock`/`Sd(Crop)` install nothing);
   `logical_screen::install` (§5).
7. `enable()` re-states everything as one `boot state -- …` INFO because spice2x's
   `debughook` often attaches AFTER `early_apply` has logged (see learnings
   2026-09-07) — that line is the cabinet-triage anchor.

## 3. PRESENT fixup (`present.rs`, post-`graphics_init`)

PRESENT rt = `*(render_surfaces_global + 0x80)`; RT struct `{+0x08 colour id,
+0x10 depth id, +0x14 u16 w, +0x16 u16 h, +0x18 u8 msaa}` (0x1C). The ctor sizes
it with the (patched) render imms; the present chain re-points its colour at the
output-sized back-buffer every frame, so `+0x14/+0x16` become the OUTPUT dims here
(gate: they must read the render dims first). Depth (R12): when render < output
in either dimension D3D9 needs a depth ≥ the colour target, so `replace_depth`
runs the ctor's own idiom (20260825 `FUN_1801f10e0` @ +0xD13, binding
`render_depth`): `new = surface_create(out.w, out.h, 0x4b, msaa 0, &{u32 0, u8 0})`
(**5 args** — the 5th is a pointer to a zeroed options block the ctor passes for
every surface; `FUN_180250950(u16 w, u16 h, u32 fmt, u32 msaa, opts*) -> u32 id`),
`if old { slot = 0; release(old) }`, `slot = new; addref(new)` (`release` =
`FUN_180250be0(id)`, `addref` = `FUN_180250b30(id)`, derived as the 2nd/3rd CALL
after the PRESENT dim store — the derivation matches `C7 40 16 ?? ?? 00 00` because
the RENDER set has already rewritten the height by then). Missing derivation /
create → 0 ⇒ depth NULLED + WARN (the PRESENT pass is a Z-off textured quad).
Cabinet: `depth replaced: id 0xA02D2 -> 0x160F72 (3840x2160 output-sized)`.

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

## 6. Letterbox / present policy (`letterbox.rs`)

`letterbox_rect_fn(this, int mode)` (20260825 `FUN_1801f5010`): `screen_w ==
<imm>` ⇒ 1:1 POINT copy; mode 1 ⇒ 960-px centre crop (SD cabinets shipped with
it; re-selected at EVERY scene transition by `FUN_18002e7b0`-class callers ×13);
else width-fit LINEAR letterbox (full-screen for matching aspect; the TEST menu
asks for 0). The detour maps the mode per `plan::present_mode` on every call.
With render ≠ output the engine's `StretchRect` is the scaler (Phase 1 of D8).

## 7. Signatures (all unique + byte-identical bodies on 20250805 / 20260224 / 20260721 / 20260825)

`display_backbuffer_dims`, `window_client_size`, `render_surface_hoist`,
`list_viewport_table`, `letterbox_rect_fn`, `scissor_handler`; derived
(`derive_custom_resolution`): `aa_config_imm`, `graphics_init`,
`render_surfaces_global`, `screen_w/h_global` (`C7 05 disp32 imm32`: RIP is
AFTER the imm — `decode_rip_relative(disp) + 4`), `surface_create`,
`present_depth_release/addref`. `CustomResolutionAnchors` re-derives the subset
`early_apply` needs before `resolve_derived` runs.

## 8. Hypothesis outcomes (research §10)

- H1 REFUTED offline (bm2d VS reads c50–c53) → the render family of §5.
- H2 CONFIRMED benign on CrossOver/D3DMetal (720p depth under a 1080p/4K colour
  target rendered), fixed anyway for real D3D9 by §3.
- H3 no read-back consumer surfaced through gameplay → results at 4K (native and
  1080p-render); photo/upload path not separately exercised.
- H4 spice2x forwards the back-buffer size unchanged; its `-w` window pinning is
  the reason for `window_client_size` + `fit_window`.
- H5 the live cabinet (CrossOver) boots with AA config 3 → forced 0 by the OUTPUT
  set (`graphics_init display struct: … aa_config=0`). `msaa` is now a real
  choice: `off`/`2x`/`4x` are written into the display struct `+0x18` by the
  `graphics_init` detour pre-original (covers every onBoot branch — the imm patch
  only reaches the pcType-2..4 `MOV [RSP+d],3`); `stock` keeps the game's value
  and is refused when render ≠ output (mode 3 skips the present-chain scaler).
  2×/4× on CrossOver/D3DMetal is untested as of 2026-09-07.
- H7 1.5× softness is visible in Tier A (LINEAR `StretchRect`); Tier B is crisp.
- H8/H9 hold (u16 dims; 4K on D3DMetal playable).

## 9. Phase-2 shader scaler — decision

Deferred. Tier B (render == output) is the recommended configuration on every
panel the maintainer tested and needs no scaler; the render < output perf mode
is a CrossOver escape hatch whose LINEAR softness was judged acceptable. Revisit
only if a cabinet needs the perf mode as its daily configuration.

## 10. Gotchas

- Never touch the `1280.0f/720.0f` rodata, `ScreenRoot` defaults or
  `DAT_18046091c/DAT_180464108` — the logical canvas (`cull_window` verifies them).
- `C7 05 disp32 imm32` stores: RIP is AFTER the imm32.
- `surface_create` is FIVE args; NULL for the 5th substitutes an unverified
  global default block — pass the zeroed `{u32,u8}` the ctor passes.
- The RENDER set runs before `resolve_derived`: any derivation anchored on a
  stock immediate the set rewrites must accept the patched value (the
  `aa_config_imm` 3-or-0 and `present_depth_*` `?? ??` precedents).
- The DLL's early_apply INFO/WARN lines may be absent from `log.txt` (debughook
  attach race) — read the `boot state` line and the `debughook: attached` line
  number before concluding a step did not run.
- Do not combine with spice2x `-forceres` / `-windowresize`.
