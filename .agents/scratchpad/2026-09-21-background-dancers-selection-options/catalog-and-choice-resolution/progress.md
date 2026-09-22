# progress — catalog-and-choice-resolution

- [x] `selection.rs`: test fixtures moved into `#[cfg(test)] pub(crate) mod fixtures` (`rows`,
      `real_map_rows`, new `real_chara_rows` — the 26 stock dancer rows) so `catalog.rs` can reuse them.
- [x] `selection.rs::resolve_choice` + `resolve_choice_rules` test (chosen key ⇒ only that key's rows, both
      `boom00` rows drawn; per-slot dancer; unknown ⇒ None; degenerate inputs; all-RANDOM ≡ `pick_stage` +
      `pick_dancers` under the same seed).
- [x] `catalog.rs` (pure): `RANDOM`, `RANDOM_LABEL`, `MAX_LABEL_BYTES`, `Kind`, `CatalogEntry`, `Catalog`
      (`count/entry/label/key`), `split_key`, `label_for`, `build_catalog`, `clamp_to_catalog`; tests over
      the full stock lists (26 dancers / 25 stages, every expected label, ≤ 15 bytes untruncated,
      longest = `REPLICANT #6`), defensive truncation, accessors, clamp edges.
- [x] `mod.rs`: `pub mod catalog;`; harness mount in `scripts/validate_background_dancers.sh`.
- [x] `./scripts/validate_background_dancers.sh`: 117 passed.
- [x] `cargo check --target x86_64-pc-windows-msvc`: clean.

## Deviations
- Pure functions live in `catalog.rs` rather than `options.rs` (the plan's file), because the host harness
  mounts dependency-free files only and `options.rs` will hold the `crate::`-dependent registration. Same
  API surface as the design §4.2.
- `Catalog::label(kind, 0)` returns `Some("RANDOM")` (the catalog owns the RANDOM text) — the design put
  that in the options layer's `label()`; equivalent behaviour, one fewer place to keep the string.

Status: Complete (uncommitted — maintainer commits manually)
