# Task: Add Shared XACT Format Codecs

## Description
Implement the game-independent XACT2 wave-bank and MS-ADPCM foundation required
for pitch-preserved song-rate generation. Port and adapt the focused format code
from the sibling `ddr-chart-tools` repository into a shared `src/core/xact/`
boundary, enforce the approved DDR World song-bank profile, and migrate Assist
Tick only after byte-parity proves the shared codec is behavior-preserving.

## Background
Song-rate generation must parse stock and compatible custom streaming XWBs,
decode stereo MS-ADPCM, and rebuild byte-valid banks without depending on the
sibling CLI crate at runtime. The current hook repository has a fixed mono
Assist Tick encoder and writer, while `ddr-chart-tools` has the broader container
and codec implementation. This task establishes one reusable format/codec source
before any time stretching or game hook integration is added.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-08-05-song-playback-speed/design/detailed-design.md`

**Additional References (if relevant to this task):**
- `.agents/planning/2026-08-05-song-playback-speed/research/pitch-preservation.md`
- `.agents/planning/20260725-assist-tick/research/xact-bank-format.md`
- `scripts/validate_se_bank_synth.sh`
- `src/services/se_bank_synth/`
- Sibling repository `ddr-chart-tools`: `src/xwb/container.rs`, `src/xwb/adpcm/`, `src/util/io.rs`

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Before editing code, create `.agents/planning/2026-08-05-song-playback-speed/progress.md` using the repository's canonical progress format; record Step 1 in progress, this task as the next action, and the exact sibling source revision.
2. Add shared pure XACT modules under `src/core/xact/` and export them from `src/core/mod.rs`; keep them independent of Windows/game-hook services so a host harness can compile them directly.
3. Implement a borrowed XWB v43 parser and serializer with typed errors and checked arithmetic.
4. Enforce the exact approved two-entry streaming profile: version/header/flags, segment lengths/order/bounds, metadata/name sizes, 2048 alignment, `<code>` and `<code>_s` names in either order, stereo MS-ADPCM format, entry flags, duration, loop, and data-range invariants.
5. Support documented stock trailing remainders only when the logical-duration/complete-block equation holds; generated output must contain exact complete blocks.
6. Port/adapt arbitrary-channel MS-ADPCM decode and direct interleaved block encode without silent padding or whole-song channel duplication.
7. Preserve bank and entry identity/order, packed format, sample rate, and XSB-visible indices in serializer output.
8. Record the sibling source revision and intentional deltas in module documentation.
9. Migrate Assist Tick's compatible ADPCM use only after tests prove byte-identical output, silence blocks, and container behavior; retain its fixed XSB/container policy locally.
10. Add tests with synthetic fixtures only; game-derived banks must remain external and uncommitted.
11. Run `./scripts/validate_se_bank_synth.sh` after any Assist Tick migration, then run the repository check/format/release build gates.

## Dependencies
- Approved song-playback-speed design and Step 1 plan.
- Read access to the sibling `ddr-chart-tools` source checkout.
- Existing `src/services/se_bank_synth` implementation and host validator.
- No dependency on later Step 1 tasks.

## Implementation Approach
1. Create the canonical feature progress record and capture the sibling revision.
2. Define minimal shared XWB/format/error models and strict parser validation.
3. Port the serializer with checked segment/alignment arithmetic and streaming writes.
4. Port/adapt the generic MS-ADPCM decoder and direct interleaved block encoder.
5. Build synthetic main/preview banks in both entry orders and add malformed-profile tests.
6. Prove codec parity with the sibling implementation and Assist Tick before migrating shared primitives.
7. Run host parity validation and all repository build gates; update `progress.md` with outcomes and the next task.

## Acceptance Criteria

1. **Strict XWB Parsing**
   - Given valid synthetic DDR World song banks in both main/preview entry orders
   - When the shared parser reads them
   - Then it returns borrowed entry views with preserved identity and rejects every malformed field covered by the approved profile tests

2. **Stock Tail Compatibility**
   - Given entries with allowed zero, one-byte-short, and two-byte-short trailing remainders whose declared durations fit complete blocks
   - When they are decoded
   - Then PCM is trimmed to logical duration, while arbitrary tails or inconsistent complete-block counts are rejected

3. **Deterministic MS-ADPCM Codec**
   - Given mono and stereo synthetic PCM with exact block-aligned lengths
   - When it is encoded twice and decoded
   - Then both encodings are byte-identical, contain no hidden padding, preserve channel ordering, and satisfy the required SNR/error checks

4. **Serializer Identity**
   - Given a parsed supported bank with replacement encoded payloads
   - When it is serialized and reparsed
   - Then bank name, entry names/order, format fields, durations/loops, aligned ranges, and segment framing match the requested output exactly

5. **Assist Tick Regression Safety**
   - Given the existing Assist Tick fixtures and clap synthesis
   - When shared codec primitives replace compatible local code
   - Then `validate_se_bank_synth.sh` remains fully green and generated bytes/placement are unchanged

6. **Build Readiness**
   - Given all format and codec changes
   - When the host parity checks, `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`, and `./build.sh` run
   - Then all commands pass and `progress.md` records the evidence and Task 2 as the next action

## Metadata
- **Complexity**: High
- **Labels**: rust, xact, xwb, ms-adpcm, audio-formats, step-1
- **Required Skills**: code-assist, verification, self-documenting-code
- **Generated By**: code-task-generator 2026-08-05
- **Source Plan**: `.agents/planning/2026-08-05-song-playback-speed/implementation/plan.md`
- **Plan Step**: Step 1: Build the deterministic host audio pipeline
