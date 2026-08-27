# Overlay Draw — Command-List Emission Research (Spike Notes)

Status: **COMPLETE — GO for shader-backed overlay backgrounds** (2026-08-24).
Feature: overlay-menu rewrite Step 2 (shader-background go/no-go). Addresses are
file-relative to gamemdx.dll base 0x180000000, build 20260616 unless noted. Builds on
`docs/custom_arrow_renderer_research.md` (tag map, §3) and
`docs/shader_replacement_research.md`.

## What shipped (spike instrumentation)

- `src/services/overlay_draw/encode.rs` — pure record encoders (tags 0x03, 0x04 header
  shape, 0x07, 0x0C, 0x11, 0x13, 0x14) + size-chain walker; host-tested
  (`scripts/validate_overlay_draw.sh`, 12 tests).
- `src/services/overlay_draw/mod.rs` — per-scene diagnostics (always on, one INFO per
  scene id) + `DDR_OVERLAY_DRAW_POC`-gated tinted-quad emission from
  `widget_renderer::wrapper_render_hook` (pre-original).

## New layout facts (Ghidra, 20260616)

1. **Tag 0x03 (untextured quads), handler `FUN_180268090`:** header
   `{u16 tag, u16 size, u32 count @+4, u64 payload_ptr @+8}` — same shape as 0x04.
   Payload count × 0x24 `{x0,y0..x3,y3: f32, color: u32}`; expanded to the triangle
   list `(p0,p1,p2)(p2,p3,p0)` (corners trace the perimeter); the color dword is
   copied verbatim into all 6 vertices (D3DCOLOR AARRGGBB). Coordinates map through
   the walker's current 2D context.
2. **Tag 0x07 (2D context), handler `FUN_180268c40`:** payload `{f32 canvas_w @+4,
   canvas_h @+8, offset_x @+0xC, offset_y @+0x10}` (size 0x14). Sets the virtual
   canvas: draw x is scaled by `K/canvas_w` and offset by `offset_x/rt_w` (rt dims at
   walker+0x144/+0x146). `set_context_2d(1280,720,0,0)` = standard full-screen canvas.
3. **Tag 0x0C (scissor), handler `FUN_180269080`:** payload `{u16 enable @+4, x @+6,
   y @+8, w @+0xA, h @+0xC}`; content ends +0xE — we emit size 0x10 (the walker chains
   purely by the size field). Disable ignores the rect.

## Autonomous run results (CrossOver bottle, 2026-08-24)

**Diagnostics boot (POC off):** one line per scene id, boot (−1, 0, 1) through the
attract band (2, 3, 5, 6, 14, 16):

- The active command list is **non-null with the bump invariant
  (`write == base + size`) holding in every scene sampled**.
- The default shader container is boot-resident everywhere: same object
  (`progs=2` — stock prog 0 + the shader-fixes AA overlay program) from scene −1 on.
- Arena sizes at the wrapper-render sample point are tiny: 0x38–0x96EC observed
  (boot menus largest); attract demo ~0xA90–0xC20. The 8 MiB soft cap is far above
  any observed frame.

**POC boot (`DDR_OVERLAY_DRAW_POC=1`):** 18,000+ emissions over ~110 s spanning
multiple full attract cycles (incl. demo gameplay), **zero gate WARNs, zero crashes**.
Each emission: context(1280×720) → scissor-on(200,100,880,520) → SetShader(default,
prog 0) → one 50 %-black quad → SetShader restore → scissor-off (0xC8 bytes).

**Finding — multi-list emission:** heartbeat cadence (~300 emissions/s at 60 fps)
shows the per-frame gate (`same list && size >= last_emit_end`) re-arms on **active-
list switches within a frame** — the layer dispatcher points different layers at
different lists (or resets between layers), so the POC emits into ~5 lists/frame.
Harmless (gates hold, sizes tiny), but the production emitter must pick ONE layer —
either by emitting only when the active list matches the widget layer's list, or by
identifying the widget layer slot (the z-probe session will inform which).

## Visual / z-probe session (maintainer, 2026-08-24) — VERDICT: GO

Maintainer-captured screenshots (attract demo; POC quad alone, and with the mod menu
opened on top):

1. **The quad renders ABOVE all game content.** Everything inside the rect is darkened
   — background video, arrows, combo text, the song-info card, ENERGY readouts where
   they overlap. (An earlier agent-side misreading of the first screenshot — "the song
   card is above the quad" — was wrong: the card is merely partially OUTSIDE the rect;
   the portion inside is darkened. Corrected by the maintainer.)
2. **The quad renders BELOW the DLL's text widgets.** With the mod menu open, its
   white text is fully bright on top of the quad. This is the exact three-layer
   sandwich the menu design needs (game → background quad → widget text/chrome), with
   the emission happening at the wrapper pre-original site and the menu widgets being
   later render-list nodes.
3. **Scissor is pixel-exact.** Maintainer-measured unobscured margins (~150–200 px
   left/right, ~120 px top/bottom) match the programmed rect (200 px left/right,
   100/100 px top/bottom on a 1280×720 canvas) precisely.

Notes for interpretation: the multi-list emission (below) means quads landed in ~5
per-layer lists, and the observed result composites them ABOVE the game's layers —
i.e., at least one emission lands in a layer that draws over the full game scene.
The production emitter should still pin ONE list (the last-drawing one that precedes
the widget pass) rather than emit into all of them; with the widget-layer sandwich
confirmed working, the pragmatic recipe is: keep emitting from the wrapper
pre-original site, but gate to a single emission per frame per FRAME (not per list) —
the visually-topmost list is the one active at the widget layer's draw, which is
exactly where the wrapper hook runs.

## Production recipe (for Step 8)

1. Emit from `wrapper_render_hook` pre-original (current site) — the active list at
   that moment belongs to the layer that draws the DLL's widgets, which composites
   above game content and below the widget nodes that render after the wrapper.
2. Gate ONE emission per frame total (not per list): latch on the first wrapper
   render after the active list's arena reset, exactly as the POC does, but ALSO
   require... (empirically the POC's per-list gate emitted ~5×/frame; the extra
   emissions were harmless but wasteful — Step 8 should sample the list pointer at
   the widget layer once and emit only when `active_command_list()` matches it, or
   simply accept the first-list-of-frame emission since the sandwich held visually).
   Decision deferred to Step 8 implementation with this data.
3. Keep the full gate ladder + bump invariant + context(1280,720) + scissor discipline
   unchanged — 18k emissions, zero faults.
4. Swap SetShader(default, 0) for SetShader(default, theme_prog_idx) behind the
   program-count gate; add the c48 time/rect constants (design §5).

## Production notes (for Step 8)

- Frame/list gating needs a layer-identity check, not just the reset heuristic.
- No PS-constant record exists; time reaches the PS via a VS interpolator (design §5).
- The SetShader handler has NO bounds check — the `progs >= idx+1` gate is mandatory
  (already enforced in the POC's ladder).
- The bump invariant (`write == base + size`) held universally — keep it as a hard
  refuse-to-emit gate.

## Step 8 production outcome (2026-08-25)

The emitter shipped as `src/services/overlay_draw/` production code (POC env
gate removed). Two findings amended the plan above:

- **BmpString wrappers render through the hooked path only while DIRTY.**
  A menu-owned "anchor" wrapper (created first in the menu's registration
  order so its pre-original emission would land beneath the panel) fired
  exactly ONCE per repaint — static text is served from a cached path that
  never calls `wrapper_render`. The GAME's own wrappers re-render
  continuously in every scene, which is why the POC saw steady calls. A
  DLL-widget-anchored layer-identity gate is therefore NOT viable; the
  production emitter uses the POC's per-(list, frame) gate unchanged
  (`same list && size >= last_emit_end` ⇒ already emitted), which the
  spike's z-probe already validated visually — the DLL's widgets composite
  above the quad regardless of which wrapper's render carried the append.
- Emission rate in practice: ~200–240/s (multi-list, as the POC observed;
  arena sizes ~0xBF8–0xC58 at the menu's typical scenes) — harmless, and
  the per-list dedup keeps it bounded.

Production shape (cabinet-validated): context(1280,720) → scissor-on
(60,60,1160,600) → SetVSConstantF(c48={time%3600, x, y, 0},
c49={w, h, p0, p1}) → SetShader(default, theme_prog) behind the
`progs >= idx+1` gate → one opaque-black quad → SetShader(default, 0) →
scissor-off. Theme program indices come from `shader_synthesis`'s
publish (`overlay_draw::theme_program_indices()`); activation from the
menu's `set_background` feed (open ∧ animate ∧ theme shader-backed ∧
indices present). 60 consecutive gate failures latch the emitter off for
the session (one WARN).

### Feedback-round amendments (2026-08-25, same day)

Maintainer live testing exposed two more layer facts, settling the final
gating design:

- **Static scenes render no dirty text some frames** — title/language-select
  screens produce ZERO wrapper renders per frame, so even the any-wrapper
  spray goes silent there. Solved with a menu-owned self-sustaining ANCHOR:
  `refresh_all` seeds its dirty flag (`render_state+0x68` — the byte
  `TextWidget::set_text` sets), and a post-original hook leg re-arms it
  every frame while the background is active (the game's render pass clears
  the flag, so a pre-original write is clobbered). This guarantees ≥1
  wrapper render per frame in every scene.
- **The anchor's own list composes BELOW the attract movie** — an
  anchor-only emission was invisible during movie-backed attract songs
  (visible only as flashes while the movie loader showed black). The
  multi-list spray (per-(list,frame) dedup) is what puts the quad above
  movies. Production = anchor (frame guarantee) + spray (layer coverage).

Also: the quad's vertex alpha carries MENU OPACITY (the theme PS' master
fade); the theme PS rounds the modal's corners via a TEXCOORD2-fed r=20
SDF; and the menu dims its gradient panel to a ~35 % wash over a live
animation so the shader stays visible at 100 % opacity.

### HARD CONSTRAINT: the layer-slot table is NOT walkable (2026-08-25)

An experiment appending the background block to EVERY slot of the
ScreenRenderer state's layer table (`state+0x40 + slot*8`, slots 0..=8)
to solve scene-dependent layer visibility CRASHED the game in-engine
(gamemdx stack through the render path; ddr_world_hook at the top).
Non-active slots may hold lists the engine is concurrently consuming,
resetting, or that are otherwise not in an appendable state — the bump
invariant read on a torn list is meaningless. **Only
`active_command_list()` (the `state+0x68` index path) is a verified-safe
append surface.** Scene-dependent top-layer coverage needs real RE of the
layer walk/composition order instead (find where wrappers' display quads
are drawn per layer and emit at the widget layer's own walk) — left as
the documented follow-up; until then, title-screen-class scenes show the
static gradient (the design's degrade) while text-churning scenes
(attract songs, gameplay, most menus) show the animation.

## RE spike: the per-frame layer dispatcher (2026-08-25, gamemdx 20260721)

The proper all-scene emission site, found via the `screen_renderer_state`
global's xrefs:

- **`FUN_18002af10` (20260721) / `FUN_18002b530` (20260616) — the layer
  dispatcher.** Called ONCE PER FRAME, unconditionally, from the render
  orchestrator `FUN_180003020` (20260721: call at `0x180003285`), which is
  the game's per-frame render tick (increments the frame counter global,
  runs a prepare pass, then the dispatcher, then the consumer kick
  `0x1801f0430`).
- The dispatcher iterates an **11-entry layer table** at `0x1806f2d18`
  (20260721), stride 0x18: `{+0: override_ptr, +8: layer_object,
  +0x10: list_index (u32)}`. Per entry:
  - `override_ptr != null` ⇒ `state+0x50 = override; state+0x68 = 2`,
    else `state+0x68 = entry.list_index` — **this is the only writer of
    the active-list index** (`state+0x40 + idx*8` selects the list).
  - The layer is WALKED (`layer_object->vtbl[+0x28]`) iff
    `byte[layer+0x10] == 0 && byte[layer+0x12] != 0` (the orchestrator's
    earlier prepare pass uses `+0x11` with `vtbl[+0x20]`).
- Signature (verified unique on 20260721 @ 0x18002af10 and 20260616 @
  0x18002b530; prologue + the two global loads, disp32s wildcarded):
  `48 89 5C 24 08 57 48 83 EC 60 48 8B 15 ? ? ? ? 4C 8B 05 ? ? ? ? 0F 29 74 24 50 0F 57 F6`
  Derivations: `layer_table` = RIP disp32 at match+13;
  `screen_renderer_state` cross-check = RIP disp32 at match+20. The
  `LEA EDI,[RBX+0xB]` (11-entry count) sits shortly after — structural
  confirmation.
- **Emission design**: detour the dispatcher; PRE-original, for every
  entry the dispatcher itself will walk (same null/flag conditions, same
  thread, same moment — no torn-list risk, unlike blind slot walking),
  append the background block to that entry's list. The layer's own walk
  then appends its content AFTER our quad ⇒ the quad sits at the bottom
  of every composed layer: under the menu widgets, over everything the
  lower layers drew. Once per frame by construction — no anchor, no
  dirty-flag chain, no per-list frame gates needed.
- OPEN QUESTION for the validation boot: are the walked lists' arenas
  reset BEFORE the dispatcher (prepare pass) or inside each walk? The
  diagnostic logs list sizes at detour entry — near-zero sizes = reset
  already happened (pre-original append survives).

### The widget layer is an OVERRIDE entry (2026-08-25)

Cabinet table dump (20260721): entries 0–5 are ordinary slot layers
(idx 0/1), entry 6 empty, entries **7–10 are override entries** — their
`override_ptr` is each layer's own PRIVATE CommandList, which the
dispatcher installs at `state+0x50` (and sets index 2) while walking
that layer. The DLL's widget manager (`*(scene_mgr)+0xB0`) IS entry 7's
layer object. Entry 7–10's layer objects are all render-list managers
(constructor `FUN_180217d50`, vtable `0x1803897e8` on 20260721; walk =
vtbl+0x28 = `0x180218000`, prepare = vtbl+0x20). The private lists are
globals: an 8-pointer array at `0x1806f1620` (20260721; entry 7 → index
3 = `0x1806f1638`, entry 8 → 1, entry 9 → 4, entry 10 → 5). A
dispatcher-detour append to entry 7's private list (pre-dispatch)
validated with 16.8k emissions across a full attract cycle — but see the
loading-screen investigation below: SEGMENT-START POSITION IS WRONG.

## Loading-screen investigation (2026-08-25) — the final emission site

The dispatcher-detour emitter rendered in every scene EXCEPT two loading
interstitials (login→wheel = scene 21 CAUTION band, select→gameplay =
scenes 26/27), where the animation vanished while the menu text and
panel stayed visible. Four diagnostic rounds settled it:

1. **Survival probe + stale-chain tag dump.** The per-frame reset only
   rewinds the arena (size=0, write=base) — last frame's record chain
   stays readable, so a probe at emission time can dump what the layer
   recorded after our block. CAUTION: a first-8-bytes survival probe
   COLLIDES — the layer walk's own first record is `07:14` with canvas
   1280.0, byte-identical to our block's `set_context_2d` head. Only the
   full tag-chain dump told the truth.
2. **The frame-begin reset.** `FUN_1801f6e30` (20260721; `FUN_1801f6540`
   on 20260616) resets all 8 private lists and records each list's
   0x24-byte prefix (tag 0x13 SetShader(default,0) + tag 0x08 blend
   {1, 0x1220625}). Single static caller `FUN_1801f2c00` (submit+reset),
   itself called only from the render orchestrator `FUN_180003020` (top
   of frame; a second flag-gated call site at +0x3218 runs once on the
   first frame only). AOB, unique on both builds:
   `48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 48 89 7C 24 20 41 54
   4C 8B 15 ? ? ? ? 41 BB 08 00 00 00 4C 8D 05 ? ? ? ? 45 8B CB 33 DB 90`
   (both RIP disp32s wildcarded). A post-original emission detour on it
   was tried and REVERTED — the block then provably survived to the
   frame's final chain on the loading screens and STILL didn't render.
3. **Cabinet Tests A/B/C** (menu-bar navigation during loading; env
   `DDR_OVERLAY_DRAW_STOCK_BIND=1` stock program 0 + plain black quad;
   scissor removal): the widget layer IS composed live on the loading
   screens (bar moves), the invisibility is NOT theme-shader-specific,
   and NOT the scissor (dropped from the block permanently — the quad's
   corners already trace the rect; the PS SDF rounds the corners).
4. **Root cause: z-position WITHIN the layer.** A segment-start append
   draws below everything the layer's own walk records that frame. On
   the loading interstitials, the full-screen loading art itself renders
   through the WIDGET layer's walk (the layer register `FUN_18002aa60`
   boot-installs `BM2DGroupWithPan` wrappers into entries 7–10's
   managers — game content flows through the same wrapper walk the DLL's
   widgets use), burying a segment-start quad while the DLL's
   later-registered widgets stay on top. "Same list" does NOT mean
   "same z" — position within the walk is what matters.

### Production architecture (FINAL — cabinet-validated 2026-08-25)

**Identity-gated anchor emission.** The menu creates a dedicated anchor
text widget FIRST in `allocate_widgets` (before the panel; single-space
text, offscreen, permanently hidden — `line_desc+0x49` only gates
rasterization, not the walk's `wrapper_render` dispatch) and publishes
its WRAPPER address + dirty-flag byte to
`overlay_draw::set_emit_anchor`. The wrapper-render hook emits the
background block pre-original ONLY when `this` == the anchor (one atomic
compare per wrapper), into `render_notes_hook::active_command_list()` —
the engine's own installation for the walk in progress, the
verified-safe surface. Post-original, the hook re-arms the anchor's
dirty byte (`render_state+0x68`) while active, so dirty-gated walks keep
dispatching it every frame. z by construction: above everything the
widget layer drew earlier (loading art included), below the panel and
every menu widget. The identity gate is load-bearing: the round-2 spray
(per-(list,frame) dedup, no identity) let an EARLIER game wrapper claim
the emission below the art — that is why it failed on title screens.

Block shape: context(1280,720) → SetVSConstantF(c48/c49) →
SetShader(default, theme_prog) behind the `progs >= idx+1` gate → one
quad (vertex alpha = MENU OPACITY) → SetShader(default, 0). No scissor.
The layer-dispatcher detour remains installed as a pure passthrough —
its successful install is `emitter_ready()` (the menu's ANIMATED
BACKGROUND availability gate). Validated: animations persistent on
every screen incl. both loading windows, 120 emissions/s
(`pre_size≈0x9400` at the anchor — mid-walk, vs 0x24 at segment start),
zero WARNs, zero gaps (maintainer sign-off).
