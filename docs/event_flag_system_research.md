# DDR World Event Flag System — Reverse Engineering Research

**Binaries analyzed**:
- `gamemdx.dll` (build `20260324`) — event dispatch, struct consumers
- `ess.dll` (build `20260324`) — network packet receivers, data transfer to gamemdx

**Status**: second pass. Incorporates verified findings from both modules and corrects several speculative claims from the first pass (see [Corrections from v1](#corrections-from-v1) at the bottom).

All addresses in this document are **Ghidra file addresses** (gamemdx file base `0x180000000`, ess file base `0x180000000`). Runtime addresses depend on ASLR; compute as `runtime = file_addr + (runtime_module_base - 0x180000000)`.

---

## Table of Contents

1. [Overview](#overview)
2. [Data Flow — Server to Dispatcher](#data-flow--server-to-dispatcher)
3. [Packet Format — `playerdata_load`](#packet-format--playerdata_load)
4. [Packet Format — `mergeddata_load`](#packet-format--mergeddata_load)
5. [CSV Format — `event_str`](#csv-format--event_str)
6. [Parsed Event Struct (0x28 bytes)](#parsed-event-struct-0x28-bytes)
7. [Difficulty Bitmask Encoding](#difficulty-bitmask-encoding)
8. [Per-Player Work Fields Touched By Events](#per-player-work-fields-touched-by-events)
9. [Song Flag Bits (`song_data[+0x1AC]`)](#song-flag-bits-song_data0x1ac)
10. [Global Session Struct (`DAT_1806EA478`)](#global-session-struct-dat_1806ea478)
11. [Full Event Type Dispatch Table (`ReflectPlayerWork`)](#full-event-type-dispatch-table-reflectplayerwork)
12. [Hardcoded Event ID Queries](#hardcoded-event-id-queries)
13. [Helper Functions Reference](#helper-functions-reference)
14. [`SetRewardEventSaveData` (save-table writer)](#setrewardeventsavedata-save-table-writer)
15. [Backend Implementation Guide](#backend-implementation-guide)
16. [Verification Notes](#verification-notes)
17. [Open Questions](#open-questions)
18. [Corrections from v1](#corrections-from-v1)

---

## Overview

DDR World's "event flag" system is the server-side mechanism the game uses to gate features and unlocks on a per-profile basis. A single events table controls all of:

- **Song / chart unlocks** — per-difficulty unlocks, tier unlocks, pre-release (preview) flags
- **Song metadata flags** — per-mcode flags set on `song_data[+0x1AC]` (bit 0, 10, 11, 15, 16, 17, 25, 26)
- **Global feature toggles** — Extra Stage, Encore Extra Stage, Dan Ranking UI, World League UI, Platinum membership badge, Galaxy Play gate, etc.
- **Timed / progression events** — threshold-based tracking with server-synchronized savedata
- **Per-mcode std::map storage** — used by Brave trial, league registration, etc.

Events are delivered as an array of rows inside the player data response. Each row is a 7-tuple `(eventid, eventtype, eventno, condition, reward, comptime, savedata)`. Two network endpoints deliver events with different encodings (see [Data Flow](#data-flow--server-to-dispatcher)). Both endpoints eventually produce the same in-memory layout — a 0x28-byte struct in a table of up to 0x400 entries per player.

The game processes events each time the per-player "work" struct is reflected from the server data, via `ReflectPlayerWork(gamemdx+0x13C80)`. This function iterates the event table and dispatches each row via a large `switch(eventtype)`. The effects range from writing single-byte flags on song data to registering mcodes into std::map structures used by UI code.

---

## Data Flow — Server to Dispatcher

**v2 correction**: Earlier drafts assumed playerdata and mergeddata events converge on the same master table. They **do not**. They live in different storage and are processed by two different dispatchers, one per-player and one session-global.

### playerdata_load path (per-player event table)

```
Server
  │
  ▼
ess.dll : sys_playerdata_load_receiver (FUN_180025D70)
  │   XML → ess playerdata buffer (0x54BFC0 bytes per player)
  │   Events written as raw CSV strings at buf+0x5081F0 (1024 × 257 bytes)
  │
  ▼
ess.dll : ess_base3_playerdata_load_wait
  │   Ordinal_240 transfer (0x54BFC0 bytes) → gamemdx staging
  │
  ▼
gamemdx.dll : FUN_18001BAC0 case 0x1B (playerdata state machine)
  │   Destination: &DAT_1804E1C78 + player * 0x101BC8
  │   On success, calls ReflectPlayerWork(player)
  │
  ▼
gamemdx.dll : (CSV parser — not yet located; populates DAT_1805D5E60)
  │   At some point between the transfer and ReflectPlayerWork, CSVs at buf+0x5081F0
  │   are parsed into 0x28-byte structs at DAT_1805D5E60 + player * 0x101BC8.
  │   See Open Questions.
  │
  ▼
gamemdx.dll : ReflectPlayerWork (+0x13C80) — per-player dispatcher
  │   Iterates DAT_1805D5E60 + player * 0x101BC8 (up to 0x400 entries)
  │   Terminator: eventid < 1
  │   switch(eventtype) dispatch
  │   Effects scoped to ONE player's work struct
```

### mergeddata_load path (session-global event table)

```
Server
  │
  ▼
ess.dll : sys_mergeddata_load_receiver (FUN_18002DD40)
  │   XML → ess mergeddata buffer (0xA008 bytes)
  │   Events written as pre-parsed 0x28 structs at buf+0x10 (1024 × 0x28)
  │
  ▼
ess.dll : ess_base3_mergeddata_load_wait
  │   Ordinal_240 transfer (0xA008 bytes) → gamemdx
  │
  ▼
gamemdx.dll : FUN_1800A7840 case 0x4 (entry-flow mergeddata state)
  │   Destination: DAT_1804BEAB0 (static buffer in gamemdx data section)
  │   On success, calls FUN_180016DC0
  │
  ▼
gamemdx.dll : FUN_180016DC0 — session-global mergeddata dispatcher
  │   Reads events from &DAT_1804BEAB0 + 0x10 + i * 0x28 (up to 0x400 entries)
  │   Writes session flags to *DAT_1806EA478[+0x138..+0x13D] from packet header
  │   switch(eventtype) dispatch — AFFECTS BOTH PLAYERS
  │   Uses FUN_1801DB750 (session-wide bitfield setter) instead of
  │        FUN_1801E5F50 (per-player). Handles a SUBSET of event types;
  │        see "Mergeddata dispatcher differences" below.
```

### Active-event queries & save writes

```
gamemdx.dll : feature code reads the active-event vector (work+0x80)
  │   IsEventIdActive(eventid) at +0x1DBD50
  │   (and a few inline std::find lookups; see Hardcoded Event ID Queries)
  │
  ▼
gamemdx.dll : save writes back via SetRewardEventSaveData (+0x1C220)
      writes DAT_1804C8AC0 (save table, stride 0xA00, 0x40 entries per player)
      → later serialized via playerdata_save to the server
```

### Two dispatchers, two tables

| Dispatcher            | Source table           | Per-player? | Triggered by            | Scope of effects |
|-----------------------|------------------------|-------------|-------------------------|------------------|
| `ReflectPlayerWork`   | `DAT_1805D5E60`        | yes (stride 0x101BC8) | playerdata_load         | ONE player's work struct |
| `FUN_180016DC0`       | `DAT_1804BEAB0` (+0x10)| no (global)  | mergeddata_load         | BOTH players' work structs (via iteration) + global session flags |

**Implication for servers**: if you want an event to affect both players simultaneously (e.g. a shared "event active" flag), send it via `mergeddata_load`. If you want per-player state (unlocks, progress), send via `playerdata_load`.

### Mergeddata dispatcher differences (`FUN_180016DC0` vs `ReflectPlayerWork`)

The mergeddata dispatcher at `FUN_180016DC0` is NOT a clone of `ReflectPlayerWork`. Key differences:

- **No cases 100, 101, 200, 201, 1000** in mergeddata — these types are silently ignored on the mergeddata path.
- **Type 0xD simplified**: only the `eventno != 0 && comptime != 0` branch is present. The other three branches (setters for `eventno == 0`, save-only, etc.) are absent.
- **Type 9999 side effects reduced**: eventids 97, 98 still toggle `+0x13E`, but 112/113 (toggling `+0x13F`) and 108 (reserved no-op) do NOT appear — only 97/98 are hardcoded here.
- **Different bitfield setter**: uses `FUN_1801DB750` (session-wide, likely applies to both players) instead of `FUN_1801E5F50` (per-player).
- **Type 10000 applies to both players**: pushes `eventid` onto BOTH P1 and P2 active-event vectors in one iteration. Calls `SetRewardEventSaveData` once per player.
- **Header prefix**: before iterating events, the dispatcher copies the packet's first 5 bytes into the session struct: `*DAT_1806EA478[+0x138] = result` (s32 from `DAT_1804BEAB0`), `[+0x13C] = league_class` high byte, `[+0x13D] = is_advance_border_exceeded` byte. (`is_exists_subscribed_user` at packet offset +0x0D is NOT copied to the session struct — it ends up in a separate location.)
- **Song flag bit sets are identical**: same bit 11 / 15 / 16 / 17 / 25 / 26 writes as ReflectPlayerWork for the types that handle song flags (0x06, 0x12, 0x19, 0x2C, 0x32, 0x34, 0x5A).

The mergeddata dispatcher effectively serves as a "mid-session event refresh" that can change global feature gates without re-reading the full playerdata. This is how the game keeps Dan Rank / League / Extra Stage flags updated between songs.

---

## Packet Format — `playerdata_load`

Received by `sys_playerdata_load_receiver` in ess.dll (`FUN_180025D70`). The receiver unpacks a long XML response (result header, common, option, lastplay, filtersort, checkguide, rival, score, **event**, league, customize, brave, grade) into a single per-player buffer of `0x54BFC0` bytes.

Event section (relevant to this doc):

```xml
<event>
  <event_str __type="str">CSV</event_str>       <!-- up to 1024 of these -->
  <event_str __type="str">CSV</event_str>
  …
</event>
```

Each `<event_str>` is up to 0x101 (257) bytes. They are copied into the ess buffer at:

```
buf + 0x5081F0 + i * 0x101   (i = 0 .. 0x3FF)
```

**ess.dll does not parse the CSV**. The strings remain in buffer form until gamemdx's CSV parser fires.

**Terminator**: the XML `<event>` node iteration stops when `<event_str>` has fewer than 1024 children. The parsed event table uses `eventid < 1` as a separate sentinel.

---

## Packet Format — `mergeddata_load`

Received by `sys_mergeddata_load_receiver` in ess.dll (`FUN_18002DD40`). The receiver unpacks a much smaller response (result header + league flags + event table) into a per-player buffer of `0xA008` bytes.

Layout of the ess mergeddata buffer:

| Offset | Size  | Field                         | XML element                  |
|--------|-------|-------------------------------|------------------------------|
| +0x00  | 4     | result                        | `<result>` (s32)             |
| +0x08  | 4     | league_class                  | `<league_class>` (s32)       |
| +0x0C  | 1     | is_advance_border_exceeded    | `<is_advance_border_exceeded>` (bool) |
| +0x0D  | 1     | is_exists_subscribed_user     | `<is_exists_subscribed_user>` (bool)  |
| +0x10  | 0x28 × 0x400 | event struct array     | 1024 × `<event>`             |

Each `<event>` contains seven child nodes, parsed directly into a 0x28-byte struct:

```xml
<event>
  <eventid   __type="s32">…</eventid>
  <eventtype __type="s32">…</eventtype>
  <eventno   __type="s32">…</eventno>
  <condition __type="s32">…</condition>
  <reward    __type="s32">…</reward>
  <comptime  __type="u64">…</comptime>
  <savedata  __type="u64">…</savedata>
</event>
```

The field-to-offset mapping established here is the authoritative struct layout used throughout the game (see next section).

After all XML nodes are parsed, `ess_base3_mergeddata_load_wait` memcpys the entire 0xA008-byte buffer to a gamemdx-supplied destination pointer. The destination becomes the input to gamemdx's event table updater (which merges it into `DAT_1805D5E60`).

---

## CSV Format — `event_str`

```
eventid,eventtype,eventno,condition,reward,comptime,savedata
```

Comma-separated, no quoting, no whitespace. The field semantics are identical to the mergeddata XML fields.

| Field       | Type | Meaning                                                                                                                                      |
|-------------|------|----------------------------------------------------------------------------------------------------------------------------------------------|
| `eventid`   | s32  | Unique identifier per event row. `eventid < 1` terminates iteration. Queried by name for global feature gates (see [Hardcoded Event ID Queries](#hardcoded-event-id-queries)). |
| `eventtype` | s32  | Dispatch code — selects the handler in the `ReflectPlayerWork` switch. Values 1..200 are normal; 1000, 9999, 10000 are special.              |
| `eventno`   | s32  | Secondary identifier. Semantics depend on `eventtype`: for per-difficulty types it's the chart tier index (0..4); for 9999/10000 it's 0; otherwise a sub-key. |
| `condition` | s32  | Threshold or bitmask (depending on type). For reward events, `savedata >= condition` satisfies. For 9999/10000, AND-ed with savedata. For 200/201, selects which setter to call. |
| `reward`    | s32  | Target value. For song-related events this is the music code (mcode). For other types it's an index or parameter.                            |
| `comptime`  | u64  | Completion state. `0` = not completed; `1` = completed (boolean semantics — satisfies threshold unconditionally). Any other value is a completion timestamp (used by save logic). |
| `savedata`  | u64  | Current progress value. For threshold events, compared to condition. For bitmask types (9999/10000), AND-ed with condition.                  |

---

## Parsed Event Struct (0x28 bytes)

Stored in the master event table `DAT_1805D5E60`, per-player stride `0x101BC8`, max `0x400` entries. The XML field names and their offsets (from the mergeddata receiver) are the canonical source:

| Offset | Size | Field     | C type  |
|--------|------|-----------|---------|
| +0x00  | 4    | eventid   | s32     |
| +0x04  | 4    | eventtype | s32     |
| +0x08  | 4    | eventno   | s32     |
| +0x0C  | 4    | condition | s32     |
| +0x10  | 4    | reward    | s32     |
| +0x14  | 4    | (padding — always read as 0 by consumers) | — |
| +0x18  | 8    | comptime  | u64     |
| +0x20  | 8    | savedata  | u64     |

**Verified independently**:
- `ReflectPlayerWork` reads these offsets matching the named fields.
- `SetRewardEventSaveData` reads `eventid`, `eventtype`, `eventno`, `reward`, `comptime`, `condition` at these offsets.
- `sys_mergeddata_load_receiver` writes into these exact offsets (with `+0x10` base in its temp buffer).

**Max entries**: 0x400 (1024) per player.
**Per-player stride** (everywhere in gamemdx): **0x101BC8** bytes.
**Work-pointer array**: `DAT_1806EBE50` — 2 entries, one per player.
**Per-player parse-source block base**: `DAT_1804E1C78` (gamemdx block this ess→gamemdx data is copied to).

---

## Difficulty Bitmask Encoding

Many event types use 10-bit binary-string bitmasks (e.g. `"0000000001"`) parsed with `stoi(str, idx, 2)`. Each bit maps to a chart slot:

| Bit | Value | Chart slot            |
|-----|-------|-----------------------|
| 0   | 1     | Single Beginner       |
| 1   | 2     | Single Basic          |
| 2   | 4     | Single Difficult      |
| 3   | 8     | Single Expert         |
| 4   | 16    | Single Challenge      |
| 5   | 32    | (unused — "Double Beginner" slot; DDR has no Double Beginner chart) |
| 6   | 64    | Double Basic          |
| 7   | 128   | Double Difficult      |
| 8   | 256   | Double Expert         |
| 9   | 512   | Double Challenge      |

Bit 5 is structurally present but never corresponds to a real chart; compound "tier" bitmasks still set it so that the single-side bit N and the matching double-side bit N+5 light up symmetrically. Since no Double Beginner chart ever exists, the bit is harmless.

### Compound bitmasks used by the game

| String       | Decimal | Bits set          | Meaning                                   |
|--------------|---------|-------------------|-------------------------------------------|
| `0000000001` | 1       | 0                 | Single Beginner only                      |
| `0000000010` | 2       | 1                 | Single Basic only                         |
| `0000000100` | 4       | 2                 | Single Difficult only                     |
| `0000001000` | 8       | 3                 | Single Expert only                        |
| `0000010000` | 16      | 4                 | Single Challenge only                     |
| `0001000000` | 64      | 6                 | Double Basic only                         |
| `0010000000` | 128     | 7                 | Double Difficult only                     |
| `0100000000` | 256     | 8                 | Double Expert only                        |
| `1000000000` | 512     | 9                 | Double Challenge only                     |
| `0000100001` | 33      | 0, 5              | Beginner tier (single + unused double slot) |
| `0001000010` | 66      | 1, 6              | Basic tier (single + double)              |
| `0010000100` | 132     | 2, 7              | Difficult tier (single + double)          |
| `0100001000` | 264     | 3, 8              | Expert tier (single + double)             |
| `1000010000` | 528     | 4, 9              | Challenge tier (single + double)          |
| `0001100011` | 99      | 0, 1, 5, 6        | Beginner + Basic                          |
| `0011100111` | 231     | 0, 1, 2, 5, 6, 7  | Up through Difficult                      |
| `0111101111` | 495     | 0, 1, 2, 3, 5, 6, 7, 8 | Up through Expert                    |
| `0xFFFFFFFF` | ~0      | all               | Unlock everything (type 10)               |

---

## Per-Player Work Fields Touched By Events

Offsets are relative to `*DAT_1806EBE50[player]` (the work pointer). These are fields that the event dispatcher or its helpers read or write; many other fields exist but are not event-related.

| Offset     | Type                    | Written by                                            | Purpose |
|------------|-------------------------|-------------------------------------------------------|---------|
| `+0x80`    | `std::vector<int>`      | Types 9999, 10000                                      | Active eventids. Queried by `IsEventIdActive` and inline `std::find`. |
| `+0x88`    | (vec end)               | —                                                      | `std::vector::end` for above. |
| `+0xA0`    | `std::map<int, u64>`    | Type 10000                                            | Keyed by reward (mcode). Stores per-event savedata for timed events. |
| `+0x178`   | (per-chart state map)   | Types 1, 5, 0x32, 0x34 (via `FUN_1801B2070`)           | Per-difficulty chart state. Indexed `[mcode][diff]`. |
| `+0x1338`  | `std::map<int, u32>`    | `SetRewardEventBitfield` (`FUN_1801E5F50`)             | Primary unlock bitmask by mcode. |
| `+0x1358`  | `std::map<int, u32>`    | `SetRewardEventBitfield`                               | Secondary unlock bitmask by mcode. |
| `+0x1418`  | `std::map<int, i64[9]>` | Type 0x19 (via `FUN_1801E6440`)                        | Per-mcode, per-eventno condition values. |
| `+0x1438`  | `std::map<int, i64[9]>` | Type 0x19 (via `FUN_1801E6590`)                        | Per-mcode, per-eventno savedata values. |
| `+0x1478`  | `std::map<int, int>`    | Type 0x1A                                              | condition keyed by reward. |
| `+0x1498`  | `std::map<int, int>`    | Type 0x1B                                              | condition keyed by reward. |
| `+0x1538`  | `std::map<int, int>`    | Type 0x0D (`eventno != 0 && comptime == 0`) via `FUN_1801E70D0` | Keyed by eventno, value mcode. |
| `+0x1558`  | `std::map<int, u32>`    | Type 0x1E via `FUN_1801E72C0`                          | Keyed by mcode, value savedata. |
| `+0x1578`  | `std::set<int>`         | Type 200 (`condition == 11`) via `FUN_1801E7370`       | Sorted set of reward values. Also receives `0x4C1` on Platinum popup init. |
| `+0x1598`  | `std::set<int>`         | Type 200 (`condition == 12`) / Type 201 (`condition == 2`) via `FUN_1801E7450` | Sorted set of reward values. Also receives `0x4C2` on Platinum popup init. |
| `+0x1708`  | s32                     | `ReflectPlayerWork` entry                              | Server-provided profile rank value (read by eventid-70 gate). |
| `+0x1750`  | u64                     | `ReflectPlayerWork` entry                              | Starting tick reference for timed events. |
| `+0x1758`  | u64                     | `ReflectPlayerWork` entry                              | `GetTickCount64()` snapshot at profile load. |

---

## Song Flag Bits (`song_data[+0x1AC]`)

Bits OR-ed into `song_data[+0x1AC]` (uint32) by the event dispatcher:

| Bit | Value       | Set by                    | Purpose                                                      |
|-----|-------------|---------------------------|--------------------------------------------------------------|
| 0   | `0x00000001`| (from music DB — not events) | Extra Stage eligibility (queried with eventid 1)           |
| 10  | `0x00000400`| (from music DB — not events) | Encore Extra Stage eligibility (queried with eventid 71)   |
| 11  | `0x00000800`| Types 0x12, 0x2C          | Event-song marker                                            |
| 15  | `0x00008000`| Type 0x32 (when `condition != 0`) / 0x34 (when `condition != 0`) | Pre-playable marker (visible, unlockable)  |
| 16  | `0x00010000`| Type 0x06                 | Generic flag                                                 |
| 17  | `0x00020000`| Type 0x32 (when `savedata != 0`) | Pre-playable locked marker                            |
| 25  | `0x02000000`| Type 0x19                 | Event-type-25 marker (queried with eventid 96)               |
| 26  | `0x04000000`| Type 0x5A                 | Event-type-90 marker                                         |

Bits 0 and 10 are queried by event gates but set from the music database (`musicdata_load`) — they are not written by the event dispatcher.

---

## Global Session Struct (`DAT_1806EA478`)

`DAT_1806EA478` is a pointer to an 8-byte outer struct whose `+0x00` dereferences to a 0x190-byte inner struct. Allocated and initialized once by `FUN_1801DACA0`; subsequent defaults set by `FUN_1801DB000`.

Key fields of the inner struct (the one referenced as `*DAT_1806EA478`):

| Offset   | Type    | Purpose                                                             |
|----------|---------|---------------------------------------------------------------------|
| `+0x00`  | s32     | Session type / cabinet classification (2 = default from init)       |
| `+0x08`  | s32     | Player count / current player index                                 |
| `+0x0C`  | s32     | Stage number (read for `local_29c = +0xC + 1` in ReflectPlayerWork) |
| `+0x10`  | s32     | `0xFFFFFFFF` on init — acts as "invalid" sentinel                   |
| `+0x14`  | s32     | 0                                                                   |
| `+0x18`  | s32     | `0xFFFFFFFF` on init — mcode-like field (38707 / `0x9733` is treated as excluded in one Extra Stage gate) |
| `+0x1C`  | s32     | **Result scene selector.** `== 9` → `scene_result_brave` (GALAXY BRAVE result screen). `== 10` → `scene_result_danrank` (DAN RANK result screen). Used in `FUN_1800B9590`, `FUN_180033340`, `FUN_180035A90`, etc. Server-driven via event handlers. |
| `+0x20`  | s32     | 2 on init — probably region                                         |
| `+0x28`  | ptr     | Sub-object with vtable (SessionInfo or similar)                     |
| `+0x30..+0x54` | hardcoded u32s | Initialized to `{0x702, 0x1CD, 0x63F9, 0x250C, 0x92, 0x702, 0x1A1, 0x508B, 0x241E, 0x91}` — likely fixed service IDs / hash seeds |
| `+0x59`  | u8      | "Encore-earned" flag (preserved by event 55 gate in `FUN_1801DB600`) |
| `+0x60..+0x78` | std::string | Inline short-string buffer (cap 0xF). Used for refid / session id lookup. |
| `+0x70`  | ptr     | Heap backing when string capacity > 0xF, otherwise 0                |
| `+0xB0..+0xB8` | std::vector | Cleared on init                                             |
| `+0xD0`  | s32     | Cabinet mode state (1 or 2 for subscribed cabs)                    |
| `+0x138` | s32     | 0 on init                                                           |
| `+0x13C` | u8      | 0 on init                                                           |
| `+0x13D` | u8      | 0 on init                                                           |
| `+0x13E` | u8      | **Flag written by type-9999 handler for eventids 97 (→1) / 98 (→0).** Reader not yet located despite checking ~20 of the 200+ readers of `DAT_1806EA478`. Session-global flag (written by both playerdata and mergeddata dispatchers). |
| `+0x13F` | u8      | **Flag written by type-9999 handler for eventids 112 (→1) / 113 (→0).** **Read by `FUN_180022AE0`** (per-frame I/O polling called from top-level per-frame update at `FUN_180003060 → FUN_180022D30 → FUN_180022AE0`). When set, forces bit `0x8000000` (bit 27) into a per-player state dword at `DAT_1806EBC70 + 0x934 + player*0x498`. The surrounding logic polls coin/service/test switch state from `arkmdxbio2` I/O function pointers — so `+0x13F` appears to be a **"force credit-available / coin-override" flag** used during special events. |
| `+0x140..` | sub-struct | Score/result/brave/grade sub-object (large)                      |
| `+0x180 + player*4` | s32 | Per-player stage counter                                    |

The semantic identity of `+0x13E` is still TBD (reader not yet located). `+0x13F` has been resolved: it's checked during per-frame coin/service-switch polling to force "credit available" state. See [Open Questions](#open-questions).

---

## Full Event Type Dispatch Table (`ReflectPlayerWork`)

Entry point: `gamemdx.dll +0x13C80`. After initializing the work struct from the per-player block, the function iterates up to 0x400 entries from `DAT_1805D5E60`, terminating early when `eventid < 1`. Each row is dispatched via a single `switch(eventtype)`.

**Shorthand**: "threshold check" means the row is currently considered satisfied, evaluated as:
```
comptime != 0
OR (condition > 0 AND savedata != 0xFFFFFFFFFFFFFFFF AND savedata >= condition)
```
Types that apply a bitmask call `SetRewardEventBitfield(work, mcode, mask, mask)` (`FUN_1801E5F50`), which OR-s `mask` into both `work+0x1338` and `work+0x1358` per-mcode std::maps.

### Gameplay / Chart-State Types

| Type (dec) | Type (hex) | Effect |
|------------|------------|--------|
| 1          | 0x01       | `FUN_1801B2070(mcode, 0..3, savedata)` — sets state `savedata` on difficulty slots 0..3 (Single Beg, Bas, Dif, Exp, which internally also propagate to 5..8). |
| 2          | 0x02       | If `FUN_1801B22D0(mcode) != NULL` (song exists): `song_data[+0x141] = (char)savedata`. Single-byte song display attribute. |
| 4          | 0x04       | If song exists: `song_data[+0x17E] = 1`. Single-byte flag — purpose unclear (possibly "has event data"). |
| 5          | 0x05       | `FUN_1801B2070(mcode, 4, savedata)` — sets state on Single Challenge (internally also propagates to Double Challenge). |
| 6          | 0x06       | If song exists: `song_data[+0x1AC] |= 0x10000`. Sets song flag bit 16. |

### Conditional Reward Types (threshold → bitmask)

All require the threshold check to pass. When satisfied, they apply the corresponding bitmask via `SetRewardEventBitfield`, then call `SetRewardEventSaveData` to record completion.

| Type (dec) | Type (hex) | Bitmask              | Effect |
|------------|------------|----------------------|--------|
| 10         | 0x0A       | `0xFFFFFFFF`         | Unlock all difficulties |
| 11         | 0x0B       | `0111101111` (495)   | Unlock up through Expert tier |
| 15         | 0x0F       | `0001100011` (99)    | Unlock Beginner + Basic |
| 16         | 0x10       | `0011100111` (231)   | Unlock up through Difficult |
| 17         | 0x11       | `0111101111` (495)   | Unlock up through Expert (duplicate of 11 — context differs at emission site, not at handler) |
| 18         | 0x12       | `1000010000` (528)   | Unlock Challenge tier AND sets song flag bit 11 (`0x800`) unconditionally |
| 25         | 0x19       | Per-`eventno` tier   | Sets song flag bit 25 (`0x2000000`). Unconditionally calls `FUN_1801E6440(work, mcode, eventno, condition)` and `FUN_1801E6590(work, mcode, eventno, savedata)`. If threshold met, additionally applies the tier bitmask (eventno=0→`0000100001`, 1→`0001000010`, 2→`0010000100`, 3→`0100001000`, 4→`1000010000`) via `SetRewardEventBitfield`. |
| 90         | 0x5A       | Per-`eventno` tier   | Same as 0x19 but sets bit 26 (`0x4000000`) instead. Does NOT call the 0x19 helpers. |

### Per-Difficulty Chart Unlock (type 0x0D — complex)

Branches on `eventno == 0` and `comptime`:

| `eventno == 0`? | `comptime` | Effect |
|-----------------|------------|--------|
| Yes             | 0          | `FUN_1801E6B80(work, mcode, savedata)`, `FUN_1801E6A00(work, mcode, condition)`, `FUN_1801E6F50(work, mcode, eventid)`. (Three per-player setters — details TBD.) |
| Yes             | 1          | Calls `SetRewardEventSaveData` (records completion). |
| Yes             | other      | Falls through. |
| No              | 0          | `FUN_1801E70D0(work, eventno, mcode)` — inserts `(eventno, mcode)` into std::map at work+0x1538. |
| No              | != 0       | For each difficulty `d` in 0..4: calls `song_vtable[0xD8](work, some_flag, d)`. If return == 2, applies per-tier bitmask via `SetRewardEventBitfield` (d=0→`0000100001`, 1→`0001000010`, 2→`0010000100`, 3→`0100001000`, 4→`1000010000`). |

### Bit-Shift Reward

| Type (dec) | Type (hex) | Effect |
|------------|------------|--------|
| 23         | 0x17       | If threshold check passes: `shift = (eventno < 5) ? (eventno + 1) % 5 : eventno`; `mask = 1 << shift`. Calls `FUN_1801E5F50(work, mcode, 0, mask)` (fills only the secondary map at work+0x1358, not primary at +0x1338). |

### Player-Work Map Writes

| Type (dec) | Type (hex) | Effect |
|------------|------------|--------|
| 26         | 0x1A       | `work+0x1478` map: `map[reward] = condition`. |
| 27         | 0x1B       | `work+0x1498` map: `map[reward] = condition`. |

### Direct Save / Reward

| Type (dec) | Type (hex) | Effect |
|------------|------------|--------|
| 12         | 0x0C       | If `comptime == 0`: calls `SetRewardEventSaveData` (records completion state). |
| 30         | 0x1E       | Calls `FUN_1801E72C0(work, mcode, savedata)` — `work+0x1558` map: `map[mcode] = savedata`. Then if `comptime == 1`, calls `SetRewardEventSaveData`. |
| 32         | 0x20       | Always calls `SetRewardEventSaveData` (unconditional save). |

### Pre-Playable (song pre-release)

**Note**: existing v1 doc called these gates "savedata / comptime"; actual code tests `savedata / condition` (v1 was wrong). See [Corrections](#corrections-from-v1).

| Type (dec) | Type (hex) | Effect |
|------------|------------|--------|
| 50         | 0x32       | **PRE_PLAYABLE**. If `savedata == 0 && condition == 0`: sets state 0 on all 4 base single difficulties. If `savedata == 0 && condition != 0`: sets state 2 on all 4, applies bitmask `0111101111` (up through Expert), sets song flag bit 15 (`0x8000`). If `savedata != 0`: sets song flag bit 17 (`0x20000`). Emits log `"P%d PRE_PLAYABLE : mcode=%d, savedata=%d, condition=%d"`. |
| 52         | 0x34       | **PRE_PLAYABLE_CHA**. If `savedata == 0 && condition == 0`: sets state 0 on Challenge. If `savedata == 0 && condition != 0`: sets state 2 on Challenge, applies bitmask `1000010000`, sets song flag bit 15. If `savedata != 0`: no-op. Emits log `"P%d PRE_PLAYABLE_CHA : mcode=%d, savedata=%d, condition=%d"`. |

### Individual Difficulty Unlocks (0x46–0x54)

Formula: `eventtype = 0x46 + chart_slot_index` where chart_slot_index skips over the unused Double Beginner gap.

| eventtype (dec) | eventtype (hex) | Bitmask string | Chart slot unlocked |
|-----------------|-----------------|----------------|---------------------|
| 70              | 0x46            | `0000000001`   | Single Beginner     |
| 71              | 0x47            | `0000000010`   | Single Basic        |
| 72              | 0x48            | `0000000100`   | Single Difficult    |
| 73              | 0x49            | `0000001000`   | Single Expert       |
| 74              | 0x4A            | `0000010000`   | Single Challenge    |
| 81              | 0x51            | `0001000000`   | Double Basic        |
| 82              | 0x52            | `0010000000`   | Double Difficult    |
| 83              | 0x53            | `0100000000`   | Double Expert       |
| 84              | 0x54            | `1000000000`   | Double Challenge    |

**The gap**: eventtypes `0x4B..0x50` (75..80) do NOT exist in the switch. Bit 5 is the unused Double Beginner slot, and the event types that would correspond to it are simply not allocated.

All of these require the standard threshold check. When satisfied, they call `SetRewardEventBitfield(work, mcode, mask, mask)`.

### Gold Cabinet Only (types 0x28–0x2C)

All gated by `FUN_180012E50() ∈ {6, 7, 8}` (gold cabinet variants). If the gate fails, the handler returns without effect. Otherwise behaves like the non-gold parallel:

| Type (dec) | Type (hex) | Equivalent |
|------------|------------|-----------|
| 40         | 0x28       | Like type 10 (all-diff unlock via `0xFFFFFFFF`) |
| 41         | 0x29       | Like type 15 (bitmask `0001100011`) |
| 42         | 0x2A       | Like type 16 (bitmask `0011100111`) |
| 43         | 0x2B       | Like type 17 (bitmask `0111101111`) |
| 44         | 0x2C       | Like type 18 — bit 11 on song + bitmask `1000010000` |

### League (types 100, 101)

| Type (dec) | Type (hex) | Effect |
|------------|------------|--------|
| 100        | 0x64       | Calls `FUN_1801DB510(mcode)` — registers `mcode` as a league song. |
| 101        | 0x65       | Calls region-detection function pointer at `DAT_1806EBA10(&local)`. If result is 0 or 1 (or clamped from negative): falls through to case 100's handler. If result is 2 or out-of-range: skipped. Region-aware league registration. |

### Conditional Unlock (types 200, 201)

Both gated by `reward != 0 && savedata != 0 && condition > 0 && comptime == 0`, AND a byte match between `DAT_1804E1C70[player_stride]` and `comptime` (OR `savedata == 2`). When active, `(condition - base)` selects the setter:

| Value of `condition - base` | Effect |
|-----------------------------|--------|
| 0                           | `FUN_1801E7370(work, mcode)` — inserts mcode into std::set at `work+0x1578`. |
| 1                           | `FUN_1801E7450(work, mcode)` — inserts mcode into std::set at `work+0x1598`. |

| Type (dec) | Type (hex) | Condition base | Extra gate |
|------------|------------|----------------|-----------|
| 200        | 0xC8       | `0xB` (11)     | None — so `condition == 11` hits set 1, `condition == 12` hits set 2. |
| 201        | 0xC9       | `0x1` (1)      | Also requires `FUN_18001F160() != 2` (region must be 0 or 1). So `condition == 1` hits set 1, `condition == 2` hits set 2. |

### Network Event (type 1000)

| Type (dec) | Effect |
|------------|--------|
| 1000       | If `DAT_1806EBCF0 != 0` (network-connected state): calls `FUN_1801ACCB0(mcode, savedata)` — inserts/updates a std::map at `DAT_1806EBCF0 + 0x20` (some network-state sub-object) keyed by mcode with value `savedata`. Does NOT send a network packet — it's an in-memory registration used elsewhere by the networking subsystem. Otherwise the event is skipped. |

### Global Flag (type 9999)

Always pushes `eventid` onto the per-player active-event vector (`work+0x80`) via `FUN_18002C720` (std::vector push_back). Additionally, for specific eventids, performs hardcoded side effects on the global session struct:

| eventid (dec) | eventid (hex) | Side effect |
|---------------|---------------|-------------|
| 97            | 0x61          | `*DAT_1806EA478[+0x13E] = 1` |
| 98            | 0x62          | `*DAT_1806EA478[+0x13E] = 0` |
| 108           | 0x6C          | No-op in this build (case falls through without side effect). |
| 112           | 0x70          | `*DAT_1806EA478[+0x13F] = 1` |
| 113           | 0x71          | `*DAT_1806EA478[+0x13F] = 0` |

All other eventids that appear with `eventtype == 9999` only join the active-event vector; they have no hardcoded side effect here.

### Timed Event (type 10000)

Always:
1. Pushes `eventid` onto the active-event vector at `work+0x80`.
2. Inserts/updates `work+0xA0[reward] = savedata` (std::map). Keyed by mcode.
3. Writes `GetTickCount64() - work+0x1758` added to `work+0x1750` as the elapsed-time reference.
4. Calls `SetRewardEventSaveData` unconditionally.

The `work+0x1758` field is set at `ReflectPlayerWork` entry to the current tick count. Elapsed time is "time since profile load," not absolute time.

### Event Types NOT in the switch

Types `3`, `7–9`, `13` no-ops not covered above, `14`, `19–22`, `24`, `28–31`, `33`, `35–69` excluding those documented above, `75–80`, `85–89`, `91–99`, `102–199`, `202–999`, `1001–9998`, `10001+` — fall through the switch without effect.

No evidence found that any of these are handled elsewhere (e.g. on a different dispatcher) — they appear to be reserved / unimplemented values.

---

## Hardcoded Event ID Queries

The game queries "is eventid N active?" via a shared helper, and also through a handful of inline `std::find` lookups.

### `IsEventIdActive(eventid)` — `gamemdx.dll +0x1DBD50`

```
IsEventIdActive(eventid):
    for each player in (P1, P2):
        work = *DAT_1806EBE50[player]
        if eventid in {35, 36} and env_flag_cached == 'Z':
            return true                 # dev-env backdoor
        if eventid in {1, 5}:
            *DAT_1806EB1BC = 0x15        # set error code
            continue                    # refuse — fall through to next player
        if std::find(work+0x80 .. work+0x88, eventid) != end:
            return true
    return false
```

The `env_flag_cached` value is a one-time-cached read via function pointer `DAT_1806EB3C0` (likely `GetEnvironmentVariableA`) against `DAT_180CEC458`. The cached result is checked via `DAT_180CEC45E == 'Z'`. This allows dev builds to force-activate eventids 35/36 without a server.

Returns true if **any** player has the eventid active.

### All 13 call sites of `IsEventIdActive`

Extracted by reading the `MOV ECX, imm32` immediately before each call:

| Call site        | eventid    | Caller                           | Feature gated |
|------------------|------------|----------------------------------|---------------|
| `0x1800921D7`    | 70 (0x46)  | `FUN_180091470` (profile widget) | Profile rank badge — reads `work+0x1708` when active. Premium/Platinum membership indicator. |
| `0x1800B8659`    | 1 (0x01)   | `FUN_1800B7D60` (end-of-song)    | **Extra Stage** — sets "extra stage earned" state when active. |
| `0x1800B59C8`    | 2 (0x02)   | `FUN_1800B5930` (tick callback)  | **Extra Stage trigger announcement**. |
| `0x1800B59E7`    | 37 (0x25)  | `FUN_1800B5930`                  | **Extra Stage trigger announcement** (networked-cabinet variant). |
| `0x1800BE194`    | 47 (0x2F)  | `FUN_1800BC4D0` (song-select SM) | **GALAXY PLAY gate** — song-selection state machine bypasses a special mode handler when eventid 47 is active. Confirmed as Galaxy Play via RemyWiki: the GALAXY BRAVE event is exclusively accessible in Galaxy Play mode. |
| `0x1800C5B83`    | 2 (0x02)   | `FUN_1800C5630` (tick callback alt) | **Extra Stage trigger** (also excludes mcode `0x9733` = 38707). |
| `0x1800C5BB3`    | 37 (0x25)  | `FUN_1800C5630`                  | **Extra Stage trigger** (networked variant). |
| `0x1801B0C7E`    | 1 (0x01)   | `CanPlayExtraStage` (`FUN_1801B0C70`) | True iff eventid 1 active + stage = extra stage + song flag bit 0. |
| `0x1801B0CEE`    | 71 (0x47)  | `CanPlayEncoreStage` (`FUN_1801B0CE0`) | True iff eventid 71 active + stage = extra stage+1 + song flag bit 10. |
| `0x1801B1556`    | 96 (0x60)  | `FUN_1801B1490` (per-chart avail) | **EXTRA SAVIOR WORLD** unlock gate. Called from stage voice builder (`FUN_180033340`) to emit `vo_choice_savior`, and from course display (`FUN_180035A90`) to emit `cosh_call_savior`. These strings are the internal code for the Extra Savior mechanic (carried over from DDR A3's EXTRA SAVIOR A3). Requires song flag bit 25 on the chart. Confirmed as the primary DDR WORLD unlock event per RemyWiki. |
| `0x1801B137F`    | 96 (0x60)  | `FUN_1801B1330` (per-chart avail, alt) | Same Extra Savior gate, different caller. |
| `0x1801DB713`    | 55 (0x37)  | `FUN_1801DB600` (stage-transition) | Preserves "encore earned" flag on `*DAT_1806EA478[+0x59]`. |

### Inline `std::find` queries (same pattern but not via the helper)

The game also uses inline `std::find(work+0x80, work+0x88, eventid)` directly at some call sites. v1 of this doc listed 2; there may be more. A full sweep is planned — see [Open Questions](#open-questions).

| Location        | eventid    | Feature gated |
|-----------------|------------|---------------|
| `0x180092FBF`   | 118 (0x76) | **Dan Ranking** UI — shows "coming soon" (`sceawi_profile_danrank_status_comingsoon_text`) when NOT active. |
| `0x180093B18`   | 101 (0x65) | **World League** profile display — reads league class when active. |

### Summary of hardcoded eventids

| eventid | hex  | Gates                                                                    |
|---------|------|--------------------------------------------------------------------------|
| 1       | 0x01 | Extra Stage (availability + end-of-song trigger)                         |
| 2       | 0x02 | Extra Stage trigger announcement                                         |
| 35 / 36 | 0x23 / 0x24 | Dev-env backdoor (auto-active if env var is `'Z'`)                |
| 37      | 0x25 | Extra Stage trigger announcement (networked cabinet only)                |
| 47      | 0x2F | **GALAXY PLAY gate** — controls the Galaxy Play special-mode entry in song select. Confirmed: the GALAXY BRAVE event (Feb 2025+) is "exclusive to Galaxy Play", and session struct `+0x1C == 9` triggers the `scene_result_brave` result screen (GALAXY BRAVE). |
| 55      | 0x37 | Encore Extra Stage transition flag                                       |
| 70      | 0x46 | Profile rank badge display (Premium/Platinum indicator)                  |
| 71      | 0x47 | Encore Extra Stage availability                                          |
| 96      | 0x60 | **EXTRA SAVIOR WORLD** — DDR WORLD's primary song-unlock event (carried forward from A3's EXTRA SAVIOR A3). Gates voice/text lines `vo_choice_savior` and `cosh_call_savior`. Requires song flag bit 25 on the chart. |
| 97      | 0x61 | Sets `*DAT_1806EA478[+0x13E] = 1` (semantic TBD — reader not yet located)|
| 98      | 0x62 | Clears `*DAT_1806EA478[+0x13E]`                                          |
| 101     | 0x65 | World League profile display                                             |
| 108     | 0x6C | Reserved (no-op in this build)                                           |
| 112     | 0x70 | Sets `*DAT_1806EA478[+0x13F] = 1` — **forces "credit available" bit in per-frame coin polling** (see `FUN_180022AE0`) |
| 113     | 0x71 | Clears `*DAT_1806EA478[+0x13F]` — returns to normal coin polling      |
| 118     | 0x76 | Dan Ranking availability ("coming soon" when not active)                 |

All other eventids not in this list are opaque — the game records them in the active-event vector but never checks them by number. They serve as CSV bookkeeping identifiers (unique per event row) or forward-compat hooks.

### Notes on reserved eventids

- **Eventid 1 and 5**: `IsEventIdActive` refuses to look these up; writes error code `0x15` to `0x1806EB1BC`, then continues to the next player. In practice both players get the same event set, so the result is unchanged — only slower by one loop iteration.
- **Eventid 35 and 36**: force-active in dev environments (env var check). Servers should avoid using these or understand that dev builds will always report them active.

### Cross-reference: DDR WORLD events → eventids

Correlations with the [RemyWiki DDR WORLD events list](https://remywiki.com/AC_DDR_WORLD), based on feature semantics identified in the dispatcher and call-site analyses above:

| DDR WORLD event                       | Related eventids / session fields                                |
|---------------------------------------|------------------------------------------------------------------|
| **EXTRA SAVIOR WORLD** (Aug 2024+)    | Eventid **96** (savior unlock gate); individual songs also use event types 1/2/5/18 per row |
| **WORLD LEAGUE** (Oct 2024+)          | Eventid **101** (profile league-class display); event types 100/101 handle per-song league registration |
| **GALAXY BRAVE / GALAXY PLAY** (Feb 2025+) | Eventid **47** (Galaxy Play gate); session struct `+0x1C == 9` → `scene_result_brave` |
| **DAN RANK** (Mar 2026+)              | Eventid **118** (Dan Ranking availability); session struct `+0x1C == 10` → `scene_result_danrank` |
| **PLATINUM MEMBER PASS** (Jun 2025+)  | Eventid **70** (Premium/Platinum profile badge); session struct `+0x34` (cabinet play mode) |
| **EXTRA STAGE** (core mechanic)       | Eventids **1** (availability), **2** and **37** (trigger announcement, networked variant), **55** (encore transition), **71** (Encore Extra Stage availability) |
| **BEMANI PRO LEAGUE** seasons         | Uses per-song event rows with type 18 / 0x19 for unlocks — no dedicated eventid gate found |
| **Music Creator Auditions**, **Classic / White Challenge**, **MYSTICAL Re:UNION**, **BPL Triple Tribe** | All appear to unlock via per-song event rows with standard types (10–18, 0x19, 0x5A, 200/201) — no dedicated game-wide gate |
| **Dev environment backdoor**          | Eventids **35 / 36** auto-activate if env var check returns `'Z'` |

Unmatched eventids (reader not yet located): **97 / 98 / 108**. All three are type 9999 which pushes them onto the active-event vector and 97/98 additionally toggle session struct `+0x13E`. Candidates for 97/98 semantics: coordinated event-season toggle (e.g. "EXTRA SAVIOR Part 1 → Part 2 switchover"), network/feature announcement banner, or maintenance-mode flag. Eventid 108 is a no-op in this build — likely reserved for a future or removed feature.

---

## Helper Functions Reference

| Address (file) | Name (my name)                  | Purpose |
|----------------|----------------------------------|---------|
| `+0x13C80`     | `ReflectPlayerWork`              | Main event dispatch. Iterates master event table and runs the switch. |
| `+0x1C220`     | `SetRewardEventSaveData`         | Writes event progress to the per-player save table; marks completion. |
| `+0x1DBD50`    | `IsEventIdActive`                | Shared query helper: is eventid N active for any player? |
| `+0x1E5F50`    | `SetRewardEventBitfield`         | OR-s a bitmask into two per-mcode std::map structures (primary + secondary unlock bitfields). |
| `+0x1B2070`    | `SetChartState`                  | Writes state slot on a song (`song_data[+0x22C + diff*4]`). Called by types 1, 5, 0x32, 0x34. |
| `+0x1B22D0`    | `GetSongDataByMcode`             | Song-data lookup by mcode. Returns pointer or NULL. |
| `+0x12E50`     | `GetHardwareConfig`              | Returns cabinet classification (6/7/8 = gold variants). Gates types 0x28–0x2C. |
| `+0x1F160`     | (`GetRegion` wrapper)            | Clamps `DAT_1806EBA10()` result to `[0, 2]`. Used by type 0xC9 gate. |
| `+0x1E6440`    | (type-0x19 condition setter)     | Inserts `condition` at index `eventno` in std::map<mcode, i64[9]> at work+0x1418. |
| `+0x1E6590`    | (type-0x19 savedata setter)      | Inserts `savedata` at index `eventno` in std::map<mcode, i64[9]> at work+0x1438. |
| `+0x1E6B80`    | (type-0x0D setter A)             | One of three setters for type 0x0D `eventno == 0 && comptime == 0`. Details TBD. |
| `+0x1E6A00`    | (type-0x0D setter B)             | Ditto. Details TBD. |
| `+0x1E6F50`    | (type-0x0D setter C)             | Ditto. Details TBD. |
| `+0x1E70D0`    | (type-0x0D alt setter)           | std::map<eventno, mcode> at work+0x1538. Used when `eventno != 0 && comptime == 0`. |
| `+0x1E72C0`    | (type-0x1E reward setter)        | std::map<mcode, savedata> at work+0x1558. |
| `+0x1E7370`    | (set-1 inserter)                 | std::set<int> at work+0x1578. Used by type 200 `condition == 11` and Platinum popup init (`0x4C1`). |
| `+0x1E7450`    | (set-2 inserter)                 | std::set<int> at work+0x1598. Used by type 200 `condition == 12`, type 201 `condition == 2`, Platinum popup init (`0x4C2`). |
| `+0x1DBAA0`    | (cabinet / session-state check)  | **NOT** an event-ID lookup (v1 doc was wrong). Reads `*DAT_1806EA478[+0x70]` (session pointer) and checks cabinet mode `+0xD0`. Returns boolean. |
| `+0x1DBCE0`    | (max-stage clamp)                | **NOT** an event-ID lookup (v1 doc was wrong). Iterates players reading `work+0x44` (stage progress) and clamps to `[0, 2]`. |
| `+0x02C720`    | `std::vector::push_back<int>`     | Used for active-event vector at work+0x80. |
| `+0x1ACCB0`    | (type-1000 handler)              | Called when networked. Takes `(mcode, savedata)`. Inserts into the std::map at `DAT_1806EBCF0 + 0x20` — this is an in-memory registration used by the networking subsystem (not a packet send). |

---

## `SetRewardEventSaveData` (save-table writer) — `+0x1C220`

Writes event progress to the per-player save table at `DAT_1804C8AC0` (stride `0xA00`, max `0x40` entries per player). Inferred signature:

```c
bool SetRewardEventSaveData(int player, int reward_or_mcode,
                            int eventtype_key, int eventno_key,
                            uint64_t new_savedata)
```

Algorithm:

1. Scan master event table at `DAT_1805D5E60[player]` for a row matching `(eventtype_key, eventno_key, reward_or_mcode)`.
2. If found, capture its `eventid` and current `comptime`. Compute "newly-complete" flag:
   - **Normal types**: `new_savedata >= condition`
   - **Types 9999 / 10000**: `(condition & new_savedata) == condition` (bitmask AND)
   - Already complete (`comptime == 1`): stays complete
3. Scan save table for a row matching `(eventid, eventtype_key, eventno_key)`:
   - Found: update `savedata`, set `comptime = 1` if newly complete. Log `"update save eventdata"`.
   - Not found: create new entry. Log `"add save eventdata"`.
4. Returns true on success, false if the save table is full.

Save-table entries are serialized back to the server in `playerdata_save`.

---

## Backend Implementation Guide

How to emit events for DDR World from a custom server. The two endpoints use different encodings but map to the same struct.

### CSV format (playerdata_load)

```
eventid,eventtype,eventno,condition,reward,comptime,savedata
```

Separators are commas, no quoting, no whitespace. Field types:

- `eventid`: s32 (1..2^31-1, must be unique per event row)
- `eventtype`: s32 (must match one of the documented dispatch codes)
- `eventno`: s32 (semantics vary by type — often 0 for type 9999; chart tier 0..4 for per-difficulty types)
- `condition`: s32 (threshold, bitmask, or selector depending on type)
- `reward`: s32 (mcode for song types, index otherwise)
- `comptime`: u64 (0 = not complete, 1 = complete, or unix timestamp for timed events)
- `savedata`: u64 (current progress; interpreted per-type)

### XML format (mergeddata_load)

```xml
<event>
  <eventid   __type="s32">118</eventid>
  <eventtype __type="s32">9999</eventtype>
  <eventno   __type="s32">0</eventno>
  <condition __type="s32">0</condition>
  <reward    __type="s32">0</reward>
  <comptime  __type="u64">0</comptime>
  <savedata  __type="u64">0</savedata>
</event>
```

Both endpoints should send a consistent event list. `mergeddata_load` fires between songs to refresh event flags mid-session.

### Enabling standard features

To enable common gameplay features on profile load, emit these type-9999 entries. The `eventid` values are canonical (the game hardcodes queries for them):

```
"1,9999,0,0,0,0,0"     # Extra Stage
"47,9999,0,0,0,0,0"    # GALAXY PLAY gate (confirmed: needed for GALAXY BRAVE event access)
"55,9999,0,0,0,0,0"    # Encore Extra Stage transition
"70,9999,0,0,0,0,0"    # Premium/Platinum profile badge
"71,9999,0,0,0,0,0"    # Encore Extra Stage availability
"96,9999,0,0,0,0,0"    # Event-type-25 song chart gate
"101,9999,0,0,0,0,0"   # World League UI
"118,9999,0,0,0,0,0"   # Dan Ranking UI
```

Additionally these type-9999 eventids toggle specific session-state flag bytes (semantics TBD — use cautiously):

```
"97,9999,0,0,0,0,0"    # Sets *DAT_1806EA478[+0x13E] = 1
"98,9999,0,0,0,0,0"    # Clears *DAT_1806EA478[+0x13E]
"112,9999,0,0,0,0,0"   # Sets *DAT_1806EA478[+0x13F] = 1
"113,9999,0,0,0,0,0"   # Clears *DAT_1806EA478[+0x13F]
```

**Avoid**:
- Eventid **1** and **5** for anything other than their documented purpose (query helper writes an error code when asked about them).
- Eventid **2** (tick-callback code uses it as a secondary Extra Stage requirement).
- Eventids **35** and **36** (dev builds force-activate them; unreliable signaling).

### Unlock specific song charts

Song unlocks use single-chart types (0x46–0x54) or compound-tier types (0x0A–0x12). Each row needs a unique `eventid` — any value > 100 not in the reserved list works (`20000+` is conventional).

Example — unlock all charts of mcode `38667`:

```
"20000,70,0,1,38667,1,1"   # Single Beginner
"20001,71,0,1,38667,1,1"   # Single Basic
"20002,72,0,1,38667,1,1"   # Single Difficult
"20003,73,0,1,38667,1,1"   # Single Expert
"20004,74,0,1,38667,1,1"   # Single Challenge
"20005,81,0,1,38667,1,1"   # Double Basic
"20006,82,0,1,38667,1,1"   # Double Difficult
"20007,83,0,1,38667,1,1"   # Double Expert
"20008,84,0,1,38667,1,1"   # Double Challenge
```

Field meaning for these entries: `condition=1` (any non-zero satisfies the threshold); `reward=mcode`; `comptime=1` (already completed); `savedata=1` (satisfies `savedata >= condition`).

Compound alternative — unlock all difficulties in one row with type 0x0A:

```
"20100,10,0,1,38667,1,1"   # type 10 = 0xFFFFFFFF bitmask (all chart slots)
```

Or bitmask-tier shortcut with type 0x11:

```
"20101,17,0,1,38667,1,1"   # Unlock up through Expert tier
```

### Unlock with progress tracking

For events requiring actual play progress (threshold-based):

```
"20200,11,0,100,38667,0,0"   # Unlock up-through-Expert after savedata reaches 100
```

Backend tracks per-player progress and updates `savedata` via `playerdata_save` until it reaches `condition`. When `savedata >= condition`, the game applies the unlock bitmask AND calls `SetRewardEventSaveData`, which marks the save entry `comptime=1` and serializes it back.

### Pre-release song availability

Type 0x32 (PRE_PLAYABLE) controls "preview / locked" songs:

```
"20300,50,0,0,38667,0,0"    # Song exists but all 4 base diffs in state 0 (locked)
"20301,50,0,1,38667,0,0"    # Song in state 2 (unlockable via play), sets bit 15
                            # (shows as "pre-playable" in UI, applies Expert-tier bitmask)
"20302,50,0,0,38667,0,1"    # savedata != 0 → just sets bit 17 (pre-playable-locked marker)
```

Type 0x34 is the same but for Challenge charts only.

**Correction from v1**: these gates test `condition`, not `comptime` as the v1 doc stated. See [Corrections](#corrections-from-v1).

### Timed events

Type 10000 events store server-provided timestamp data keyed by mcode:

```
"5000,10000,0,0x3F,1234,1,0xDEADBEEF"
```

`condition=0x3F` is interpreted as a bitmask for the completion check; `savedata` is the server-provided value. The game records elapsed time since profile load via `GetTickCount64()` snapshotted in `work+0x1758`.

---

## Verification Notes

The following claims are verified via static analysis on both modules:

- ✅ Struct layout (0x28 bytes) — **cross-verified** between three independent sources:
  - `ReflectPlayerWork` field reads in gamemdx
  - `SetRewardEventSaveData` field reads in gamemdx
  - `sys_mergeddata_load_receiver` field writes in ess.dll (with XML element names)
- ✅ Field names (`eventid/eventtype/eventno/condition/reward/comptime/savedata`) — from Konami's own XML schema in ess.dll + log format strings in gamemdx.
- ✅ Per-player stride `0x101BC8` — from `lVar31 * 0x101bc8` in ReflectPlayerWork entry.
- ✅ Master table at `DAT_1805D5E60`, save table at `DAT_1804C8AC0`, work pointer array at `DAT_1806EBE50`.
- ✅ All switch cases 1..1000..9999..10000 documented — direct decompilation of `ReflectPlayerWork`.
- ✅ Bitmask encoding (10-bit, bits 0–4 single / 5–9 double, bit 5 unused) — from the `stoi` string arguments for each case.
- ✅ Type 0x46–0x54 formula (`type = 0x46 + chart_slot`, with 0x4B–0x50 skipped due to Double Beginner gap) — direct case-by-case confirmation.
- ✅ All 13 `IsEventIdActive` call sites enumerated; eventid literals extracted from `MOV ECX, imm` just before each call.
- ✅ Inline `std::find` queries at `0x180092FBF` (eventid 118) and `0x180093B18` (eventid 101) — confirmed by disassembly.
- ✅ Special eventids in type-9999 handler (97, 98, 108, 112, 113) — direct switch decompilation inside ReflectPlayerWork.
- ✅ ess.dll data flow: CSV write for playerdata_load (string-based), direct struct write for mergeddata_load (typed-XML-based).
- ✅ ess→gamemdx memcpy sizes: 0x54BFC0 (playerdata), 0xA008 (mergeddata).

---

## Open Questions

Items still on the investigation list (active work):

1. **Complete inline-std::find enumeration**. v1 listed 2 inline queries; a full sweep of gamemdx for the pattern `cmp [reg], imm32; je ...; add reg, 4; cmp reg, [work+0x88]; jne` will likely reveal more hardcoded eventids the current doc doesn't cover.
2. **Semantics of `+0x13E`**. Known: eventids 97/98 toggle this byte via type 9999. `+0x13F` (eventids 112/113) has been **resolved** — it's checked every frame by the coin/service-switch polling function `FUN_180022AE0` to force bit `0x8000000` into the per-player state register at `DAT_1806EBC70+0x934`, effectively a "force credit-available" override. `+0x13E` reader not yet located; ~20 of the 200+ `DAT_1806EA478` readers have been checked without a match. Since it's written by both playerdata and mergeddata dispatchers it's session-global, likely another I/O or scene-gating flag. Not blocking for backend — eventids 97/98 can still be emitted, they just don't have a named feature yet.
3. **Type 1000 handler `FUN_1801ACCB0`** — **resolved**. Inserts `(mcode, savedata)` into a std::map at `DAT_1806EBCF0 + 0x20` (networking subsystem sub-object). It's not a packet send; it's an in-memory registration that the network code uses later. Network-gated so a non-networked cabinet ignores the event.
4. **Type 0x0D `eventno == 0 && comptime == 0` setters** (`FUN_1801E6B80`, `FUN_1801E6A00`, `FUN_1801E6F50`). Three consecutive `FUN_`s called in this case. Each is a per-player map/set inserter. Need individual analysis to label.
5. **Eventid 47 = GALAXY PLAY gate** — **resolved** via cross-reference to RemyWiki. The GALAXY BRAVE event is explicitly Galaxy-Play-exclusive, and session struct `+0x1C == 9` drives the brave result scene. Eventid 47 is the "is Galaxy Play selected/available" switch.
6. **Mergeddata dispatcher deeper analysis**. `FUN_180016DC0` uses `FUN_1801DB750` (session-wide bitfield setter) instead of `FUN_1801E5F50`. Need to compare the two and document whether they touch per-mcode maps, per-player maps, or global state. Also need to understand why some event types (100, 101, 200, 201, 1000) are intentionally omitted.

---

## Corrections from v1

The following claims from the previous document iteration have been verified against the current code and are **incorrect**. Fixed versions are in the main doc body above.

1. **`FUN_1801DBAA0` and `FUN_1801DBCE0` described as event-ID query helpers**
   - v1: "FUN_1801DBAA0 is likely 'is the per-song-flag version active'" and "FUN_1801DBCE0 is likely 'get extra-stage bonus score'"
   - Verified: Neither touches `work+0x80`. `FUN_1801DBAA0` is a cabinet/session-state check that reads `*DAT_1806EA478[+0x70]` and `+0xD0`. `FUN_1801DBCE0` is a per-player max-stage clamp that reads `work+0x44`.

2. **Pre-playable gate field for types 0x32 and 0x34**
   - v1: inner branches gated on `comptime == 0` vs `comptime != 0` (with savedata condition)
   - Verified: the local `iVar10` tested in the inner branches is `condition` (set from `local_3168._4_4_ = (int)(uVar29 >> 0x20)` before the switch, and not reassigned inside cases 0x32 / 0x34). The actual gating is on `condition == 0` vs `condition != 0`. The `savedata != 0 → bit 17` branch was correct.

3. **Type 0x19 vs Type 0x5A relationship**
   - v1 implied both called the same 0x19 per-mcode helpers.
   - Verified: Only type 0x19 calls `FUN_1801E6440` / `FUN_1801E6590`. Type 0x5A sets a different song-flag bit and applies the tier bitmask but does NOT call those helpers. The two events serve different purposes (different flags, different map population).

4. **Condition-base for type 200 / 201**
   - v1: implied any value of `condition` could hit either branch.
   - Verified: the handlers only fire for `(condition - base) ∈ {0, 1}`. For type 200 (base=11) that means `condition ∈ {11, 12}` only. For type 201 (base=1) that means `condition ∈ {1, 2}` only. Other values are ignored.

5. **Struct field names attributed to "Konami's own log strings"**
   - v1: cited log format strings in gamemdx as the source of the field names.
   - Verified: the XML schema in `ess.dll::sys_mergeddata_load_receiver` has the *authoritative* mapping of field names to offsets (XML element names are `eventid`, `eventtype`, `eventno`, `condition`, `reward`, `comptime`, `savedata`, written at offsets `+0x10, +0x14, +0x18, +0x1C, +0x20, +0x28, +0x30` in a struct that's laid out identically to `DAT_1805D5E60` entries).

6. **Per-player stride `0x101AB8` (listed as an "old build" alternate)**
   - v1 retained the mention as possibly being a prior build.
   - Verified: in build 20260324 the stride is exclusively `0x101BC8`. No `0x101AB8` appears anywhere in this build's code; that old-doc claim was likely inherited from an even earlier build and is now fully superseded.

7. **playerdata_load and mergeddata_load events assumed to merge into the same table**
   - v1 implied both paths converge on `DAT_1805D5E60` and both are processed by `ReflectPlayerWork`.
   - Verified: they're **two separate tables with two separate dispatchers**:
     - `DAT_1805D5E60` (per-player) is processed by `ReflectPlayerWork` from the playerdata_load path.
     - `DAT_1804BEAB0` (session-global) is processed by `FUN_180016DC0` from the mergeddata_load path.
   - The mergeddata dispatcher handles a SUBSET of event types (no 100, 101, 200, 201, 1000) and its effects span both players (and update global session flags at `*DAT_1806EA478[+0x138..+0x13D]`). See [Data Flow](#data-flow--server-to-dispatcher) for full details.
