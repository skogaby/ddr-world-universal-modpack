# Progress — Overlay Menu Rewrite

Updated: 2026-08-25
Status: FEATURE COMPLETE — all 9 steps implemented and maintainer-signed-off
(uncommitted — maintainer commits manually)
NEXT ACTION: none for this feature. Follow-ups live OUTSIDE this feature:
the maintainer's full docs overhaul + open-sourcing prep (separate session;
README intentionally untouched here), and archiving this planning dir to
`.agents/planning/_archive/` at the maintainer's convenience.

Final walkthrough (2026-08-25): maintainer approved everything; one
feedback round applied same-day — option-row labels are Title Case (ALL
CAPS reserved for headers, tab labels, and enum value labels; convention
recorded in api.rs::display_name docs + enforced style-wise by review, the
lint checks presence only).

Resume protocol: read `implementation/plan.md` (step checklist + specs),
`design/detailed-design.md` (approved design), `idea-honing.md` (decision register).
Task files land under `.agents/tasks/2026-08-24-overlay-menu-rewrite/step<NN>/`;
per-task working records under `.agents/scratchpad/2026-08-24-overlay-menu-rewrite/`.

## Done

- PDD complete: register (22 decisions, idea-honing.md), research (orientation /
  widget-rendering / shader-spike), design approved 2026-08-24, plan approved
  2026-08-24 (9 steps), summary.md written.
- **Step 1 complete (uncommitted):** `src/mods/mod_menu.rs` → `src/mods/mod_menu/`
  module split (zero behavior change) + widget-pool boot diagnostic in
  `widget_renderer` — **254 free nodes** (design §4.5 pool risk retired).
- **Step 2 complete (uncommitted): GO for shader backgrounds.**
  `src/services/overlay_draw/` — pure record encoder (12 host tests,
  `scripts/validate_overlay_draw.sh`) + `DDR_OVERLAY_DRAW_POC`-gated quad emitter at
  the wrapper pre-original site. Autonomous gates passed (list valid + bump invariant
  in every scene; default shader boot-resident, progs=2; 18k emissions zero-fault).
  Maintainer z-probe: quad ABOVE all game content, BELOW DLL text widgets, scissor
  pixel-exact — the exact game→background→widgets sandwich the menu needs. RE notes +
  production recipe: `docs/overlay_draw_research.md`. POC stays env-gated until
  Step 8 promotes it.
- **Step 3 complete (uncommitted), maintainer-signed-off:** tabbed shell — pure model
  (`src/mods/mod_menu/model.rs`, 13 host tests via `scripts/validate_mod_menu.sh`)
  + shell integration (`tabs.rs` new; `rows.rs`/`input.rs`/`render.rs` reworked;
  registration API frozen — five registrant mods untouched). TOGGLE MODS +
  GLOBAL SETTINGS tabs (per-mod section headers, uppercase), 12-row dense layout in
  the amended modal footprint (1160×600 @ (60,60) — maintainer wants only ~50–75 px
  unobscured margins), footer = selected-row description (0.55 scale) + verbatim
  key legend, N/M overflow indicator, right-aligned value column, active-tab grow
  affordance (no brackets), "(by skogaby)" credit beside the title, pinpad nav
  removed (menu buttons only; pinpad = 0-0-0 close, 1/3 tabs), header scroll-trap
  fixed in `follow_scroll` (+2 regression tests). Functional walkthrough validated
  toggles/adjusts/persistence; maintainer confirmed gestures + final visuals.

- **Step 4 complete (uncommitted), maintainer-signed-off ("everything looks
  perfect"):** modal chrome. task-01: pure `src/mods/mod_menu/chrome.rs` (SDF
  rounded-rect coverage, vertical gradient panel 1160×600 r20 with baked
  opacity, 64×16 white strip, PNG encode, clamp+snap 25..=100/5, cache
  keys/stems; 10 host tests; harness mounts model.rs+chrome.rs with the
  `image` dep). task-02: `overlay_menu.opacity` config section;
  `src/mods/mod_menu/chrome_loader.rs` (boot-kicked background synthesis →
  CacheHasher sidecars under `data_mods/_cache/mod_menu/` → game-thread
  asset_loader load/resolve pump → atomics; ladder piece-Failed ⇒ solid strip
  rung ⇒ text-only, one latched WARN each; `DDR_MOD_MENU_CHROME_FAULT`
  dev faults); `widget_renderer::free_node_count()`; render.rs chrome
  (panel + 12 header bars + tab underline + selection bar + proportional
  scrollbar) created BEFORE text in `allocate_widgets` (z = creation order)
  behind a 17+32-node headroom check. Feedback round: tab labels center-
  aligned over their underlines (`TextAlignment::Center` at the underline
  midpoint; grow affordance now symmetric).

## In flight

- Nothing — feature complete.

- **Step 9 COMPLETE (uncommitted), maintainer-signed-off 2026-08-25**
  ("everything looks great"; one feedback round: Title Case option labels —
  30 hunks incl. the APPEARANCE built-ins + model test + lint-window fix,
  details in the display-strings-sweep scratchpad).
  Breakdown approved 2026-08-25 (3 tasks; maintainer amendments: README
  dropped — full docs overhaul happens in a separate session before
  open-sourcing; recommendations accepted: agent-authored strings, profile
  rows + headers stay both-menus, contributed rows untouched). Tasks
  (`.agents/tasks/.../step09/`, per-task records in the scratchpad):
  task-01 display-strings-sweep — all 41 custom-option rows gain explicit
  `.display_name` (canonical `option_strings.py` en labels) +
  `.description` (agent-authored); enum labels on perspective /
  training_progress_pos / customize_movie_size; webui cosmetics
  `.in_game_only()` + `def.display_name` wired; headers → tuple table;
  grep-based display-string lint in validate_custom_options.sh (proven
  red/green). Cabinet: 41/41 registered on a rebuild boot, 0 new WARNs.
  task-02 removals-and-ribbons — mwsl X/Y overlay rows + save wiring
  removed (config reads intact); STOCK_RIBBONS += seop_op_left/right —
  the 6 atlas-REBUILD WARNs are gone (validated on a forced-rebuild boot).
  task-03 agents-md-refresh — row_order → option_menu_settings, new
  overlay_menu bullet, new Mod Menu entry-point row, mwsl row corrected;
  README untouched per maintainer.

- Step 8 (below) is COMPLETE and signed off.

- **Step 8 COMPLETE (uncommitted), maintainer-signed-off 2026-08-25**
  ("everything works perfectly — animations persistent on every screen,
  every frame, no interruptions"). Three tasks as summarized below, PLUS
  a fourth feedback round that settled the FINAL emitter architecture:
  the layer-dispatcher segment-start append vanished on two loading
  interstitials (the loading art renders through the WIDGET layer's own
  wrapper walk, burying a segment-start quad — "same list ≠ same z").
  Final: IDENTITY-GATED ANCHOR emission — a hidden anchor text widget
  created FIRST in the menu's `allocate_widgets`
  (`widget_renderer::create_text_widget_with_wrapper`), published via
  `overlay_draw::set_emit_anchor(wrapper, dirty_addr)`; the wrapper-render
  hook emits the block into the ACTIVE command list pre-original only for
  that wrapper and re-arms its dirty byte post-original; no scissor in
  the block (redundant — quad corners = rect, PS SDF rounds corners);
  layer-dispatcher detour kept as passthrough (`emitter_ready`). Dev
  debug env: `DDR_OVERLAY_DRAW_STOCK_BIND=1`. Full investigation trail
  (frame-reset RE incl. the `FUN_1801f6e30` AOB, probe/tag-dump craft,
  Tests A/B/C): docs/overlay_draw_research.md §Loading-screen
  investigation + the emitter-promotion scratchpad round-4 entry +
  learnings.md.
  Breakdown
  approved 2026-08-25 (3 tasks; recommendations: wrapped time mod 3600 s,
  MINIMAL greyed, soak rides normal play; the layer-identity gate was
  replaced mid-implementation by its pre-approved fallback — see below).
  Tasks (`.agents/tasks/.../step08/`):
  task-01 theme-shaders — `shaders/src/themes/` (shared passthrough VS
  `vs_theme_main` forwarding c48/c49 via interpolators + arrows/bubbles/
  wavefield ps_3_0 effects, all wrap-seamless frequencies); 4 new blobs
  via the fxc golden path (14–164 instr); build_shaders.sh manifest.
  task-02 theme-synthesis — pure `avs_layeredfs/shader_layout.rs`
  (planned/default_programs/default_theme_indices; persp EXACTLY at 1;
  themes LAST; 5 tests, mounted in validate_overlay_draw.sh → 18);
  `Plan.themes` (shader-fixes ∧ mod-menu ∧ 4 blobs, soft-degrade);
  default container synthesized whenever persp||themes; fingerprint v3;
  `overlay_draw::publish_theme_programs`/`theme_program_indices()`
  (published on BOTH build and cache-hit paths). Cabinet: 5-program
  container (gsp_pack-inspected), indices 2/3/4, shader-fixes-off ⇒
  stock + no publish.
  task-03 emitter-promotion — production overlay_draw (POC env gate
  REMOVED; `set_background` atomic feed; c48={time%3600,x,y,0}/
  c49={w,h,p0,p1}; theme bind behind `progs>=idx+1`; restore; 60-fail
  session latch); theme.rs `Background::Shader{ThemeProgram}` (MINIMAL
  Static); mod_menu `update_background_feed()` at open/close/theme/
  animate + `background_available()` → animate_greyed. **New RE fact:**
  BmpString wrappers render through the hooked path only while DIRTY —
  a menu-owned anchor wrapper fired once per repaint, so the emitter
  uses the POC's per-(list,frame) gate (spike-verified sandwich);
  recorded in docs/overlay_draw_research.md §Step 8 production outcome.
  Cabinet: heartbeats prove program 2→3→4 theme switching, a 14 s
  emission gap on MINIMAL, stop on animate-off/close; ~200–240
  emissions/s; 0 panics, 0 new WARNs. Screenshots (14) in
  `.agents/scratchpad/2026-08-24-overlay-menu-rewrite/emitter-promotion/shots/`.
  **Feedback round (2026-08-25, maintainer live testing) — 3 fixes
  landed + validated** (details in the emitter-promotion scratchpad):
  (1) MENU OPACITY now drives the shader's master fade (quad vertex
  alpha; PS 0.92 base dropped); (2) all-scene visibility via the
  COMBINED gating — self-sustaining dirty-flag anchor (frame guarantee
  in static scenes; new `TextWidget::mark_dirty`/`dirty_flag_addr`,
  post-original re-arm) + every-wrapper spray (composes above attract
  movies); (3) rounded corners via a TEXCOORD2-fed r=20 SDF mask in the
  theme PS (blobs rebuilt), plus the gradient panel dims to a ~35 %
  wash (`PANEL_ALPHA_OVER_ANIMATION`) over a live animation so the
  shader stays visible at 100 % opacity.   Validated: static-scene dwell
  (anchor-only frames, arena 0x108), 2-min attract cycle (44
  heartbeats, no gaps), opacity sweep. Screenshots 15–19 archived.
  **Feedback round 2 (2026-08-25): title-screen invisibility is a
  LAYER-COMPOSITION limit, not a frame-drive bug** — the list active at
  wrapper rasterization composes below title art/movies in some scenes.
  The all-slots-walk fix attempt CRASHED (the layer table is NOT
  walkable — only `active_command_list()` is safe; lesson in
  learnings.md + the research doc) and was fully reverted. Shipped:
  active-list emitter + anchor chain — animation shows in text-churning
  scenes (attract songs, gameplay, menus), title-class scenes degrade
  to the static gradient. Proper all-scene fix = follow-up RE of the
  layer walk (documented in docs/overlay_draw_research.md).
  Demo state on the cabinet: arrows(RHYTHM) / animate ON / opacity 95.
  **Feedback round 3 (2026-08-25) — all-scene rendering SOLVED (ship
  gate):** RE spike found the per-frame LAYER DISPATCHER
  (`layer_dispatcher` signature + `layer_table` derivation, unique on
  both builds); the emitter is now a detour on it, appending
  pre-original to the WIDGET layer's private override CommandList
  (manager pointer-identity; the widget layer = table entry 7, an
  override entry — full trail in docs/overlay_draw_research.md).
  Spray/anchor machinery fully retired. Validated: full attract cycle,
  16.8k emissions into the widget layer's list, zero gaps/panics/WARNs
  — the quad now rides the exact layer whose text shows in every scene
  (title, movies, loading screens, above song-wheel jackets).

- Step 9 (sweep/removals/docs) — not started.

- **Step 7 complete (uncommitted), maintainer-signed-off 2026-08-25**
  ("everything looks great"; one feedback round applied: the 4th tab's
  label is **APPEARANCE**, not THEME — model.rs label + test; enum stays
  `TabId::Theme`). Breakdown
  approved 2026-08-25 (2 tasks; 4 recommendations accepted: agent-authored
  palettes, persist-every-change, animate default ON, generation-tokened
  re-synthesis; the placeholder-display-strings question resolved as the
  planned Step 9 sweep, not a Step 5/6 gap). Tasks
  (`.agents/tasks/.../step07/`):
  task-01 theme-model — pure `theme.rs` (Palette: 11 text `[f32;3]` +
  3 tint `[u8;3]` + panel stops; `gradient()`; `THEMES` ×4 arrows/RHYTHM,
  bubbles/BUBBLES, wavefield/WAVEFIELD, minimal/MINIMAL, all Static;
  `resolve_theme_index` arrows-fallback; clamped `theme()`), `TabId::Theme`
  (ALL len 4, 4-tab wrap tests updated), `build_theme_tab` (keys
  `theme`/`animate_bg`/`opacity`; opacity formatted "NN%"), harness
  MODULES += theme.rs → 36 tests.
  task-02 theme-integration — `OverlayMenuConfig` += theme/
  animate_background; chrome_loader owns the appearance state (THEME_ID
  const gone; kick reads the full section, unknown theme ⇒ one WARN +
  arrows; `resynthesize()` = generation-tokened latest-wins panel-only
  regen, PANEL_FAILED cleared, publish-on-resolve keeps the old panel up);
  render.rs COL_*/TINT_RGB → `pal()` palette lookups (fixed ALPHA_*
  consts), six creation-only widgets re-colored per refresh, fallback
  tint from panel_top; tabs.rs real Theme arm; input.rs Theme arm
  (store + whole-section `save_json_key("overlay_menu")` + resynthesize +
  rebuild).
  **Autonomous runtime validation passed** (see Deploy log). Screenshots
  in `.agents/scratchpad/2026-08-24-overlay-menu-rewrite/theme-integration/shots/`.
  Demo state left on the cabinet: wavefield / animate off / opacity 70.
  Palette RGBA values are the agent-authored first cut — maintainer
  approved as-is; future tweaks are table edits in theme.rs.

- **Step 6 complete (uncommitted), maintainer-signed-off 2026-08-25** (mirror
  + reverse mirror, selector for one/two-side sessions, session gating,
  round-trip persistence — all validated). Two tasks
  (`.agents/tasks/.../step06/`):
  task-01 player-tab-model — TabId::PlayerSettings, pinned-slot navigation
  (NavState.pinned_focus + Navigator::new_with_pinned; selector joins the
  wrap cycle at the top), RowKind::Scalar.formatted (Option<String>),
  MirroredRowSnap/build_player_tab, eligibility fns (editable_sides/
  resolve_selected_side/selector_state, fail-closed); 30 model tests.
  task-02 player-tab-integration — snapshot wiring + per-tab geometry
  (PLAYER = selector band + 11 rows; slots/bars/scrollbar repositioned per
  refresh), marshaled Mirrored edits (gate re-check inside the closure),
  selector side-switch (Free ⇒ LEFT=P1/RIGHT=P2), NO ACTIVE SESSION /
  OPTIONS FRAMEWORK UNAVAILABLE banners over a strip backing, observer +
  scene-change coalesced repaint (REFRESH_PENDING latch), gesture-side
  capture.
- **Two fixes landed during Step 6 validation** (details in the task-02
  scratchpad progress): (1) scripts/game_nav/launch.sh was missing
  `-audiohookdisable` → the documented Wine movie-graph crash under
  movie_mode=fallback (harness gap, not a DLL bug); (2) PRE-EXISTING
  Training Mode highlight-seeder ping-pong (chart vs audio publications
  alternately re-stamping ROWS_DIGEST when persistently skewed — 66k seeds
  → cabinet wedge 2026-08-25) fixed in driver.rs::select_step: the audio
  seeder now runs ONLY when no chart publication exists (`chart.is_none()`).
  Post-fix live session: seeds proportional (83), 1 audio fallback, 0 panics.
- 2026-08-25 (Step 6 demo, maintainer): mirror + reverse mirror, selector
  (solo locked / versus free-switch), session gating banner, round-trip
  persistence — ALL validated. Step 6 ticked.

- **Step 5 complete (uncommitted), 3 tasks** (breakdown user-approved incl.
  press-path observer, no registration-prime dispatch, format_scalar_value →
  api.rs, SJIS ± → UTF-8 "±"):
  - task-01 ordering-placement-core: `row_order` DELETED (schema now
    `custom_options.option_menu_settings` [{id, overlay?, in_game?}], array
    order = display order both menus); ordering.rs reworked (settings store +
    `placement_override_for`); repo AND cabinet mod-config.json migrated
    (cabinet backup /tmp/mod-config.json.pre-migration.bak); new harness
    `scripts/validate_custom_options.sh` (log-stub macros + once_cell).
  - task-02 registration-surface: api.rs `MenuPlacement` + `RegisterSpec.
    {menus, display_name, description}` + builders + `EnumValue.display_label`
    /`with_display` + bool_toggle OFF/ON labels + `prettify_id`/
    `prettify_texture_suffix` + `format_scalar_value` moved to api.rs +
    `format_scalar_value_utf8`; registry copy-through (headers may carry
    display strings/placement); builder_hook resolved-!in_game filter;
    asset_gen label skip for non-in-game rows (probe-verified via log).
  - task-03 observer-snapshot: `observers.rs` (token multicast, panic-
    contained, never under lock) wired at ALL SIX mutation paths (incl. the
    in-game press path; observers changed-only — caught by a red test that
    exposed the primitive's unconditional-write contract);
    `overlay_snapshot(side)` (`OverlayRowInfo`/`OverlayRowKind`; availability
    ⊕ placement ⊕ configured order ⊕ live bounds ⊕ per-side ShowWhen reported
    not filtered; formatted parity); `show_when_satisfied` moved to registry
    (shared evaluator). Harness 40 tests total.
  - ~~PENDING (rides with Step 6 demo)~~ DONE 2026-08-25: maintainer ran the
    in-game MODS tab regression — "everything looked good". Step 5 fully
    signed off.
  - Step 9 note: atlas-REBUILD boots WARN ×6 on seop_op_left/right (missing
    from asset_gen STOCK_RIBBONS — pre-existing; add them there).

## Deploy & test log

- 2026-08-25 (Step 8 loading-screen fix, 6 deploys): survival-probe build
  (wipe hypothesis killed — signature collision caught by tag dump);
  tag-dump build (chains proved block present on CAUTION yet invisible);
  post-reset-emission build (block in FINAL chain, still invisible);
  transition-telemetry build (no list switches — emission side fully
  correct); Tests A/B (stock-bind env) + C (scissor removal) — draw path
  live, not shader-specific, not scissor; ANCHOR build — maintainer
  sign-off: animations persistent on every screen incl. both loading
  windows (`pre_size≈0x9400` mid-walk, 120/s, 0 WARNs). Harnesses
  18+36+40; check 0 warnings; builds clean throughout.

- 2026-08-25 (Step 8 autonomous, attract): task-01 — all 8 blobs compile
  on the fxc golden path, 4 pre-existing byte-stable. task-02 — default
  container 8236 B / 5 programs / 3 VS / 4 PS; indices 2/3/4 published
  (build AND cache-hit boots); v3 sidecar; shader-fixes-off ⇒ stock, no
  publish. task-03 — three diagnostic deploys nailed the dirty-wrapper
  finding (anchor emitted once per repaint), then the per-(list,frame)
  build validated end-to-end: continuous heartbeats (program follows the
  THEME row 2→3→4), MINIMAL/animate-off/close all stop emission, 0
  panics, 0 new WARNs across the churn.
- 2026-08-25 (Step 7 autonomous, attract-only — theme rows are
  cabinet-wide): boot with NO overlay_menu section ⇒ arrows default
  (`mm_panel_arrows_80` synthesized, strip cache hit, both resolved,
  0 WARNs); THEME cycled arrows→bubbles→wavefield ⇒ per-theme panel
  synthesis + resolve ~200 ms each; ANIMATED BACKGROUND toggled;
  opacity 80→70 ⇒ per-opacity stems; mod-config.json gained
  `{"theme":"wavefield","animate_background":false,"opacity":70}`;
  relaunch ⇒ cache hit `mm_panel_wavefield_70` (persistence);
  `"theme":"bogus"` boot ⇒ exactly one WARN + arrows fallback. 0 panics.
  Harnesses 36+40+12; check 0 warnings; build clean.
- 2026-08-24 (Step 1): boot clean; free pool 254; no regressions.
- 2026-08-24 (Step 2 diagnostics + POC boots): all gates green; multi-list-per-frame
  finding recorded. Maintainer z-probe session: GO.
- 2026-08-24 (Step 3): keypad-injected functional walkthrough passed (FPS TARGET
  120→144→120 persisted; hello-world OFF→ON→OFF persisted+logged; zero panics).
  Feedback round applied (tabs/footer/credit/pinpad/scroll-trap); one silent-patch
  slip caught by the maintainer (bracketed tabs survived one deploy). Final visual
  sign-off received.
- 2026-08-24 (Step 4 autonomous): cold boot (cache cleared) — both PNGs
  synthesized (panel 15 327 B, strip 277 B) + sidecars, textures resolved <1 s
  after the first wrapper frame, free pool 254, no new WARNs; warm boot — both
  cache hits + resolve; fault boot (`DDR_MOD_MENU_CHROME_FAULT=panel`) — exactly
  one WARN, panel Failed, strip resolved (solid rung armed). No panics. Harness
  23/23 + 12/12; check 0 warnings; build clean. Maintainer visual sign-off
  pending (Step 4 demo gate).
- 2026-08-24 (Step 4 sign-off): maintainer visual review — approved after one
  feedback round (tab labels center-aligned over the underlines; underline
  placement kept). Step 4 ticked in the plan.
- 2026-08-24/25 (Step 5 autonomous): task-01 boot — migrated 41-entry config
  parsed clean (no unknown-id WARN), headers 4/4; task-02 probe boot —
  `"in_game": false` on timing_stats ⇒ "label texture skipped" INFO + revert
  boot back to the 6 pre-existing WARNs (found: atlas-REBUILD boots add 6
  seop_op_left/right WARNs — pre-existing STOCK_RIBBONS gap, Step 9);
  task-03 boot — inert (no subscribers), 0 panics, 41 registrations.
  Harnesses: custom_options 40/40, mod_menu 23/23, overlay_draw 12/12;
  check 0 warnings; builds clean. Step 5 ticked; in-game MODS tab eyeball
  rides with Step 6's demo session.
- Test capability notes: agent may autonomously build (`./build.sh`), deploy (copy
  DLL into the CrossOver bottle install), boot, harvest `$DDR_WORLD_INSTALL/log.txt`
  (attract ~30 s after window creation; ~35–40 s wait suffices), and kill the game.
  **ALL visual verification is the maintainer's** (standing instruction) — hand off
  with what to check; do not render visual verdicts from screenshots.

## Deviations & open questions

- The music_wheel_song_length X/Y rows and the webui in_game placement etc. are
  later-step work (Steps 5/9) — GLOBAL SETTINGS currently shows the X/Y rows still.
- `rows.rs` keeps its own registration `RowKind` alongside `model::RowKind`
  (public API frozen; tabs.rs converts) — intentional, documented in both files.

## Key facts for a cold resume

- Git: NEVER commit — maintainer commits manually; task completion recorded as
  `Complete (uncommitted)` in each task's scratchpad progress.md.
- Readiness gates: `cargo check --target x86_64-pc-windows-msvc` → `cargo fmt`
  (whole crate) → `./build.sh`. Host tests via the temp-crate harnesses
  (`scripts/validate_mod_menu.sh`, `scripts/validate_overlay_draw.sh`) — plain
  `cargo test` cannot compile retour on ARM hosts.
- Step 8 (animated shader backgrounds) is GO per the Step 2 spike; production
  recipe in `docs/overlay_draw_research.md`.
- Step 5's display-string fallbacks (prettified ids) make the Step 9 sweep polish,
  not a blocker.
