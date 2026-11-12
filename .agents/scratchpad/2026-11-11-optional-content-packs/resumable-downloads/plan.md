# Plan: resumable-downloads

Status: Approved 2026-11-11 (auto mode; stands on the approved plan and design, see
`context.md`).

## Shape
- `download::Target { name, url, size, sha256: Option<String> }` with `from_asset`.
- `download::fetch(agent, &Target, dest_dir, resume, progress) -> Result<Downloaded, NetError>`
  replaces `fetch_asset`. `Downloaded` gains `resumed_from: u64`.
- `verify` takes the target. `NetError::Verify(String)` is new.
- Internally: decide the starting offset from the part file, request, classify the
  response (append / restart / retry without range), stream, classify the ending.

## Test scenarios (in-process listener; payload = 200 000 patterned bytes)
| # | Setup | Expect |
|---|---|---|
| 1 | no part; resume off and on | final = payload, `resumed_from` 0, no part |
| 2 | part = first 70 000; server honours Range | server saw `bytes=70000-`; final = payload; `resumed_from` 70 000 |
| 3 | part; URL redirects to the file | file request carried Range; `resumed_from` 70 000 |
| 4a | part; server ignores Range (200) | final = payload; `resumed_from` 0 |
| 4b | part; server answers 416 to ranged requests | restart; final = payload |
| 4c | part; server answers 206 from offset 0 | restart; final = payload |
| 5a | server cuts the body at 50 000; resume on | `Transport`; part kept, ≤ 50 000 bytes; second run on a healthy server completes with `resumed_from` > 0 |
| 5b | same; resume off | `Transport`; folder empty |
| 6a | wrong digest; both modes | `Verify`; folder empty |
| 6b | target size ≠ advertised length; both modes | `Verify`; folder empty |
| 7a | part = whole payload | Ok; server saw no request |
| 7b | part = right length, wrong bytes | restart; final = payload |
| 8 | existing tests | unchanged and green |
| 9 | `#[ignore]` live: first MiB of the v1.4 asset as part | verifies against the published digest; `resumed_from` = 1 048 576 |

The listener is a test helper, not a stub of the code under test: every scenario fails
until `fetch` exists.

## Risks
- Listener flakiness: one thread, sequential connections, `Connection: close`, bound to
  port 0.
- The live test depends on the v1.4 release staying published.
