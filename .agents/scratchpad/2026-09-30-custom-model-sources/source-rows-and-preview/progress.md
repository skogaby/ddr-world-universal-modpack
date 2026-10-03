# Progress — source-rows-and-preview
- [x] `options.rs` rewritten (source + per-source rows, aliases, ShowWhen, Duplicate, observer mirror, `Request` readers, `row_choice_key`); `preview/mod.rs` wired
- [x] `cargo check` clean, `cargo fmt`, both harnesses green (lint OK), `./build.sh` clean
## Deploy & test log
- 2026-09-30: cabinet pass OK (maintainer) — rows, visibility, labels, preview, mirror, persistence all as specified.
## Deviations
- `register_one` takes the overlay strings so the lint sees them beside `register_option(`; no interface change.
Status: Complete (uncommitted — maintainer commits manually)
