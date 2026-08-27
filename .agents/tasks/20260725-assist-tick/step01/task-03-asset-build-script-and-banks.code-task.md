# Task: Assist-Tick Asset Build Script and Committed Banks

## Description

Add `scripts/build_assist_tick_bank.sh` to this repository, and commit the assets it produces: the
StepMania clap sample as the pipeline's input, and the generated XACT wave-bank + sound-bank pair
that the assist-tick mod loads at runtime.

The script transcodes the source sample, invokes the bank generator in the sibling
`ddr-chart-tools` repository, and validates the result — including, when a game install is
available, comparing the generated container's shape against the game's own stock in-memory sound
effect banks.

This closes Step 1 of the plan: after this task, every later step has a real, validated asset to
load.

## Background

**Working directory: this repository** (the DDR World hook DLL / modpack).

The assist-tick mod plays its clap through the game's own XACT 2 audio engine, which means handing
that engine a wave bank and a sound bank it accepts. Those two files are generated **offline, once**
and committed, rather than synthesized at runtime — because the engine **silently rejects** a
malformed sound bank (no error, audio simply goes dark), which is miserable to diagnose against a
running game. Generating offline lets the format be validated before it ever reaches the game.

This mirrors how this repository already ships committed binary build products for its shader
feature: files under `data_mods/<mod>/`, read at runtime through the existing mod-path resolver,
degrading gracefully when absent. Replacing the two bank files replaces the sound with no rebuild.

**This repository is going open source.** That makes machine-independence a hard requirement rather
than a preference — see Technical Requirements 6 and 7, which are as load-bearing here as the
format correctness.

## Reference Documentation

**Required:**
- Design: `.agents/planning/20260725-assist-tick/design/detailed-design.md` — §4.4 "Asset pipeline"
  for the file layout, naming, and format decisions; §7.1 for the offline validation this script
  performs; §7.3 for how the local install is used during testing
- `.agents/planning/20260725-assist-tick/implementation/plan.md` — Step 1, whose demo requirement
  this task satisfies

**Additional References (if relevant to this task):**
- `.agents/planning/20260725-assist-tick/research/xact-bank-format.md` — the stock in-memory bank
  anatomy tables (the comparison targets for requirement 5) and the engine's validator rules
- `scripts/build_shaders.sh` — the in-repo precedent for a committed asset-build script, including
  the `${VAR:-default}` environment-override pattern this script should follow

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. Add `scripts/build_assist_tick_bank.sh`: bash, `set -euo pipefail`, executable, with a comment
   header documenting what it produces and what it requires. Re-runnable and idempotent.
2. Commit the source sample at `data_mods/assist_tick/source/clap.ogg` (copy it from wherever the
   maintainer currently holds it) so the pipeline is reproducible from repository contents alone.
   The script must default to that path as its input, and accept an override argument.
3. Pipeline: transcode the source to 16-bit PCM mono 44100 Hz with `ffmpeg`, then invoke the sibling
   repository's bank generator to emit `data_mods/assist_tick/banks/tick.xwb` and
   `data_mods/assist_tick/banks/tick.xsb`, with the bank/cue name the design specifies.
4. Locate the generator **without any hardcoded path**: accept `--chart-tools <path>` and/or the
   `DDR_CHART_TOOLS` environment variable, and fall back to the binary being on `PATH`. When it
   cannot be found, exit non-zero with a message that names the flag, the variable, and what the
   tool is.
5. Validate after generating, and fail the script on any violation:
   - assert the engine's container-validator rules against the generated wave bank (the sibling
     tool's header-dump mode plus shell assertions is sufficient — do not reimplement the parser)
   - assert the two files' sizes are within the expected range
   - assert the sound bank's wave-bank name matches the wave bank's internal name
   - **when, and only when, a game install is available**: dump the install's stock in-memory sound
     effect wave banks and assert that the bank-level fields the design requires to match do match
6. **The stock-bank comparison must be optional.** Run it only when a game install is pointed at via
   the `DDR_WORLD_INSTALL` environment variable and the expected files are readable. Otherwise skip
   that check with a clear notice and **exit 0** — someone cloning this repository with no game
   install must still be able to run the script successfully.
7. **No machine-specific content anywhere in what is committed.** No absolute paths, no usernames,
   no home-directory literals, no references to the maintainer's directory layout. Every external
   location comes from an environment variable or a flag, with defaults that are either
   repository-relative or a documented standard install location. A `grep` of the committed script
   for a home path or a username must return nothing.
8. **The script must not install anything.** Copying assets into a game install is explicitly out of
   scope and must not be added, so that no precedent exists for selective `data_mods` subdirectory
   installation. Say so in the script's comment header.
9. Generation must be reproducible: running the script twice must produce byte-identical bank files,
   and re-running it on a clean checkout must reproduce the committed bytes exactly.
10. Print a short summary on success: the input used, both output paths with their sizes, and which
    validations ran versus were skipped.
11. Document the script in the repository README alongside the other asset-pipeline scripts: what it
    does, its prerequisites (`ffmpeg`, the sibling tool), that the sound is swappable by replacing
    the two bank files, and that the generated files are committed so most contributors never need
    to run it.

## Dependencies

- **Task 02 (SE bank-pair generator)** must be complete — this script invokes it, including its
  header-dump mode
- `ffmpeg` on the host (development-time only; not a runtime dependency of the DLL)
- A checkout of the sibling `ddr-chart-tools` repository, located at runtime per requirement 4
- Optional: a DDR World install, for requirement 5's stock-bank comparison only
- `.gitignore` note: this repository ignores `*.arc`, but the assets here are `.xwb`, `.xsb` and
  `.ogg`, which are committable — verified, no `.gitignore` change needed

## Implementation Approach

1. Read `scripts/build_shaders.sh` first and follow its structure and conventions — argument
   parsing, environment overrides, failure messages, and summary output. Consistency with the
   existing script matters more than novelty here.
2. Write the script's discovery and transcode stages, and confirm the generator can be invoked
   non-interactively.
3. Add the validation stage, with the install-dependent comparison behind an availability check.
4. Run it, inspect the outputs, then commit the source sample and both generated banks.
5. Re-run on a clean tree to confirm byte-for-byte reproducibility, and run once with
   `DDR_WORLD_INSTALL` unset to confirm the skip path exits 0.
6. Update the README.

## Acceptance Criteria

1. **The script generates a validated bank pair**
   - Given the committed source sample, `ffmpeg` available, and the sibling tool locatable
   - When `scripts/build_assist_tick_bank.sh` is run with no arguments
   - Then it exits 0, writes both bank files under `data_mods/assist_tick/banks/`, and prints a
     summary naming the input, both outputs with sizes, and the validations performed

2. **Generation is reproducible**
   - Given a previously generated pair
   - When the script is run again
   - Then the output files are byte-identical to the committed ones

3. **A missing generator fails clearly**
   - Given the sibling tool is not on `PATH`, `DDR_CHART_TOOLS` is unset, and no `--chart-tools`
     flag is passed
   - When the script is run
   - Then it exits non-zero with a message naming the flag, the environment variable, and the tool
     — and it does not leave partial output files behind

4. **No game install is required**
   - Given `DDR_WORLD_INSTALL` is unset
   - When the script is run
   - Then it generates and validates the banks, prints a notice that the stock-bank comparison was
     skipped and why, and exits **0**

5. **The stock-bank comparison runs when an install is present**
   - Given `DDR_WORLD_INSTALL` points at a real install
   - When the script is run
   - Then the generated wave bank's container version, flag bits, alignment, and entry-name element
     size are asserted equal to the stock in-memory sound effect banks', and the script fails if any
     differs

6. **A corrupted output is caught**
   - Given the validation stage is exercised against a deliberately malformed bank (a flipped byte
     in a validator-covered field)
   - When the script validates it
   - Then the script fails and names the rule that was violated

7. **The committed script is machine-independent**
   - Given the committed `scripts/build_assist_tick_bank.sh`
   - When it is searched for home-directory literals, absolute user paths, and usernames
   - Then there are no matches, and every external location resolves through a flag, an environment
     variable, or a repository-relative default

8. **No installation behavior exists**
   - Given the committed script
   - When it is read
   - Then it contains no copy into a game install and no flag that would perform one, and its
     comment header states that installing assets is out of scope

9. **Assets are committed and documented**
   - Given a fresh clone of this repository
   - When `data_mods/assist_tick/` is inspected and the README consulted
   - Then the source sample and both generated bank files are present, and the README explains what
     they are, how to regenerate them, and that the sound can be swapped by replacing the two bank
     files

## Metadata
- **Complexity**: Low-Medium
- **Labels**: build-script, asset-pipeline, xact, audio, open-source-hygiene, docs
- **Required Skills**: Bash scripting with strict-mode discipline; `ffmpeg` invocation; binary-output
  validation in shell; writing scripts that degrade cleanly when optional environment resources are
  absent
- **Generated By**: code-task-generator 2026-07-25
- **Source Plan**: `.agents/planning/20260725-assist-tick/implementation/plan.md`
- **Plan Step**: Step 1 — Generate and commit the clap bank pair (offline asset pipeline)
