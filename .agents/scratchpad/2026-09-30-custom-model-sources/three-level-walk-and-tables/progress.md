# Progress — three-level-walk-and-tables
- [x] `custom_scan.rs`: three-level walk, `read_pack_dir` takes `source`, per-source INFO
- [x] `lifecycle.rs`: `Tables.custom`, `custom_entries_snapshot()`, source count in the tables INFO
- [x] `mod.rs`: flat adapter (Step 3 replaces)
- [x] Gate: `cargo check` clean, `cargo fmt`, `./build.sh` clean (1m31s), background_dancers harness 207/207, custom_options harness 72/72
## Deploy & test log
- Not deployed on its own — the walk is exercised by the Step 4 cabinet pass (AC1–AC3 there).
Status: Complete (uncommitted — maintainer commits manually)
