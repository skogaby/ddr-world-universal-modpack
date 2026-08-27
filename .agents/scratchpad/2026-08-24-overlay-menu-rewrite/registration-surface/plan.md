# Plan — registration-surface (Step 5 task-02)

Status: Approved 2026-08-24 (auto mode — verified chain + in-session
breakdown approval)

## Approach (in order)
1. api.rs: MenuPlacement + RegisterSpec fields (4 ctor sites) + 5 builders;
   EnumValue.display_label + with_display + bool_toggle OFF/ON;
   prettify_id/prettify_texture_suffix; format_scalar_value MOVED here
   (pub(crate)) + format_scalar_value_utf8 (SJIS ±→"±"); in-file tests
   (red first via todo!() helper bodies under the harness).
2. rows.rs: delete the fn, `use super::api::format_scalar_value;`;
   scalar_format_tests.rs import → super::api.
3. registry.rs: RegisteredOption {menus, display_name, description} +
   try_register copy-through + header-validation comment (allowed).
4. builder_hook.rs: availability filter also drops resolved-`!in_game` rows
   (`ordering::placement_override_for(&opt.id).0.unwrap_or(opt.menus.in_game)`).
5. mod.rs: capture menus before spec move; skip register_label_for when
   resolved in_game == false (one INFO).
6. Harness MODULES += api.rs.

## Test scenarios
- prettify: "song_speed"→"Song Speed", single word, digits, empty, repeated
  underscores.
- prettify_texture_suffix: "seop_op_dark"→"Dark", "seop_image_x_on"
  (only prefix stripped), unprefixed passthrough.
- MenuPlacement default both-true; builders in_game_only/overlay_only/menus.
- bool_toggle: two values carry display labels OFF/ON; with_display carries.
- format_scalar_value parity spot-checks (moved fn) + utf8: all 7 variants;
  SignedUnit zero == "±0ms"; nonzero "+5ms"/"-41ms".

## Risks
- rows.rs import churn (2582 lines) — single-line removal + one use line.
- builder_hook filter must resolve placement per id WITHOUT holding a second
  lock (ordering's OnceCell is lock-free reads — safe inside the STATE lock).
