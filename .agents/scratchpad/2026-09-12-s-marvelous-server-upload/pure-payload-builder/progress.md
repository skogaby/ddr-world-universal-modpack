# Progress — Step 2: DLL pure payload builder + raw stream reader

Status: Complete (uncommitted — maintainer commits manually)

- `records.rs`: `RawStreams { grades, ms, judged }` + `read_raw_streams` (unfiltered, stream-aligned
  judged mask via `judged_mask`, min-length tolerant like `filter_judged`); `read_streams` refactored
  to `read_raw_streams().judged_only()` + the unchanged counter gate (bit-identical behaviour; test).
- `upload.rs` (std-only): `RecordInputs`, `SMarvPayload`, `Leaf`, `build_payload`, `overlay_ghost`,
  `stock_ghost`, `to_leaves`, `chart_index`, consts `CLEAR_KIND_SMFC=11`, `GHOST_SMARV_CHAR=b'8'`,
  `NODE_NAME="s_marv"`. 9 tests incl. every §5.1 invariant.
- `scripts/validate_s_marvelous.sh`: records+upload mounted nested under `pub mod s_marvelous` so
  `super::records` resolves. Harness: 155 passed. `cargo check --target x86_64-pc-windows-msvc` clean.
