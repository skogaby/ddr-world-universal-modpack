# Progress — Custom Resolution (arbitrary resolution rendering)

Updated: 2026-09-05
Status: Step 6 of 8 — not started (Steps 1–5 complete, checkpoints #1 and #2 passed)
NEXT ACTION: plan Step 6 — native render (Tier B): patch groups 3 (surfaces) / 4 (list viewports) / 5
(letterbox src) via `sites.rs` as an atomic RENDER set in `patches.rs`, the `scissor_handler` detour
(`scissor.rs`, design §4.8), flip `plan::GATES.native_render = true` (+ T17), then the fresh checkpoint #3
(1920×1080 and 3840×2160 with `render = output`).

Resume protocol: read this file, then `implementation/plan.md` (checklist), then
`design/detailed-design.md` (§4 components), then `idea-honing.md` (register) and
`research/*.md` only when a design claim needs its evidence.

## Done

- PDD Steps 1–7: workspace, orientation, register (accepted wholesale 2026-09-05),
  research (SD path via Ghidra, H1 refuted by shader bytecode, `sys_copy` shape,
  spice2x source, four-build AOB sweep), readiness confirmed, design + plan
  approved (maintainer pre-authorized autonomous continuation to the first
  cabinet checkpoint = plan Step 3).
- Impl Steps 4+5 (checkpoint #2 passed after 5 runs): `letterbox.rs` (present-mode policy detour),
  `rows.rs` (RESOLUTION / RENDER SCALE overlay rows), `logical_screen.rs` (D5 v3 — the app layer's
  per-display-info readers see 1280×720, AFP callbacks see render, renderer/device see output), README;
  `GATES.letterbox_policy = true` (Tier A live). Record: `.agents/scratchpad/.../step04-05-canvas-rows-letterbox/`.
- Impl Step 1: `plan.rs` (16 host tests via `scripts/validate_custom_resolution.sh`),
  `ResolutionConfig` section, module wiring; `cargo check` clean. Working record:
  `.agents/scratchpad/2026-09-05-arbitrary-resolution/step01-plan-model/`.
- Impl Step 3 (code): anchors accessor, `patches.rs`, `present.rs`, `display_modes.rs`, `mod.rs`,
  lib.rs registration, config section. Record: `.agents/scratchpad/.../step03-output-path/`.
- Impl Step 2: `sites.rs` (7 fixture tests), 5 AOBs + `derive_custom_resolution` (8 derived
  names); `validate_signatures.sh` ALL GREEN on all four builds, `shape_diff.py` identical
  through 0x1200. Record: `.agents/scratchpad/.../step02-signatures/`.

## In flight

- Step 6 — not started.

## Deploy & test log

### Checkpoint #1 run 1 (Step 3) — SD 640×480, CrossOver window — PARTIAL (2026-09-05)
Maintainer: back-buffer 640×480 + SD crop rendered correctly, BUT the window stayed 1280×720 and the
picture was stretched into it. Log confirmed every expected line except two:
1. `screen globals read 480x0` — derivation off by 4: `C7 05 disp32 imm32` puts RIP AFTER the imm32,
   so `decode_rip_relative(disp)` must be `+4`. Fixed in `custom_resolution_anchors` (w = +0x6F1524,
   h = +0x6F1520 on 20260721/0825, matching the research doc's DAT_1806f0524/0520).
2. Window size: the game's `main` (FUN_180003bf0) builds the window descriptor with a hard-coded
   1280×720 CLIENT size before display init (`AdjustWindowRectEx` + `CreateWindowExW(1290,756)`);
   spice2x's `SetWindowPos_hook` swallows every later game SetWindowPos for MDX in `-w`, so the client
   never follows the back-buffer and D3D stretches. Fix: new AOB `window_client_size` (unique on all
   four builds) + `sites::window_client_sites`; the OUTPUT set now also rewrites those two imms
   (5 writes for SD: hd_w/hd_h/window_w/window_h/aa). Fullscreen ignores the client size.
Rebuilt (fmt/harness 24/24/sweep ALL GREEN/build clean). → run 2 pending.

### Checkpoint #1 run 2 — PARTIAL (2026-09-05)
Log: window now CREATED at 650×516 (`CreateWindowExW hook hit (…, 650, 516, …)`), 5 writes applied, no
WARNs — yet the visible window was still 1280×720. Cause (spice2x source, `graphics.cpp`
`CreateWindowExW_hook`): for MDX in `-w` spice2x REPLACES the requested client size with a hard-coded
1280×720 (800×600 under its `-o` SD flag) regardless of what the game asked, and swallows later game
`SetWindowPos` calls. So the game-side `window_client_size` patch is correct for stock/fullscreen but
spice2x windowed mode needs a post-init fit: `present::fit_window` (after `graphics_init`) compares
`GetClientRect` with the output and, when different, resizes via `user32!SetWindowPos` resolved by
`GetProcAddress` (bypasses spice2x's IAT hook). Expected new log line:
`window client 1280x720 -> requested 640x480 (SetWindowPos=1, now 640x480) -- a loader pinned the window size`.

### Checkpoint #2 run 1 — PARTIAL (2026-09-05, logs `log_a/b/c.txt` in the install)
A/B (SD crop / letterbox): root-7 re-canvas WORKS — mod menu, toasts, PUS widgets, training timeline at the
right places; loading screens now show whole (were top-left-cropped, which is what a stock SD cabinet shows
since Konami removed the `_sd` layouts). NEW REGRESSION: the game's own system sprites/text in root 7
(ONLINE/PASELI/CREDIT footer, version string, "Insert coin(s)", title logos) shrank to 0.5× and pulled
inward — that class converts DESIGN→SCREEN px (`screen × x / ref`) and now draws into a 1280 canvas.
Fix: `sysfont.rs` redirects the 3 per-display-info loads of that class (module window around the new
`sysfont_set_position` AOB; float-use gated; count must be exactly 3) + the loading-layer placement load
(anchored by the "NOW LOADING ... %d%%" string) to a fake {1280,720} block — verified offline on all four
builds (3+1 sites each, addresses match Ghidra). Everything else in the log clean (letterbox detour,
`present mode 1 -> 0`, `SYSTEM root re-canvased … (frame 0)`).
C (Tier A 1080p): NOT actually exercised — the fail-safe refused: the Mac's logical desktop is 1512×982 and Wine
enumerates no 1080p mode, so the mod stayed at stock (log_c WARN). Fix: `display_modes::spice_windowed()`
(process command line carries `-w`) skips the mode check — a window may exceed the desktop. Rebuilt.

### Checkpoint #2 run 2 — PARTIAL (2026-09-05, `log_a_2.txt`, `log_c_2.txt`)
A: unchanged (footer/logos still 0.5×) despite the 4 redirected loads; C: Tier A applied (1080p window,
letterbox, PRESENT fixed) but the footer/logos drew OFF-SCREEN (1.5×). Ghidra on the footer setup
(`FUN_1800092d0`): those texts are agcs text objects positioned in SCREEN px from the per-display info +
the `FUN_180249070` getter and registered into layer 7 via `FUN_18002b060(7)` — the same manager our
widgets register into — and the loading art is bottom-right ANCHORED (`screen − 1280/720` offset), not
scaled. The screen-px readers feeding root 7 are many (footer setup, title logos, getters shared with the
layer set-size loop, debug drawers, sysfont …) and not safely classifiable across builds.
**D5 OVERRIDDEN (by evidence):** root 7 stays at its stock screen size; the DLL's widgets are scaled
instead — `widget_renderer::set_canvas_scale(output/1280, output/720)` at boot, applied in the six widget
setters (Text position/scale, Image position/size, image create, default text scale). Game content in
root 7 is now byte-for-byte stock. `canvas_fix.rs` and `sysfont.rs` (+ its AOB) removed.
Known consequence: the game's 1280-authored loading art in root 7 is stock-anchored (bottom-right) — at SD it
is cropped as on a stock SD cabinet, at 1080p/4K it covers the bottom-right 2/3. FOLLOW-UP (plan Step 8 / later):
scale the loading BM2DGroup itself.

### Checkpoint #2 run 3 — PARTIAL (2026-09-05, screenshots `~/Desktop/480_mod_menu.png`, `normal.png`, `1080.png`)
Widget write-scaling (run-3 build) came out DOUBLE-scaled in the mod menu (panel/text at 0.25×) while the
game footer stayed right; the version text SHRANK whenever the menu opened; at 1080p "Insert coin(s)" /
"PASELI can be used" sat at 2/3. Root cause of the double scale: `overlay_draw`'s mid-walk
`set_context_2d(1280,720)` (bg quad emission at the menu's anchor) switches the walker's canvas for
EVERYTHING after it in root 7's list — our widgets (written ×0.5) and the game's version text — while
root 7's own canvas was 640. (Explains the shrink-on-open too.) The 2/3 placements are design-authored
content in SCREEN-sized roots (1/3/5/6/7): stock engine behaviour once the `_sd` layouts are gone.
**D5 v3 — "logical screen":** instead of moving roots or widgets, classify the 38 readers of the
per-display-info POINTER and redirect (a) the app layer — the two `screen w/h` getters (which the layer
set-size loop uses for roots 1/3/5/6/7), footer/version/attract text, loading text, TEST-menu drawers,
system font (17–18 loads) — to a fake `{1280,720}` block, (b) the AFP callbacks + BM2DGroup rect (4) to
`{render}` (D19), leaving 16 physical loads (surface ctor, letterbox, list viewports, frame-begin,
gd device layer) untouched. Content+anchor classifier verified offline on all four builds (exact same
16-physical set each). `logical_screen.rs`; widget write-scaling reverted (widgets at design coords in a
1280 root again); no per-frame work.

### Checkpoint #2 run 4 — PASSED except one regression (2026-09-05)
Maintainer: every raised issue fixed (title layout, mod menu, loading screens whole, 1080p attract text).
Regression: the boot HARDWARE CHECK screen no longer scales/centres. Cause: the ark draw-callback API
(gamemdx's `createFont/drawFont/drawFontCenter/drawSprite/drawLine/drawFillRect/…` table handed to
arkmdxbio2 — `screen × pct / 100`) and the system font draw into a screen-sized BARE list whose walker
context is the physical viewport; feeding them 1280×720 doubled them. They were "design" only because they
convert to float. New classifier rules: a RIP-relative SSE reference to `100.0f` within [−0x60,+0x80) of the
load (the percentage divisor) ⇒ physical; `DIVSS xmm,[r+0x74/0x78]` (sysfont design divisor) ⇒ physical.
Design family is now EXACTLY 4 on every build (2 getters + footer + loading text), render 4, physical 29–30;
the install refuses on any other shape.

### Checkpoint #2 run 5 — PASSED (2026-09-05). Maintainer: results clean; Steps 4+5 marked complete.
A (SD) + C (1080p): everything from run 4 still right, PLUS the boot hardware-check screen centred/scaled
(as in the stock-SD/early builds) and the TEST menu (ark-drawn, same callbacks) readable at both sizes.
Log: `logical screen installed -- 4 app-layer load(s) read 1280x720 (design), 4 AFP load(s) read … (render);
30 untouched physical` (29 on 20250805).
A (SD crop): log `logical screen installed -- 18 app-layer load(s) read 1280x720 (design), 4 AFP load(s)
read 1280x720 (render); 16 untouched physical`. Title screen == `title_prev.png` layout (footer corners,
version top-left, logos right column at stock size); mod menu laid out like at 720p (no shrink); version
text unchanged when the menu opens; loading screens WHOLE (design art in a 1280 root); TEST menu readable.
C (1080p output / 720p render): "Insert coin(s)" + "PASELI can be used" at the bottom; footer in the corners;
loading screens whole; mod menu right. H2 depth experiment still stands. If anything is BLACK or crashes at
boot → a physical reader was misrouted: report the log's `logical screen installed` counts.
A (SD crop): footer/version/logos stock (as `title_prev.png`); mod menu/toasts/PUS/strip at the right places
(now via widget scaling); loading screens cropped top-left = stock SD behaviour. Log: `widget canvas scale
0.5000x0.6667 (SYSTEM root stays screen-sized)`.
C (Tier A 1080p): footer/logos in the corners at 1080p; mod menu etc. placed correctly; loading art at 2/3 in
the bottom-right (known); picture soft (LINEAR StretchRect). The H2 depth experiment still applies.
A: SD crop — footer text/version/title logos at their stock corners and size (compare `~/Desktop/title_prev.png`);
   log `4 system-sprite/loading screen-size load(s) now read 1280x720`. Everything from run 1 still good.
C: Tier A `output 1920x1080` / `render 1280x720` — now actually applies under `-w`: log
   `spice2x windowed mode (-w) -- display-mode check skipped`, `OUTPUT set applied (5 write(s))`, window client
   → 1920x1080 (spice2x pins 1280x720, our fit resizes), `present mode 1 -> 0 (ForceLetterbox)`,
   `PRESENT rt dims 1280x720 -> 1920x1080 (depth replacement pending …)`. This is the H2 depth experiment:
   black/missing picture = expected failure mode → Step 7 pulled forward.
B (SD letterbox) need not be repeated unless A regresses.
(Launch with `run_ddr`; `log.txt` lands when the game exits.)
Run A — SD crop again (`output: "640x480"`): the mod menu (0-0-0), toasts and the PUS timing widget must now sit
where they do at 720p (previously 2× and clipped); loading-screen art intact; log has
`SYSTEM root re-canvased 640x480 -> 1280x720 (frame N)`. WARN `SYSTEM root never read the output size` = layout
gate failed → report. Also open the mod menu: the Custom Resolution toggle now has RESOLUTION / RENDER SCALE rows;
change RESOLUTION and confirm `mod-config.json` `resolution.output` updated (whole section rewritten, other keys intact).
Run B — SD letterbox (`"sd_present": "letterbox"`): full HUD visible in a 640×360 band inside the 4:3 window,
black bars top/bottom; TEST menu unchanged; log `present-mode detour installed (Sd(Letterbox))` and one
`present mode 1 -> 0` INFO.
Run C — Tier A (`output: "1920x1080"`, `render: "1280x720"`, `sd_present` anything): 1920×1080 window (or
fullscreen mode on the cabinet), full-screen picture via the engine's LINEAR StretchRect (soft but complete —
NO crop at scene transitions), mod menu placed correctly, `present mode 1 -> 0 (ForceLetterbox)`,
`PRESENT rt dims 1280x720 -> 1920x1080 (depth replacement pending (plan Step 7) -- left stock)`.
**This run is also the H2 experiment**: the PRESENT pass binds a 720p depth to a 1080p colour target. If the
picture is black/missing or the log shows D3D errors, note it — Step 7's depth replacement gets pulled forward.
Optionally: `render: "1280x720"` with `output: "3840x2160"` on the Mac for the D3DMetal fill-rate feel.

### Checkpoint #1 run 3 — PASSED (2026-09-05)
Maintainer: window is 640×480, SD crop picture correct. (Log read too early — spice2x writes log.txt at exit; run-2 log otherwise clean. Nit fixed: `[-] aa_config_imm -- shape not found` in
`resolve_derived` was the derivation seeing our own 0 — it now accepts 3 or 0.)
Expected: the game window is 640×480; the picture is the stock SD 960-px centre crop (outer 160 canvas px
per side missing — P1's lane loses ~32 px in single play); TEST menu shows the full canvas letterboxed;
boot log has `CustomResolution: plan = output 640x480 (4:3), render 1280x720, present SD crop …`,
`OUTPUT set applied (5 write(s))` (hd_w/hd_h/window_client_w/window_client_h/aa — sd_w/sd_h are already 640/480), `graphics_init display struct:
hd_flag=1 aa_config=0 …`, `PRESENT rt dims 1280x720 -> 640x480 (depth stock, render covers output)`,
`screen globals confirm 640x480`, `early_apply complete`; spice2x's `CreateWindowExW hook hit (…, 650, 516, …)` (640×480 client + decoration) and the window itself is 640×480. KNOWN-NOT-YET-DONE: mod menu / toasts / PUS widget
render at 2× (root 7's canvas is still 640×480 — Step 4). Any `layout gate failed`, `screen globals read …`
or `rolling back` WARN = stop and report the log. Also run once with the mod OFF and confirm the boot log
is stock (`CustomResolution` absent except the skip line).

## Deviations & open questions

- Design refinement over register D16: instead of forcing the HD-flag byte, group 1
  patches the output dims into BOTH the HD and SD branches of the back-buffer
  selector (same effect — machine type becomes irrelevant — one patch group fewer).
- Steps 3–6 gate `compute` with `SUPPORTS_LETTERBOX_POLICY` / `SUPPORTS_NATIVE_RENDER`
  so each checkpoint only exposes configurations the shipped code can present
  correctly.
- Open (cabinet): H2 depth-size behaviour on retail D3D9 when render < output
  (Step 5 observes, Step 7 fixes); H3 read-back consumers; H5 shipped AA config;
  root-7 loading art after re-canvas; whether the maintainer's SD cabinet is P3IO.

## Key facts for a cold resume

- Everything physical is boot-time immediates + 2–3 detours; see design §1.
- Patch-site byte offsets (all builds): back-buffer imms at match+15/+25/+37/+47;
  `MOV R15D,0x500` at hoist+9 (`41 BF` at +7), `MOV ESI,0x2d0` at +0x2B;
  letterbox `MOV EDX,0x500` at fn+0x34 (imm +0x35), `[RBX+0x298]=0x2d0` at
  fn+0xDD (imm +0xE3); AA store = `fps_target_imm32 + 0x69` (imm at +0x6D);
  graphics_init CALL = first `E8` after fps+0x71; surface object global = first
  `48 8B 0D` RIP load after that CALL.
- RT struct: `+0x08` colour id, `+0x10` depth id (0 = none, refcounted via
  release/addref calls that follow the `C7 40 16` PRESENT dims store), `+0x14/+0x16`
  u16 w/h, `+0x18` u8 msaa; PRESENT rt = `*(surfaces+0x80)`.
- Scissor handler `(walker, record)`: record `+4` enable, `+6/+8/+A/+C` x,y,w,h;
  rt dims at `walker[1]+0x144/+0x146`; 2D ctx at `*walker` (`+0x00` offset,
  `+0x10` scale).
- Root 7 `ScreenRoot` == `widget_renderer::render_list_manager()`; `+0x50/+0x54`
  = canvas w/h floats; vtable slot 1 = `set_size(this, f32 w, f32 h)`.
- Letterbox fn `(this, mode)`: mode 1 crop (re-selected on every scene switch by
  `FUN_18002e3b0`), mode 0 letterbox (TEST menu); `screen_w == <imm>` ⇒ 1:1 POINT.
- Never touch the `1280.0f/720.0f` rodata, `ScreenRoot` defaults, or
  `DAT_18046091c/DAT_180464108` (logical canvas).
