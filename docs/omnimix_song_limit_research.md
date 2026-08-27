# Omnimix Song Limit Research

Reverse engineering document for DDR World's song loading pipeline — how musicdb.xml is parsed, how songs are stored in memory, what limits exist, and how to remove them to support an Omnimix with thousands of songs.

**Game binary**: `gamemdx.dll`
**Ghidra DB**: MDX-003_20250805 (base `0x180000000`)
**Build delta**: Functions shifted ~+0x1F070 between the 20250805 and 20260324 builds. All Ghidra addresses in this doc are from the 20250805 build. All AOB signatures were verified against the 20260324 build at runtime. **AOB signatures should be used for address resolution, not hardcoded offsets.**
**Tools used**: Ghidra (static analysis of 20250805), Cheat Engine (runtime analysis of 20260324)
**Prerequisite**: [series_filter_internals.md](series_filter_internals.md), [filter_ui_extension.md](filter_ui_extension.md)

---

## Summary

DDR World's song database uses `std::vector` for storage — **there is no hardcoded song count limit**. The vector grows dynamically as songs are parsed from musicdb.xml.

The **only hard bottleneck** is the XML read buffer: a fixed 1MB (0x100000) allocation used to read musicdb.xml into memory. The stock file is 663KB (1430 songs). At ~463 bytes per song in XML, the 1MB buffer caps out at ~2262 songs. For an Omnimix targeting 3000+ songs, this buffer must be expanded.

| Potential Limit | Status | Details |
|----------------|--------|---------|
| XML read buffer | **BOTTLENECK — must patch** | 1MB, caps at ~2262 songs |
| Song entry vector | No limit | `std::vector`, dynamic growth (1.5x) |
| Series mapper | Already solved | Existing series expansion handling covers values >21 |
| Memory usage | Trivial | 600 bytes/song × 5000 = 3MB |
| Song entry struct | No limit | Fixed 0x258 bytes per song, no ID-indexed arrays found |

---

## Song Loading Pipeline

### Call Chain

```
FUN_18019e2b0  — Music DB constructor (creates empty std::vectors)
  → FUN_18019f230  — Master loader
      → FUN_1801a0ca0  — Parse license.xml (1MB buffer)
      → FUN_1801a0ec0  — Parse musicdb.xml (1MB buffer) ← PRIMARY TARGET
          → per-song loop:
              → FUN_18019f2f0  — Parse one <music> element into 0x258-byte entry
              → FUN_1801a0430  — Push entry into std::vector (grows if needed)
      → FUN_1801a1030  — Parse coursedb.xml (1MB buffer)
      → FUN_1801a0630  — Post-processing (sort/index)
```

### musicdb.xml Parser (`FUN_1801a0ec0`)

The parser:
1. Allocates a 1MB buffer via the game's allocator (`[0x18042e258]`)
2. Opens `/data/gamedata/musicdb.xml` via the AVS file system (`[0x1806b5cd8]`)
3. Reads the XML into the buffer (size passed as parameter)
4. Navigates to `/mdb/music` node
5. Iterates `<music>` children, calling `FUN_18019f2f0` for each
6. Each song is pushed into the vector via `FUN_1801a0430`
7. Frees the buffer

**The 1MB buffer is used for BOTH the allocation size AND the read size parameter.** Both must be patched together.

---

## Music Database Object Layout

The music DB object (created by `FUN_18019e2b0`) has this layout:

```
+0x00  ptr    music_begin      std::vector<MusicEntry> begin
+0x08  ptr    music_end        std::vector<MusicEntry> end
+0x10  ptr    music_capacity   std::vector<MusicEntry> end_of_storage
+0x18  ...    (padding/other)
+0x28  ptr    linked_list_1    (0x50-byte sentinel node)
+0x30  ...
+0x48  ptr    linked_list_2    (0x50-byte sentinel node)
+0x50  ...
+0x60  ptr    lookup_begin     (another vector, possibly name→index map)
+0x68  ptr    lookup_end
+0x70  ptr    lookup_capacity
+0x80  ...
+0xa0  ptr    course_begin     std::vector<CourseEntry> begin
+0xa8  ptr    course_end       std::vector<CourseEntry> end
+0xb0  ptr    course_capacity  std::vector<CourseEntry> end_of_storage
+0xc0  u8     flag
+0xc8  ...
+0xd0  [0x28] buffer_1         (zeroed)
+0xf8  [0x28] buffer_2         (zeroed)
```

### Confirmed via runtime analysis:
- **Music vector**: 1419 entries loaded (11 filtered from 1430 in XML)
- **Entry stride**: 0x258 (600) bytes — confirmed by contiguous memory walk
- **Vector is contiguous**: entries at stride 0x258 from begin to end

---

## Music Entry Structure (0x258 bytes)

Partial layout confirmed from runtime memory inspection of "fixer" (mcode 38873):

```
+0x00  u32    mcode            Song ID (e.g., 38873 = 0x97D9)
+0x04  u8     unknown          (value 4 observed)
+0x05  char[5] basename        4-char code + null (e.g., "fixe\0")
+0x0A  ...    (padding/flags)
+0x10  string title            MSVC std::string SSO (e.g., "fixer")
       +0x10  [16] buf         SSO buffer (≤15 chars) or heap pointer
       +0x20  u64  length
       +0x28  u64  capacity    (0x0F = SSO mode)
+0x30  string title_yomi       (reading/pronunciation, same SSO layout)
+0x50  string artist           (same SSO layout)
+0x60  ptr    property_obj     Pointer to property accessor object (vtable+0xA0 = series)
+0x70  ...
+0x78  u64    unknown
+0x80  u64    unknown
+0x88  u16[10] bpm_data        BPM values (min/max per chart?)
       observed: D7 00 5A 00 = max 215, min 90 (matches fixer)
+0xA0  ...    (more BPM/timing data)
+0xBC  u32[10] difficulty_levels  (diffLv array)
+0xE4  ...    (limited_ary, flags, etc.)
...
+0x258 (end of entry)
```

The exact field layout beyond the basics is not critical for the buffer expansion — the game's own parser fills these fields. What matters is that the struct is a fixed 0x258 bytes and the vector handles growth.

---

## Vector Growth Mechanism

### Push function (`FUN_1801a0430`)

```
1. Check if insertion point is within existing range
2. If end == capacity → call grow function (FUN_1801a0790)
3. Copy entry data to end position
4. Increment end pointer by 0x258
```

### Grow function (`FUN_1801a0790`)

Standard MSVC `std::vector::_Grow`:
- **Growth factor**: 1.5x (new_cap = old_cap + old_cap/2)
- **Max size check**: ~1.97 × 10^16 elements (theoretical, not practical)
- **Allocator**: Game's custom allocator at `[0x18042e258]`
- **Reallocation**: Allocates new buffer, copies elements, frees old buffer

**No hardcoded capacity limit.** The vector will grow until the allocator runs out of memory.

---

## XML Buffer — The Bottleneck

### Buffer Sizes

| File | Buffer Size | Stock File Size | Usage | Max Songs (est.) |
|------|------------|----------------|-------|-----------------|
| musicdb.xml | 1MB (0x100000) | 663KB | 63% | ~2,262 |
| coursedb.xml | 1MB (0x100000) | small | low | N/A |
| license.xml | 1MB (0x100000) | small | low | N/A |

### Why 1MB Is the Limit

The buffer is allocated once and the entire XML file is read into it. If the file exceeds the buffer, it is truncated. The XML parser then operates on the truncated data, which means:
- Songs beyond the buffer boundary are silently lost
- If truncation happens mid-element, the parser may stop at the last complete `<music>` element
- No error is reported — the game just loads fewer songs

### Scaling Estimates

| Target Songs | Est. XML Size | Required Buffer | Recommended |
|-------------|--------------|----------------|-------------|
| 2,000 | ~926KB | 1MB (tight) | 2MB |
| 3,000 | ~1.4MB | 2MB | 4MB |
| 5,000 | ~2.3MB | 4MB | 4MB |
| 10,000 | ~4.6MB | 8MB | 8MB |

**Recommendation**: Patch to **0x800000 (8MB)**. This supports ~17,000 songs with headroom. The allocation is freed after parsing, so the temporary memory cost is negligible.

---

## Patch Sites

### AOB Signature for Buffer Allocation

All three parsers share the same prologue pattern:

```
45 33 C0 BA 00 00 10 00 E8
```

This is: `XOR R8D,R8D; MOV EDX,0x100000; CALL allocator`

- **3 hits** in gamemdx.dll (one per parser)
- `MOV EDX` immediate is at pattern offset +4 (bytes 4-7 = `00 00 10 00` little-endian)
- To change to 8MB: write `00 00 80 00` at offset +4

### AOB Signature for Buffer Size Parameter

```
C7 44 24 20 00 00 10 00
```

This is: `MOV dword ptr [RSP+0x20], 0x100000`

- **3 hits** in gamemdx.dll (one per parser)
- Immediate is at pattern offset +4 (bytes 4-7 = `00 00 10 00`)
- To change to 8MB: write `00 00 80 00` at offset +4

### Instruction Encoding

```
MOV EDX, 0x100000:        BA 00 00 10 00  (5 bytes, imm32 at byte 1)
MOV [RSP+0x20], 0x100000: C7 44 24 20 00 00 10 00  (8 bytes, imm32 at byte 4)

To patch to 0x800000 (8MB):
  BA 00 00 10 00 → BA 00 00 80 00  (change byte 3 from 0x10 to 0x80)
  C7 44 24 20 00 00 10 00 → C7 44 24 20 00 00 80 00  (change byte 6 from 0x10 to 0x80)
```

---

## What Does NOT Need Patching

### Song entry vector
The `std::vector<MusicEntry>` grows dynamically. No patch needed.

### Song entry structure
The 0x258-byte struct is filled by the game's own XML parser. No size limit.

### Series mapper
Already handled by the series expansion patch. Custom series values >21 pass through.

### Filter UI
Already handled by the filter UI extension. Custom filter entries are injected.

### Game allocator
The game's custom allocator at `[0x18042e258]` delegates to the OS heap. No practical limit for the sizes involved (tens of MB).

---

## Per-Song Availability Check (`FUN_18019f2f0`)

During parsing, each song goes through an availability check before being added to the vector. The check involves:

1. Reading the series value via vtable+0xA0 (same accessor used by the series mapper)
2. Calling a function at `[0x1806b5130]` — likely an "is content available" check
3. If the check fails, the song is **skipped** (not added to the vector)

This is why 1419 songs are loaded from 1430 in the XML — 11 songs fail the availability check.

**Impact on Omnimix**: Custom songs added to musicdb.xml go through this same check. If the check is based on e-amusement unlock state or region restrictions, custom songs might be filtered out. This would need investigation if songs are missing after expanding the buffer.

**Potential patch**: If custom songs are being filtered, the availability check could be hooked to bypass filtering for songs with series values >21 (custom songs). This is a separate concern from the buffer expansion.

---

## Song ID Checks in Parser

The per-song parser (`FUN_18019f2f0`) has two hardcoded song ID checks:

```asm
CMP dword ptr [RSP + 0x30], 0x931C   ; mcode == 37660?
CMP dword ptr [RSP + 0x30], 0x9525   ; mcode == 38181?
```

These apply special handling to specific songs (likely fixing metadata for known problematic entries). They do not affect custom songs and do not impose any limit.

---

## Addresses Quick Reference

All addresses are Ghidra base (`0x180000000`). Note: running build has a ~+0x1F070 delta from the Ghidra DB.

| Symbol | Ghidra Address | Description |
|--------|---------------|-------------|
| `music_db_constructor` | `0x18019E2B0` | Creates empty vectors, initializes DB object |
| `master_loader` | `0x18019F230` | Calls license, musicdb, coursedb parsers |
| `license_parser` | `0x1801A0CA0` | Parses license.xml (1MB buffer) |
| `musicdb_parser` | `0x1801A0EC0` | Parses musicdb.xml (1MB buffer) — **primary target** |
| `coursedb_parser` | `0x1801A1030` | Parses coursedb.xml (1MB buffer) |
| `per_song_parse` | `0x18019F2F0` | Parses one `<music>` element (0x258 bytes) |
| `vector_push` | `0x1801A0430` | Pushes entry into music vector |
| `vector_grow` | `0x1801A0790` | Grows vector (1.5x factor, standard MSVC) |
| `post_process` | `0x1801A0630` | Post-processing after load |
| `game_allocator` | `[0x18042E258]` | Global allocator pointer |
| `avs_filesystem` | `[0x1806B5CD8]` | AVS file system pointer |
| `musicdb_xml_str` | `0x18035F370` | "/data/gamedata/musicdb.xml" string |
| `coursedb_xml_str` | `0x18035F640` | "/data/gamedata/coursedb.xml" string |
| `availability_check` | `[0x1806B5130]` | Per-song availability check function pointer |

### AOB Signatures

**XML buffer allocation** (all 3 parsers):
```
45 33 C0 BA 00 00 10 00 E8
```
3 hits. Patch byte at offset 6 (the `0x10` in the imm32).

**XML buffer read size** (all 3 parsers):
```
C7 44 24 20 00 00 10 00
```
3 hits. Patch byte at offset 6 (the `0x10` in the imm32).

---

## Open Questions

1. **Availability check behavior for custom songs**: Will songs with custom series values (>21) pass the availability check at `[0x1806b5130]`? If not, a bypass hook is needed. This can only be tested by actually adding custom songs to musicdb.xml.

2. **Song select UI scalability**: The song select screen uses a scrolling list. With 3000+ songs, does the UI lag or crash? The filter system should help (users filter by version/difficulty), but the "ALL MUSIC" view would show everything. Needs live testing.

3. **Score data / save system**: Does the game pre-allocate score storage based on song count? If there is a fixed-size score array indexed by mcode, adding songs with high mcodes could overflow it. Needs investigation if crashes occur during gameplay with custom songs.

4. **SSQ/chart file loading**: Each song needs chart data (SSQ files) in `data/mdb_apx/ssq/`. Missing SSQ files for custom songs could cause crashes during gameplay. The game likely handles missing files gracefully (the "INVALID SSQ" debug string suggests error handling exists), but this needs verification.
