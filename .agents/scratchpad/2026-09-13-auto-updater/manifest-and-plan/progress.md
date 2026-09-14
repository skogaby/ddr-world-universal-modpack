# Progress — Step 2 Task 02: manifest + plan
Status: Complete (uncommitted — maintainer commits manually)
- [x] `manifest.rs` (+5 tests): Manifest/new/read/write(temp+rename)/needs_update (equality, case-insensitive digest)/sha256_hex/sha256_file/hash_tree/hex
- [x] `plan.rs` (+6 tests): Action (serde for the journal), DiskState, MergedFiles, Plan, `build` per §4.10; probe consulted only for stage + previous paths
- [x] fmt/test: 58 unit + 9 e2e green
Deviations: none (one test expectation corrected — previous-manifest paths iterate in BTreeMap order).
