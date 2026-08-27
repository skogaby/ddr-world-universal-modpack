# Task: SE Bank-Pair Generator

## Description

Add an entry point to this crate that converts a short audio file into a complete XACT bank pair —
an in-memory (buffer) wave bank plus an SE-profile sound bank — suitable for a mod to register
directly with DDR World's XACT 2 engine at runtime. Also add a header-dump mode that prints a wave
bank's metadata as text, so that a downstream build script can compare generated banks against the
game's own stock banks without reimplementing the parser.

The crate already has every primitive needed: an Ogg decoder, an MS-ADPCM encoder, a fully
parameterized XWB v43 writer, and (from the preceding task) an SE-profile XSB writer. This task is
the assembly glue plus the format constants that make the output conform to what the engine's
validator accepts.

## Background

**Working directory: the `ddr-chart-tools` repository** — a sibling checkout, not the modpack repo
that this task file lives in. Read that repository's `CLAUDE.md` first.

The consumer is an "assist tick" feature in a sibling project (a hook DLL for DDR World) that plays
a short clap at each arrow's chart timestamp. It routes the sound through the game's own audio
engine, which means handing that engine a wave bank and a sound bank it accepts. Two properties of
the engine shape this task:

- The engine's in-memory wave-bank creation path **validates the container strictly** — exact
  segment offsets, an identity between the file length and the wave-data segment's declared length,
  a required entry-name element size even when no names are used, a minimum alignment, and
  name-field termination. The validator's rules were transcribed during reverse engineering and are
  recorded in the research note referenced below.
- Every wave entry in every DDR bank on disk is **MS-ADPCM**; there is not one raw-PCM entry
  anywhere. PCM is structurally accepted by the validator but that playback path is entirely
  unexercised on real hardware, so the generated bank uses ADPCM deliberately. Mono is proven in use
  by the game's own in-memory system SE bank.

Because a malformed sound bank is rejected *silently* by the engine (audio just goes dark, with no
error), the tests in this task are the last line of defence before a human is left guessing at a
cabinet.

## Reference Documentation

**Required:**
- Design: `.agents/planning/20260725-assist-tick/design/detailed-design.md` (in the sibling
  *modpack* repository) — §4.4 "Asset pipeline" for the required container shape and naming rules;
  §7.1 for the offline validation this task's tests implement
- `.agents/planning/20260725-assist-tick/research/xact-bank-format.md` (in the sibling modpack
  repository) — the primary specification for this task: the engine's validator rules, the exact
  wave-bank flag/alignment/version values, the stock in-memory banks' anatomy for comparison, the
  meaning of the per-entry duration field, and the encoder parameters (mono, 44100 Hz, and the
  block-alignment value DDR uses)

**Additional References (if relevant to this task):**
- `docs/xsb_format.md` (in **this** repository) — for the sound-bank side, already implemented by
  the preceding task; read only if the two sides need reconciling
- This repository's existing `src/xwb/container.rs`, `src/xwb/adpcm/encode.rs`, and
  `src/ogg/decode.rs` — the primitives being assembled

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. Add a generator entry point that takes an input audio file path, a bank/cue name, and an output
   directory, and writes two files: the wave bank and the sound bank. Expose it in whichever way
   fits this crate's existing conventions (a CLI subcommand, a committed example, or a library
   function with a thin binary over it) — the choice is the implementer's, but it must be
   invocable non-interactively from a shell script and must be documented in the repository's
   README.
2. Decode the input to PCM, then encode to **MS-ADPCM, mono, 44100 Hz**, using the block-alignment
   value the research note records for DDR's banks. If the decoded audio is not mono 44100 Hz,
   fail with a clear error naming what was found — do **not** silently resample or downmix.
3. Emit a **buffer (non-streaming)** wave bank with the flags, alignment, header version, and
   entry-name element size the research note specifies, containing **exactly one entry** named
   after the bank name, with the duration and loop fields set as that note documents.
4. Emit the companion sound bank via the SE-profile writer from the preceding task, with the
   sound-bank and wave-bank name fields matching the wave bank's internal name **byte-for-byte,
   case included**.
5. Generation must be **deterministic**: the same input file and name must produce byte-identical
   output across runs and machines. Any timestamp-like field in the container must be fixed rather
   than taken from the clock.
6. Add a **header-dump mode** that parses an existing wave bank and prints its bank-level and
   per-entry metadata as stable, greppable text. It must work on the game's own stock banks, not
   only on generated ones. This is what the downstream build script uses for comparison, so the
   output format should be treated as an interface and documented.
7. Errors must be surfaced through this crate's existing error types with actionable messages; no
   panics on bad input, missing files, or unsupported formats.
8. This repository's gates must pass clean: `cargo test`,
   `cargo clippy --all-targets -- -D warnings` (zero warnings), and `cargo fmt` (zero diff).

## Dependencies

- **Task 01 (SE-profile sound bank writer)** must be complete — this task calls it
- Existing `src/xwb/container.rs` (writer), `src/xwb/adpcm/encode.rs`, `src/ogg/decode.rs`
- No new crate dependencies
- A copy of DDR World's stock in-memory SE wave banks is needed for the comparison test in
  criterion 6. These live inside the game install and are **not** committed to either repository. If
  they are not available in the environment, that one test must skip with a clear message rather
  than fail — the rest of the suite must not depend on them

## Implementation Approach

1. Read the research note's container recipe and validator-rules sections in full, and the stock
   in-memory bank anatomy tables. These give concrete target values for every field; do not infer
   them from the public XACT headers, which differ from this engine version in places.
2. Wire the decode → encode → container-assembly path, constructing the wave-bank struct explicitly
   with the specified constants rather than defaulting anything.
3. Implement the header-dump mode over the existing parser.
4. Write the tests below, then run the three gates.
5. Generate a bank from the real clap sample as a smoke check and record its byte size in the
   task's progress notes, so the downstream script has an expected value.

## Acceptance Criteria

1. **A bank pair is generated from a real input**
   - Given the assist-tick clap sample (mono 44100 Hz Ogg Vorbis, roughly 0.21 s)
   - When the generator is invoked with a four-character name and an output directory
   - Then two files are written, the wave bank parses cleanly through this crate's own parser, and
     the sound bank is the expected size for a single-cue SE-profile bank

2. **The container conforms to the engine's validator**
   - Given a generated wave bank
   - When each validator rule recorded in the research note is asserted against its bytes — segment
     offsets, the file-length/wave-data-segment identity, entry-name element size, minimum
     alignment, bank-name termination, and entry flag bits
   - Then every rule holds

3. **The audio survives the encode**
   - Given a generated wave bank
   - When its single entry is decoded back to PCM with this crate's ADPCM decoder
   - Then the sample count matches the source within the codec's block granularity, the result is
     not silence, and it does not clip

4. **Wave-bank and sound-bank names agree**
   - Given a generated pair
   - When the wave bank's internal name and the sound bank's wave-bank name field are compared
   - Then they are byte-identical, case included

5. **Generation is deterministic**
   - Given the same input file and name
   - When the generator is run twice into different directories
   - Then both outputs are byte-identical

6. **Generated banks match the stock banks' shape**
   - Given the game's stock in-memory SE wave banks, when available in the environment
   - When both they and a generated bank are passed through the header-dump mode
   - Then the bank-level fields that the design requires to match (container version, flag bits,
     alignment, entry-name element size) are identical, and the only differences are the intended
     ones: bank name, entry count, and per-entry format and length
   - And when the stock banks are not available, the test skips with an explanatory message instead
     of failing

7. **Bad input fails cleanly**
   - Given a stereo input file, a non-44100 Hz input file, a missing file, and a file that is not
     audio
   - When the generator is invoked with each
   - Then it returns a distinct, actionable error in every case and never panics

8. **Header-dump output is stable and documented**
   - Given the same wave bank
   - When the header-dump mode is run twice
   - Then the output is identical, and its format is documented alongside the entry point

9. **Gates are clean**
   - Given the completed change
   - When `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check` are run
   - Then all three pass with no failures, no warnings, and no diff

## Metadata
- **Complexity**: Medium
- **Labels**: binary-format, xact, audio, adpcm, cli, cross-repo
- **Required Skills**: Rust; audio codec plumbing (PCM/ADPCM); binary container assembly against a
  transcribed validator; CLI ergonomics
- **Generated By**: code-task-generator 2026-07-25
- **Source Plan**: `.agents/planning/20260725-assist-tick/implementation/plan.md`
- **Plan Step**: Step 1 — Generate and commit the clap bank pair (offline asset pipeline)
