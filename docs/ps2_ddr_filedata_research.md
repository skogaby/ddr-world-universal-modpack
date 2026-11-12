# PS2 DDR Archives (FILEDATA, DAT) and DDR STRIKE Assets — RE Notes (2026-09-29, 2026-09-30, 2026-10-03)

**Question.** How are the assets of the PS2 DDR games stored? The goal is to extract them
exactly, not by scanning for known headers. What does DDR STRIKE contain on the way to
porting its 3D dancers? (2026-09-30: and how do Party Collection, Festival, SuperNova,
SuperNova 2, X and X2 differ?)

**Answer.**
- **The archive has an exact file table (TOC) in the ELF.** It is a sorted array of 8-byte
  `{u16 id, u24 sector, u24 sectors}` entries over one sector space. FILEDATA.BIN,
  FILEDT02.BIN and FILEDT03.BIN form that space back to back. No heuristics are needed to
  cut the archive into files (§1–§2).
- **Party Collection and Festival use the same FILEDATA layout** without the `0xFFFF`
  terminator, and carry the same System 573 dancer rig as STRIKE (§6). **All 86 dancers are
  ported** (2026-10-03, §6.1): Festival's 26 as `dancers/DDR FESTIVAL/`, Party Collection's 60
  (the cast of 1st..7thMIX and the CS mixes) as `dancers/DDR PARTY COLLN/`. Neither disc has
  any 3D stage: the gameplay backgrounds are IPU movies. Not yet cabinet-tested.
- **SuperNova .. X2 replaced FILEDATA with four named archives** (SYSTEM / IMAGE / SOUND /
  MDB `.DAT`), each with its own table in the ELF: names, exact sizes, dates and a byte-sum
  checksum per file. All 8954 files of the four discs verify (§7).
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
  - **All 45 are ported** as `data_mods/custom_models/dancers/DDR Strike/<Name> <n>`
    (`tools/blender_ddr_addon/examples/port_character_strike.py`). They are not yet
    cabinet-tested.
- The song backgrounds are MPEG-1 clip sets. The attract movies show pre-rendered
  characters, and the lesson / workout sprites are 2D frame sequences. None of those is
  geometry.
- **SuperNova's dancers and stages are `.TZM` packs of a new (XSI-exported) engine** (§7.4):
  skinned strip meshes with up to three weights on a 22-joint HumanIK skeleton, 30 Hz
  quaternion routines, coloured static stage meshes with SRT / material tracks. Container,
  textures, MODEL, MOTION and the skinning rule are decoded (`scripts/tzm_dump.py`), and
  **all eight dancers are ported** with their own routines
  (`tools/blender_ddr_addon/examples/port_character_supernova.py`) and **all 20 stages** with
  their layers and object animation (`port_stage_supernova.py`); shipped under
  `data_mods/custom_models/{dancers,stages}/DDR SUPRNVA 1+2/` (cabinet-validated 2026-09-30).
- **SuperNova 2 reuses the engine, the routines and the stages** (§7.5): the 29 clip packs and
  the 20 stage packs are byte for byte SuperNova's (+ one new `system_bg002`). Its dancers are
  12 characters × 2 costumes whose costume 01 for the returning eight is the SuperNova skin
  minus the face — the eyes / mouth moved to per-skin `_face.TZM` expression masks hung off
  the Head. The **16 new dancers** (every costume 02 + YUNI, ALICE, CONCENT, JULIO) are ported
  with the neutral mask joined to the body (`GAME=sn2`, 2026-10-03), merged into the same
  source. Not yet cabinet-tested.
- **X and X2 reuse it all again** (§7.6): the routines, SuperNova 2's skins as their costumes
  02 / 03, one new costume per character in each game (X's 01, X2's recolour 02) plus BONNIE,
  ZERO, a second BABY-LON and X2's four PIX pigs; six stages of X's own, one of them with
  render-target TV screens. The **33 dancers** the two games add and the **6 stages** are
  ported (`GAME=x` / `x2`, 2026-10-03) into one `DDR X + X2` source, the TVs textured
  `offscreen1` so World's STAGE SCREENS mode plays the song's movie on them. Not yet
  cabinet-tested.

**Tools.**
- `scripts/extract_ps2_ddr_data.py` does the extraction, for STRIKE and the six games of
  §6–§7 (`GAMES` there):
  - it reads the TOC / DAT tables and extracts every entry, plus the unreferenced sector
    ranges;
  - optionally it unpacks nested tables, Bemani LZ, TGCD and FrameInfo, and converts
    TCB / TIM2 / TGCD → PNG and Svag / VIG → WAV;
  - it copies the dancer rig out of the ELF;
  - its `find-toc` subcommand locates the table(s) when bringing up another game.
- `scripts/tzm_dump.py` decodes the SuperNova-engine `.TZM` packs (§7.4): `info`, `png`,
  `preview` (a software render of a skin at rest or at a clip frame) and `survey`.
- `scripts/test_ps2_ddr_formats.py` and `scripts/test_tzm_dump.py` hold the host tests.
  `scripts/validate_ps2_ddr_tools.sh [extracted-dir]...` runs them and surveys an extraction
  (573-format meshes and clips, every `.TZM`, the checksums).

**Sources.**
- The disc rip is at `~/Desktop/PS2 DDR ISOs/Dance Dance Revolution Strike (Japan)/`
  (SLPM_662.42, JP, VER 1.02), with the ISO beside it. The extraction is in
  `extracted_full/` there.
- The six later rips are beside it in `~/Desktop/PS2 DDR ISOs/` (§6–§7 give the ELF of each).
- Ghidra programs `ddr_strike`, `ddr_party_collection`, `ddr_festival`, `ddr_supernova`,
  `ddr_supernova_2`, `ddr_x`, `ddr_x2` (R5900, ELF image base 0x100000).
- RhythmCodex (`Source/RhythmCodex.Lib/Games.Ddr.Ps2`, `Compressions.BemaniLz`,
  `Graphics.Tcb`, `Sounds.Vag`; MIT), root670's ddr-tools (`filedata-tool.py`,
  `tcb-convert.c`; MIT) and vgmstream's `vig_kces.c` (the VIG header).

**Address convention.** Code and data addresses are ELF virtual addresses, as Ghidra shows
them. The file offset in SLPM_662.42 is `VA − 0x100000 + 0x180`. The Ghidra program now
names `FUN_001939a0` `FindTocEntryById`, `FUN_00193c30` `GetTocEntrySectorRange` and
`FUN_00191b70` `SetArchiveBaseLba`. It also labels the tables below `g_filedata_toc`,
`g_filedata_base_lba`, `g_chara_lst_by_type`, `g_chara_lst_28`, `g_chara_pos`,
`g_chara_parent` and `g_dance_routine_names`.

The six later ELFs share the image base, with the segment at file offset 0x80 (PC, SN, SN2),
0x200 (Festival) or 0x180 (X, X2); `extract_ps2_ddr_data.va_to_offset` reads it from the
program header. Their Ghidra programs now name `InitializeFiledata`,
`SetArchiveBaseLba` and `g_filedata_toc` / `g_chara_*` (PC, Festival); `MountDatArchives` and
`g_dat_table_system` / `_image` / `_sound` / `_mdb` (SN, SN2, X, X2); `CheckTgcdHeader` and
`DecompressTgcd` (SN); `OpenDatFileByIndex` and `GetLanguageIndex` (X2). SuperNova's
character table (§7.4) is at VA 0x3A5260, its motion name list at 0x3A5B8D; SuperNova 2's
(§7.5) at 0x3D2D90 / 0x3D3480 with the same 29-name list at 0x3D4152.

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
| +0x04 | f32 | `FUN_001ae860` → `FUN_001b1010` scales two draw parameters by it (0.55–0.75); the same engine's debug info in Party Collection names it `get_shadow_bri` (§6.1) |
| +0x08 | u32 flags | bits 0–1 = `chara.lst` type (always 1 here); bit 2 set on costume 1 |
| +0x0C | ptr | → a 0x40-byte block, or a runtime buffer for the first 8 (`FUN_001ae960`) |
| +0x10 | ptr | its first byte is read by `FUN_001ae8d0`; the motion-set list (`get_dancer_default_mot`, §6.1) |
| +0x14 | ptr | 16 × 16-byte per-joint entries (`FUN_001ae880(k, joint)`; `get_edge_info`, §6.1) |
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
- RhythmCodex's integration tests also open SuperNova 2's `MDB_SN2.DAT` / `IMAGE.DAT`, but
  scan them the same way. §7's tables make that unnecessary too.

## 6. Party Collection and Festival (FILEDATA without a terminator)

| Game | ELF | Base LBA | TOC (VA, entries) | Span (sectors) | `GAMES` key |
|---|---|---|---|---|---|
| Party Collection (JP, 2003, VER 1.01) | SLPM_624.27 | 0xC00 | 0x2A14C8, 459 | 0x384A3, FILEDATA.BIN only | `party_collection_jp` |
| Festival (JP, 2004, VER 1.01) | SLPM_657.75 | 4000 | 0x2A2098, 1105 | 0x8CF17, FILEDATA.BIN only | `festival_jp` |

- **Base LBA.** As in STRIKE, one call hard-codes FILEDATA's LBA:
  `InitializeFiledata` (PC 0x112B70, Festival 0x1163F0) calls
  `SetArchiveBaseLba("filedata.bin", LBA)` (PC 0x205E10, Festival 0x1F9C20).
- **No terminator.** The entry after the last one is other data (`id 0x3C`, sector
  0x3C0040, 0 sectors in both games), not `0xFFFF`.
  - Neither game looks files up by id. Code and data hold pointers to entries: PC's table at
    0x2891C8 points at them, and Festival's `FUN_0025a240` loads the first file with
    `FUN_001163e0(&toc[0], buf)`.
  - The data pointers into the table all target entries: 538 in PC (indexes 0–458), 1113
    in Festival (0–1103).
  - The extractor's `terminated: False` checks the table ends where the ascending, in-span
    id run ends, so `toc_count` can't be too small. `find-toc` ranks a run that tiles the
    span exactly next after terminated ones; PC's does, and Festival's is the longest run.
- **Coverage.** PC's entries tile the span exactly. Festival leaves 2 ranges unreferenced:
  0x86852 (186 sectors, a bare `ipum` stream) and 0x86C70 (38 sectors, an offsets table).
- **Video is IPU**, not MPEG-1 as in STRIKE: `{u32 header_size, "FrameInfo\0\0\0", ...}`
  (a frame index), then an `ipum` stream (`"ipum", u32 size, u16 w, u16 h, u32 frames`)
  that ends exactly at `header_size + 8 + size`. ddr-tools calls these "MPEG2". The
  extractor names them `.fipu` and `--unpack` writes the bare `.ipu`.
- **Dancers.** Both have the System 573 rig in the ELF, byte-identical to STRIKE's
  (`chara.lst`, `chara20.lst`, `chara.pos`). PC: `FUN_0023f530` picks the object table
  from 0x28BED0 + type × 0x40, `FUN_002424e0` reads the rest offsets from 0x28C0D6;
  Festival: `FUN_002172a0` / `FUN_0021a590` with 0x28A940 / 0x28AB46. The extractor copies
  them to `elf/`.
  - PC: 120 `.cmd` meshes (108 with 28 objects, 12 with 20: the `chara20.lst` type) and 45
    motion sets (553 clips). `sys573_dancer_dump.parse_cmd` now accepts the 20-object form.
  - Festival: 52 meshes (all 28 objects) and 45 motion sets.
- **Inventory** (manifest types after `--unpack`):

| Type | PC | Festival |
|---|---|---|
| IPU behind FrameInfo | 259 (+1 bare) | 866 (+1 bare, +1 hidden) |
| Svag | 49 | 79 |
| TCB (LZ / raw) | 49 / 7 | 84 / 6 |
| pairs / offsets / lzseq tables | 65 / 18 / 4 | 36 / 22 / 4 |
| unknown | 7 | 9 |

- `DATA/SOUND/SD.BIN` is outside the archive and not extracted: in PC it starts with an
  `Svag` header (plus `DDRPC.HD` / `.BD` / `.TD` beside it), in Festival it is a
  `TYOSD v-2.00` bank like STRIKE's 0x069E.

### 6.1 The dancers and their port (2026-10-03)

**Files.** Each dancer is a `pairs` table `{TCB 192×256 8 bpp, .cmd}` as in STRIKE. The motion
sets are `offsets` tables of 17 routine slots, the same 16 routines plus the `normal` idles:

| | PC | Festival |
|---|---|---|
| Dancers (ids, minus multiples of 8) | 0x134–0x177 (60) | 0x3DF–0x3FC (26) |
| Motion sets | 0x12B–0x12F, 0x131–0x133 | 0x3D6, 0x3D7, 0x3D9–0x3DE |
| Re-bundle (byte-identical, ignored) | 0x179: all 60 + set 0x12B | 0x4B6: all 26 + set 0x3D6 |
| Character table (VA) | 0x29E270, 60 records | 0x29ED80, 26 records |

Neither ELF refers to a re-bundle's TOC entry; the per-set pointer tables (PC 0x28B630,
Festival 0x28A6E0) point only at the eight sets.

**Debug info.** Both ELFs keep a DWARF 1 `.debug` section (`.line` too): names, source files and
`low_pc` / `high_pc` of 328 (PC) / 1100 (Festival) functions, e.g. the dancer engine under
`E:\DDR\ddrb2\src\AM\s573\doll\*.c` (PC; Festival `ddrf`). Each record is `{u32 length, u16 tag,
attributes}`, an attribute `u16 (name << 4 | form)`. Subroutines (tags 0x06 / 0x0A) carry
`AT_name` (0x03, string), `AT_low_pc` (0x11) and `AT_high_pc` (0x12). The Ghidra programs
don't carry these names yet. The names below come from the section.

**Character table.** STRIKE's 0x1C-byte record (§4.4), record k = the k-th dancer id. Its getters
in `doll/ddata.c` (PC VAs) give each field a name:

| Offset | Getter | Meaning |
|---|---|---|
| +0x00 | `get_dancer_scale` 0x23EF70 | the uniform scale (§4.4) |
| +0x04 | `get_shadow_bri` 0x23EE50 | the floor shadow's brightness (`draw_dancer_model` × 1.27); STRIKE's "likely the outline" field |
| +0x08 & 3 | `get_chara_lst_id` 0x23EE20 | 1 = `chara.lst`, 0 = `chara20.lst` (`dancer_model_setup`) |
| +0x08 bit 2 | `get_chara_hide` 0x23EDF0 | `ps2dancerInitHide` sets a bitmask bit per flagged dancer (the hide / unlock semantics were not traced). Set on PC's six 1stMIX characters and on Festival's costume 1s |
| +0x0C | `get_primdt_tbl` 0x23EF50 | the per-object primitive table |
| +0x10 | `get_dancer_default_mot` / `rnd_get_dancer_mot` | 4 motion-set indices (values 1–7; the base was not traced): the default is byte 0, otherwise one is drawn at random per song |
| +0x14 | `get_edge_info` 0x23EE70 | 16 per-joint `{f32 x, y, z, u32 enable}` (0.87–1.09): the toon edge (outline) of each joint |

- Six PC dancers are type 0 and their meshes have 20 objects: KONSENTO:01, SPACE MAN,
  KONSENTO:02, TAMAKO, OSHARE-ZUKIN, KAERU-ZUKIN.
- The motion-set lists give the sex. `01010103` .. `04040402` are the male sets and `05050505` /
  `05050606` / `07070705` the female ones: PC records 0–30 male, 31–59 female; Festival BLUES /
  DRUM / DISCO / RAGE male. Every port still gets all 16 routines, as STRIKE's did.
- The shadow brightness and the edge info are draw style; World draws its own shadow and
  outlines, so the port does not carry them.
- Names: PC `AFRO(1st)` .. `BUS(7th)` (`<NAME>(<mix>)`, CS = the console mixes, 7th = MAX2,
  plus `EMI(unpublished)`); Festival `BLUES1` .. `EMI3` (eight characters × 3–4 costumes).

**Overlap with other sources.** Fingerprinting every port's vertex buffer and `.dds` gives:
- **Festival reuses PC's dancers.** DISCO 1/2/3 = PC's AFRO 1st / 2nd / 5th, EMI 1/2/3 = EMI
  3rd / EMI(1) 4th / EMI 5th, RAGE 1/2/3 = RAGE 3rd / 4th / 5th, LADY 1 = LADY 1st (mesh and
  texture identical).
- **Festival to STRIKE.** Festival's LADY 3 is STRIKE's LADY1 exactly. BLUES 3 and DRUM 2 are
  STRIKE's BLUES1 / DRUM1 meshes in other colours.
- **PC to the arcade ports.** Several PC dancers wear an arcade 573 texture on a different
  mesh: Akira 4th = 4thMIX Xman, Astro 4th = 4thMIX Space M B, Charmy 4th = Space F B,
  Konsento 4th = Robo B, Rage 4th = Yaro B, Baby-Lon 5th = STRIKE Baby-Lon 1.

All are shipped anyway: each source carries its game's whole cast, as STRIKE did beside the
3rd–5thMIX ports.

**No stages.** Both games draw the polygon dancers over the song's IPU background clips (PC
259, Festival 866).
- A structural scan of every extracted file for `.cmd`-shaped data finds exactly the 120 / 52
  dancer meshes.
- The debug info names no stage or set-piece module. The only other geometry is `draw_floor`, a
  static in `doll/dancer.c` (PC 0x23E3C0, called from `ps2dancerDispStep`). It is procedural: a
  7×7-vertex grid of 5-unit cells that follows the dancer, faded by distance from it and scaled by
  its `bri` (brightness) argument. No asset is involved, and World draws its own floor and shadow.
- The other unrecognized payloads are 2D: `PS2D`-tagged layouts, step charts, a CSV, and two
  large packed blobs (PC 0x1AF, Festival 0x4AF) that are neither Bemani LZ nor mesh-shaped.

There is nothing to port as a World stage.

**Port** (`port_character_strike.py GAME=festival|pc`): the STRIKE / 573 pipeline unchanged,
except that 20-object meshes take `chara20.lst` (a 21-bone rig: root, the 16 joints and the 4
face helpers; one hand shape per hand, so no hand helpers).
- Labels: Festival `<Name> <n>` (`Blues 1`), PC `<Name> <mix>` (`Afro 1st`, `Space Man CS1st`,
  `Emi Unpublished`). Shortened: OSHARE-ZUKIN → O-Zukin, PRINCESS-ZUKIN → P-Zukin,
  KONSENTO:03/2 (4th) → Konsento 4th.
- Keys: `fest<name><n>00` / `pc<name><mix>00`. Model scale is the table's.
- The source folder is `DDR PARTY COLLN`: the requested `DDR PARTY COLL.` ends in a period,
  which Windows cannot store.
- The worst joint error over all 86 × 16 clips is 0.11 mm. Every dancer re-imports and renders.
  The script checks its hard-coded tables against the ELF beside the extraction (names, scales,
  sex lists, object table type).

## 7. SuperNova .. X2 (DAT archives)

| Game | ELF | Files (SYSTEM / IMAGE / SOUND / MDB) | `GAMES` key |
|---|---|---|---|
| SuperNova (JP, 2006, VER 1.02) | SLPM_666.09 | 15 / 1134 / 411 / 176 (`MDB_SN1`) | `supernova_jp` |
| SuperNova 2 (JP, 2007, VER 1.01) | SLPM_669.30 | 16 / 1230 / 423 / 191 (`MDB_SN2`) | `supernova2_jp` |
| X (JP, 2008, VER 1.02) | SLPM_550.90 | 16 / 1863 / 645 / 179 (`MDB_X1`) | `x_jp` |
| X2 (US, 2009, VER 1.00) | SLUS_219.17 | 14 / 1858 / 643 / 140 (`MDB_X1`) | `x2_us` |

### 7.1 Mount and tables

- `MountDatArchives` (SN 0x1C2250, SN2 0x189F10, X 0x187320, X2 0x18BF10) fills archive
  k's slot (0x125C bytes each) with a `(count, table)` pair from the ELF, then names it
  `"<NAME>.DAT"` and `"<NAME>/"` under `cdrom0:\` + `~DATA\`. Files are opened by name, so
  no LBA is hard-coded.
- The tables (`g_dat_table_system` / `_image` / `_sound` / `_mdb` in Ghidra):

| Game | SYSTEM | IMAGE | SOUND | MDB |
|---|---|---|---|---|
| SN | 0x3337A0 | 0x333A70 | 0x33FD90 | 0x34ACE0 |
| SN2 | 0x35F110 | 0x35F400 | 0x36C7A0 | 0x3711A0 |
| X | 0x2CF3B0 | 0x2CF6A0 | 0x2E3710 | 0x2EA620 |
| X2 | 0x28CB40 | 0x28CE20 | 0x2A2AC0 | 0x2AA390 |

- **Table:** `u32 count`, then `count` entries of 11 `u32` (X2: 12):
  `index, type, [variant], size, sector, char *name, byte_sum, year, month, day, hour, minute`.
  - `size` is exact bytes; files start on sectors.
  - `byte_sum` is the byte sum of the file mod 2³²: all 8954 files match.
  - `index` equals the position except once (SN2 IMAGE 752, `ending/usn_enlogo01.TM2c`,
    repeats 753's 0x2F1). The game indexes by position (`param_2 * 0x30 + table`).
  - `type` follows the extension: 1 `.bin`, 3 `.TM2`/`.TM2c`, 4 `.ipu`, 7 `.TZM`, 8 `.CAP`,
    9 `.vig`, 10 `.SWP`, 12 `.PTF`, 13 `.SSQ`, 14 `.cl2`, 15 `.dat`, 16 `.EIM`, 17 `.dld`,
    18 `.dtf`, 20 `.m2v`, 22 `.pak`.
- **Entry 0** is the archive header: sector 0, 0x20 bytes, empty name. The header is 12 bytes
  (`0xCC` fill in SN, three words in the later discs), then the same five date words as the
  entry. The extractor checks that date, so a table and archive from different builds are
  rejected.
- The entries tile every archive exactly: no hidden ranges on any of the four discs.
- **X2 variants.** The word at +8 is 0 or `0x80000000 | k`, k = 0..2. 660 entries (220
  triples: `ux2_` / `fx2_` / `sx2_`, or `` / `f_` / `s_`) are English / French / Spanish copies.
  `OpenDatFileByIndex` (0x18AF00) and its siblings add `GetLanguageIndex() - k` to the
  index. The extractor keeps all three and records the variant.
- `find-toc <elf> DATA/*.DAT` finds each table by its entry 0 and names the archive it
  validates against; on the four discs it finds exactly the 16 tables above.

### 7.2 Formats

- **TGCD** (every `.TM2c`: 2597 files) is a TIM2 compressed by the game's own codec.
  `CheckTgcdHeader` (SN 0x14DE70) requires `{"TGCD", out_size != 0, in_size > 0x20, 0x7FFF,
  0xFFFF, 0xFF, 0x8000}`; +0x1C is the byte sum of the stream. `DecompressTgcd`
  (SN 0x14DF10) reads 4-byte ops from 0x20:
  - `u16 a` with bit 15 set: copy the `u16 n` literal bytes that follow, padded to 16;
  - otherwise copy `u8 n` bytes from `max(0, written − 0x7FFF) + a` (byte by byte, so
    overlaps repeat), then append the literal `u8`;
  - until `out_size` bytes are written. Every file decodes to exactly `out_size`.
- **TIM2** (`.TM2`, and every decoded TGCD) is the standard Sony layout: format 0 puts the
  picture at 0x10, format 1 at 0x80 (58 files in SN2, 9 in X, 1 in X2). The type bytes are
  `pict_format, mips, clut_type, image_type`, where TCB (§3) stores `clut_type, -,
  image_type, -`. Seen: 4 bpp and 8 bpp with 32-bit CLUTs, 4 bpp with 16-bit CLUTs (e.g. X's
  arrows), 32-bit direct, one picture per file, one mip level. 256-colour CLUTs are
  CSM1-swizzled as in TCB. `.cl2` is a CLUT-only `CLT2`.
- **VIG** (every `.vig`, 2717 files) is vgmstream's `vig_kces`: `{u32be 0x01006408, 0,
  data_offset (0x800), data_size, loop_start, loop_length, rate, channels, flags,
  interleave}` then PS-ADPCM. All are 44.1 kHz, flags 0 (not encrypted), mono (1977) or
  stereo with 0x10 interleave (740). Loops are ignored.
- MPEG-2 `.m2v` (SN2 and later) and IPU `.ipu` (SN) are the song movies; written as-is.
- `.TZM` 3D model packs (magic `3B F2 94 A0`) hold the polygon dancers and stages: §7.4
  (`scripts/tzm_dump.py`).
- Named but not decoded: `.dtf` / `.dld` 2D layouts (`\x89DTF` / `\x89DLD`), `.EIM`, `.PTF`
  fonts, `.CAP`, `.SWP` (sound effects), `.SSQ` step charts, and X's 331 `stm/event/*.dat`
  story scripts.
- `--unpack` finds tables in a few `.bin` / `.pak` files by structure: SN's
  `mdb/music_dt.bin` and `mdb/csq_all.bin` split into 84 members each, and X2's 33 `.pak`
  files into 894 `.dtf` / `.dld` / TIM2 members.

### 7.3 Contents

- `MDB_*`: `MUSIC/<code>.vig` (full songs, stereo; 303–416 MB per disc), `SEL/<code>_s.vig`
  (previews), `MOVIE/*` (song movies: 6 IPU in SN, 17–21 MPEG-2 later).
- `SOUND`: `BGM`, `SE`, `VOICE*` / `X_VOICE` (announcer), `SOUNDTEST`.
- `IMAGE`: the UI (TM2c), `model/` (the TZM characters and stages, §7.4), `mdb/` (song and
  chart databases), per-mode folders (`smm`, `stm`, `dmm`, `workout`, `edit`, ...).
- `SYSTEM`: fonts and test screens.

### 7.4 TZM model packs (the 3D dancers and stages)

SuperNova's dancers and stages are `.TZM` packs under `IMAGE/model/` (185 TZMs on the disc;
the ones in `smm/`, `shop/`, `ending/` are the same format). Decoded 2026-09-30 by parsing
every TZM of the disc, rendering AFRO in its rest pose and in `FF_HH_01`, and porting all eight
dancers (`scripts/tzm_dump.py`; port `tools/blender_ddr_addon/examples/
port_character_supernova.py`). It is an XSI export: `globalSRT` roots, `DefaultLib.Material`,
`DefaultLib.Scene_Material`, XYZ Euler joints with X along the bone.

**Container.** `{u32 magic 0xA094F23B, u32 0x10000, u32 chunk_count}`, then chunk_count
directory entries of 0x5C bytes: `char name[0x40], u32 zero[4], u32 sector, u32 sectors,
u32 size`. Chunks sit on 0x800 sectors within the file. Chunk kinds: `IMAGELIST` (`{u32 hash,
u32 n}` + n × `char[0x50]` texture chunk names), one chunk per texture (named `<name>_png`),
`MATERIALLIST`, `MODEL`, `MOTION`; the `test/` packs also have `CAMERALIST` / `LIGHTLIST`.
Most chunks are a small header followed by a TGCD stream (§7.2); the offsets inside the decoded
data are relative to the chunk start WITH that header in place (MODEL 0x30, MOTION 8), i.e. the
game decompresses in place behind the header.

- **Texture chunk:** `u32 hash, char name[0x40]`, at 0x54 `u16 w, u16 h`, at 0x5C `{u32
  payload, u32 clut_bytes, u32 stream_bytes, u32 row_bytes, u32 1}`, at 0x70 a LINEAR (not
  CSM1) RGBA32 CLUT of 256 (8 bpp) or 16 (4 bpp) colours, alpha 0x80 = opaque, then the TGCD
  stream of `w × h` indices (4 bpp: two per byte, low nibble first). Dancer and stage colour
  sheets are 512² 8 bpp; the tutorial / system UI sheets are 4 bpp.
- **MODEL chunk:** a 0x30 header `{u32 hash, u32 1, (u32 count, u32 offset) × 5}` + TGCD.
  Sections: 0 = the root record (`globalSRT`, name only), 1 = objects, 2 = the mesh-slot table
  (`{i32 first_mesh, -1, 0, 0}` per object that has meshes), 3 = the mesh offset table (`u32 ×
  n`, relative to the table) and the meshes, 4 = bones. A skin's node lists (objects, bones)
  start with their own `globalSRT`; a stage's objects start with the blend-layer roots `add`
  (additive), `glo` (glow), `dec` (opaque) and `ble` (alpha) that group its parts.
  - **Node record (0xD0, name first):** `char name[0x40]`, 0x10 zero, `f32 T[4] R[4] S[4]` = the
    file's POSE, local to the parent (the skins' default stance — arms down — or, for JENNY /
    ROBOZUKIN / RUBY, the bind itself), `f32 T2[4] R2[4] S2[4]` = the GLOBAL bind pose (the
    T-pose the mesh is modelled in; its origin is 0.819 below the Hip, feet at y ≈ −8.6), then
    `i32 links[8]`: bones `{parent, first_child, next_sibling, prev_sibling, 0, 3, 0, 0}`,
    objects `{mesh_slot (−1 = none), 0x80000000, parent, first_child, next_sibling,
    prev_sibling, 0, 0}` (AFRO: `accessory` and `muffler` are children of the `AFRO` body).
    Rotations are XYZ Euler radians, `M = Rz · Ry · Rx` on column vectors (the joints' X axis
    then runs to the child: Spine → Spine1 1.000, legs 0.998). Composing the pose hierarchy
    lands within 0.03 units of T2 on the torso; the arms differ because the pose is not the
    bind. A mesh's vertices are in its object's frame: apply the object's world transform
    (stage props scale to `S = (2, 0, 2)` — flattened decals).
  - **Mesh record:** `char material[0x18]` ("DefaultLib.Material"), 0x38 zero, `i32 hdr[24] =
    {−1, format, nverts, stride, nverts × stride, 4, nverts − 2, tri_or_index_count, a, b,
    palette[9], palette_count, next_mesh, 0, 0, 0}`, the vertices, then (skinned meshes) 2 ×
    stride zero bytes of scratch. `next_mesh` chains the meshes of one object. Formats seen:
    `0x112` (stride 0x30: position, normal, uv — the foot panels), `0x11A` (0x40: + weights;
    the dancers), `0x152` (0x40: + colour; the stages), `0x15A` (0x50: weights + colour; GUS's
    glasses). Each row is a float4: position `(x, y, z, 0)`; weights `(w0, w1, w2)` + the four
    bone PALETTE indices as bytes (≤ 3 influences, sum 1, palette ≤ 9 bones); normal (unit);
    colour RGBA on a 0..128 scale (the glasses are 60 % alpha); uv `(u, v, 1, flags)` with v
    down. The vertex stream is triangle strips: vertex i kicks `(i−2, i−1, i)` unless its uv
    flags carry 0x8000 (the GS ADC bit); every strip after the first starts with two flagged
    vertices. Facing is not consistent (the GS does not cull): flip against the vertex normals
    (`consistent_winding`, never off by more than the 0.3 dot margin).
  - **Skinning** (verified: AFRO renders as a person at rest and dances in `FF_HH_01`):
    `v_world = Σ wᵢ · v · Object · Bindᵢ⁻¹ · Worldᵢ` with `Bindᵢ = (R2ᵢ, T2ᵢ)` global,
    `Object` the owning object's `(T, R, S)` (AFRO's `muffler` is authored at `T = (0, 5.5,
    −1), R = (−90°, 0, 0)` and lands on the neck only with it; the AFRO body carries `S.x =
    1.0347`), `Worldᵢ` composed down the bone links from the clip. The 8.836-unit offset between
    the bind frame and the root frame (Hip `T.y` 9.655 vs `T2.y` 0.819) is absorbed by the
    product; the standing feet come out at y ≈ 0.
  - **SCALE.** BABYLON and the `DDR_cspigs*` stand-ins have a 23rd node `SCALE` between
    `globalSRT` and `Hip` with `S = (0.6, 0.6, 0.6)` in the pose and no bind: the whole
    character drawn at 0.6 (its mesh is 21.3 units tall, AFRO's 17.5). The port folds it into
    the unit scale and drops the node.
- **MOTION chunk:** `{u32 hash, u32 nrecords}` + TGCD. `nrecords` headers of 0x7C first: `char
  name[0x50], u32 ntracks, s32 first, s32 last` (60 Hz scene frames: routines 1..1133–1395,
  tutorial −8..1823), `s32 a, s32 b, u32 0, f32 fps, f32 fps/60, u32 table + 8, u32 size, u32
  1`; then each record's track table (`u32` offsets relative to the table) and tracks, in
  record order. Track: `char name[0x50], u32 kind, u32 n, u32 flag (2 = one static key), u32
  nbytes, u32 nkeys`, keys. Character clips (24 tracks): kind 2003 = local rotation quaternion
  `(x, y, z, w)`, 2004 = local translation `(x, y, z, 0)`, one key per 1/fps at 30 fps (scene
  frames first, first + 2, …; the last key repeats the first on the n + 1-key clips), the Hip
  has both, every other joint 2003 only, `globalSRT` a static identity. Joints without a track
  keep the file pose's T. Stage / camera records use kinds 0..8 (3 and 4 = `S(3) Q(4) T(3)`
  keys, 5 / 6 = camera position / interest, 7 = FOV, 8 = roll) and material tracks (500..,
  1302, below) at 60 or 29.97 fps (240 keys either way: a 4 s or an 8 s loop, `first..last`
  = 1..240; `stage007` runs 0..240 and its camera record starts at −22 with a 22-key hold —
  the re-export `stage017` starts at 0); a stage pack holds two records (`stageNNN` +
  `cameraNNN` / `stageNNN_cam`).
- **Key layouts by `flag`** (binder `FUN_00149a60`, its `(kind, n, flag)` → evaluator table of
  354 entries at 0x36B810): `0` = one value per record frame (key i = frame `first + i`),
  `2` = one static value, `3` = an XSI **fcurve** of 7-float keys `{time, left handle time,
  right handle time, u32 interpolation, value, left handle value, right handle value}`
  (`FUN_0014a650` binary-searches the key pair around the clock frame and holds before the
  first / after the last key; `FUN_00149150`: interpolation 1 = linear, 2 = the cubic Bezier
  through `(k0.value, k0.time)`, k0's right handle, k1's left handle, `(k1.value, k1.time)`
  solved for the frame by bisection on the curve parameter — `FUN_00149340` starts at 0.5,
  step 0.25, until the time error ≤ 0.1 × the clock step — 0 = hold). The `n` field is the
  component: 8 / 9 / 10 / 11 = x / y / z / w of a vector, 7 = a scalar, 6 = a camera vector,
  2 = an SRT. `tzm_dump.fcurve_value` / `track_value` reproduce this.
- **Material tracks** are named after the material and bind to its runtime record
  (`FUN_00140d40` by name): kind `500 + 100 × texture_stage + component` with component 0 =
  texture scale (record `+0x160 + 0x80·stage`, x/y/z at +0), 1 = rotation (+0x10, wrapped to
  ±π), 3 = translation (+0x20 .. +0x28); the composed texture matrix (`FUN_00141a00`:
  `Trans(0, −1, 0) · S · Rx·Ry·Rz · Trans(T)`, row-vector, uploaded to VU1 register 0x10 by
  `FUN_00143040` / `FUN_00142560`) transforms the GS-native (v-down) uv the mesh stores, so a
  translation of `(−2, 0)` over the loop scrolls the texture two repeats along u.
  `504 + 100 × stage` (n 8..11) = the stage's RGBA colour multiplier (`+0x70 + 0x20·stage`,
  multiplied into the draw colour; doubled only for untextured stages), `1302` (n 7) = the
  glow strength (`+0x590`; the glow pass colour = `+0x580` × strength × draw colour), `1301` is
  skipped, `1` is a static u32 1 flag. Every material carries static 500 / 501 / 503 (1, 0,
  0) and 1302 (0) tracks; the animated ones: `_uvani` / `_uv1` / `_ani` materials scroll u
  (−0.5 .. −2 per loop, linear; `stage003`'s `add_tex1_uvani1` scrolls v 0 → 1), the
  `_addani` / `_ani` additive materials pulse their colour (0.498 → 1 → 0.498, or 1 / 0
  blinks, Bezier), every `glo` material (two textures) pulses 1302 (0 → 1 → 0 or 1 → 0.5 → 1
  over the first 2 s), `stage010 glo_tex2` with non-flat handles. No scale / rotation / w
  fcurves exist. `tzm_dump.material_animation` samples them per record frame.
- **Camera records**: per camera (`Camera_001..010`, `Camera_neu`; `stage_chara_camera.TZM`:
  `chara_Camera_001..010`, the shared dancer close-ups aimed at (0, 12, 0)) kind 4 = the
  camera null's SRT (always identity), 5 = position, 6 = interest `(x, y, z, 0)` in stage
  units (per frame or static), 7 = the **horizontal** field of view in radians of the 4:3
  frame (0.93616 = **53.638°, XSI's default camera FOV**, on 244 of 262 cameras; 42°..100° on
  the rest — and exactly the horizontal angle of DDR A3's stock Maya cameras, 41.53° vertical
  at film aspect 1.333), 8 = roll (always 0). `Camera_neu` is the static neutral shot: 3 m
  in front of the dancer at 2 m, aimed at chest height.
- **Timing.** The clips are authored at 120 BPM like World's: `FF_NE_01` / `MM_NE_01` (the
  idles) are 242 / 241 scene frames = 8 beats, exactly `mc_*_ne01_loop`'s 242. The routines are
  19–23 s (566–698 keys) and not measure-quantised.

**Dancers** (`model/chara/skin/`, 21 packs, 512² textures, 22 bones + root, 3–7 meshes,
3.0–4.3 k strip vertices): AFRO, BABYLON, EMI, GUS, JENNY, RAGE, ROBOZUKIN, RUBY, and 13
`DDR_cspigs00..12` (the "pigs" character-select stand-ins, one `pigs00` object each). Extra
objects are costume parts (`accessory`, `muffler`, `skirt`, `cap`, `kami` (hair), `belt`,
`glasses`, `Shirt`). The skeleton is HumanIK-named: `Hip → Spine → Spine1 → Spine2 →
{Right,Left}Shoulder → Arm → ForeArm → Hand`, `Spine2 → Neck → Head`, `Hip → {Right,Left}UpLeg
→ Leg → Foot → Toes`; the male skins share one skeleton (Hip 9.655 above the root; RAGE 9.509,
GUS 9.624), the female ones another (9.785). Costume-variant packs (`model/chara/parts/`)
appear from SuperNova 2 on (103 in SN2, X and X2 have 84–94 skins).

**The character table** (SLPM_666.09 VA 0x3A5260, 21 records of 0x60 = the 8 characters + 13
pigs): `char *name, u32 1, u32 IMAGE file index, char *"globalSRT", u32 motions[…]` (indices
into the 29-name list at 0x3A5B8D: `FF_NE_01, FF_BR_01, FF_BR_02, FF_HH_01..03, FF_HT_01..03,
FF_JA_01..02, FF_SF_01..03` = 0..13, `MM_NE_01, MM_BR_01..03, MM_HH_01..02, MM_HT_01..04,
MM_JA_01..02, MM_SF_01..03` = 14..28; 30 ends the list), `…, u32 female at +0x54`:

| Character | Sex | Routines (its NE idle first) |
|---|---|---|
| AFRO | M | MM_NE_01, MM_JA_01, MM_JA_02, MM_SF_01, MM_SF_02, MM_SF_03 |
| EMI | F | FF_NE_01, FF_HH_02, FF_HT_01, FF_HT_02, FF_SF_01, FF_SF_02, FF_SF_03 |
| BABYLON | M | MM_NE_01, MM_HT_01, MM_HT_02, MM_JA_01, MM_JA_02, MM_SF_01, MM_SF_02, MM_SF_03 |
| ROBOZUKIN | F | FF_NE_01, FF_HT_01, FF_HT_02, FF_JA_02, FF_SF_01, FF_SF_02, FF_SF_03 |
| RAGE | M | MM_NE_01, MM_BR_01, MM_BR_02, MM_BR_03, MM_HH_01, MM_HH_02, MM_HT_01, MM_HT_02, MM_HT_03, MM_HT_04 |
| JENNY | F | FF_NE_01, FF_BR_01, FF_BR_02, FF_HH_01, FF_HH_02, FF_HH_03 |
| GUS | M | MM_NE_01, MM_HT_01, MM_HT_02, MM_HT_03, MM_HT_04 |
| RUBY | F | FF_NE_01, FF_BR_01, FF_BR_02, FF_HH_03, FF_JA_01 |

**Dance routines** (`model/chara/motion/`, 29 packs + 31 tutorial): `FF_*` (female) and
`MM_*` (male) × style `BR`, `HH`, `HT`, `JA`, `NE`, `SF` × take; `MM_TU_*` are 30-key
excerpts (windows of a 1823-frame timeline) that demonstrate the tutorial steps; `ftpanel_*`
animate the foot-panel model (`model/footpanel.TZM`) with kind-3 SRT tracks. The camera moves
are their own MOTION-only packs (`stage/stage_chara_camera.TZM`: ten `chara_Camera_0NN`
records of 240 frames, `stage_baby_camera.TZM`, `ending/ending_cam.TZM`).

**Stages** (`model/stage/`, 20 + system/tutorial/howto backgrounds, `ad001`/`ad002`, and
`gs_stage002`): each pack is MODEL (format-0x152 coloured meshes, no bones, 11–40 objects) +
MOTION (`stageNNN` with SRT / material tracks for the animated parts, `cameraNNN` or
`stageNNN_cam`) + 1–3 textures (one 512² colour sheet plus 256² `_g` glow and `_t` base sheets)
with a `<stage>_conf.PTF` beside it. The objects hang under **blend-layer roots**: `dec`
(opaque), `add` (additive), `sub` (subtractive), `glo` (glow: the material names two textures,
the `_t` base and the `_g` glow) and `ble` (alpha) — the very names World's own XSI-lineage
stage parts carry (`gm_dawnstreet00_{dec,ble,glo}`; `dawnstreet00`'s sheets are `jx_st005_*`,
i.e. DDR X's stage 5). Object names below are Japanese scene parts (`butai` stage, `haikei`
background, `kazari` decoration, `wakka` rings, `kanransya` ferris wheel, ...); the `bg` /
`sphere` subtree is the skydome (radius 1000). The **MATERIALLIST** chunk (`{u32 hash, u32 n,
8 × 0xCD}` + n records of `(size − 0x10) / n` bytes: `char name[0x40]`, texture chunk name at
+0x50, RGBA diffuse at +0xA0, texture count at +0x350, ambient at +0x360, the glow texture at
+0x370) maps materials to textures; a mesh's 0x18-byte material field truncates long names
(`DefaultLib.add_tex1_uvan` = `…_uvani`). Animated objects have kind-3 / kind-2 SRT tracks
(10 floats `S Q T`, 7 = `Q T`); `_uvani` / `_ani` materials scroll their UVs with kind-503
tracks; flattened decals use `S = (2, 0, 2)` and pulse their scale. `stage001`'s floor is a
1200 × 1200 plane and `Camera_neu` sits at (0, 20, 30) looking at (0, 9.7, 1.8), so a stage unit
is the dancer's unit (≈ 0.1 m). `stage011..020` are the ten `stage001..010` scenes again with
recoloured textures (`stage011` even keeps the motion name `stage001`). The
`smm/bg/sm001..011.tzm` packs are the Shop Mode room in eleven texture variants;
`smm/bg/*_cam.tzm` and `test/light_test.TZM` store their MODEL / textures raw (no TGCD) and
`test/glowtest.TZM` has an older MOTION header — the survey skips `test/`.

**Stage port** (`port_stage_supernova.py`, staged 2026-09-30 to a folder per stage): one World
part per layer with the stock flags (`dec` 0x0001, `ble` 0x02C1, `add` 0x06C1/4, `sub`
0x06C1/8, `glo` = opaque `_t` copy + additive `_g` copy; `bg` = the skydome subtree, `:-2`),
everything two-sided, vertex colours kept (TZM's GS 0..128 → `round(255 v)`, written via
`color_srgb`; the stages shipped before 2026-10-04 were sRGB-encoded by Blender's `color` accessor
and were re-ported on 2026-10-04 -- X Stage 05: 632 of 632 shipped RGB triples are the TZM's); a flat rig (root + one bone per animated object,
static sub-chains baked) with a looping `_play_loop.anm` re-evaluating within 3e-4 of the TZM
worlds; the SuperNova foot panel as `footpanel`. **Material animation**: each part with animated
materials gets a `gm_<key>_<part>_play_loop.sanm` (World's own material-clip shape, doc
`3d_model_format_research.md` §7: kind-8 tracks per record frame + a wrap key on parameter
floats 2 / 3 = `m_vTexAnime` offU / offV for the texture translation, 4..6 = `vConstatntColor`
rgb for the 504 colour on the base pass and the 1302 glow strength on the `_g` additive
pass, materials addressed by their `.model` identity); those materials are exported with the
`mdl_ch_constant_c_vc` shader and their frame-0 values seeded in the `.model`. The hook DLL
samples the clip on the stage clock into the render item's private material copies
(`core::anm::sanm`, the frame board's material lane). **Cameras**: `Camera_001..010` →
`camera/<key>_st01..10.camanm` (the main rotation), `Camera_neu` → `<key>_non01`, the ten
`chara_Camera_*` close-ups → `_non02..11` (`CHARA_CAMERAS=0` omits them); one key per record
frame at 60 fps (`60 / fps` frames apart, 4 s / 8 s clips), position × `GAME_SCALE` in
centimetres, the look-at orientation (+Y up, roll applied), the FOV through the inverse of
World's CameraNode projection so that the 16:9 frame keeps SuperNova's VERTICAL extent
(53.638° horizontal on 4:3 = 41.53° vertical → game hFOV 68°; `FOV_KEEP=horizontal` keeps the
horizontal extent instead — the two 100° cameras then land past 90° game hFOV, a negative
slot-2 value the recipe handles). Every clip is checked against its look-at (< 1e-3) and the
previews render the staged stage + a ported dancer through the written clips.

**Port** (`port_character_supernova.py`, staged 2026-09-30 to a folder per character): the
frame is World's (Y-up, facing +Z, left at +X — no mirror), scale 0.970 / 9.655 m per unit
(the Hip at World's Hips height), one skinned mesh per character with the object transforms
applied and the winding made consistent, the CLUT sheet as the texture, the character's own
routine list (minus the idle) as `motion/<clip>.anm` with the 30 Hz keys on even frames, role
aliases `Hips` / `LeftToeBase` / `RightToeBase` → `Hip` / `LeftToes` / `RightToes`. Every clip
re-evaluates within 0.15 mm of the TZM pose; all eight re-import and render.

### 7.5 SuperNova 2's dancers (2026-10-03)

SLPM_669.30 keeps the engine and the 29 routine packs (`model/chara/motion/*.TZM` are byte for
byte SuperNova's; so are the 20 stage packs, `stage_chara_camera`, `footpanel` and `system_bg001`
— the only new stage is `system_bg002`, a grid / light-beam / star scene with a 480-frame 60 fps
loop and one `Camera_Root` shot). The dancers changed:

- **Twelve characters × two costumes.** `model/chara/skin/<skin>NN.TZM` with NN = 01 / 02:
  the SuperNova eight (`afro`, `emi`, `babylon`, `zukin`, `rage`, `jenny`, `gus`, `ruby`) plus
  `yuni`, `alice`, `concent`, `julio`; `wakka_{male,female,all}` are the character-select
  stand-ins ("DISK-A/B/?"), and `chara/parts/` holds 103 small accessory packs (`SN2_<Joint>_<set>N`)
  the CS costume editor hangs off joints (not ported). Each skin's root node is named after
  the pack (`afro01`, `concent01`), not `globalSRT`; the clips still carry one static root
  track under the ORIGINAL rig's name (`DDR_AFRO_NEW`, `globalSRT`), so the root binds by
  position. CONCENT's root carries `T = (0, 0.41, 0)` with the Hip's pose at 0.41 (the two sum to
  the 0.819 bind) and its object chain cancelling it (`concent01` +0.41, `CONCENT` −0.41): the
  game must discard the root pose, since with it the standing feet come out at y = 0.45 instead
  of everyone's 0.04 (`tzm_dump.clip_worlds`). Scales: `SCALE` 0.9 on `julio01`, 0.4 on
  `babylon02` (a chibi with its own rig, Head bind at 2.43), 0.6 on `babylon01`.
- **The costume-01 bodies of the returning eight are SuperNova's skins**: identical colour
  sheets (EMI's and ROBOZUKIN's recoloured), identical rigs and binds, and the same triangles
  minus the face — SuperNova 2 cut the eyes / mouth triangles out of every body (RUBY: 260 of
  2979; the strips were re-exported with far fewer restarts, so vertex counts went up while the
  surface shrank). Port decision: the merged `DDR SUPRNVA 1+2` source ships the SuperNova body
  as `<Name> 1` and only SuperNova 2's costume 02 (+ both costumes of the four new characters).
- **Face packs.** `<skin>_face.TZM`: MODEL only (no bones), one object tree per expression —
  `faceNN (root, T 0) > trans_null (T = head-local offset, R = the Head bind rotation's inverse
  (−0.426, 0.001, −1.569)) > faceNN (mesh, format 0x112, 15–657 vertices)` — and three 128²
  8 bpp sheets `<skin>_faceNN_png` (`face01` neutral, `face02` smiling, `face03` eyes shut;
  the table lists them 01, 03, 02). The game hangs the root off the Head joint: a mask vertex
  sits at `Head_bind · W_object · v` in the bind frame (ALICE's `face02` sheet is authored in
  world space and its mesh object carries `T = (0, −6.35, −0.57)` to cancel it; every one of
  the 24 masks lands inside its head's vertex box, 0.1 units behind the face —
  `tzm_dump.face_overlay`). All sheets are fully opaque. CONCENT's pack also holds `body01 >
  body_trans_null (R = (0, −0.202, 0), the fan's tilt) > fan01 (T = (0, 0, −0.31))`, its chest
  fan (85 vertices, 64² sheet; the body's chest grille is open behind it), hung off `Spine1`
  (the table's `"body01", "Spine1"` slot: a vertex sits at `Spine1_bind · W_object · v`, the
  fan centre at (0, 2.73, −0.03) in the chest), and the pack's own `ddr_concent_fan` MOTION
  (frames 0..240 at 30 fps, 121 keys): kind-3 SRT holds on every node, and on `fan01` a
  **kind-2 Q T track** (7 floats) turning it a uniform −6° per key about its local z — two
  revolutions per 4 s loop, the translation constant. An object's T precedes its own R, so
  the spin turns the blades and never their offset. `parts/convent01_body01.tzm` is the same
  fan on a `Spine1` root with static tracks only. The port gives the fan its own joint and
  lays the spin onto every dance clip (`tzm_dump.attach_part_bone` / `part_spin_track`); the
  two other expressions are left out (the body carries `face01`). `gus02.TZM` carries a stray
  `gus_face02_png` as its FIRST texture — pick a body's sheet through the MATERIALLIST, not by
  position.
- **The character table** (SLPM_669.30 VA 0x3D2D90 for costume 01, 0x3D3480 for costume 02,
  12 records of 0x94 each; both costumes dance the same list): `char *name, u32 body IMAGE
  index, char *body skin, u32 face IMAGE index, (char *face mesh, u32 face index) × 3 = face01,
  face03, face02, [CONCENT: u32 face index, "body01", "Spine1", u32 face index,
  "ddr_concent_fan", u32 1], f32 (0.35–0.85, untraced), u32 motions[…] terminated by 30 (the same
  29-name list, at 0x3D4152), u32 RGBA theme colour at +0x8C (AFRO 0x80961496 purple, ...)`.
  No sex flag — the routine family (`FF_*` / `MM_*`) gives it. 0x3D3B70 holds `CONCENT_PARTS`
  (IMAGE 282) and the three `DISK-*` wakka records.

| Character | Costumes | Routines (its NE idle first) |
|---|---|---|
| AFRO, EMI, BABYLON, ROBOZUKIN, RAGE, JENNY, GUS, RUBY | 01 (= SuperNova), 02 | as the SuperNova table |
| YUNI | 01, 02 | FF_NE_01, FF_HH_01, FF_HH_02, FF_HT_03, FF_SF_01, FF_SF_02, FF_SF_03 |
| ALICE | 01, 02 | FF_NE_01, FF_HT_01, FF_HT_02, FF_JA_01, FF_JA_02, FF_SF_02, FF_SF_03 |
| CONCENT | 01, 02 | MM_NE_01, MM_HT_01, MM_JA_01, MM_JA_02, MM_SF_01, MM_SF_02, MM_SF_03 |
| JULIO | 01, 02 | MM_NE_01, MM_BR_01, MM_BR_02, MM_BR_03, MM_HH_01, MM_HH_02, MM_HT_01, MM_HT_02, MM_SF_02 |

**Port** (`port_character_supernova.py GAME=sn2`, 2026-10-03, 16 dancers): the SuperNova
pipeline plus the `face01` mask joined to the body mesh as its own material slot
(`sn2<skin>_face`, 128²) weighted 1.0 to `Head`; CONCENT's fan as a 24th joint `fan01` under
`Spine1` (bind = Spine1's bind × the pack chain; slot `sn2<skin>_body01`) with a rotation track
on every clip = tilt × spin sampled at the clip's key frames (one revolution per 2 s relative
to Spine1 in the written `.anm`); all 98 clips re-evaluate within 0.15 mm, all sixteen re-import
and render with their faces. `port_stage_supernova.py GAME=sn2` ports
`system_bg002` as `System BG 2` (`snsystembg002`: `bg` + `add` parts, a 480-frame loop, one
`_uvani` scroll `.sanm`, `Camera_Root` → `_non01` + the shared close-ups). Shipped merged with
SuperNova's content under `data_mods/custom_models/{dancers,stages}/DDR SUPRNVA 1+2/`
(`Afro 1` / `Afro 2`, `System BG 1` / `System BG 2`; the renamed source changes the option-row
id to `background_{dancer,stage}_ddr_suprnva_1_2`).

### 7.6 DDR X and X2 (2026-10-03)

SLPM_550.90 (X, JP 2008) and SLUS_219.17 (X2, US 2009) keep the engine once more: the 29
routine packs are byte for byte SuperNova's (+ a `tutorial/` set), `stage_chara_camera`,
`footpanel` and `cs_footpanel` are SuperNova 2's, and `chara/parts/` shrank to one pack. What
the two discs add:

- **Skins.** `model/chara/skin/`: 84 packs in X, 94 in X2 — twelve characters × three costumes
  (`<skin>01` new, `02` / `03` = SuperNova 2's `01` / `02` byte for byte in X; in X2 `01` = X's,
  `02` NEW (X's 01 bodies on new colour sheets — a recolour per character, 13 packs), `03` =
  SuperNova 2's 02), plus `babylon02` (X: a second new BABY-LON on the SuperNova-style rig),
  `bonnie01` / `zero01` (new characters; X2 adds `bonnie02`, `zero02` recolours sharing the 01
  face packs), X2's `pix01..04` (the PIX pigs: `pigs01` meshes on the standard rig under a
  `SCALE` 0.45 node, root `globalSRT_jx_pixNN`), and the `wakka_*` DISK trio again.
  `afro01_atama.TZM` / `bonnie01_hair.TZM` are hair sprites for X2's `dmm/` board game, not
  costume parts. Every body keeps SuperNova 2's shape: 23 nodes (root + the 22 joints; `SCALE`
  on BABYLON 0.4, JULIO 0.9, PIX 0.45), the root named after the pack (`jx_afro01`, `Zero01`,
  `globalSRT_jx_pix01`), the face cut out into `<skin>_face.TZM`. X2's `alice01` differs from
  X's by four vertices of one strip (same sheet); X2's `stage006` only swaps the `10th`
  anniversary logo sheet for a blank one.
- **Face packs** are SuperNova 2's layout (`faceNN > trans_null > faceNN`, 128² sheets; 32 of
  X's 41 are byte-identical to SuperNova 2's); PIX names its roots `jx_pixNN_face1/2/3`
  (`jx_pix02_face01`, `Head3 > pix_kao1`), so the face root comes from the character table.
  All 33 ported masks land inside their head's vertex box (`tzm_dump.face_overlay`).
- **CONCENT's fan** moved out of the face pack into `chara/parts/convent01_body01.tzm`
  (`Spine1 (root) > convent01_body01`, 107 vertices, a 64² sheet, authored in Spine1's bind frame
  with a static MOTION); the table hangs it off `Spine1` (`body01` / `Spine1` slot). In the bind
  pose it sits in the chest window of the SuperNova-2-style costumes and inside the jacket of
  X's costume 01 (`tzm_dump.part_overlay`, the bone-parametrised `face_overlay`).
- **The character table** (SLPM_550.90 VA 0x34686C, 41 records; SLUS_219.17 VA 0x2E433C, 35;
  0xDC bytes each): `u32 name hash, char *name, u32 skin IMAGE index, char *skin root, (u32 face
  IMAGE index, char *face root) × 3 = neutral, eyes-shut, smile, [CONCENT: (u32, "body01"),
  "Spine1", (u32, "ddr_concent_fan")], the 2D cut-in assets (`dance/cutin/<chara>/*.dld`
  indices), f32 shadow scale at +0x8C, u32 motions[16] at +0x90 terminated by 31 (the same
  29-name list + `MM_TU_bsd` at 0x3490C6 / 0x2E66EB), u32 female at +0xD4`. The lists are per
  character (every costume the same) and much longer than SuperNova's: the whole `MM_*` family
  minus `MM_BR_01` / `02` for AFRO, BABYLON, GUS, CONCENT, JULIO, DISK-A (12 routines), all of
  it for RAGE and ZERO (14), the whole `FF_*` family for every woman (13; BONNIE's idle is
  `MM_NE_01`), and a mixed six for PIX (`MM_HT_03`, `MM_HT_04`, `FF_HH_02`, `FF_HT_03`,
  `FF_SF_01`, `FF_SF_02`; female by the flag). Shadow scales: AFRO / GUS 0.75, EMI / JENNY /
  RUBY / YUNI / ALICE / DISK 0.6, BABYLON / PIX 0.35, ROBOZUKIN 0.8, RAGE / ZERO 0.7, CONCENT
  0.85, JULIO 0.45, BONNIE 0.65 — the footprint the game draws under each dancer, carried
  into the sidecar.
- **The DISK trio** (`wakka_male` / `_female` / `_all`: 22 joints, no `RightToes`, a 64² sheet)
  are strip meshes of zero-area triangles — rings the GS drew as lines. They would not render
  as World geometry and are not ported.
- **Stages** (`model/stage/`): six, `stage001..006` (X's own set — `dawnstreet00` in World is
  stage 5's XSI scene, same `jx_st005_*` sheets), each with a `_conf.PTF`; cameras are one
  `jx_stNNN_cam_FIX2` record (`Camera_001..008`, `Camera_neu`, `Camera_non_chara01..04` — the
  scenery cut-aways, 240 frames at 60 fps, FOV 0.9362 everywhere). `stage001` ships its
  eighteen beat-pulsing speakers separately (`stage001_speaker.TZM`, a 29.97 fps `add` overlay)
  and `stage001_2play.TZM` is exactly the two merged with the same cameras; `stage002_2play` /
  `stage003_2play` are the reduced two-player dressings and `stage005_2play` a 5-mesh stub.
  **`stage002`'s TVs are render targets**: objects `RenderBIGTV` (a 16:9 quad, u 0→1 left to
  right as seen, v 0.2–0.8 of its target), `RenderBIGTV2` (a coplanar 70-vertex glass over it,
  material `monita`, a blue-gradient placeholder sheet), `RenderSBTOPTV` and `RenderSBTVa/b/c`
  (curved bands, v 0.2–0.8) — the surfaces X drew its sub-monitor feed on (`stage/submonitor/
  <song>/*.dtf` layouts). Also here: `AppealEffect*.TZM` (full-combo effects), `m_ball.tzm`,
  `ux_tutorial.TZM`, `lensFlare*.dtf`. X2's only other pack, `system_bg002.tzm`, is SuperNova 2's
  byte for byte; X2's `IMAGE/dmm/model/` holds its board-game pieces (`afro.tzm` .. `zukin.tzm`:
  15-joint `Hip`-rooted chibi rigs under a `globalSRT.AFRO` object, `floor1..15`, `dice`,
  `cursor`) — another layout, not ported.

**Port** (`port_character_supernova.py GAME=x` / `GAME=x2`, `port_stage_supernova.py GAME=x`,
2026-10-03): one source folder `data_mods/custom_models/dancers/DDR X + X2/` with the 33 dancers
neither game shares with SuperNova 2 — X's costume 01 of all twelve (`Afro 1` .. `Robo-Zukin 1`,
keys `x<skin>`), `Baby-Lon 2`, `Bonnie 1`, `Zero 1`; X2's recolours (`Afro 2` .., `Baby-Lon 3`,
keys `x2<skin>`), `Bonnie 2`, `Zero 2`, `Pix 1..4` — each with its neutral face mask, CONCENT
with the fan (`x<skin>_body01` slot), its own 12–14 routines (all within 0.15 mm) and the
table's shadow scale. `stages/DDR X + X2/`: `Stage 01` (from `stage001_2play`) .. `Stage 06`
(keys `xstage001..006`; parts by layer, loops, `.sanm` UV scrolls / glows, the 8 + 1 + 4 own
shots as `_st01..08` / `_non01..05` + the ten close-ups `_non06..15`, no foot panel). Stage 02's
five `Render*` surfaces are textured **`offscreen1`** (World's movie render target — the DLL's
STAGE SCREENS mode plays the song's movie on them), their authored v band remapped onto the
square's 16:9 band (0.21875–0.78125), vertex colour white; `RenderBIGTV2` is dropped as the
coplanar duplicate. All previews re-import and render; not yet cabinet-tested.

## 8. Reproduce

```bash
G=~/Desktop/"PS2 DDR ISOs/Dance Dance Revolution Strike (Japan)"
python3 scripts/extract_ps2_ddr_data.py find-toc "$G/SLPM_662.42" "$G"/DATA/FILEDATA.BIN "$G"/DATA/FILEDT0{2,3}.BIN
python3 scripts/extract_ps2_ddr_data.py extract strike_jp "$G" "$G/extracted_full" --unpack --png   # ~30 s, 2.4 GB
./scripts/validate_ps2_ddr_tools.sh "$G/extracted_full"    # 89 dancer meshes, 45 motion sets, 0 problems
```

A re-run is byte-identical: both manifests and the whole `unpacked/` tree match.

The later discs, each ~10 s with `--unpack --png`:

```bash
D=~/Desktop/"PS2 DDR ISOs"
G="$D/Dance Dance Revolution - Party Collection (Japan)"
python3 scripts/extract_ps2_ddr_data.py extract party_collection_jp "$G" "$G/extracted_full" --unpack --png   # 0.7 GB
./scripts/validate_ps2_ddr_tools.sh "$G/extracted_full"    # 120 dancer meshes, 45 motion sets, 0 problems
G="$D/DDR Festival - Dance Dance Revolution (Japan)"
python3 scripts/extract_ps2_ddr_data.py extract festival_jp "$G" "$G/extracted_full" --unpack --png           # 1.9 GB
./scripts/validate_ps2_ddr_tools.sh "$G/extracted_full"    # 52 dancer meshes, 45 motion sets, 0 problems
G="$D/Dance Dance Revolution SuperNova (Japan)"
python3 scripts/extract_ps2_ddr_data.py find-toc "$G/SLPM_666.09" "$G"/DATA/*.DAT
python3 scripts/extract_ps2_ddr_data.py extract supernova_jp "$G" "$G/extracted_full" --unpack --png          # 1.0 GB, 1736 checksums ok
# likewise supernova2_jp (SLPM_669.30), x_jp (SLPM_550.90), x2_us (SLUS_219.17): 1.2-1.4 GB each;
# --exclude vig,mpeg,ipu skips the audio and movies, --names 'IMAGE/LOGO/*' selects files.
```

All six runs: 0 checksum mismatches, 0 conversion errors (3641 / 4103 / 1064 / 1044 / 869 /
1341 PNGs). `--wav` is not part of those runs: it decodes about 50× faster than real time,
so a full disc's songs take minutes.

The Festival and Party Collection dancers (§6.1; `GAME=festival` / `GAME=pc`, defaulting to the
extractions above, writing straight into their source folders). A Blender run ports one dancer
in ~5–10 s, so run the two games in separate Blender processes:

```bash
GAME=festival DANCERS=all PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
    --python tools/blender_ddr_addon/examples/port_character_strike.py   # ~4 min -> dancers/DDR FESTIVAL/ (26)
GAME=pc DANCERS=all PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
    --python tools/blender_ddr_addon/examples/port_character_strike.py   # ~8 min -> dancers/DDR PARTY COLLN/ (60)
```

The SuperNova TZM packs (§7.4):

```bash
I="$G/extracted_full/files/IMAGE"
./scripts/validate_ps2_ddr_tools.sh "$G/extracted_full"      # + 183 TZM packs (95 models, 169 motion records), 0 problems
python3 scripts/tzm_dump.py info "$I/model/chara/skin/afro.TZM"
python3 scripts/tzm_dump.py preview "$I/model/chara/skin/afro.TZM" /tmp/afro.png \
    --motion "$I/model/chara/motion/FF_HH_01.TZM" --frame 200       # the software render used for the RE
DANCERS=all PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
    --python tools/blender_ddr_addon/examples/port_character_supernova.py   # ~25 s for the 8 dancers
STAGES=all PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
    --python tools/blender_ddr_addon/examples/port_stage_supernova.py       # ~30 s for the 21 stages
```

SuperNova 2 (§7.5; the port scripts take `GAME=sn2` and default to its extraction):

```bash
G="$D/Dance Dance Revolution SuperNova 2 (Japan)"
python3 scripts/extract_ps2_ddr_data.py extract supernova2_jp "$G" "$G/extracted_full" --unpack --png --exclude vig,mpeg,ipu   # ~9 s, 1860 checksums ok
./scripts/validate_ps2_ddr_tools.sh "$G/extracted_full"      # 250 TZM packs (187 models, 228 motion records), 0 problems
GAME=sn2 DANCERS=all PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
    --python tools/blender_ddr_addon/examples/port_character_supernova.py   # ~60 s for the 16 new dancers
GAME=sn2 STAGES=all PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
    --python tools/blender_ddr_addon/examples/port_stage_supernova.py       # system_bg002
# then move the staged folders into data_mods/custom_models/{dancers,stages}/DDR SUPRNVA 1+2/
```

DDR X and X2 (§7.6; `GAME=x` / `GAME=x2`, defaulting to their extractions):

```bash
G="$D/Dance Dance Revolution X (Japan)"
python3 scripts/extract_ps2_ddr_data.py extract x_jp "$G" "$G/extracted_full" --unpack --png --exclude vig,svag,mpeg,ipu   # 2703 checksums ok
G2="$D/Dance Dance Revolution X2 (USA)"
python3 scripts/extract_ps2_ddr_data.py extract x2_us "$G2" "$G2/extracted_full" --unpack --png --exclude vig,svag,mpeg,ipu  # 2655 checksums ok
./scripts/validate_ps2_ddr_tools.sh "$G/extracted_full" "$G2/extracted_full"   # 166 / 273 TZM packs, 0 problems
GAME=x DANCERS=all PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
    --python tools/blender_ddr_addon/examples/port_character_supernova.py   # ~2 min for the 15 X-only dancers
GAME=x2 DANCERS=all PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
    --python tools/blender_ddr_addon/examples/port_character_supernova.py   # ~2 min for the 18 X2-only dancers
GAME=x STAGES=all PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
    --python tools/blender_ddr_addon/examples/port_stage_supernova.py       # ~1 min for the 6 stages
# then move both staged dancer folders and the stage folder into data_mods/custom_models/{dancers,stages}/DDR X + X2/
python3 scripts/ktmdl_dump.py "data_mods/custom_models/stages/DDR X + X2/Stage 02/mapset_xstage002/gm_xstage002_dec/gm_xstage002_dec.model" | grep offscreen1   # the screens
```
