# Research — Existing Row-Ordering Mechanism

Findings from reading the custom-options framework. All facts verified against source
at the time of writing.

## Where row order comes from today: implicit registration order

Custom option rows are injected into the game's native MODS tab (page 6) by a detour,
in the order options were **registered**. There is no explicit ordering field anywhere.

- `src/services/custom_options/registry.rs`
  - `FrameworkState.options: Vec<RegisteredOption>` — **append-only, never reordered**
    (docstring: "entries are append-only, never reordered").
  - `OptionHandle(u32)` wraps the option's **index into `options`**. Handles must stay
    stable for the process lifetime, so we **cannot reorder `options`** to change display
    order.

- `src/services/custom_options/builder_hook.rs` — `builder_detour_body`
  - Runs on the render thread when the native row builder runs (menu open).
  - Calls the original builder first (native rows land), then:
    - Snapshots `state.options.iter().enumerate()` into
      `handles: Vec<(OptionHandle, String /*id*/, RowKindTag)>` — **in registration
      order**.
    - `rows::clear_side(side)` then, **for each handle in that order**, allocates a row
      and appends it via the native `option_tab_register` helper (a `push_back`).
  - **This iteration order is the single lever that determines on-screen order.** Both
    the scene-graph row vector (visual order) and `rows::ROWS` (scroll/visibility order)
    follow it.

- `src/services/custom_options/rows.rs`
  - `static ROWS: Vec<RowSlot>` — append-only; slots pushed in builder_hook injection
    order (so ROWS order == builder_hook iteration order).
  - `row_ptrs_for_side(side)` returns ROWS entries for the side, filtered by `ShowWhen`,
    **preserving vector order** → consumed by the scroll driver
    (`options_scroll` via `custom_options::row_handles_for_tab`). So reordering the
    builder-hook iteration keeps scene order and scroll order consistent automatically.

### Consequence

The **minimal, correct** change is to reorder the `handles` snapshot in
`builder_detour_body` according to a configured order, **without** touching the
`options` Vec (so `OptionHandle` indices stay valid). Everything downstream (ROWS push
order, scroll order) follows for free.

## ShowWhen (parent/child) is order-independent

`rows::is_show_when_satisfied` resolves a `ShowWhen::Equals { parent_id, .. }` by
**id lookup** into `state.options`, not by position. So visibility works regardless of
where the parent and child land in display order. Reordering a child (e.g. `weight`)
ahead of its parent (`is_disp_weight`) is **functionally safe** — only visually unusual.

Parent/child pairs that currently exist:
- `weight` → child of `is_disp_weight` (webui_options `profile_fields.rs`).
- `pacemaker_threshold` → likely child of `pacemaker_to_mserror` (power_user_statistics)
  *(to re-confirm at implementation time)*.

## Config plumbing

- `src/mods/config.rs`
  - `CustomOptionsConfig` (deserialized from `custom_options`) already carries
    `persist_network`, `persist_json`, `lane_gamma_correction: Option<f32>`,
    `preview_window: Option<i32>`, `animate_backgrounds: Option<bool>`, all `#[serde(default)]`.
    Adding `row_order: Option<Vec<String>>` with `#[serde(default)]` is a one-line
    addition; absent → `None`.
  - Config is read **once** at boot into a `OnceCell` (`config::init`, early in the init
    sequence, before services init).
  - All DLL write paths (`save_mod_states`, `save_custom_options_values`,
    `save_json_key`, migration) do **read-modify-write on the raw JSON** and only touch
    named keys → a hand-authored `row_order` is **preserved** across all DLL writes. The
    DLL never needs to write `row_order` itself (operator-authored only, like
    `preview_window`).

- **Precedent for a service reading config:** `custom_options_persistence.rs` already
  does `use crate::mods::config;` and reads `config::get()...custom_options` at its init.
  So `custom_options::init()` reading `config::get()...row_order` is consistent with the
  codebase (no layering violation introduced).

- Init ordering: `config::init` → services (`custom_options::init`, then
  `custom_options_persistence::init`) → mods register + `enable_with_config` (options get
  registered here) → menu opens fire `builder_hook` much later. So:
  - Reading the configured order list at `custom_options::init` is safe (it's just
    strings; options need not exist yet).
  - The registry is **complete** by the time `builder_hook` first fires, so unknown-id
    validation there is accurate.

## Registered option ids (current universe)

Static (always the same id):
- `premium_free`, `autoplay`, `center_arrows_1p`
- `timing_stats`, `pacemaker_to_mserror`, `pacemaker_threshold`, `step_data_export`
- `overlay_scale`, `overlay_opacity` (overlay_element_styling)
- `arrow_scale`, `arrow_opacity` (playfield_styling)
- `is_disp_weight`, `weight` (webui_options profile_fields)

WebUI cosmetics (`webui_options/discovery.rs` `CATEGORIES`, registered only when assets
are discovered / mod enabled):
- `customize_appeal_board`, `customize_background`, `customize_background_gameplay`,
  `customize_character_p1`, `customize_character_p2`, `customize_lane_single`,
  `customize_lane_double`, `customize_lanecover_single`, `customize_lanecover_double`,
  `customize_movie_size`

Note: the exact set present at runtime depends on which mods are enabled and (for webui)
which assets exist on the cabinet. An id in `row_order` that isn't registered this boot
(disabled mod, absent assets) is indistinguishable from a typo at builder-hook time →
handled by the "warn + ignore" rule.

## Non-goals confirmed by reading

- The DLL overlay **mod menu** (triple-0) is a separate system (`mods/mod_menu.rs` +
  `register_scalar_row`), not the custom-options MODS tab. `row_order` does **not** apply
  there.
- Native game option rows on other tabs are untouched — the framework only injects (and
  thus only reorders) its own rows, appended after the native rows on page 6.
