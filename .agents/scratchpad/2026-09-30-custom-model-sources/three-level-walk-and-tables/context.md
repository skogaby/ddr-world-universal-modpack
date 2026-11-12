# Context — three-level-walk-and-tables (Step 2, task 03)
Approval chain as sources-module. Mode: auto.
Requirements: TR1–TR5 (walk, INFO breakdown, Tables.custom + snapshot, mod.rs adapter, readiness gate).
Files: `src/mods/background_dancers/custom_scan.rs`, `lifecycle.rs`, `mod.rs`.
Build/test: `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`, `./build.sh`, both harnesses. Engine-facing — the walk itself is cabinet-validated (Step 4 deploy).
