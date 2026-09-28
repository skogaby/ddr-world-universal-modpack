# Progress — enhanced-sites
- [x] Four signatures + `derive_series_enhanced` + `series_enhanced_sites()`
- [x] `cargo check` clean
- [x] Sweep ALL GREEN; every enhanced name resolves on 20250805/20260224/20260721/20260825/20260915
      with the research addresses (e.g. 20260915 builder +0x124220, press +0x127810, set-one
      +0x1D5680, notify +0x137230, predicate range +0x123E72)
- [x] shape_diff identical through every window (builder, press, toggle, predicate, tab factory)
- [x] Enhanced init branch logs the sites
## Deviations
- String assign cross-checked against the existing `string_assign` derivation (same function on
  every build) rather than published under that name.
- Cabinet log check folded into the Step 4 cabinet test (autonomous-run instruction).
Status: Complete (uncommitted — maintainer commits manually)
