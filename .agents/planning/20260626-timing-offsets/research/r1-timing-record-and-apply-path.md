# R1 — Timing record & apply path (fresh re-verification, both 64-bit builds)

**Status: CONFIRMED on both 20260324 (primary) and 20260526 (cabinet).** Every claim
below was re-derived from the binary this session via the named string/structure anchors;
the documented absolute addresses in `docs/hex_edit_porting.md` were treated as provenance
only and independently re-verified. Addresses are file-relative to the `0x180000000` base.

## Anchors (re-found by string search)

| String | 20260324 VA | 20260526 VA |
|---|---|---|
| `"ConfigBank.csv"` | `0x18035a1d0` | `0x18035c210` |
| `"Timing Init: %d"` | `0x18035a260` | `0x18035c2a0` |
| `"SOUND_OFFSET"` | `0x18035a218` | `0x18035c258` |
| `"INPUT_OFFSET"` | `0x18035a228` | `0x18035c268` |
| `"RENDER_OFFSET"` | `0x18035a238` | `0x18035c278` |
| `"BOMB_FRAME_OFFSET"` | `0x18035a248` | `0x18035c288` |
| `"HIGH_PRECISION_INPUT"` | `0x180359910` | `0x18035b950` |

## Function map (re-derived via xrefs to the anchors)

| Role | 20260324 | 20260526 |
|---|---|---|
| Timing init / **boot publisher** | `FUN_18002bbd0` | `FUN_18002bbb0` |
| Record **builder** (`out, presetIndex`) | `FUN_180012f50` | `FUN_180012e30` |
| Preset **selector** (hardware→index) | `FUN_180012e50` | `FUN_180012d30` |
| Config-map **int setter** (`key, value`) | `FUN_1801acbf0` | `FUN_1801ae460` |
| Config-map **int getter** (`key, *out`) | `FUN_1801ace10` | `FUN_1801ae680` |
| Config-map **bool setter** (HIGH_PRECISION_INPUT) | `FUN_1801acb50` | `FUN_1801ae3c0` |
| Config-map **global** (root) | `DAT_1806ebcf0` | `DAT_1806f1d70` |
| **GamePlayActor ctor** (latches offsets) | `FUN_18005b4c0` | `FUN_18005a6b0` |
| 2nd publisher (re-publish w/ user delta) | `FUN_18002e2b0` | `FUN_18002e180` |

## Boot publisher (`FUN_18002bbd0` @ 20260324) — verified decompile

```c
// ... loads ConfigBank.csv row, picks preset index (csv-name match → 9+idx, else selector) ...
puVar3 = (ulonglong*)FUN_180012f50(&rec, presetIndex);   // builder(out, index)
local_68 = *puVar3;        // [+0x00]=SOUND, [+0x04]=INPUT
local_60 = puVar3[1];      // [+0x08]=RENDER, [+0x0C]=BOMB_FRAME
local_58 = (u32)puVar3[2]; // [+0x10]=HIGH_PRECISION_INPUT (bool, low byte)
if (DAT_1806ebcf0 != 0) {                                  // config map exists
    FUN_1801acbf0("SOUND_OFFSET",      local_68 & 0xffffffff);
    FUN_1801acbf0("INPUT_OFFSET",      local_68 >> 32);
    FUN_1801acbf0("RENDER_OFFSET",     local_60 & 0xffffffff);
    FUN_1801acbf0("BOMB_FRAME_OFFSET", local_60 >> 32);
    FUN_1801acb50((u8)local_58);                           // bool setter, key hardcoded
}
XCnbrep700017c(&..., "Timing Init: %d", presetIndex);
```

Cabinet build `FUN_18002bbb0` is structurally identical (setter `FUN_1801ae460`, bool
setter `FUN_1801ae3c0`, builder `FUN_180012e30`, selector `FUN_180012d30`).

## Record layout (5 fields, 0x14 bytes) — CONFIRMED

| Off | Key | Type | rec0 default |
|---|---|---|---|
| `+0x00` | `SOUND_OFFSET` | i32 | 0x57 = 87 |
| `+0x04` | `INPUT_OFFSET` | i32 | 0x1C = 28 |
| `+0x08` | `RENDER_OFFSET` | i32 | 0x11 = 17 |
| `+0x0C` | `BOMB_FRAME_OFFSET` | i32 | 0x00 = 0 |
| `+0x10` | `HIGH_PRECISION_INPUT` | bool | 0x01 = on |

The builder copies exactly 5 dwords (0x14 bytes) for clamped index 0..9
(`MOVSXD; LEA RAX+RAX*4; LEA RSP+RCX*4` → 5 dword stride). Record 0's first 16 bytes load
from `.rdata` via `MOVDQA`; records 1..9 and all `+0x10` bool bytes are inline `MOV`
immediates.

### `.rdata` rec0 defaults — read from the binary

- 20260324: `0x180358960` = `57 00 00 00 | 1C 00 00 00 | 11 00 00 00 | 00 00 00 00`
- 20260526: `0x18035a950` = `57 00 00 00 | 1C 00 00 00 | 11 00 00 00 | 00 00 00 00`

Both → SOUND 87, INPUT 28, RENDER 17, BOMB 0. (The bool's `+0x10` byte is not in this
16-byte `.rdata` blob — it's set inline to 1 in every record.)

## Config-map setter internals (`FUN_1801acbf0`) — verified

- Hashes the key string with **FNV-1a** (seed `0x811c9dc5`, prime `0x1000193`).
- Walks the int sub-map at `*(DAT_1806ebcf0 + 0x28)` (a red-black tree keyed by the hash).
- **Update-only:** writes `value` to `node+0x1c` **iff the key node already exists**; if not
  found it returns failure WITHOUT inserting. → The four timing keys must already be in the
  map (they are, after the boot publisher runs) for a runtime re-set to take effect.
- Int **getter** `FUN_1801ace10(key, *out)` is the same hash+walk but reads `node+0x1c`
  into `*out`. Bool setter `FUN_1801acb50` hashes the hardcoded `"HIGH_PRECISION_INPUT"`
  and writes `node+0x1c` (low byte) in the bool sub-map at `*(DAT_1806ebcf0 + 0x08)`.

## Setter call-site census (apply-lever safety)

The int setter `FUN_1801acbf0` has **exactly 8 call sites**, ALL timing-related:
- 4 in the boot publisher `FUN_18002bbd0` (the four offsets),
- 4 in the 2nd publisher `FUN_18002e2b0` (re-publish base+userdelta; see R2/R4).

No other code calls this setter (the lone extra xref `0x1812930d0` is the CFG/IAT entry
to the function itself, not a caller). **Consequence:** hooking the int setter and
filtering by the four key strings intercepts every write to the timing offsets and nothing
else — a clean, complete apply lever. (The getter `FUN_1801ace10` is called only 4×, all
in the GamePlayActor ctor — see R2.)

## AOB signatures (authored + cross-checked on both builds)

### Int setter (the apply-lever hook target) — resolve SEMANTICALLY, not by prologue

> ⚠️ **CORRECTION (found at Step-1 implementation via `scan_all`).** The setter *prologue*
> AOB `48 89 7C 24 10 4C 8B C9 48 83 C9 FF 33 C0 49 8B F9 44 8B D2 41 B8 C5 9D 1C 81` is
> **NOT unique** — it matches **two** byte-identical functions on each build (the timing
> setter `0x1801acbf0`/`0x1801ae460` AND a twin `0x1801ac9d0`/`0x1801ae240` that sets a
> *different* config map). The two are identical FNV-1a int-setters differing only in the
> RIP-relative map-global they load (`MOV RDX,[rip+disp]` near the tail), so **no pure-byte
> prologue pattern can distinguish them.** The `44 8B D2` byte only distinguishes setter from
> *getter*, not setter from its twin. Do not use the prologue AOB to resolve the setter.

**Correct resolution — derive from the publisher's `LEA RCX,[SOUND_OFFSET]; CALL setter`
site (semantic anchor; the int-setter the game calls to publish `SOUND_OFFSET` *is* the
timing setter by definition).** Landmark AOB (the publisher's 4 consecutive config-set
pairs; first match = the SOUND pair), **verified unique to the publisher on both builds**
(3 overlapping hits of the 4-call run, all inside the publisher, nothing elsewhere):

```
8B 55 ?? 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 8B 55 ?? 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ??
```
- Each pair is `MOV EDX,[RBP+d]; LEA RCX,[rip+key]; CALL setter`. The **first** `E8` (at
  landmark offset **+0x0A**) is the SOUND_OFFSET set-call; `decode_call_rel32` it → the
  timing int setter address.
- Verified: 20260324 landmark @ `0x18002bd71`, `+0x0A` CALL → `0x1801acbf0`. 20260526 landmark
  @ `0x18002bd51`, `+0x0A` CALL → `0x1801ae460`. Both correct.
- Implementation: add the landmark as signature `timing_set_call_landmark`; derive
  `timing_config_set_int` in `resolve_derived()` via `decode_call_rel32(landmark + 0x0A)`.

### Record builder inline pair (alt anchor / for default-patch fallback) — both builds

```
C7 45 ?? 57 00 00 00 C7 45 ?? 1C 00 00 00
```
- This is the **record-1** inline `MOV [RBP+d],0x57; MOV [RBP+d],0x1C` pair
  (`180012f88`/`180012e68`). Record 0 is the `MOVDQA` from `.rdata`, so this anchors the
  builder body. Matches the doc's anchor and present on both builds.

## Preset-selection nuance (not used by this mod)

The boot publisher first tries to match a ConfigBank.csv-derived name string against a
3-entry table (`local_50..` loaded from `.rdata`), using `index+9` if matched; otherwise it
calls the hardware selector `FUN_180012e50` (Hack 6's target). This only chooses *which of
the 10 presets* seeds the published values; this mod overrides the published values
regardless of preset, so the selection path is immaterial here. (Documented for
completeness / Hack-6 cross-reference.)

## Cross-version / running-build caveat

Both Ghidra DBs (20260324, 20260526) match structurally; only absolute addresses differ.
Per the `filtersort-version-bitfield-count` memory note, the *running* OmniMAX-style
gamemdx may not match either DB offset-for-offset, so all addresses MUST be AOB-resolved at
load time, never hardcoded. The setter prologue AOB above is the primary resolution target;
the builder inline-pair AOB is the secondary anchor. Confirm `scan_all` uniqueness on the
live build during implementation.

## Bottom line for design

- **Apply lever = hook the int setter** (AOB above), filter on the four FNV-hashed keys (or
  compare the key string pointer arg), override/seed the value. One detour, covers both
  publishers, and the GamePlayActor latch then reads our value at gameplay entry.
- Record layout, defaults, setter/getter/bool-setter, and the config-map global are all
  re-confirmed. HIGH_PRECISION_INPUT location confirmed for provenance (out of scope per
  requirements Q1).
