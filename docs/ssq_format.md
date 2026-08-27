# DDR World SSQ File Format

Complete reverse-engineering of the SSQ step-file format used by DanceDanceRevolution. This document maps every byte of the format, validated against `gamemdx.dll` (DDR World, build MDX-003, 2026-03-24) and the **full set of 1523 SSQs** shipped in that release.

---

## 1. Quick reference

- **Endianness**: Little-endian throughout.
- **Alignment**: All chunks are dword-aligned (chunk length always a multiple of 4). Within a chunk, step bytes are 1-byte packed and the freeze-info block is 2-byte aligned.
- **Magic**: None. The file is identified by extension (`.ssq`) and path (`data/mdb_apx/ssq/{basename}.ssq`). Multiple charts for one song share one SSQ.
- **Terminator**: A final `00 00 00 00` sentinel (read by the parser as "chunk length = 0"). Some files have an additional 0–4 zero bytes of trailing padding; the game ignores them.
- **Chunk types used in DDR World**: 1 (tempo), 2 (events), 3 (steps), 4 (effect data — purpose not fully understood), 5 (effect data — auxiliary), 9 (song metadata, rare), 17 (section markers).
- **Ticks per second (TPS)**: **Not fixed.** Stored per-file in the tempo chunk's `param2`. In DDR World the only observed values are `1000` and `150` (roughly 50/50 split across 1523 files).
- **Measure length**: 4096 ticks per measure (whole note), used by all tick-valued fields (tempo chunk offsets, event chunk offsets, step chunk offsets).

File layout at the top level:

```
+------------------+
| Chunk: TEMPO     |   type = 1                   (required, exactly one)
+------------------+
| Chunk: EVENTS    |   type = 2                   (required, exactly one)
+------------------+
| Chunk: STEPS #1  |   type = 3, one difficulty
+------------------+   ...
| Chunk: STEPS #N  |
+------------------+
| Chunk: SECTIONS  |   type = 17                  (optional, rare)
+------------------+
| Chunk: EFFECTS-A |   type = 4                   (optional; paired with type 5)
+------------------+
| Chunk: EFFECTS-B |   type = 5                   (optional; paired with type 4)
+------------------+
| Chunk: METADATA  |   type = 9                   (optional, rare)
+------------------+
| 00 00 00 00      |   terminator
+------------------+
```

Ordering rules observed:
- Tempo chunk (type 1) is always first.
- Events chunk (type 2) always immediately follows tempo.
- Step chunks (type 3) follow the events chunk. The order of difficulties within step chunks varies between files (see §11 for observations) but Single charts are usually grouped together and so are Double.
- Auxiliary chunks (types 4, 5, 9, 17) come after step chunks.

The only **required** chunks for the step engine are tempo (1), events (2), and at least one step chunk (3). Everything else is optional.

### 1.1 Two format generations

DDR World ships SSQs authored under **two different pipelines**, distinguished by the tempo chunk's TPS value:

| TPS   | Generation | Files  | Auxiliary chunks allowed |
|-------|------------|--------|--------------------------|
| 1000  | Modern     | 763    | Types 1/2/3 only         |
| 150   | Legacy     | 760    | May include types 4, 5, 9, 17 |

The TPS-1000 format is a strict subset of the TPS-150 format — TPS-1000 files never contain auxiliary chunks, while TPS-150 files may include effect / camera / section / metadata chunks inherited from earlier DDR titles. Authoring tools targeting modern DDR should use TPS=1000 and emit only types 1, 2, 3.

---

## 2. Chunk header

Every chunk begins with a 12-byte header:

| Offset | Type | Name     | Description                                                 |
|--------|------|----------|-------------------------------------------------------------|
| +0x00  | u32  | length   | Total chunk size in bytes, **including this header**. Always a multiple of 4. |
| +0x04  | u16  | type     | Chunk type (1, 2, 3, 4, 5, 9, or 17 in DDR World)           |
| +0x06  | u16  | param2   | Type-specific metadata                                      |
| +0x08  | u16  | param3   | Type-specific metadata (usually an entry count)             |
| +0x0A  | u16  | param4   | Type-specific metadata. **Always 0 in DDR World** (1523/1523 files). |
| +0x0C  | ...  | body     | Chunk body, `length − 12` bytes                             |

### 2.1 Chunk lookup

The game has two ways of locating chunks:

- **Linear walk for tempo/events** — `FUN_1801ca230` walks from the start of the file, picking the first chunk whose `type` field matches.
- **Scan by (type, param2) for steps** — `FUN_1801cafe0` walks until it finds a chunk matching both `type` and `param2` (used to pick a specific difficulty).

Both loops share two termination conditions: they stop when `length == 0` (terminator reached) OR when they encounter a chunk with `param2 == 0xFFFF` (see §2.2).

Auxiliary chunks (types 4, 5, 9, 17) are NOT looked up by the step engine — they are consumed by other systems in the game (effects/camera scripting, metadata).

### 2.2 `param2 = 0xFFFF` sentinel

If any chunk has `param2 == 0xFFFF`, the game's chunk-lookup loops treat it as an "end-of-useful-data" marker and abort the search. This is a forward-compatibility mechanism.

Across 1523 sample files: **no chunk ever uses `param2 == 0xFFFF`**. Authoring tools should avoid this value.

### 2.3 File terminator

After the last real chunk, a single `00 00 00 00` dword ends the file. The parser reads this as "chunk length = 0" and stops. Some files have additional trailing zero bytes (0 to 4) — presumably padding from authoring-time alignment — which the game silently ignores.

---

## 3. Chunk type 1 — tempo / BPM changes

Exactly one per file. This is the authoritative source of both tempo changes and the file's tick rate.

| Header field | Value / meaning                                        |
|--------------|--------------------------------------------------------|
| type         | `1`                                                    |
| param2       | **Ticks per second (TPS)** — `150` or `1000` observed  |
| param3       | Number of tempo entries (N)                            |
| param4       | `0`                                                    |

### 3.1 Body layout

```
+------------------------+
| i32 time_offset[0]     |   N × 4 bytes
| i32 time_offset[1]     |
|        ...             |
| i32 time_offset[N−1]   |
+------------------------+
| i32 tempo_data[0]      |   N × 4 bytes
| i32 tempo_data[1]      |
|        ...             |
| i32 tempo_data[N−1]    |
+------------------------+
```

- `time_offset[i]` — position on the song timeline, in **measure ticks** (4096 per whole note).
- `tempo_data[i]` — cumulative position in **seconds-ticks** (elapsed time × TPS) measured from the song's logical start.

**Invariants** (verified across all 1523 files):
- `time_offset[0]` is always `0`.
- `tempo_data[0]` is an audio-sync offset in seconds-ticks (same unit as other `tempo_data[i]` values: `seconds × TPS`). Its distribution differs sharply between format generations:

  | TPS   | Files | `td0 = 0` | `td0 > 0`              | `td0 < 0`              |
  |-------|-------|-----------|------------------------|------------------------|
  | 150   | 760   | 405       | 292 (+1 to +635 = +4.23 s) | 63 (−1 to −7 = −47 ms) |
  | 1000  | 763   | 252       | 309 (+1 to +22 = +22 ms)   | 202 (−1 to −22 = −22 ms)|

  In TPS=1000 (modern), values are tightly bounded to ±22 ms — classic audio-sync fine-tune. In TPS=150 (legacy), positive values range up to +4.2 seconds, consistent with an audio pre-roll / leading-silence duration that the chart pre-advances to compensate for. The exact sign convention (which direction shifts audio vs chart) is inferred: a positive `td0` means the tempo-time axis starts partway through an `audio` pre-roll — i.e. by the time the chart reaches tick 0, `td0/TPS` seconds of audio-time have already elapsed. The opposite convention (chart leads audio) gives the same numbers with flipped sign and hasn't been disambiguated by live tracing.

Total body size: `2 × 4 × N = 8N` bytes.  
Total chunk size: `12 + 8N` bytes (always a multiple of 4).

### 3.2 Converting to BPM

For each pair of consecutive entries `i−1`, `i` (with `i ≥ 1`):

```
delta_measure = time_offset[i] − time_offset[i−1]     (measure ticks)
delta_seconds = tempo_data[i]  − tempo_data[i−1]      (seconds-ticks at TPS)

BPM = (delta_measure / 4096) / ((delta_seconds / TPS) / 240)
    = 240 × TPS × delta_measure / (4096 × delta_seconds)
```

With `TPS = 1000` this simplifies to `BPM = 60000 × delta_measure / (1024 × delta_seconds)`.  
With `TPS = 150` it simplifies to `BPM = 9000 × delta_measure / (1024 × delta_seconds)`.

`delta_measure == 0` signals a stop (see §3.3) — treat it as a special case.

### 3.3 Stops

A stop is encoded as two consecutive entries with the **same** `time_offset[i]` but different `tempo_data[i]`:

```
stop_seconds = (tempo_data[i] − tempo_data[i−1]) / TPS
```

The BPM formula in §3.2 would divide by zero (`delta_measure == 0`); parsers must special-case this.

Stops are observed in 214 of 1523 files. Some songs have many stops — `anan.ssq` has 60.

### 3.4 Runtime pre-computation

At chunk-load time `FUN_1801ca230` computes a normalized per-entry value:

```c
normalized[i] = round(tempo_data[i] × 1000 / TPS + 0.5f)
```

Stored as a `std::vector<int>` alongside the reader. This converts to a TPS-invariant millisecond-scale representation, which downstream code uses. The game tolerates any TPS value because everything gets rescaled here.

### 3.5 Worked example — `aeth.ssq` tempo chunk (TPS=150, with stops)

```
offset   bytes                      meaning
------   -----                      -------
0x0000   54 00 00 00                chunk length = 84
0x0004   01 00                      type = 1
0x0006   96 00                      param2 = 150 (TPS)
0x0008   09 00                      param3 = 9 (entries)
0x000A   00 00                      param4 = 0
0x000C   00 00 00 00                time_offset[0] = 0
0x0010   00 10 00 00                time_offset[1] = 4096
0x0014   00 20 01 00                time_offset[2] = 73728
0x0018   00 20 01 00                time_offset[3] = 73728    ← stop: same as [2]
0x001C   00 28 04 00                time_offset[4] = 272384
0x0020   00 a8 05 00                time_offset[5] = 370688
0x0024   00 a8 05 00                time_offset[6] = 370688   ← stop: same as [5]
0x0028   00 b0 05 00                time_offset[7] = 372736
0x002C   00 70 0a 00                time_offset[8] = 684032
0x0030   01 00 00 00                tempo_data[0]  = 1
0x0034   5e 00 00 00                tempo_data[1]  = 94
0x0038   99 06 00 00                tempo_data[2]  = 1689
0x003C   25 07 00 00                tempo_data[3]  = 1829     ← stop duration = (1829-1689)/150 = 0.933s
0x0040   e8 18 00 00                tempo_data[4]  = 6376
0x0044   7c 2a 00 00                tempo_data[5]  = 10876
0x0048   da 2a 00 00                tempo_data[6]  = 10970    ← stop duration = (10970-10876)/150 = 0.627s
0x004C   38 2b 00 00                tempo_data[7]  = 11064
0x0050   0d 47 00 00                tempo_data[8]  = 18189
```

BPM between entries 1 and 2: `240 × 150 × (73728 − 4096) / (4096 × (1689 − 94)) ≈ 383.7 BPM`. This matches ÆTHER's authored BPM (which is intentionally very high — the song is perceived as 192 BPM because the chart is half-time, but the tempo-chunk BPM is the true musical BPM).

Cross-validation against other TPS=150 files: `aaaa.ssq` → 93 BPM, `sota.ssq` → 190 BPM, `abys.ssq` → 142 BPM, all matching known values. Against TPS=1000 files: `fizz.ssq` → 175 BPM, `aceo.ssq` → 135 BPM, `blli.ssq` → 171 BPM, `asgm.ssq` → 146 BPM. The formula works uniformly regardless of TPS.

---

## 4. Chunk type 2 — event stream

Exactly one per file. Contains song-structural events the game consumes alongside steps (song start/stop markers, section-change flags, etc.).

| Header field | Value / meaning                                          |
|--------------|----------------------------------------------------------|
| type         | `2`                                                      |
| param2       | Always `1` in DDR World (1523/1523 files)                |
| param3       | Number of event entries (N)                              |
| param4       | `0`                                                      |

### 4.1 Body layout

```
+-----------------------+
| i32 time_offset[0]    |   N × 4 bytes
|        ...            |
| i32 time_offset[N−1]  |
+-----------------------+
| u8  event[0].code     |   N × 2 bytes
| u8  event[0].arg      |
|        ...            |
| u8  event[N−1].code   |
| u8  event[N−1].arg    |
+-----------------------+
```

Each event is 2 bytes: `(code, arg)` in file order.  
Total body size: `4N + 2N = 6N` bytes.  
Total chunk size: `12 + 6N` bytes.

### 4.2 Event dispatch

From `FUN_1801ca470` at the `pbVar22` stream:

| code | Parser action                                                        |
|------|----------------------------------------------------------------------|
| 1    | Silently consumed. No event is emitted. Purpose unknown.             |
| 2    | **Flow-control marker**. Emits a step-stream marker note (see §4.3). |
| 4    | Emits `{time, 4, arg}` into a separate events vector.                |
| 5    | Emits `{time, (arg & 0x3F) + 1, rng()}` into the events vector.      |
| other | Silently consumed.                                                  |

### 4.3 Code-2 sub-events

When `code == 2`, `arg` selects a sub-type which becomes the marker byte of a note pushed into the step-stream:

| arg | Marker byte | Typical position / inferred meaning              |
|-----|-------------|--------------------------------------------------|
| 1   | 0xFB        | At tick 0 — song start / music-on                |
| 2   | 0xFA        | At tick 4096 — chart start / "ready go" off      |
| 3   | 0xF9        | Near song end — pre-end cue                      |
| 4   | 0xFE        | At song end tick — song end / results trigger    |
| 5   | 0xF8        | At tick 4096 — alternate start-region cue        |
| other | (skipped) |                                                  |

The precise gameplay effect of each marker (which UI/audio transitions it triggers) is downstream of the parser and not part of the file format contract.

### 4.4 Canonical event sequence

The standard 6-entry pattern seen in most DDR World files:

```
event[0] = (0x01, 0x04)   at tick 0              -- code-1, ignored
event[1] = (0x02, 0x01)   at tick 0              -- song start (0xFB)
event[2] = (0x02, 0x02)   at tick 4096           -- chart start (0xFA)
event[3] = (0x02, 0x05)   at tick 4096           -- (0xF8)
event[4] = (0x02, 0x03)   at SONG_END−4096       -- pre-end cue (0xF9)
event[5] = (0x02, 0x04)   at SONG_END            -- song end (0xFE)
```

Some files have 7, 8, 10, or more entries — the extras are additional `(0x01, arg)` code-1 events placed at mid-song ticks. The step parser silently consumes them (verified by reading the dispatcher in `FUN_1801ca470` — the code-1 branch has no effect beyond advancing the event iterator). **No second consumer of the event chunk exists in `gamemdx.dll`**: no other function calls either chunk walker (`FUN_1801ca230` or `FUN_1801cafe0`) and no other function reads from the SsqReader's event-chunk pointer field (+0x20).

**Distribution of code-1 events across 1523 files**:

- 1519/1523 files contain at least one code-1 event (the canonical `(tick=0, arg=4)` is present in 1437 of those).
- 82 files contain code-1 events at ticks other than 0 and/or with arg ≠ 4.
- Observed arg values in those 82 files: `1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 17`. Dominant value is 4, followed by 3, 2, 5.
- Non-canonical code-1 events appear in both format generations (19 of 760 TPS=150 files, 63 of 763 TPS=1000 files) — more common in modern tooling.
- Zero overlap with type-17 section chunks (0 files have both non-canonical code-1 and type-17), so these events are NOT the authoring layer's section markers.

The most likely explanation is that code-1 is a reserved opcode that the current step engine doesn't act on; authoring tools still emit it (possibly as metadata for an editor or external system), and the game tolerates it.

### 4.5 Worked example — `fizz.ssq` events chunk

```
offset  bytes                      meaning
------  -----                      -------
0x0024  30 00 00 00                chunk length = 48
0x0028  02 00                      type = 2
0x002A  01 00                      param2 = 1
0x002C  06 00                      param3 = 6 (entries)
0x002E  00 00                      param4 = 0
0x0030  00 00 00 00                time_offset[0] = 0
0x0034  00 00 00 00                time_offset[1] = 0
0x0038  00 10 00 00                time_offset[2] = 4096
0x003C  00 10 00 00                time_offset[3] = 4096
0x0040  00 d0 04 00                time_offset[4] = 315392
0x0044  00 e0 04 00                time_offset[5] = 319488
0x0048  01 04                      event[0]: code=1 arg=4
0x004A  02 01                      event[1]: code=2 arg=1 (song start)
0x004C  02 02                      event[2]: code=2 arg=2 (chart start)
0x004E  02 05                      event[3]: code=2 arg=5
0x0050  02 03                      event[4]: code=2 arg=3 (pre-end)
0x0052  02 04                      event[5]: code=2 arg=4 (song end)
```

---

## 5. Chunk type 3 — step chart

One per difficulty/style combination. Contains the actual arrows for one chart.

| Header field | Value / meaning                                  |
|--------------|--------------------------------------------------|
| type         | `3`                                              |
| param2       | **Difficulty code** (see §5.1)                   |
| param3       | Number of step entries (N)                       |
| param4       | `0`                                              |

### 5.1 Difficulty codes (param2)

The difficulty code is a 16-bit value composed of two bytes:

```
  +----------+----------+
  |  slot    |  style   |
  +----------+----------+
   high byte   low byte
```

**Play style** (low byte):

| Byte | Style  | Active panels |
|------|--------|---------------|
| 0x14 | Single | 4 (bits 0–3)  |
| 0x18 | Double | 8 (bits 0–7)  |

**Slot** (high byte):

| Byte | Slot name  | Alt names (by mix)               |
|------|------------|----------------------------------|
| 0x01 | Basic      | Light                            |
| 0x02 | Difficult  | Standard, Another, Trick         |
| 0x03 | Expert     | Heavy, Maniac, SSR               |
| 0x04 | Beginner   |                                  |
| 0x06 | Challenge  | Oni, CHAOS                       |

**Full table of valid codes** (observed counts across 1523 DDR World files):

| Value   | Chart             | Count |
|---------|-------------------|-------|
| 0x0114  | Single Basic      | 1448  |
| 0x0214  | Single Difficult  | 1450  |
| 0x0314  | Single Expert     | 1449  |
| 0x0414  | Single Beginner   | 1485  |
| 0x0614  | Single Challenge  | 517   |
| 0x0118  | Double Basic      | 1448  |
| 0x0218  | Double Difficult  | 1448  |
| 0x0318  | Double Expert     | 1448  |
| 0x0418  | Double Beginner   | 11    |
| 0x0618  | Double Challenge  | 514   |

The DDR World game code (`FUN_1801ca350`, the SsqReader vtable[1] dispatcher) only maps difficulty indices `{0, 1, 2, 3, 4}` to slots `{4, 1, 2, 3, 6}`. Slot value `0x05` is **not accepted by DDR World**; charts using it won't be found by the game.

### 5.2 Body layout

```
+--------------------------+
| i32 time_offset[0]       |   N × 4 bytes
|        ...               |
| i32 time_offset[N−1]     |
+--------------------------+
| u8  step[0]              |   N × 1 byte
|        ...               |
| u8  step[N−1]            |
+--------------------------+
| (up to 1 byte of 2-byte  |   pad so freeze block starts at a 2-byte-aligned offset
|  alignment padding)      |
+--------------------------+
| u8 freeze[0].panels      |   F × 2 bytes  (F = count of zero-valued step bytes)
| u8 freeze[0].kind        |
|        ...               |
| u8 freeze[F−1].panels    |
| u8 freeze[F−1].kind      |
+--------------------------+
| (up to 2 bytes of dword  |   pad so chunk total length is dword-aligned
|  alignment padding;      |
|  value = 00 00 when      |
|  present)                |
+--------------------------+
```

**Offsets, relative to chunk start**:
- Header: `[0, 12)`
- Time offsets: `[12, 12 + 4N)`
- Step bytes: `[12 + 4N, 12 + 4N + N)`
- Freeze block: `[12 + 4N + ((N + 1) & ~1), chunk_end)`

The freeze block starts at `step_block_start + round_up_to_even(N)`. If `N` is odd there is one padding byte between the last step byte and the freeze block; the game does not read this byte. If `N` is even there is no padding.

**Size invariants**:
- `chunk_length = 12 + 4N + round_up_to_even(N) + 2F + trailing_pad` where `trailing_pad ∈ {0, 2}` to make `chunk_length` a multiple of 4.
- When present, the trailing pad is `00 00`. The parser treats it as a freeze entry with `kind = 0x00 ≠ 0x01`, so it has no effect (see §5.4).

### 5.3 Step byte encoding

Each step byte represents one "row" — a set of panels struck simultaneously at `time_offset[i]`.

| Value      | Meaning                                                     |
|------------|-------------------------------------------------------------|
| `0x00`     | **Freeze-end marker** — consume one `freeze` entry (§5.4)   |
| `0xFF`     | **Both-side shock arrow** (all 8 panels)                    |
| `0x0F`     | **P1-side shock arrow** (in Double mode)                    |
| `0xF0`     | **P2-side shock arrow** (in Double mode only)               |
| any other  | **Normal step** — bitmask of panels pressed                 |

Bit layout for normal steps:

| Bit | Mask | Single mode       | Double mode       |
|-----|------|-------------------|-------------------|
| 0   | 0x01 | P1 Left           | P1 Left           |
| 1   | 0x02 | P1 Down           | P1 Down           |
| 2   | 0x04 | P1 Up             | P1 Up             |
| 3   | 0x08 | P1 Right          | P1 Right          |
| 4   | 0x10 | (unused — 0)      | P2 Left           |
| 5   | 0x20 | (unused — 0)      | P2 Down           |
| 6   | 0x40 | (unused — 0)      | P2 Up             |
| 7   | 0x80 | (unused — 0)      | P2 Right          |

In Single mode the high nibble of a normal-step byte is always zero.

### Shock arrows

The game classifies a note as a shock arrow when **all 4 panels of a player's side** are hit simultaneously (check in `FUN_1801c6d80`). That means:

| Byte | Single-mode chart | Double-mode chart         |
|------|-------------------|---------------------------|
| 0x0F | Shock (P1 side)   | Shock (P1 side only)      |
| 0xF0 | (never occurs)    | Shock (P2 side only)      |
| 0xFF | Shock (P1 side)   | Shock (both sides)        |

**Empirical validation** — across 1523 files, 69 contain shock arrows, mostly on Challenge charts. `aceo.ssq` Double Challenge (the heaviest shock-containing chart observed) uses:
- 71 × `0xFF` (both-side shocks)
- 25 × `0x0F` (P1-only shocks)
- 23 × `0xF0` (P2-only shocks)

All three encodings are legitimate and used by the game.

In Single-mode charts, only `0xFF` appears; `0x0F` and `0xF0` never occur (the game's Single/Double dispatch uses different chart chunks).

### 5.4 Freeze block

A **freeze arrow** in SSQ is encoded in two parts:

1. An earlier normal-step byte that hits the panels which will be held (the freeze HEAD).
2. A later `0x00` step byte whose time offset = the freeze's end time (the freeze TAIL marker).
3. One `(panels, kind)` pair in the freeze block that identifies which of those panels the freeze-end applies to.

The freeze block contains **one entry per `0x00` step byte**, in file order.

| Offset | Type | Name   | Description                                                    |
|--------|------|--------|----------------------------------------------------------------|
| +0x00  | u8   | panels | Bitmask of panels whose freeze ends here (same layout as §5.3) |
| +0x01  | u8   | kind   | `0x01` = normal freeze. Other values are silently ignored.     |

### How the parser resolves a freeze

When the main parse loop in `FUN_1801ca470` encounters `step[i] == 0`:

```c
uint16_t pair = read_u16_le(freeze_block + 2*freeze_index);
if ((pair & 0xFF00) == 0x0100) {          // i.e. kind == 0x01
    emit_freeze(notes, player, pair & 0xFF, time_offset[i]);
}
freeze_index += 1;     // always consume, regardless of kind
```

`emit_freeze` (`FUN_1801cab40`) then:

1. Walks the **already-built note vector backward** starting from the most recent note.
2. For each panel bit in `panels`, finds the most recent earlier note where that panel was hit.
3. On that earlier note:
   - Stores the freeze **duration** (`freeze_end_time − head_time`) in the note's per-panel duration slot.
   - Promotes other panels in the same note that were hit but have duration=0 to "part of this freeze" (duration=1).
   - Marks the note as a freeze head.
4. Clears the matched panel bit from the pending set.
5. Continues walking backward until the pending set is empty.

### 5.5 Authoring freezes — the contract

To write a freeze note programmatically:

1. At the freeze **start time**, emit a normal step byte whose bits include the panels of the freeze head. (The step can include other non-freeze panels; those become ordinary hits.)
2. At the freeze **end time**, emit a `0x00` step byte.
3. Append a `(panels, 0x01)` entry to the freeze block, where `panels` is the bitmask of panels whose hold ends at this step.

**Multiple freeze heads can share one freeze-end** by listing multiple bits in the `panels` byte. The parser walks backward per-bit, so each bit can match a different earlier note.

**Trailing dword padding**: After emitting F freeze entries (2F bytes), if `2F + round_up_to_even(N) + N` is not a multiple of 4, append `00 00` to pad. The parser will treat it as a freeze entry with `kind = 0` which it ignores.

### 5.6 Worked example — `fizz.ssq` Single Basic chunk (12 freezes)

```
offset  bytes                           meaning
------  -----                           -------
0x01E4  44 04 00 00                     chunk length = 1092
0x01E8  03 00                           type = 3
0x01EA  14 01                           param2 = 0x0114 (Single Basic)
0x01EC  d3 00                           param3 = 211 (entries)
0x01EE  00 00                           param4 = 0
0x01F0  ...                             211 × i32 time_offsets  (ends at 0x053C)
0x053C  ...                             211 × u8 step bytes     (ends at 0x060F)
0x060F  ?                               (padding — 211 odd, so 1 byte pad to even)
0x0610  ...                             12 × 2-byte freeze entries
0x0628                                  next chunk begins

freeze entries:
0x0610  04 01        freeze[0]  panels=0x04 (Up)    kind=0x01
0x0612  01 01        freeze[1]  panels=0x01 (Left)  kind=0x01
0x0614  08 01        freeze[2]  panels=0x08 (Right) kind=0x01
0x0616  02 01        freeze[3]  panels=0x02 (Down)  kind=0x01
0x0618  01 01        freeze[4]  panels=0x01 (Left)  kind=0x01
0x061A  04 01        freeze[5]  panels=0x04 (Up)    kind=0x01
0x061C  02 01        freeze[6]  panels=0x02 (Down)  kind=0x01
0x061E  01 01        freeze[7]  panels=0x01 (Left)  kind=0x01
0x0620  08 01        freeze[8]  panels=0x08 (Right) kind=0x01
0x0622  01 01        freeze[9]  panels=0x01 (Left)  kind=0x01
0x0624  08 01        freeze[10] panels=0x08 (Right) kind=0x01
0x0626  01 01        freeze[11] panels=0x01 (Left)  kind=0x01
```

Chunk length check: `12 + 4×211 + 211 + 1 + 12×2 = 12 + 844 + 211 + 1 + 24 = 1092` ✓

---

## 6. Chunk type 4 — effect data stream A

An auxiliary chunk that appears in 96 of 1523 files. **Only appears in TPS=150 files** (0 TPS=1000 files contain it) — this is a legacy-format feature. Always paired with a type 5 chunk (see §7); no file has type 4 without type 5 or vice versa. Not consumed by the step/tempo/event parser. Believed to encode a **stage-lamp on/off script** synchronized to the song, based on the binary-valued `data` byte and the `arkMDXSetLamp` / `arkMDXSetDimlamp` / `arkMDXChangeSatellite` / `arkMDXChangeTapeled` exports in the game.

| Header field | Value / meaning                  |
|--------------|----------------------------------|
| type         | `4`                              |
| param2       | Always `1`                       |
| param3       | Entry count (N)                  |
| param4       | `0`                              |

### 6.1 Body layout

```
+-----------------------------+
| i32 time_offset[0..N−1]     |   N × 4 bytes
|   time_offset[0] = -99999   |     sentinel value — always `61 79 FE FF` LE
|   time_offset[1..N] = ticks |     (monotonically non-decreasing, measure ticks)
+-----------------------------+
| u8  data[0..N−1]            |   N × 1 byte
+-----------------------------+
| 0..3 trailing pad bytes     |   to dword-align total chunk; always zero
+-----------------------------+
```

Body size: `5N + pad` where `pad ∈ {1, 3}` in the observed samples (always the minimum needed to make the total chunk length a multiple of 4).

**Validated invariants** (all 96/96 type-4 chunks):
- `time_offset[0] == -99999` (i.e. the first 4 bytes are always `61 79 FE FF`). This is a fixed sentinel, not a magic value with any endianness interpretation — earlier investigations mistook it for a 4-byte header magic; it is simply `offsets[0]` taking a constant negative value. Authoring tools **must** emit this sentinel.
- Remaining time offsets (`offset[1..N]`) are monotonically non-decreasing.
- `data[i] ∈ {0x80, 0xFF}` — only those two values occur across all 96 × ~480 ≈ ~43600 data bytes. A pure binary toggle.
- Pad bytes are all-zero.
- A round-trip parser/serializer using this layout reproduces all 96 type-4 chunks byte-for-byte.

### 6.2 Per-byte data semantics (inferred)

The `data` byte is a single-bit state toggle sampled at each `time_offset[i]` tick. The two values observed are `0x80` and `0xFF`. Without live-tracing the consumer, the precise meaning isn't pinned down, but the strong signal from neighbouring symbols (`arkMDXSetLamp`, `arkMDXSetDimlamp`) and the binary nature of the data suggests one of:
- a stage-beacon / light on/off script, with `0xFF` = "illuminated" and `0x80` = "off / dim baseline",
- a cabinet tape-LED strip toggle,
- a dim-lamp intensity selector (low vs high).

All three would present as a simple two-state stream. Finding the type-4 consumer in `gamemdx.dll` would disambiguate; the type-5 chunk paired with it probably carries the richer scripting (per §7, section-A tag 0x45's `arg` byte looks enum-like).

### 6.3 Authoring

A round-trippable writer:
1. Emit `offset[0] = -99999` as the first i32.
2. Emit `offset[1..N]` as measure ticks in non-decreasing order.
3. Emit `N` u8 values, each either `0x80` or `0xFF`.
4. Append `1` or `3` zero bytes to make the total chunk length a multiple of 4.

Since type-4 and type-5 always co-occur and only appear in legacy (TPS=150) files, modern (TPS=1000) authoring should simply omit both chunks.

---

## 7. Chunk type 5 — effect data stream B

Paired with type 4 — the same 96 files have both. **Only appears in TPS=150 files.** Also auxiliary, not consumed by the step engine. The layout is byte-exact and round-trips for all 96 samples.

| Header field | Value / meaning      |
|--------------|----------------------|
| type         | `5`                  |
| param2       | `0`                  |
| param3       | Time-offset count (N)|
| param4       | `0`                  |

### 7.1 Body layout

```
+-----------------------------+
| i32 time_offset[0..N−1]     |   N × 4 bytes (ticks; monotonically non-decreasing)
+-----------------------------+
| record  sectA[0..N−2]       |   (N − 1) × 4 bytes  — "section A" records
+-----------------------------+
| u8[4]   separator           |   4 bytes: `95 14 00 00` (always, exactly once)
+-----------------------------+
| i32     sectB_count (M)     |   4 bytes
+-----------------------------+
| record  sectB[0..M−1]       |   M × 4 bytes  — "section B" records
+-----------------------------+
```

**Size**: body = `4N + 4(N − 1) + 4 + 4 + 4M = 8N + 4M + 4` bytes. Always a multiple of 4, so no trailing pad is needed.

**Validated invariants** (all 96/96 type-5 chunks):
- Exactly one occurrence of the separator `95 14 00 00` at a dword-aligned offset in the body.
- `sectA_record_count == N − 1` (not `N`).
- `sectB_count` field equals the actual number of trailing 4-byte records.
- Time offsets are monotonically non-decreasing (duplicate ticks are permitted — seen in e.g. `para.ssq`).
- Time offsets live in the same tick space as the tempo / step chunks (measure ticks, 4096 per whole note).
- `M ∈ [1, 8]` across the 96 samples; no file has `M = 0`.

### 7.2 Section A record format

Each 4-byte section-A record is:

| Offset | Type | Name   | Description                               |
|--------|------|--------|-------------------------------------------|
| +0x00  | u8   | tag    | Event-type tag (opcode enum, see below)   |
| +0x01  | u8   | arg    | Small tag-dependent integer argument      |
| +0x02  | u16  | param  | Tag-dependent parameter                   |

The N offsets define N − 1 **segments** `[offset[i], offset[i+1]]`, and `sectA[i]` is the event that applies during segment `i` (or fires at `offset[i]` and persists until `offset[i+1]`). This matches the observation that the number of records is one less than the number of offsets.

Section-A tag histogram across all 96 chunks (2892 records total):

| Tag   | Count | Typical form       | Notes                                       |
|-------|-------|--------------------|---------------------------------------------|
| 0x15  | 1427  | `15 aa 00 00`      | Most common. `param` is always 0.           |
| 0x45  | 912   | `45 aa 06 00`      | `param` almost always 6 (also 2, 4, 8).     |
| 0x49  | 226   | `49 aa pp pp`      | `param` varies widely.                      |
| 0x63  | 214   | `63 aa pp pp`      | Appears only in 96 files (type-5 range).    |
| 0x19  | 125   | `19 aa pp pp`      |                                             |
| 0x55  | 84    | `55 aa 00 00`      | `param` always 0.                           |
| 0x35  | 34    | `35 aa pp pp`      |                                             |
| 0x46  | 21    | `46 aa 04 00`      |                                             |
| 0x39  | 18    | `39 aa 27 00`      |                                             |
| 0x25  | 15    | `25 aa pp pp`      |                                             |
| 0x29  | 6     | `29 aa pp pp`      |                                             |
| 0x16  | 4     | `16 aa pp pp`      |                                             |
| 0x3a  | 3     | `3a 00 27 00`      |                                             |
| 0x1a  | 1     | `1a 02 27 00`      |                                             |

The precise per-tag semantics (camera cue, lamp pattern, particle effect, etc.) are not pinned down from static analysis alone — that would require live tracing. For authoring purposes the records can be copied through byte-for-byte from a reference file.

### 7.3 Section B record format

Each 4-byte section-B record is also `{u8 tag, u8 arg, u16 param}`. Only four tag values occur:

| Tag   | Count | Sample form      |
|-------|-------|------------------|
| 0x49  | 209   | `49 aa pp pp`    |
| 0xa9  | 131   | `a9 aa pp pp`    |
| 0x89  | 126   | `89 aa pp pp`    |
| 0x29  | 120   | `29 aa pp pp`    |

Section B is not indexed by any time offset — it is a plain list of M configuration/summary records. No correlation is observed between `sectB_count` and the number of step chunks (charts) in the file.

### 7.4 Authoring

A round-trip parser/serializer using the §7.1 layout reproduces all 96 observed type-5 chunks byte-for-byte. Authoring tools targeting DDR World should simply omit the chunk (none of the 763 TPS=1000 files contain it) unless they need to preserve legacy-format effect scripting, in which case passing through an existing chunk or constructing one using the layout above is sufficient. The step/tempo/event engine ignores the chunk entirely.

### 7.5 Worked example — `abys.ssq` type-5 chunk (smallest example)

```
offset  bytes                      meaning
------  -----                      -------
0x????  5c 00 00 00                chunk length = 92
0x????  05 00                      type = 5
0x????  00 00                      param2 = 0
0x????  09 00                      param3 = N = 9
0x????  00 00                      param4 = 0
# Body: N = 9 time offsets
+0x00   00 50 00 00                time_offset[0] =  20480
+0x04   00 90 00 00                time_offset[1] =  36864
+0x08   00 10 01 00                time_offset[2] =  69632
+0x0c   00 90 01 00                time_offset[3] = 102400
+0x10   00 10 02 00                time_offset[4] = 135168
+0x14   00 94 02 00                time_offset[5] = 168960
+0x18   00 14 03 00                time_offset[6] = 201728
+0x1c   00 54 03 00                time_offset[7] = 218112
+0x20   00 94 03 00                time_offset[8] = 234496
# Section A: N − 1 = 8 records
+0x24   45 1d 06 00                sectA[0] tag=0x45 arg=29 param=6  (segment 20480..36864)
+0x28   45 00 06 00                sectA[1] tag=0x45 arg=0  param=6  (segment 36864..69632)
+0x2c   45 15 06 00                sectA[2] tag=0x45 arg=21 param=6
+0x30   45 00 06 00                sectA[3] tag=0x45 arg=0  param=6
+0x34   45 1d 06 00                sectA[4] tag=0x45 arg=29 param=6
+0x38   45 15 06 00                sectA[5] tag=0x45 arg=21 param=6
+0x3c   45 1c 06 00                sectA[6] tag=0x45 arg=28 param=6
+0x40   45 1d 06 00                sectA[7] tag=0x45 arg=29 param=6
# Separator + section B
+0x44   95 14 00 00                separator
+0x48   01 00 00 00                sectB_count = 1
+0x4c   49 26 b5 00                sectB[0] tag=0x49 arg=38 param=0x00b5
```

Length check: `12 + 4×9 + 4×8 + 4 + 4 + 4×1 = 12 + 36 + 32 + 4 + 4 + 4 = 92` ✓

---

## 8. Chunk type 9 — song metadata (rare)

Observed in exactly 1 of 1523 files: `thr8.ssq` (DJ TECHNORCH's "8000000"). This is a special-case chart the game also treats specially in `FUN_180032240` (hard-coded filename check against `"thr8.ssq"`).

| Header field | Value / meaning      |
|--------------|----------------------|
| type         | `9`                  |
| param2       | `0`                  |
| param3       | `14392` (not an entry count — this field's role is unclear) |
| param4       | `0`                  |

### 8.1 Body layout (observed)

The 40-byte body in `thr8.ssq`:

```
00 44 4a 20 54 45 43 48 4e 4f 52 43 48 00   "\0DJ TECHNORCH\0"
00 00 00 00 00 00 00 00 00                  padding
02 05 08 09 0a                               5 bytes (purpose unknown)
02 06 08 09 0a                               5 bytes (purpose unknown; mirrors above with 2nd byte changed)
6f 00                                        u16 = 111
78 03                                        u16 = 888
6e 00                                        u16 = 110
00                                           trailing byte
```

The embedded "DJ TECHNORCH" is the song's artist. The rest is not decoded.

Since this appears in only one file and is not used by the step engine, authoring tools can omit it.

---

## 9. Chunk type 17 — section markers

Observed in 13 of 1523 files. A short chunk listing pairs of tick ranges.

| Header field | Value / meaning            |
|--------------|----------------------------|
| type         | `17` (`0x11`)              |
| param2       | `0`                        |
| param3       | Number of section pairs    |
| param4       | `0`                        |

### 9.1 Body layout

```
+--------------------------+
| i32 section[0].start     |   2N × 4 bytes
| i32 section[0].end       |
|        ...               |
| i32 section[N−1].start   |
| i32 section[N−1].end     |
+--------------------------+
```

Body size: `8 × N` bytes (strict — no padding).

### 9.2 Example — `inzo.ssq` section markers

```
offset  bytes           meaning
------  -----           -------
0x191C  1C 00 00 00     chunk length = 28
0x1920  11 00           type = 17
0x1922  00 00           param2 = 0
0x1924  02 00           param3 = 2 (section pairs)
0x1926  00 00           param4 = 0
0x1928  00 00 00 00     section[0].start = 0
0x192C  00 30 00 00     section[0].end   = 12288
0x1930  00 10 01 00     section[1].start = 69632
0x1934  00 30 01 00     section[1].end   = 77824
```

Semantics of the sections (e.g., "stealth zones", "special scoring zones") aren't pinned down. Not consumed by the step engine, so safe to omit.

---

## 10. Runtime note-stream (post-parse) — for reference only

This section describes what the game builds from the SSQ data in memory. It is **not** part of the file format.

After parsing, the game has a `std::vector<step::Note>`, with each `Note` being 0x60 (96) bytes. The first byte of each Note is a **marker** identifying what kind of event it represents:

| Marker | Meaning                                       | Source                            |
|--------|-----------------------------------------------|-----------------------------------|
| 0x00   | Normal step OR freeze head                    | non-zero step byte                |
| 0x02   | Freeze tail (synthesized post-parse)          | generated from freeze head + duration |
| 0x80   | Tempo-change event                            | tempo chunk                       |
| 0xF8   | Event-chunk code-2 arg=5                      | event chunk                       |
| 0xF9   | Event-chunk code-2 arg=3                      | event chunk                       |
| 0xFA   | Event-chunk code-2 arg=2 (chart start)        | event chunk                       |
| 0xFB   | Event-chunk code-2 arg=1 (song start)         | event chunk                       |
| 0xFD   | Placeholder/default (skipped — not emitted)   | (internal)                        |
| 0xFE   | Event-chunk code-2 arg=4 (song end)           | event chunk                       |

Each Note carries two parallel 8-entry int32 arrays at offsets `+0x1C` (panel hit flags) and `+0x3C` (per-panel freeze durations). The freeze-emit post-processing walks these to synthesize tail notes (marker `0x02`).

---

## 11. Reverse-engineering anchors

Key functions and addresses in `gamemdx.dll` (file-relative, image base `0x180000000`). Subject to change across builds; resolve by RTTI / AOB rather than hard-coding.

| Address        | Role                                                                      |
|----------------|---------------------------------------------------------------------------|
| `0x1804B7D38`  | RTTI: `.?AVSsqReader@step@@`                                              |
| `0x180383868`  | vtable: `step::SsqReader::vftable`                                         |
| `0x1801CA230`  | `SsqReader` init — locates type-1 and type-2 chunks, precomputes tempo    |
| `0x1801CA350`  | vtable[1] entry — maps (style, difficulty index) → difficulty code; dispatches to step parser |
| `0x1801CA470`  | **Main step parser** — interleaves tempo + event + step streams, emits Note records |
| `0x1801CAB40`  | `emit_freeze` — walks notes backward to convert step into freeze head     |
| `0x1801CAC50`  | Event-vector push helper (used for type-4 and type-5 event-chunk events)  |
| `0x1801CAFE0`  | Chunk-by-(type, param2) finder                                            |
| `0x1801C6D80`  | High-level analyze wrapper — calls parser, runs freeze post-processing    |
| `0x180032240`  | Per-difficulty dispatch — invokes SsqReader once per player (Single/Double) |

**Key strings** (all at `.rdata`):

| Address        | String                                                                 |
|----------------|------------------------------------------------------------------------|
| `0x18035A9C0`  | `"INVALID SSQ : mcode=%d, difficulty=%d, isAnalyzable=%d, noteNum=%d"` |
| `0x18035AA38`  | `"sota.ssq"` (hard-coded special case)                                 |
| `0x18035AA48`  | `"thr8.ssq"` (hard-coded special case)                                 |
| `0x18037E538`  | `"data/mdb_apx/ssq/"`                                                  |
| `0x18037E638`  | `"%s%s_%c.ssq"`                                                        |
| `0x18037E648`  | `"%s%s.ssq"`                                                           |

---

## 12. Open questions and future work

1. **Type 4 per-byte semantics** — layout is fully decoded (§6) and round-trips 96/96 byte-for-byte; the per-tick `data` byte is always either `0x80` or `0xFF` (a pure binary toggle). The precise meaning (stage lamp, tape-LED, dim-lamp, or beacon) requires live tracing of the consumer. Likely the same consumer that calls `arkMDXSetLamp` / `arkMDXSetDimlamp` / `arkMDXChangeSatellite` / `arkMDXChangeTapeled`.
2. **Type 5 per-tag semantics** — layout is fully decoded (§7) and round-trips byte-for-byte across all 96 samples, but the semantics of each section-A/B tag (camera cue vs lamp pattern vs particle effect) are not established from static analysis alone.
3. **Type 9 metadata format** — only one sample. A larger sample from older DDR titles might clarify whether this chunk has a stable schema or was hand-rolled for `thr8.ssq` specifically.
4. **Type 17 section semantics** — we have the layout but not the gameplay effect. Could be "challenge zones", "stealth regions", or "bonus multiplier sections".
5. **`tempo_data[0]` exact sign convention** — §3 describes the value as a seconds-ticks audio-sync offset, with TPS=1000 values in a tight ±22 ms range and TPS=150 values reaching +4.23 s. The exact direction convention (does a positive value delay the chart, or delay the audio?) is inferred from the tempo formula and hasn't been confirmed by a live trace. An attempted correlation against leading silence in a 13-song XWB audio sample was inconclusive because every song in that particular subset has `tempo_data[0] = 0`. Confirming the sign needs audio for songs like `chao.ssq` (TPS=150, td0=635) or `flfl.ssq` (TPS=1000, td0=22), which are not in the sample audio directory.
6. **Code-1 events — actual consumer unknown** — the step parser silently ignores them, and no other function in `gamemdx.dll` reads the event chunk. Non-canonical code-1 events appear in 82 of 1523 files with args ranging 1–17 (see §4.4). Possible consumers outside `gamemdx.dll` (e.g. the attached service DLL or a reserved future feature) have not been investigated.

---

## 13. Cross-file consistency summary

Across the full DDR World (MDX-003) sample of **1523 SSQ files**:

- **Tempo chunks**: 1523 (one per file).
- **TPS values**: `150` (760 files) and `1000` (763 files). Two conventions coexist.
- **Event chunks**: 1523 (one per file, always `param2 = 1`).
- **Step chunks**: 11,216 total across files.
- **Difficulty codes observed**: all 10 valid combinations. Distribution heavily favours Single/Double × {Basic, Difficult, Expert} (~1448 each); Single Beginner is the most common slot (1485). Double Beginner is rare (11 files). Challenge exists for ~1/3 of songs.
- **Shock arrows**: 69 files contain at least one `0xFF`/`0x0F`/`0xF0` byte. Concentrated on Challenge charts.
- **Stops**: 214 files contain at least one stop. `anan.ssq` has 60 stops.
- **`param4 != 0`**: never. (0/1523 files.)
- **`param2 == 0xFFFF`**: never. (0/1523 chunks.)
- **Missing terminator**: never. (0/1523 files.)
- **Truncated last chunk**: never. (0/1523 files.)
- **Chunk types observed**:
  - type 1 (tempo): 1523
  - type 2 (events): 1523
  - type 3 (steps): 11216
  - type 4 (effects A): 96
  - type 5 (effects B): 96 (always paired with type 4)
  - type 9 (metadata): 1 (only `thr8.ssq`)
  - type 17 (sections): 13
- **Type-4 layout invariants**: every type-4 chunk has `offset[0] = -99999` (the 4-byte sentinel `61 79 FE FF`); `offset[1..N]` monotonic; every `data[i]` is either `0x80` or `0xFF` (no other values across any of ~43,600 data bytes in the 96 chunks); pad bytes are zero (96/96). A round-trip parser/serializer using the §6.1 layout reproduces all 96 chunks byte-for-byte.
- **Type-5 layout invariants**: every type-5 chunk has exactly one `95 14 00 00` separator at a dword-aligned offset, the i32 count after the separator matches the count of trailing section-B records, and section A has exactly `N − 1` records where `N = param3` (96/96). A round-trip parser/serializer using the §7.1 layout reproduces all 96 chunks byte-for-byte.
