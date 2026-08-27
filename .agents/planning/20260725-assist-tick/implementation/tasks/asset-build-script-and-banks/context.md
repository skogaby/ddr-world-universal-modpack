# Context — Assist-Tick Asset Build Script and Committed Banks (Step 1, task 03)

**Task file:** `.agents/tasks/20260725-assist-tick/step01/task-03-asset-build-script-and-banks.code-task.md`
**Working docs:** this directory. Same override and rationale as tasks 01/02.

## Upstream approval chain

Verified as before. Its stated dependency — **task 02** — is complete, gates green, uncommitted in
the sibling repo.

## Build / test commands

This repo has **no test harness**; validation is deploy-and-observe. For this task specifically the
gates are the script's own exit status plus its post-generation assertions, and — because no Rust
changes — `cargo check` only as a sanity check that nothing else was disturbed.

Baseline: working tree clean at `7d51fd5 Checkpoint before implementing assist tick`.

## Functional requirements

Restated from the task file, 11 requirements and 9 acceptance criteria. Not duplicated here; the
plan maps each to how it is met. The two that dominate every other decision:

- **R7 / AC7 — no machine-specific content in anything committed.** This repo is going open source.
  Every external location resolves through a flag or an environment variable, with a
  repository-relative or documented-standard default.
- **R9 / AC2 — reproducible.** Running the script twice, and running it on a clean checkout, must
  reproduce the committed bytes exactly.

## Names: the file names and the bank/cue name differ, deliberately

Design §4.4 specifies the committed files as `tick.xwb` / `tick.xsb`, while §3.3/§4.2 specify the
cue the DLL plays as `asti`. Those are **two independent namespaces**, and the game itself does the
same thing: research §6.1 records that the stock file is `data/sound/win/se_system.xwb` (lowercase)
while its *internal* bank name — the one the engine matches a sound bank against — is `SE_SYSTEM`
(uppercase).

Nothing resolves our banks by filename: the DLL reads the two files by path and hands their bytes to
`CreateInMemoryWaveBank` / `CreateSoundBank`; the engine links the pair by internal name and finds
the cue by `strcmp`. So the script generates with `--name asti` and renames the outputs to
`tick.*`. No ambiguity to escalate — the design is consistent once the two namespaces are separated.

## The ffmpeg conflict, and how it is resolved

**R3** says: "transcode the source to 16-bit PCM mono 44100 Hz with `ffmpeg`, then invoke the
sibling repository's bank generator". **R9/AC2** says the output must reproduce the committed bytes
on a clean checkout, on any machine.

These conflict. `ffmpeg -c:a libvorbis` output depends on the host's libvorbis build, so a
re-encode in the default path would make the committed bytes irreproducible elsewhere — and there is
nothing to gain: research §8.1 established that `clap.ogg` is *already* mono 44.1 kHz, so
`ffmpeg -ac 1 -ar 44100` would be a lossy no-op.

Resolution: **ffmpeg is used when, and only when, the source does not already conform.** The script
probes with `ffprobe`; a conforming Ogg Vorbis mono/44100 source goes straight to the generator (the
reproducible path, which is what the committed asset uses), and a non-conforming one is transcoded
with a loud warning that the result will not be byte-reproducible across machines. R3's ffmpeg step
is a *means*; R9 is an *end*, and it is also acceptance criterion 2.

Consequence: **ffmpeg is an optional prerequisite**, needed only for a non-conforming source. Stated
as such in the script header. If neither ffprobe nor ffmpeg is present and the source does not
conform, the generator rejects it with an actionable message and the script prints the exact ffmpeg
command to run — no silently broken path.

## Generator discovery (R4)

Three routes, in order: `--chart-tools <path>`, `$DDR_CHART_TOOLS`, then `ddr-se-bank` on `PATH`.
For the first two the path may reasonably be any of three things, so all are handled:

| Given | Invocation |
|---|---|
| a directory containing `Cargo.toml` (a repo checkout) | `cargo run --release --manifest-path <p>/Cargo.toml --bin ddr-se-bank --` |
| a directory containing the built executable | `<p>/ddr-se-bank` |
| the executable itself | `<p>` |

Failure names the flag, the variable, and what the tool is.

## Validation surface

The engine silently rejects a malformed sound bank, so validation is the point of this task. It runs
against the generated wave bank via `ddr-se-bank dump`'s documented `key=value` output plus shell
assertions — R5 explicitly says not to reimplement the parser.

Dump keys → validator rules asserted:

| Dump key(s) | Engine rule |
|---|---|
| `bank.type` must be `buffer` | the bank-type bit, a hard gate in both directions |
| `bank.header_version` = 42 | `IsHeaderValid` |
| `bank.version` = 43, `bank.flags` = `0x00090000` | stock in-memory value; rule 5's legal-bit mask |
| `segment0.offset` = 52, `segment0.length` = 96 | rules 2, 4 |
| `segment1.offset` = 148, `segment1.length` = 24 | rules 13, 14 |
| `segment2.length` = 0 | forced by rule 15 |
| `segment3.offset` = 172, `segment3.length` = 64 | rules 15, 17 |
| `file.length − segment4.offset` = `segment4.length` | **rule 20** — the easiest to get wrong and fatal |
| `bank.entry_metadata_element_size` = 24 | rule 6a |
| `bank.entry_name_element_size` = 64 | rule 10 |
| `bank.alignment` = 4 | rule 11 |
| `bank.name_terminated`, `entry0.name_terminated` | rules 9, 18 |
| `entry0.entry_flags` = 0 | rule E1 |
| `entry0.duration` ≥ `loop_start + loop_length` | rule E7 |
| `entry0.codec` = 2, `channels` = 1, `sample_rate` = 44100 | rules E3–E5 |
| `entry0.data_length % block_align` = 0 | not a rule — stock banks are sloppy here (126 of 138 entries are one byte short of a whole block); we are strict deliberately |

Plus, outside the dump: the sound bank's wave-bank-name field (the 64 bytes at `0x8A`) must equal
the wave bank's internal name, and the sound bank's size must be exactly `0x101 + len(name) + 1`.

### Stock comparison (R5 last bullet, R6)

Runs only when `DDR_WORLD_INSTALL` is set and `data/arc/se_normal.arc` is readable, else skipped
with a notice and **exit 0**. `se_normal.arc` is a single-entry ARC stored **raw** (research §2.1),
so extracting `se_normal.xwb` needs nothing but reading the payload offset and length out of the
16-byte-per-entry table and slicing — no AVSLZ. `se_system.arc` is AVSLZ-compressed and is
deliberately *not* used.

Reading from the install is not installing; nothing is ever written there (R8).

## Sizes to expect

From task 02's verified run on the real clap: **`tick.xwb` 5,416 B**, **`tick.xsb` 262 B**. The
design's "roughly 5.5 KB and 330 B" is right for the wave bank; the 330 B figure is stale — it was
the *song*-profile sound bank's size, and the SE profile is smaller.

## Non-functional expectations

- `set -euo pipefail`, `cd "$(dirname "$0")/.."`, `die()`, `${VAR:-default}` — all following
  `scripts/build_shaders.sh`, which R-approach explicitly names as the precedent.
- `python3` is *not* required (unlike `build_shaders.sh`): `od` covers the one binary read needed.
- Temp work in `mktemp -d` with a trap, so a failure leaves no partial output (AC3).
