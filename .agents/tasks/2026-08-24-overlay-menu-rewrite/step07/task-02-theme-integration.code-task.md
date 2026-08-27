# Task: Theme integration — config r/w, chrome re-synthesis, palette repaint, row wiring

## Description
Wire the theme layer into the running menu: the full `overlay_menu`
config section (read at kick, written on change via `save_json_key`);
runtime appearance state (active theme / animate / opacity) owned by
`chrome_loader`; generation-tokened chrome re-synthesis on theme or
opacity change (old panel stays until the replacement resolves);
render.rs palette lookups replacing the hardcoded `COL_*`/`TINT_*`
constants (including re-coloring the six creation-only text widgets so a
theme change repaints live); the real `TabId::Theme` tabs.rs arm; and
the `RowSource::Theme` edit arm in input.rs.

## Background
Step 7 of the overlay-menu rewrite (design §4.6, §4.4, §6). Approved
decisions (2026-08-25): persist on every change (quick_restart
precedent); ANIMATED BACKGROUND defaults ON and is functional-but-inert
(plain toggle; Step 8 adds the availability gate + greyed state);
rapid theme cycling handled with a generation token — latest publish
wins, stale resolves discarded (stale textures load harmlessly and are
never released, per the process-lifetime texture rule).

Current facts (verified 2026-08-25):
- `chrome_loader.rs`: `THEME_ID: &str = "default"` (line 48) is an
  explicit placeholder; `EFFECTIVE_OPACITY: AtomicI32` (:61);
  `KICKED: AtomicBool` once-latch (:63); `PANEL_TEX`/`STRIP_TEX`
  AtomicI64 published on pump resolve (:324-325);
  `PANEL_FAILED`/`STRIP_FAILED` are set-only latches (:57-59);
  `kick()` (:141-171) reads `overlay_menu.opacity`, clamps, spawns ONE
  `synthesis_thread(opacity)` under `catch_unwind`; the gradient is
  hardwired at :242 (`chrome::synthesize_panel(&chrome::DEFAULT_GRADIENT,
  opacity)`); `PendingFile { kind, path, stem }` mailbox → game-thread
  `pump()` → `asset_loader::load` → resolve → store → one `refresh_all`.
  The swap mechanism is already fresh stems: `chrome::panel_file_stem(
  theme_id, opacity)` varies by both, the engine caches textures by name
  hash, and rebinding is a repaint-time `set_texture_id` write.
- `render.rs` color constants (:104-115) and strip tints (:154-169) are
  commented as pre-theme placeholders. Six text widgets are colored at
  creation only and never re-colored by `refresh_all`: title (:251),
  credit (:258), N/M indicator (:283), cursor (:291), footer desc
  (:322), footer hints (:329). Everything else re-colors every
  `refresh_all`. The solid-fallback rung tints the strip with
  `DEFAULT_GRADIENT.top` (:430-439) — must use the active theme's
  gradient. `TextWidget::set_color(r, g, b, a)`;
  `ImageWidget::set_color(abgr)`; the `abgr(a, r, g, b)` helper is at
  :154-156. Fixed alphas keep their current values (selbar 0x38, hdrbar
  0x30, tab indicator 0xE6, banner 0xC8, scroll track 0x26 / thumb 0x80
  stay white).
- `tabs.rs` has the task-01 temporary `TabId::Theme => Vec::new()` arm
  in `rebuild_tabs` (:70-77); `rebuild_and_refresh()` (:158-173) is the
  post-edit rebuild path.
- `input.rs` `activate_selected` (:255-312) holds the
  `RowSource::Theme => {}` stub (:309); `compute_new_value(kind, to_on)`
  (:154-203) already handles bool flip / scalar ±step with Start-coarse /
  enum clamp-at-ends; the repeat thread services any Scalar/Enum row
  regardless of source (:348-352) — the opacity row repeats for free.
- `config.rs`: `OverlayMenuConfig { opacity: Option<i32> }` (:343-352)
  with the doc comment pre-announcing this step's fields;
  `save_json_key(key, value)` (:680-698) replaces the WHOLE top-level
  section — every write must serialize all three keys.
- Persist precedent: `src/mods/quick_restart_or_fail.rs`
  `load_delay_from_config`/`persist_delay`/`set_delay` (:363-394) —
  atomic store + whole-section `save_json_key` from the input/repeat
  thread, non-blocking, no game calls.
- `mod.rs`: `chrome_loader::kick()` is the first line of `enable`
  (:173); `schedule_coalesced_refresh()` (:252-267) is the coalesced
  repaint entry.
- theme.rs (task-01): `THEMES`, `Palette` (text `[f32;3]`, tints
  `[u8;3]`, `panel_top`/`panel_bottom` `[u8;4]`, `gradient()`),
  `resolve_theme_index(Option<&str>) -> (usize, bool)`,
  `theme(index) -> &'static Theme`, `DEFAULT_THEME_INDEX`.
- model.rs (task-01): `build_theme_tab(theme_index, theme_labels,
  animate, animate_greyed, opacity) -> Vec<Row>` with row keys
  `"theme"` / `"animate_bg"` / `"opacity"`.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-24-overlay-menu-rewrite/design/detailed-design.md (§4.6 theme system, §4.4 configuration, §6 error ladder)

**Additional References (if relevant to this task):**
- .agents/tasks/2026-08-24-overlay-menu-rewrite/step07/task-01-theme-model.code-task.md (the pure layer this consumes)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. **config.rs**: `OverlayMenuConfig` gains `#[serde(default)] pub
   theme: Option<String>` and `#[serde(default)] pub
   animate_background: Option<bool>`; update the struct doc comment
   (the section is now DLL-written on THEME tab change).
2. **chrome_loader appearance state** (chrome_loader owns all three —
   it already owns opacity and needs theme; animate rides along as the
   one-stop appearance store):
   - Delete the `THEME_ID` const. Add `ACTIVE_THEME: AtomicUsize`
     (index into `theme::THEMES`), `ANIMATE: AtomicBool` (default
     true), and a `SYNTH_GENERATION: AtomicU32`.
   - `pub(super) fn active_theme_index() -> usize`,
     `pub(super) fn animate_background() -> bool`,
     `pub(super) fn effective_opacity() -> i32` accessors (relaxed
     loads; render/tabs consumers).
   - `kick()` reads the FULL section: theme via
     `theme::resolve_theme_index` (unknown id ⇒ one WARN naming the
     value + arrows fallback — design §6), `animate_background`
     defaulting true, opacity via the existing clamp. Stores all three,
     then synthesizes with the active theme's id + gradient
     (`theme::theme(idx).palette.gradient()` replaces
     `DEFAULT_GRADIENT` at the :242 call site; stems/keys already take
     the theme id).
3. **Re-synthesis** — `pub(super) fn resynthesize()`:
   - Bump `SYNTH_GENERATION`; capture `(generation, theme_index,
     opacity)`; clear `PANEL_FAILED`; spawn a synthesis thread
     (catch_unwind like kick's) that regenerates the PANEL only (the
     strip is theme/opacity-invariant) under the new stem/key and
     deposits a `PendingFile` extended with the generation.
   - `pump()` publish: store `PANEL_TEX` only when the entry's
     generation equals the current `SYNTH_GENERATION` (stale resolves
     are discarded — the texture loads harmlessly and is never
     released). The boot kick participates in the same scheme
     (generation 0/initial).
   - Failure of a re-synthesis marks `PANEL_FAILED` only if its
     generation is still current (a stale failure must not knock out a
     healthy newer panel).
   - The old panel keeps rendering until the replacement resolves —
     this falls out of publish-on-resolve; assert nothing clears
     `PANEL_TEX` on the re-synthesis path.
4. **Persistence** — `fn persist_overlay_menu()` (chrome_loader or
   mod.rs, implementer's choice): reads the three atomics + theme id
   and calls `config::save_json_key("overlay_menu", json!({ "theme":
   id, "animate_background": animate, "opacity": opacity }))` —
   whole-section, all keys, every change (quick_restart precedent; runs
   on the input/repeat thread, no game calls).
5. **render.rs palette lookups**: replace the `COL_*` consts and the
   RGB portion of the `TINT_*` consts with lookups off
   `theme::theme(chrome_loader::active_theme_index()).palette`
   (suggest one `fn pal() -> &'static theme::Palette` + small helpers;
   fixed alphas stay local consts). Scroll track/thumb stay white.
   Re-color the six creation-only text widgets (title, credit, N/M
   indicator, cursor, footer desc, footer hints) in `refresh_all` so a
   theme change repaints them live. The solid-fallback rung (:430-439)
   tints with the ACTIVE theme's `panel_top`. Resolve `pal()` once per
   `refresh_all`, not per widget.
6. **tabs.rs**: replace the temporary arm with
   `TabId::Theme => model::build_theme_tab(chrome_loader::active_theme_index(),
   &labels, chrome_loader::animate_background(), false,
   chrome_loader::effective_opacity())` where `labels` collects
   `theme::THEMES` displays (animate_greyed hardcoded false until
   Step 8's availability gate).
7. **input.rs `RowSource::Theme` arm** in `activate_selected`: compute
   via the shared `compute_new_value`, then match `row.key`:
   - `"theme"`: new enum index → store `ACTIVE_THEME`, persist,
     `chrome_loader::resynthesize()`, `tabs::rebuild_and_refresh()`
     (the repaint applies the new palette immediately; the panel swaps
     when the new texture resolves).
   - `"animate_bg"`: store `ANIMATE`, persist, `rebuild_and_refresh()`
     (visually inert until Step 8).
   - `"opacity"`: clamp via `chrome::clamp_opacity`, store
     `EFFECTIVE_OPACITY`, persist, `resynthesize()`,
     `rebuild_and_refresh()`.
   - Unknown key: debug-log and ignore (defensive; no panic — this arm
     is hook-reachable).
8. **No new WARN classes beyond**: unknown config theme (once, at
   kick). Re-synthesis failures reuse the existing latched
   `WARNED_SYNTH`/`WARNED_LOAD` paths.
9. **Budget note**: `TEXT_WIDGET_COUNT` already grew by 1 via
   `TabId::ALL.len()` (task-01); verify the headroom check constants
   need no further edits.

## Dependencies
- task-01 (theme.rs table, `TabId::Theme`, `build_theme_tab`) — must be
  complete first.

## Implementation Approach
1. config.rs fields; chrome_loader state + full-section kick read +
   gradient threading (boot path only — verify a normal boot still
   synthesizes/loads under the arrows stems).
2. Generation-tokened `resynthesize()` + pump generation check.
3. render.rs palette lookups + creation-only re-coloring + fallback
   tint.
4. tabs.rs real arm; input.rs Theme arm + persistence.
5. Gates: `./scripts/validate_mod_menu.sh` (67+ tests from task-01) →
   `cargo check --target x86_64-pc-windows-msvc` → `cargo fmt` (bare)
   → `./build.sh`.
6. Autonomous runtime check (attract suffices; theme rows are
   cabinet-wide): deploy, boot, open the menu via
   `cli keypads write 0 "000"`, tab to THEME (`cli keypads write 0
   "3"` ×3), cycle THEME / adjust MENU OPACITY via `press` helpers,
   harvest log.txt for the persist lines + re-synthesis + resolve +
   zero WARNs/panics; confirm mod-config.json gained the full
   `overlay_menu` section; relaunch and confirm the selection loaded.
   Screenshots archived for the maintainer — no visual verdicts.

## Acceptance Criteria

1. **Config round-trip**
   - Given `overlay_menu: { "theme": "wavefield", "opacity": 65 }` in
     mod-config.json
   - When the DLL boots
   - Then the menu paints the WAVEFIELD palette, synthesis runs under
     the wavefield stems, and the THEME tab rows show WAVEFIELD /
     ON / 65%.

2. **Unknown theme falls back**
   - Given `overlay_menu: { "theme": "bogus" }`
   - When the DLL boots
   - Then one WARN naming the value, the RHYTHM (arrows) theme applies,
     and a subsequent THEME edit persists a valid id.

3. **Live theme change**
   - Given the menu open on the THEME tab
   - When THEME is cycled
   - Then text/tint colors repaint immediately (including title, footer
     and cursor), mod-config.json's `overlay_menu` section is rewritten
     with all three keys, and the panel texture swaps only when the new
     synthesis resolves (the old panel never disappears first).

4. **Rapid cycling is latest-wins**
   - Given several THEME/opacity changes in quick succession
   - When the in-flight syntheses resolve out of order
   - Then only the latest generation publishes `PANEL_TEX`; stale
     resolves and stale failures are discarded; no WARN storm.

5. **Relaunch persistence**
   - Given a theme + opacity selected via the menu
   - When the game relaunches
   - Then the same theme/opacity load from `overlay_menu` (the Step 7
     demo gate).

## Metadata
- **Complexity**: High
- **Labels**: mod-menu, theme, chrome, config, integration
- **Required Skills**: Rust, repo hook-DLL conventions (panic-free hook paths, render-thread marshaling), game-nav harness
- **Generated By**: code-task-generator 2026-08-25
- **Source Plan**: .agents/planning/2026-08-24-overlay-menu-rewrite/implementation/plan.md
- **Plan Step**: Step 7: THEME tab — theme system with static backgrounds
