# Progress — source-rows-and-preview
- [x] `options.rs` rewritten (source + per-source rows, aliases, ShowWhen, Duplicate, observer mirror, `Request` readers, `row_choice_key`); `preview/mod.rs` wired
- [x] `cargo check` clean, `cargo fmt`, both harnesses green (lint OK), `./build.sh` clean
## Deploy & test log
- Pending: the Step 4 cabinet pass (design §7.3) — maintainer.
## Deviations
- `register_one` takes the overlay strings so the lint sees them beside `register_option(`; no interface change.
Status: Complete (uncommitted — maintainer commits manually; cabinet validation pending)
