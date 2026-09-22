# progress — scalar-format-dynamic

- [x] Tests first: `scalar_format_tests.rs::dynamic_labeler_and_fallback`, `api.rs` utf8 Dynamic case,
      `registry.rs::formatted_parity_across_all_variants` Dynamic case (failed to compile against the
      absent variant — expected).
- [x] `api.rs`: `DynamicLabelFn` alias, `ScalarFormat::Dynamic`, id-threaded
      `format_scalar_value` / `format_scalar_value_utf8`, the new arm.
- [x] Call sites: `rows.rs::push_scalar_value_text` (`&opt.id`), `registry.rs::overlay_row` (`&opt.id`).
- [x] `./scripts/validate_custom_options.sh`: 57 passed, display-string lint OK.
- [x] `cargo check --target x86_64-pc-windows-msvc`: clean.

## Deviations
- None.

Status: Complete (uncommitted — maintainer commits manually)
