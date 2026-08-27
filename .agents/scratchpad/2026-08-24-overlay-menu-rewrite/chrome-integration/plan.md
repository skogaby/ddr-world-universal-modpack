# Plan — chrome-integration (Step 4 task-02)

Status: Approved 2026-08-24 (auto mode — approval supplied by the verified chain:
task Generated-By + plan/design both `Status: Approved 2026-08-24`; see context.md)

## Implementation approach (5 pieces, in order)

1. **config.rs** — `OverlayMenuConfig { opacity: Option<i32> }`, `ConfigFile`
   field, both default blocks.
2. **widget_renderer.rs** — extract the free-pool walk:
   private `walk_free_pool() -> Result<usize, &'static str>`, public
   `free_node_count() -> Option<usize>`; `log_free_pool_count_once` rewritten
   on top (keeps its per-reason unavailability lines).
3. **mod_menu/chrome_loader.rs** (new, impure) —
   - Atomics: PANEL_TEX/STRIP_TEX (AtomicI64, −1 sentinel), PANEL_FAILED/
     STRIP_FAILED (AtomicBool), EFFECTIVE_OPACITY (AtomicI32, default 80);
     `status() -> ChromeStatus` snapshot for render.rs.
   - `kick()` (from enable, once-latched): read+clamp opacity; spawn the
     synthesis thread (`catch_unwind`; panic ⇒ both pieces Failed + WARN).
   - Synthesis thread: ensure `./data_mods/_cache/mod_menu/`; per piece
     (panel: stem `chrome::panel_file_stem("default", opacity)`, key
     `chrome::cache_key_material`; strip: stem `chrome::strip_file_stem()`,
     key `chrome-strip:v{LAYOUT_VERSION}`): CacheHasher hit-check → synthesize
     + encode_png + write + commit on miss → deposit (kind, path, stem) into a
     PENDING mailbox; failures mark the piece Failed (latched WARN). Then
     schedule the pump.
   - Pump (game thread, self-requeues while work remains): drain PENDING →
     `asset_loader::load` (unavailable/refused ⇒ Failed + WARN) storing the
     AssetHandle (never released — process-lifetime); poll `resolve_hash` →
     publish texture id; on any transition schedule one `refresh_all` (via
     MOD_MENU_STATE lock, is_open-gated).
   - `DDR_MOD_MENU_CHROME_FAULT` = panel|strip|load fault injection.
4. **mod.rs** — 6 new widget state fields (panel, 12 header bars Vec, tab
   indicator, selection bar, scroll track, scroll thumb) + Lazy init;
   `mod chrome_loader;`; `enable()` calls `chrome_loader::kick()`.
5. **render.rs** — `MODAL_H`; chrome geometry + ABGR tint constants
   (`const fn abgr`); `allocate_chrome_widgets` called FIRST in
   `allocate_widgets` (headroom check: skip all chrome + one WARN when
   `free_node_count()` < chrome(17)+text(32); None ⇒ proceed); refresh wiring
   (panel bind incl. strip-stretch fallback rung, header bars per visible
   Header slot, tab underline indicator, selection bar tracking the cursor,
   proportional scrollbar on overflow); hide_all/destroy cover chrome.

## Test scenarios

Engine-facing task — no host-testable surface (pure layer landed in task-01;
plan.md Step 4 lists only those tests). Validation:
- Gates: harness (untouched tests stay green) → check → fmt → build.
- Autonomous cabinet boot: log shows synthesis (miss) → load → resolve lines;
  second boot shows cache hits; no WARN/panic.
- Maintainer visual sign-off (the step's demo): rounded 80 % modal, selection
  bar, tab indicator, scrollbar on overflow, header bars.
- Ladder probe (optional autonomous): boot with DDR_MOD_MENU_CHROME_FAULT=panel
  ⇒ one WARN + solid-rung path selected in log.

## Risks
- Loose-PNG UV mapping assumed 0..1 full-content (strip_hud precedent).
- Chrome geometry values are first-deploy guesses — visual tuning expected.
