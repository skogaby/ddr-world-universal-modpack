# Progress — Assist-Tick Asset Build Script and Committed Banks (Step 1, task 03)

**Updated:** 2026-07-26
**Status:** Complete, all verification green. **Not committed** — the maintainer owns commits.

## Checklist

- [x] `scripts/build_assist_tick_bank.sh` added, executable, strict mode, documented header
- [x] `data_mods/assist_tick/source/clap.ogg` committed (10,704 B)
- [x] `data_mods/assist_tick/banks/tick.xwb` (5,416 B) and `tick.xsb` (262 B) generated + committed
- [x] Generator discovery: flag, env var, `PATH` — with a message naming all three on failure
- [x] Post-generation validation of the engine's container rules, via the dump, no second parser
- [x] Optional stock-bank comparison, gated on `DDR_WORLD_INSTALL`, skip ⇒ exit 0
- [x] `--self-check` proving validation rejects a corrupted bank and names the rule
- [x] README section
- [x] V1–V10 all run (below)

## Files (this repo, uncommitted)

| File | Change |
|---|---|
| `scripts/build_assist_tick_bank.sh` | **new**, 353 lines, executable |
| `data_mods/assist_tick/source/clap.ogg` | **new** — 10,704 B, Ogg Vorbis mono 44100 Hz, 9,423 samples |
| `data_mods/assist_tick/banks/tick.xwb` | **new** — 5,416 B, `sha256:46e6602892dc681c…` |
| `data_mods/assist_tick/banks/tick.xsb` | **new** — 262 B, `sha256:d2fe533ea65e03e6…` |
| `README.md` | new `## Assist Tick Sound` section |

Nothing under `src/`. `cargo check --target x86_64-pc-windows-msvc` still clean.

## Verification run

| # | Check | Result |
|---|---|---|
| V1 | first run, no arguments | exit 0, both files written, summary printed |
| V2 | second run | exit 0, "committed files reproduced byte-for-byte (unchanged)" |
| V3 | generator not locatable anywhere | exit 1, message naming `--chart-tools`, `DDR_CHART_TOOLS`, `PATH` and what the tool is; **no partial output** (only the `source/` dir existed) |
| V4 | `DDR_WORLD_INSTALL` unset | exit **0**, "stock-bank comparison SKIPPED: DDR_WORLD_INSTALL is not set" |
| V4b | `DDR_WORLD_INSTALL=/tmp` (set, but no arc) | exit **0**, skipped naming the unreadable path |
| V5 | `DDR_WORLD_INSTALL` = the real install | comparison **runs and passes** against the install's own `se_normal.xwb` |
| V6 | `--self-check` | exit 0 having proved validation rejects a corrupted bank, printing the violated rule |
| V7 | grep the committed script for `/Users/`, `/home/`, `$HOME`, a username, `CrossOver`, `Bottles` | **no matches** |
| V8 | any copy into a game install | none — `DDR_WORLD_INSTALL` appears only in reads; the only `cp`s target the temp dir and this repo's `data_mods/` |
| V9 | reproducibility | byte-identical across runs, across debug/release generator builds, and after an intervening failed run |
| V10 | `cargo check --target x86_64-pc-windows-msvc` | clean |
| — | non-conforming input (stereo 48 kHz) | exit 1, generator's own message surfaced (`input must be mono, got 2 channels`) plus the conversion command; **committed banks untouched** |
| — | `bash -n` | clean. `shellcheck` is not installed on this machine — flagged below |

## Two bugs this task's own verification caught

### B1 — validation silently did not fail inside a conditional (`set -e` trap)

`--self-check` reported *"validation accepted a bank with a corrupted header version"* — and it was
right. `validate_pair` originally relied on `set -e` to abort on a failed assertion. But `set -e` is
disabled for the entire call tree of a command used as a condition, so calling
`if validate_pair …; then` made every assertion non-fatal and the function returned the status of
its *last* statement, which succeeded.

Fixed by counting failures in `VALIDATION_ERRORS` and returning `[[ $VALIDATION_ERRORS -eq 0 ]]`
explicitly. Better in two ways: correct in both calling contexts, and a single run now reports
*every* violated rule instead of only the first. This is exactly what `--self-check` was for; without
it the validation stage would have shipped looking like it worked.

### B2 — hand-computed ARC magic number was wrong

The stock comparison silently skipped with "unexpected ARC magic" because I wrote `0x19751120` as
the decimal literal `426190624`; it is `427102496`. Replaced with `$((0x19751120))` so bash does the
conversion and the constant reads as the hex value it actually is.

## Deviations

### D1 — the ffmpeg transcode branch was removed, on the maintainer's instruction

Requirement 3 says "transcode the source to 16-bit PCM mono 44100 Hz with `ffmpeg`, then invoke the
generator". That only composes if the generator accepts WAV; task 02's generator accepts **Ogg
Vorbis** (rationale in that task's `context.md` — reproducibility). So the script initially
transcoded to *Ogg* instead.

That turned out to be unworkable on this machine, which is a useful signal rather than a local
quirk: this host's ffmpeg has no `libvorbis` at all, and its built-in `vorbis` encoder **cannot
produce mono** ("Current FFmpeg Vorbis encoder only supports 2 channels"). So the one branch that
requirement 3 describes could not run here, while `pcm_s16le`/WAV — which requirement 3 literally
asks for — works in every ffmpeg build.

Escalated to the maintainer as a three-way choice: add minimal WAV input to the generator; keep
Ogg-only with a convert-it-yourself message; or keep Ogg-only and drop the branch entirely.
**Maintainer chose to drop the branch entirely.**

Result: no ffprobe, no ffmpeg, no conditional path, and no prerequisite beyond the generator. The
input goes straight in; if it does not conform, the generator's own error is surfaced verbatim
(`input must be mono, got 2 channels — downmix it yourself…`) followed by the command to convert it.
Requirement 3's ffmpeg step is therefore **not implemented**, deliberately and with approval, and
requirement 9's reproducibility guarantee is strengthened rather than compromised — the only path
that exists is the reproducible one. ffmpeg is no longer listed as a prerequisite.

The WAV-input option remains available as a clean additive follow-up if the maintainer ever wants
`ffmpeg | ddr-se-bank` to work end to end.

### D2 — the generator is built once rather than invoked through `cargo run`

Discovery originally used `cargo run --manifest-path <checkout>/Cargo.toml`. Run from this repo,
rustup resolves the toolchain from the **current directory**, so it picked up this repo's nightly pin
and printed a toolchain-sync banner on every one of the script's several generator invocations —
noisy, slow, and building the sibling crate with the wrong toolchain. Now the script builds once in a
subshell `cd`'d into the checkout (so that repository's own toolchain applies) and invokes the
resulting binary directly.

### D3 — `--self-check` and `--name` were added beyond the task's argument list

Acceptance criterion 6 requires the validation stage to be exercised against a deliberately malformed
bank. `--self-check` makes that a repeatable property of the script rather than a manual one-off — and
it immediately earned its keep by finding B1. `--name` exists because the bank/cue name is a real
parameter of the format and hard-coding it in two places invites drift.

### D4 — README section slightly precedes the mod

Requirement 11 asks for README documentation now, but the mod that consumes these assets does not
exist until Step 3, and the plan assigns the mod's own README section (what it does, how to enable it,
the label caveat, StepMania attribution) to **Step 6**. The section added here is scoped to the
*asset and its pipeline*. Step 6 should fold it together with the mod's entry in the *Included Mods*
table rather than adding a second, overlapping section. Noted so it is not duplicated.

### D5 — Not committed

As tasks 01 and 02.

## Notes and open items

- **`shellcheck` is not installed here**, so the script has only had `bash -n` plus the ten
  behavioural checks above. Worth a `shellcheck` pass if the maintainer has it — the script is 353
  lines of bash and it is the kind of code static analysis helps.
- `od -An -tu4` assumes a little-endian host when reading the ARC header. Every platform this repo
  targets is LE; noted in a comment at the call site. It is only reached in the optional stock branch.
- `dd`'s `iflag=skip_bytes,count_bytes` is GNU-only, so the ARC slice falls back to
  `tail -c +N | head -c L` on BSD/macOS. Both paths are present; the fallback is what actually runs
  here, and it produced a bank that parsed and matched.
- The design's stated sizes ("roughly 5.5 KB and 330 B") are right for the wave bank and stale for
  the sound bank — 262 B, because 330 B was the *song*-profile figure. Worth correcting in the design
  doc if it is ever revised.
