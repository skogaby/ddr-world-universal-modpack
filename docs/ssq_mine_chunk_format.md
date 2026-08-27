# SSQ Mine-Chunk Format Specification

> **Prerequisite**: Read [`ssq_format.md`](ssq_format.md) first. This document extends the general SSQ format spec with a new chunk type (`kind = 20`, `MINE_DATA`) that carries ITG-style mines for DDR World. All conventions from `ssq_format.md` apply unless called out here.

This specification is intended as a self-contained handoff for authoring tools (e.g. `ddr-chart-tools`) to implement SSC↔SSQ round-trip with mines. A reader with this document and `ssq_format.md` in hand should be able to write a parser and a serializer without needing to consult the DLL source.

---

## 1. Quick reference

- **Chunk kind**: `20` (`0x14`). Chosen to avoid collision with all chunk kinds observed in DDR World; see §7 for justification.
- **Purpose**: Carries mine entries — per-panel "hazard" notes the player must avoid stepping on, modeled on the StepMania/ITG `M` note type.
- **Per-difficulty**: One MINE_DATA chunk per difficulty, keyed by the **same `param2` difficulty code** as the corresponding step chunk (kind=3). A song with unique mines per difficulty therefore has one MINE_DATA chunk per difficulty in the file, mirroring the step-chunk convention from `ssq_format.md §5.1`. See §2.
- **Consumer**: The `NoteTypesExpansion` mod in the DDR World hook DLL. Vanilla DDR ignores this chunk (standard unknown-chunk path in `FUN_1801CA230`'s linear walk — unknown kinds are skipped by `length`).
- **Endianness**: Little-endian, same as all other SSQ content.
- **Alignment**: Chunk length is always a multiple of 4. The body's natural entry stride (8 bytes) already satisfies this — no trailing padding is needed unless the chunk is empty.
- **Body stride**: 8 bytes per mine entry. See §3.
- **Entry count**: Stored in the chunk header's `param3` field.
- **Tick space**: Same as FOOTSTEP (chunk type 3) — measure ticks, 4096 per whole note. Integer math end-to-end.
- **Placement in the file**: After all step chunks (type 3), before or after any legacy auxiliary chunks (types 4/5/9/17). The exact position does not matter — see §6.

---

## 2. Chunk header

Mine chunks use the standard 12-byte SSQ chunk header (see `ssq_format.md §2`):

| Offset | Type | Name    | Value for `MINE_DATA`                                                    |
|--------|------|---------|--------------------------------------------------------------------------|
| +0x00  | u32  | length  | `12 + 8·N`, where `N` is the number of mine entries. Always mul-4.       |
| +0x04  | u16  | type    | `20` (decimal) = `0x14`.                                                 |
| +0x06  | u16  | param2  | **Difficulty code** — matches the paired step chunk's `param2`. See §2.1.|
| +0x08  | u16  | param3  | `N` — number of mine entries in the body.                                |
| +0x0A  | u16  | param4  | `0`. Matches the DDR World convention (1523/1523 vanilla chunks use 0).  |

### 2.1 `param2` — difficulty code

`param2` carries the exact same `(slot, style)` code as the paired step chunk, per `ssq_format.md §5.1`:

```
  +----------+----------+
  |  slot    |  style   |
  +----------+----------+
   high byte   low byte
```

| Value   | Paired chart     |
|---------|------------------|
| `0x0114`| Single Basic     |
| `0x0214`| Single Difficult |
| `0x0314`| Single Expert    |
| `0x0414`| Single Beginner  |
| `0x0614`| Single Challenge |
| `0x0118`| Double Basic     |
| `0x0218`| Double Difficult |
| `0x0318`| Double Expert    |
| `0x0418`| Double Beginner  |
| `0x0618`| Double Challenge |

The DLL hooks the game's post-parse Analyze step, which runs per-difficulty (once for each step chunk the game decides to play). On each invocation the DLL has the current `(mode, difficulty)` pair in hand and looks up `(kind=20, param2=<that difficulty's code>)` to find the matching mine chunk. If no MINE_DATA chunk with a matching `param2` exists, the chart has no mines on that difficulty — this is a legal and common case (e.g. mines only on Challenge, no mines on Beginner).

Authoring tools must emit one MINE_DATA chunk per difficulty that has any mines. Emitting a single MINE_DATA chunk for "all difficulties" is not supported — the DLL will only find it for the specific `param2` it was stamped with.

### 2.2 Header notes

- `param2 == 0xFFFF` is the SSQ-wide "abort chunk lookup" sentinel (see `ssq_format.md §2.2`). No legitimate difficulty code is `0xFFFF`, so this is not a collision risk for properly-authored mine chunks.
- A chunk with `N == 0` (no mines) is legal: `length = 12`, body empty. Authoring tools should omit the entire chunk when a given difficulty has no mines rather than emit an empty one, but parsers must accept both.
- Duplicate `(type=20, param2=X)` pairs in the same file are malformed — the DLL's lookup stops at the first match, matching `ssq_format.md §2.1`'s documented behavior for the step-chunk finder.

---

## 3. Body layout

The body is a packed array of `N` fixed-size 8-byte mine entries. Entries are laid out contiguously with no separators or per-entry headers.

### 3.1 Entry struct

| Offset | Type | Name       | Meaning                                                      |
|--------|------|------------|--------------------------------------------------------------|
| +0x00  | i32  | beat_count | Tick position, in measure ticks (4096 per whole note).       |
| +0x04  | u8   | panels     | Panel bitmask — see §3.2.                                    |
| +0x05  | u8   | flags      | **Reserved. Must be `0` in v1.** See §3.3.                   |
| +0x06  | u16  | reserved   | **Reserved. Must be `0` in v1.**                             |

Total: 8 bytes per entry. No alignment padding between entries.

Body size is exactly `8 · N` bytes, always a multiple of 4, so no trailing alignment pad is ever required (contrast with the freeze-block layout in `ssq_format.md §5.2`).

### 3.2 Panel bitmask (`panels`)

Uses the exact convention from `ssq_format.md §5.3` (the step-byte bit layout):

| Bit | Mask   | Single mode  | Double mode   |
|-----|--------|--------------|---------------|
| 0   | `0x01` | P1 Left      | P1 Left       |
| 1   | `0x02` | P1 Down      | P1 Down       |
| 2   | `0x04` | P1 Up        | P1 Up         |
| 3   | `0x08` | P1 Right     | P1 Right      |
| 4   | `0x10` | (unused — 0) | P2 Left       |
| 5   | `0x20` | (unused — 0) | P2 Down       |
| 6   | `0x40` | (unused — 0) | P2 Up         |
| 7   | `0x80` | (unused — 0) | P2 Right      |

Multiple bits may be set in one entry (a "multi-panel mine") — this is a single mine occupying multiple panels at the same beat. At runtime the DLL emits one render entry per set bit but records one `MineEntry` per set bit in the sidecar, so hits on different panels are judged independently.

"Single mode" vs "Double mode" above is determined by the chunk's `param2` style nibble (`0x14` = Single, `0x18` = Double), which must match the paired step chunk.

**Illegal values**:
- `panels == 0x00` — a mine with no panels is a no-op. Writers must not emit; parsers should skip with a warning.
- `panels == 0xFF`, `0x0F`, or `0xF0` — these are the vanilla shock-arrow encodings (see `ssq_format.md §5.3`). Within a mine chunk they would classify as four-in-a-row shock patterns at the hook-inject site and may trigger the renderer's shock classifier. The v1 mod treats any entry whose set-bit count on one player's side is exactly 4 as malformed and skips it with a warning. Writers must split such patterns into multiple per-panel entries.
- In Single-mode chunks (param2 low byte = `0x14`), the high nibble (`0xF0`) of `panels` must be zero. Cross-player mines are only valid in Double-mode chunks (`0x18`).

### 3.3 `flags` and `reserved`

Both are reserved for forward compatibility. v1 requires both to be `0`. Parsers in v1 must:
- Accept `flags == 0` only. Any non-zero value should be logged at WARN level and the entry should be skipped (not applied as an ordinary mine — the field may denote semantic differences in a future version).
- Accept `reserved == 0` only. Non-zero is a malformed chunk; skip the entry with a warning.

Future use candidates for `flags` (not v1, documented here so authors understand why the byte exists):
- bit 0 — "do not deduct gauge on hit" (mine is cosmetic / practice-mode-only)
- bit 1 — "silent hit" (no sound effect)
- bit 2–7 — reserved for further per-mine variants (e.g. cold-mine vs standard mine). Per-entry `flags` is the canonical extension point for mine subtypes in this spec, since `param2` is already consumed by the per-difficulty key.

If a future revision claims a `flags` bit, this document will be bumped to v2 and the chunk-kind spec will be extended; chunk kind `20` stays.

### 3.4 Tick space and tempo conversion

`beat_count` lives in the same tick space as `ssq_format.md §5` step-chunk time offsets: 4096 ticks per whole note (measure), stored as a signed 32-bit integer, 0 at the song's logical start. Authoring tools should emit in the same tick space the step chunks use; no separate tempo conversion is needed at author time.

At load time the DLL converts each `beat_count` to a `musicCount` value using the file's tempo chunk (type 1) — the same linear interpolation over `time_offset[]` / `tempo_data[]` pairs the game itself uses for regular notes (see `ssq_format.md §3`). The mod performs this conversion via integer math matching the game's rounding convention, so the result is bit-identical to what the game would compute for a regular note at the same beat position.

---

## 4. Sorting and validity rules

Authoring tools must enforce the following invariants; parsers enforce the "validity" set and may log warnings on violations but should not crash.

### 4.1 Sort order (writer must emit in this order)

Entries are sorted ascending by `beat_count` as the primary key. Ties (multiple entries at the same `beat_count`) are sorted by `panels` ascending as a secondary key — purely for deterministic output. The game tolerates any order but sorting aids diffing and round-trip testing.

### 4.2 Co-location with regular notes

Mines may coexist with regular step entries at the same `beat_count`:

- **Same panel, same tick (regular-note + mine)**: The **arrow wins**. At runtime the DLL's mine injector detects this co-location and skips emitting a mine record for that panel (the mine becomes a no-op on that tick). Writers should avoid emitting such pairs in the first place — it's a wasted entry — but it is not a hard error. A warning is logged at load time.
- **Different panel, same tick**: Fully legal. A mine on panel P and an arrow on panel Q at the same `beat_count` are independent. The player must step on Q and not on P.
- **Same panel, same tick, mine + freeze-tail marker (`step == 0x00`)**: Legal but pathological. The mine is emitted normally; the freeze-tail marker is independent. Writers should avoid this construction.

### 4.3 Shock-arrow co-existence

Shock arrow rows (`step == 0xFF / 0x0F / 0xF0` in the step chunk) already cover all panels on one player's side. Emitting mines at the same `beat_count` as a shock row is legal — the mines are rendered and judged independently of the shock — but visually noisy. Writers should generally avoid it.

### 4.4 Density limits

No hard limit. The DLL pre-sizes its Notes vector growth budget based on `param3` at chunk-load time, so declaring a large `N` up-front is fine. Memory footprint scales at ~`0x60` bytes per mine entry injected (the vanilla `step::Note` struct size); a chart with 10,000 mines costs ~600 KB of app-heap memory — harmless on arcade hardware.

### 4.5 Negative tick values

`beat_count < 0` is **illegal for v1**. The DLL clamps mine positions to `beat_count >= 0` at load time and skips entries that fall outside the chart's positive tick range. Writers must emit non-negative values only.

---

## 5. StepMania `M` → `MINE_DATA` entry mapping

StepMania's SSC/SM notation uses `M` in a step row to denote a mine. The round-trip rule is:

| SSC input                                                               | SSQ output                                                                            |
|-------------------------------------------------------------------------|---------------------------------------------------------------------------------------|
| `M` on panel P at beat B                                                | One `MINE_DATA` entry with `beat_count = B · 4096`, `panels = (1 << P)`, `flags = 0`. |
| `M` on multiple panels at the same beat                                 | One `MINE_DATA` entry with multiple bits set in `panels`.                             |
| `M` co-located with a regular note `1/2/3/4` on a different panel       | One `MINE_DATA` entry at that beat + the regular step byte already carries the note.  |
| `M` co-located with a regular note `1/2/3/4` on the **same** panel      | Emit the regular note only. **Skip the mine** — runtime would drop it (arrow wins).   |

**SSC panel index → SSQ panel bit**: StepMania's dance-single `M` columns 0..3 map to SSQ bits 0..3 (Left, Down, Up, Right) in that order. Dance-double columns 0..7 map to bits 0..7. Matches the `pad` / `dance` style panel order used by StepMania internally; no per-column remapping needed.

**Lifts (`L`), rolls (`R`), fakes (`F`)** are not handled by this spec — they belong to future chunk kinds. Strip them or warn-and-drop at serialization time.

---

## 6. Vanilla-compatibility and forward-compatibility guarantees

### 6.1 Vanilla DDR (unmodded)

Vanilla DDR World's chunk walker (`FUN_1801CA230` for tempo/events, `FUN_1801CAFE0` for steps) only looks up chunks by known (type, param2) pairs. An `MINE_DATA` chunk (`type = 20`) matches none of them. Both functions advance by the chunk's `length` field when they encounter a chunk they don't match, which means an unknown chunk is silently skipped. This behavior is verified across the 1523 vanilla SSQ files — all use kinds `{1, 2, 3, 4, 5, 9, 17}`, and kind `20` is guaranteed not to conflict with any existing lookup.

Concretely, loading a mine-enabled SSQ on unmodded DDR World produces:
- Tempo parse: unaffected (chunk 1 still found first).
- Events parse: unaffected (chunk 2 still found immediately after tempo).
- Step parse: unaffected (chunk 3 still found; unknown chunk 20 is stepped over).
- Rendering: no mines appear.
- Judge: no mine judgments.
- Score: identical to the same chart with the mine chunk removed.

This property is also required by US-11 of the `20260427-itg-mine-support` feature.

### 6.2 Modded DDR with mod disabled

When the hook DLL is loaded but the `note-types-expansion` mod is toggled off via the mod menu, the mod's Analyze hook is unregistered. The mine chunk is again silently skipped at load time — identical behavior to vanilla. Toggling the mod back on before the next chart load re-activates mine handling.

### 6.3 Forward compatibility within modded DDR

Mine-variant extensions land via the per-entry `flags` byte (see §3.3), not via `param2` — the latter is already consumed by the per-difficulty key. Specifically:

- A v2 DLL that uses a `flags` bit will set it when serializing (e.g. from an updated `ddr-chart-tools`) and honor it when parsing. A v2-emitted chart loaded on a v1 DLL will log a WARN (unknown flag) and skip the affected entries. The chart still loads; the mines just don't appear on older DLLs. This is the desired graceful-degradation behavior.
- New difficulty codes (if Konami ever extends the slot/style matrix in `ssq_format.md §5.1`) automatically extend the mine-chunk discriminator space — no spec change required on this doc's side.

### 6.4 If Konami adds a chunk kind 20

The one uncovered risk. Konami has used kinds `{1,2,3,4,5,9,17}` historically and has headroom up through ~`0x10` / `0x11` by convention, so kind 20 has visible buffer. If Konami's next release claims kind 20 with different semantics, our chunks would be misinterpreted.

Mitigation:
- If Konami's chunk uses `param2` values that don't match any valid difficulty code (the set `{0x0114, 0x0214, 0x0314, 0x0414, 0x0614, 0x0118, 0x0218, 0x0318, 0x0418, 0x0618}`), the two coexist — the DLL would need a lookup tightening to require membership in the difficulty-code set on its mine-chunk find.
- If Konami's chunk collides on a valid difficulty code, re-authoring mine-enabled charts against a new kind (21+) is a tooling task, not a chart-author task, and can be shipped in a single `ddr-chart-tools` release plus a matching DLL update.

---

## 7. Collision-avoidance justification (why `kind = 20`?)

DDR World's in-use chunk kinds, from `ssq_format.md §1`:

| Kind | Purpose                          | Present in DDR World? |
|------|----------------------------------|-----------------------|
| 1    | Tempo                            | Yes (every file)      |
| 2    | Events                           | Yes (every file)      |
| 3    | Steps (one per chart)            | Yes (every file)      |
| 4    | Effect-data stream A (stage lamps — legacy only) | Yes (96 TPS=150 files) |
| 5    | Effect-data stream B (paired with type 4)        | Yes (96 TPS=150 files) |
| 9    | Song metadata (rare)             | Yes (1 file — `thr8.ssq`) |
| 17   | Section markers                  | Yes (rare, legacy TPS=150 only) |
| other| (none observed across 1523 files)| No                    |

Candidates for a new kind:

- **Re-use one of 4/5/9/17 with special `param2`**: the legacy types (4,5,9,17) have established semantics in 96 charts. A DDR World build upgraded to parse a mine-kind `param2` of type 4 would need to disambiguate by content — brittle across 96 pre-existing chunks. Rejected.
- **Pick a low unused value (6, 7, 8, 10–16, 18, 19)**: these sit inside the range Konami has demonstrably used (17 is in use, 9 is in use). Konami could plausibly claim any of them in a future release. Rejected for collision risk.
- **Pick a high value (20, 21, …)**: clear of the Konami-used range by at least 2 increments; gives clearance for Konami to add kinds through ~19 without collision. **Chosen**.
- **Pick a very high value (50, 100)**: maximally conservative but arbitrary. 20 is tight enough to feel motivated, loose enough to avoid immediate risk. Chosen.

Future note types take 21, 22, … — see `20260427-itg-mine-support/design.md` Decision 2. This spec owns kind 20 only.

---

## 8. Worked example — annotated byte-level

A minimal mine-enabled SSQ fragment. Only the mine chunk is shown; the rest of the file (tempo / events / step chunks) follows the standard layout from `ssq_format.md`. Assume this chunk appears after the step chunk(s), before the `00 00 00 00` terminator.

### 8.1 Scenario

Three mines, on the Double Expert chart of a TPS=1000 song (i.e. the paired step chunk has `param2 = 0x0318`, per `ssq_format.md §5.1`):

1. At beat 2.0 (half a measure = 2048 ticks) — single mine on P1 Up (bit 2, `0x04`).
2. At beat 4.0 (one measure = 4096 ticks) — multi-panel mine on P1 Left + P1 Right (bits 0 + 3, `0x09`).
3. At beat 4.0 — a second mine, same beat, on P2 Left (bit 4, `0x10`). (Shows co-location across players at the same tick.)

Additionally, the chart has a regular step byte (not shown here — lives in the step chunk) at beat 4.0 hitting P1 Down (bit 1, `0x02`) — a co-location of a regular arrow with the multi-panel mine at the same tick on a **different** panel. The mine chunk emits the mine normally (§4.2 case 2).

### 8.2 Entry values

Converting beats to ticks: `beat × 4096`.

| # | beat | beat_count (i32) | panels (u8) | flags (u8) | reserved (u16) |
|---|------|------------------|-------------|------------|----------------|
| 0 | 2.0  | 8192 = `0x00002000` | `0x04`    | `0x00`     | `0x0000`       |
| 1 | 4.0  | 16384 = `0x00004000` | `0x09`   | `0x00`     | `0x0000`       |
| 2 | 4.0  | 16384 = `0x00004000` | `0x10`   | `0x00`     | `0x0000`       |

Three entries × 8 bytes = 24 bytes of body. Total chunk length = `12 + 24 = 36` bytes, already dword-aligned.

### 8.3 Byte stream

```
offset  bytes                      meaning
------  -----                      -------
+0x00   24 00 00 00                chunk length = 36
+0x04   14 00                      type = 20 (MINE_DATA)
+0x06   18 03                      param2 = 0x0318 (Double Expert — paired step chunk's code)
+0x08   03 00                      param3 = 3 (entry count)
+0x0A   00 00                      param4 = 0
  --- body ---
+0x0C   00 20 00 00                entry 0: beat_count = 8192 (beat 2.0)
+0x10   04                         entry 0: panels = 0x04 (P1 Up)
+0x11   00                         entry 0: flags = 0
+0x12   00 00                      entry 0: reserved = 0
+0x14   00 40 00 00                entry 1: beat_count = 16384 (beat 4.0)
+0x18   09                         entry 1: panels = 0x09 (P1 Left + P1 Right)
+0x19   00                         entry 1: flags = 0
+0x1A   00 00                      entry 1: reserved = 0
+0x1C   00 40 00 00                entry 2: beat_count = 16384 (beat 4.0)
+0x20   10                         entry 2: panels = 0x10 (P2 Left)
+0x21   00                         entry 2: flags = 0
+0x22   00 00                      entry 2: reserved = 0
```

Length check: body = 3 × 8 = 24 bytes; total = 12 + 24 = 36 bytes = `0x24` = dword-aligned. ✓

Co-location note: at beat 4.0 the chart's step chunk is independently emitting a step byte `0x02` (P1 Down). The mine-chunk entries at the same tick (entries 1 and 2) cover panels that do not overlap P1 Down — §4.2 case 2 applies and no skipping is required.

---

## 9. Validation checklist (for writers)

Before emitting a `MINE_DATA` chunk, a writer must verify:

- [ ] `type == 20` (`0x14`) in the header.
- [ ] `param2` is the difficulty code of the paired step chunk — one of the 10 valid values from `ssq_format.md §5.1` (`0x0114`, `0x0214`, `0x0314`, `0x0414`, `0x0614`, `0x0118`, `0x0218`, `0x0318`, `0x0418`, `0x0618`). See §2.1.
- [ ] `param3 == N` (entry count) matches the body length.
- [ ] `param4 == 0` in the header.
- [ ] Header `length` field equals `12 + 8·N`.
- [ ] No other MINE_DATA chunk in the file shares the same `param2` — one chunk per difficulty (see §2.2).
- [ ] A step chunk with the same `param2` exists in the file (a MINE_DATA chunk with no paired step chunk is an orphan and will be ignored by the DLL).
- [ ] For every entry:
  - [ ] `beat_count >= 0`.
  - [ ] `panels != 0`.
  - [ ] `panels` is not `0xFF`, `0x0F`, or `0xF0` (shock encodings).
  - [ ] In Single-mode chunks (`param2` low byte = `0x14`), `panels & 0xF0 == 0`.
  - [ ] `flags == 0`.
  - [ ] `reserved == 0`.
- [ ] Entries are sorted ascending by `beat_count` (ties by `panels`).
- [ ] No same-panel-same-tick collision with a regular step byte in the paired step chunk (drop the mine at serialization time if present).
- [ ] The chunk is placed after the step chunks and before the `00 00 00 00` terminator.

Parsers in v1 should accept malformed input where possible (logging WARN-level diagnostics per the bulleted rules in §3 and §4) and must refuse to crash. A parser should abort processing of the chunk and continue to the next one if `param3 × 8 + 12 != length` (i.e. the declared entry count is inconsistent with the declared length), or if `param2` is not a valid difficulty code.

---

## 10. Versioning

This is version **v1** of the mine-chunk spec. The chunk kind (`20`) is permanent for the mine family; subsequent revisions bump the spec version and may:

- Assign meaning to `flags` bits (new behavior gated on a bit; v1 DLLs degrade gracefully by skipping the entry). This is the canonical per-mine-variant extension point — see §3.3 and §6.3.
- Never change the meaning of `param2`: it is permanently the paired step chunk's difficulty code. Per-mine subtypes (cold mines, cosmetic mines, etc.) land on `flags` bits, not on `param2`.
- Never change the 8-byte entry stride (doing so requires a new chunk kind — 21+ is the correct home for a structurally different note type).

Changes to this file should include a "Version history" table appended to this section when v2 arrives.
