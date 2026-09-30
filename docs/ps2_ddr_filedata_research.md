# PS2 DDR FILEDATA Archive and DDR STRIKE Assets — RE Notes (2026-09-29)

**Question.** How are the assets of the PS2 DDR games stored? The goal is to extract them
exactly, not by scanning for known headers. What does DDR STRIKE contain on the way to
porting its 3D dancers?

**Answer.**
- **The archive has an exact file table (TOC) in the ELF.** It is a sorted array of 8-byte
  `{u16 id, u24 sector, u24 sectors}` entries over one sector space. FILEDATA.BIN,
  FILEDT02.BIN and FILEDT03.BIN form that space back to back. No heuristics are needed to
  cut the archive into files (§1–§2).
- **STRIKE's polygon dancers are the System 573 engine's**, carried over almost unchanged
  (§4):
  - 45 dancers: 38 distinct meshes, 7 of which come in two colourings;
  - 8 motion sets, drawn from 17 routines;
  - the rig tables are in the ELF.
  - `scripts/sys573_dancer_dump.py parse_cmd` reads every STRIKE mesh unchanged. Rest-pose
    renders with the ELF rig and the TCB textures look correct.
  - The motion files keep 573's semantics in a PS2 key-block layout (§4.3), which
    `parse_cmm` / `sample` now read.
  - The game's character table (§4.4) names all 45 dancers and gives each one a scale.
  - **All 45 are ported** as `data_mods/custom_models/dancers/Strike *`
    (`tools/blender_ddr_addon/examples/port_character_strike.py`). They are not yet
    cabinet-tested.
- The song backgrounds are MPEG-1 clip sets. The attract movies show pre-rendered
  characters, and the lesson / workout sprites are 2D frame sequences. None of those is
  geometry.

**Tools.**
- `scripts/extract_ps2_ddr_data.py` does the extraction:
  - it reads the TOC and extracts every entry, plus the unreferenced sector ranges;
  - optionally it unpacks nested tables and Bemani LZ, and converts TCB → PNG and
    Svag → WAV;
  - it copies the dancer rig out of the ELF;
  - its `find-toc` subcommand locates the table when bringing up another game.
- `scripts/test_ps2_ddr_formats.py` holds the host tests. `scripts/validate_ps2_ddr_tools.sh
  [extracted-dir]...` runs them and surveys an extraction.

**Sources.**
- The disc rip is at `~/Desktop/PS2 DDR ISOs/Dance Dance Revolution Strike (Japan)/`
  (SLPM_662.42, JP, VER 1.02), with the ISO beside it. The extraction is in
  `extracted_full/` there.
- Ghidra program `ddr_strike` (R5900, ELF image base 0x100000).
- RhythmCodex (`Source/RhythmCodex.Lib/Games.Ddr.Ps2`, `Compressions.BemaniLz`,
  `Graphics.Tcb`, `Sounds.Vag`; MIT) and root670's ddr-tools (`filedata-tool.py`,
  `tcb-convert.c`; MIT).

**Address convention.** Code and data addresses are ELF virtual addresses, as Ghidra shows
them. The file offset in SLPM_662.42 is `VA − 0x100000 + 0x180`. The Ghidra program now
names `FUN_001939a0` `FindTocEntryById`, `FUN_00193c30` `GetTocEntrySectorRange` and
`FUN_00191b70` `SetArchiveBaseLba`. It also labels the tables below `g_filedata_toc`,
`g_filedata_base_lba`, `g_chara_lst_by_type`, `g_chara_lst_28`, `g_chara_pos`,
`g_chara_parent` and `g_dance_routine_names`.

## 1. Disc layout

| File | LBA | Sectors | Span sector |
|---|---|---|---|
| `DATA/FILEDATA.BIN` | 4000 | 0x8001C | 0 |
| `DATA/FILEDT02.BIN` | 528316 | 0x800D2 | 0x8001C |
| `DATA/FILEDT03.BIN` | 1052814 | 0x156B1 | 0x1000EE |
| `DATA/DATA0{0,1,2,3,4,6}.BIN` | — | — | `MWo3` code overlays (`info`, `les`, `workout`, `rec`, `opt`, `dmm` `_rel4cd.prg`), loaded at 0x840000; not part of the archive |

- `FUN_0011dd50` hard-codes FILEDATA's LBA with `FUN_00191b70("filedata.bin", 4000)`, which
  stores it at 0x70B66C.
- Every read (`FUN_00192150`, `FUN_001922a0`) adds a TOC sector offset to that base.
- The three files are adjacent on the disc. Offsets past FILEDATA's end therefore land in
  FILEDT02 / FILEDT03.
- No TOC entry straddles a file boundary.

## 2. The TOC

- **Location:** VA 0x2CB668: 2951 entries, then a `0xFFFF` terminator.
- **Entry:** `u64 v`, with `id = v & 0xFFFF`, `sector = (v >> 16) & 0xFFFFFF` and
  `sectors = v >> 40`. `FUN_00193c30` decodes it this way.
- **Lookup:** `FUN_001939a0` binary-searches the table by `id & 0x7FFF`. The table pointer
  and count are read from 0x70B6B0 / 0x70B6B4; the store that sets them was not traced.
- **Handles:**
  - A file handle is either a raw id or a pointer to a TOC entry.
  - `FUN_00193ae0` and its siblings dispatch on `(handle & 7) != 0`, which means a raw id.
    Consequently **no id is a multiple of 8**; the ids skip 0x8, 0x10, …, 0xD30.
  - Game code mostly passes entry pointers. For example, the pointer table at 0x289B20 holds
    per-song entries.
- **Names:** there are none. A variable-length name table is supported (0x70B6BC /
  `FUN_00193710`: `{u16, u16 id, char name[]}`), but STRIKE never fills it.
- **Coverage:** the entries plus 4 unreferenced ranges (3850 sectors) cover the span
  exactly, with no overlaps.
  - The ranges follow ids 0xC1A, 0xCFB, 0xD33 and 0xD3B. The middle two sit where ids are
    missing (0xCFC–0xCFF, 0xD34–0xD3A): 0x114D8A holds 6 tables like 0xD3B's, one per
    missing id.
  - The extractor writes them to `hidden/`. Each range is split where a payload states its
    own size: a table's extent, a TCB total, an MPEG end code or an LZ end.
- **Other games:** root670's `filedata-tool.py` reads the same entry layout at per-ELF
  offsets for MAX JP/US, MAX 2, EXTREME JP and Party Collection. `find-toc` scans for the
  shape: ascending ids, in-span, contiguous, terminated. On STRIKE it ranks 0x2CB668 first
  (the only terminated run).

## 3. Payload formats and STRIKE's inventory

**Leaf formats:**
- **Svag:** `{ "Svag", data_size, rate, channels, interleave }`, then PS-ADPCM from 0x800 in
  `interleave`-byte channel blocks.
- **TCB:** TIM2 behind a `"TCB\0"` + 12-zero file header:
  - picture header at 0x10;
  - pixels at `0x10 + header_size + 0x10`, CLUT at `… + image_size + 0x10`; each section
    has a 16-byte GIF-tag prefix;
  - byte 0x22 is the image type (4 = 4 bpp, 5 = 8 bpp);
  - 256-colour CLUTs are CSM1-swizzled (8–15 ↔ 16–23 of every 32);
  - alpha 0x80 = 1.0.
- **Bemani LZ:** the codec of `extract_sys573_data.decode_lz`. A copy that reaches before
  the output start reads zeros; one STRIKE stream relies on that.

**Containers.** The game picks each file's parser in code; the extractor recognizes them by
structure (`--unpack`):

| Layout | Shape | Seen in |
|---|---|---|
| pairs | `{off, size}` × n, n = min(off)/8, slots unsorted, (0,0) empty | UI packs 0xC91–0xC9F, 0xCB2–0xCE4, 0xD2C, 0xD3B, hidden 0x114D8A… |
| offsets | `off` × n, n = min(off)/4, slots unsorted, 0 empty (RhythmCodex's "unbound table") | motion sets 0xCA9–0xCB1, sprite sets 0xD17–0xD27, 0xCEE/0xCF1… |
| counted | `n, off[n+1]` | 0xD01–0xD16, the inner table of 0xCFA/0xCFB |
| lzseq | LZ streams on a fixed 0x400 stride | 0xCE5 / 0xCE6 (760 frames each) |

**Inventory:**

| Ids | Content |
|---|---|
| 0x0001–0x0C8F | 57 Svag-led groups: songs with an LZ-compressed TCB banner (52) and 0–148 MPEG-1 320×240 background clips (2701 in all, abstract visuals); 0xC8B–0xC8F have no clips |
| 0x069E | An 87 MB `TYOSD v-2.00` sound bank, built from SCEI HD/BD-style chunks (`IECSVers`, `IECSHead`, `IECSVagi`, …); not decoded |
| 0x0C91–0x0C96 | System / UI texture packs; 0xC92–0xC96 are five near-identical variants |
| 0x0C97–0x0C9F | Menu texture packs |
| 0x0CA1–0x0CA7 | Attract Svag + MPEG-1 448×336 movies (pre-rendered characters) |
| 0x0CA9–0x0CB1 | **8 dancer motion sets** (§4.3) |
| 0x0CB2–0x0CE4 | **45 dancers**: `{TCB 192×256 8bpp, .cmd}` pairs (§4.1) |
| 0x0CE5–0x0CE6 | 2D dancer sprite sequences, 120×176, 760 frames each (lesson / workout) |
| 0x0CE7–0x0CFB | Mode UI; 0xCF2 re-bundles 44 of the dancers + 5 motion sets, byte-identical |
| 0x0D01–0x0D16 | 20 × {640×448 background, 2nd texture} |
| 0x0D17–0x0D27 | 90-frame TCB reveal animations (awards / trophies) |
| 0x0D29–0x0D3B | Misc UI; 0xD2F is "Save Edit Library" data |

Unrecognized after unpacking:
- 0x069E (above), 0xCEC, 0xCED (a 1-sector colour ramp), 0xCF9 / 0xD31 (`PS2D`-tagged
  pairs), 0xD2F;
- the tail of hidden range 0x110E18 (1413 sectors);
- the 177-entry table at slot 0 of 0xC92–0xC96.

## 4. The dancers

### 4.1 Meshes

- Member 1 of 0xCB2–0xCE4 is a System 573 `.cmd` (docs/sys573_dancers_research.md §2):
  - 8 zero bytes, then `nobj = 28`;
  - `{offset, nsub, 0x1000}` per object;
  - `0x34` textured sub-meshes of int16 vertices and normals.
- `sys573_dancer_dump.parse_cmd` parses all 89 copies unchanged (`validate_ps2_ddr_tools.sh`).
  There are 38 distinct meshes. Seven are shared by two ids that differ only in the texture:
  0xCC6/7, 0xCC9/A, 0xCD2/3, 0xCD4/5, 0xCD6/7, 0xCDF/0xCE1 and 0xCE2/3.

### 4.2 Textures

- Member 0 is an 8 bpp **192×256** TCB.
- The `.cmd` UVs address a 256×256 page with the texture at its left edge. A rest-pose render
  with the TCB placed at (0, 0) of an empty 256² page textures correctly (checked on 6
  dancers).

### 4.3 Rig and motion

**Rig** — data in the ELF, in 573's file layouts. The extractor copies it to `elf/`:
- `chara.lst` (VA 0x296AF0, 57 bytes) is 573's 28-object table byte for byte: joints 13/14/15
  own 5 objects each.
  - `FUN_001af050` picks `0x296AB0 + type * 0x40`.
  - Type 0 (`chara20.lst`, 0x296AB0) is a 20-object variant with one hand shape per hand.
- `chara.pos` (VA 0x296CB0, 17 × int16 xyz): `FUN_001b21d0` reads from `+6`, skipping entry
  0 as 573 does.
  - Values: hips (0, −1061, 0), thighs (±77, 128, −24), and so on. It is PSX Y-down and
    symmetric.
  - `parse_pos` reads it unchanged.
- The parent table at VA 0x296C60 is 18 ints `[−3, −2, −1, 0, 1, 2, 0, 4, 5, 0, 7, 8, 7, 10, 7,
  9, 11, 12]`, i.e. 573's `PARENT` behind an anchor and root.

**Motion** — the 8 sets are offsets tables of 17 routine slots. The slot names are the
pointer table at VA 0x296B70 → strings at 0x2B0900:

| Slot | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15 | 16 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Routine | normal | capoera1 | hiphop1 | hiphop2 | hopping1 | jazz1 | jazz2 | lock1 | mhouse1 | n31 | sino_ | soul1 | soul2 | thouse2 | thouse3 | y11 | y31 |

- Every set has `normal` (9 clips: `normal_m5 … normal_00`) plus 4 routines of 13–14
  lettered clips.
- The per-routine phrase-letter strings follow at 0x2B0AF0 (`abcde`, `efghi`, …, `ijkln`).
- The container and clip header match 573: `'S', n, 8, {name, clip}`; clip
  `{size, ntracks = 17, last = 1920, → track table}`.
- **The binary layout below the clip is the PS2's own.** The semantics are 573's:
  - **Track:** STRIKE drops the size word:
    `{u8 index, u8, u8 has_translation, u8, u32 nchan, u32 → channel table (0x0C)}`, whereas
    573 has `{…, u32 size, u32 nchan, u32 → table}`. `cmm_layout` tells them apart by the
    word at +8.
  - **Channel:** `{u8 type, 3 pad}` with its key block inline at +4. 573 stores a pointer at
    +8 instead.
  - **Key block** (sampler `FUN_001b3030`): `u16` value shift, then 8 `u16` segment offsets
    indexed by `t >> 8`, then 2 unused bytes, then the `(u16 time, s16 value)` keys at +0x14.
    573 keeps the value shift at +6, computes the segment shift, and puts the offsets at
    +0x16 with a stride of 4. Interpolation and truncating division are the same.
  - **Evaluator** (`FUN_001b3140`): channel types 0/1/2 rotation, 6/7/8 translation (only
    when `has_translation`), 10 selector (+1 on the head). `FUN_001b2d00` builds
    `R = Rx·Rz·Ry` from 4096-per-turn angles. `t` wraps past `last`. It is 573's
    `FUN_8003c420` rule for rule.
  - Every clip is Euler. The first channel is never type 1, so the evaluator's second branch
    (`FUN_001b2e40`) is unused.
- The 8 sets hold each routine one to three times; the copies differ only in trailing padding,
  and every joint agrees to 0 mm. Rendered poses (hiphop1, capoera1 with its handstand) are
  correct, and in the normal clips the hips sit between −1111 and −828 mm.

### 4.4 The character table

At VA 0x2B0410 there are 45 records of 0x1C bytes, in dancer-id order (record k is the k-th
id of 0xCB2–0xCE4):

| Offset | Field | Getter / use |
|---|---|---|
| +0x00 | f32 scale | `FUN_001ae980`. `FUN_001aef40` stores `scale × 0.01`. `FUN_001b1430` multiplies every joint's world translation by it, and `FUN_001adda0` divides the floor position by it, i.e. a uniform character scale (1.0; RAGE 0.97/0.98, P-ZUKIN 0.93, YUNI 0.96, ROBO 0.98, NAOKI 1.01, AKIRA 1.06, BABY-LON **0.4**) |
| +0x04 | f32 | `FUN_001ae860` → `FUN_001b1010` scales two draw parameters by it (0.55–0.75; likely the outline) |
| +0x08 | u32 flags | bits 0–1 = `chara.lst` type (always 1 here); bit 2 set on costume 1 |
| +0x0C | ptr | → a 0x40-byte block, or a runtime buffer for the first 8 (`FUN_001ae960`) |
| +0x10 | ptr | its first byte is read by `FUN_001ae8d0` |
| +0x14 | ptr | 16 × 16-byte per-joint entries (`FUN_001ae880(k, joint)`) |
| +0x18 | char * | the name |

The names, in id order: BLUES, RHYTHM, DRUM, BASS, RAGE, EMI, ASTRO, CHARMY, ALICE, BABY-LON,
BOLDO, TRACY, JENNY, JOHNNY, PRINCESS-ZUKIN, ROBO2001, NAOKI, LADY, AKIRA, YUNI, J.C., SA-JA.
Each comes as `<NAME>1` / `<NAME>2`, the two costumes; RHYTHM3 (gold) is last.
- Several match 5thMIX / 4thMIX ports by texture: RAGE = Rage, EMI = 3rdMIX Onna,
  ASTRO = Spacem B, CHARMY = Spacef B, ALICE = Hongkong, BABY-LON = QP, JOHNNY = John,
  PRINCESS-ZUKIN = Zukin A, ROBO2001 = Robo, NAOKI = Naoki, AKIRA = Xman, YUNI = Yuni.
- BLUES, RHYTHM, DRUM, BASS, BOLDO, TRACY, JENNY, LADY, J.C. and SA-JA match no ported
  3rd–5thMIX dancer.

**Port** (`port_character_strike.py`):
- The textures get binary alpha. The game's palette alpha is inconsistent: NAOKI1 is 0x40
  everywhere.
- The table's scale goes into the sidecar's model_scale column.
- The worst joint error over 45 × 16 clips is 0.11 mm, and a re-run is byte-identical.

## 5. RhythmCodex vs. this tool

- RhythmCodex's PS2 DDR support is heuristic, and it lives only in `[Explicit]` integration
  tests: there is no CLI command for it.
  - It scans FILEDATA sector by sector for `Svag` headers and TCB magic.
  - It finds "unbound" LZ tables by the smallest-offset rule, and finds step tables by
    SSQ-shaped first entries.
- The TOC replaces all of that for cutting files. The extractor ports only the parts that
  decode payloads:
  - Bemani LZ;
  - the TCB layout and CSM1 filter (with ddr-tools' pixel/CLUT offsets);
  - the Svag header rules;
  - the unbound-table count rule, generalized to unsorted slots and empty slots.
- RhythmCodex's `VagStreamReader` never copies data into its output buffer. The ADPCM decoder
  here is written from the PS-ADPCM definition.

## 6. Reproduce

```bash
G=~/Desktop/"PS2 DDR ISOs/Dance Dance Revolution Strike (Japan)"
python3 scripts/extract_ps2_ddr_data.py find-toc "$G/SLPM_662.42" "$G"/DATA/FILEDATA.BIN "$G"/DATA/FILEDT0{2,3}.BIN
python3 scripts/extract_ps2_ddr_data.py extract strike_jp "$G" "$G/extracted_full" --unpack --png   # ~30 s, 2.4 GB
./scripts/validate_ps2_ddr_tools.sh "$G/extracted_full"    # 89 dancer meshes, 45 motion sets, 0 problems
```

A re-run is byte-identical: both manifests and the whole `unpacked/` tree match.
