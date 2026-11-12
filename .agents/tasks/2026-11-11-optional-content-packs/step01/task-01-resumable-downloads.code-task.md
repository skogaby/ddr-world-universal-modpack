# Task: Resumable Downloads

## Description
Generalise the updater's download function so it can fetch any named file (not only a
GitHub release asset) and can resume an interrupted transfer, and prove with a live test
that a ranged request survives GitHub's download redirect. The base release download
keeps its current behaviour.

## Background
Optional content packs are up to roughly 190 MB each and are downloaded on arcade-venue
connections. The design has packs resume from a kept partial file. `updater/src/download.rs`
today takes a GitHub `Asset`, streams it to its final name, and deletes the file on any
failure. GitHub's release download URLs answer with a redirect to a signed URL that
honours `Range`; whether `ureq` forwards the `Range` header across that redirect was
never observed and the design names this step as the place to prove it.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-11-11-optional-content-packs/design/detailed-design.md (§4.7)

**Additional References (if relevant to this task):**
- .agents/planning/2026-11-11-optional-content-packs/research/hosting-and-downloads.md (§3, ranged downloads)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. The download function takes a descriptor — file name, URL, expected size (0 =
   unknown), optional SHA-256 — plus a `resume` flag. `run_default` in
   `updater/src/main.rs` adapts the GitHub asset to the descriptor and passes
   `resume = false`.
2. Transfers are written to `<name>.part` in the destination folder and renamed to
   `<name>` only after verification.
3. With `resume`, an existing part file is hashed and the request carries
   `Range: bytes=<length>-`:
   - `206` whose `Content-Range` starts at that length: append.
   - `200`: the server ignored the range; start over from zero.
   - `416`, or a `206` starting elsewhere: discard the part and start over, once.
   - A part file already as long as the expected size is verified without a request and
     discarded if it does not verify.
4. A transport error or a body that ends early keeps the part file when `resume` is set
   and deletes it otherwise.
5. A verification failure — the server advertising a different total size, more bytes
   than expected, or a digest mismatch — deletes the part file in both modes and is
   reported as a distinct error from a transport failure (inferred: the pack step later
   treats the two differently).
6. The result reports how many bytes were reused from a part file.
7. Without `resume`, observable behaviour is unchanged: the final file exists only after
   a verified transfer, and nothing is left behind on failure.
8. If the live test shows the `Range` header is dropped on the redirect, resolve the
   redirect by hand and record the finding in the research document.

## Dependencies
- `ureq` 2.12 (already a dependency); no new crates.
- Network access for the one `#[ignore]` live test only.

## Implementation Approach
1. Add an in-process HTTP listener helper to the module's tests (serve a payload; honour
   or ignore `Range`; answer `416`; redirect; advertise a wrong length; cut the body
   short; record the request headers it saw).
2. Write the tests against it, then generalise the function and adapt `main.rs`.
3. Add the live test against the published v1.4 asset: fetch its first megabyte with a
   ranged request, store it as the part file, resume, and require both that the result
   verifies against GitHub's published digest and that the megabyte was reused.

## Acceptance Criteria

1. **Fresh download**
   - Given no part file
   - When a file is downloaded with or without `resume`
   - Then the final file holds the payload, its hash and size are reported, zero bytes
     are reported as reused, and no part file remains

2. **Resume appends**
   - Given a part file holding the first K bytes and a server that honours `Range`
   - When the download runs with `resume`
   - Then the server receives `Range: bytes=K-`, the final file equals the payload, and
     K bytes are reported as reused

3. **Range across a redirect**
   - Given a URL that redirects to the file and a part file of K bytes
   - When the download runs with `resume`
   - Then the request that reaches the file carries the `Range` header and K bytes are
     reused

4. **Server ignores or refuses the range**
   - Given a part file and a server that answers `200` with the whole file, or `416`,
     or a `206` from a different offset
   - When the download runs with `resume`
   - Then the transfer restarts from zero and the final file equals the payload

5. **Interrupted transfer**
   - Given a server that closes the connection part-way through the body
   - When the download runs with `resume`
   - Then a transport error is returned and the part file is kept; a second run against
     a healthy server completes from it
   - And when it runs without `resume`, nothing is left in the folder

6. **Verification failure**
   - Given a wrong expected digest, or a server advertising a different total size
   - When the download runs in either mode
   - Then a verification error (not a transport error) is returned and no part or final
     file remains

7. **Completed part file**
   - Given a part file that is already the whole, correct payload
   - When the download runs with `resume`
   - Then it succeeds without contacting the server

8. **Base download unchanged**
   - Given the existing updater test suite
   - When it runs
   - Then every existing test passes, and a failed base download still leaves no file

9. **Live proof**
   - Given network access
   - When the ignored live test runs against the published v1.4 asset
   - Then the resumed download verifies against GitHub's published digest with the first
     megabyte reused

## Metadata
- **Complexity**: Medium
- **Labels**: updater, download, networking
- **Required Skills**: Rust, HTTP range requests, test-driven development
- **Generated By**: code-task-generator 2026-11-11
- **Source Plan**: .agents/planning/2026-11-11-optional-content-packs/implementation/plan.md
- **Plan Step**: Step 1: Resumable downloads, proven against GitHub
