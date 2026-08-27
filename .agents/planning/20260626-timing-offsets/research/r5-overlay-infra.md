# R5 — Overlay infra (cabinet-button suppression + scalar rows)

Codebase research for the two `mod_menu` upgrades the timing mod needs: (A) cabinet
menu-button navigation with Start-held coarse adjust + game-side suppression of those
buttons while the overlay is open, and (B) typed scalar rows with parent/child visibility.
All facts below are from the current source (`src/services/input_manager.rs`,
`src/mods/mod_menu.rs`, `src/types/buttons.rs`, `src/services/custom_options/api.rs`).

## A. Input model

### What exists today

- **`input_manager` resolves the five menu-button getters** `arkMDXGetStart`, `arkMDXGetUp`,
  `arkMDXGetDown`, `arkMDXGetLeft`, `arkMDXGetRight` via `GetProcAddress`, all with signature
  `unsafe extern "C" fn(i32 player, *mut u32 trigger, *mut u32 hold)` (the `TriggerHoldFn`
  type). It polls them every frame in `poll_player` and ORs `(trigger|hold) & 0xFF` into the
  per-player held bitmask `player_state[p]`.
- **Held state is already exposed:** `input_manager::get_button_state(Player) -> u32` returns
  the current held bitmask. So on a `MENU_LEFT`/`MENU_RIGHT` press event we can read
  `get_button_state(player) & button::START` to choose coarse vs. fine — **no new infra
  needed for the hold gesture.** (`button::START = 1<<0`.)
- **The overlay already half-accepts cabinet buttons:** `mod_menu::handle_exclusive_input`
  matches `MENU_UP|NUM_8`, `MENU_DOWN|NUM_2`, `MENU_LEFT|NUM_4`, `MENU_RIGHT|NUM_6`. So
  cabinet-button nav is mostly wiring it as primary + adding the coarse gesture.
- **Suppression today is numpad-only:** `init()` installs a `GenericDetour` on
  `arkMDXGet10Key` (`get_10key_detour`); when `IS_INPUT_SUPPRESSED` is set it zeroes the
  game-side buffers, while the modpack's own poll bypasses via the `IN_MODPACK_POLL`
  re-entry flag. There is **no** detour on the five menu-button getters → cabinet-button
  presses currently reach the game even while the overlay is open.

### The gap to close (new work)

Add suppression for the five menu-button getters, mirroring the get_10key pattern exactly:

- Install a `GenericDetour<TriggerHoldFn>` on each of `arkMDXGetStart/Up/Down/Left/Right`
  (the function pointers are already resolved in `ArkExports`; keep the detour handles
  alongside `GET_10KEY_DETOUR`).
- In each detour: call the original, then **if** `IS_INPUT_SUPPRESSED && !IN_MODPACK_POLL`,
  zero `*trigger` and `*hold` for the game-side caller. The modpack poll sets
  `IN_MODPACK_POLL=true` around its own reads (already done in `poll_player`), so the
  modpack keeps seeing real state.
- Net: while the overlay is open (`set_input_suppressed(true)` is already called in
  `mod_menu::open`), Start/Up/Down/Left/Right no longer bleed into the game — matching what
  the numpad already gets.

> Feasibility is established by the working get_10key detour: same module, same export
> family, identical `(i32, *mut u32, *mut u32)` shape. Whether zeroing those out-params
> fully stops the game from acting on the buttons is the one runtime question — confirm on
> the cabinet with a diagnostic build (strong precedent: numpad suppression works the same
> way). Loading arkmdxbio2 into Ghidra is NOT required for this.

### Decisions baked in (from idea-honing Q5)

- Open/close stays **triple-press numpad 0** (`on_zero_pressed`, unchanged).
- Cabinet buttons = primary nav; `2/4/6/8` retained as a secondary alias (both already
  matched in `handle_exclusive_input`).
- Coarse adjust = Left/Right **with Start held** (`get_button_state & START`); fine = Left/
  Right alone. Numpad `4/6` are fine-only (no hold semantics) — acceptable.

## B. Typed rows + parent/child visibility in `mod_menu`

### What exists today

`mod_menu` is a **flat list of registry mods**. `ModMenuState` holds `mod_entries:
Vec<ModInfo>` (from the registry via `entries_callback`), a `selected_index`, a
`scroll_offset`, and a pre-allocated pool of `VISIBLE_SLOTS (=7)` `Slot`s (each = name +
desc + status `TextWidget`). `refresh_slots` maps `scroll_offset+i → mod_entries[idx]` and
sets each slot's text; navigation moves `selected_index`; Left/Right calls
`toggle_selected(bool)` which flips the mod's enabled state via `toggle_callback` and
persists via `config::save_mod_states`. All widget mutation is deferred to the render thread
via `widget_renderer::run_on_render_thread`.

There is **no** notion of a row that isn't a whole registered mod, nor of a scalar value,
nor of indentation / child rows.

### Reference shape to mirror (`custom_options::api`)

The game-side options framework already models exactly the row taxonomy we want (we adopt
the *shape*, not the code — that framework renders through the game's native option UI; the
overlay renders plain `TextWidget`s):

- `UiKind::Scalar { min, max, step_fine, step_coarse, format: ScalarFormat }` — numeric row;
  Left/Right by `step_fine`, Start-held by `step_coarse`. `ScalarFormat::Integer |
  FixedPoint{decimals}`.
- `ShowWhen::Equals { parent_id, value }` — child row visible only when the parent option
  equals `value`; the framework excludes hidden rows from the navigable list.
- `OnChangeFn` — fires on value change.

### Proposed overlay row model (new)

Generalize the overlay's list from "mods only" to a list of **typed rows**. Minimal design:

```rust
enum RowKind {
    ModToggle { id: String },                 // existing behavior: boolean, toggles a mod
    Bool      { /* value mirror */ },          // a non-mod boolean (future use)
    Scalar { value: i32, min: i32, max: i32,
             step_fine: i32, step_coarse: i32 },// numeric, Left/Right adjusts
}
struct MenuRow {
    label: String,
    hint: String,                              // shown in the desc widget
    kind: RowKind,
    indent: u8,                                // 0 = top-level, 1 = child (visual indent)
    visible_when: Option<(/*parent row key*/ String, i32)>, // ShowWhen analog
    on_change: Option<Arc<dyn Fn(i32) + Send + Sync>>,
}
```

- The menu builds its row list from (a) the registry mods (as `ModToggle` rows, unchanged)
  plus (b) **rows contributed by mods** that want overlay config UI. A mod registers its
  rows with `mod_menu` (a new `register_rows`-style API), analogous to how mods register
  `custom_options`. The timing mod registers: nothing extra for its master toggle (that's
  its existing `ModToggle` row) + four `Scalar` child rows with `indent=1` and
  `visible_when = Some(("timing-offsets", 1))`.
- **Visibility filtering** happens when building the visible/navigable list each refresh:
  skip any row whose `visible_when` parent isn't at the required value (parent = the
  timing-offsets ModToggle's enabled state). This mirrors `ShowWhen` row-exclusion. The
  existing `scroll_offset`/`selected_index`/`adjust_scroll` logic then operates over the
  filtered list, so scrolling and the `>` cursor keep working unchanged.
- **Scalar adjust:** in `handle_exclusive_input`, when the selected row is a `Scalar`,
  Left/Right changes `value` by `step_coarse` if `START` is held (via `get_button_state`)
  else `step_fine`, clamped to `[min,max]`; fire `on_change(value)`; refresh. When the
  selected row is a `ModToggle`/`Bool`, keep the existing toggle behavior.
- **Rendering a scalar row:** reuse the `Slot`'s three widgets — name = (indented) label,
  desc = hint, status column = the signed integer value (plain, per Q9). Indentation = a
  couple leading spaces or a small X-offset on the name widget for `indent>0`.

### Render-thread / safety notes (unchanged invariants)

- All row text mutation stays inside `run_on_render_thread` closures (as today).
- The mod's `on_change` callback runs on the render thread (input is polled there); it must
  be non-blocking and panic-free, and must not re-enter `mod_menu` locks. The timing mod's
  callback just stores the value, persists via `config::save_json_key`, and calls the
  game's int setter to push the live value — all cheap, no widget work.
- Keep the `Mutex<ModMenuState>` discipline: don't hold the lock across a
  `run_on_render_thread` schedule (matches the existing drop-then-schedule pattern).

## Scope note (two-part effort, confirmed in requirements)

- **Part I (infra, reusable):** menu-button suppression detours + typed-row/scalar/
  parent-child support + coarse-adjust gesture in `mod_menu`/`input_manager`.
- **Part II (the mod):** `timing-offsets` registers its four scalar child rows and wires
  `on_change` to the setter-hook apply path (R4). If Part I's row infra fails to initialize,
  the mod still applies config-seeded values (R4/Q8 independent degradation).

Both parts are pure codebase work (no further Ghidra needed); the only runtime unknown is
whether the menu-button suppression fully blocks game-side input — a cabinet diagnostic
check, with the numpad suppression as strong precedent.
