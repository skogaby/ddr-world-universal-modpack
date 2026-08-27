# XACT Bank Format — XWB + XSB Synthesis for a Custom In-Memory Sound (assist-tick)

**Purpose.** De-risk the accepted design: synthesize our own XWB (wave bank) + XSB (sound bank)
offline, embed both with `include_bytes!`, and at runtime call
`IXACT2Engine::CreateInMemoryWaveBank` + `CreateSoundBank` on them, then play a cue by name.

**Targets analysed**

| Artefact | Identity |
|---|---|
| Engine binary | `contents/com/xactengine2_10.dll`, x64 PE, 413,104 bytes, **imported into Ghidra project `DDRWorld_Ghidra` as `xactengine2_10.dll`, image base `0x00400000`** |
| Game data | `.../CrossOver/Bottles/bemani/drive_c/ddr_world/contents/data/**` |
| Sibling tooling (read-only) | the sibling `ddr-chart-tools` repository |

> **Address convention.** Because Ghidra loaded `xactengine2_10.dll` at its preferred base
> `0x00400000`, every address in this document is both the file-relative RVA + `0x400000` **and**
> the literal Ghidra listing address. They coincide with the addresses cited in
> `ddr-chart-tools/docs/xsb_format.md`, so that document's function references were verified
> directly rather than re-derived. **[OBS]** `ghidra_get_current_program_info` →
> `"image_base":"00400000","language":"x86:LE:64:default"`.

> **Evidence discipline.** Every claim is **[OBS]** (read directly out of the disassembly /
> decompilation, or out of an on-disk game file) or **[INF]** (inferred — re-verify before
> depending on it). Nothing in this document is taken from `xact3.h`, MSDN, or FAudio and
> presented as observed. Where a public-header name is used for a slot the game never calls, the
> row is marked **[INF]**.

> **Analysis note (reproducibility).** Ghidra's auto-analysis only recovered 917 of ~1900
> functions in this DLL (it is a COM server with 5 exports; everything else is vtable-reachable).
> I rebuilt the function table from the x64 `.pdata` `RUNTIME_FUNCTION` array
> (`.pdata` = `0x460000..0x4659ff`, 1898 entries) and forced each function body to its `.pdata`
> range. Function count went 917 → 2173. **Anyone re-opening this program must do the same or
> the key functions below will show as undisassembled bytes.** The two scripts are trivial and
> are reproduced in [Appendix A](#appendix-a--ghidra-bootstrap).

---

## Contents

1. [Overview / bottom line](#1-overview--bottom-line)
2. [Stock in-memory bank anatomy](#2-stock-in-memory-bank-anatomy)
3. [Engine validation rules (XWB)](#3-engine-validation-rules-xwb)
4. [Codec support](#4-codec-support)
5. [XSB: validation, CRC, hash, and the sound/cue profile](#5-xsb-validation-crc-hash-and-the-soundcue-profile)
6. [Sound-bank ↔ wave-bank linking and creation order](#6-sound-bank--wave-bank-linking-and-creation-order)
7. [Vtable layouts](#7-vtable-layouts)
8. [Concrete generation recipe](#8-concrete-generation-recipe)
9. [Offline validation plan](#9-offline-validation-plan)
10. [Open questions](#10-open-questions)
11. [Appendix A — Ghidra bootstrap](#appendix-a--ghidra-bootstrap)

---

## 1. Overview / bottom line

The design is sound and the format is now fully pinned down. Headlines:

1. **Ship MS-ADPCM (codec 2), mono, 44100 Hz, `block_align_raw = 48`.** **[OBS]** *Every* wave
   entry in *every* DDR wave bank on disk is codec 2 — there is not one PCM entry anywhere
   (§2). **[OBS]** `se_system.xwb` — an in-memory (`CreateInMemoryWaveBank`) bank — contains
   **11 mono entries out of 13**, including `SYS_COIN`, `SYS_CARD` and `X_sys_OK1`, i.e. the
   coin-insert and card-read sounds. Mono + in-memory + ADPCM is therefore the *shipping game's
   own exercised path*, on this exact engine build, under this exact CrossOver bottle.
2. **Bank flags `0x00090000`, `dwAlignment = 4`, `dwEntryNameElementSize = 64`,
   `dwHeaderVersion = 42`.** All four are hard requirements or proven-stock values (§3).
   `CreateInMemoryWaveBank` **rejects** a bank with `WAVEBANK_TYPE_STREAMING` (flags bit 0) with
   `HRESULT 0x8AC70006` **[OBS]** `0x00418f18`.
3. **Create the wave bank first, then the sound bank** — although the engine tolerates either
   order, because `CreateSoundBank` provably does **not** resolve wave banks (it allocates a
   *zeroed* `IXACT2WaveBank*` array and returns) **[OBS]** `0x00417380`. Linking is **by
   name**, late (§6).
4. **`xsb::write` works unmodified** and produces a file the engine accepts — but it emits the
   *song* profile (category 4 / category 3 + RPC curve `0xF8`), not the *SE* profile. The
   gameplay SE bank `se_normal.xsb` uses **category 6** with a bare 12-byte simple sound and no
   RPC **[OBS]**. That's a behavioural difference (mix category / volume / ducking), not a
   validity one — see §5.5 and §8.3.
5. **The whole recipe was built and validated offline** before writing this document: a real
   5,576-byte `asti.xwb` was generated from `clap.ogg` and passed a faithful
   re-implementation of the engine's validator, which also passes both stock in-memory banks
   (§9).

Biggest remaining risk: the **mix category** the tick lands in, and the four unnamed
`IXACT2Cue` slots. Neither blocks implementation.

---

## 2. Stock in-memory bank anatomy

### 2.1 Getting the bytes out

**[OBS]** ARC container layout, read off the two files:

```
+0x00 u32 magic      = 0x19751120
+0x04 u32 version    = 1
+0x08 u32 entry_count
+0x0C u32 (unk)      = 2
then entry_count × 16 bytes: { u32 name_offset, u32 data_offset, u32 unpacked_size, u32 packed_size }
then the NUL-terminated path strings, then the payloads.
`packed_size == unpacked_size` ⇒ stored raw; otherwise AVSLZ-compressed.
```

| ARC | entries | payload | unpacked | packed | stored |
|---|---|---|---|---|---|
| `data/arc/se_normal.arc` (17,740,288 B) | 1 | `data/sound/win/se_normal.xwb` @ `0x40` | 17,740,212 | 17,740,212 | **raw** |
| `data/arc/se_system.arc` (328,448 B) | 1 | `data/sound/win/se_system.xwb` @ `0x40` | 671,900 | 328,360 | **AVSLZ-compressed** |
| `data/arc/soundbanks.arc` (15,424 B) | 4 | 4 × `.xsb` | — | — | mixed (se_system.xsb raw, other 3 compressed) |

> ⚠️ **Correction to `game-sound-engine.md`.** That document says `se_system.arc` is "same deal"
> as `se_normal.arc` i.e. plain/uncompressed. **[OBS] it is not — it is AVSLZ-compressed**
> (`packed 328,360` vs `unpacked 671,900`; payload begins `5F 57 42 4E 44 2B 00 80` = an AVSLZ
> flag byte followed by the literal `WBND+`). Any Route-A LayeredFS work on `se_system.arc` must
> compress on the way out. `se_normal.arc` *is* raw as stated. Decompression uses this repo's own
> `src/services/avs_layeredfs/avslz.rs::decompress` (I ported it to Python verbatim to extract).

### 2.2 XWB header field offsets (confirmed against the engine, not guessed)

**[OBS]** derived from `0x00418f18` (`WaveBank::Initialize`), which computes its runtime
pointers as `base + *(u32*)(base + N)`:

| Offset | Field | Engine reference |
|---|---|---|
| `0x00` | `dwSignature` (`'WBND'` = `0x444E4257`) | **[OBS]** wb vtable `+0x98` = `0x00419290`: `MOV RAX,[RCX+0x228]; MOV EAX,[RAX]` |
| `0x04` | `dwVersion` | **[OBS]** wb vtable `+0x88` = `0x00419250`: `MOV EAX,[RAX+0x4]` — **read but its value is never tested** (see §3.2) |
| `0x08` | `dwHeaderVersion` | **[OBS]** wb vtable `+0x90` = `0x00419270`: `MOV EAX,[RAX+0x8]` — **must be 42** |
| `0x0C/0x10` | `Segments[0]` = BANKDATA `{off,len}` | **[OBS]** `0x00418f18`: `param_1[0x46] = *(u32*)(base+0xc) + base` |
| `0x14/0x18` | `Segments[1]` = ENTRYMETADATA | **[OBS]** `param_1[0x42] = *(u32*)(base+0x14) + base` |
| `0x1C/0x20` | `Segments[2]` = SEEKTABLES | **[OBS]** `param_1[0x47] = *(u32*)(base+0x1c) + base` |
| `0x24/0x28` | `Segments[3]` = ENTRYNAMES | **[OBS]** `param_1[0x43] = *(u32*)(base+0x24) + base`, gated on wb `+0x78` |
| `0x2C/0x30` | `Segments[4]` = ENTRYWAVEDATA | **[OBS]** `param_1[0x44] = *(u32*)(base+0x2c) + base` |

**[OBS]** BANKDATA (96 bytes, always at file offset `0x34`):

| Offset in bankdata | Absolute | Field | Engine reference |
|---|---|---|---|
| `+0x00` | `0x34` | `dwFlags` | **[OBS]** wb `+0xa8` = `0x00418ee0` returns `*(u32*)(bankdata)`; masked `& 0xFFF0FFFE` at `0x0040f120` |
| `+0x04` | `0x38` | `dwEntryCount` | **[OBS]** wb `+0x70` = `0x004192a0` returns `*(u32*)(bankdata+4)` |
| `+0x08` | `0x3C` | `szBankName[64]` | **[OBS]** wb `+0x68` = `0x00419230`: `MOV RAX,[RCX+0x230]; ADD RAX,0x8` — **this accessor is the wave-bank name the sound bank matches against** |
| `+0x48` | `0x7C` | `dwEntryMetaDataElementSize` | **[OBS]** `0x0040f120` requires 24 (non-compact) / 4 (compact) |
| `+0x4C` | `0x80` | `dwEntryNameElementSize` | **[OBS]** `0x0040f120` requires **exactly 64** |
| `+0x50` | `0x84` | `dwAlignment` | **[OBS]** `0x0040f120` requires ≥ 4 (≥ 2048 if streaming) |
| `+0x54` | `0x88` | `CompactFormat` | **[OBS]** only consulted on the compact path (`0x0040f090`) |
| `+0x58` | `0x8C` | `BuildTime` (u64) | **[OBS]** wb `+0xb0` = `0x00418ef0` returns `bankdata+0x58`; **never validated** |

**[OBS]** WAVEBANKENTRY (24 bytes, non-compact), from the walk in `0x0040f120`:

```
+0x00 u32 dwFlagsAndDuration   (low 4 bits = flags, high 28 = Duration in samples)
+0x04 u32 Format               (packed WAVEBANKMINIWAVEFORMAT)
+0x08 u32 PlayRegion.dwOffset  (relative to Segments[4].dwOffset)
+0x0C u32 PlayRegion.dwLength
+0x10 u32 LoopRegion.dwStartSample
+0x14 u32 LoopRegion.dwTotalSamples
```

`Format` bit layout **[OBS]** from the validator's own extractions at `0x0040f120`
(`& 3`, `>> 2 & 7`, `& 0x7FFFE0`, `>> 0x17 & 0xFF`, sign bit) — identical to
`ddr-chart-tools/src/xwb/container.rs:78-123`:

```
bits [0:1]   codec        0=PCM, 1=XMA, 2=ADPCM, 3=(rejected)
bits [2:4]   nChannels    must be 1..6
bits [5:22]  nSamplesPerSec  must be != 0
bits [23:30] dwBlockAlign (raw)
bit  [31]    bits-per-sample flag   0 = 8-bit, 1 = 16-bit  (PCM only)
```

### 2.3 The two in-memory banks — header fields

**[OBS]** all values read out of the extracted payloads.

| Field | `se_normal.xwb` | `se_system.xwb` | `aaaa.xwb` (streaming, for contrast) |
|---|---|---|---|
| file bytes | 17,740,212 | 671,900 | 6,443,008 |
| `dwVersion` (`0x04`) | 43 | 43 | 43 |
| `dwHeaderVersion` (`0x08`) | **42** | **42** | **42** |
| `Segments[0]` BANKDATA | `0x34 / 0x60` | `0x34 / 0x60` | `0x34 / 0x60` |
| `Segments[1]` ENTRYMETADATA | `0x94 / 0xCF0` | `0x94 / 0x138` | `0x94 / 0x30` |
| `Segments[2]` SEEKTABLES | `0xD84 / 0x0` | `0x1CC / 0x0` | `0xC4 / 0x0` |
| `Segments[3]` ENTRYNAMES | `0xD84 / 0x2280` | `0x1CC / 0x340` | `0xC4 / 0x80` |
| `Segments[4]` ENTRYWAVEDATA | `0x3004 / 0x10E81B0` | `0x50C / 0xA3B90` | (2048-aligned) |
| `dwFlags` | **`0x00090000`** | **`0x00090000`** | **`0x00090001`** |
| → decoded | TYPE_BUFFER + ENTRYNAMES(bit16) + bit19 | same | **TYPE_STREAMING** + ENTRYNAMES + bit19 |
| `szBankName` | `"se_normal"` | `"SE_SYSTEM"` | `"aaaa"` |
| `dwEntryMetaDataElementSize` | 24 | 24 | 24 |
| `dwEntryNameElementSize` | **64** | **64** | **64** |
| `dwAlignment` | **4** | **4** | **2048** |
| `CompactFormat` | 0 | 0 | 0 |
| `BuildTime` | `0x01DCB2B0C6A6FDF7` | `0x01C9493C744BB5B6` | (real FILETIME) |
| `dwEntryCount` | 138 | 13 | 2 |

**[OBS]** the `_n` sibling variants have the identical shape:
`se_system_n.arc` → name `"SE_SYSTEM"`, flags `0x00090000`, align 4, 13 entries, codec {2:13},
channels {1:11, 2:2}. `se_normal_n.arc` → name **`"SE_NORMAL_n"`**, flags `0x00090000`, align 4,
**258 entries**, codec {2:258}, channels **{1:136, 2:122}**.
And the two loose streaming banks: `bgm_menu.xwb` name `"bgm_menu"` flags `0x00090001`;
`voice.xwb` name `"voice"` flags `0x00090001`.

**Key takeaway:** the *only* header difference between an in-memory bank and a streaming bank is
`dwFlags` bit 0 and `dwAlignment`. Everything else — including `dwEntryNameElementSize = 64`,
`Segments[2]` being zero-length, and `Segments[3]` sitting immediately after `Segments[1]` — is
constant across the whole shipping asset set.

### 2.4 `se_system.xwb` — all 13 entries (the mono, in-memory model to copy)

**[OBS]**. `blkAl` = `(baRaw+22)*ch`; `spb` = `((blkAl − 7·ch)·8)/(4·ch) + 2`; both derived, not
stored.

| idx | codec | ch | rate | baRaw | bps | blkAl | spb | dataOff | dataLen | blocks | Duration | loopStart | loopTotal | name |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 0 | 2 | **1** | 44109 | 48 | 0 | 70 | 128 | 0 | 56,000 | 800 | 102,401 | 0 | 102,400 | `sdtest_a3` |
| 1 | 2 | **1** | 44128 | 48 | 0 | 70 | 128 | 56,000 | 46,130 | 659 | 84,353 | 0 | 84,352 | `sdtest_b3` |
| 2 | 2 | **1** | 44098 | 48 | 0 | 70 | 128 | 102,132 | 46,900 | 670 | 85,761 | 0 | 85,760 | `sdtest_c3` |
| 3 | 2 | **1** | 44124 | 48 | 0 | 70 | 128 | 149,032 | 61,670 | 881 | 112,769 | 0 | 112,768 | `sdtest_c4` |
| 4 | 2 | **1** | 44102 | 48 | 0 | 70 | 128 | 210,704 | 48,440 | 692 | 88,577 | 0 | 88,576 | `sdtest_d3` |
| 5 | 2 | **1** | 44126 | 48 | 0 | 70 | 128 | 259,144 | 53,900 | 770 | 98,561 | 0 | 98,560 | `sdtest_e3` |
| 6 | 2 | **1** | 44077 | 48 | 0 | 70 | 128 | 313,044 | 49,700 | 710 | 90,881 | 0 | 90,880 | `sdtest_f3` |
| 7 | 2 | **1** | 44099 | 48 | 0 | 70 | 128 | 362,744 | 59,780 | 854 | 109,313 | 0 | 109,312 | `sdtest_g3` |
| 8 | 2 | 2 | 44095 | 48 | 0 | 140 | 128 | 422,524 | 96,739 | 690 | 88,193 | 0 | 88,192 | `sdtest_loud220` |
| 9 | 2 | 2 | 44095 | 48 | 0 | 140 | 128 | 519,264 | 96,739 | 690 | 88,193 | 0 | 88,192 | `sdtest_soft220` |
| 10 | 2 | **1** | 43949 | 48 | 0 | 70 | 128 | 616,004 | 7,980 | 114 | 14,593 | 0 | 14,592 | **`SYS_COIN`** |
| 11 | 2 | **1** | 44124 | 48 | 0 | 70 | 128 | 623,984 | 15,470 | 221 | 28,289 | 0 | 28,288 | **`X_sys_OK1`** |
| 12 | 2 | **1** | 44100 | 48 | 0 | 70 | 128 | 639,456 | 31,150 | 445 | **56,960** | 0 | **56,960** | **`SYS_CARD`** |

All 13 have `dwFlagsAndDuration & 0xF == 0` (entry flags nibble = 0) **[OBS]**.
All 13 are cue-addressable — `se_system.xsb` has 13 simple cues, one per wave, mapping
`SYS_COIN → waveIndex 10`, `X_sys_OK1 → 11`, `SYS_CARD → 12` **[OBS]** (§5.5). Those are
coin-insert / card-read sounds, so **mono ADPCM in an in-memory bank is unambiguously played by
the shipping game.**

### 2.5 `se_normal.xwb` — first 6 entries + aggregate

**[OBS]**

| idx | codec | ch | rate | baRaw | bps | dataOff | dataLen | `dwFlagsAndDuration` | Duration | loopStart | loopTotal | name |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 0 | 2 | 2 | 44101 | 48 | 0 | 0 | 191,659 | `0x002AB810` | 174,977 | 0 | 174,976 | `se_kansei_small` |
| 1 | 2 | 2 | 43667 | 48 | 0 | 191,660 | 3,919 | `0x0000D010` | 3,329 | 0 | 3,328 | `se_result_count_up` |
| 2 | 2 | 2 | 44084 | 48 | 0 | 195,580 | 85,819 | `0x00131810` | 78,209 | 0 | 78,208 | `se_result_eamusement_send` |
| 3 | 2 | 2 | 44117 | 48 | 0 | 281,400 | 145,039 | `0x00205010` | 132,353 | 0 | 132,352 | `se_result_in` |
| 4 | 2 | 2 | 44066 | 48 | 0 | 426,440 | 90,719 | `0x00143010` | 82,689 | 0 | 82,688 | `se_result_total_window_in` |
| 5 | 2 | 2 | 44100 | 48 | 0 | 517,160 | 181,159 | `0x00286010` | 165,377 | 0 | 165,376 | `se_result_total_window_out` |

Aggregate over all 138 entries **[OBS]**:

- `codec = {2: 138}` — **100 % MS-ADPCM, zero PCM**
- `channels = {2: 138}` — all stereo in *this* bank (mono lives in `se_system` / `se_normal_n`)
- `block_align_raw = {48: 138}`, `bits-per-sample flag = {0: 138}`
- 78 distinct sample rates from 43,359 to 44,382 — the authoring tool writes the *measured* rate
  per clip, not a canonical 44100. **⇒ arbitrary rates are accepted.**
- `LoopRegion.dwStartSample = {0}` for all 138; `dwTotalSamples % 128 == 0` for all 138
- `Duration − dwTotalSamples = {1: 126, 0: 5, 2: 7}` — **the relationship is not fixed**; it is
  `dwTotalSamples`, `+1`, or `+2` depending on entry. The engine only requires
  `Duration ≥ dwStartSample + dwTotalSamples` (§3.3), which all three satisfy.
- `dataLength mod blockAlign = {139: 126, 0: 5, 138: 7}` — i.e. **126 of 138 entries declare a
  `PlayRegion.dwLength` one byte short of a whole number of ADPCM blocks.** A Konami tool
  off-by-one that the engine tolerates because it only bounds-checks the region against
  `Segments[4].dwLength`. `se_system` is clean (all exact multiples). **⇒ do not imitate this;
  write exact multiples.**

### 2.6 Streaming song bank + its 326-byte XSB, for contrast

**[OBS]** `data/sound/win/dance/aaaa.xwb`: flags `0x00090001` (TYPE_STREAMING), align 2048,
bank name `"aaaa"`, 2 entries:

| idx | codec | ch | rate | baRaw | dataOff | dataLen | blocks | Duration | loopTotal | name |
|---|---|---|---|---|---|---|---|---|---|---|
| 0 | 2 | 2 | 47999 | 48 | 0 | 5,650,819 | 40,362 | 5,166,209 | 5,166,208 | `aaaa` (full song) |
| 1 | 2 | 2 | 48000 | 48 | 5,652,480 | 787,500 | 5,625 | 720,000 | 720,000 | `aaaa_s` (preview) |

Note `dataOff` of entry 1 = 5,652,480 = 2048-aligned **[OBS]** — the streaming alignment rule.
Songs are 48 kHz stereo; SEs are ~44.1 kHz.

**[OBS]** full byte-level decode of the stock `aaaa.xsb` (326 bytes), field by field:

```
0x00 'SDBK'    0x04 content_version=43   0x06 tool_version=43   0x08 crc=0xCB7F
0x0A timestamp=0x01CDEFAD3E0E4F23        0x12 flags=0x01
0x13 simple_cue_count=2    0x15 complex_cue_count=0   0x17 =0
0x19 total_cues(buckets)=16 0x1B wavebank_count=1     0x1C sound_count=2
0x1E cue_name_table_length=12            0x20 =0
0x22 simple_cue_offset=0x104   0x26 complex_cue_offset=-1   0x2A cue_name_offset=0x13A
0x2E =-1   0x32 variation=-1  0x36 transition=-1
0x3A wavebank_name_offset=0x8A  0x3E cue_hash_offset=0x10E
0x42 cue_name_index_offset=0x12E          0x46 sound_offset=0xCA
0x4A soundbank_name = "aaaa"   0x8A wavebank_name = "aaaa"
0xCA sound#0 COMPLEX (39 B): 05 03 00 B4 00 00 00 27 00 01 | 07 00 01 F8 00 00 00 |
                             B4 E0 00 00 00 01 01 00 00 20 00 00 FF 0C 01 00 00 FF 00 00 00 00
0xF1 sound#1 SIMPLE  (19 B): 04 04 00 B4 00 00 00 13 00 | 00 00 00 | 07 00 01 F8 00 00 00
0x104 cue0 = 04 CA 00 00 00   (→ sound 0xCA = COMPLEX/preview)
0x109 cue1 = 04 F1 00 00 00   (→ sound 0xF1 = SIMPLE/main)
0x10E hash[16]: bucket 1 = 1, bucket 14 = 0, rest 0xFFFF
0x12E nameIdx: {0x13A, 0xFFFF}, {0x141, 0xFFFF}
0x13A "aaaa_s\0aaaa\0"
```

Two facts fall straight out of this that matter for §8:

1. **In the stock file, the main track's `wave_index` is `0`** (bytes at `0xFA` = `00 00`), and
   the preview's is `1` (the byte at `0xE9` = `01`) — matching `aaaa.xwb` where entry 0 is the
   song and entry 1 the preview. **[OBS]**
2. **`ddr-chart-tools` uses the inverse convention** — `SIMPLE_SOUND_BYTES[9] = 0x01`
   (`src/xsb/mod.rs:219-222`, main → wave **index 1**) and its COMPLEX template byte at track
   offset 14 is `0x00` (`src/xsb/mod.rs:239`, preview → wave **index 0**). **[OBS]** So the
   sibling writer's XSB is self-consistent with an XWB whose entry 0 is the *preview* and entry 1
   the *main*. This is the single reason our XWB needs two entries (§8.1).

**Byte-plausibility of `xsb::write` against a real file: confirmed.** Every field the sibling
writer emits appears in the stock `aaaa.xsb` with the same value or the same *kind* of value, the
section order and all seven section offsets are identical, and the total size for a 4-char code
is 326 bytes in both **[OBS]**. Its CRC and its hash both reproduce the stock file exactly (§5.3,
§5.4).

---

## 3. Engine validation rules (XWB)

Three functions run, in this order, when `CreateInMemoryWaveBank` is called.

### 3.1 `CreateInMemoryWaveBank` — entry conditions

**[OBS]** `FUN_00410d60` @ **`0x00410d60`** (engine vtable `+0x50`, see §7.1):

```c
HRESULT CreateInMemoryWaveBank(
    IXACT2Engine* this,            // RCX
    const void*   pvBuffer,        // RDX
    DWORD         dwSize,          // R8D
    DWORD         dwFlags,         // R9D
    DWORD         dwAllocAttributes,  // [rsp+0x28]
    IXACT2WaveBank** ppWaveBank);     // [rsp+0x30]
```

| Check | Failure HRESULT |
|---|---|
| `EnterCriticalSection(engine + 0x158)` | — |
| `GetCurrentThreadId() != *(int*)(engine + 0x70)` | **`0x8AC70012`** if equal |
| `*(int*)(engine + 200) != 0` (engine initialized) | `0x8AC70002` |
| `pvBuffer != NULL` | `0x80070057` (E_INVALIDARG) |
| `dwSize != 0` | `0x80070057` |
| `ppWaveBank != NULL` | `0x80070057` |
| allocate a 0x240-byte WaveBank object (`FUN_0041ff20(0x240)`) | `0x8007000E` |
| construct: `FUN_00418e20` (installs vtable `0x004035f0`) | — |
| **initialize: `FUN_00418f10(obj, pvBuffer, dwSize, dwFlags & 1, dwAllocAttributes)`** | see §3.2/§3.3 |
| link into the engine's wave-bank list at `engine+0xb0 / +0xc0`; `*ppWaveBank = obj` | — |

Two behavioural facts worth acting on:

- **`dwFlags` is masked to `& 1`** **[OBS]** — only one flag bit exists on this call; the game
  passes 0 and so should we.
- **The thread check.** `*(int*)(engine + 0x70)` is compared against the caller's TID and the call
  is rejected with `0x8AC70012` if they match. The same guard is on `CreateSoundBank`
  (`0x00410bc0`), `CreateStreamingWaveBank` (`0x00410f00`), `SoundBank::Prepare` (`0x004236e0`)
  and `SoundBank::Play` (`0x00423990`) **[OBS]**. **[INF]** `engine+0x70` holds the TID of the
  thread currently dispatching an XACT notification callback. ⇒ **never create banks or play cues
  from inside an XACT notification callback**; the game thread is fine (`gamemdx` plays from
  `judgeNotes` / `frame_main` there today).

### 3.2 Header identity check — `FUN_0042b310` @ `0x0042b310`

Called as WaveBank vtable `+0xd8` from `0x00418f18`. **[OBS]** verbatim:

```c
bool WaveBank::IsHeaderValid(this) {
    if (this->vt[0x98]() != 0x444E4257) return false;   // dwSignature @0x00 == 'WBND'
    if (this->vt[0x90]() != 0x2A)       return false;   // dwHeaderVersion @0x08 == 42
    this->vt[0x88]();                                    // dwVersion @0x04 — READ, RESULT DISCARDED
    return this->vt[0x70]() != 0;                        // dwEntryCount != 0
}
```
Failure ⇒ the caller returns **`0x8AC70007`** **[OBS]** `0x00418f18`.

⇒ **`dwHeaderVersion` (offset `0x08`) must be exactly 42. `dwVersion` (offset `0x04`) is NOT
validated here** — write 43 anyway to match every stock file (and because
`ddr-chart-tools`'s own parser rejects anything else, `src/xwb/container.rs:216-219`).

### 3.3 Structural validator — `FUN_0040f120` @ `0x0040f120`

`HRESULT validate(const void* pv /*param_1*/, DWORD cb /*param_2*/)`. This is the whole rule set,
transcribed from the decompilation. **All rows [OBS].**

**Error-code vocabulary** (all `0x8AC706xx`):

| Code | Meaning (by the checks that raise it) |
|---|---|
| `0x8AC70630` | header / segment sizing |
| `0x8AC70631` | entry `Format` field |
| `0x8AC70632` | BANKDATA field |
| `0x8AC70633` | entry region / duration |
| `0x8AC70634` | entry-name table not NUL-terminated |

**Phase 1 — header + BANKDATA**

| # | Rule | Fail code |
|---|---|---|
| 1 | `cb >= 0x34` | `0630` |
| 2 | **`Segments[0].dwOffset == 0x34` exactly** | `0630` |
| 3 | `cb − Segments[0].dwOffset >= 0x60` | `0630` |
| 4 | **`Segments[0].dwLength == 0x60` exactly** (BANKDATA is a fixed 96 bytes) | `0630` |
| 5 | **`(dwFlags & 0xFFF0FFFE) == 0`** ⇒ the only legal flag bits are `0x000F0001` | `0632` |
| 6a | if `dwFlags` bit 17 (`0x20000`, COMPACT) **clear**: `dwEntryMetaDataElementSize == 24` | `0632` |
| 6b | if bit 17 **set**: `dwFlags` bit 0 must **also** be set (COMPACT ⇒ STREAMING), `dwEntryMetaDataElementSize == 4`, and `CompactFormat` must pass `FUN_0040f090` | `0632` |
| 7 | `dwEntryCount != 0` | `0632` |
| 8 | `dwEntryCount * dwEntryMetaDataElementSize` must not wrap | `0632` |
| 9 | **byte 63 of `szBankName` (file offset `0x7B`) must be `0`** | `0632` |
| 10 | **`dwEntryNameElementSize == 0x40` (64) — unconditionally, even with no entry-name table** | `0632` |
| 11 | **`dwAlignment >= 4`** | `0632` |
| 12 | if `dwFlags` bit 0 (STREAMING): `dwAlignment >= 0x800` (2048) **and** `Segments[0].dwOffset <= 0x79F` | `0632` / `0630` |

Rule 5 is the observation that pins the flag layout: **bit 0 = type, bits 16..19 = flags,
everything else is rejected.** Bit 16 is proven to be ENTRYNAMES (rules 15–18 + the runtime
predicate in §3.4); bit 17 is proven to be COMPACT (rule 6b's 4-byte entries + compact-format
path). **[OBS]** Bits 18 and 19 are *allowed but never tested anywhere in this DLL* — stock banks
set bit 19 (`0x80000`). **[INF]** bit 19 = SEEKTABLES, bit 18 = SYNC_DISABLED (from the numeric
pattern only; unverifiable from this binary).

**Phase 2 — ENTRYMETADATA segment + per-entry**

| # | Rule | Fail code |
|---|---|---|
| 13 | **`Segments[1].dwOffset == 0x94` exactly** (= `0x34 + 0x60`) | `0630` |
| 14 | `cb − Segments[1].dwOffset >= dwEntryCount * dwEntryMetaDataElementSize` **and** `Segments[1].dwLength == dwEntryCount * dwEntryMetaDataElementSize` exactly | `0630` |
| | *for each of the `dwEntryCount` entries:* | |
| E1 | **`(dwFlagsAndDuration & 7) == 0`** — entry-flag bits 0,1,2 must be clear (bit 3 is permitted) | `0633` |
| E2 | `PlayRegion.dwOffset <= Segments[4].dwLength` and `Segments[4].dwLength − dwOffset >= dwLength` | `0633` |
| E3 | **`Format.codec < 3`** ⇒ codec 3 rejected outright | `0631` |
| E4 | **`1 <= Format.nChannels <= 6`** | `0631` |
| E5 | `Format.nSamplesPerSec != 0` (bits 5..22 non-zero) | `0631` |
| E6 | **if codec == 0 (PCM): `Format.dwBlockAlign == (bitsPerSample/8) * nChannels`** where `bitsPerSample = 16` if bit 31 else `8`. *No block-align validation at all for codec 1 or 2.* | `0631` |
| E7 | **`(dwFlagsAndDuration >> 4) >= LoopRegion.dwStartSample + LoopRegion.dwTotalSamples`** (and the sum must not wrap) | `0633` |

E7 is the *only* constraint on `Duration`. Nothing here requires `Duration` to relate to
`PlayRegion.dwLength`.

**Phase 3 — TYPE_BUFFER-only tail checks** (skipped entirely for streaming banks)

Reached only when `dwFlags & 1 == 0`:

| # | Rule | Fail code |
|---|---|---|
| 15 | if `dwFlags` bit 16 set: **`Segments[3].dwOffset == 0x94 + dwEntryCount*24`** — the cursor immediately after the entry-metadata array. **This forces `Segments[2]` (SEEKTABLES) to be zero-length and `Segments[3]` to start immediately after `Segments[1]`.** | `0630` |
| 16 | `cb − Segments[3].dwOffset >= dwEntryCount * 64` | `0630` |
| 17 | **`Segments[3].dwLength == dwEntryCount * 64` exactly** | `0630` |
| 18 | **byte 63 of every 64-byte entry name must be `0`** | `0634` |
| 19 | `cb >= Segments[4].dwOffset` | `0630` |
| 20 | **`cb − Segments[4].dwOffset == Segments[4].dwLength` exactly** — the wave-data segment must run *precisely* to the end of the buffer. No trailing bytes, no slack, no lying about the length. | `0630` |

Rule 20 is the easiest one to get wrong and it is fatal. Note the interaction with the ARC
container: **[OBS]** `se_normal.arc` declares an unpacked size of 17,740,212 while the `.arc` file
itself is 17,740,288 bytes (76 bytes of trailing padding). The `cb` handed to
`CreateInMemoryWaveBank` is the AVS file record's size, i.e. the declared 17,740,212 — and
`12,292 + 17,727,920 = 17,740,212` exactly. Our own path passes `include_bytes!(...).len()`, so
just make sure the blob has no padding.

Rule 15 is why the `ddr-chart-tools` writer's segment layout is *mandatory* rather than merely
conventional: `container::write` computes `seg2_off = seg1_off + seg1_len`, `seg2_len = 0`,
`seg3_off = seg2_off` (`src/xwb/container.rs:341-351`), which is exactly what rule 15 demands.

Rules 1–4 + 13 + 15 together mean **the entire header layout is rigid**: header 52 bytes,
BANKDATA at `0x34`+96, ENTRYMETADATA at `0x94`, SEEKTABLES empty, ENTRYNAMES immediately after,
wave data aligned to `dwAlignment` and running to EOF.

### 3.4 What the runtime object reads (and one flag/segment consistency trap)

**[OBS]** the WaveBank object's own accessors, disassembled:

| wb vtable | Address | Returns |
|---|---|---|
| `+0x68` | `0x00419230` | `bankdata + 8` — **the bank name string** |
| `+0x70` | `0x004192a0` | `*(u32*)(bankdata + 4)` — `dwEntryCount` |
| `+0x78` | `0x004192c0` | **`*(u32*)(header + 0x28) != 0`** — i.e. `Segments[3].dwLength != 0` |
| `+0x88` | `0x00419250` | `*(u32*)(header + 4)` — `dwVersion` |
| `+0x90` | `0x00419270` | `*(u32*)(header + 8)` — `dwHeaderVersion` |
| `+0x98` | `0x00419290` | `*(u32*)(header + 0)` — `dwSignature` |
| `+0xa8` | `0x00418ee0` | **`*(u32*)(bankdata + 0)`** — `dwFlags` (this is the STREAMING test) |
| `+0xb0` | `0x00418ef0` | `bankdata + 0x58` — `BuildTime` |
| `+0x58` | `0x00418ea0` | `GetEntry(u16 index, WAVEBANKENTRY* out)` — copies 24 bytes from `entrymeta + index*24` |
| `+0x60` | `0x00419200` | seek-table lookup for an entry (`*(u32*)(seektables + index*4)`, `-1` = none) |

⚠️ **Consistency trap [OBS]:** the *validator* (rule 15/17) gates the entry-name table on
**`dwFlags` bit 16**, but the *runtime* (`wb+0x78`) gates it on **`Segments[3].dwLength != 0`**.
These are two independent switches. Set them consistently — flag bit 16 set **and**
`Segments[3].dwLength == dwEntryCount*64` — which is what every stock bank does. (Setting bit 16
with a zero-length seg3 would pass the validator's `!= cursor` check only by accident and then
make the runtime think there are no names; setting a non-zero seg3 without bit 16 would leave
seg3 unvalidated but still consumed at runtime.)

**[OBS]** `FUN_0042b910` @ `0x0042b910`, called at the end of `WaveBank::Initialize`, sets
`*(u32*)(wavebank + 0x1F8) = 1` and posts notification type **`0x11`** through
`FUN_00411a00`. Notification `0x11` is one of the five `gamemdx` registers at `+0x1AAB60`
(`game-sound-engine.md` line 78) — i.e. **`WAVEBANKPREPARED` is signalled synchronously inside
`CreateInMemoryWaveBank`.** ⇒ **an in-memory wave bank is usable the instant the call returns; we
do not have to pump `DoWork` or wait for a notification.** (This is the opposite of a streaming
bank, whose header read is driven by `DoWork` in `FUN_00424af0`.)

### 3.5 The streaming/file-backed path, for completeness

**[OBS]** `FUN_00424af0` @ `0x00424af0` is the file-backed header state machine. Same identity
checks (`'WBND'`, header version 42), then additionally requires
`Segments[0].dwOffset < 0x7A0`, reads more of the file if the cached header is shorter than
`Segments[4].dwOffset`, and — crucially — **inverts the `wb+0xa8` test**: it errors with
`0x8AC70006` if `dwFlags & 1 == 0`. So:

> **[OBS] The bank-type bit is a hard gate in both directions.**
> `CreateInMemoryWaveBank` requires `WAVEBANK_TYPE_BUFFER` (bit 0 clear) — `0x00418f18`:
> `if (vt[0xa8]() & 1) return 0x8AC70006;`
> The file/streaming path requires `WAVEBANK_TYPE_STREAMING` (bit 0 set) — `0x00424af0`:
> `if ((vt[0xa8]() & 1) == 0) { hr = 0x8AC70006; }`

---

## 4. Codec support

### 4.1 What the validator accepts

**[OBS]** from rules E3–E6 in §3.3:

| codec | Accepted? | Block-align rule | Notes |
|---|---|---|---|
| 0 (PCM) | **yes** | **`dwBlockAlign == (bitsPerSample/8) * nChannels`, exactly** | `bitsPerSample` = 16 if `Format` bit 31 set, else 8. Mono 16-bit PCM ⇒ `dwBlockAlign` must be **2**; stereo 16-bit ⇒ **4**; mono 8-bit ⇒ **1**. |
| 1 (XMA) | **yes** | none | Xbox codec; no DDR asset uses it. |
| 2 (MS-ADPCM) | **yes** | **none whatsoever** | The validator never touches `dwBlockAlign` for codec 2. |
| 3 (WMA) | **no** — `0x8AC70631` | — | `if ((Format & 3) < 3)` else error. |

Channels: **`1..6` inclusive**; `0` and `>6` rejected with `0x8AC70631`. **Mono is explicitly
legal at the validator level.** **[OBS]**

8-bit vs 16-bit PCM: it *does* matter — bit 31 selects it and it feeds rule E6's exact
block-align equation. Get it wrong and the bank is rejected. **[OBS]**

### 4.2 What is actually exercised

**[OBS]** Across every wave bank in the install — `se_normal` (138), `se_normal_n` (258),
`se_system` (13), `se_system_n` (13), plus the song banks — **every single entry is codec 2**.
There is not one PCM or XMA entry anywhere in DDR World.

**[OBS]** Mono in an in-memory bank is exercised: `se_system.xwb` has 11 mono entries of 13, and
`se_normal_n.xwb` has 136 mono of 258. `SYS_COIN` / `SYS_CARD` / `X_sys_OK1` are mono,
in-memory, and cue-addressable (§5.5). This is the decisive evidence: the mono ADPCM in-memory
path is not a theoretical corner of the format, it is what the cabinet plays when you insert a
coin.

### 4.3 Negative result on the decoder location

**[OBS]** I searched the whole image for the standard MS-ADPCM tables and found **none**:

- coefficient pairs `(256,0)(512,−256)(0,0)(192,64)(240,0)(460,−208)(392,−232)` as int16 →
  no match
- adaptation table `230,230,230,230,307,409,512,614,768,614,512,409,307,230,230,230` as int16 →
  no match
- the distinctive int16 subsequences `F0 00 00 00 CC 01 30 FF` and `00 03 66 02 00 02 99 01` →
  no match

**[OBS]** the import table contains **no `msacm32`** (no `acmStreamOpen`/`acmStreamConvert`) and
no static `dsound` import — DirectSound is resolved dynamically (`LoadLibraryW` +
`GetProcAddress` are imported, and the DLL's only two audio-backend strings are `DirectSound`
and `DirectSoundEnumerateW`, per `game-sound-engine.md` line 45).

**[INF]** ⇒ ADPCM→PCM conversion is not performed by literal table lookup inside
`xactengine2_10.dll`; it is either delegated to the DirectSound/OS mixer via a non-PCM
`WAVEFORMATEX`, or computed without the canonical tables. **I did not determine which, and it
does not change the recommendation** — the format demonstrably plays, including under the
CrossOver bottle this project targets, because the game's own SEs are all ADPCM.

### 4.4 Recommendation

**Ship MS-ADPCM (codec 2), mono, 44100 Hz, `block_align_raw = 48`** (⇒ `dwBlockAlign` = 70 bytes,
128 samples/block), using `ddr-chart-tools`'s existing encoder.

Rationale: ADPCM is the 100 %-proven path on this engine build, this codec configuration
(`baRaw = 48`) is the only one the authoring tool ever emits, mono is proven on the in-memory
path, and `ddr-chart-tools/src/xwb/adpcm/encode.rs` already produces it. PCM is *structurally*
accepted (§4.1) and would let us skip a lossy encode — but **no DDR asset is PCM, so the PCM
playback path in this engine build is completely unexercised.** That is a gratuitous risk for a
0.2 s clap where ADPCM's ~4:1 compression is a bonus, not a cost. **Do not ship PCM.**

---

## 5. XSB: validation, CRC, hash, and the sound/cue profile

### 5.1 `CreateSoundBank` and `SoundBank::Initialize`

**[OBS]** `FUN_00410bc0` @ **`0x00410bc0`** (engine vtable `+0x48`) — identical shape to
`CreateInMemoryWaveBank`: same thread guard (`0x8AC70012`), same `engine+200` initialized check
(`0x8AC70002`), same `NULL`/zero-size `E_INVALIDARG`s, allocates a **0x210**-byte object,
constructs with `FUN_00417090` (installs vtable `0x00402ea0`), then calls
`FUN_00417380(obj, pv, cb, flags & 1, allocAttr)`.

**[OBS]** `FUN_00417380` @ `0x00417380` = `SoundBank::Initialize`, verbatim order:

```c
this->xsb   = pv;  this->cb = cb;
hr = FUN_004232c0(this, pv, cb);        // 1. magic + tool version
if (hr >= 0) {
    hr = FUN_00424200(this, pv, cb);    // 2. CRC-16
    if (hr >= 0) {
        ... optional engine hook ...
        hr = FUN_0040e970(pv, cb);      // 3. full structural validation
        if (hr < 0) hr = 0x8AC70007;
    }
}
// 4. allocate + ZERO an array of `wavebank_count` IXACT2WaveBank* at this+0x1E8
if (xsb[0x1B] != 0) {
    this[0x3d] = alloc(xsb[0x1B] * 8);
    for (i = 0; i < xsb[0x1B]; i++) this[0x3d][i] = NULL;   // <<-- nothing is resolved
}
if (hr < 0) return hr;
return FUN_00424370(this);              // 5. alloc + zero a (simple+complex)*4 instance table
```

Step 4 is the answer to the creation-order question (§6).

### 5.2 Magic / version — `FUN_004232c0` @ `0x004232c0`

**[OBS]** verbatim:

```c
if (cb > 0x89 && *(u32*)pv == 0x4B424453 /* 'SDBK' */) {
    return (*(short*)((char*)pv + 6) != 0x2B) ? 0x8AC70007 : 0;
}
return 0x8AC70007;
```

⇒ `cb >= 0x8A` (138), magic `'SDBK'`, and **`tool_version` at offset `0x06` must be 43 (`0x2B`)**.
**`content_version` at offset `0x04` is not checked here** — write 43 anyway to match stock.

### 5.3 CRC-16 — `FUN_00424200` @ `0x00424200` — **confirms the sibling doc exactly**

**[OBS]** verbatim decompilation:

```c
byte* p = (byte*)pv + 0x12;
u16 crc = 0xFFFF;
for (int i = cb - 0x12; i != 0; i--) {
    byte b = *p++;
    crc = *(u16*)(&TABLE_00404310 + ((b ^ (crc & 0xFF)) * 2)) ^ (crc >> 8);
}
if (*(u16*)((char*)pv + 8) != (u16)~crc) return 0x8AC70007;
return 0;
```

- **coverage = bytes `[0x12 .. cb)`** ✔ matches `docs/xsb_format.md` §CRC-16
- **stored at offset `0x08` as the bitwise NOT of the accumulator** ✔ matches
- **256-entry u16 table at `0x00404310`** ✔ matches the doc's "`FUN_00424200`" reference
- failure ⇒ `0x8AC70007`, i.e. `CreateSoundBank` fails outright (it does *not* silently go dark —
  the HRESULT is returned; the "goes dark" symptom is because `gamemdx`'s
  `soundbank_create` at `+0x1AAFA0` ignores the HRESULT and just leaves the slot NULL)

**[OBS] Independently verified numerically.** Reimplementing the reflected CRC-16 (poly `0x8408`,
init `0xFFFF`, final NOT) and running it over all five stock sound banks:

| file | stored @0x08 | computed | |
|---|---|---|---|
| `aaaa.xsb` | `0xCB7F` | `0xCB7F` | ✔ |
| `bgm_menu.xsb` | `0xC255` | `0xC255` | ✔ |
| `se_normal.xsb` | `0x6BEB` | `0x6BEB` | ✔ |
| `se_system.xsb` | `0xF5FF` | `0xF5FF` | ✔ |
| `voice.xsb` | `0xDCAA` | `0xDCAA` | ✔ |

`ddr-chart-tools/src/xsb/mod.rs:343-347` (`write_crc`) implements precisely this. **No change
needed.**

### 5.4 Cue-name hash — `FUN_0040fad0` @ `0x0040fad0` — confirmed, and confirmed as the one `GetCueIndex` uses

**[OBS]** raw bytes at `0x0040fad0` disassembled:

```
440fb61a   MOVZX  R11D, byte [RDX]        ; first char
664533d2   XOR    R10W, R10W              ; h = 0
4584db     TEST   R11B, R11B
742b       JZ     end
loop:
410fb7c2   MOVZX  EAX, R10W               ; AX = h
450fb7ca   MOVZX  R9D, R10W               ; R9W = h
48ffc2     INC    RDX
6603c0     ADD    AX, AX                  ; AX = 2h
6641d1e9   SHR    R9W, 1                  ; R9W = h>>1
4403d0     ADD    R10D, EAX               ; h += 2h        -> 3h
410fbec3   MOVSX  EAX, R11B               ; sign-extend char
448a1a     MOV    R11B, byte [RDX]        ; next char
4503d1     ADD    R10D, R9D               ; h += h>>1
664403d0   ADD    R10W, AX                ; h += char (u16 wrap)
4584db     TEST   R11B, R11B
75d8       JNZ    loop
```

⇒ `h = (3*h + (h >> 1) + c) mod 2^16`, then reduced modulo the bucket count. **Exactly**
`docs/xsb_format.md` §"Cue Name Hash" and `src/xsb/mod.rs`'s `cue_name_hash_bucket`. The `MOVSX`
is real (chars are sign-extended) — irrelevant for ASCII, as the doc notes.

**[OBS]** `FUN_00423d00` @ `0x00423d00` is `IXACT2SoundBank::GetCueIndex(PCSTR)` (SoundBank vtable
slot 0 — the index `gamemdx` calls at `+0x1AB7C5`), and it is the *consumer* of that hash:

```c
XACTINDEX GetCueIndex(this, const char* name) {
    if (name == NULL) return 0xFFFF;
    if ((xsb[0x12] & 1) == 0) return <fallback FUN_00423e3b>;      // no cue-name table
    bucket = FUN_0040fad0(engine, name, *(u16*)(xsb + 0x19));      // hash % total_cues
    idx    = *(i16*)(hashTable + bucket*2);                        // vt+0x98 -> xsb + [0x3E]
    while (idx != -1) {
        e       = vt[0xa8](this, idx);            // 6-byte name-index entry
        namePtr = vt[0x70](this, *(u32*)e);       // resolve the name-string offset
        if (strcmp(name, namePtr) == 0)          // byte-exact, NUL-terminated
            return vt[0x80](this, e);            // (e - buckets*2 - hashBase) / 6
        idx = *(i16*)(e + 4);                     // next in chain
    }
    return 0xFFFF;
}
```

Every structural claim in the sibling doc is corroborated: bucket count comes from
`*(u16*)(xsb+0x19)`, chains walk the `next` field at name-index `+4`, `0xFFFF` terminates,
`0xFFFF` is the not-found sentinel, and **the name comparison is a byte-exact `strcmp` — cue
lookup is case-sensitive.** **[OBS]**

**[OBS] Verified numerically** against the stock `aaaa.xsb`:
`hash("aaaa") % 16 = 1` → bucket 1 holds cue index 1 → name `"aaaa"`;
`hash("aaaa_s") % 16 = 14` → bucket 14 holds cue index 0 → name `"aaaa_s"`. Both round-trip.
And for our target code: `hash("asti") % 16 = 8`, `hash("asti_s") % 16 = 13` — **distinct buckets,
both chains length 1.**

### 5.5 Structural validator — `FUN_0040e970` @ `0x0040e970`

**[OBS]** Rule set, transcribed. `flagsByte` = `*(u8*)(pv + 0x12)`.

| # | Rule | Fail code |
|---|---|---|
| 1 | `cb >= 0x8A` | `0x8AC70610` |
| 2 | **`(flagsByte & 2) == 0`** | `0x8AC70610` |
| 3 | **byte at `0x89` (last byte of the 64-byte soundbank name at `0x4A`) must be `0`** | `0x8AC70610` |
| 4 | `FUN_0040d310` — parses the category/RPC/DSP-preset tables | (propagated) |
| 5 | if `wavebank_count` (`u8` @ `0x1B`) != 0: **`wavebank_name_offset` (`i32` @ `0x3A`) == `0x8A`** | `0x8AC70610` |
| 6 | `cb − 0x8A >= wavebank_count * 64` | `0x8AC70610` |
| 7 | **byte 63 of every 64-byte wave-bank name must be `0`** | `0x8AC70611` |
| 8 | if `sound_count` (`u16` @ `0x1C`) != 0: **`sound_offset` (`i32` @ `0x46`) == `0x8A + wavebank_count*64`** — sounds must immediately follow the wave-bank names | `0x8AC70610` |
| 9 | per sound: `FUN_0040e3f0` walks and validates each entry, building a sorted table of the actual sound-entry offsets | `0x8AC70602` etc. |
| 10 | `simple_cue_count` (`u16` @ `0x13`) + `complex_cue_count` (`u16` @ `0x15`) `< 0x10000` | `0x8AC70610` |
| 11 | if `simple_cue_count` != 0: **`simple_cue_offset` (`i32` @ `0x22`) == cursor after the sound entries**, and `cb − offset >= simple_cue_count * 5` (**5 bytes per simple cue**) | `0x8AC70610` |
| 12 | per simple cue: **flags byte must have bit 0 clear, bit 1 clear, bit 2 SET** (i.e. `0x04`), and the `u32` at cue+1 must be **found by binary search in the table of real sound-entry offsets** | `0x8AC70628` |
| 13 | if `complex_cue_count` != 0: `complex_cue_offset` (@ `0x26`) == cursor, `15` bytes per complex cue, per-cue checks | `0x8AC70610` / `0x8AC70629` |
| 14 | **`u16` @ `0x17` must be `0`** | `0x8AC70610` |
| 15a | if `(flagsByte & 1) == 0` (no cue names): **`u16@0x19 == 0`, `i32@0x1E == 0`, `i32@0x3E == −1`, `i32@0x42 == −1`, `i32@0x2A == −1`** | `0x8AC70610` |
| 15b | if `(flagsByte & 1)` set: `simple+complex != 0`, and **`u16 @ 0x19 == max(16, simple+complex)`** | `0x8AC70610` |
| 16 | `cue_hash_offset` (@ `0x3E`) == cursor after the cue arrays; `cb − offset >= buckets*2` | `0x8AC70610` |
| 17 | every hash bucket `u16` must be `0xFFFF` or `< simple+complex` | `0x8AC7062A` |
| 18 | `cue_name_index_offset` (@ `0x42`) == cursor after the hash table; `cb − offset >= total*6` (**6 bytes per name-index entry**) | `0x8AC70610` |
| 19 | **`cue_name_offset` (@ `0x2A`) == `cue_name_index_offset + total*6`** | `0x8AC70610` |
| 20 | `cue_name_offset < cb` | `0x8AC70610` |
| 21 | **`cue_name_table_length` (`i32` @ `0x1E`) == `cb − cue_name_offset`** — the string table must run exactly to EOF | `0x8AC70610` |
| 22 | **the last byte of the file must be `0`** | `0x8AC70610` |
| 23 | per name-index entry: `name_offset` in `[cue_name_offset, cb)`, and `next` is `0xFFFF` or `< total` | `0x8AC7062B` |

Notes:

- **Rule 14 confirms** `docs/xsb_format.md`'s "unknown, always 0" at `0x17`. Rule 15a confirms the
  `−1` sentinels at `0x2E`… — well, *almost*: **[OBS]** rule 15a checks `0x3E`, `0x42`, `0x2A`,
  `0x19` and `0x1E`, not `0x2E`. **I found no check on `0x2E`, `0x32` or `0x36` in this
  function.** The sibling doc says `0x2E` is "validated as an exact value (−1)"; that specific
  claim is **not** corroborated here (it may be checked elsewhere, e.g. inside `FUN_0040d310`).
  Writing `−1` is correct regardless — it matches stock.
- **Rule 15b confirms `total_cues = max(16, simple + complex)`** — and this is corroborated by all
  four stock banks **[OBS]**: `se_system` 13+0 → 16; `bgm_menu` 16+1 → **17**; `se_normal` 139+0 →
  **139**; `voice` 30+211 → **241**. The sibling writer hard-codes 16 with 2 cues, which is
  correct for our case.
- **Rule 2 reframes the byte at `0x12`.** The sibling doc calls it `platform` (`0x01` = Windows).
  **[OBS]** it is a **flags** byte: **bit 0 = "cue-name table present"** (rules 15a/15b, and
  `GetCueIndex` bails out entirely when it is clear) and **bit 1 must be clear**. All four stock
  banks have `0x01`. Writing `0x01` is right; the *meaning* is "has cue names", not "Windows".

### 5.6 The sound-entry profile: SE banks differ from song banks — **act on this**

**[OBS]** Decoding the stock sound banks' sound entries:

`se_system.xsb` (in-memory SE bank, slot 1) — all 13 sounds:

| field | value |
|---|---|
| sound `flags` | **`0x00`** (not `0x04`) |
| `entry_length` | **12** (9-byte prefix + `u16 wave_index` + `u8 wavebank_index`) |
| `category` | **5** for all 13 |
| `volume` | 180 for the `sdtest_*`, **254** for `SYS_COIN` / `X_sys_OK1`, **202** for `SYS_CARD` |
| RPC block | **absent** |

`se_normal.xsb` (in-memory SE bank, slot 2 — the one `se_game_shockarrow` lives in), 138 sounds:

| field | histogram |
|---|---|
| sound `flags` | **`{0x00: 129, 0x01: 1, 0x04: 8}`** |
| `entry_length` | **`{12: 129, 32: 1, 19: 8}`** |
| `category` | **`{6: 138}`** — every gameplay SE is category 6 |

Song banks (`aaaa.xsb`): `flags 0x05` cat **3** (preview, complex, 39 B) + `flags 0x04` cat **4**
(main, simple+RPC, 19 B). **[OBS]**

Three consequences:

1. **A bare 12-byte simple sound with `flags = 0x00` and no RPC block is valid and is what 129 of
   138 gameplay SEs use.** The 7-byte RPC tail is optional (gated on `flags & 0x04`), which
   corroborates `docs/xsb_format.md` §"Simple sound body" and extends it.
2. **The `category` field decides which mix bus the sound lands on.** Category **6** = gameplay
   SE, **5** = system SE, **4** = song main track, **3** = song preview. **[OBS]** from the four
   stock banks. `ddr-chart-tools`'s `xsb::write` emits **4 and 3** — the *song* categories.
3. `se_system.xsb` also proves that **cue-name → wave-index mapping is arbitrary** (its cue 0
   `sdtest_soft220` points at wave index 9; cue 10 `SYS_COIN` at index 10) and that **hash chains
   really do collide in stock files** (`next = 0x9`, `0xB`, `0xC` appear) — so the sibling
   writer's chaining code is exercising a real code path.

⇒ `xsb::write` is *valid* unmodified, but it will put the assist tick on the **music** bus with the
song's RPC curve attached. See §8.3 for the (tiny) alternative.

---

## 6. Sound-bank ↔ wave-bank linking and creation order

### 6.1 Is the link by name? — yes

**[OBS]** Evidence chain:

1. The XSB carries `wavebank_count` 64-byte **name** strings at `0x8A`, validated for NUL
   termination (§5.5 rules 5–7). It carries **no** wave-bank identifier other than the name.
2. Each sound entry carries a `u8 wavebank_index` selecting *which of those names*, plus a
   `u16 wave_index` selecting the entry inside it. **[OBS]** `se_system.xsb`: every sound has
   `wbIndex = 0`.
3. The WaveBank object exposes its own `szBankName` through vtable `+0x68` = `0x00419230`
   (`bankdata + 8`) **[OBS]** — an accessor that exists for no other reason.
4. **In all four stock pairs the two names are byte-identical, including case** **[OBS]**:

| XSB | soundbank name | XSB's wave-bank name | XWB's `szBankName` | match |
|---|---|---|---|---|
| `bgm_menu.xsb` | `bgm_menu` | `bgm_menu` | `bgm_menu` | exact |
| `se_normal.xsb` | `se_normal` | `se_normal` | `se_normal` | exact |
| `se_system.xsb` | `SE_SYSTEM` | **`SE_SYSTEM`** | **`SE_SYSTEM`** | exact (**uppercase!**) |
| `voice.xsb` | `voice` | `voice` | `voice` | exact |
| `aaaa.xsb` | `aaaa` | `aaaa` | `aaaa` | exact |

`se_system` is the interesting one: the *file* is `data/sound/win/se_system.xwb` (lowercase) but
the *internal* bank name is `SE_SYSTEM` (uppercase) — **and the XSB's wave-bank-name field is
uppercase too.** Two independent namespaces: `gamemdx`'s `bank_slot_of_file` (`+0x1AA3C0`) keys
off the lowercase *file basename*, while the engine's soundbank↔wavebank link keys off the
*internal* name. **[INF]** the internal match is case-sensitive (consistent with `GetCueIndex`'s
`strcmp`); regardless, **matching exact case is the proven configuration** — make the XWB's
`szBankName` byte-identical to the XSB's wave-bank name.

### 6.2 When does it happen? — lazily, not at `CreateSoundBank`

**[OBS]** `FUN_00417380` (§5.1) allocates `wavebank_count * 8` bytes for
`soundbank + 0x1E8` and **explicitly writes `NULL` into every slot**:

```c
this[0x3d] = alloc(xsb[0x1B] * 8);
for (i = 0; i < xsb[0x1B]; i++) this[0x3d][i] = NULL;
```

No wave-bank lookup, no name comparison, no failure path for a missing wave bank. ⇒

> **`CreateSoundBank` SUCCEEDS with no wave bank created at all.** The `IXACT2WaveBank*` cache is
> a lazily-populated array; resolution happens later, on the path from
> `SoundBank::Prepare` → `Cue::Init` → `Sound::Init` → wave creation. **[OBS]** for the zeroed
> array and the absence of resolution in `Initialize`; **[INF]** for the exact frame in which the
> lookup fires (I traced `Prepare`(`0x004236e0`) → `Cue::Init`(`0x00421400`) →
> `FUN_0040bc60` → `Sound::vt[8]`, and stopped before the wave-creation leaf).

Corroborating behavioural evidence **[OBS]**: `gamemdx` loads all four `.xsb` files out of
`soundbanks.arc` and the four wave banks through *independent AVS async file loads* whose
completion order is not guaranteed (`sound_file_register` at `+0x1AA520` dispatches whichever
arrives first). The engine must therefore tolerate either order, and it does.

### 6.3 Recommended creation order

```
1. eng->CreateInMemoryWaveBank(ASTI_XWB.as_ptr(), ASTI_XWB.len(), 0, 0, &mut wb);   // vt +0x50
2. eng->CreateSoundBank      (ASTI_XSB.as_ptr(), ASTI_XSB.len(), 0, 0, &mut sb);   // vt +0x48
3. let idx = sb->GetCueIndex(b"asti\0".as_ptr());                                   // vt +0x00
   // idx == 0xFFFF  => something is wrong; bail, do not Play
4. per tick:  sb->Play(idx, 0, 0, &mut cue);                                        // vt +0x20
5. reap:      cue->GetState(&st); if (st & 0x20) cue->Destroy();          // vt +0x10 / +0x18
```

Why wave-bank-first even though either works: it removes any dependence on the lazy-resolution
timing, and the in-memory wave bank is **fully prepared synchronously** before `CreateSoundBank`
is even called (§3.4 — `WAVEBANKPREPARED` is posted inside `Initialize`). Cost: nothing.

Two extra observed facts for step 4:

- **`ppCue = NULL` is legal** for a plain simple cue. **[OBS]** `FUN_00423990`:
  `if (ppCue == NULL) { hr = ((cueEntry[0] & 2) != 0) ? 0x8AC70006 : 0; }` — it only fails for
  cues with the variation/interactive bit. Our cue's flags are `0x04`, so true fire-and-forget
  works and the engine auto-releases (`Cue::vt[0x70](cue, ppCue == NULL)`). This closes the
  `[INF]` in `game-sound-engine.md` line 424. Mirroring the game's take-and-reap pattern is still
  the safer choice for a first implementation.
- **`timeOffset` is not ignored.** **[OBS]** `FUN_00423990` rejects `timeOffset < 0` with
  `E_INVALIDARG`, forwards it to `Prepare` (`0x004236e0`), which forwards it to
  `Cue::Init` (`0x00421400` = Cue vtable `+0x90`), which passes it to
  `FUN_0040bc60(cue, soundOffset, &sound, timeOffset, 0)` → `Sound::vt[8](sound, timeOffset)`.
  That call can return **`0x8AC70019`**, and `FUN_0040bc60` has a retry-with-`timeOffset = 0`
  branch — but the retry flag is passed as **0** from `Cue::Init`, so from `Play` a rejected
  `timeOffset` **fails the whole call**. **[INF]** its precise semantics (scheduled start vs seek)
  are still undetermined. ⇒ **pass `timeOffset = 0`**, exactly as `gamemdx` always does; do not
  design on it without a live test.

---

## 7. Vtable layouts

All tables below were read out of the **constructor's own vtable stores**, so the base addresses
are `[OBS]`, not pattern-matched guesses:

- `FUN_00417090` @ `0x00417090` (SoundBank ctor): `*param_1 = &PTR_FUN_00402ea0`
- `FUN_00418e20` @ `0x00418e20` (WaveBank ctor): `*param_1 = &PTR_FUN_004035f0`
- `FUN_004236e0` @ `0x004236e0` (SoundBank::Prepare): plain simple cue gets
  `*plVar7 = &PTR_FUN_00404540`

### 7.1 `IXACT2Engine` — vtable base **`0x00402260`**

**Base derivation [OBS]:** five independent `gamemdx` call sites pin it simultaneously. The
0x402240..0x402258 words belong to a different, smaller interface table; the engine table runs
`0x402260 .. 0x402318` (28 slots).

| Offset | Address | Method | Confidence |
|---|---|---|---|
| `+0x00` | `0x00410ac0` | `QueryInterface` | **[INF]** (position) |
| `+0x08` | `0x00438e00` | `AddRef` | **[INF]** |
| `+0x10` | `0x00410a80` | `Release` | **[INF]** |
| `+0x18` | `0x00411fb0` | `GetRendererCount` | **[INF]** |
| `+0x20` | `0x00411fe0` | `GetRendererDetails(u16 idx, out*)` | **[OBS]** shape: `(this, u16, ptr)`, `E_INVALIDARG` on NULL, delegates to `FUN_0041cd70` |
| `+0x28` | `0x00412020` | **`GetFinalMixFormat`** | **[OBS]** `gamemdx +0x1AAB60` calls `+0x28`; body = initialized-check + NULL-check + delegate to the device object's `+0x38` |
| `+0x30` | `0x0040fc90` | **`Initialize(const XACT_RUNTIME_PARAMETERS*)`** | **[OBS]** `gamemdx +0x1AAB60`; body reads `params[0]`, `params+0x08/+0x10` (`pGlobalSettingsBuffer`/size), `params+0x18`, `params+0x30/+0x38`, calls `GetFinalMixFormat` via `+0x28`, builds the DirectSound device, `QueryPerformanceCounter` |
| `+0x38` | `0x00412060` | **`ShutDown`** | **[OBS]** body destroys every wave bank, sound bank and cue in the engine's four lists, then releases the device — unmistakable. *This upgrades `game-sound-engine.md`'s `[INF]` row to `[OBS]`.* |
| `+0x40` | `0x004122e0` | **`DoWork`** | **[OBS]** `gamemdx +0x3020` per frame |
| `+0x48` | `0x00410bc0` | **`CreateSoundBank(pv, cb, flags, allocAttr, ppSB)`** | **[OBS]** `gamemdx +0x1AAFA0`; body allocates 0x210, ctor `0x00417090`, init `0x00417380` |
| `+0x50` | `0x00410d60` | **`CreateInMemoryWaveBank(pv, cb, flags, allocAttr, ppWB)`** | **[OBS]** `gamemdx +0x1AB050`; body allocates 0x240, ctor `0x00418e20`, init `0x00418f10` |
| `+0x58` | `0x00410f00` | **`CreateStreamingWaveBank(const XACT_STREAMING_PARAMETERS*, ppWB)`** | **[OBS]** `gamemdx +0x1AB050`; body validates `params+0x08 & 0x7FF == 0` (2048-aligned) and `params+0x10 >= 2` (packet size), allocates 0x298, ctor `0x00424830`, init `0x00424a70` |
| `+0x60` | `0x0040ff80` | **`PrepareWave(flags, szPath, packetSize, align, playOffset, loopCount, ppWave)`** | **[OBS]** takes a path, opens a media source, requires `wFormatTag` ∈ {`1`, `0x165`} |
| `+0x68` | `0x004104e0` | **`PrepareInMemoryWave(flags, entry*, seekTable*, buffer, offset, loopCount, ppWave)`** | **[OBS]** shape. *This settles open question 1 in `game-sound-engine.md`: `PrepareWave`/`PrepareInMemoryWave` DO exist, and at the `xact3.h` positions.* |
| `+0x70` | `0x004106d0` | `PrepareStreamingWave` | **[INF]** (position) |
| `+0x78` | `0x00411090` | **`RegisterNotification`** | **[OBS]** `gamemdx +0x1AAB60` ×5 |
| `+0x80` | `0x004111c0` | `UnRegisterNotification` | **[INF]** |
| `+0x88` | `0x00410920` | `GetCategory` | **[INF]** |
| `+0x90` | `0x004125d0` | `Stop` | **[INF]** |
| `+0x98` | `0x00411300` | `GetGlobalVariableIndex` | **[INF]** |
| `+0xa0` | `0x00411420` | `SetGlobalVariable` | **[INF]** |
| `+0xa8` | `0x00411550` | `GetGlobalVariable` | **[INF]** |
| `+0xb0` | `0x00411640` | category-scoped **set volume** `(u16 categoryIdx, float)` | **[OBS]** shape: validates the index against the XGS category table (`0x8AC7000A`), then `FUN_00429870(cat, vol, 0)` |
| `+0xb8` | `0x00411760` | category-scoped **get volume** `(u16 categoryIdx, float*)` | **[OBS]** shape |

### 7.2 `IXACT2SoundBank` — vtable base **`0x00402ea0`**

| Offset | Address | Method | Confidence |
|---|---|---|---|
| `+0x00` | `0x00423d00` | **`XACTINDEX GetCueIndex(PCSTR)`** | **[OBS]** §5.4; `gamemdx +0x1AB7C5` |
| `+0x08` | `0x00423e60` | `GetNumCues` | **[INF]** |
| `+0x10` | `0x00423f00` | `GetCueProperties` | **[INF]** |
| `+0x18` | `0x004236e0` | **`Prepare(XACTINDEX, DWORD, XACTTIME, IXACT2Cue**)`** | **[OBS]** §6.3; `gamemdx +0x1AB6xx` |
| `+0x20` | `0x00423990` | **`Play(XACTINDEX, DWORD, XACTTIME, IXACT2Cue**)`** | **[OBS]** = `Prepare` + `Cue::vt[0x70]`; `gamemdx +0x1AB805` |
| `+0x28` | `0x00423b80` | `Stop` | **[INF]** |
| `+0x30` | `0x00423600` | **`Destroy()`** | **[OBS]** body stops every cue in the bank's list at `+0x1E0`, then releases via `+0x18`; `gamemdx +0x1AB3D0` |
| `+0x38` | `0x00423c70` | `GetState` | **[INF]** |
| `+0x40`…`+0x98` | `0x004174d0`, `0x00417540`, `0x004172a0`(×4), `0x00417110`, `0x004172c0`, `0x00417310`, `0x00417300`, `0x00417190`, `0x004171f0` | internal XSB-structure accessors (`+0x60` sound-entry base, `+0x70` name-string resolve, `+0x80` name-index→cue-index, `+0x90` XSB base pointer, `+0x98` hash-table base, `+0xa0` cue-entry by index, `+0xa8` name-index entry by index) | **[OBS]** roles, from their use in `GetCueIndex` / `Prepare` / `Initialize` |

### 7.3 `IXACT2WaveBank` — vtable base **`0x004035f0`**

| Offset | Address | Method | Confidence |
|---|---|---|---|
| `+0x00` | `0x0042b9b0` | **`Destroy()`** | **[OBS]** `gamemdx +0x1AB3D0` |
| `+0x08` | `0x0042b410` | `GetState` / `GetNumWaves` | **[INF]** |
| `+0x10` | `0x0042b490` | `GetWaveIndex(PCSTR)` | **[INF]** |
| `+0x18` | `0x0042b860` | `GetWaveProperties` | **[INF]** |
| `+0x20` | `0x0042b590` | `Prepare` | **[INF]** |
| `+0x28` | `0x0042b6e0` | **`Play(...)`** | **[INF]** — *this is `game-sound-engine.md` open question 2. The slot exists and is a real 175-byte function; its identity as `Play` is still positional, since neither `gamemdx` nor any code path I traced calls it.* |
| `+0x30` | `0x0042bac0` | `Stop` | **[INF]** |
| `+0x38` | `0x0042b370` | — | **[INF]** |
| `+0x58` | `0x00418ea0` | `GetEntry(u16 idx, WAVEBANKENTRY* out)` | **[OBS]** §3.4 |
| `+0x60` | `0x00419200` | seek-table lookup | **[OBS]** |
| `+0x68` | `0x00419230` | **bank-name pointer** (`bankdata+8`) | **[OBS]** |
| `+0x70` | `0x004192a0` | `dwEntryCount` | **[OBS]** |
| `+0x78` | `0x004192c0` | `Segments[3].dwLength != 0` ("has entry names") | **[OBS]** |
| `+0x88` | `0x00419250` | `dwVersion` | **[OBS]** |
| `+0x90` | `0x00419270` | `dwHeaderVersion` | **[OBS]** |
| `+0x98` | `0x00419290` | `dwSignature` | **[OBS]** |
| `+0xa8` | `0x00418ee0` | `dwFlags` (the STREAMING test) | **[OBS]** |
| `+0xb0` | `0x00418ef0` | `BuildTime` pointer | **[OBS]** |
| `+0xd8` | `0x0042b310` | `IsHeaderValid()` | **[OBS]** §3.2 |
| (others) | `0x00439cf0`, `0x004248d0`, `0x004192e0`, `0x0041a680`, `0x0042bbc0`, `0x0042bc10`, `0x004191d0`, `0x0042b2d0`, `0x00419060`, `0x00418e90`, `0x004196c0` | internal | **[INF]** |

### 7.4 `IXACT2Cue` (simple cue) — vtable base **`0x00404540`**

This closes the biggest unknown in `game-sound-engine.md`.

| Offset | Address | Method | Confidence |
|---|---|---|---|
| `+0x00` | `0x0040ab30` | **`Play()`** | **[OBS]** `gamemdx +0x1ABB30` |
| `+0x08` | `0x0040aba0` | **`Stop(DWORD)`** | **[OBS]** `gamemdx +0x1AA7C0`, `+0x1AA850` |
| `+0x10` | `0x0040ad90` | **`GetState(DWORD*)`** | **[OBS]** `gamemdx +0x1AB8C0`, `+0x1ABB30` |
| `+0x18` | `0x0040aaa0` | **`Destroy()`** | **[OBS]** `gamemdx +0x1ABB30`, **and** the failure paths of `Prepare`/`Play` inside the engine itself |
| `+0x20` | `0x0040af10` | **unknown (189 B)** | **[OBS]** the slot exists; identity unknown |
| `+0x28` | `0x0040b0f0` | **unknown (165 B)** | **[OBS]** exists |
| `+0x30` | `0x0040b260` | **unknown (184 B)** | **[OBS]** exists |
| `+0x38` | `0x0040b320` | **unknown (182 B)** | **[OBS]** exists |
| `+0x40` | `0x0040b3e0` | **`SetMatrixCoefficients(u32 src, u32 dst, float*)`** | **[OBS]** `gamemdx +0x1ABF90` |
| `+0x48` | `0x0040b750` | unknown | **[INF]** |
| `+0x50` | `0x0040c7b0` | unknown | **[INF]** |
| `+0x58` | `0x0040c8a0` | unknown | **[INF]** |
| `+0x60` | `0x0040ac70` | **`Pause(BOOL)`** | **[OBS]** `gamemdx +0x1AB840` |
| `+0x70` | `0x0040c100` | internal `Start(bool autoRelease)` | **[OBS]** called by `Play`(`0x00423990`) with `ppCue == NULL` as the argument |
| `+0x90` | `0x00421400` | internal `Init(XACTTIME timeOffset)` | **[OBS]** called by `Prepare`(`0x004236e0`) |
| `+0xa8`/`+0xb0`/`+0xb8` | `0x00413c70` | 3-byte stub (shared) | **[OBS]** |

⇒ **Confirmed: `IXACT2Cue` in v2.10 has four extra real methods at `+0x20..+0x38` between
`Destroy` and `SetMatrixCoefficients`, which is why the interface does not match `xact3.h`.**
Our design touches only `+0x00/+0x08/+0x10/+0x18/+0x40/+0x60`, all `[OBS]`. Note also that a
*complex* cue gets a different vtable (`0x00403990`) and a *variation* cue a third object type
(0x318 bytes, ctor `0x0041a990`) **[OBS]** `0x004236e0` — our cue is the simple kind.

---

## 8. Concrete generation recipe

Source: `clap.ogg` — **[OBS]** `ffprobe`: `codec_name=vorbis, sample_rate=44100,
channels=1, duration=0.213673`; decoded to raw s16le it is exactly **9,423 samples**
(18,846 bytes). It is *already* mono 44.1 kHz, so **no transcode is required** — feed the `.ogg`
straight to `ddr_chart_tools::ogg::decode`.

### 8.1 The XWB — exact field values

```rust
// codec=2 (MS-ADPCM), channels=1, rate=44100, block_align_raw=48, bits flag=0
const FMT: u32 = 2 | (1 << 2) | (44100 << 5) | (48 << 23);   // == 0x1815_8886
// derived, not stored:  dwBlockAlign = (48+22)*1 = 70 bytes;  samplesPerBlock = 128
```

| `XwbBank` field | Value | Why |
|---|---|---|
| `header_version` | **`42`** | **[OBS]** hard requirement, §3.2 |
| `flags` | **`0x0009_0000`** | **[OBS]** byte-for-byte the stock in-memory value (§2.3). Decomposes as TYPE_BUFFER (bit 0 clear — **mandatory** for `CreateInMemoryWaveBank`, §3.5) + ENTRYNAMES (bit 16 — must agree with `Segments[3].dwLength != 0`, §3.4) + bit 19. `(0x00090000 & 0xFFF0FFFE) == 0` ✔ rule 5 |
| `name` | `b"asti"` + 60 × `0x00` | **[OBS]** must equal the XSB's wave-bank name exactly, incl. case (§6.1); byte 63 must be `0` (rule 9) |
| `entry_name_element_size` | **`64`** | **[OBS]** rule 10 — *required even if you skip names* |
| `alignment` | **`4`** | **[OBS]** rule 11 minimum, and exactly what both stock in-memory banks use. (2048 would also validate but bloats the blob.) |
| `compact_format` | `0` | **[OBS]** stock; unused on the non-compact path |
| `build_time` | `0` | **[OBS]** never validated (§2.2); stock writes a real FILETIME, irrelevant |
| `entries` | 2, in this order | see below |

**Entry 0 — `"asti_s"`, the preview slot (never played)**

| Field | Value |
|---|---|
| `format` | `0x18158886` |
| `data` | **one silent ADPCM block, 70 bytes** (encode a single zero sample; the encoder pads to a whole block) |
| `flags_and_duration` | `128 << 4` = **`0x0000_0800`** |
| `loop_start` | `0` |
| `loop_length` | `128` |
| `name_bytes` | `b"asti_s"` (writer pads to 64) |

**Entry 1 — `"asti"`, the clap (this is what plays)**

| Field | Value |
|---|---|
| `format` | `0x18158886` |
| `data` | **5,180 bytes** = `ceil(9423 / 128)` = **74 blocks** × 70 B |
| `flags_and_duration` | `9472 << 4` = **`0x0002_5000`** (74 × 128 = 9,472) |
| `loop_start` | `0` |
| `loop_length` | `9472` |
| `name_bytes` | `b"asti"` |

**Why two entries.** `ddr-chart-tools`'s XSB writer points the **main** cue at wave **index 1**
and the preview cue at index 0 (`src/xsb/mod.rs:219-222` and `:239`) **[OBS]**. Rather than touch
the sibling repo, give the XWB two entries and let index 0 be a 70-byte silent stub. Cost: 72
bytes of the final blob (70 + 2 alignment). Recommended.

**On `Duration`.** **[OBS]** the *only* engine constraint is
`Duration >= loop_start + loop_length` (rule E7). Setting `Duration == loop_length == 9472`
satisfies it with equality, which is exactly what stock `se_system` entry 12 (`SYS_CARD`) does
**[OBS]**. `Duration` is *not* required to relate to `PlayRegion.dwLength` in any way. The units
are samples (bits 4..31 of `dwFlagsAndDuration`), corroborated numerically: `se_system` entry 0
has `Duration = 102,401` and 800 blocks × 128 samples/block = 102,400.
**[INF]** whether the engine uses `Duration` or `LoopRegion.dwTotalSamples` (or the play-region
byte length) to decide when the wave ends — I did not trace the playback leaf. Mirroring stock
(all three mutually consistent, block-aligned) sidesteps the question. The 49 samples of
trailing silence (9,472 − 9,423 = 1.1 ms) are inaudible.

**Resulting layout — computed and verified (§9):**

```
0x0000  header               52 B   'WBND', 43, 42, 5 × {off,len}
0x0034  Segments[0] BANKDATA 96 B   flags=0x00090000, count=2, "asti", 24, 64, 4, 0, 0
0x0094  Segments[1] META     48 B   2 × 24
0x00C4  Segments[2] SEEK      0 B
0x00C4  Segments[3] NAMES   128 B   2 × 64  ("asti_s", "asti")
0x0144  Segments[4] WAVEDATA 5252 B  entry0 @ +0 (70 B), entry1 @ +72 (5180 B)
------  total                5576 B     (0x1748)
```
`cb − Segments[4].dwOffset = 5576 − 324 = 5252 == Segments[4].dwLength` ✔ rule 20.
`Segments[3].dwOffset = 0x94 + 2*24 = 0xC4` ✔ rule 15.

### 8.2 The XSB

**`ddr_chart_tools::xsb::write("asti", &mut out)` — unmodified. [OBS]**

- `validate_code` accepts 1–16 ASCII alphanumerics (`src/xsb/mod.rs:95-104` doc comment);
  `"asti"` qualifies.
- Output size: `0x4A + 64 + 64 + (39 + 19) + (2×5) + (16×2) + (2×6) + 12` = **326 bytes** — the
  same size as every stock 4-char song bank **[OBS]**.
- Soundbank name = wave-bank name = main cue name = `"asti"`; preview cue = `"asti_s"`.
- Hash buckets: **8** for `"asti"`, **13** for `"asti_s"` — distinct, no chaining **[OBS]**
  (computed with the verified `FUN_0040fad0` algorithm).
- CRC is computed and back-patched by `write_crc` using the algorithm confirmed verbatim in §5.3.
- Every field satisfies §5.5: `cb=326 ≥ 0x8A` ✔; `flagsByte=0x01` (bit 1 clear) ✔;
  byte `0x89` = 0 ✔; `wavebank_name_offset = 0x8A` ✔; `sound_offset = 0xCA = 0x8A + 1*64` ✔;
  `simple_cue_offset = 0x104 = 0xCA + 39 + 19` ✔; `u16@0x17 = 0` ✔;
  `u16@0x19 = 16 = max(16, 2+0)` ✔; `cue_hash_offset = 0x10E` ✔;
  `cue_name_index_offset = 0x12E = 0x10E + 32` ✔; `cue_name_offset = 0x13A = 0x12E + 2*6` ✔;
  `cue_name_table_length = 12 = 326 − 314` ✔; last byte `0` ✔; both simple-cue flag bytes `0x04`
  with `sb_code` ∈ {`0xCA`, `0xF1`} = real sound offsets ✔.

**The cue to play at runtime is `"asti"`** (the main/simple cue, → wave index 1 = the clap).
Never play `"asti_s"` — it is the *complex* sound carrying the song-preview loop event.

### 8.3 The one thing that is *valid* but probably *wrong*: the mix category

**[OBS]** `xsb::write` emits `category = 4` for the main (simple) sound and `category = 3` for the
preview, plus the RPC reference `0xF8` — the DDR **song** profile. **[OBS]** the game's own
gameplay SEs (`se_normal.xsb`, incl. `se_game_shockarrow`) use **`category = 6`** with the bare
12-byte simple-sound form (`flags = 0x00`, no RPC block), and the system SEs use `category = 5`.

Consequences of shipping the unmodified writer output **[INF]**: the tick is mixed on the
song/music bus, so it inherits whatever volume, RPC curve and category-level ducking or
`IXACT2Engine::Stop`-by-category the game applies to song audio — and it will *not* follow the
SE volume the operator/`se_set_volume` controls. For an assist tick that must be audible against
the music, that is very likely the wrong bus.

**If a change is wanted, it is tiny — describe only, do not make it here** (sibling repo):

1. In `ddr-chart-tools/src/xsb/mod.rs`, `SIMPLE_SOUND_BYTES` (line 219) is
   `[0x04, 0x04, 0x00, 0xB4, 0x00, 0x00, 0x00, 0x13, 0x00, 0x01, 0x00, 0x00, 0x07, 0x00, 0x01, 0xF8, 0x00, 0x00, 0x00]`.
   The SE profile equivalent is the **12-byte** form
   `[0x00, 0x06, 0x00, 0xFE, 0x00, 0x00, 0x00, 0x0C, 0x00, 0x01, 0x00, 0x00]`
   — `flags = 0x00` (no RPC), `category = 6`, `volume = 0xFE` (254, what `SYS_COIN` uses),
   `entry_length = 12`, `wave_index = 1`, `wavebank_index = 0`.
2. That changes `SIMPLE_SOUND_SIZE` from 19 to 12, which shifts `Layout::compute`'s
   `simple_cue`/`hash_table`/`name_index`/`cue_names` offsets and the cue 1 `sb_code` — all
   already derived from the constant, so the only edits are the constant and the array. Total
   file size becomes 319 bytes.
3. It would need to be a new function (e.g. `xsb::write_se(code, category, volume, out)`) rather
   than a change to `write`, because `write` is the song-authoring path and must keep emitting the
   song profile.

**Alternative that avoids the sibling repo entirely:** ship the unmodified 326-byte XSB first,
measure how the tick behaves against the music, and only take the change if the category actually
bites. The formats are independent — swapping the XSB later costs nothing.

**Second, smaller sibling-repo option** (mentioned for completeness): changing
`SIMPLE_SOUND_BYTES[9]` from `0x01` to `0x00` would point the main cue at wave **index 0** and let
our XWB carry a single entry, saving 72 bytes. Not worth a sibling change on its own.

### 8.4 Step-by-step offline procedure

```bash
# 0. (only if the source is not already mono/44.1k — clap.ogg already is)
ffmpeg -y -i input.wav -ac 1 -ar 44100 -c:a libvorbis -q:a 6 clap.ogg
```

Throwaway generator — a scratch crate that path-depends on the sibling, e.g.
`/tmp/astigen/Cargo.toml`:

```toml
[package]
name = "astigen"
version = "0.0.0"
edition = "2021"

[dependencies]
ddr-chart-tools = { path = "../ddr-chart-tools" }
```

`/tmp/astigen/src/main.rs`:

```rust
use ddr_chart_tools::xwb::{self, adpcm, WaveFormat, XwbBank, XwbEntry};
use ddr_chart_tools::{ogg, xsb};
use std::fs;

const CODE: &str = "asti";
const FMT_BITS: u32 = 2 | (1 << 2) | (44100 << 5) | (48 << 23); // 0x18158886

fn entry(name: &str, pcm: &[i16], fmt: WaveFormat) -> XwbEntry {
    let data = adpcm::encode::encode(pcm, &fmt).expect("adpcm encode");
    let blocks = data.len() / fmt.block_align() as usize;
    let total_samples = blocks as u32 * fmt.samples_per_block();
    XwbEntry {
        flags_and_duration: total_samples << 4, // entry flags nibble MUST be 0
        format: fmt,
        data,
        loop_start: 0,
        loop_length: total_samples,
        name_bytes: name.as_bytes().to_vec(), // writer pads to 64
    }
}

fn main() {
    let audio = ogg::decode::decode(&fs::read("clap.ogg").unwrap()).unwrap();
    assert_eq!(audio.channels, 1, "source must be mono");
    assert_eq!(audio.sample_rate, 44100, "source must be 44100 Hz");

    let fmt = WaveFormat::from_packed(FMT_BITS);
    assert_eq!((fmt.block_align(), fmt.samples_per_block()), (70, 128));

    let mut name = [0u8; 64];
    name[..CODE.len()].copy_from_slice(CODE.as_bytes());

    let bank = XwbBank {
        header_version: 42,
        flags: 0x0009_0000,
        name,
        entry_name_element_size: 64,
        alignment: 4,
        compact_format: 0,
        build_time: 0,
        entries: vec![
            entry(&format!("{CODE}_s"), &[0i16], fmt),   // index 0: silent stub, 70 B
            entry(CODE, &audio.samples, fmt),            // index 1: the clap, 5180 B
        ],
    };

    let mut xwb_bytes = Vec::new();
    xwb::write(&bank, &mut xwb_bytes).unwrap();
    let mut xsb_bytes = Vec::new();
    xsb::write(CODE, &mut xsb_bytes).unwrap();

    // Round-trip + expected sizes (see §9).
    let reparsed = xwb::parse(&xwb_bytes).unwrap();
    assert_eq!(reparsed.name_str(), CODE);
    assert_eq!(reparsed.entries.len(), 2);
    assert_eq!(reparsed.entries[1].data, bank.entries[1].data);
    assert_eq!(xwb_bytes.len(), 5576, "XWB size");
    assert_eq!(xsb_bytes.len(), 326, "XSB size");

    fs::write("/tmp/astigen/asti.xwb", &xwb_bytes).unwrap();
    fs::write("/tmp/astigen/asti.xsb", &xsb_bytes).unwrap();
    println!("asti.xwb = {} B, asti.xsb = {} B", xwb_bytes.len(), xsb_bytes.len());
}
```

```bash
cd /tmp/astigen && cargo run --release
# -> asti.xwb = 5576 B, asti.xsb = 326 B
```

Then copy the two blobs into the modpack (e.g. `assets/assist_tick/asti.{xwb,xsb}`; the exact
path is the implementer's call — this document changes no repo source) and reference them with
`include_bytes!`. **Total embedded payload: 5,902 bytes.**

**Expected output byte sizes**

| Artefact | Bytes | Note |
|---|---|---|
| `asti.xwb` | **5,576** | verified by construction, §9 |
| `asti.xsb` | **326** | same as every stock 4-char song bank |
| combined | **5,902** | ~5.8 KB added to the DLL |

*(If entry 0 duplicates the clap instead of being a silent stub, the XWB is **10,684** bytes.)*

---

## 9. Offline validation plan

Everything below was **actually run** while writing this document, in `/tmp/xact/` (no repo files
touched). Reproduce it before deploying, and after any change to the generator.

### 9.1 Round-trip through the sibling parser

`xwb::parse(&generated)` then compare fields and per-entry `data` against the input bank.
`container.rs`'s own tests already prove `parse ∘ write` is byte-identical
(`src/xwb/container.rs:566-607`). Assert the two output sizes (5576 / 326) — a size change is the
cheapest possible tripwire for an accidental layout change.

**[OBS] Result** for the generated bank:

```
name='asti' flags=0x00090000 align=4 header_version=42 nameElem=64 entries=2
 [0] 'asti_s' codec=2 ch=1 rate=44100 baRaw=48 bps=0 doff=    0 dlen=  70 Duration=128  loop=(0,128)
 [1] 'asti'   codec=2 ch=1 rate=44100 baRaw=48 bps=0 doff=   72 dlen=5180 Duration=9472 loop=(0,9472)
segments: [(0x34,0x60), (0x94,0x30), (0xc4,0x0), (0xc4,0x80), (0x144,0x1484)]
```

### 9.2 Replay the engine's own validator, offline

Transcribe `FUN_0040f120` + `FUN_0042b310` + the `0x00418f18` in-memory gate into a checker (I
used Python; a Rust integration test in the *modpack* would be better long-term) and — critically
— **run it against the two stock in-memory banks as a control.** A checker that rejects a stock
bank is wrong; a checker that accepts everything is useless.

**[OBS] Result:**

```
asti.xwb              5576 B  ->  PASS (engine would accept)
se_normal.xwb     17740212 B  ->  PASS (engine would accept)
se_system.xwb       671900 B  ->  PASS (engine would accept)
```

The 24 rules the checker enforces are exactly the table in §3.3 plus §3.2 and §3.4's
flag/segment consistency trap. This is the single highest-value pre-deploy gate: it turns
"`CreateInMemoryWaveBank` returned `0x8AC70632` and we don't know why" into a named rule.

### 9.3 Compare header fields against the stock banks

Diff the generated bank's BANKDATA against `se_system.xwb`'s field-by-field. Everything except
`dwEntryCount`, `szBankName` and `BuildTime` should be **identical** (`flags 0x00090000`,
`metaElem 24`, `nameElem 64`, `alignment 4`, `compactFormat 0`, `header_version 42`) — and it is
**[OBS]**.

### 9.4 Verify the XSB's CRC and hash independently of the writer

Recompute the CRC over `[0x12..]` with the algorithm from §5.3 and compare against the byte
stored at `0x08`; recompute both cue-name buckets with the algorithm from §5.4 and check they
index name-index entries whose strings match. **Validate the checker against stock first** —
mine reproduces all five stock CRCs and both stock `aaaa.xsb` bucket→name round-trips **[OBS]**.

### 9.5 Decode the ADPCM back and measure SNR

Decode the generated entry with `ddr_chart_tools::xwb::adpcm::decode::decode` and compare against
the source PCM. This catches a silently-broken encode (all-zero output, wrong predictor, wrong
nibble packing) that the container validator cannot see.

**[OBS]** With a naive reference encoder (predictor 0 only) I measured 9,472 decoded samples and
**17.6 dB SNR** — audibly correct but unremarkable, as expected for fixed-predictor MS-ADPCM.
The sibling crate's encoder selects among the 7 predictors, so **expect materially better than
17.6 dB; treat anything at or below that as a red flag**, and treat < 6 dB or a NaN/silent decode
as a hard fail.

### 9.6 What cannot be validated offline

- Whether the tick is audible at the right level (the category question, §8.3).
- `IXACT2SoundBank::Play`'s latency/jitter in practice (`game-sound-engine.md` §Timing).
- Handle/instance pressure — note **[OBS]** `SoundBank::Play` enforces a per-cue instance limit
  (`0x8AC70008`) using `soundbank[0x3e][cueIndex]` against the sound entry's byte 9; our simple
  sound's `flags & 1 == 0` so that branch is skipped, but confirm live.

---

## 10. Open questions

Ordered by how much they matter to this feature.

1. **Which mix category should the tick use, and does the song category actually hurt?**
   **[OBS]** the facts (SE = 6, system = 5, song main = 4, preview = 3); **[INF]** the
   consequences. Settle it with one deploy: ship the unmodified `xsb::write` output, then compare
   the tick's loudness against `se_game_shockarrow` and check it survives the song-end
   transition. §8.3 has the fix if it bites.
2. **What `IXACT2Cue` slots `+0x20`, `+0x28`, `+0x30`, `+0x38` are.** **[OBS]** all four exist as
   real 165–189-byte functions. Our design never calls them, so this is documentation debt, not
   risk. Would be closed by finding *any* caller (nothing in `gamemdx` touches them).
3. **`timeOffset` semantics.** **[OBS]** it is validated `>= 0`, forwarded through
   `Prepare → Cue::Init(+0x90) → FUN_0040bc60 → Sound::vt[8]`, and can fail with `0x8AC70019`.
   **[INF]** whether it means scheduled-start or seek. Trace `Sound::vt[8]` (the Sound vtable is
   installed by `FUN_0040ba70`) if a future revision wants sample-accurate scheduling. **Pass 0.**
4. **`IXACT2WaveBank::Play`'s identity.** **[OBS]** vtable `+0x28` = `0x0042b6e0` exists (175 B).
   **[INF]** that it is `Play`. Only matters if we ever want to bypass the sound bank entirely —
   which would also mean losing the category/volume machinery, so probably never.
5. **Which of `Duration` / `LoopRegion.dwTotalSamples` / `PlayRegion.dwLength` the playback engine
   uses to end a wave.** **[OBS]** the validator only relates the first two. Mirroring stock makes
   all three consistent, so this is only interesting if we later want sub-block trimming.
6. **Where ADPCM is actually decoded.** **[OBS]** negative result: no MS-ADPCM coefficient or
   adaptation tables anywhere in `xactengine2_10.dll`, no `msacm32` import. **[INF]** delegated to
   the DirectSound/OS mixer via a non-PCM `WAVEFORMATEX`. Academic — the game's own ADPCM SEs play
   on this platform.
7. **Whether the internal wave-bank name match is case-*sensitive*.** **[OBS]** all five stock
   pairs match byte-for-byte including case, and `GetCueIndex` uses a byte-exact `strcmp`.
   **[INF]** the wave-bank comparison is likewise exact. Moot if we always match case (we do).
8. **The `0x2E` sentinel.** **[OBS]** `FUN_0040e970` does *not* check the `i32` at `0x2E` (nor
   `0x32`/`0x36`), contradicting `docs/xsb_format.md` §Header's claim that it is validated as an
   exact `−1`. It may be checked inside `FUN_0040d310`. Writing `−1` (as the sibling does) is
   correct either way.
9. **`FUN_0040d310` / `FUN_0040e3f0`** — the XGS-linked category/RPC/DSP table parser and the
   per-sound-entry validator. Not traced. Relevant only if we start hand-authoring sound entries
   with unusual `flags`, which §8.3's 12-byte form would (mildly) do — the 129 stock precedents in
   `se_normal.xsb` make that low-risk.
10. **Cross-version stability.** **[OBS]** every address in this document belongs to
    `xactengine2_10.dll`, **not** `gamemdx.dll`. Per `game-sound-engine.md` §Cross-Version
    Caution, these are stable as long as the shipped engine DLL is unchanged. **Fingerprint the
    engine DLL (413,104 bytes) rather than the game DLL if any of these addresses are ever
    hard-coded** — though our design hard-codes only vtable *indices*, which is one level safer.

---

## Appendix A — Ghidra bootstrap

Reproduce the function table before doing anything else in this program (see the note at the top).
Both scripts run via `ghidra_run_script_inline` against `xactengine2_10.dll`.

**Pass 1 — create a function at every `.pdata` entry point:**

```java
Memory mem = currentProgram.getMemory();
MemoryBlock pdata = mem.getBlock(".pdata");
long ps = pdata.getStart().getOffset(), pe = pdata.getEnd().getOffset();
long base = currentProgram.getImageBase().getOffset();
for (long a = ps; a + 12 <= pe + 1; a += 12) {
    int begin = mem.getInt(toAddr(a));
    if (begin == 0) continue;
    Address fa = toAddr(base + (begin & 0xFFFFFFFFL));
    if (getFunctionAt(fa) != null) continue;
    disassemble(fa);
    createFunction(fa, null);
}
```
**[OBS] Result:** `pdata entries: 1898 / created=981 existed=917 failed=0`.

**Pass 2 — force each body to its `.pdata` `{BeginAddress, EndAddress}` range** (pass 1 alone
leaves many bodies at 1–18 bytes because flow is still incomplete):

```java
// ... same .pdata walk, also reading EndAddress at a+4 ...
AddressSet body = new AddressSet(st, en);
Function f = getFunctionAt(st);
if (f == null) currentProgram.getFunctionManager()
        .createFunction(null, st, body, SourceType.ANALYSIS);
else if (f.getBody().getNumAddresses() < body.getNumAddresses()) f.setBody(body);
```
**[OBS] Result:** `ranges=1898 / made=15 fixed=595 err=18`, `total funcs=2173`.
`FUN_00423d00` goes from a 1-byte body to 135 bytes; `FUN_0040f120` and `FUN_0040e970` become
decompilable.

Note the class name in an inline script must match the file the tool writes
(`McpInline_*`/`<ClassName>.java`); a stale failing file in `~/ghidra_scripts` will print a
compile error on every later run without blocking it.

---

## Appendix B — HRESULT quick reference

Collected from the functions cited above. **All [OBS].**

| HRESULT | Raised by |
|---|---|
| `0x80070057` E_INVALIDARG | NULL/zero args to `Create*`; `timeOffset < 0` in `Prepare`/`Play`; bad streaming params |
| `0x8007000E` E_OUTOFMEMORY | object allocation failed |
| `0x8AC70002` | engine not initialized (`*(int*)(engine+200) == 0`) |
| `0x8AC70006` | **bank-type mismatch**: STREAMING bank to `CreateInMemoryWaveBank` (`0x00418f18`) / BUFFER bank to the file path (`0x00424af0`); also `Play(ppCue=NULL)` on a variation cue |
| `0x8AC70007` | invalid data: XWB header identity fail; XWB structural validator fail (wrapped); XSB magic/tool-version fail; **XSB CRC mismatch**; XSB structural validator fail |
| `0x8AC70008` | per-cue instance limit reached (`SoundBank::Play`) |
| `0x8AC7000A` | bad category index (engine `+0xb0`/`+0xb8`) |
| `0x8AC7000C` | cue index out of range (`Prepare`/`Play`) |
| `0x8AC70012` | **called from the notification-callback thread** (all `Create*`, `Prepare`, `Play`) |
| `0x8AC70019` | `timeOffset` rejected by the Sound |
| `0x8AC70602` | XSB complex-sound/track field out of range |
| `0x8AC70610` | XSB header/section sizing |
| `0x8AC70611` | XSB wave-bank name not NUL-terminated |
| `0x8AC70628` / `0x8AC70629` | XSB simple / complex cue entry invalid |
| `0x8AC7062A` / `0x8AC7062B` | XSB hash bucket / name-index entry out of range |
| `0x8AC70630` | XWB header or segment sizing |
| `0x8AC70631` | XWB entry `Format` field |
| `0x8AC70632` | XWB BANKDATA field |
| `0x8AC70633` | XWB entry region or `Duration` |
| `0x8AC70634` | XWB entry name not NUL-terminated |




