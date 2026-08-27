# Progress — task-03 emitter-promotion

- [x] encode.rs production-sequence host test (c48/c49 payload pinned)
- [x] overlay_draw production emitter (POC gate removed, activation feed,
      constants, theme bind, failure latch)
- [x] theme.rs Background::Shader + ThemeProgram (+ test)
- [x] mod_menu feed wiring (open/close/theme/animate) + availability gate
- [x] gates: harnesses (18+36+40) → cargo check → cargo fmt → ./build.sh
- [x] cabinet validation (all acceptance criteria; see log)

## Log

- 2026-08-25: encode.rs `production_background_sequence_walks_back_exactly`
  (context/scissor/consts/bind/quad/restore/scissor-off; c48/c49 floats +
  absolute payload ptr + bind-vs-restore program indices byte-checked).
- 2026-08-25: overlay_draw production rework — `set_background(Option<
  BackgroundParams{program, rect, params}>)` atomic feed; POC env gate /
  POC_RECT / POC_COLOR deleted (`DDR_OVERLAY_DRAW_POC` grep-clean in
  src/); `emit_background()` keeps the POC's entire gate ladder + adds
  `set_vs_const_f(0, [[time%3600, x, y, 0],[w, h, p0, p1]])`, theme bind
  behind `progs >= program+1`, restore to program 0, opaque-black quad;
  `note_gate_failure()` 60-consecutive session latch; first-emit INFO +
  600-emission heartbeat (carries the program id).
- 2026-08-25: theme.rs `ThemeProgram{Arrows,Bubbles,Wavefield}.slot()` +
  `Background::Shader{program}`; arrows/bubbles/wavefield flipped,
  MINIMAL Static; `background_mapping` test replaces the all-static guard.
- 2026-08-25: mod_menu — `MENU_OPEN` atomic mirror; `background_available()`
  (active theme Shader ∧ indices published) feeds tabs.rs animate_greyed;
  `update_background_feed()` at open/close (close = immediate stop) and
  in the input.rs theme/animate arms; `render::modal_rect()` single
  source of truth (60,60,1160,600).

## Deviations

- **Layer-identity anchor REPLACED by the POC's per-(list,frame) gate**
  (the pre-approved fallback). Cabinet diagnosis: a menu-owned anchor
  wrapper emitted exactly once per repaint — NEW RE FACT: BmpString
  wrappers render through the hooked path only while DIRTY (static text
  is served from a cached path; the game's own wrappers re-render
  continuously, which is why the POC/emitter work from any wrapper).
  Two intermediate builds (offscreen space anchor, in-modal glyph anchor)
  proved the diagnosis; the anchor machinery was then fully reverted
  (create_text_widget_with_wrapper, set_emit_anchor, bg_anchor_widget all
  removed — no orphaned code). The POC gate's z-sandwich was already
  maintainer-verified in the Step 2 probe. Recorded in
  docs/overlay_draw_research.md §"Step 8 production outcome".

## Deploy & cabinet validation (2026-08-25)

- Synthesis leg (see task-02): default container 5 programs, indices
  2/3/4 published on build + cache-hit; shader-fixes-off ⇒ no publish
  (⇒ the row greys via `background_available()` = indices None — the
  greyed calculation is the same pure OR verified in the tabs arm).
- Emitter leg (arrows/animate-on persisted): first-emit INFO
  `program=2, rect=60,60,1160,600`; heartbeats every ~2.5–3 s
  (~200–240 emissions/s, arena 0xBF8–0xC58); theme cycle heartbeats
  show program 2→3→4; **MINIMAL = 14 s heartbeat gap** (emission
  stopped); animate-off + menu close stop emission (no heartbeats
  after close). 0 panics; 0 overlay_draw/mod_menu WARNs (the log's 6
  WARNs are the pre-existing Series Expansion signature misses).
- Screenshots (14) archived under `shots/` for the maintainer — visual
  verdicts are theirs (esp. the animated field vs the static gradient
  in 09–14).
- Demo state left on the cabinet: arrows / animate ON / opacity 80.

## Feedback round (2026-08-25, maintainer live testing)

Three observations, all fixed + cabinet-validated:

1. **Opacity didn't affect the shader** — the quad's master-fade alpha was
   hardcoded 0xFF. Fixed: `BackgroundParams.alpha` carries
   `chrome::opacity_alpha(effective_opacity())` into the quad's vertex
   color; the theme PS multiplies by it (the 0.92 base constant dropped —
   100 % = opaque); the opacity row edit now refreshes the feed.
2. **Scene-dependent visibility** — two separate causes, now both solved
   by COMBINING the mechanisms: (a) static scenes (title/language select)
   render no dirty game text ⇒ no wrapper renders ⇒ no emission — fixed
   by the SELF-SUSTAINING ANCHOR (refresh_all seeds its dirty flag at
   render_state+0x68; `after_wrapper_render` — post-original, because the
   game's render pass clears the flag — re-arms it every frame while
   active; `TextWidget::mark_dirty`/`dirty_flag_addr` added); (b) the
   anchor's own single-list emission sat BELOW the attract movie
   (maintainer saw flashes during movie-load black) — fixed by emitting
   on EVERY wrapper render with the per-(list,frame) dedup (the Step 2
   spray, which composes above movies). Anchor = frame guarantee;
   spray = layer coverage.
3. **Hard corners** — the theme VS now passes TEXCOORD2
   {px_in_rect, rect_w, rect_h}; every theme PS multiplies alpha by an
   r=20 rounded-rect SDF coverage (mirrors chrome.rs). Blobs rebuilt
   (arrows 103 instr / bubbles 176 / wavefield 57 / VS 15 — all inside
   SM3).
   Plus: `PANEL_ALPHA_OVER_ANIMATION = 0x59` — while the animation is
   live (`overlay_draw::is_background_active()`), the gradient panel
   tints to a ~35 % wash so the animation stays visible even at
   MENU OPACITY 100 (maintainer's "can't see animations at 100%").

Validation (launch12): static-scene dwell — continuous heartbeats with
arena 0x108 (the anchor chain alone driving frames — the designed
behavior); 2-minute attract-cycle dwell — 44 heartbeats, ~220
emissions/s, zero gaps, 0 panics; feed log line flips Some(program) ⇄
None on open/close; opacity sweep persisting. Screenshots 15–19
archived in shots/.

## Feedback round 2 (2026-08-25, maintainer live testing)

Report: boot-check flicker, title screen shows NO animation (attract
gameplay fine). Diagnosis chain:

- The anchor keeps EMISSION alive everywhere (title-dwell heartbeats
  with near-empty arenas), but the list active at a wrapper's
  RASTERIZATION is not the layer its display quads composite in —
  the anchor-time list composes BELOW title art / the attract movie.
  Scene-dependent top-layer coverage is a real layer-composition
  problem, not a frame-drive problem.
- **Attempted fix (all-slots walk) CRASHED the game**: appending to
  every layer-table slot (`state+0x40+slot*8`, 0..=8) with full null +
  bump-invariant gates still hit a torn/concurrently-consumed list —
  in-engine crash through gamemdx's render path. REVERTED to the
  active-list-only emitter (the verified-safe surface). Lesson recorded
  in `.agents/learnings/learnings.md` + docs/overlay_draw_research.md
  ("the layer-slot table is NOT walkable").
- Shipped state (safe build, 60 s soak clean, 12k emissions): active
  list + per-(list,frame) dedup + the self-sustaining anchor. Animation
  visible in text-churning scenes (attract songs, gameplay, most
  menus); title-screen-class scenes degrade to the static gradient
  (harmless — emission lands in a non-composed layer). Proper fix =
  new RE of the layer walk/composition order (emit at the widget
  layer's own draw) — documented follow-up, not in this step.

Status: Complete (uncommitted — maintainer commits manually)

## Feedback round 3 (2026-08-25): the layer-dispatcher emitter (FINAL)

Maintainer requires all-scene animations (ship gate). RE spike via
Ghidra (docs/overlay_draw_research.md §layer dispatcher + §override
entry): found `FUN_18002af10` — the once-per-frame layer dispatcher
(11-entry table at the derived `layer_table` global; the ONLY writer of
the active-list index). New signature `layer_dispatcher` (unique on
20260721 + 20260616) + `derive_layer_table`. The emitter is now a
GenericDetour on the dispatcher: pre-original, it walks the table with
the dispatcher's own conditions and appends the block to the WIDGET
layer's entry — an OVERRIDE entry whose override pointer is the layer's
private CommandList (manager pointer-identity match;
last-walked-slot fallback). Retired: the wrapper-render spray, the
per-(list,frame) gates, the self-dirty anchor, TextWidget::mark_dirty/
dirty_flag_addr, create_text_widget_with_wrapper (all reverted — no
orphaned code); added `widget_renderer::render_list_manager()`
(lock-free via a scene-mgr mirror) + `render_notes_hook::
command_list_at` + `overlay_draw::emitter_ready()` (feeds the menu's
availability gate). Iteration trail: first pick (last-walked fallback,
entry 5/slot 0) worked title+most scenes but drew UNDER the song-wheel
jacket and vanished on loading screens (maintainer); the manager-match
fix (entry 7's private list) validated with 16.8k emissions across a
full attract cycle, zero gaps/panics/WARNs.

Status: Complete (uncommitted — maintainer commits manually)

## Feedback round 4 (2026-08-25): loading screens — the ANCHOR emitter (FINAL, maintainer-signed-off)

The dispatcher emitter vanished on two loading interstitials (scene 21
CAUTION, scenes 26/27 pre-gameplay) while menu text/panel stayed up.
Four diagnostic rounds (full trail in docs/overlay_draw_research.md
§Loading-screen investigation):

1. Survival probe + stale-chain tag dumps — found the frame-begin reset
   `FUN_1801f6e30` (rewind + 0x24 prefix on all 8 private lists; AOB
   verified unique on 20260721+20260616, recorded in the research doc);
   a post-reset emission detour proved the block SURVIVES to the final
   chain on loading screens and still doesn't render. (Probe gotcha: a
   first-8-bytes survival check collides with the walk's own `07:14`
   context record — only full chains tell the truth.)
2. Cabinet Tests A/B/C: menu bar moves live on the loading screens
   (composition is live), `DDR_OVERLAY_DRAW_STOCK_BIND=1` stock-program
   plain quad equally invisible (not shader-specific), scissor removal
   no effect (scissor permanently dropped from the block — redundant).
3. Root cause: Z WITHIN THE LAYER. The loading art renders through the
   WIDGET layer's own wrapper walk (boot-registered BM2D group wrappers
   in the same manager) — a segment-start quad draws under it.
4. Fix: IDENTITY-GATED ANCHOR emission. A hidden anchor text widget is
   created FIRST in `allocate_widgets`
   (`create_text_widget_with_wrapper`); `overlay_draw::set_emit_anchor`
   publishes its wrapper + dirty byte; the wrapper hook emits into the
   ACTIVE list pre-original only for that wrapper and re-arms the dirty
   byte post-original. Dispatcher detour kept as passthrough
   (`emitter_ready`); reset detour + all probe diagnostics removed;
   `command_list_frame_reset` signature removed from code (bytes live in
   the research doc).

Validated on cabinet: `pre_size≈0x9400` at the anchor (mid-walk) vs
0x24 at segment start; 120 emissions/s, zero WARNs. Maintainer:
"everything works perfectly this time — animations persistent on every
screen, every frame, no interruptions."

Status: Complete (uncommitted — maintainer commits manually)
