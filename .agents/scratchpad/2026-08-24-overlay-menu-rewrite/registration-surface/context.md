# Context — registration-surface (Step 5 task-02)

## Provenance (verified)
- Task: `.agents/tasks/2026-08-24-overlay-menu-rewrite/step05/task-02-registration-surface.code-task.md`
  (Generated-By present; breakdown user-approved in session incl. the
  format_scalar_value→api.rs move and the SJIS→"±" mapping).
- Plan/design Approved 2026-08-24. Depends on task-01 (Complete). Mode: auto.

## Verified facts
- api.rs (592 lines, ZERO crate imports — mountable): RegisterSpec 4
  constructor literal sites (bool_toggle :354, enum_values :388, scalar :408,
  header :444); builders :459-:531; EnumValue ctors new/with_preview
  (:79/:89, no literals elsewhere); default_on_change_noop/:534,
  is_default_on_change :541.
- registry.rs: RegisteredOption :27-52 (copy-through site try_register
  :196-208); header validation :164-184 (persist/callback/show_when/
  transforms/value — menus+display strings to be explicitly allowed).
- format_scalar_value: rows.rs:2001-2040 pub(super), pure, zero
  crate/super deps in body; callers rows.rs:1974 + scalar_format_tests.rs:11.
- builder_hook.rs snapshot filter :180 `.filter(|(_, opt)| opt.available)`.
- asset_gen label site: mod.rs:241 `asset_gen::register_label_for(id)` (spec
  consumed at :222 — capture menus before the move).
- ordering::placement_override_for(id) -> (in_game, overlay) from task-01.
- Harness: scripts/validate_custom_options.sh MODULES=(ordering.rs).

## Interpretations (auto mode record)
- prettify_id: '_'-split, Title Case each word ("song_speed" → "Song Speed").
- prettify_texture_suffix: strip known prefixes (seop_image_, seop_op_,
  seop_item_) then prettify remainder.
- asset_gen skip: LABEL texture only (task/design text); preview images left
  generated (fail-open bias).
- Header + menus/display strings: allowed (approved in the task background).
- bool_toggle display labels: "OFF"/"ON".

## Gates
harness (ordering+api) → validate_mod_menu (untouched) → check → fmt → build
→ boot regression (in-game menu unchanged; optional fault-config probe).
