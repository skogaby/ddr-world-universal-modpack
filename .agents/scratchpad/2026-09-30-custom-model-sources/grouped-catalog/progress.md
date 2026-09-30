# Progress — grouped-catalog
- [x] `catalog.rs` rewritten: `SourceCatalog`, grouped `Catalog` + accessors, `STOCK_LABEL`, `flat_entries`, builders; tests migrated + 3 new
- [x] `options.rs` Step-3 shim (`Flat`), `mod.rs` adapter removed
- [x] Harness 209/209; `cargo check` clean
## Cycles
1. Rewrite → 2 test failures (my fixture built the stock reference AFTER extending the tables) → fixed the tests → green.
## Deviations
- None from the task; `STOCK_LABEL` const added.
Status: Complete (uncommitted — maintainer commits manually)
