# R1 — The FPS target site (app-init imm32)

**Status: CONFIRMED, fresh, on all THREE loaded builds.** Re-verified against the binary
this session (not inherited from `docs/hex_edit_porting.md`). Addresses are file-relative
to `gamemdx.dll` base `0x180000000` (Ghidra VAs).

## Summary

The fullscreen display-refresh target ("FPS") is a single **imm32 = `0x3C` (60)**
written into a stack-local struct inside the main app-init function `Application::onBoot()`
(`FUN_1800020f0`). A sibling branch overrides it to `0x4B` (75) iff `MachineType == 1`.
The struct is then handed to the screen-graph init, which (R2) copies the value into a
global and consumes it **once** to configure the D3D device.

## The site (build 20260324 — `gamemdx.dll`)

```asm
180002631:  LEA  RCX,[RSP+0x40]
180002637:  CALL qword ptr [0x1806eb2a8]      ; arkMDXGetMachineType(&machineType@[RSP+0x40])
18000263d:  MOV  EDX,dword ptr [RSP+0x40]
18000263b:  DEC  EDX
18000263d:  MOV  dword ptr [RSP+0x6c],0x3c    ; <-- DEFAULT TARGET = 60   (imm32 here)
180002645:  JNZ  0x18000264f                  ; if (machineType-1 != 0) skip
180002647:  MOV  dword ptr [RSP+0x6c],0x4b    ; machineType==1 -> 75
18000264f:  ...                               ; (PCType probe + more flags)
1800026ae:  LEA  RCX,[RSP+0x50]               ; &struct
1800026b3:  CALL 0x1801f0030                  ; screen-graph init (see R2)
```

- Containing function: **`FUN_1800020f0` = `Application::onBoot()`** (confirmed by the
  string `"Application::onBoot() end."` referenced near the function epilogue, and the
  whole-function decompile being the engine/screen-graph/sound bootstrap). Body
  `1800020f0–180002a9e`.
- The patchable immediate is the **`3C` at match-offset +4** (the `MOV [RSP+0x6c], 0x3C`).
- `[RSP+0x6c]` is **struct_base+0x1C** (struct base = `[RSP+0x50]`, passed to the
  consumer). See the +0x1C correction in R2.

## AOB signature (authored + uniqueness-checked this session)

```
C7 44 24 ?? 3C 00 00 00 75 08 C7 44 24 ?? 4B 00 00 00
```

- `C7 44 24 ??` = `MOV dword ptr [RSP+disp8], imm32`; the `??` wildcards the stack disp8
  (it is `0x6C` on all three builds, but wildcarding is harmless and future-proof).
- imm32 `3C 00 00 00` = the default 60. **The byte to patch is at match_offset + 4.**
- `75 08` = `JNZ +8`; `C7 44 24 ?? 4B 00 00 00` = the `==1 -> 75` branch.

### Uniqueness — single match on every build

| Build | Ghidra program name | match VA | imm32 (`3C`) VA | bytes at match (19) |
|---|---|---|---|---|
| 20260324 | `gamemdx.dll` | `0x18000263d` | `0x180002641` | `c744246c3c0000007508c744246c4b00000048` |
| 20260526 | `gamemdx_20260526.dll` | `0x1800025bd` | `0x1800025c1` | `c744246c3c0000007508c744246c4b00000048` |
| 20250805 | `gamemdx_20250805_STOCK.dll` | `0x18000261d` | `0x180002621` | `c744246c3c0000007508c744246c4b00000048` |

`search_byte_patterns` returned **exactly one** hit per build. Bytes are **byte-identical
across all three** builds (same stack disp `0x6C`, same structure). The AOB is safe to use
unwildcarded-disp, but we keep the `??` per project convention.

## Cross-version note

The prior doc (`hex_edit_porting.md` Hack 5) verified only 20260324 + 20260526 and gave
the same two match VAs — both reproduced exactly. **20250805 is a third build the prior
doc never checked**; it also matches uniquely with identical bytes, strengthening the
version-agnostic claim. (Absolute addresses differ; AOB-resolve, never hardcode the
offset — per CLAUDE.md rule 9. The "no hardcoded *file offsets*" clarification from
idea-honing Q6 applies: an AOB-resolved byte patch here is fully convention-compliant.)

## `MachineType==1 -> 75` branch

- Probe is `(*DAT_1806eb2a8)(&machineType)` = the `arkMDXGetMachineType` dispatch
  (matches the prior doc + Hack 6 notes). Per maintainer (idea-honing Q4): **no real
  cabinet reports MachineType==1**, so the 75 branch is dead in practice.
- For our patch we overwrite only the `0x3C` immediate. The `==1 -> 75` branch still
  exists after patching; on a (hypothetical) MachineType==1 cabinet it would override our
  value with 75. Moot in practice. If we ever want to be airtight we could also NOP the
  `JNZ`/`75`-store, but it's unnecessary given no 75Hz cabinets exist.

## What this means for the apply lever

Two viable levers (decision in R2 / design):
1. **Byte-patch the imm32** at `match+4` (AOB-resolved) — overwrite `0x3C` with the
   chosen target. Mirrors `patches.js`. Must win the race to patch before `onBoot` runs
   that line (R2 analyzes timing).
2. **Hook `onBoot`** (or a narrower point) and rewrite the value after compute — captures
   genuine stock naturally. Heavier; see R2 for the race/timing analysis that decides
   between (1) and (2).

> Convention-compliant either way (AOB-resolved). The tiebreaker is the boot-timing race
> (R2).
