# Plan — SE Bank-Pair Generator (Step 1, task 02)

**Status: Approved 2026-07-26** — inherited from the verified upstream approval chain
(`context.md`). No design decision is changed here; the two judgement calls the task file
delegates to the implementer (how the entry point is exposed, and which input containers are
accepted) are recorded in `context.md` → *Design decisions* with their alternatives.

## Files

| File | Change |
|---|---|
| `src/job/se_bank.rs` | **new** — the generator: `generate()`, `SeBankOutput`, `SeBankError`, the format constants |
| `src/job/mod.rs` | `pub mod se_bank;` |
| `src/xwb/dump.rs` | **new** — `describe(&[u8]) -> Result<String, XwbError>`, the documented dump format |
| `src/xwb/mod.rs` | `pub mod dump;` |
| `src/bin/se_bank.rs` | **new** — the `ddr-se-bank` binary: `generate` and `dump` subcommands |
| `Cargo.toml` | second `[[bin]]` |
| `src/error.rs` | one wrapping variant for `SeBankError` |
| `README.md` | a section documenting the tool and the dump format |

## Implementation approach

### `job::se_bank`

```rust
pub struct SeBankOutput {
    pub xwb_path: PathBuf, pub xsb_path: PathBuf,
    pub xwb_len: usize,    pub xsb_len: usize,
    pub source_samples: usize, pub blocks: usize, pub total_samples: u32,
}

pub fn generate(input: &Path, name: &str, out_dir: &Path) -> Result<SeBankOutput, Error>;
```

Order of operations, chosen so nothing half-written can reach disk:

1. Build the **sound bank** bytes first — `xsb::write_se` validates the name, so a bad name fails
   before any audio work and before any file is created.
2. Read + `ogg::decode` the input.
3. Reject non-mono / non-44100 / empty (`SeBankError`), naming what was found.
4. `adpcm::encode` at the fixed SE `WaveFormat`; derive `blocks` and `total_samples`.
5. Assemble the `XwbBank` with every field written **explicitly** from a named constant — no
   `Default`, nothing inherited from the song path (whose flags are `0x0009_0001`/align 2048,
   i.e. *streaming*, which `CreateInMemoryWaveBank` rejects outright).
6. `xwb::write` into a `Vec`.
7. `create_dir_all`, then write both files.

### `xwb::dump`

`describe(bytes) -> Result<String, XwbError>` parses via `container::parse` (which validates
segment bounds), then additionally reads from the raw header the two things the parsed `XwbBank`
discards but task 03's validator assertions need: the five segment descriptors, and
`entry_metadata_element_size`; plus each entry's `data_offset` out of segment 1.

**The dump format is an interface** (R6). One `key=value` per line, `\n`-terminated, no blank
lines, keys sorted into a fixed order, so `grep '^bank.alignment=' | cut -d= -f2` is a stable
contract:

```
file.length=<bytes>
bank.name=<string>
bank.version=<u32>
bank.header_version=<u32>
bank.flags=0x%08X
bank.type=buffer|streaming
bank.has_entry_names=0|1
bank.entry_count=<u32>
bank.entry_metadata_element_size=<u32>
bank.entry_name_element_size=<u32>
bank.alignment=<u32>
bank.compact_format=<u32>
bank.build_time=<u64>
segment<N>.offset=<u32>          # N = 0..4
segment<N>.length=<u32>
entry<N>.name=<string>
entry<N>.codec / .channels / .sample_rate / .block_align_raw / .block_align
entry<N>.samples_per_block / .bits_per_sample_flag
entry<N>.entry_flags / .duration
entry<N>.data_offset / .data_length
entry<N>.loop_start / .loop_length
entry<N>.name_terminated=0|1     # validator rule 18: byte 63 of the 64-byte name field
```

`entry_flags` and `duration` are the two halves of `flags_and_duration` (low nibble / `>> 4`),
split because rule E1 constrains the nibble and rule E7 constrains the duration, and asserting a
combined hex word in shell would be unreadable.

`bank.type` is derived from flags bit 0 — the single most important field in the whole dump, since
it is a hard gate in **both** directions (`CreateInMemoryWaveBank` requires it clear; the
file-backed path requires it set).

### `ddr-se-bank`

Thin: parse, init logging the same way `main.rs` does, dispatch, map to the same exit codes
(`0` ok / `1` file error / `2` CLI error).

```
ddr-se-bank generate --input <FILE> --name <NAME> --out-dir <DIR>
ddr-se-bank dump <BANK.xwb>
```

Subcommands are safe here because the binary is new — no existing invocation to break.

## Test scenarios

Unit tests beside the code (`src/job/se_bank.rs`, `src/xwb/dump.rs`), following the repo's
conventions: scenario-named, `tempfile` for filesystem work, `?`/`Result` rather than `unwrap`
where a failure is a real mode.

Test input is **synthesized**: a mono 44100 Hz decaying burst, encoded with the crate's own
`ogg::encode`. Helper `synth_ogg(dir, frames, channels, rate) -> PathBuf`.

| # | Scenario | Expected | AC |
|---|---|---|---|
| S1 | Generate from a synthesized mono/44100 Ogg | `Ok`; both files exist; the XWB re-parses through `container::parse` with 1 entry; the XSB is exactly `0x101 + name.len() + 1` bytes | A1 |
| S2a | Container conforms — bank level | version 43, header_version **42**, flags **`0x00090000`**, bit 0 clear, alignment **4**, entry_name_element_size **64**, entry_count 1, compact_format 0, build_time 0 | A2 |
| S2b | Container conforms — segments | `seg0 == (0x34, 0x60)`; `seg1 == (0x94, 24)`; `seg2.length == 0`; `seg3.offset == 0x94 + 1*24` and `seg3.length == 1*64`; **`file_len − seg4.offset == seg4.length`** | A2 |
| S2c | Container conforms — entry | `entry_flags & 7 == 0`; `duration >= loop_start + loop_length`; codec 2, channels 1, rate 44100, `block_align_raw` 48 ⇒ `block_align` 70, `samples_per_block` 128; `data_length % 70 == 0`; byte 63 of the 64-byte name field is `0`; byte 63 of the bank-name field is `0` | A2 |
| S3 | Audio survives the encode | decode the entry back with `adpcm::decode`: sample count `== blocks * 128` and within one block of the source; not all-zero; no `i16::MIN`/`MAX` clipping run; SNR **> 17.6 dB** (the naive fixed-predictor floor the research measured — at or below it means the 7-predictor search regressed) | A3 |
| S4 | Wave-bank and sound-bank names agree | the XWB's 64-byte name field equals the XSB's wave-bank-name field at `0x8A..0xCA`, byte for byte, with a **mixed-case** name | A4 |
| S5 | Deterministic | generate twice into two temp dirs from the same input and name ⇒ both `.xwb` and both `.xsb` byte-identical | A5 |
| S6 | Matches stock shape | with `DDR_STOCK_XWB` set to a readable stock `.xwb`: `bank.version`, `bank.flags`, `bank.alignment`, `bank.entry_name_element_size` identical; `bank.name` and `bank.entry_count` allowed to differ. Unset/unreadable ⇒ **skip with an explanatory message**, never fail | A6 |
| S7a | Stereo input rejected | `SeBankError::NotMono { channels: 2 }`, message names the count | A7 |
| S7b | Non-44100 input rejected | `SeBankError::WrongSampleRate { .. }`, message names the rate | A7 |
| S7c | Missing file rejected | an I/O error, not a panic | A7 |
| S7d | Non-audio file rejected | an Ogg decode error, not a panic | A7 |
| S7e | Bad name rejected before anything is written | `XsbError::BadCode`, **and the output directory contains no files** | A7, R7 |
| S8 | Dump is stable | dumping the same bytes twice gives identical strings; every line matches `^[a-z0-9_.\[\]]+=` ; the documented keys are all present | A8 |
| S9 | Dump works on a foreign bank | dump a **streaming**-shaped bank (flags `0x00090001`, alignment 2048, two entries) built via `container::write` ⇒ `bank.type=streaming`, `entry1.*` present. Proves the dump is not specialised to our own output (a proxy for "must work on the game's own stock banks" that does not need the install) | A8, A6 |

## Risks

| Risk | Mitigation |
|---|---|
| Copying the song path's bank fields by habit (`flags 0x00090001`, align 2048) would produce a **streaming** bank that `CreateInMemoryWaveBank` rejects with `0x8AC70006` | S2a asserts flags and alignment explicitly; every field is written from a named constant, none inherited |
| A silently broken encode (all-zero, wrong predictor, wrong nibble packing) passes every container check | S3 decodes back and measures SNR against the research note's measured floor |
| The dump format drifts and breaks task 03's shell assertions | S8 pins stability and the key set; the format is documented in the module doc **and** the README |
| Adding a second binary changes the default `cargo run` target | `Cargo.toml` already names the primary `[[bin]]` explicitly, and it keeps the package name, so `cargo run` still resolves to it. Verified as part of the gates |

## Out of scope

- WAV or any non-Ogg input container (rationale in `context.md`).
- ARC / AVSLZ extraction of stock banks — task 03's job.
- Anything in the modpack repository — task 03.
