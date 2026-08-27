# Progress — task-02 theme-integration

- [x] config.rs OverlayMenuConfig fields
- [x] chrome_loader appearance state + full-section kick + gradient threading
- [x] chrome_loader generation-tokened resynthesize + pump guards
- [x] render.rs palette lookups + creation-only re-coloring + fallback tint
- [x] tabs.rs real Theme arm
- [x] input.rs Theme arm + persistence
- [x] gates: harness → cargo check → cargo fmt → ./build.sh
- [x] autonomous runtime check (boot / theme cycle / persist / relaunch / bogus)

## Log

- 2026-08-25 config.rs: `OverlayMenuConfig` += `theme: Option<String>`,
  `animate_background: Option<bool>`; doc notes the section is DLL-written.
- 2026-08-25 chrome_loader.rs: `THEME_ID` const deleted; appearance state
  (`ACTIVE_THEME`/`ANIMATE`/`EFFECTIVE_OPACITY`) + accessors/setters +
  `persist_overlay_menu()` (whole-section save_json_key). `kick()` reads
  the full section (unknown theme ⇒ one WARN + arrows). New
  `resynthesize()`: `SYNTH_GENERATION` bump + `PANEL_FAILED` clear +
  panel-only synthesis via shared `spawn_synthesis(generation,
  theme_index, opacity, include_strip)`; `PendingFile`/`Loading` carry
  `generation`; `mark_failed` and the pump's Panel publish are
  generation-guarded (stale resolves/failures discarded with an INFO).
  Gradient threaded from `theme::theme(idx).palette.gradient()`.
- 2026-08-25 render.rs: `COL_*` consts → `pal()` palette lookups
  (`set_rgb` helper); `TINT_*` RGB → palette tints with fixed
  `ALPHA_*` consts (`tint` helper); scroll track/thumb stay white
  consts. The six creation-time-colored widgets (title, credit, N/M
  indicator, cursor, footer desc, footer hints) re-color every
  `refresh_all`; solid-fallback rung tints with `pal().panel_top`.
- 2026-08-25 tabs.rs: real `TabId::Theme` arm (labels from
  `theme::THEMES`, state from chrome_loader accessors, animate_greyed
  hardcoded false until Step 8).
- 2026-08-25 input.rs: `RowSource::Theme` arm — key match on the model
  consts; theme/opacity ⇒ store + persist + resynthesize +
  rebuild_and_refresh; animate ⇒ store + persist + rebuild; unknown key
  ⇒ INFO + ignore. No lock held across any of it.
- Gates: mod_menu harness 36/36, custom_options 40/40, check 0
  warnings, fmt, build.sh clean (logs/).

## Deploy & runtime validation (2026-08-25, autonomous, attract)

- Boot with NO overlay_menu section: arrows default —
  `mm_panel_arrows_80.png` synthesized, strip cache hit, both resolved,
  0 WARNs.
- Menu opened via keypad 000; tab ×3 → THEME tab. THEME cycled right
  twice: `mm_panel_bubbles_80` then `mm_panel_wavefield_80` synthesized
  + resolved ~200 ms each (panel swap on resolve). ANIMATED BACKGROUND
  toggled OFF; MENU OPACITY 80→75→70: `mm_panel_wavefield_75`/`_70`
  synthesized + resolved. mod-config.json gained
  `{"theme":"wavefield","animate_background":false,"opacity":70}`.
  0 panics, no ModMenu WARNs.
- Relaunch: cache hit `mm_panel_wavefield_70.png` — persisted state
  loaded (Step 7 demo-gate leg).
- `"theme":"bogus"` boot: exactly one WARN
  (`unknown overlay_menu.theme "bogus" — using "arrows"`), arrows
  applied at the configured opacity 70. Config restored to
  wavefield/false/70 afterward.
- Screenshots archived under `shots/` (01 menu open … 06 relaunch) for
  the maintainer — visual verdicts are theirs.

## Deviations
- None against the task. `set_effective_opacity` uses Release ordering
  to pair with `status()`'s Acquire (the other two appearance atomics
  are Relaxed like the task implied — render-thread reads tolerate a
  frame of lag).

Status: Complete (uncommitted — maintainer commits manually)
