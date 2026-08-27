# Progress — SE Bank-Pair Generator (Step 1, task 02)

**Updated:** 2026-07-26
**Status:** Implementation complete, all gates green. **Not committed** — the maintainer owns
commits (see task 01's `progress.md` → D4).

## Checklist

- [x] `xwb::dump` — tests first (red), then `describe`; 6 tests
- [x] `job::se_bank` — tests first (red), then `generate`; 14 tests
- [x] `SeBankError` + wrapping variant in `crate::error::Error`
- [x] `ddr-se-bank` binary + second `[[bin]]`
- [x] README section documenting the tool and the dump format
- [x] Gates: `cargo test` **337** pass / 0 fail · `cargo clippy --all-targets -- -D warnings` exit 0 · `cargo fmt --check` exit 0 · `cargo build --release` clean
- [x] Independent XWB validator replay, controlled against a stock bank
- [x] A6's stock comparison exercised for real against the install's `se_normal.xwb`

## Files changed (`ddr-chart-tools`, uncommitted)

| File | Change |
|---|---|
| `src/job/se_bank.rs` | **new** — generator, `SeBankOutput`, `SeBankError`, format constants, 14 tests |
| `src/xwb/dump.rs` | **new** — `describe()`, the documented `key=value` dump format, 6 tests |
| `src/bin/se_bank.rs` | **new** — the `ddr-se-bank` binary (`generate` / `dump`) |
| `src/job/mod.rs` | `pub mod se_bank;` |
| `src/xwb/mod.rs` | `pub mod dump;` |
| `src/error.rs` | `SeBank(#[from] …)` variant |
| `Cargo.toml` | second `[[bin]]` + `default-run` |
| `README.md` | `### Sound-effect bank pairs (ddr-se-bank)` |

## TDD cycles

### Cycle 1 — `xwb::dump`
Wrote the six tests first (stability, key shape, documented bank/segment keys, documented entry
keys, a foreign *streaming* two-entry bank, non-bank rejection). Red with
`E0432: unresolved import ...dump::describe`. Implemented `describe(&[u8])`. One failure, and it
was mine: I hand-computed a decimal `build_time` wrong. Fixed the test to derive it from the hex
literal rather than restate it. 6/6 green.

### Cycle 2 — `job::se_bank`
Wrote the fourteen tests first, covering A1–A7. Red with 50 compile errors (`generate`,
`SeBankError`, `SAMPLES_PER_BLOCK`, `STOCK_XWB_ENV` all absent). Implemented `generate` with every
container field written from a named constant, plus the ordering that makes a partial write
impossible: build the sound bank first (so the name is validated before any audio work or any
filesystem touch), then decode → validate → encode → assemble → write both. 14/14 green, with A6
skipping cleanly when `DDR_STOCK_XWB` is unset.

### Cycle 3 — the binary, README, gates
`ddr-se-bank` with `generate`/`dump` subcommands (safe here: new binary, no existing invocation to
break), same exit-code scheme as `main.rs`. README section. All gates green.

## Verification beyond the unit tests

### End-to-end on the real asset

`ddr-se-bank generate --input clap.ogg --name asti` →
**`asti.xwb` 5,416 B (74 blocks)** and **`asti.xsb` 262 B**, `duration=9472`, segment 4 at 236
running exactly to EOF. 9,472 − 9,423 source samples = 49 samples (1.1 ms) of trailing block
padding, matching the research note's prediction exactly.

Note 5,416 ≠ the research note's predicted 5,576: that figure includes the two-entry shape's silent
stub entry (70 B) and its second 64-byte name slot. The single-entry design amendment accounts for
the whole difference.

**Reproducibility confirmed empirically**, not just asserted: debug build and release build, two
separate runs, byte-identical output.
`asti.xwb` `sha256:46e6602892dc681c…`, `asti.xsb` `sha256:d2fe533ea65e03e6…`.

### Independent replay of the engine's wave-bank acceptance path

Transcribed `FUN_0042b310` (IsHeaderValid), `FUN_00418f18`'s TYPE_BUFFER gate and `FUN_0040f120`
(the 20-rule structural validator, plus the per-entry rules E1–E7 and §3.4's flag/segment
consistency trap) into a throwaway checker (`/tmp/xwb_validate.py`, not committed).

| Bank | Result |
|---|---|
| **stock `se_normal.xwb`** (17,740,212 B, 138 entries — extracted from the install's `se_normal.arc`, which is stored raw) | **PASS** — the control |
| generated `asti.xwb` (5,416 B) | **PASS** |

Shown to be discriminating — six single-byte/length corruptions, each tripping exactly the right
rule and nothing spurious:

| Corruption | Rule tripped |
|---|---|
| `dwHeaderVersion` 42 → 43 | H2 |
| flags bit 0 set (⇒ streaming) | the TYPE_BUFFER gate (+ rule 12) |
| `dwAlignment` → 2 | rule 11 |
| `dwEntryNameElementSize` → 32 | rule 10 |
| overwrite `szBankName` byte 63 | rule 9 |
| truncate the file by one byte | rule 20 |

### A6 exercised for real

Ran the stock-shape comparison with `DDR_STOCK_XWB=/tmp/…/se_normal.xwb`: **passes**. So the
generated bank's `version`, `flags`, `alignment`, `entry_name_element_size`, `header_version` and
`entry_metadata_element_size` are all byte-equal to the game's own in-memory gameplay-SE bank, with
only `name` and `entry_count` differing.

For task 03: `se_normal.arc` is a **raw**-stored single-entry ARC — payload at offset `0x40`,
length from the header at `+0x18` — so extracting `se_normal.xwb` in shell needs nothing but a
seek and a read. Do **not** reach for `se_system.arc`; that one is AVSLZ-compressed.

## Findings for the maintainer

### F1 — ADPCM encode quality on the clap: fine, but ~5 dB is being left on the table

The research note said to treat ≤ 17.6 dB SNR as a red flag. The real clap measures **17.36 dB**,
so I investigated rather than dismissing it. Same source, same pipeline, measured three ways:

| Variant | SNR |
|---|---|
| **as shipped** (7-predictor search, truncating quantizer) | **17.36 dB** |
| predictor search disabled (predictor 0 only) | 16.59 dB |
| rounding rather than truncating quantizer | **22.49 dB** |

Conclusions:

1. **The encoder is not broken.** The predictor search is working — disabling it costs 0.77 dB. The
   note's "expect materially better than 17.6 dB" bar was calibrated against *its own* reference
   implementation, not this crate's; on a like-for-like basis this crate's naive equivalent is
   16.59 dB and its actual output beats it.
2. **17 dB on a broadband percussive transient is serviceable**, and it is the same codec at the
   same settings as every one of the game's own 138 gameplay sound effects.
3. **`adpcm::encode`'s quantizer truncates where it should round** (`error / delta` instead of
   rounding to nearest), which costs about 5 dB. That is a genuine, one-line improvement — and it
   is **deliberately not taken here**, because `adpcm::encode` is shared with the song-conversion
   path, so changing it would change the audio bytes of every song this tool has ever converted.
   That is the maintainer's call, not this task's. If taken, the committed assist-tick bank must be
   regenerated.

The measurements are recorded in a comment at the assertion site in `se_bank.rs` so the numbers are
where someone would look for them.

### F2 — the unit test's SNR threshold was recalibrated

It originally asserted `> 17.6 dB` (borrowed from the research note). That is nearly meaningless
against this test's *tonal* synthetic input, which measures **47.35 dB**. Now `> 30 dB`, which is a
real tripwire for the material actually used, well clear of the research note's `< 6 dB` hard-fail
line, and with F1's real-world numbers recorded alongside.

### F3 — `cargo run` briefly became ambiguous

Adding a second `[[bin]]` made `cargo run` fail with "available binaries: …", which would have
broken `CLAUDE.md`'s documented `cargo run -- --help`. My plan predicted Cargo would resolve it via
the package name; it does not. Fixed with `default-run = "ddr-chart-tools"` and verified
(`cargo run -- --version` → `ddr-chart-tools 0.1.0`).

## Deviations

### D1 — Input is Ogg Vorbis only, and the reproducible path does not run ffmpeg

Task 02's requirement 2 and task 03's requirement 3 together imply the generator might take
ffmpeg's 16-bit-PCM output. It does not: it accepts Ogg Vorbis and *rejects* non-mono/non-44100
rather than converting.

The forcing argument is reproducibility. Task 02's requirement 5 and task 03's requirement 9 both
demand byte-identical output across runs and machines. An `ffmpeg -c:a libvorbis` re-encode makes
the bytes a function of the host's libvorbis build, so the committed asset could not be reproduced
on a clean checkout elsewhere. Research §8.1 already established no transcode is needed — the clap
is *already* mono 44.1 kHz — so the deterministic path feeds the committed `.ogg` straight through.
ffmpeg's role in task 03 is therefore preparing or inspecting a *non-conforming* source, outside
the reproducible path. Full reasoning in `context.md`.

Adding a WAV reader was considered and rejected: it needs either a new dependency (which this repo
requires a design doc to justify) or format-parsing code in a module `structure.md` says does not
own it. Clean additive follow-up if wanted.

### D2 — Exposed as a second binary rather than a subcommand

Requirement 1 left this to the implementer. A subcommand on the existing CLI would mean making the
required `--from-format`/`--to-format` optional and reworking `Cli::validate`/`into_jobs` plus 11
tests — a large blast radius on a working CLI, for an unrelated feature. A committed example gives
no installable executable, and task 03's requirement 4 wants a `PATH` fallback. `Cargo.toml` already
declared its `[[bin]]` explicitly, so a second one cost two lines. Alternatives and rationale in
`context.md` → *Design decisions*.

`src/bin/` is not in `.spec/steering/structure.md`'s documented layout. It is a Cargo convention
directory inside `src/`, not a new top-level module, so it does not trip that document's
prohibition — but it is an addition the maintainer may want to record there.

### D3 — Stock-bank comparison is opt-in via `DDR_STOCK_XWB`

Task 02's own dependencies section requires this test to skip when the stock banks are unavailable.
Extraction from the install's ARC containers needs an ARC/AVSLZ reader, which lives in the modpack,
not here — so the test consumes an already-extracted `.xwb` pointed at by the variable, and skips
with an explanatory message otherwise. It **was** run against the real stock bank (see above), so
the skip path is a convenience, not an untested branch.

### D4 — Not committed

As task 01. Both repositories left clean and green.

## Notes for task 03

- Committed-asset sizes to assert: **`tick.xwb` 5,416 B**, **`tick.xsb` 262 B** — the design's
  "roughly 5.5 KB and 330 B" is right for the wave bank and stale for the sound bank (330 B was the
  *song*-profile figure).
- Bank/cue name per the design is `asti`; the *files* are named `tick.xwb` / `tick.xsb`. The
  generator names its output `<name>.xwb` / `<name>.xsb`, so the script must **rename**
  `asti.*` → `tick.*` after generating, or pass `--name tick`. Note the name is the **cue** name
  the DLL will play, so changing it to `tick` changes what Step 2's `play_cue` must pass. Flag this
  to the maintainer rather than choosing silently.
- Discovery for requirement 4: `DDR_CHART_TOOLS` / `--chart-tools` should accept either the repo
  checkout (then `cargo run --release --manifest-path …/Cargo.toml --bin ddr-se-bank --`) or a
  directory containing the built binary; plus `ddr-se-bank` on `PATH`.
- The container-validator assertions requirement 5 wants map onto these dump keys:
  `segment0.offset`/`.length`, `segment1.offset`/`.length`, `segment2.length`, `segment3.offset`/
  `.length`, `file.length` vs `segment4.offset`/`.length`, `bank.entry_name_element_size`,
  `bank.alignment`, `bank.name_terminated`, `entry0.name_terminated`, `entry0.entry_flags`,
  and `bank.type` (must be `buffer`).
