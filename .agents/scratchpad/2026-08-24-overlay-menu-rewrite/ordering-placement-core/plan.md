# Plan — ordering-placement-core (Step 5 task-01)

Status: Approved 2026-08-24 (auto mode — verified chain: task Generated-By +
plan/design Approved + in-session breakdown approval)

## Implementation approach

1. **Harness first** (`scripts/validate_custom_options.sh`, validate_mod_menu
   template): mounts `src/services/custom_options/ordering.rs`; generated
   lib.rs defines `#[macro_export] macro_rules! log_warn/log_info/log_debug`
   no-op stubs before the `#[path]` mount; `[dependencies] once_cell = "1"`.
   Checkpoint: current ordering.rs's 8 tests pass under it (mount proven).
2. **ordering.rs rework**: `OptionMenuSetting { id, overlay, in_game }`;
   `set_configured_settings` (lowercase ids at store, OnceCell one-shot);
   `compute_order` takes `Option<&[OptionMenuSetting]>` (semantics preserved
   verbatim — iterate `.id`); new pure `placement_override(configured, id) ->
   (Option<bool>, Option<bool>)` (first match wins) + runtime shell
   `placement_override_for(id)`; `display_order_for` unchanged signature;
   warn text renamed to option_menu_settings; module docs rewritten.
3. **config.rs**: `OptionMenuSettingConfig { id, overlay, in_game }` serde
   struct; CustomOptionsConfig field swap + doc.
4. **mod.rs read**: map config → ordering type, `set_configured_settings`.
5. **String/doc sweep**: api.rs:50, builder_hook.rs:192/199,
   decorative_option_headers.rs (docs + description + log).
6. **mod-config.json**: row_order array → option_menu_settings
   `[{"id": ...}, ...]` (order + p1/p2 blocks preserved) — python json
   round-trip with read-back verification.

## Test scenarios (in-file, run via the new harness)

- The 8 existing order-semantics tests ported to the new configured type.
- NEW: placement_override matrix — listed id explicit flags both/one;
  listed id no flags ⇒ (None,None); unlisted ⇒ (None,None); "neither"
  (false,false) verbatim; case-insensitive; duplicate entries first-wins;
  unconfigured ⇒ (None,None).
- Order + placement composition: an entry present only for placement still
  takes its listed order position.

## Risks
- mod-config.json hand-migration typo ⇒ operator ordering lost — mitigated by
  scripted conversion + diff read-back + boot regression.
