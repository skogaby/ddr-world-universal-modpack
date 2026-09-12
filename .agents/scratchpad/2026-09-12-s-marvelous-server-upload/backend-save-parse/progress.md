# Progress — Step 5: bemani-buddy save handler parses s_marv
Status: Complete (uncommitted — maintainer commits manually)
- `parse_smarv_node(node, mcode, chart)` in `playdata.rs` (lenient; identity cross-check → warn+None; all-or-nothing required children; empty ghost → None).
- Wired in `handle_save_scores` (`data.child("s_marv")`), copied to the attempt; `handle_save_dan_results` sets `smarv: None`.
- 5 unit tests (present/absent/mismatch ×2/double-style chart/missing child). 64 → tests green; clippy baseline 33 unchanged.
