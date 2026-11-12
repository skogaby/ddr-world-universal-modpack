# Plan — three-level-walk-and-tables
Status: Approved 2026-09-30 (auto — verified upstream approval chain)
No host tests (impure `std::fs` walker; `custom_scan.rs` imports crate services). Validation = readiness gate now, cabinet AC1–AC3 at the Step 4 deploy.
Implementation: `walk_kind_root` per design §4.2 with `partition_models` / `names` / `file_name` helpers; `read_pack_dir(.., source, ..)`; summary INFO with `plan.source_counts()`; `Tables.custom` + `custom_entries_snapshot()`; "tables ready" counts sources; `mod.rs` flat adapter with a Step 3 marker.
