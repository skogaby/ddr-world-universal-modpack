# Task: Add Host Validation Harness

## Description
Create the mandatory host-side validator for the complete Step 1 audio pipeline.
The script must compile the pure modules beside the sibling `ddr-chart-tools`
implementation, run synthetic and optional release-corpus validation, generate
75%/125% demo banks outside source control, and emit the stable JSON evidence
required by later implementation steps.

## Background
Step 1 is complete only when the format, codec, rate, stretch, and transformer
can be exercised independently of the Windows DLL and produce measurable proof.
The existing Assist Tick validator demonstrates the throwaway-host-harness
pattern. This task turns Tasks 1-3 into the approved command and closes the Step
1 checklist only after its demo and all build gates pass.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-08-05-song-playback-speed/design/detailed-design.md`

**Additional References (if relevant to this task):**
- `.agents/planning/2026-08-05-song-playback-speed/research/pitch-preservation.md`
- `scripts/validate_se_bank_synth.sh`
- Sibling repository `ddr-chart-tools`: `Cargo.toml`, `src/xwb/`
- `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md` Step 1 demo

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Add executable `scripts/validate_song_playback_speed.sh` using a throwaway host Cargo harness or an equivalently isolated host runner; it must not link/load the game DLL.
2. Support `DDR_CHART_TOOLS_DIR` with default `../ddr-chart-tools` and `DDR_SONG_RATE_CORPUS_DIR` as documented by the design.
3. Run all Tasks 1-3 tests plus cross-repository parser/codec comparisons and shared synthetic fixtures.
4. In ordinary development mode, permit synthetic-only validation; in release-corpus mode, fail nonzero if the corpus or required profiles are missing.
5. Validate external local stock/custom banks by digest without copying them into the repository; cover both entry orders and both 75%/125% outputs.
6. Measure exact duration/rate, pitch error, SNR, clipping, stereo lag, preview seam, deterministic output, peak working set, and generation latency against the approved thresholds.
7. Write stable schema `song-rate-validation/v1` to `target/song-rate-validation/report.json`; include input digests, sibling revision, per-check status/metrics, and overall pass/fail.
8. Generate demo XWBs only under ignored target/temp output and clearly print their paths; never write game-derived output under tracked source/data directories.
9. Exit 0 only when every required check passes; unknown arguments, missing sibling source, failed checks, or missing release-required corpus exit nonzero with actionable errors.
10. Run `./scripts/validate_se_bank_synth.sh` to prove Assist Tick remains green, then run the normal repository build gates.
11. After the complete Step 1 demo passes, update canonical `progress.md` to Step 1 done with Step 2 as `NEXT ACTION`, and check only the Step 1 checkbox in the approved implementation plan.

## Dependencies
- Task 1 shared XACT/XWB/MS-ADPCM code.
- Task 2 exact rate and deterministic stretch code.
- Task 3 complete song-XWB transformer.
- Host Cargo toolchain and sibling `ddr-chart-tools` source checkout.
- Local corpus only for release-grade validation; no corpus files are committed.

## Implementation Approach
1. Mirror the proven throwaway-harness structure from `validate_se_bank_synth.sh` while keeping an independent stable report schema.
2. Wire shared module tests and sibling comparison into one command.
3. Add synthetic fixtures and corpus discovery/digest reporting.
4. Add metric collection, threshold evaluation, demo-output isolation, and stable JSON serialization.
5. Exercise success/failure CLI paths and determinism of report structure.
6. Run synthetic mode, release-corpus mode, Assist Tick regression, and repository gates.
7. Record evidence in canonical progress and mark Step 1 complete only after every Task 1-4 criterion passes.

## Acceptance Criteria

1. **Synthetic Host Validation**
   - Given a sibling `ddr-chart-tools` checkout and no game corpus
   - When `./scripts/validate_song_playback_speed.sh` runs in ordinary mode
   - Then it compiles/runs outside the DLL, validates all pure pipeline fixtures, emits schema-v1 JSON, and exits 0 only on complete success

2. **Release Corpus Validation**
   - Given a local corpus containing supported stock/custom banks in both entry orders
   - When release-corpus validation runs
   - Then input hashes are recorded without copying source audio, both rates pass profile/reparse/metric checks, and demo outputs remain under ignored target/temp storage

3. **Measured Thresholds**
   - Given the synthetic and local release corpus
   - When timing, memory, pitch, SNR, clipping, stereo, seam, exact-length, and determinism metrics are evaluated
   - Then every metric is present in the JSON report and any threshold violation causes a nonzero exit

4. **Actionable Failure Behavior**
   - Given missing sibling source, missing required corpus, malformed bank, failed comparison, or unknown CLI input
   - When the validator runs
   - Then it exits nonzero, identifies the failed precondition/check, and does not leave tracked or falsely successful output

5. **Regression and Build Gates**
   - Given the complete Step 1 implementation
   - When the song-rate validator, Assist Tick validator, target `cargo check`, `cargo fmt`, and `./build.sh` run
   - Then every command passes without unrelated source churn

6. **Step 1 Demo and Closure**
   - Given passing 75% and 125% local demo transformations
   - When the final report is inspected
   - Then it shows exact frame ratios, pitch within tolerance, preserved bank/entry identity, and overall success; canonical progress records Step 1 done and the plan checks only Step 1

## Metadata
- **Complexity**: Medium
- **Labels**: shell, rust, host-validation, cross-repo, audio-metrics, step-1
- **Required Skills**: code-assist, verification, self-documenting-code
- **Generated By**: code-task-generator 2026-08-05
- **Source Plan**: `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md`
- **Plan Step**: Step 1: Build the deterministic host audio pipeline
