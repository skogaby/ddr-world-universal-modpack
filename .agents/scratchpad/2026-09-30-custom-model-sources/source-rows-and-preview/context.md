# Context — source-rows-and-preview (Step 4, task 02)
Approval chain as request-logic. Mode: auto.
Requirements: TR1–TR8 (options.rs rewrite: OnceLock tables + leaked ids, registration with aliases/ShowWhen, Duplicate path, mirror observer, readers; preview wiring; lint; no panics).
Files: `src/mods/background_dancers/options.rs` (rewritten), `preview/mod.rs`.
Build/test: `cargo check`, both harnesses (display-string lint), `./build.sh`. Engine wiring is cabinet-validated.
