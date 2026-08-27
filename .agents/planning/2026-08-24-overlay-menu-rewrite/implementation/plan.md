# Implementation Plan: Overlay Menu Rewrite (Mod Menu v2)

Status: Approved 2026-08-24

Design: `.agents/planning/2026-08-24-overlay-menu-rewrite/design/detailed-design.md`
(Approved 2026-08-24). Each step leaves the DLL buildable (`cargo check` → `cargo fmt` →
`./build.sh`) and cabinet-deployable with demonstrable behavior. Cabinet deploys are the
only real validation for engine-facing work; host tests cover every pure layer in the
same step that introduces it. Maintain `progress.md` in this feature directory
throughout (per AGENTS.md).

## Checklist

- [x] Step 1: Module restructure + widget-pool diagnostic
- [x] Step 2: Shader spike — command-list emitter POC (go/no-go for animated themes)
- [x] Step 3: Tabbed shell — row model, MODS + GLOBAL SETTINGS tabs, dense layout
- [x] Step 4: Modal chrome — synthesized panel, scrollbar, opacity
- [x] Step 5: custom_options extensions — placement, display strings, observer, snapshot, `option_menu_settings`
- [x] Step 6: PLAYER SETTINGS tab — mirroring, side selector, session gating
- [x] Step 7: THEME tab — theme system with static backgrounds
- [x] Step 8: Animated shader backgrounds
- [x] Step 9: Registration sweep, removals, documentation

## Steps

### Step 1: Module restructure + widget-pool diagnostic

**Objective:** Create the `src/mods/mod_menu/` subdirectory with the existing behavior
intact, and de-risk the widget budget.

**Guidance:** Mechanical split of `mod_menu.rs` into `mod.rs` (lifecycle/gesture/state),
`rows.rs` (row model + registration API), `input.rs` (exclusive consumer + repeat
thread), `render.rs` (widgets/layout/refresh) — no behavior change; public API paths
(`crate::mods::mod_menu::{register_scalar_row, register_enum_row, remove_rows_for, …}`)
unchanged so the five registrants compile untouched. Add the boot-time free-pool walk in
`widget_renderer` (count nodes from the free head to the sentinel; one INFO line).

**Tests:** None new (pure restructure). Existing behavior re-verified on cabinet.

**Integration:** Baseline for every later step.

**Demo:** Menu opens/navigates/toggles exactly as before; boot log shows
`widget pool: N free nodes`.

### Step 2: Shader spike — command-list emitter POC

**Objective:** Prove (or disprove) the animated-theme mechanism before any theme work:
draw a stock-shader tinted quad, scissored, in attract / song select / gameplay, above
the game and below our widgets. This is the design's only genuinely new RE and its
go/no-go decides whether Step 8 exists.

**Guidance:** New `src/services/overlay_draw.rs` with a **pure record-encoding layer**
(builds scissor/SetShader/SetVSConstantF/quad/restore byte sequences into a caller
buffer) and an impure emitter called from `wrapper_render_hook`
(`overlay_draw::on_wrapper_render()`, relaxed-atomic no-op when inactive). Gate the POC
behind a dev env var (e.g. `DDR_OVERLAY_DRAW_POC`) so production builds are unaffected
until Step 8. Follow the staged gates from the design §4.7: per-scene active-list
diagnostics → tinted quad (stock program 0) → z-probe (emission point vs widget creation
order) → arena-headroom measurement. Every read range-validated; every gate fail-open
with one latched WARN. Record outcomes (incl. the z recipe and headroom numbers) in a
`docs/` research note.

**Tests:** Host tests for the record-encoding layer (exact tag/size/payload layouts per
the documented tag map; size-chaining integrity across a multi-record emission).
Cabinet: the staged success criteria above; full attract→gameplay loop without crash.

**Integration:** Consumes `render_notes_hook::active_command_list()` and the derived
default-shader global; hooks into Step 1's `widget_renderer` call site.

**Demo:** With the env var set: a translucent tinted rectangle clipped to a test rect,
visible in all three scene classes, under a test text widget. **If the spike fails:**
record why, mark Step 8 dropped, and let themes ship static-only (design's degrade
path) — later steps proceed unchanged.

### Step 3: Tabbed shell — row model, MODS + GLOBAL SETTINGS tabs, dense layout

**Objective:** The core UX rewrite: unified row model, tab bar with the first two tabs,
~12-row density, footer, cursor-skip logic. Text-only rendering (chrome arrives in
Step 4).

**Guidance:** Implement `Row`/`RowKind`/`RowSource` and `tabs.rs` builders per design
§4.2: MODS from registry entries; GLOBAL SETTINGS from contributed rows grouped under
per-mod section headers (`parent_row_key` reinterpreted as owning-mod id; disabled mod ⇒
group hidden). New layout constants in `render.rs` (row height ≈34, text scale ≈0.55,
footer block, `N/M` text). `input.rs`: `1`/`3` tab switch with per-tab cursor memory,
header/greyed skip with wrap, repeat thread unchanged. Only MODS and GLOBAL tabs render
in this step — the tab bar shows what exists.

**Tests:** Host tests (pure builders over injected snapshots): MODS list from a fake
registry (mod-menu excluded, order preserved); GLOBAL grouping/hiding matrix; navigation
model (header/greyed skip, wrap, scroll clamp — encode the underflow guard from the old
file as a test); tab cursor memory.

**Integration:** Replaces Step 1's `rebuild_rows`/`refresh_slots` internals; the
registration API and toggle/persist paths are unchanged underneath.

**Demo:** Open the menu: tab bar (MODS / GLOBAL SETTINGS), 12 dense rows, footer showing
the selected row's description + key hints, `1`/`3` switching tabs, all existing toggles
and scalar/enum rows working from their new homes.

### Step 4: Modal chrome — synthesized panel, scrollbar, opacity

**Objective:** Make it look like a modal: rounded-corner panel with baked gradient at
configurable opacity, selection bar, tab indicator, scrollbar.

**Guidance:** `chrome.rs` per design §4.5: background-thread PNG synthesis (`image`
crate) → `data_mods/_cache/mod_menu/` with hash sidecars → async `asset_loader` load
kicked at boot. Read `overlay_menu.opacity` (25–100, clamp+snap 5; full section lands in
Step 7). Widget allocation order = z order: panel first, then chrome sprites, then text
(text rows allocate before decorative chrome so pool exhaustion degrades looks, not
function — see design §6 ladder). Fallback ladder: solid strip texture → text-only.

**Tests:** Host tests: synthesized panel dimensions, corner-alpha profile, opacity
mapping, cache-key stability (opacity change ⇒ new hash; unchanged inputs ⇒ cache hit).

**Integration:** Renders beneath Step 3's text layout; no row-model changes.

**Demo:** The menu is a rounded, 80 %-opaque modal floating over visible gameplay edges,
with a selection bar, active-tab indicator, and a proportional scrollbar + `N/M` readout
on overflow.

### Step 5: custom_options extensions

**Objective:** Everything the PLAYER SETTINGS tab needs from the framework, landed and
host-tested before any overlay consumer exists — plus the `row_order` removal.

**Guidance:** Per design §4.3–4.4, all additive except the removal: `MenuPlacement` +
`RegisterSpec.menus` (+ builder setters; `builder_hook` filters `!in_game` like
unavailable rows); `display_name`/`description`/`EnumValue.display_label` with
prettified-id fallbacks (no registrant sweep yet — fallbacks carry Step 6);
`subscribe_value_changed` multicast (dispatched after lock release, panic-contained, on
every mutation path); `overlay_snapshot(side)`; `format_scalar_value` exposed to the
snapshot builder. `ordering.rs` rework: `OptionMenuSetting` parse,
`set_configured_settings`, order + placement-override computation, **`row_order` reading
code deleted**.

**Tests:** Host tests: config parse (optional booleans, unknown keys/ids ⇒ warn-once
path); order computation (listed-first, unlisted append, unlisted-headers-excluded);
placement resolution matrix (registration default ⊕ config override incl. "neither");
snapshot construction over a synthetic registry (availability, placement, ShowWhen
per side, live bounds, formatted-scalar parity across every `ScalarFormat` variant);
observer dispatch (all mutation paths fire; panicking subscriber contained; no dispatch
under lock).

**Integration:** In-game menu behavior unchanged by default (all placements default
BOTH; no config ⇒ registration order). The overlay doesn't consume any of it yet.

**Demo:** Cabinet regression: in-game MODS tab renders/orders/persists as before; an
`option_menu_settings` test config reorders the in-game tab and hides a row via
`"in_game": false`; a `row_order` key in config is ignored.

### Step 6: PLAYER SETTINGS tab — mirroring, side selector, session gating

**Objective:** The third tab: live-mirrored per-player options with side selection and
session gating.

**Guidance:** Per design §4.2/§4.8/§4.9: tab builder over `overlay_snapshot(side)`;
pinned `CONFIGURING: PLAYER 1/2` selector (default = gesture side from
`InputEvent.player`; eligibility = `side_entered` ∧ scene outside 2..=16, fail-closed);
`NO ACTIVE SESSION` banner + all-greyed rendering; edits marshaled via
`run_on_render_thread` → `set_value`, with the gate re-checked inside the closure;
observer subscription marks rows dirty → repaint next refresh. Headers render as
section separators.

**Tests:** Host tests: gating/eligibility matrix (entered × scene band × records-
unavailable); side-selector lock/grey states; PLAYER list construction from synthetic
snapshots (ShowWhen filtering, header skip, greyed-all state); edit-refusal on
stale-open race (pure state machine).

**Integration:** First consumer of every Step 5 API; Steps 3–4 shell hosts the tab.

**Demo (cabinet):** Card in on P1 → PLAYER SETTINGS shows the full mirrored option set;
edit PREMIUM FREE in the overlay → in-game MODS tab shows it flipped (and vice versa);
values persist through card-out (network + JSON as per each option's PersistMode);
during attract the tab shows the banner with everything greyed; versus shows both sides
selectable and independent.

### Step 7: THEME tab — theme system with static backgrounds

**Objective:** The fourth tab and the full appearance system, shipping static-gradient
backgrounds (animated arrives in Step 8).

**Guidance:** `theme.rs` per design §4.6: `Theme`/`Palette` table with all four built-ins
(palettes final; backgrounds all `Static` for now); THEME / ANIMATED BACKGROUND /
MENU OPACITY rows; full `overlay_menu` config section read at init + written on change
(`save_json_key` pattern); theme/opacity change triggers async chrome re-synthesis
(old panel until the replacement resolves) + immediate palette repaint of all text/chrome
tints.

**Tests:** Host tests: theme table integrity (unique ids/displays, every palette entry
populated); config round-trip (unknown theme ⇒ default + warn; opacity clamp/snap);
chrome cache keys vary by theme id.

**Integration:** Reuses Step 4's synthesis/caching; the ANIMATED BACKGROUND row is
functional-but-inert (greyed once Step 8's availability gate reports no shader path;
plain toggle until then).

**Demo:** Cycle THEME on the tab: palette and panel gradient change live (panel swaps
when re-synthesis resolves); opacity slider works; selections survive a relaunch via
`overlay_menu` in mod-config.json.

### Step 8: Animated shader backgrounds

**Objective:** The showpiece: per-theme procedural animated backgrounds via the game's
command list, gated on Step 2's proven recipe. **Skip this step entirely if Step 2
concluded no-go** (themes remain static; ANIMATED BACKGROUND row stays greyed).

**Guidance:** Per design §4.7: extend `scripts/build_shaders.sh` + `shaders/src/themes/`
with the shared passthrough VS and three theme pixel shaders (arrows / bubbles /
wavefield) — committed `.d3dbc` blobs under `data_mods/shader_fixes/blobs/`; extend
`shader_synthesis` to append theme programs to `gs_screencommand_default` **after** all
existing programs in every configuration (perspective stays index 1 — assert it),
recording indices for the emitter. Promote `overlay_draw` from POC to production:
open-state + animations + theme gates, c48/c49 constant block, modal-rect scissor, full
state restore, arena-headroom refusal, session-latching WARN on repeated failure.
Remove the POC env-var path.

**Tests:** Host tests: constant-block encoding; synthesis-plan tests asserting program
ordering/indices across AA×perspective×theme configurations (extend the existing
synthesis fingerprint tests). Build gate: all theme HLSL compiles via fxc 9.29.
Cabinet: per-theme visual check in attract/select/solo/versus gameplay; scene-churn +
multi-hour soak (design §7); perspective + AA regression (stock lane visuals unaffected).

**Integration:** Composes Step 2's emitter, Step 7's theme table (backgrounds flip from
`Static` to `Shader{program}`), and the shader-fixes synthesis pipeline. ANIMATED
BACKGROUND row now live; shader-fixes disabled ⇒ static degrade with greyed row.

**Demo:** RHYTHM theme scrolls a subtle DDR-arrow field behind the modal in any scene;
BUBBLES/WAVEFIELD likewise; toggling ANIMATED BACKGROUND off snaps to the static
gradient; disabling shader-fixes and relaunching degrades identically with one WARN.

### Step 9: Registration sweep, removals, documentation

**Objective:** Polish pass that makes the mirrored menu read like a product, and land
the agreed removals + docs.

**Guidance:** Sweep every custom-option registration site (~30 options across ~12 mods)
adding explicit `display_name`/`description`/enum display labels (replacing prettified
fallbacks); `webui_options` cosmetics gain `.in_game_only()`. Remove the Music Wheel
Song Length X/Y overlay rows + `save_json_key` wiring (config keys still read at
enable). Update README (Mod Menu feature row, configuration section, complete-example
JSON: `row_order` → `option_menu_settings`, new `overlay_menu` section, theme list) and
AGENTS.md (mod_menu entry-point row, config section notes). Add/refresh the `docs/`
research note from Step 2 with final emitter facts.

**Tests:** No new code paths — compile gates + a display-string lint-style host test
(every registered option resolves a non-fallback display name; every enum value has a
display label). Cabinet regression: full menu walkthrough on all tabs; music-wheel
offsets still honored from config.

**Integration:** Final integration/cleanup; no orphaned code remains (POC flag gone in
Step 8, old `mod_menu.rs` gone in Step 1, `row_order` gone in Step 5).

**Demo:** Every row in both menus carries a curated label + footer description; the
README documents the new config schema end-to-end; a fresh operator can theme and
configure the menu from docs alone.
