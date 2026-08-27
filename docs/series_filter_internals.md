# Series Filter Internals

Reverse engineering document for DDR World's series value system — how the `<series>` field from musicdb.xml is parsed, stored, validated, and used for filtering in the song select screen.

**Game binary**: `gamemdx.dll` (MDX-003_20260324)
**Ghidra base**: `0x180000000`
**Tools used**: Ghidra (static analysis), Cheat Engine (runtime breakpoints)

---

## Summary

The series value flows through three stages:

```
musicdb.xml <series>N</series>
    ↓ (XML parse → per-song property object, vtable+0xA0 accessor returns raw u8)
Raw series value (u8, 0–255)
    ↓ (series_mapper switch statement — FUN_1800ff7b0)
Mapped series value (0–21, with 15/16 merged; unknown values → 0)
    ↓ (version filter predicate — FUN_1801235e0)
Range comparison against filter entry table (stride 0x88, 9 entries + sentinel)
```

**The bottleneck is the series_mapper function** (`FUN_1800ff7b0`). It contains a switch statement that explicitly enumerates valid series values 1–21. Any value outside this range (including 0 and 22+) is mapped to 0, causing the song to be categorized as "1st–5thMIX". The song still appears in ALL MUSIC but is miscategorized in the VERSION filter.

---

## Key Functions

### `FUN_1800ff7b0` — Series Mapper (THE bounds check)

**Ghidra address**: `0x1800ff7b0`
**Runtime address**: `gamemdx.dll + 0xFF7B0`
**AOB signature**: `5C C3 CC CC CC CC CC CC CC CC CC CC CC CC CC CC 40 57 48 83 EC 40` (offset +16 to reach function start)
**Callers**: 2 — the version filter predicate and a thin wrapper at `0x180191950`

**What it does**:
1. Reads the raw series value via virtual call: `call qword ptr [rax+0xA0]` on the song's property accessor object
2. The return value is a **u8** (`movzx esi, al`)
3. Runs the value through a switch statement implemented as a jump table
4. Returns the mapped value, or 0 for unknown values

**Assembly of the bounds check** (at function+0x5D):
```asm
movzx eax, sil          ; eax = raw series value (u8)
dec   eax                ; eax = raw - 1
cmp   eax, 0x14          ; compare against 20 (max valid: 21 - 1 = 20)
ja    default_case        ; if (raw - 1) > 20, goto default (return 0)
; ... jump table for cases 0–20 ...
default_case:
xor   eax, eax           ; return 0
```

**Switch mapping** (raw value → returned value):

| Raw | Returned | Notes |
|-----|----------|-------|
| 0 | 0 | Falls through to default (raw-1 underflows to 0xFFFFFFFF > 0x14) |
| 1–14 | 1–14 | Identity mapping |
| 15 | 15 | Merged with 16 |
| 16 | 15 | Merged with 15 (DDR 2013 + DDR 2014 share a filter entry) |
| 17–21 | 17–21 | Identity mapping |
| 22+ | 0 | **Default case — must be patched for custom series** |

### `FUN_1801235e0` — Version Filter Predicate

**Ghidra address**: `0x1801235e0`
**Runtime address**: `gamemdx.dll + 0x1235E0`

**What it does**:
1. Calls `FUN_1800ff7b0` to get the mapped series value for a song
2. Iterates a linked list of selected filter entries
3. For each entry, checks: `entry[i].series_start <= mapped_series < entry[i+1].series_start`
4. Returns true if the song matches any selected entry, false otherwise

**Key detail**: The comparison uses the **mapped** series value (output of series_mapper), not the raw value. So even if the mapper is patched to pass through value 30, the filter predicate will only match if there is a filter entry whose range includes 30. Without a filter entry for series 30, the song appears in ALL MUSIC but cannot be filtered by version.

**Critical finding (from disassembly)**: The predicate loads the table base address via a single `LEA R8, [0x180cef1f0]` instruction at `0x18012361e`. All subsequent table accesses use `[RAX + R8 + offset]` where RAX = index × 0x88. This means patching the LEA's RIP-relative displacement redirects ALL table lookups to a new, larger table. See `docs/filter_ui_extension.md` for full disassembly and the AOB signature.

### `FUN_18011eda0` — Filter System Initialization

**Ghidra address**: `0x18011eda0`
**Runtime address**: `gamemdx.dll + 0x11EDA0`
**Namespace**: `sequence::selectmusic` (from lambda vtable RTTI)

This massive function initializes ALL filter categories for the song select screen. It builds static arrays of filter entries for each category. The version filter entries are built in the `DAT_1812303d0 & 2` block.

---

## Version Filter Entry Table

**Base address (Ghidra)**: `0x180cef1f0`
**Base address (runtime)**: `gamemdx.dll + 0xCEF1F0`
**Entry stride**: `0x88` bytes
**Entry count**: 10 (9 real entries + 1 sentinel)

Each entry contains (offsets relative to entry start):
- `+0x00`: Group index (u32) — which cabinet generation group this entry belongs to
- `+0x08`: Internal key string (std::string, 0x20 bytes SSO) — e.g., "world", "a20plus"
- `+0x30`: Series start value (u32) — the first series value in this entry's range
- `+0x38`: Short code string (std::string)
- `+0x58`: Display name string (std::string) — e.g., "WORLD", "EXTREME"

**Filter range logic**: Entry N covers series values `[entry[N].series_start, entry[N+1].series_start)`. The sentinel entry's series_start acts as the exclusive upper bound for the last real entry.

### Complete Entry Map

Verified against the live game's VERSION filter UI (scene 25):

| Index | Internal Key | Series Start | Series End (excl.) | Display Name | Group |
|-------|-------------|-------------|-------------------|-------------|-------|
| 0 | `1th5th` | 0 | 6 | 1st – 5thMIX | 0 (CLASSIC) |
| 1 | `maxex` | 6 | 9 | MAX – EXTREME | 1 (CLASSIC) |
| 2 | `novanova2` | 9 | 11 | SuperNOVA – SuperNOVA2 | 2 (CLASSIC) |
| 3 | *(3-char code)* | 11 | 14 | X – X3 VS 2ndMIX | 3 (CLASSIC) |
| 4 | *(4-char code)* | 14 | 17 | 2013 – 2014 | 4 (WHITE) |
| 5 | *(1-char code)* | 17 | 18 | A | 5 (WHITE) |
| 6 | `a20plus` | 18 | 20 | A20 – A20 PLUS | 6 (GOLD) |
| 7 | *(2-char code)* | 20 | 21 | A3 | 7 (GOLD) |
| 8 | `world` | 21 | 22 | WORLD | 8 (GOLD) |
| 9 | *(sentinel)* | 22 | — | *(empty)* | 9 |

### Cabinet Generation Groups

Defined in the `DAT_1812303d0 & 4` block of the filter init function:

| Group Name | Series Range Start | Filter Entry Indices |
|-----------|-------------------|---------------------|
| CLASSIC | 0 | 0–3 |
| WHITE | 4 | 4–5 |
| GOLD | 6 | 6–8 |
| *(sentinel)* | 9 | — |

The GROUP tabs (GOLD / WHITE / CLASSIC) in the filter UI correspond to these groups. Each group covers a contiguous range of filter entries.

---

## Raw Series Value Accessor

The raw series value is read via a virtual method call:

```asm
; At series_mapper + 0x27 (FUN_1800ff7b0 + 0x27):
mov rcx, [rax]       ; rcx = object pointer (from property accessor)
mov rax, [rcx]       ; rax = vtable pointer
call [rax+0xA0]      ; call vtable[20] — returns raw series as u8 in AL
movzx esi, al        ; zero-extend to 32-bit
```

The object accessed is obtained via `FUN_1801a5360` which appears to be a property lookup on the song's metadata. The vtable+0xA0 method returns the raw `<series>` value from musicdb.xml as a single byte.

**Confirmed via breakpoint**: 3,286 hits observed when toggling the WORLD filter. RSI values seen: 3, 5, 7, 8, 9, 11, 12, 14, 15, 17, 18, 19, 20, 21 — all valid series values from the current song library. No value > 21 observed (no custom songs in the test musicdb.xml).

---

## Internal Representation

- **Storage**: The series value is stored as a **u8** (single byte) in the per-song property object
- **Not a bitmask**: Values are used as direct integers, not bit flags
- **Not an index**: The raw value from musicdb.xml is used directly (after the mapper switch)
- **Theoretical range**: 0–255 (u8), but the mapper restricts to 0–21
- **The mapper is the only restriction**: The XML parser and property storage accept any u8 value. The mapper function is the single point where out-of-range values are clamped to 0.

---

## Patch Approaches for Series Range Extension

### Option A: Hook the series_mapper function (Recommended)

Hook `FUN_1800ff7b0` (AOB: `5C C3 CC CC CC CC CC CC CC CC CC CC CC CC CC CC 40 57 48 83 EC 40`, offset +16). In the hook:

1. Call the original function to get the mapped value
2. If the result is 0, re-read the raw series value (call vtable+0xA0 again, or capture it before calling original)
3. If the raw value is > 21, return the raw value instead of 0
4. Otherwise return the original result (preserves vanilla behavior including 15/16 merge)

**Pros**: Clean, minimal, does not break vanilla songs, survives game updates if the function prologue is stable.
**Cons**: Requires calling the original (trampoline).

### Option B: Binary patch the bounds check

Patch the `cmp eax, 0x14` at function+0x63 to a larger value (e.g., `cmp eax, 0xFE` = `83 F8 FE`), and add identity-return cases for values 22+ (or patch the `ja` to always fall through to a passthrough path).

**Pros**: Simpler conceptually.
**Cons**: The jump table is fixed-size (21 entries). Values > 21 would need a code cave to handle. More fragile across game updates.

### Option C: Hook + replace the entire function

Replace the function body with a simple passthrough: read vtable+0xA0, return the raw value directly (removing the switch entirely).

**Pros**: Simplest hook body.
**Cons**: Loses the 15/16 merge behavior. May cause issues if any code depends on the merge.

**Recommendation**: Option A. It is the safest — vanilla behavior is preserved exactly, and custom values pass through.

---

## What Happens to Songs with Series > 21 (Without Patching)

1. **musicdb.xml parsing**: Song loads normally. The `<series>30</series>` value is stored as u8 = 30.
2. **ALL MUSIC list**: Song **appears** (the song list builder does not filter by series).
3. **Version filter**: The series_mapper returns 0 for series 30. The song is categorized as "1st–5thMIX" (series range 0–5). Selecting the "1st–5thMIX" filter would show it alongside actual 1st–5thMIX songs.
4. **Cabinet generation**: The mapped value 0 falls in the CLASSIC group. The song appears under GROUP CLASSIC.

So custom songs are **not invisible** — they are **miscategorized**. Patching the mapper fixes the categorization; filter UI injection adds a proper filter entry for them.

---

## Addresses Quick Reference

All addresses are Ghidra base (`0x180000000`). Add `gamemdx.dll` runtime base to get runtime address.

| Symbol | Ghidra Address | Offset from DLL base | Description |
|--------|---------------|---------------------|-------------|
| `series_mapper` | `0x1800FF7B0` | `+0xFF7B0` | Series value mapper/validator (THE bounds check) |
| `series_mapper_cmp` | `0x1800FF813` | `+0xFF813` | The `cmp eax, 0x14` instruction |
| `series_mapper_ja` | `0x1800FF816` | `+0xFF816` | The `ja default` instruction |
| `series_mapper_default` | `0x1800FF8C3` | `+0xFF8C3` | Default case: `xor eax, eax` |
| `series_mapper_jmptable` | `0x1800FF8D8` | `+0xFF8D8` | Jump table (21 entries × 4 bytes) |
| `version_filter_predicate` | `0x1801235E0` | `+0x1235E0` | Filter comparison logic |
| `predicate_table_lea` | `0x18012361E` | `+0x12361E` | LEA R8 that loads table base — patchable |
| `filter_init` | `0x18011EDA0` | `+0x11EDA0` | Filter system initialization |
| `version_filter_entries` | `0x180CEF1F0` | `+0xCEF1F0` | Version filter entry array (data) |
| `series_mapper_wrapper` | `0x180191950` | `+0x191950` | Thin wrapper calling series_mapper |
| `series_string` | `0x18037E4AC` | `+0x37E4AC` | "series" string in .rdata |

### AOB Signatures

**series_mapper** (FUN_1800ff7b0):
```
5C C3 CC CC CC CC CC CC CC CC CC CC CC CC CC CC 40 57 48 83 EC 40
```
Offset +16 to reach function start. Unique in gamemdx.dll.

**series_mapper bounds check** (the `cmp eax, 0x14; ja` sequence inside the function):
```
40 0F B6 C6 FF C8 83 F8 14 0F 87
```
This is `movzx eax, sil; dec eax; cmp eax, 0x14; ja ...` — the core bounds check.

---

## Hook Signature

The series_mapper function takes one parameter (pointer in RCX) and returns u32 in EAX:

```
type SeriesMapperFn = unsafe extern "C" fn(*const u8) -> u32;
```

The parameter is a pointer to a shared_ptr-like wrapper around the song's property accessor. The function dereferences it (`mov rcx, [rcx]`) internally before using it.

---

## Resolved Questions

**Q: Is the series stored as a raw u8, index, or bitmask?**
A: Raw u8. Direct integer, not bitmask, not index.

**Q: Is the filter entry list fixed-size or dynamic?**
A: Fixed static array in .data with 59 hardcoded code references. Cannot be relocated — must use hook-based approach for injection (see filter_ui_extension.md).

**Q: What limits the valid range of series values?**
A: The switch statement in series_mapper (`FUN_1800ff7b0`). Single function, 2 callers. Hook it to extend the range.
