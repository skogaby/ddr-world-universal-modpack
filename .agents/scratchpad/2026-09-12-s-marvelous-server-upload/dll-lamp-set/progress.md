# Progress — Step 7: DLL S-MFC set (wire + local feed) + codec
Status: Complete (uncommitted — maintainer commits manually)
- `lamp_codec.rs` (std-only, harness-mounted): `Entry`, `decode` (lenient), `encode` (sorted), `smfc_set`, `WIRE_NAME`. 3 tests.
- `lamp.rs`: per-side `HashSet<(mcode, chart)>`, `activate` registers string field (`save: None`, load = replace) + card-in clear (idempotent), `insert_local` (from the producer on clearkind 11), `is_smfc`, `count`.
