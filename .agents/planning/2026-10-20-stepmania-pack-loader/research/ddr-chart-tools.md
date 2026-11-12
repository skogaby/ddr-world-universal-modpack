# Research: ddr-chart-tools (sibling checkout)

Survey of the `ddr-chart-tools` repository (git HEAD `a733404`) as a source for the
runtime SM → DDR conversion. Paths below are relative to **that** repository's root.

## Bottom line

- A pure-Rust format library plus CLI, **not** a musicdb/asset generator. It produces
  `.ssq`, `.xwb`, `.xsb` for one song and nothing else (no `musicdb.xml`, jackets,
  thumbnails, titles). Those are explicitly out of scope (`README.md`,
  `.spec/steering/product.md`).
- Parsers and writers are reusable in memory: `ssc::parse(&str)`, `sm::parse(&str)`,
  `ogg::decode::decode(&[u8])`, `ssq::writer::write(..)`, `xwb::write`, `xsb::write`,
  `adpcm::encode::encode`.
- The end-to-end glue is **private** in `src/job/mod.rs` (`sm5_to_ddr`,
  `synthesize_events`, `chart_end_tick`, `write_ddr_audio`, `build_xwb_bank`,
  `slice_preview`, `ddr_wave_format`, `validate_ddr_audio`). It must be lifted or
  reimplemented.
- **Vendoring/porting beats a path dependency**: no feature flags; a dependency drags in
  `clap`, `anyhow`, `env_logger`, `vorbis_rs` (C libvorbis via `cc` — breaks the
  cargo-xwin and Win7 `-Z build-std` builds) and `rustfft`. Light `crate::` coupling
  everywhere except `src/job/`. Licence is ambiguous (`Cargo.toml` says MIT OR Apache-2.0,
  `LICENSE` is Apache-2.0 text) — same maintainer, so moot.
- ~534 `#[test]`s, all synthetic fixtures (`tests/common/mod.rs`).

## Vendoring recipe (inferred from the `use crate::` graph; verify against `mod` lines)

| Concern | Files |
|---|---|
| SM/SSC parse | `src/ssc/{mod,notes,msd}.rs`, `src/sm/mod.rs`, `src/model/` |
| SSQ write | `src/ssq/{mod,writer,events,chunk,tempo,steps,mines,auxiliary}.rs`, `src/util/io.rs` |
| Audio | `src/xwb/{container,dump,mod}.rs`, `src/xwb/adpcm/{encode,decode,mod}.rs`, `src/xsb/mod.rs`, `src/ogg/decode.rs` (lewton) |
| Skip | `src/ogg/encode.rs` (C dep), `src/sync/` (rustfft), legacy/Hudson/Ultramix readers |

## StepMania parsing

- `.ssc` (`src/ssc/mod.rs::parse`), `.sm` (`src/sm/mod.rs::parse`, reuses
  `ssc::apply_song_tag`). **No `.dwi`.** MSD tokenizer `src/ssc/msd.rs::tokenize`
  handles `//` comments, backslash escapes, and recovers a missing `;`.
- Note symbols (`src/ssc/notes.rs::decode_row`): `1` tap; `2`/`3` hold (length = tail −
  head); `4` roll → **treated as hold**; `M` mine — full row (`MMMM`, `MMMMMMMM`,
  `MMMM0000`, `0000MMMM`) → `Shock{side}`, partial row → `Mine` mask
  (`classify_mine_row`); `F` and `L` dropped silently. A second `2` on a panel before its
  `3` silently overwrites the open head.
- Row beat is exact rational; `merge_coincident_notes` folds same-beat notes.
- `#STEPSTYPE`: only `dance-single`, `dance-double`. `#DIFFICULTY`: SM5 canonical names
  only (Beginner/Easy/Medium/Hard/Challenge; Edit skipped with warning). **`#METER`,
  `#RADARVALUES`, `#CHARTNAME`, `#CREDIT` ignored — meter never reaches the model.**
- Timing: `#BPMS` (BPM ≤ 0 is a hard error), `#STOPS` (seconds), `#OFFSET` (sign flipped
  to DDR convention, `sm_offset_to_model`). `#DELAYS`, `#WARPS` → warning, dropped.
  Per-chart `#OFFSET/#BPMS/#STOPS/#DELAYS/#WARPS` inside `#NOTEDATA` → warning, dropped
  (song timing used). `#SAMPLESTART`/`#SAMPLELENGTH` → `Song::preview`, default 30 s /
  10 s. Ignored: `#BANNER`, `#BACKGROUND`, `#JACKET`, `#CDTITLE`, `#SUBTITLE`,
  `#DISPLAYBPM`, `#SELECTABLE`, **`#MUSIC`** (audio is paired by basename instead).
- Whole-file failures (not per-chart): unsupported `#STEPSTYPE` inside `#NOTEDATA`
  (the `#NOTES` path merely warns — inconsistent), unknown difficulty name, unclosed
  hold / orphan tail, bad row length or unknown note char (keysound `[n]`, attack `{…}`,
  `K`), negative/zero BPM. Text read via `read_to_string` (UTF-8 only).

## SSQ writer (`src/ssq/writer.rs::write`)

- TPS 1000 only (`SsqError::CannotWriteTps`); 1024 ticks/beat, 4096/measure; triplets
  tick-rounded. Chunk order: type 1 tempo, type 2 events, type 3 per chart, type 20
  (mines, custom — only the modpack's `note-types-expansion` reads it) per chart,
  zero-length terminator.
- Tempo anchors synthesized from segments + stops in `i128` fixed point (10⁻⁶ ms); a
  stop = two anchors at one measure tick; BPM change + stop on one beat share an anchor;
  a trailing anchor is always added at chart end; `tempo_data[0] = −#OFFSET × TPS`.
- Events (private `synthesize_events`): `(0,1,4)`, `(0,2,1)` song start, `(4096,2,2)`
  chart start, `(4096,2,5)`, `(end−4096,2,3)` FINISH, `(end,2,4)` END, where `end` =
  last note incl. freeze end rounded up to a measure + 2 measures (`chart_end_tick`).
  **FINISH/END must be bracketed by tempo anchors** or the game locks at READY.
- Steps: `param2` = style byte (`0x14` single, `0x18` double) | slot (Beginner 4, Basic 1,
  Difficult 2, Expert 3, Challenge 6). Step byte: `0x00` freeze-end, `0xFF`/`0x0F`/`0xF0`
  shock, else panel mask — **so a single-side quad (`0x0F`) is misread as a shock**.
  Taps + hold heads on one tick OR into one byte; freeze block lists held panels.

## Audio

- Decode: `lewton` Vorbis only (`src/ogg/decode.rs`). Accepts stereo at 32/44.1/48 kHz
  only (`validate_ddr_audio`); mono/other rates rejected, not converted. Rate is written
  per entry and the engine resamples (confirmed RE note in `.spec/steering/tech.md`).
- Encode: in-house MS-ADPCM (`src/xwb/adpcm/encode.rs`), exhaustive 7-predictor search
  per block. Pure, deterministic, no external tools.
- XWB (`src/xwb/container.rs`, `build_xwb_bank`): version 43 / header 42, flags
  `0x0009_0001`, alignment 2048, bank name `{code}`, entries **preview first**
  (`{code}_s`) then main (`{code}`). Preview is cut from the main audio (hard cut, no
  fades; 0-byte if start is past the end).
- XSB (`src/xsb/mod.rs::write(code)`, `docs/xsb_format.md`): 326 B for a 4-char code;
  one wavebank, cues `{code}` (simple, category 4) and `{code}_s` (complex looping,
  category 3); CRC-16 over `[0x12..]` at `0x08`, **silently enforced by the engine**.
  Code rule 1–16 chars lowercase alnum/underscore; cue names match the basename
  byte-exactly or the song plays silently.
- Audio is never shifted; only the chart moves (`src/job/sync_offset.rs`).

## What it does not do (the runtime mod must)

musicdb entry synthesis, identity (mcode/basename) allocation, meter → `diffLv`,
jacket/thumbnail art, `#MUSIC` resolution, encoding detection, MP3/WAV/FLAC decode,
mono→stereo / resampling, delays/warps/per-chart timing, lifts, quad splitting,
tolerant parsing, panic containment, preview fades, lead-in normalisation.
