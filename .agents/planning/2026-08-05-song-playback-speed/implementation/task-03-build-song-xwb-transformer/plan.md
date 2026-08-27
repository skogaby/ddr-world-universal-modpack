# Task 3 Plan: Song XWB Transformer

Status: Approved 2026-08-05 (inherits the approved generated task and source design)

## Test-Driven Sequence

1. Add pure MD5 known-vector/incremental tests before the digest module.
2. Add transformer tests before implementation.
   - Main/preview and preview/main inputs at 75% and 125% preserve order/identity and report exact main rate.
   - Reparse proves formats, rates, flags, alignment, whole blocks, mapped loops, and output framing.
   - Repeated transforms are byte-identical and digest-identical.
   - Stock one-/two-byte-short tails decode and transform successfully.
   - Code mismatch, malformed profile, low memory limit, and invalid output writer return typed failures.
   - Cancellation is injected at source hash, decode, stretch, encode, header/entry write, output digest, and validation; every case returns no report.
3. Add cancellable per-block/window codec and stretcher variants while preserving existing APIs as always-continue wrappers.
4. Add streaming XWB metadata/payload writer support with exact payload-length enforcement.
5. Implement checked loop mapping, memory estimation, sequential entry transformation, digesting, output reparse, and report assembly.
6. Run the complete host/regression/build gates and update canonical progress with Task 4 as next action.

## Implementation Shape

- `digest.rs` is a dependency-free incremental MD5 implementation suitable for source/output chunking.
- `transform.rs` owns request/result/error/checkpoint models and orchestration.
- The serializer writes canonical header/metadata first, then invokes one payload callback per physical entry; each callback holds only that entry's decoded and stretched PCM and streams encoded blocks directly to output.
- After both entry workspaces are dropped, output is read once for digest and strict postcondition reparse.

## Risks

- Writer cancellation surfaces through `std::io::ErrorKind::Interrupted`; it must map back to the transform's typed cancellation rather than an opaque I/O failure.
- Output metadata must be known before payload generation, so rate/loop/memory preflight must be complete and overflow-safe.
- A seekable in-memory test writer duplicates output bytes by construction, but the production temp-file path does not; memory admission models the production path.
