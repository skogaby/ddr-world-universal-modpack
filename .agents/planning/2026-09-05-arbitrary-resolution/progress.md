# Progress — Custom Resolution (arbitrary resolution rendering)

Updated: 2026-09-09
Status: COMPLETE + one-knob revision (2026-09-09) awaiting ONE cabinet check (uncommitted — maintainer commits manually)
NEXT ACTION: cabinet check of the one-knob build at `"output": "1920x1080"` (mod ON): expect `boot state -- output
1920x1080 (16:9), render 1920x1080, present stock (1:1), aa game's choice; OUTPUT 6 write(s), RENDER 22 write(s)` and
the NEW line `present chain -- aa_config=3 (onBoot chose 3) -> direct (3): …` (a `0` there on this cabinet = regression);
picture identical to checkpoint #3 (direct mode renders 3D+2D straight into the output-sized `display`); then a 640x480
regression run (`aa_config=0 (onBoot chose 3)`, `-> offscreen composite (0)`, SD crop picture). Ask the 1080p-stutter
reporters for their `present chain` line + FPS Unlock setting. Open follow-ups (not blockers): the scissor detour has
never been exercised by stock content (watch for its `first dispatch` INFO); planning dir can move to
`.agents/planning/_archive/` after the check.

Resume protocol: read this file, then `implementation/plan.md` (checklist), then
`design/detailed-design.md` (§4 components), then `idea-honing.md` (register) and
`research/*.md` only when a design claim needs its evidence.

## Done

- **Menu-toggle persistence fix (2026-09-09, tester report)** — a tester could not change the resolution from the
  0-0-0 menu: picking a size then restarting always came back at 720p; only hand-editing `mod-config.json` worked.
  Root cause was NOT the row (`persist()` → `save_json_key("resolution", …)` is correct) but `is_active()` returning
  `applied` ("a non-stock plan landed THIS boot"): on a fresh install (mod OFF at launch, or ON at stock 720p) the
  registry recorded the ON toggle as self-disabled, and the mod-menu's `toggle_registry_mod` persists `enabled` for
  EVERY mod on every toggle — writing `mods["custom-resolution"] = false` back (and hiding the child rows). Fix:
  `is_active` = `capable` (load-bearing sites resolved, computed in `init` — the fps_unlock model); registry tracks
  `requested` (intent) vs `enabled` (effective) and `save_mod_states` persists `requested`; the same trap fixed in
  `gameplay_timing_fixes` (`armed_next_launch`). Learnings entry + AGENTS.md pattern added. Cabinet check: from a
  config with the mod OFF, open 0-0-0 → toggle CUSTOM RESOLUTION on → pick 1920x1080 → close menu → `mod-config.json`
  reads `"custom-resolution": true` + `"output": "1920x1080"` (log: `Mod enabled: Custom Resolution`, NOT
  `self-disabled`) → relaunch at 1080p.

- **One-knob revision (2026-09-09)** — triggered by user reports of 1080p stutter on mid-range cabinets that run other
  native-1080p Bemani games fine. Audit: the mod adds zero per-frame CPU; the ONE mod-introduced GPU cost was the
  `msaa: "off"` default forcing AA config 3 → 0, i.e. kicking every pcType-2..4 cabinet (and spice2x) out of the
  engine's "direct" present chain into the offscreen-composite one (+2 `StretchRect`, +1 clear, +1 depth-copy quad, +2
  RT switches per frame at 1080p — 20260825 RE of `FUN_1801f10e0` + the three `AfterRenderConditionImpl` vfuncs, table
  in `docs/custom_resolution.md` §3a). `shader_fixes`' AA pixel shaders were ruled out (≈4× per lane pixel at EVERY
  resolution — not resolution-specific). Maintainer decision: remove render ≠ output entirely and manage AA
  opinionatedly. Changes: `plan.rs` — `PlanInput {output, sd_present}` only, `render_for` (16:9 ⇒ output, 4:3 ⇒ 720p),
  `AaPolicy::{Stock, ForceOff}` (ForceOff for 4:3 only), `ForceLetterbox`/`PresentDepth`/`Gates` removed,
  `present_chain_shape()` added, 24 host tests green; `present.rs` — depth swap removed, records onBoot/effective AA
  (`aa_config()`), logs the chain shape at graphics_init; `mod.rs` — one-shot `present chain -- …` INFO in `enable`
  (deferred via a self-removing scene callback when `enable` beats `graphics_init`); `letterbox.rs` — SD LETTERBOX only,
  a miss no longer rolls the plan back; `rows.rs` — RENDER SCALE + MSAA rows gone; `config.rs` / `mod-config.json` —
  `render` + `msaa` fields dropped (unknown keys ignored); `signatures.rs` — `surface_create` / `present_depth_*`
  derivations removed (`CustomResolutionAnchors` is 5 fields; sweep ALL GREEN). Docs: `custom_resolution.md` §1a/§3/§3a
  + touch-ups, AGENTS.md row + config, README.

- PDD Steps 1–7: workspace, orientation, register (accepted wholesale 2026-09-05),
  research (SD path via Ghidra, H1 refuted by shader bytecode, `sys_copy` shape,
  spice2x source, four-build AOB sweep), readiness confirmed, design + plan
  approved (maintainer pre-authorized autonomous continuation to the first
  cabinet checkpoint = plan Step 3).
- Impl Step 8 (2026-09-07): `docs/custom_resolution.md` (implementation record + RE facts + H1–H9 outcomes + Phase-2
  decision), AGENTS.md Key Entry Points row + `resolution` config entry, research doc §10 outcomes banner, learnings
  entry (debughook attach race + the caught texpresso panic).
- Impl Step 7 (checkpoint #4 passed 2026-09-07): `present.rs::replace_depth` — output-sized PRESENT depth via the
  engine's `surface_create(w,h,0x4b,0,&{0,0})` + release/addref idiom; nulled fallback. Record:
  `.agents/scratchpad/.../step07-depth-replacement/`.
- Impl Step 6 (checkpoint #3 passed 2026-09-07, 1080p + 4K native): `patches.rs::apply_render_set` (groups 3/4/5,
  22 writes, atomic with the OUTPUT set), `scissor.rs` (tag-0x0C record rescale detour), `GATES.native_render = true`,
  `enable()`-time `boot state` summary line. Record: `.agents/scratchpad/.../step06-native-render/`.
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

- Nothing.

## Deploy & test log

### Post-completion follow-up (2026-09-07) — MSAA + SD PRESENT MODE overlay rows — PENDING cabinet check
`msaa` gained real values: `off` (default; `auto` = legacy alias) / `2x` / `4x` / `stock`. `off`/`2x`/`4x` are written into
the display struct `+0x18` by the `graphics_init` detour PRE-original (covers every onBoot branch; the imm patch still
handles the pcType-2..4 `MOV [RSP+d],3` for `off`); `stock` is REFUSED by `plan::compute` when render ≠ output (mode 3 =
direct, no present scaler). Two new rows under the mod toggle: MSAA (OFF/2X/4X/STOCK) and SD PRESENT MODE (CROP/LETTERBOX).
Check: set MSAA 4X at 4K native → log `graphics_init display struct: hd_flag=1 aa_config=2 (onBoot chose 3) fps=120`,
`plan = … aa 4x MSAA`; verify the game boots and geometry edges (guidelines, 3D background) are smoothed; also `RENDER
set applied` unchanged. Risk: D3DMetal MSAA surface support / fill rate at 4K — if it fails to boot, `off` is one config
edit away. Both rows persist the whole `resolution` section (`mod-config.json` default flipped `auto` → `off`).

### Checkpoint #4 (Step 7, perf mode: render < output) — PASSED (2026-09-07, `log_g/h/i.txt`; maintainer visual OK)
G (4K / 1080p render, through gameplay → STAGE_RESULT → RESULTS_DETAIL): `boot state -- … render 1920x1080, present
letterbox … RENDER 22 write(s), scissor detour on, present-mode detour on, logical screen on`; `PRESENT rt dims 1920x1080
-> 3840x2160 (depth replaced: id 0xA02D2 (render-sized) -> 0x160F72 (3840x2160 output-sized))`; `present mode 1 -> 0
(ForceLetterbox)`; `4 AFP load(s) read 1920x1080 (render)`. H (4K / 75 % = 2880x1620): same shape, depth 0xA02D2 ->
0x160FA2. I (SD 640x480 regression): unchanged (`OUTPUT 5 write(s), RENDER 0 write(s), scissor detour off … depth stock`).
Zero CustomResolution WARNs in all three. SIDE FINDING (log_g, results screen): `PANIC at texpresso … output.len() >=
compressed_size` + `LayeredFS: panic in avs_fs_open hook — serving original file` — a pre-existing `ifs_textures::
cache_texture` bug (padded DXT5 buffer encoded with the PNG's pre-padding dims), NOT resolution-related; fixed in the
same tree (rebind dims after padding, `compressed_size` allocation) + learnings entry. Original run script:
Run G — `"resolution": {"output": "3840x2160", "render": "1920x1080"}` (mod ON, `run_ddr`, exit, read `log.txt`).
Expected: `boot state -- output 3840x2160 (16:9), render 1920x1080, present letterbox, aa forced 0; OUTPUT 7 write(s),
RENDER 22 write(s), scissor detour on, present-mode detour on, logical screen on`; `PRESENT rt dims 1920x1080 ->
3840x2160 (depth replaced: id 0x… (render-sized) -> 0x… (3840x2160 output-sized))`; one `present mode 1 -> 0
(ForceLetterbox)` INFO; `logical screen installed -- … 4 AFP load(s) read 1920x1080 (render)`. Visual: full-screen
picture (no crop at scene transitions), 1080p-soft but complete, HUD/AFP aligned, menus/mod menu placed right; NO black
panel (that was the H2 risk on real D3D9 — a depth smaller than the colour target; CrossOver was tolerant).
Run H (optional) — `"render": "75%"` at 4K (2880×1620): same shape; `OFFSCREEN1 2880x2880`.
Run I (regression) — `"output": "640x480"` SD crop: unchanged from checkpoint #2 (`depth stock, render covers output`).
Failure signatures: `PRESENT depth NULLED … unresolved` WARN (a derivation missed — the picture should STILL render, the
PRESENT quad needs no Z; report which name); `surface_create(...) returned 0` WARN; a black/missing picture with the
`replaced` line present ⇒ the swapped depth is rejected by the device — report and we fall back to nulling.

### Checkpoint #3 (Step 6, native render) — PASSED (2026-09-07, `log_4k.txt`; maintainer: "visually everything looked
good" at 1080p and 4K). Two log findings, both fixed in the follow-up build:
1. `log_4k.txt` has NO `CustomResolution` early_apply lines — not a mod bug: spice2x's `debughook` ATTACHED (log lines
   282–283, 00:32:17) after our DLL had already logged through mid-derivation (first captured DDR-Hook line = a
   `[+] sprite_vtable` derivation). Everything before it (signature-scan summary, SongLimit/FpsUnlock/CustomResolution
   early_apply) was never captured. Runtime evidence still proved the plan: `graphics_init display struct: … aa_config=0`,
   `PRESENT rt dims 3840x2160 -> 3840x2160 (depth stock, render covers output)` (the rt struct READ 3840x2160 ⇒ the
   group-3 hoist + PRESENT stores landed), `screen globals confirm 3840x2160`, window client → 3840x2160, and correct
   scissoring on screen. NOTE the level tag is irrelevant — `[DDR-Hook][WARN]` lines DO appear in the same log
   (`present_depth_release`, PremiumFree diag) — the loss is purely attach timing. Fix: `enable()` now logs a late
   `CustomResolution: boot state -- <plan>; OUTPUT N write(s), RENDER N write(s), scissor detour on/off, present-mode
   detour on/off, logical screen on/OFF` line (init phase, after the debughook attach).
2. `[-] present_depth_release / present_depth_addref -- shape not found (from render_surface_hoist)`: the derivation
   anchored on the STOCK `C7 40 16 D0 02 00 00` PRESENT store, which the RENDER set had already rewritten to the render
   height. Now matches `C7 40 16 ?? ?? 00 00` (first occurrence — offline-verified to be 0xbdd on all four builds with or
   without the patch); Step 7 depends on these two.
Confirmation run (2026-09-07 00:50, `log.txt`, 4K, title/attract only): `boot state -- output 3840x2160 (16:9), render
3840x2160, present stock (1:1), aa forced 0; OUTPUT 7 write(s), RENDER 22 write(s), scissor detour on, present-mode detour
off, logical screen on`; `[+] present_depth_release @ +0x250BE0` / `[+] present_depth_addref @ +0x250B30`; PRESENT/screen
globals/window lines as before; zero CustomResolution WARNs. Debughook again attached mid-early_apply (only the last two
early lines captured) — the summary line is load-bearing for cabinet triage.
OPEN OBSERVATION: no `scissor … -> …` rescale INFO in either 4K log, incl. the run that visited SONG_SELECT → GAMEPLAY ⇒
no scissor-flagged `ScreenRoot` (`+0x60`) rendered there (Ghidra 20260825 `FUN_1802180a0` = ScreenRoot::render: the
tag-0x0C record is emitted only when the flag is set; the research's "options menu / song wheel" list was inference). The
detour is live and dormant. Whenever a log first shows the `scissor` INFO, that screen is the one to eyeball for clipping
(candidate: the song-select OPTIONS modal).
Run 3 (00:55, `log.txt`): title → attract → song select WITH the options modals opened → gameplay with a background movie —
all clean visually (movie plane included), zero CustomResolution WARNs, and STILL no `scissor` INFO. Ghidra: the ONLY writer
of `ScreenRoot+0x60` is vtable slot 9 (`FUN_180217de0`, 20260825; ctor defaults it to 0), and the options modal is AFP
content (its clipping is AFP-side, not tag 0x0C). Working conclusion: World's stock content never enables a scissor root
in these scenes; the detour is a safety net for whatever does (the DLL's own overlay scissors were retired earlier).
Added a one-shot `scissor handler first dispatch (enable=…, viewport WxH)` INFO so a future log can at least say whether
the handler is ever reached; rebuilt (check/fmt/build clean). NOT a Step-7 blocker.
`log_1080p.txt` in the install is a STALE Sep-6 SD-crop run from the run-3 build (`widget canvas scale` line) — not the
1080p checkpoint; the 1080p pass is the maintainer's visual report only.
Run D — `"resolution": {"output": "1920x1080", "render": "output"}` (mod ON, `run_ddr`, read `log.txt` after exit).
Expected log: `plan = output 1920x1080 (16:9), render 1920x1080, present stock (1:1), aa forced 0`, `OUTPUT set applied
(5 write(s))`, `RENDER set applied (22 write(s)) -> surfaces + list viewports + letterbox src 1920x1080 (OFFSCREEN1
1920x1920)`, `scissor detour installed (canvas -> render-target px)`, `graphics_init detour installed`, NO present-mode
detour line (policy Stock), `logical screen installed -- 4 app-layer load(s) read 1280x720 (design), 4 AFP load(s)
read 1920x1080 (render); 30 untouched physical`, `graphics_init display struct: … aa_config=0`, `PRESENT rt dims
1920x1080 -> 1920x1080 (depth stock, render covers output)`, `screen globals confirm 1920x1080`, window client →
1920x1080 (spice2x pin + our fit), then up to three `scissor WxH+X+Y (canvas 1280x720) -> … (rt 1920x1080, origin
0.0,0.0)` INFOs the first time a scissored layer renders (options menu / song wheel), `early_apply complete`.
Visual: geometry CRISP (arrows, guidelines, AFP shapes — compare with the soft Tier-A picture), options menu lists /
song wheel / any scrolling list clip at their proper bounds (not the top-left 2/3), results screen + photo path (H3),
attract loop, TEST menu, mod menu placed as at 720p, `shader_fixes` AA visibly filtering lane art.
Run E — `"output": "3840x2160"` (render output): same lines with 3840x2160 / OFFSCREEN1 3840x3840; note CrossOver
frame time.
Failure signatures: `RENDER set not applied (…)` / `scissor_handler detour install failed` WARN + `rolling back` ⇒
stock boot, report the line; `PRESENT rt reads WxH, expected render …` ⇒ group 3's PRESENT store was not the first
`C7 40 16`; a picture that is a 1280×720 top-left CROP of the UI ⇒ the scissor rescale did not fire (report whether the
`scissor …` INFOs appear); black screen at boot ⇒ report `logical screen installed` counts.
Optional Run F — `"output": "3840x2160", "render": "75%"` (2880×1620 render, ForceLetterbox, depth still stock =
Step 7's H2 territory): exercises the RENDER set with render ≠ output.

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
