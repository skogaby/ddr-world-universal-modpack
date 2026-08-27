# Real Speed Fix — Anchor and Patch Site Re-Verification

## Overview

Re-verifies the binary anchors, patch sites, and byte payloads for porting the
**"Real Speed Calculations Fixed"** mod (R15, R16, R24, R25, R26 in
`docs/binary_modpack_research.md §4`) to this hook DLL using **byte-level
memory writes** rather than `retour` detours.

The mod has two independent pieces, both inside `ddr::player::Option::SetScrollSpeed`
(stock label `FUN_1801df8b0` on 20260421) and `FUN_18007ba70` (the score/scroll
display state-machine on 20260421):

1. **BPM divisor swap (R24-R26)** — replace the `divsd xmm0, [rcx]` (where
   `[rcx]` is *Max BPM* or its capped form) with `divsd xmm0, xmm2` and
   side-load *Core BPM* into `xmm2` from `[rbx + 0x88]` via a 12-byte stub
   placed in the function's int3 padding tail.

2. **`logf(0)` guard (R15-R16)** — redirect the bare `logf` call inside
   `FUN_18007ba70`'s `RSI == 0` fall-through path through a small RWX stub
   that returns 0 for zero-input instead of `-inf`/`NaN`. The R15
   single-byte JMP-disp tweak lengthens the no-fast-path branch so it lands
   *into* (not *past*) the guarded-call block.

Both pieces are **plain byte writes** at runtime-resolved addresses. No
`retour` detours, no trampolines. The R16 redirect target is a 14-byte
RWX stub allocated near `gamemdx.dll` via `core::memory::alloc_near()` so
its `JMP rel32` to bare-`logf` fits in a 4-byte displacement.

## Anchor Re-Verification

### R24-R26 anchor

```
F2 0F 5E 01 48 8D 4C 24 40
^-- divsd xmm0, qword ptr [rcx]
            ^-- lea rcx, [rsp+0x40]
```

Ghidra byte-pattern search results (no wildcards):

| Program                          | Resolved address | Match count |
|----------------------------------|------------------|-------------|
| `gamemdx.dll` (20260421)         | `0x1801df948`    | 1 (unique)  |
| `gamemdx_20250805_MODIFIED.dll`  | (no match)       | 0           |

The 20250805 modded build returns no match because R25 has *already* been
applied — its divsd reads `F2 0F 5E C2` (xmm0, xmm2) instead of
`F2 0F 5E 01` (xmm0, [rcx]). Searching for the patched form
`F2 0F 5E C2 48 8D 4C 24 40` on the modded DLL yields a unique match at
`0x1801c98d8`, confirming the doc's stated 20250805 stock anchor address.

The function around the anchor on 20260421 is `FUN_1801df8b0` —
`ddr::player::Option::SetScrollSpeed` (identified by Konami's own
`Ordinal_382("ddr::player::Option::SetScrollSpeed", "Updated. %d->%d", ...)`
log call at `0x1801df8c0`). On the 20250805 modded DLL it sits at the
homologous `0x1801c98d8` — same function body, same instruction layout.

### R15-R16 anchor

```
0F 28 C7 E8 ?? ?? ?? ?? F3 0F 58 C6
^-- movaps xmm0, xmm7
      ^-- call (bare logf in 'text' no-dot section)
                       ^-- addss xmm0, xmm6
```

Wildcards: 4-byte `CALL rel32` displacement (changes between versions).

| Program                          | Resolved address | Match count |
|----------------------------------|------------------|-------------|
| `gamemdx.dll` (20260421)         | `0x18007bc56`    | 1 (unique)  |
| `gamemdx_20250805_MODIFIED.dll`  | `0x180077be6`    | 1 (unique)  |

Both versions resolve to a single match. The byte at the anchor's start
(`0F`, the first byte of `MOVAPS XMM0, XMM7`) is the *target* of the R15
JMP after patching — see "Patch Site Layout" below.

## Scroll-Speed Display Decompile (20260421)

Ghidra decompile of `FUN_1801df8b0` — the function containing R24-R26
(comments mine; cleaned for clarity):

```c
void ddr::player::Option::SetScrollSpeed(longlong opt, double new_speed) {
    if ((double)*(int *)(opt + 0x14) != new_speed) {
        Ordinal_382("ddr::player::Option::SetScrollSpeed",
                    "Updated. %d->%d",
                    *(int *)(opt + 0x14), new_speed);
        *(int *)(opt + 0x14) = (int)new_speed;
    }

    double max_bpm = *(double *)(opt + 0x90);   // <-- Max BPM
    double cap     = DAT_18038dc48;             // <-- max-bpm cap (a *double* sentinel)
    double *divisor_ptr;
    int displayed_bpm;

    if (max_bpm <= _DAT_1802d9e10) {            // _DAT_1802d9e10 is 0.0 / lower bound
        *(int *)(opt + 0x10) = 100;             // displayed BPM = 100, return
        return;
    }

    if (max_bpm <= cap)
        divisor_ptr = &local_max_bpm;           // RCX = stack copy of max_bpm
    else
        divisor_ptr = &local_cap;               // RCX = stack copy of cap

    displayed_bpm = (int)((double)(*(int *)(opt + 0x14) * 100) / *divisor_ptr);
    // ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    //   This is the divsd at R25.  *divisor_ptr is Max BPM (or cap).
    //
    // The mod replaces *divisor_ptr with a register XMM2 that the cave
    // pre-loaded from [opt + 0x88] = Core BPM.

    /* clamp displayed_bpm to >= 25 and possibly to a different default,
       then store at [opt + 0x10] and return */
}
```

### Struct identity for `[rbx + 0x88]`

The research doc calls the BPM-source struct "ChartData", but the
disassembly disagrees. In `FUN_1801df8b0`:

```
1801df8b2: SUB RSP,0x30
1801df8bf: MOV RBX, RCX             <-- rbx = param_1 (the SetScrollSpeed `this`)
...
1801df8fd: MOVSD XMM1, [RBX + 0x90] <-- read max_bpm (used in the cap check)
```

`RBX` is `param_1`, which is the `ddr::player::Option*` (the Konami player
option block whose `+0x14` field is the scroll-speed multiplier). The
`+0x88` slot is on **`ddr::player::Option`**, not on `ChartData`. There
are three contiguous `double` fields at `+0x80` / `+0x88` / `+0x90` that
get populated by `FUN_1801df840` (a vtable `Set...BPMs(min,core,max)`
helper called from `*(*opt + 0xd0)`):

```c
void FUN_1801df840(longlong *opt, longlong p2, longlong p3, longlong p4) {
    opt[0x10] = p2;   // [opt + 0x80]    = Min BPM (or first BPM slot)
    opt[0x11] = p3;   // [opt + 0x88]    = Core BPM (the value the mod uses)
    opt[0x12] = p4;   // [opt + 0x90]    = Max BPM (the value stock divides by)
    ...
}
```

The `+0x80 / +0x88 / +0x90` triple has been stable across the 20250805
and 20260421 builds. Both anchors above land in the same function body
in both DLLs, with identical surrounding instruction layout — the
struct layout is an architectural invariant of the Konami player-option
class, not a build-time artifact.

**Correction to research doc:** s/`ChartData`/`ddr::player::Option`/. This
matters because the value at `[+0x88]` is whatever `Set...BPMs(min, core,
max)` was last called with — the Core BPM extracted from the chart's BPM
list — not a chart-row offset.

### int3 padding window (R26 cave location)

On 20260421, `FUN_1801df8b0` ends with `RET` at `0x1801df991`. Reading
the next 14 bytes:

```
0x1801df992: cc cc cc cc cc cc cc cc cc cc cc cc cc cc
                                           (14 bytes of int3)
0x1801df9a0: 48 89 5C 24 08 ...   <-- next function's prologue
```

14 bytes available; the cave needs **12** (8 for `MOVSD XMM2, [RBX+0x88]`,
2 for `JA rel8`, 2 for `JMP rel8`). The 2-byte tail is left as int3
padding — confirms the doc's claim that the cave fits in "the same
function's int3-padded tail, so it doesn't disturb existing code".

For 20250805 modded the cave at `0x1801c9922` is followed by the same
2 leftover `cc cc` bytes at `0x1801c992e`, then the next function at
`0x1801c9930` — identical layout.

## Patch Site Layout (Per-Version)

All five sites derive from anchor + a fixed offset; the offsets are
**version-agnostic** because the surrounding function layout is stable
between 20250805 and 20260421.

### R24-R26 (BPM divisor swap)

| Site | Offset from anchor (R24-R26) | 20250805 mod VA | 20260421 stock VA | Bytes |
|------|-------------------------------|-----------------|-------------------|-------|
| R24  | anchor − 0x1C                 | `0x1801c98bc`   | `0x1801df92c`     | 2 (`77 05` → `EB 64`) |
| R25  | anchor + 0x03                 | `0x1801c98db`   | `0x1801df94b`     | 1 (`01` → `C2`) |
| R26  | anchor + 0x4A                 | `0x1801c9922`   | `0x1801df992`     | 12 (cave payload) |

- **R24** is a 2-byte instruction; the `77` opcode (JA) becomes `EB`
  (JMP), and the displacement `05` becomes `64`. The unconditional
  jump's target is `(R24+2) + 0x64 = anchor + 0x4A` — exactly the
  cave's start.
- **R25** is a single-byte ModR/M change inside the 4-byte
  `divsd xmm0, …` instruction. ModR/M `01` selects `[rcx]`; `C2` selects
  `xmm2`. Opcode bytes `F2 0F 5E` are unchanged.
- **R26** writes 12 bytes into the int3 padding tail; the JA/JMP
  displacements inside the cave are functions of the cave's distance
  from the anchor (which is exactly 0x4A on both versions, so the
  displacement bytes are byte-identical).

### R15-R16 (logf guard)

| Site | Offset from anchor (R15-R16) | 20250805 mod VA | 20260421 stock VA | Bytes |
|------|-------------------------------|-----------------|-------------------|-------|
| R15  | anchor − 0x38                 | `0x180077bae`   | `0x18007bc1e`     | 1 (`48` → `37`) |
| R16  | anchor + 0x04                 | `0x180077bea`   | `0x18007bc5a`     | 4 (rel32 disp) |

- **R15** is the displacement byte of `JMP rel8` whose opcode `EB` sits
  one byte earlier (at anchor − 0x39). On stock the JMP skips *past*
  the entire `MOVAPS + CALL logf + ADDSS + CVTTSS2SI` block (lands at
  anchor + 0x10). Patched to `0x37`, the JMP lands at the anchor itself
  (the `MOVAPS XMM0, XMM7`), routing the `RSI == 0` fall-through
  through the guarded call instead of skipping it.

  The new displacement is computed as `anchor - (R15_VA + 1)`, which is
  `0x37` on **both** versions. Same byte to write.

- **R16** is the 4-byte rel32 of the `CALL` (the `E8` opcode is at
  anchor + 3; rel32 occupies anchor + 4..+7). Patch is **version-
  dependent** and address-dependent: the new rel32 must point at our
  RWX stub. Compute at runtime as
  `rel32 = STUB_VA - (anchor + 3 + 5) = STUB_VA - anchor - 8`.

## Byte-Write Payloads (Assembled)

### R24 — convert JA+5 to JMP+0x64

Write the 2-byte instruction starting at `(anchor − 0x1C)`. Original
two bytes are `77 05` (JA +5); new two bytes are `EB 64` (JMP +0x64):

```rust
let r24_addr = anchor.byte_offset(-0x1C);          // 0x1801c98bc / 0x1801df92c
write_u8(r24_addr,            0xEB);                // opcode JMP rel8
write_u8(r24_addr.add(1),     0x64);                // disp8
```

### R25 — change divsd ModR/M

```rust
let r25_addr = anchor.byte_offset(0x03);            // 0x1801c98db / 0x1801df94b
write_u8(r25_addr, 0xC2);                            // ModR/M for xmm0,xmm2
                                                     // (original was 0x01 = [rcx])
```

### R26 — write the 12-byte cave

```rust
let cave_addr = anchor.byte_offset(0x4A);           // 0x1801c9922 / 0x1801df992
let cave_bytes: [u8; 12] = [
    0xF2, 0x0F, 0x10, 0x93, 0x88, 0x00, 0x00, 0x00,  // movsd xmm2, [rbx+0x88]
    0x77, 0x97,                                       // ja  rel8 (-0x69 → anchor-0x15)
    0xEB, 0x90,                                       // jmp rel8 (-0x70 → anchor-0x1A)
];
for (i, b) in cave_bytes.iter().enumerate() {
    write_u8(cave_addr.add(i), *b);
}
```

Why these displacements:

- `JA rel8 -0x69` from `(cave + 8) + 2 = cave + 10` lands at
  `cave + 10 - 0x69 = anchor + 0x4A + 10 - 0x69 = anchor − 0x15`,
  which is the original `JA` taken-target address (the
  `MOV EAX, [RBX+0x14]` instruction at `0x1801df933` on 20260421 /
  `0x1801c98c3` on 20250805 modded).
- `JMP rel8 -0x70` from `(cave + 10) + 2 = cave + 12` lands at
  `anchor − 0x1A`, which is the original fall-through address (the
  `LEA RCX, [RSP+0x48]` at `0x1801df92e` on 20260421 / `0x1801c98be`
  on 20250805 modded).

Both displacements depend only on the cave's offset from the anchor
(`+0x4A`), which is identical on both versions. **So the 12-byte cave
payload is the same on every supported game version.**

### R15 — JMP rel8 displacement

```rust
let r15_addr = r15r16_anchor.byte_offset(-0x38);    // 0x180077bae / 0x18007bc1e
write_u8(r15_addr, 0x37);
```

Stock value at `0x18007bc1e` on 20260421 is `0x48` (verified by
`read_memory`); the patched value is `0x37`. **Same byte on both
versions** (the inter-block size between the JMP and its new landing
point — the anchor's `MOVAPS` byte — is a structural constant).

### R16 — CALL rel32 redirect

```rust
let r16_addr  = r15r16_anchor.byte_offset(0x04);    // 0x180077bea / 0x18007bc5a
let call_pc   = r15r16_anchor.byte_offset(0x03);    // address of the E8 opcode byte
let after_pc  = call_pc.add(5);                     // address after the 5-byte CALL instruction
let new_disp  = (stub_va as isize) - (after_pc as isize);
debug_assert!(new_disp >= i32::MIN as isize && new_disp <= i32::MAX as isize,
              "stub allocated outside +/-2 GB of CALL site - use alloc_near()");
write_i32(r16_addr, new_disp as i32);
```

Validation: on 20260421 stock, the **original** rel32 at this site is
`0x0122BF62`, giving target `0x18007bc59 + 5 + 0x0122BF62 = 0x1812A7BC0`,
which is the bare `logf` function in the `text` (no-dot) section
(verified by disassembly — classic SSE-based float-`logf`, see
"logf Wrapper Strategy" below).

## logf Wrapper Strategy

R16 currently calls bare `logf` directly. The mod replaces this with a
guarded wrapper: `f(x) = (x == 0.0f) ? 0.0f : logf(x)`. The original
modder placed the wrapper inside their cave-3 RWX page; for our hook
DLL, the equivalent is a 14-byte stub allocated via
`core::memory::alloc_near` so its `JMP rel32` to bare-`logf` fits a
signed 32-bit displacement.

### Bare-`logf` derivation (no hardcoding)

The bare `logf` lives in the `text` (no-dot) section, which is *not* at
a stable VA across versions:

| Version  | `text` section start | bare `logf` address | offset within section |
|----------|----------------------|---------------------|-----------------------|
| 20250805 | `0x18126e000`        | `0x18126e000`       | `+0x000`              |
| 20260421 | `0x1812a7000`        | `0x1812a7bc0`       | `+0xbc0`              |

The function entry is at the section start on 20250805 but offset
`+0xbc0` on 20260421. **Do not hardcode either address.** Derive at
runtime from the existing R16 `CALL rel32`:

```rust
// R16 anchor -> CALL E8 byte at anchor + 3
let call_pc        = r15r16_anchor.byte_offset(0x03);
let original_rel32 = read_i32(call_pc.add(1));            // unpatched value
let bare_logf_va   = (call_pc as isize + 5 + original_rel32 as isize) as *const u8;
log_info!("Real Speed: bare logf resolved to {:p}", bare_logf_va);
```

This resolves bare-`logf` once, before the R16 patch is applied. The
runtime read happens before any byte writes; storing the value is
mandatory because once R16 is patched, `read_i32(call_pc + 1)` would
no longer return the bare-logf displacement.

### Stub layout (14 bytes)

```
Offset  Bytes              Disassembly
------  -----------------  ----------------------------------------
0x00    0F 57 C9           xorps xmm1, xmm1
0x03    0F 2E C1           ucomiss xmm0, xmm1
0x06    75 01              jne short +1                ; skip to 0x09
0x08    C3                 ret                         ; (xmm0 already 0.0)
0x09    E9 dd dd dd dd     jmp rel32  bare_logf        ; tail-call
                                                       ; rel32 at offset 0x0a
```

- The `XORPS + UCOMISS` pair tests `xmm0 == 0.0f` exactly. NaN handling:
  if input is NaN, `UCOMISS` sets PF=1 and the `JNE` (which checks ZF
  alone, not PF) takes the not-equal branch — so NaN inputs reach
  `logf` and `logf` itself decides what to return (NaN, by IEEE-754).
  This matches the original mod's stated behavior (only `0.0` is
  treated specially).
- The `RET` returns with `xmm0` containing the original input — which
  was `0.0`, so the returned value is `0.0`. **No need to re-zero
  `xmm0` before the return.**
- The tail-call is a single `JMP rel32`: bare `logf` already does its
  own stack frame and returns to the caller of our stub (the original
  R16 call site), so this is correct.

### Allocation

```rust
let stub_size  = 14usize;
// Allocate within +/-2 GB of the call site so JMP rel32 to bare_logf fits.
let stub_va    = memory::alloc_near(call_pc, stub_size);
assert!(!stub_va.is_null(), "alloc_near for logf-guard stub failed");

// Write stub bytes
let stub_bytes: [u8; 14] = [
    0x0F, 0x57, 0xC9,                   // xorps xmm1, xmm1
    0x0F, 0x2E, 0xC1,                   // ucomiss xmm0, xmm1
    0x75, 0x01,                         // jne +1
    0xC3,                               // ret
    0xE9, 0x00, 0x00, 0x00, 0x00,       // jmp rel32 (displacement filled in below)
];
std::ptr::copy_nonoverlapping(stub_bytes.as_ptr(), stub_va, 14);

// Patch the JMP rel32 inside the stub to land at bare_logf
let jmp_disp_addr  = stub_va.add(0x0A);                        // rel32 location
let jmp_after_addr = stub_va.add(0x0E);                        // address after the 5-byte JMP
let jmp_disp       = (bare_logf_va as isize) - (jmp_after_addr as isize);
debug_assert!(jmp_disp >= i32::MIN as isize && jmp_disp <= i32::MAX as isize);
write_i32(jmp_disp_addr, jmp_disp as i32);
```

`alloc_near` already returns RWX (PAGE_EXECUTE_READWRITE) per
`core::memory.rs`, so no separate `VirtualProtect` call is needed.

### Stub disable / cleanup

When the mod toggles off, the order is reverse-of-install:

1. Restore the original 4 R16 rel32 bytes (so the `CALL` points back to
   bare `logf`).
2. Restore the R15 disp8 (`0x37` → `0x48`).
3. Restore the R26 cave (12 bytes of `int3 = 0xCC`).
4. Restore R25 ModR/M (`0xC2` → `0x01`).
5. Restore R24 (`EB 64` → `77 05`).
6. `VirtualFree` the stub (or leak it; the page is small and we may
   want to re-enable later — leaking is acceptable for the lifetime of
   the process).

Keep the original byte values in mod-state on enable so they're
available on disable; do **not** assume the patches restore to a fixed
constant — even though in practice they do for this mod, that
assumption breaks if a future game version changes the surrounding
instruction sizing.

## Cross-Version Notes

**What's stable (verified):**

- The function `ddr::player::Option::SetScrollSpeed` exists at the same
  shape on both versions (verified by Konami's
  `Ordinal_382("ddr::player::Option::SetScrollSpeed", ...)` log call
  inside the function body).
- `ddr::player::Option` struct field offsets `+0x14` (scroll-speed
  multiplier int), `+0x80 / +0x88 / +0x90` (Min / Core / Max BPM
  doubles), `+0x10` (computed displayed BPM int) are stable across
  20250805 and 20260421. Verified via the field accesses in
  `FUN_1801df840` (the `Set...BPMs` setter, vtable slot `+0xd0`) and
  `FUN_1801df8b0` (the user `SetScrollSpeed`).
- The 14-byte int3 padding tail at the end of `SetScrollSpeed` exists
  on both versions and is large enough for the 12-byte cave.
- All five patch sites are at fixed offsets from their anchors;
  byte-payload values for R15, R24, R25, R26 are byte-identical on
  both versions. Only R16's rel32 is version-dependent (because it
  points at a runtime-allocated stub).

**What's NOT stable:**

- The bare `logf` VA inside the `text` (no-dot) section. On 20250805
  it's at `+0x000` of the section; on 20260421 it's at `+0xbc0`. Always
  derive at runtime by reading the original `CALL rel32` at the R16
  site **before** patching it.
- The whole `text` section's start address shifts by +0x39000 between
  the two builds. Not relevant if we always derive bare `logf` from the
  R16 `CALL` instead of from section base.

**Anchor uniqueness:** confirmed by `mcp__ghidra__search_byte_patterns`:

- R24-R26 anchor `F2 0F 5E 01 48 8D 4C 24 40` — exactly 1 match on
  20260421 (no match on 20250805 modded because R25 has been applied;
  the 20250805 stock VA from the research doc is `0x1801c98d8` and the
  modded form is `F2 0F 5E C2 48 8D 4C 24 40`, which is also unique on
  the modded DLL — confirming the anchor is structurally unique).
- R15-R16 anchor `0F 28 C7 E8 ?? ?? ?? ?? F3 0F 58 C6` — exactly 1
  match on each version: `0x18007bc56` (20260421) and `0x180077be6`
  (20250805 modded).

## Summary Table — All Five Sites at a Glance

| Site | Anchor                                                  | Offset    | Bytes (original → patched) | Version-agnostic? |
|------|---------------------------------------------------------|-----------|----------------------------|-------------------|
| R24  | `F2 0F 5E 01 48 8D 4C 24 40`                            | `-0x1C`   | `77 05` → `EB 64`          | YES               |
| R25  | `F2 0F 5E 01 48 8D 4C 24 40`                            | `+0x03`   | `01` → `C2`                | YES               |
| R26  | `F2 0F 5E 01 48 8D 4C 24 40`                            | `+0x4A`   | `CC × 12` → 12-byte cave   | YES (same bytes)  |
| R15  | `0F 28 C7 E8 ?? ?? ?? ?? F3 0F 58 C6`                   | `-0x38`   | `48` → `37`                | YES               |
| R16  | `0F 28 C7 E8 ?? ?? ?? ?? F3 0F 58 C6`                   | `+0x04`   | rel32 → rel32 to stub_va   | NO (depends on stub_va) |

## Gotchas

- **R25 ModR/M alone is not enough.** Without R26's MOVSD pre-loading
  XMM2 with Core BPM, the divsd would divide by whatever happens to be
  in XMM2 (garbage from prior code in the function). All three of
  R24/R25/R26 must be applied as a unit.

- **R15 patches a SHORT JMP, not a near JMP.** The displacement is a
  signed 8-bit value. Both stock (`0x48 = +72`) and patched
  (`0x37 = +55`) values are positive and within range — no
  sign-extension concerns. But if a future game version inserted code
  that pushed the anchor more than 127 bytes from the JMP, the rel8
  encoding would no longer reach. Verify on each new game version.

- **R26 cave's branches assume EFLAGS preservation.** The original
  COMISD at `0x1801df922` (20260421) sets the flags consumed by the
  `JA` at `0x1801df92c` (= R24 site). After R24 is replaced with `JMP`
  (which doesn't read flags), the flags reach the cave intact;
  `MOVSD XMM2, [RBX+0x88]` doesn't modify EFLAGS, so the cave's `JA`
  reads the original COMISD flags. **Do not insert any flag-modifying
  instruction before the cave's `JA`.**

- **Bare `logf` derivation is one-shot.** The R16 `CALL rel32` must be
  read **before** the patch is applied. After patching, the rel32 at
  the call site points at the stub instead. If you ever need to
  rebuild the stub (re-enable after disable), re-read the original
  rel32 from saved state, not from the current memory.

- **`alloc_near` returns null on failure.** Treat null as a
  graceful-degradation case: log a warning, leave R15/R16 unpatched,
  but still apply R24-R26 (the BPM divisor swap stands alone — it
  doesn't depend on the logf guard).

- **No `write_bytes` helper yet in `core::memory`.** The current module
  exposes `write_u8`, `write_u32`, `write_i32`, etc. — all typed.
  Either chain the byte writes per offset (as the snippets above do)
  or add a `write_bytes(addr, &[u8])` helper to `core::memory.rs` as
  part of this mod's implementation. Multiple writes do not need to
  be atomic — the patched function is not running during init-time
  install (DllMain is single-threaded before the game's render thread
  starts).

- **Two patch families, one mod toggle.** Per `idea-honing.md` Q10,
  both patches are gated by the same `real_speed_core_bpm` JSON
  sub-toggle. Apply all 5 sites on enable; revert all 5 on disable.
  Don't expose them as separate sub-toggles unless the user asks for
  finer control later.
