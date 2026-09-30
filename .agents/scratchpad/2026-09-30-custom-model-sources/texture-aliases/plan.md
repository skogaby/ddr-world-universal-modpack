# Plan — texture-aliases

Status: Approved 2026-09-30 (auto mode — the verified upstream approval chain stands in; see context.md)

## Test scenarios (`src/services/custom_options/texture_alias_tests.rs`, host harness)
1. `both_aliases_resolve`: `RegisterSpec::scalar("background_dancer_ddr_strike", 0, 3, 1, Integer)
   .label_texture_like("background_dancer").preview_texture_like("background_dancer")` ⇒
   `label_texture_name() == "seop_item_background_dancer"`,
   `preview_image_base_name() == "seop_image_background_dancer"`,
   `preview_image_names() == ["seop_image_background_dancer"]`,
   `preview_image_name_for_value(2) == "seop_image_background_dancer"`.
2. `no_alias_is_id_derived`: plain scalar `"plain_row"` ⇒ `seop_item_plain_row` / `seop_image_plain_row`.
3. `preview_alias_alone_leaves_label`: only `preview_texture_like("background_stage")` ⇒ label
   `seop_item_x`, preview `seop_image_background_stage`.
4. `label_alias_alone_leaves_preview`: symmetric.
5. `enum_with_preview_keys_uses_alias_base`: `bool_toggle("t").preview_texture_like("base")` ⇒
   names `["seop_image_base_off", "seop_image_base_on"]` (keys build on the alias base; no bare base
   because every value carries a key).

## Implementation
- `api.rs`: two `Option<&'static str>` fields (init `None` in `bool_toggle`, `enum_values`, `scalar`,
  `header`), setters, doc.
- `registry.rs`: fields on `RegisteredOption`, copied in `try_register`; `label_texture_name()` =
  `seop_item_{alias.unwrap_or(&id)}`; `preview_image_base_name()` likewise; introduce a private
  `preview_stem()` helper so `preview_image_name_for_value` builds `seop_image_{stem}_{key}` on the
  alias too.
- `mod.rs::register_option`: compute the label stem under the lock and pass it to
  `asset_gen::register_label_for`.
- Harness: add `texture_alias_tests.rs` to `TEST_MODULES` and `#[cfg(test)] mod texture_alias_tests;`.

## Risks
- `preview_image_name_for_value` today formats `seop_image_{}_{}` with `self.id` directly (not via the
  base fn) — must switch to the stem or a per-value alias would silently keep the id.
