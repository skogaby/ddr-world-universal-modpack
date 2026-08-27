# Task: Registration surface — MenuPlacement, display strings, in-game placement filter

## Description
Extend the custom_options registration API with menu placement and display
strings (all additive; every default preserves today's behavior): `MenuPlacement`
+ `RegisterSpec.menus` with builder setters, `display_name`/`description`,
`EnumValue.display_label` + `with_display`, `bool_toggle` auto OFF/ON labels,
prettified-id fallback helpers, the moved-to-`api.rs` `format_scalar_value`
(+ a UTF-8 wrapper), the `builder_hook` `!in_game` filter, and the `asset_gen`
label-texture skip for overlay-only rows.

## Background
Step 5 of the overlay-menu rewrite (design §4.3.1–.2). Registrant sweep with
explicit strings is Step 9 — this task lands the fields and the FALLBACKS.

Current shapes (verified 2026-08-24):
- `RegisterSpec` (api.rs:300-340, derives Debug+Clone): id / ui_kind /
  default_value / on_change / show_when / persist / save_transform /
  load_transform. FIVE struct-literal construction sites in api.rs
  (`bool_toggle` :354, `enum_values` :388, `scalar` :408, `header` :444 — plus
  any others found) must gain the new fields.
- Builders (consuming `mut self`): step_coarse :459 … save_transform :528.
- `EnumValue` (api.rs:61-73): value / label_texture_name / preview_key;
  constructors `new` :79, `with_preview` :89 (no struct literals elsewhere).
- `bool_toggle` (api.rs:354-376) builds OFF/ON EnumValues via `with_preview`.
- Header validation: registry.rs:164-184 rejects "state-bearing" fields on
  headers — DECISION (approved): `display_name`/`description`/`menus` are
  ALLOWED on headers (headers are section separators in the overlay; config
  can hide a header everywhere via placement).
- `RegisteredOption` (registry.rs:27-52): copy `menus`, `display_name`,
  `description` from the spec in `try_register` (registry.rs:148-210) so the
  snapshot (task-03) and builder_hook read them from state.
- `format_scalar_value`: rows.rs:2001-2040, `pub(super)`, pure
  `(ScalarFormat, i32) -> Vec<u8>` (SJIS `±` for SignedUnit zero, doc
  :1991-2000); byte-pin tests in scalar_format_tests.rs (imports
  `super::rows::format_scalar_value` at :11).
- builder_hook availability filter: builder_hook.rs:180
  (`.filter(|(_, opt)| opt.available)`) inside the per-open snapshot
  (:170-190); ordering permutation applied after (:202-210).
- asset_gen label registration: `register_label_for` called from
  register_option at mod.rs:232.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-24-overlay-menu-rewrite/design/detailed-design.md (§4.3.1 placement, §4.3.2 display strings)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. **api.rs — `MenuPlacement`**: `pub struct MenuPlacement { pub in_game: bool,
   pub overlay: bool }` (Debug/Clone/Copy/PartialEq/Eq), `Default` =
   `{ true, true }`. `RegisterSpec.menus: MenuPlacement` + builders
   `.menus(MenuPlacement)`, `.in_game_only()`, `.overlay_only()`.
2. **api.rs — display strings**: `RegisterSpec.display_name:
   Option<&'static str>`, `RegisterSpec.description: Option<&'static str>` +
   `.display_name(..)`/`.description(..)` builders.
   `EnumValue.display_label: Option<String>` + `EnumValue::with_display(value,
   label_texture_name, display_label)` (and keep `new`/`with_preview` setting
   `None`). `bool_toggle` auto-sets `"OFF"`/`"ON"` display labels on its two
   EnumValues.
3. **api.rs — fallback helpers** (pure, pub(crate)):
   `prettify_id(&str) -> String` — split on `_`, Title Case each word
   ("premium_free" → "Premium Free"); `prettify_texture_suffix(&str) ->
   String` — strip a `seop_image_`/`seop_op_` style prefix then prettify (the
   enum-label fallback per design "prettified texture-name suffix").
   Fallback APPLICATION lives in the snapshot builder (task-03) — this task
   only provides the helpers + tests.
4. **api.rs — `format_scalar_value` move**: relocate from rows.rs (body
   unchanged, now `pub(crate)`) beside `ScalarFormat`; rows.rs imports it
   (existing callers + scalar_format_tests.rs import path updated). ADD
   `format_scalar_value_utf8(format, value) -> String`: bytes → String with
   the SJIS `±` pair (0x81,0x7D) mapped to UTF-8 `"±"` (approved decision;
   Step 6 visual round validates the glyph).
5. **registry.rs**: `RegisteredOption` gains `menus`/`display_name`/
   `description` copied in `try_register`; header validation explicitly
   permits all three on headers (comment the decision).
6. **builder_hook.rs**: the per-open snapshot filter (:180) also drops rows
   whose RESOLVED in-game placement is false — resolution = config override
   from `ordering::placement_override_for(&opt.id)` (task-01) wins over
   `opt.menus.in_game`. Headers filter identically.
7. **asset_gen skip** (optimization, design §4.3.1): at the mod.rs:232
   `register_label_for` site, skip label-texture registration when the
   resolved in-game placement is false (overlay-only rows never render
   in-game labels). Fail-open: when in doubt, generate.
8. **Harness**: add `api.rs` to `scripts/validate_custom_options.sh` MODULES.
   In-file api.rs tests: MenuPlacement default; builder setters incl.
   in_game_only/overlay_only; bool_toggle carries OFF/ON display labels;
   with_display; prettify cases (single word, multi-word, digits, empty);
   moved `format_scalar_value` parity spot-checks (byte pins stay in
   scalar_format_tests.rs) + `format_scalar_value_utf8` cases per variant
   incl. the SignedUnit zero `±` mapping.
9. Defaults preserve behavior: no registrant changes; in-game menu renders
   identically with an empty/absent `option_menu_settings`.

## Dependencies
- task-01 (`ordering::placement_override_for`) — must land first.

## Implementation Approach
1. api.rs fields/builders/helpers + tests (red→green under the harness).
2. format_scalar_value move + import updates (in-crate tests stay put).
3. registry copy-through + header-validation decision.
4. builder_hook filter + asset_gen skip.
5. Gates, then an autonomous boot: in-game options menu logs unchanged; a
   test config with `"in_game": false` on one row drops it from the injection
   count (log-observable).

## Acceptance Criteria

1. **Additive defaults**
   - Given an unmodified registrant using `bool_toggle`/`scalar`/`enum_values`
   - When the crate builds and the game boots
   - Then registration behavior and the in-game menu are unchanged (menus
     default both-true, display strings None).

2. **Placement enforcement (in-game)**
   - Given a row registered `.overlay_only()` OR configured
     `"in_game": false`
   - When the options menu opens
   - Then the row is not injected (like unavailable) and its label texture was
     skipped; config override beats registration default.

3. **Display-string surface**
   - Given `bool_toggle` and `EnumValue::with_display`
   - When inspected
   - Then OFF/ON labels are auto-populated and custom labels carried;
     `prettify_id("song_speed") == "Song Speed"`.

4. **Formatted parity**
   - Given every `ScalarFormat` variant
   - When `format_scalar_value_utf8` runs
   - Then output matches the byte formatter modulo the SignedUnit-zero `±`
     mapping, verified by host tests via the harness.

## Metadata
- **Complexity**: Medium-High
- **Labels**: custom-options, api, placement, display-strings
- **Required Skills**: Rust, repo custom_options conventions
- **Generated By**: code-task-generator 2026-08-24
- **Source Plan**: .agents/planning/2026-08-24-overlay-menu-rewrite/implementation/plan.md
- **Plan Step**: Step 5: custom_options extensions — placement, display strings, observer, snapshot, `option_menu_settings`
