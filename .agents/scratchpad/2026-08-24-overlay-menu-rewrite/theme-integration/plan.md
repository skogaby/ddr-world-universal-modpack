# Plan — task-02 theme-integration

Status: Approved 2026-08-25 (auto mode — descends from the approved plan
step; see context.md)

## Implementation order

1. config.rs: `OverlayMenuConfig` += `theme: Option<String>`,
   `animate_background: Option<bool>`; doc update.
2. chrome_loader.rs:
   - Statics: delete `THEME_ID`; add `ACTIVE_THEME: AtomicUsize`,
     `ANIMATE: AtomicBool(true)`, `SYNTH_GENERATION: AtomicU32(0)`.
   - Accessors `active_theme_index/animate_background/effective_opacity`
     + setters `set_active_theme/set_animate/set_effective_opacity` +
     `persist_overlay_menu()` (whole-section save_json_key).
   - `kick()`: full-section read (theme resolve + WARN on unknown,
     animate default true, opacity clamp), then synthesis of both
     pieces at generation 0.
   - `resynthesize()`: gen bump, PANEL_FAILED clear, panel-only
     synthesis thread; `PendingFile`/`Loading` carry `generation`;
     pump + failure paths guard Panel publishes/failures on
     `generation == SYNTH_GENERATION`.
3. render.rs: `pal()` lookup; COL_*/TINT_RGB → palette fields (fixed
   alphas stay); `set_rgb`/`tint` helpers; six creation-only widgets
   re-colored in refresh_all; solid-fallback tint from
   `pal().panel_top`.
4. tabs.rs: real `TabId::Theme` arm over
   `theme::THEMES` labels + chrome_loader state.
5. input.rs: `RowSource::Theme` arm (key match on the model consts;
   theme/opacity ⇒ store+persist+resynthesize+rebuild_and_refresh;
   animate ⇒ store+persist+rebuild_and_refresh).

## Test / validation scenarios

- Host: existing 36 harness tests stay green (pure layers untouched by
  this task except none — chrome.rs/model.rs/theme.rs unchanged).
- Gates: check 0 warnings / fmt / build.sh clean.
- Runtime (autonomous, attract): boot with no `overlay_menu.theme` ⇒
  arrows stems synthesized/resolved, no WARN; open menu, tab ×3 to
  THEME; cycle THEME right ⇒ log shows persist + synthesis under
  `mm_panel_bubbles_*` + resolve; adjust opacity ⇒ same under new stem;
  mod-config.json contains {theme, animate_background, opacity};
  relaunch ⇒ same selections load; `"theme": "bogus"` boot ⇒ one WARN +
  arrows. No panics.
- Maintainer demo: palette + panel swap live; opacity slider;
  relaunch persistence (Step 7 demo gate).
