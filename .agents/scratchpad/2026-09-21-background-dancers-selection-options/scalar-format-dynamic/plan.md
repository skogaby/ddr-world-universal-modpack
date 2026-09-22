# plan — scalar-format-dynamic

Status: Approved 2026-09-21 (auto mode; verified approval chain in context.md)

## Test scenarios (write first; must fail against the absent variant)
1. `scalar_format_tests.rs::dynamic_labeler_and_fallback`: local `fn labeler(id, v)`;
   `("background_dancer", 0)` → `b"RANDOM"`, `(…, 2)` → `b"EMI #2"`, `(…, 99)` → `b"99"` (None
   fallback), `("background_stage", 2)` → `b"BOOM #3"` (id dispatch); UTF-8 twin parity.
2. `api.rs::format_scalar_value_utf8_all_variants`: add a `Dynamic` case.
3. `registry.rs::formatted_parity_across_all_variants`: add `(ScalarFormat::Dynamic(f), 2)`; a
   registered id `"s"` — labeler returns `Some("two")` for value 2.

## Implementation
- `api.rs`: type alias + variant + id-threaded signatures + arm.
- `rows.rs` / `registry.rs`: pass `&opt.id`.
- Fix all test call sites.

## Risks
- Signature change touches every test caller: mechanical, compiler-guided.
