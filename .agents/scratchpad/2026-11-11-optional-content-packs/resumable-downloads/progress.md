# Progress: resumable-downloads

## Checklist
- [x] Listener helper and scenarios written, failing for lack of `fetch`
- [x] `Target`, `fetch`, `NetError::Verify`, `main.rs` adapted; scenarios green
- [x] Live test written and run
- [x] Full suite, fmt, Win7 build
- [x] Review pass

## Record
- Baseline `cargo test` in `updater/`: 111 unit + 17 + 9 end-to-end, all green.
- Red: the new tests failed to compile on the missing `Target`, `fetch`,
  `parse_content_range`, `NetError::Verify` and `Downloaded::resumed_from` — the
  expected reason.
- Green: 122 unit (11 new; `verify_rules` and the no-partial-file test adapted to the
  descriptor) + 17 + 9, 1 ignored. The download tests were run five times in a row
  without a failure.
- Live: `cargo test -- --ignored live_range` passed. The first mebibyte of the published
  v1.4 asset was fetched with a ranged request (206 after GitHub's redirect), stored as
  the part file, and the resumed download verified against GitHub's published digest
  with `resumed_from` = 1 048 576. No manual redirect handling is needed.
- `cargo fmt` (whole crate) clean; `ONLY=updater ./build_win7.sh` built the executable;
  it contains no `ProcessPrng` import.

## Review notes
- `Downloaded::resumed_from` is not read outside tests yet (the base download never
  resumes), so it carries `#[allow(dead_code)]` until the pack step reads it.
- A verification failure used to be reported as a transport error whose text began
  "could not reach GitHub"; it is now `NetError::Verify` and prints the mismatch itself.
- A server that sends a complete body shorter than the expected size, with an honest
  `Content-Length`, is caught up front as a verification failure; only a body that ends
  before its own advertised length counts as an interruption.

## Deviations
- None from the design. One addition within the task's scope: a part file left under a
  name is discarded when the next call for that name runs without `resume`.

Status: Complete (uncommitted — maintainer commits manually)
