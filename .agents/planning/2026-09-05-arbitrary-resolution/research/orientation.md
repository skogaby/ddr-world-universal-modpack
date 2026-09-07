# Orientation — Arbitrary Resolution Rendering

Written 2026-09-05 (PDD Step 2). Sources: `docs/arbitrary_resolution_research.md`
(the 2026-09-02 static RE study, primary build 20260616, AOBs verified on
20250805/20260721/20260825), a codebase sweep for screen-pixel consumers, the
boot-patch / shader-synthesis precedents, and the local game install's
`data/arc/shader.arc` + launch line.

## 1. What the RE study already settles

The engine separates a **logical 1280×720 canvas** from the **physical render
target**: the tag-0x07 handler divides canvas coords by the canvas size, and the
RT size enters only through an offset term (research §4.1). Consequences:

- Everything drawn from geometry — AFP shapes, HUD quads, arrows (incl. the
  modpack's AA/perspective shaders), guidelines, theme backgrounds, SMX quads —
  is resolution-independent for free once the surfaces/viewports are bigger.
- Hard-coded physical sizes are all **boot-time init immediates** plus one
  small per-frame walker handler:
  1. back-buffer dims (`FUN_1801ef6d0`, two imm32 0x500/0x2d0 → `DAT_1806f0524/0520`);
  2. ten render surfaces + six RT structs (`FUN_1801f01a0`, hoisted `R15D=0x500`/`ESI=0x2d0` + u16 RT dims);
  3. eight `ScreenCommandList` viewports (`FUN_1801f5d10`, imm table);
  4. letterbox src rect (`FUN_1801f3f60`, two imm32) — the engine's own shipping
     `StretchRect` upscale path (SD cabinets use it);
  5. scissor tag 0x0C (`FUN_180269080`) copies canvas px raw into `SetScissorRect`
     — the one walker-level detour needed;
  6. PRESENT RT struct keeps 1280×720 dims + a 720p depth even though it is
     re-pointed at the back-buffer every frame (H2).
- Layer-table roots 1/3/5/6/**7** are sized to SCREEN dims; root 7 (SYSTEM list,
  drawn after the upscale blit) is where every modpack `widget_renderer`
  widget lives → physical-pixel today.
- Tiers: **A** = native output + engine upscale (720p internal); **A+** = better
  upscale PS on `sys_copy`; **B** = native internal (all surfaces/viewports at
  target); **C** = hi-res asset pipeline (**excluded** by the rough idea).
- All patterns byte-stable across the four supported builds (§9) — no
  version-gated patching expected.

## 2. Codebase facts that shape the design

### Precedents to copy
- **Boot patch** = `fps_unlock` (`src/mods/fps_unlock.rs`): linear AOB in
  `SIGNATURES` (visible in `EarlyContext`), `early_apply` reads stock imm32 →
  validates → `memory::make_writable/write_u32/restore_protection`,
  `required_signatures() = &[]`, self-disable via `is_active()`, `init()`
  re-resolve for runtime toggles, next-launch semantics + overlay `Enum` row +
  `config::save_json_key("fps_unlock", …)`. `early_apply` runs in
  `src/lib.rs` step 2c, after `resolve_all`, before `resolve_derived` — so
  anything needed at early_apply must be a **linear** AOB, not a derivation.
- **Post-init detours** (RT-struct rewrite after `FUN_1801f01a0`, viewport
  rewrite after `FUN_1801f5d10`, scissor scaling): `GenericDetour` installs —
  BUT these must be installed **before onBoot's display init runs**, i.e. in
  `early_apply`, not `enable()`. No existing mod installs a detour that early;
  the `fps_unlock` race argument (DLL init provably beats onBoot's display
  init) is what makes it viable. Alternative that avoids early detours: patch
  the ctor immediates directly (research row 5/6 AOBs give every imm site) —
  pure byte patches, same shape as fps_unlock. Worth preferring.
- **Present shader**: `sys_copy.gsp`, `sys_copy_aa.gsp`, `dam/sys_copy_depth.gsp`
  exist in the stock `shader.arc` (36 entries, confirmed against the local
  install). `shader_synthesis.rs` can already extract a stock container's
  VS/PS and repack with a replacement PS; it needs a fourth container block +
  blob const + fingerprint input + `build_shaders.sh` manifest line
  (`ps_3_0`). No generic "replace named container" API today — small addition.
- **Byte patch primitives**: `memory::apply_checked_patch` (expected-bytes
  check + rollback), `make_writable`/`write_u32`/`restore_protection`.
- **Config**: typed `XxxConfig` in `src/mods/config.rs` `ConfigFile` (+ the two
  all-`None` fallback literals) + `save_json_key` for DLL-written sections.

### Modpack code that breaks (or changes) when screen ≠ 1280×720

| Component | Regime | Outcome without a fix |
|---|---|---|
| Every `widget_renderer` text/image widget: mod menu panel+text, toasts, training strip HUD + scrub icon, PUS timing widget, splash, autoplay banner, `preview_overlay`/`bg_preview_overlay` sprites (`CHROME_ORIGIN` measured at 1280×720) | root 7 = screen px | shrink to the top-left 1280×720 corner; preview sprites drift off the game's (canvas-scaled) option art |
| `overlay_draw` background + overlay quads, SMX topmost quads | own `set_context_2d(1280,720)` | correct |
| Mod-menu panel (widget) vs animated background (canvas quad) | mixed | **desynchronize** — the clearest symptom of an unfixed root 7 |
| Theme shaders (NDC→canvas via 640/360 + c48/c49 canvas rect) | canvas | correct; ~4–9× fill at 4K (D3DMetal concern) |
| SMX touch hit-test (client/monitor px × 1280/w, 720/h) | ratio | correct iff the presented image fills the window (no letterbox band) |
| `shader_fixes` arrow AA PS (`TEXEL=1/768,1/384`, "collapses to stock at 1:1") | texel | now filters at every scale — this IS the desired 2D filter for lane art; docs/comment need updating, no code change |
| Persp VS / playfield constants (640/360/720) | canvas | correct |
| `cull_window` 720.0 (verifies the RIP target reads 720.0f rodata) | logical | safe iff the rodata constant is never touched (it must not be) |
| Scissor emission from the DLL | none shipped (test-only) | n/a; the game's own scissors need the tag-0x0C fix |
| `fps_unlock` refresh request | `(W,H,Hz)` must be an enumerable mode | same risk as today, larger W/H |
| Loading-screen art also renders via root 7's wrapper walk (learnings) | root 7 | re-canvasing root 7 to 1280×720 affects it — needs cabinet verification |

There is **no** call to `ScreenRoot`'s set-size vfunc anywhere in the codebase;
root 7's `ScreenRoot` pointer is reachable through the existing `layer_table`
derivation (`overlay_draw::resolve_widget_layer_list` identifies entry 7 by
pointer identity with `widget_renderer::render_list_manager()`).

### Environment facts
- Local install: `$DDR_WORLD_INSTALL` (CrossOver bottle) launches
  `spice64.exe -ddr -w … -icmphook -K ddr_world_hook.dll` — **windowed**.
  spice2x's D3D9 wrapper (`graphics::d3d9`) sits on `CreateDevice`/`Present`
  and implements `-w` (H4 open: does it forward non-720p back-buffer dims
  verbatim, and how does it size the window?). The game's own windowed branch
  sizes the client to `screen_w × screen_h` via `SetWindowPos`.
- The maintainer's real cabinet is Windows fullscreen; CrossOver (D3DMetal)
  is the dev loop. Fill-rate at 4K under D3DMetal is untested (H9).
- Wine detection helper exists: `core::platform::running_under_wine()`.
- Offline-checkable now (no cabinet needed): H1 (does the bm2d VS consume
  c50–c53?) by disassembling `gs_screencommand_bm2d_default.gsp` from the
  local `shader.arc`; the stock `sys_copy` PS shape; the four-build AOB sweep
  via `scripts/validate_signatures.sh` once signatures are declared.

## 3. Blind spots the rough idea does not mention

1. **Tier choice per resolution.** The idea says "alter rendering resolution"
   → Tier B (native internal). But for **480p** (downscale) Tier A — render
   720p and *downsample* through a good filter — may look better than native
   480p (bilinear minification of unmipped 720p bitmaps at 1.5× is acceptable
   but arrows/text lose more than a filtered downscale would). Also an SD
   cabinet is 640×480 **4:3** with a 960-px centre crop of the canvas (mode 1);
   a 16:9 "480p" is 854×480. Which 480p?
2. **Ultrawide has no cheap version.** The canvas is 16:9-logical; every AFP
   layout is 1280-anchored. The engine's letterbox math scales to WIDTH
   (`scaled_h = screen_w/1280·720`), so on 21:9 it would *crop* top/bottom.
   Options: (a) pillarbox 16:9 content (present-path fix, cheap, no extra
   field of view); (b) true widescreen extension (3D backgrounds + movie plane
   extend, HUD stays 1280-anchored) — large, per-scene RE. H6 (3D aspect
   source) is open.
3. **What "filters for 2D" can mean here.** At Tier B the GPU already
   bilinear-magnifies every bitmap at draw time; there is no separate upscale
   pass to filter. Better-than-bilinear = replacing the `bm2d_default`/`font`
   PS with bicubic/Lanczos (per-draw cost, synthesis path exists). For lane
   art the existing AA PS already does palette-aware 4-tap filtering. For
   downscaled (480p) targets a present-path filter only exists in Tier A.
4. **Internal ≠ output could be a feature** (supersampling: render 1440p,
   present 1080p through `sys_copy`) — the letterbox path already handles it.
   Scope question.
5. **AA config** (`DAT_1806f050c`): mode 3 ("direct", pcType 2..4 on HD cabs)
   re-wires targets; MSAA 2×/4× at 4K is expensive. Force 0 at non-720p?
6. **Fail-safe.** A back-buffer size the display cannot present = black
   screen at boot with no way in to change the setting. The DLL can enumerate
   modes itself (`Direct3DCreate9` + `EnumAdapterModes`) at `early_apply` and
   refuse the patch (WARN + stock) when fullscreen and the mode is absent.
7. **Depth surfaces** (H2) and the **RENDER_CAPTURE/read-back** consumers (H3)
   are unverified hypotheses that only Tier B exercises.
8. **Root 7 re-canvas** is load-bearing for the whole modpack UI and also
   touches the game's loading-screen art — the first thing to cabinet-verify.
9. **Config UX**: where the setting lives (config section + overlay `Enum` row
   like fps_unlock, next-launch), preset list, and whether a "native/desktop"
   auto option exists (query the desktop mode at boot).
10. **Tooling**: `preview_overlay`/`bg_preview_overlay`/`check_option_takeover.py`
    templates are 1280×720 captures — authoring captures at other resolutions
    must be downscaled (note, not runtime).

## 4. Proposed sequence

Clarification first (the tier/ultrawide/480p/filter decisions change the
design shape), with two offline research items run in parallel because they
need no cabinet: H1 (bm2d VS constant usage) and the stock `sys_copy` PS
shape / spice2x `-w` behaviour (spicetools source). Everything with a live
component (H2/H3/H4-runtime/H5/H9) becomes a plan step with a cabinet gate.
