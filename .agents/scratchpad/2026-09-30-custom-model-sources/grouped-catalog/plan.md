# Plan — grouped-catalog
Status: Approved 2026-09-30 (auto — verified upstream approval chain)
Tests: existing catalog tests migrated to `(kind, source)` accessors; new `custom_entries_group_after_the_untouched_stock_block`, `flat_view_is_the_pre_sources_list`, `sources_sort_by_label_and_kinds_stay_separate`; `value_accessors` extended with unknown-source and source-row cases; oversized labels cover custom + source labels.
Implementation: rewrite `catalog.rs` (module + tests); `options.rs` private `Flat` view over `flat_entries` (Step 4 replaces); `mod.rs` passes `custom_entries_snapshot()` directly.
