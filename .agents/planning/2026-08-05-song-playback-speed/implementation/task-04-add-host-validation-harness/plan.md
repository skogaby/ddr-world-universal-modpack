# Task 4 Plan: Host Validation Harness

Status: Approved 2026-08-05 (inherits the approved generated task and source design)

## Test-Driven Sequence

1. Define CLI/report expectations before implementation.
   - Unknown option fails.
   - Missing sibling source fails.
   - `--require-corpus` without a corpus fails.
   - Ordinary no-corpus mode succeeds.
2. Implement the shell wrapper and generated Cargo harness.
3. Run nested pure module tests, sibling codec/parser comparisons, and synthetic 75%/125% transformations.
4. Assert report schema/overall status and demo artifact locations/content.
5. Exercise optional corpus discovery and release-required profile checks without copying source data.
6. Run the validator twice and verify deterministic demo bytes plus stable report structure.
7. Run Assist Tick/check/format/release gates, update canonical progress, and check only Step 1 in the approved plan.

## Report Shape

- Top-level schema, overall status, mode, sibling revision, thresholds, checks, synthetic rate results, and corpus results.
- Each rate result records source/output frames, reduced ratio, pitch error, SNR, clipping, stereo lag, seam, deterministic status, identity status, peak memory, latency, input/output digests, and demo path relative to the repository.
- No timestamps or absolute paths, preserving deterministic structure and privacy.

## Risks

- The generated harness must include `src/core/xact/` under a `core::xact` module path so current imports compile unchanged.
- Shell failure paths must not leave a stale successful report; the script removes the prior report before validation.
- Corpus validation is optional in ordinary mode but mandatory and profile-complete when explicitly requested.
