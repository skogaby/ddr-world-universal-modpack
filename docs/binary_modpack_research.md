# Pre-modded gamemdx.dll Research — Mod Inventory and Port Strategy

## Overview

A community-distributed pre-modded build of DDR World (`gamemdx_20250805_MODIFIED.dll`) ships **17 mods** as direct binary patches against the stock 20250805 `gamemdx.dll`. The README documents user-facing behavior; this doc reverse-engineers the actual binary changes so each mod can be re-implemented in this universal hook DLL without static patching, surviving game updates.

The pre-modded build does NOT add a new module: it stays inside the same `gamemdx.dll`, repurposes the alignment padding at the end of `.text` (~416 free bytes at `0x1802b8a60`), the existing-but-tiny `text` (no dot) section at `0x18126e000` (8693 bytes of math routines), and the previously-zero-filled tail of `.rdata` at `0x1802da400` (~10 KB of strings + a runtime-relocated executable blob). At init it also calls `VirtualProtect` (added to the IAT) on a fresh page in the gap between `text` and `data` (`0x181270000–0x181270fff`) and `rep movsb`s a `0xE00`-byte payload from `.rdata` to that page, which becomes the live mod runtime.

This means **every binary edit either (a) overwrites a single instruction with a `call`/`jmp` into one of the two code caves, or (b) tweaks an immediate / branch displacement in place.** No code is inserted between existing instructions; all hook redirects use 5-byte rel32 instructions that exactly replace 5..6-byte stock instructions, with the original instruction restored as the final instruction of the trampoline before falling through.

For a portable re-implementation in this hook DLL the strategy is therefore:
1. Find each affected stock instruction by AOB scan.
2. Install a `retour` static detour or a manual mid-function inline hook there.
3. Replicate the trampoline's logic in safe Rust, replacing operator-menu reads with this codebase's existing config service.

## Binary diff summary

A byte-level diff between the stock and modified 20250805 DLLs yields **44 contiguous regions** of change (gap-tolerant grouping at 16 bytes). They fall into four classes:

| Class | Count | Where | Purpose |
|---|---|---|---|
| Inline hook redirect (5-byte `e8`/`e9`) | 11 | `.text` | Patch sites that swap an instruction for a `call`/`jmp` into a code cave |
| Instruction-level tweaks (1-20 bytes) | 12 | `.text` | Constant changes, branch displacement nudges, NOP-outs, RIP-relative redirects |
| Mod payload | 5 | `.text` padding (cave 1, 416 B), `text` section (cave 2, 8693 B), `.rdata` (loader payload, 0xE00 B + tables, ~10 KB) | The actual mod code/data |
| PE structure tweaks | 4 | PE header timestamp, IAT extension for `VirtualProtect`, two `.data` pointer-table redirects | Plumbing for the loader |

### Code cave layout

```
0x1802b8a60  +416 B   "Cave 1" — bootstrap loader, lives in .text section padding
                       0x1802b8a60  loader entry: VirtualProtect( 0x181270000, 0x1000, RX )
                                     rep movsb  copies 0xE00 B from .rdata payload to 0x181270200
                                     calls back into 0x1812703c8-style helpers, then jmp 0x180003edb
                                     (the original gameInit body that was overwritten by the install jmp)
                       0x1802b8aa8  VirtualProtect helper
                       0x1802b8af0  Premium Free (∞ stages) trampoline
                       0x1802b8b10  state-machine config-cache helper
                       0x1802b8b38  config-cache loader (operator menu → mirror BSS at 0x1802c2108..)
                       0x1802b8b80  Force Logout (save) trampoline (Premium Free)
                       0x1802b8ba8  Force Logout post-stage cmp neutralizer
                       0x1802b8bc0  unused doublewide xmm helper
                       0x1802b8bd8  Force Event Mode state-rewrite trampoline

0x18126e000  +0x21F5  "Cave 2" — math routines, lives in pre-existing 'text' section
                       Contains float versions of logf and a more accurate sin/cos used by
                       Real Speed math (see 0x1812703b0 wrapper).

0x181270000  +0x1000  "Cave 3" — payload, allocated by VirtualProtect at runtime, copied from .rdata
                       Hosts the bulk of the mod logic: most of the 11 trampolines call here.
                       Address layout (selected stubs only):
                         0x181270200  loader continuation (re-enters DllMain init flow)
                         0x181270228  read-and-cache "thres" operator value (R12 helper)
                         0x181270248  Start-held check (BOTH players) — calls arkMDXGetStart twice and returns ZF=0 when ANY player's Start is held
                         0x181270260  arkMDXGetStart wrapper for one player slot
                         0x1812702a0  "commit current step into ring buffer" (CSV export, ms-error stats)
                         0x181270300  per-state-transition mod-init dispatcher (R8) — only fires on state==0 (boot)
                         0x181270350  read "timing_windows" operator → copy label (7 B) + range table (0x18 B) into mod BSS
                         0x1812703b0  guarded logf wrapper: if x == 0 return 0, else call cave2's logf
                         0x1812703c8  Konami's avs property-tree int-getter wrapper (calls arkGetTestModeSettingsS32 via ess.dll)
                         0x181270410  4-iteration loop reading sound/input/render/bomb offsets and writing them into game's frame_offset[idx]
                         0x181270448  write per-frame internal-offset value into game's frame_offset[idx]
                         0x181270478  read "pacemaker" operator setting → BSS byte 0x1802c219a
                         0x181270490  Pacemaker→MS-Error color-decision wrapper (R14)
                         0x1812704c0  read "force_event"/"freeplay" operator settings → game state bytes
                         0x181270500  Step Export per-judgment commit + per-step ring-buffer commit (R17)
                         0x1812706e0  Pacemaker→MS-Error score-delta override (R13)
                         0x181270700  comparison: player.pacemaker_target == mod.pacemaker_mode? (returns ZF)
                         0x181270730  number-formatting helper (used for stats display)
                         0x181270898  end-of-CheckStepDataActor::onUpdate tail-recurse: drain ready queue (NOW LOADING optimization, mod #11)
                         0x1812708e8  Step Export CSV dump trigger (op-menu "save" key gate; called from R19 trampoline) — mod #12
                         0x181270920  step-data CSV write-out: opens nvram file, writes header + per-step rows
                         0x181270a58  Speed-toggle quantum dispatcher: ±0.05× by default, ±0.50× when OTHER player's Start is held (mod #13)
                         0x181270a80  step-counter / per-judgment counter bump (used by R12, R17)
                         0x181270b30  Forced shock-NG step counter at score-update (R12) — mod #2 timing stats
                         0x181270b50  Cursor::move replacement for SD-cab option-cursor behavior (R22) — mod #14, not portable
                         0x181270b88  Flare→Clear-Lamps banner switch (R21) — mod #15
                         0x181270bb0  read "flare" operator setting → BSS byte 0x1802c2199
```

Two BSS-style scratch regions are used:
- `0x1802c2199..0x1802c2214` — operator-menu mirror (force_event, freeplay, timing_windows label, pacemaker, sound/input/render/bomb offsets, threshold, flare-mode flag, step-counter buckets, etc.).
- `0x1811d0000..0x1811d4000` (16 KB inside the gap above `data`) — per-step ring buffer for 1024 steps × 8 B per step × 2 players, plus four 0x498-stride per-player stat blocks.

## Cross-version compatibility

Every AOB anchor below was verified against `gamemdx_20260421.dll` (the most recent supported version at time of writing). Each pattern resolves to exactly one match in 20260421 unless otherwise noted. Where the pattern hits multiple sites, an "ordinal" or wider-context anchor is given.

The patterns avoid wildcarding things that change across versions on principle — RIP-relative displacements are always wildcarded (`?? ?? ?? ??`), call/jmp displacements always wildcarded, but opcodes and ModR/M bytes are always fixed.

## Mod-by-mod analysis

Numbering matches the README. For each mod: **what binary edits exist**, **what each edit does**, and **how to port it AOB-style**.

---

### 1. Change Timing Windows

**Operator-menu glue.** The README says timing-window selection (Default, 12 ms, 8/12, 9/12, 10, 17/23 ITG) is read from operator menu `GAME OPTIONS → TIMING WINDOWS`.

The actual binary edits that implement this mod are buried inside the per-state-transition handler (R7+R8) and helper `0x181270350`:

- `0x181270350` reads operator key `"timing_windows"` (string at `0x1802da505`), clamps to `[0..5]`, and `rep movsb`s 7 bytes from a labels table at `0x1802da6c0` into BSS slot `0x1802c2214`, then 0x18 bytes from a paired ranges table at `0x1802da5e8` into BSS slot `0x18033c710`. These two BSS writes are what the rest of the game reads when the timing-window judgment thresholds are evaluated.

- The label table at `0x1802da6c0` contains the seven ASCII labels: `"\0"`, `"17ms"`, `"12ms"`, `"8/12ms"`, `"9/12ms"`, `"10ms"` (each 7-byte slot, NUL-padded — entry 0 is the unused empty slot, the other 5 plus 17ms make up the README's 6 selectable choices including default).
- The range table at `0x1802da5e8` is `0x18` bytes per entry × 6 entries: signed `int32` pairs `(min_ms, max_ms)` for marvelous/perfect/great judgment thresholds.

**Port strategy:** This mod doesn't need a binary patch — it overwrites the BSS slot `0x18033c710..0x18033c728` (the 0x18-byte block holding the active marvelous/perfect/great judgment thresholds in milliseconds). The hook DLL can write directly to those addresses to switch timing windows. The slot is also accessible via the existing judge_hook service since the same field is consumed in `judgeNotes`.

**AOB anchor for the threshold table:** The threshold writes happen inside hook code and are derivable as RIP-relative offsets from a unique anchor. Recommended approach: hook the same point R7 hooks (see Force Event Mode below) and mirror the BSS slot to be in sync with config.

---

### 2. Timing Statistics During Gameplay

**Replaces the "EVENT MODE" / "FREE PLAY" credit-banner text** with per-step ms-error statistics, and adds a per-frame Current/Max/Mean/AbsMean display, plus a low-corner Ms Error display per player.

Patches that implement this mod:

| File offset | VA | What changed | Why |
|---|---|---|---|
| `0x000032d5` (R2) | `0x180003ed5` | `ff 15 dd 58 2b 00` (call qword [rip+...] — `Ordinal_381` log call) → `e9 86 4b 2b 00 90` (jmp `0x1802b8a60` + nop) | DllMain entry redirect to the loader (Cave 1 entry). Once the loader returns it falls through to the original log call's continuation — the log call's emission is preserved by the loader. **All other mods are downstream of this one being installed.** |
| `0x0000872e` (R3) | `0x18000932e` | LEA RDX/RCX disp32 changes: `0x1802bc100` → `0x1802da698`, `0x1802bc100`+4 → `0x1802da69c` | Within `FUN_180009220` (the credit/PASELI banner renderer), redirects the "FREE PLAY" / "EVENT MODE" string-table base. Stock points at .rdata's static array (`0x1802bc100`); mod points at the modder's own array at `0x1802da698`, which the per-frame loop overwrites with stat strings. |
| `0x00008892` (R4) | `0x180009492` | `fa` → `f5` | Single-byte tweak: changes the magnitude of a `JL` displacement inside the same banner renderer. Stock `JL +(-6)` / mod `JL +(-11)`. Effect: skips one extra prior instruction, matching the new flow. |
| `0x0034b5eb` (R37) | `0x18034c5eb` | ASCII `'1'` → `'2'` inside the format string `"%0.1lf"` → `"%0.2lf"` | Increases the timing-statistics display precision from 1 decimal to 2 decimals. The format-string slot is at `0x18034c5e8`. |
| `0x0005aea0` (R12) | `0x18005baa0` | `mov r9d, r10d; mov r8d, r15d` (6 bytes) → `call 0x181270b30` + nop | Inside `FUN_18005b6c0` (`judgeNotes`), in the **shock-NG miss path** (where the player misses a shock arrow that should fail them). Trampoline `0x181270b30` calls `0x181270a80(rcx=7)` to bump the per-judgment counter for judgment value 7 (= shock-NG) in the modder's per-player stat buffer at `[r14+0x84]`. Then restores the original two `mov` instructions. This contributes to the on-screen "EX: -X" / "Miss: X" stat counters during gameplay. Confirmed via Konami's own log call in the surrounding stock decompile: `Ordinal_382("...judgeNotes", "shock ng : pressedDir=%d, ...")`. |

The actual rendering loop (which writes "Max: Xms", "Abs(μ): Xms", "μ: Xms" into the BSS slots that R3 redirected to) lives inside the trampolines installed by R17 (the per-judgment commit hook) and the per-frame state-update at R8. It writes 7-byte ASCII slots into `0x1802da698..0x1802da6e0` from accumulated step data in the BSS ring buffer at `0x1811d0000`.

**Port strategy:**

- This codebase already has a widget system (`services/widget_renderer`) that can draw native `kt::BmpfontSimpleString` text widgets in the credit/banner area without overwriting the game's own banner. *Don't* port the banner-overwriting trick; instead, create text widgets at the banner location, scoped visible during gameplay scenes (28..32 — the song-play scene IDs in `types/scenes.rs`).

- For the per-step error data feed: hook `judgeNotes` (already done in `services::judge_hook`) and accumulate per-step `ms_error`. The judge service exposes the ms-error to subscribed callbacks already, so a TimingStatsMod can subscribe and update widgets directly.

- For the bottom-corner "Current: Xms" indicator (added in rev 11/15/25), the same data feed works.

**AOB anchor (precision-format string):** The `"%0.1lf"` literal at `0x18034c5e8` is unique. Pattern: `25 30 2E 31 6C 66 00 00` — but this is ASCII string, which Ghidra's string search finds anyway. For runtime byte search use `25 30 2E 31 6C 66`. Verified unique in 20250805 stock and 20260421 stock.

---

### 3. Premium Free (MAX STAGE = ∞)

Lets the operator set MAX STAGE to "∞" so the game loops at stage 2 forever instead of advancing to the results/logout screen.

Patch:

| File offset | VA | What changed | Why |
|---|---|---|---|
| `0x0002f4f2` (R9) | `0x1800300f2` | `mov rcx, [rax]; inc dword [rcx+0xc]` (10 bytes) → `call 0x1802b8af0` + nop (6 bytes overlap) | Hooks the per-frame stage-counter increment. Trampoline `0x1802b8af0` reads the byte at `[rdx + 0x445ac4]` (the operator-menu MAX STAGE config slot); if value > 4 (the "∞" sentinel), zeroes the stage counter before re-incrementing — keeping the player on stage 1 forever. |

Note: The README mentions a "force logout button" (hold Start at results screen to save current gameplay mods). That mechanism is shared with mod #16 Quick Fail — see R19 in mod #16 below. Both behaviors are implemented by the same patch site.

**The operator-menu byte at `0x180445ac4`** is the `MAX STAGE` config mirror (file offset within `.data`; this is `data+0x1fac4`). The mod's loader writes to it from operator menu `GAME OPTIONS → MAX STAGE`.

**Port strategy:** This codebase has its own per-player config system. The Premium Free mod becomes:

1. Read mod-config / overlay-menu setting `premium_free: bool`.
2. Hook the stage-increment instruction at R9's site with a `retour` detour. When `premium_free` is true, write 0 to `[rcx+0xc]` before the increment.

**AOB anchor for stage increment site (R9):**
```
Pattern: 48 8B 08 FF 41 0C
            ^-- mov rcx, [rax]
                  ^-- inc dword [rcx+0xc]
```
Verified unique on 20250805 stock (`0x1800300f2`) and 20260421 (`0x180030595`). For wider context:
```
Pattern: FF 41 0C 45 33 C0 41 8D 50 68 48 8B 0D ?? ?? ?? ??
            ^-- inc dword [rcx+0xc]
                     ^-- xor r8d, r8d
                            ^-- lea edx, [r8+0x68]
                                       ^-- mov rcx, [rip+...]
```
Also unique on both versions; resolves to the same stage-counter site.

---

### 4. "Real Speed" Calculations Fixed

**The Real Speed formula in stock divides by Max BPM; this mod makes it use Core BPM, and also guards `logf(0)` against producing `-inf`/NaN.**

Patches in `FUN_180077a00` and the surrounding scroll-speed display function:

| File offset | VA | What changed | Why |
|---|---|---|---|
| `0x001c8cbc` (R24) | `0x1801c98bc` | `ja +5` → `jmp +0x64` (`eb 64`) | Instead of conditionally branching forward 5 bytes, unconditionally jumps into the embedded code cave at `0x1801c9922`. The cave is in the same function's int3-padded tail, so it doesn't disturb existing code. |
| `0x001c8cdb` (R25) | `0x1801c98db` | `divsd xmm0, qword [rcx]` → `divsd xmm0, xmm2` (1 byte: `01` → `c2`) | Changes the divisor source from "memory operand pointed to by rcx" (Max BPM) to "register xmm2" (which the cave loads with Core BPM). |
| `0x001c8d22` (R26) | `0x1801c9922` | int3 pad → `movsd xmm2, qword [rbx+0x88]; ja 0x1801c98c3; jmp 0x1801c98be` (12 bytes) | The embedded cave: load Core BPM into `xmm2`, then preserve the original `ja`/fall-through control flow. |
| `0x00076fea` (R16) | `0x180077bea` | `call 0x18126e000` → `call 0x1812703b0` (4-byte rel32 displacement change) | Inside the same scroll-speed display function, swaps the raw `logf` call (cave 2's bare implementation) for a guarded version that returns 0 on input == 0. This prevents the display from showing NaN before the song starts. |
| `0x00076fae` (R15) | `0x180077bae` | Single-byte `JMP rel8` displacement: `48` → `37` | Adjusts a forward jump to skip 0x37 bytes instead of 0x48 — lands at the now-modified `call 0x1812703b0` site instead of the unguarded `call 0x18126e000` site. |

> **CORRECTION (2026-09-01): the R15/R16 attribution above is wrong.**
> `0x180077bea` is NOT in any scroll-speed display function — it is the
> log10f call inside `NoteResultActor::onMessage` case 0x1036
> (`FUN_180077a00` on 20250805), i.e. the PACEMAKER readout's sign-slot
> computation. R16 guards a log10f that only ever receives |v| ≥ 1 (a
> no-op), and R15 rewrites the pacemaker ZERO branch's `LEA
> R13D,[RSI+1]; JMP +0x48` to jump into the log path with a stale XMM6 —
> which breaks the exact-0 `±0` render (the sign lands on the ones slot
> and overwrites the digit). The stock zero branch was never broken and
> needs no patch. Our port of R15/R16 (`logf_stub.rs`) was retired for
> this reason; only R24/R25/R26 constitute the Real Speed fix. Full
> analysis: `docs/pacemaker_display_research.md` (resolved §).

The `+0x88` offset for Core BPM is on the `ChartData` struct; in this codebase's terminology that's the `chart_metadata` / `ssq_chunk_metadata` row-block. Stock uses `[rcx]`, where `rcx` is loaded from a different struct slot for Max BPM display.

**Port strategy:**

- Hook the scroll-speed display formula function at the same site (anchor below). The function takes a chart metadata pointer; replace the read of "Max BPM" with "Core BPM" before the `logf` call.
- Guard the `logf` call with a 0-check: if input is 0, return 0; otherwise call the original `logf`. (Or equivalently, multiply the result by `step(input, 0)`.)

**AOB anchor for R24-R26 site (Real Speed BPM divisor):**
```
Pattern: F2 0F 5E 01 48 8D 4C 24 40 0F 2F C8 ...
            ^-- divsd xmm0, [rcx]
```
A more discriminating pattern that pins to this exact divisor:
```
Pattern: 0F 28 C8 0F 2F C8 76 ?? F2 0F 5E 01 48 8D 4C 24 40
                                  ^-- divsd xmm0, [rcx]
```
Verified unique on 20250805. Pattern `F2 0F 5E 01 48 8D 4C 24 40` — verified unique on 20260421 at `0x1801df948`.

**AOB anchor for R16 site (logf-guard):** The `call 0x18126e000` site's preceding instructions are uniquely:
```
Pattern: 0F 28 C7 E8 ?? ?? ?? ?? F3 0F 58 C6
            ^-- movaps xmm0, xmm7
                  ^-- call (logf in cave 2)
                                 ^-- addss xmm0, xmm6
```
Verified unique on 20250805 stock (`0x180077be6`) and 20260421 (`0x18007bc56`). The `call` is into the math cave at the `text` (no-dot) section's first function; resolves to `0x18126e000` in 20250805 and `0x1812a7bc0` in 20260421 — same function (logf), different VA due to section relocation.

---

### 5. Force Platinum Pass Features

Forces two byte fields on the `ddr::player::Work` struct to constants, regardless of network profile. Confirmed semantics from Konami's debug-string log calls visible in the surrounding decompile:

| File offset | VA | What changed | Why |
|---|---|---|---|
| `0x0001366e` (R5) | `0x18001426e` | `movzx edi, byte [rsi+r14+0x4abd38]` (9 bytes) → `mov edi, 1` (5 bytes) + 4 NOPs | Forces the byte at `Work + 0x1d` to 1. The very next instruction in the function calls `Ordinal_382("ddr::player::Work::SetPlatinumMember", "refid=%s, isSubscribed=%d", ..., 1)` — this is the **"isSubscribed" flag for Platinum Pass**. Setting it to 1 forces the player to be treated as an active Platinum subscriber. |
| `0x000136e2` (R6) | `0x1800142e2` | `movzx edi, byte [rsi+r14+0x4abd3a]` (9 bytes) → `mov edi, 0` (5 bytes) + 4 NOPs | Forces the byte at `Work + 0x1f` to 0. Konami's log call here is `Ordinal_382("ddr::player::Work::SetDispPopupPlatinumMemberDisable", "refid=%s, isDispPopupPlatinumMemberDisable=%d", ..., 0)` — this **suppresses the "Platinum Member subscription has expired" popup**. Forcing 0 keeps the popup hidden so the forced subscription doesn't trigger spurious end-of-membership UI. |

The byte at `Work + 0x1e` (the third in this set, `SetDispPopupPlatinumMemberEnable`) is left untouched — that's the "subscription is now active" popup, which the modder allows to fire normally.

**Port strategy:**

- Hook `FUN_1800140f0` mid-function (after the network-profile snapshot at `0x1804abd00..+0x60` has been copied to `Work`) and rewrite the two flag bytes:
  - `*(uint8_t *)(work + 0x1d) = 1` (force isSubscribed = true)
  - `*(uint8_t *)(work + 0x1f) = 0` (force isDispPopupPlatinumMemberDisable = false)
- Cleaner: hook the call to `FUN_1801d1860` (insert into option set) just after R5's site — when called with `0x4c1` (the Platinum-pass UI option), the call is gated by the `[+0x1d]` byte being non-zero. Forcing `[+0x1d]` = 1 makes that call always succeed.

**AOB anchor:** The `42 0F B6 BC 36` (REX.X+SIB-indexed `MOVZX EDI, byte [RSI+R14+disp32]`) is the byte sequence the compiler uses for reads from this profile-snapshot array. The full version-agnostic anchor wildcards both the disp32 (changes with `.data` layout) and the player-options struct size in the `add r8, imm32` (struct grew between versions):
```
Pattern: 42 0F B6 BC 36 ?? ?? ?? ?? 44 8B CF 4C 8B 03 49 81 C0 ?? ?? 00 00
            ^-- movzx edi, [rsi+r14+disp32]   (disp32 = profile-byte BSS offset)
                              ^-- mov r9d, edi
                                    ^-- mov r10, [rbx]
                                          ^-- add r8, imm32   (imm32 = PlayerOptions struct stride; 0x1704 in 20250805, 0x1724 in 20260421)
```
Verified to find **exactly 3 ordered matches** in both versions:
- 20250805 stock: `0x18001426e`, `0x18001429f`, `0x1800142e2`
- 20260421:        `0x1800140ad`, `0x1800140de`, `0x180014121`

The three matches correspond to consecutive byte-flag reads from the profile snapshot, written into adjacent fields `[rax+0x1d]`, `[rax+0x1e]`, `[rax+0x1f]` on the PlayerOptions struct. The pre-modded build patches **ordinal 1** (forces flag 1 = true → adds option 0x4c1) and **ordinal 3** (forces flag 0 = false → does NOT add option 0x4c2). Ordinal 2 is left untouched.

**Note:** The +1d/+1e/+1f field offsets and the 0x4c1/0x4c2 set-element values are robust across versions (they're game-protocol values), but both the `disp32` operand on the MOVZX and the `imm32` operand on the `add r8, ...` (PlayerOptions struct stride) change. Always derive these from the matched instruction at runtime — never hardcode them.

---

### 6. Force Event Mode

**Not a portable feature for this codebase.** The README describes mod #6 as "operator-menu setting GAME OPTIONS → FORCE EVENT MODE", but the binary actually implements it as **hold-Start during boot to force Event Mode entry** — and the only reason it exists in the pre-modded build is to guarantee that the credit/PASELI banner shows the "EVENT MODE" string at all times so that mod #2's timing-stats overwrite has a stable string slot to write into.

Since this codebase has its own widget system (`services/widget_renderer`) that doesn't depend on overwriting Konami's banner-string slot, **mod #6 is not needed**.

Patches (documented for completeness):

| File offset | VA | What changed | Why |
|---|---|---|---|
| `0x0002d537` (R7) | `0x18002e137` | int3 padding before the function → `lea rbp, [rax-0x268]; jmp +0xd` (8 bytes); first 7 bytes of the original `FUN_18002e140` prologue → `jmp -0x16; call 0x1802b8bd8` (8 bytes) | Splices a one-time call to `0x1802b8bd8` between the function's first instruction and its second. The cave checks `cmp ebx, 0x1e; jne done; call 0x181270248 (Start-held check); je done; reset modder BSS; rewrite ebx from 0x1e to 0x19` — i.e., when the state-machine is about to enter state 0x1e (boot/title-screen) AND the user is holding Start, rewrite the state to 0x19 (Event Mode entry). |
| `0x0002d640` (R8) | `0x18002e240` | `mov ecx, [rdx+rax*4+0x30d90]; add rcx, rdx` (10 bytes) → `call 0x181270300; call 0x1802b8b10` (10 bytes) | The function uses a jump table at `0x180030d90` for per-state dispatch. The mod replaces the table-load+adjust with two calls: (1) `0x181270300` polls all operator-menu values into the modder's BSS mirror **only when entering state 0** (the boot state); (2) `0x1802b8b10` clears modder per-frame stat state on certain transitions (state IDs in mask `0x12181`) and then performs the original `mov ecx, [rdx+rax*4+0x30d90]; add rcx, rdx` before returning. The subsequent `jmp rcx` at `0x18002e24a` is unchanged. |

The cave at `0x181270300` is the **per-state config-refresh dispatcher** (only fires on state 0). It calls `0x181270350` (timing windows), `0x1812704c0` (force_event/freeplay), `0x181270410` (sound/input/render/bomb offsets), `0x181270478` (pacemaker mode), `0x181270228` (pacemaker threshold), `0x181270bb0` (flare mode). This is the single hook through which all operator-menu values get mirrored into the BSS at `0x1802c2199..0x1802c221c`.

**This dispatcher is the "operator's menu mods are now applied on boot" mechanism** mentioned in the README's rev. 10/12/25 note. In this codebase, equivalent behavior is provided by the existing `mod-config.json` reader and `services/custom_options` framework — no patching needed.

**Port strategy:** **Skip** — neither R7 (Force Event Mode) nor R8 (operator-menu mirror infrastructure) are needed. This codebase reads its own config and exposes per-mod settings via the overlay menu instead of repurposing Konami's operator menu.

**AOB anchor for R8 site (jump-table dispatch), if ever needed:**
```
Pattern: 8B 8C 82 ?? ?? 03 00 48 03 CA FF E1
            ^-- mov ecx, [rdx+rax*4+disp32]
                            ^-- add rcx, rdx
                                  ^-- jmp rcx
```
Verified unique on 20250805 and unique on 20260421 (`0x18002e574`).

---

### 7. Pacemaker → MsError Switch

Replaces the in-game Target Score pacemaker with a per-step ms-error display when the player selects a configurable Target Score slot (e.g., MACHINE #1).

The mod tracks two BSS bytes:
- `0x1802c219a` — the operator-menu-configured "pacemaker mode" (which target-score slot triggers the swap).
- `0x1802c219b` — the operator-menu-configured ms-error threshold (white-pacemaker zone).

The core comparison is at `0x181270700`:
```c
if (current_player.pacemaker_target [+0x1308] == mod.pacemaker_mode [BSS 0x1802c219a]) {
    // Pacemaker→MS swap is active for this player
}
```

Patches in `FUN_180077a00` (the score/pacemaker render command handler):

| File offset | VA | What changed | Why |
|---|---|---|---|
| `0x00076f88` (R14) | `0x180077b88` | `mov rax, [rcx]; test esi, esi` (5 bytes) → `call 0x181270490` (5 bytes) | Trampoline `0x181270490` performs the original `mov rax, [rcx]`, calls `0x181270700` to check if the pacemaker→MS swap should activate for this player. If yes AND the abs(ms-error) < threshold, sets ZF=1 (so the subsequent `jne 0x180077baf` falls through to the white-pacemaker-color path). Otherwise behaves like the original `test esi, esi`. |
| `0x00076f36` (R13) | `0x180077b36` | `mov rdx, [rdi+0xb0]` (7 bytes) → `call 0x1812706e0` + 2 nops | Trampoline `0x1812706e0` calls `0x181270700` to check pacemaker-swap-active for this player. If yes, overwrites the pacemaker-formatter input (`*(int*)[r14+8]`) with the player's most-recent ms-error read from the BSS step-buffer at `0x2c219e + player_idx*0xe`. Then performs the original `mov rdx, [rdi+0xb0]` and returns. |

(R12 at `0x18005baa0` was originally listed here, but it's actually a **mod #2 timing-stats** patch that bumps the shock-NG counter — see mod #2 section above.)

**Port strategy:**

- Use this codebase's `services/judge_hook` to capture per-step ms-error.
- Hook the score-message handler at R13's site (or equivalent in this codebase's existing dispatchers). When the `pacemaker_to_mserror` config is active for the current player's selected target-score slot, override the formatter input with the most-recent ms-error.
- Hook R14's site to set the white-pacemaker-color zone when ms-error < threshold.

**AOB anchor for R14 site:**

The original 11-byte pattern with the JNE displacement `75 22` does NOT match 20260421 — the slow-path basic-block size shrank by 2 bytes, so the JNE displacement is `75 20` there. The version-agnostic pattern wildcards the JNE displacement:
```
Pattern: 48 8B 01 85 F6 75 ?? F3 0F 10 0D
            ^-- mov rax, [rcx]
                  ^-- test esi, esi
                       ^-- jne short ?
                              ^-- movss xmm1, [rip+...]
```
Verified unique on 20250805 stock (`0x180077b88`) and 20260421 (`0x18007bbf8`). The bare 5-byte prefix `48 8B 01 85 F6` is also unique on both versions if you want a tighter anchor — but pin to the prefix-only version since `mov rax, [rcx]; test esi, esi` is a generic compiler idiom that could shift.

---

### 8. Internal Offset Adjustments (sound/input/render/bomb)

Lets operators set frame-level offsets for sound, input, render, and bomb timing without per-version hex patching.

Implementation lives entirely in the runtime cave. The trampoline at `0x181270410`:

```asm
0x181270410:  lea rdi, [rbx + 0x2da51e]      ; rdi = "sound\0input\0render\0..."
0x181270417:  lea rsi, [rbx + 0x1344b]       ; rsi = (signed int32) range table
0x18127041e:  xor ecx, ecx                   ; idx = 0
0x181270420:  loop:
              push rcx
              mov rcx, rdi                   ; key
              mov edx, [rsi]                 ; default = (range_lo)
              call 0x1812703c8               ; read int operator value with bounds check
              pop rcx
              call 0x181270448               ; write into game's frame_offset[idx]
              add rdi, 7                     ; next key (7-byte slot)
              add rsi, 7                     ; next range
              inc ecx
              cmp cl, 4
              jl loop
              ret
```

The 4 operator-menu key strings are at `0x1802da51e` (`"sound\0input\0render\0bomb\0"`) padded to 7-byte slots. The range table at `0x1804134b` (in stock) defines (-500,500) for sound/input/render and (-50,50) for bomb. The trampoline writes into the game's `frame_offset` array at a path resolved through:

```asm
0x181270448:  mov rax, [rbx + 0x6b5b00]      ; global pointer #1
              mov rax, [rax + 0x28]          ; ->offset 0x28
              mov rax, [rax + 8]
              mov rax, [rax + 0x10]
              mov rax, [rax + 0x10]
              lea rcx, [rcx + rcx*2]
              shl rcx, 5                     ; rcx *= 0x60
              mov [rax + rcx + 0x1c], edx    ; write offset value
              ret
```

So `frame_offset` is at `*(*(*(*(*(g_thing+0x28)+8)+0x10)+0x10)+idx*0x60+0x1c)`.

**Port strategy:**

The frame_offset array exists in stock — this mod is purely "expose the operator-menu values to it". In this codebase, define four config knobs (sound_offset, input_offset, render_offset, bomb_offset) and on each config change, walk the same five-deep pointer chain to find the array and write the value.

**AOB anchor for the frame_offset array global:** The mod's site uses the address `0x1806b5b00` directly. Stock 20250805 references this address inside the trampoline's `mov rax, [rbx + 0x6b5b00]` — this is a global game pointer derivable from many call sites in the stock binary. To find it without baking in the offset, scan for any function that does `mov rax, [rip+disp]; mov rax, [rax+0x28]; mov rax, [rax+8]; mov rax, [rax+0x10]; mov rax, [rax+0x10]; ... mov [rax+rcx+0x1c], ?` — the unique 5-deep chain is the structural anchor.

A simpler approach: the existing `judge_hook` service already finds `judgeNotes` and the chart-runtime context. The same context likely has the frame_offset array; trace from there.

---

### 9. Skip Results Screen By Holding Start

After failing a song, holding Start at the "Stage Failed" screen skips the results screen and returns to song select.

This is implemented by the **same R19 patch** described under mod #16 (Quick Fail) — the case-0x1c state-transition hijack. Holding Start during the post-stage state machine forces the alternate state pair (`0x21`/`0x38` instead of `0x20`/`0x37`), which is the failed-out / quit-out scene transition that goes back to song select instead of through the results screen.

The same hook also implements the **"force logout" UX gesture** mentioned under mod #3 Premium Free.

**Port strategy:**

- Use this codebase's `services/input_manager` to detect held-Start during gameplay scene IDs (28..32) and during the Stage-Failed screen scene.
- When detected, trigger a scene redirect via `services/scene_manager` to skip past the results screen.
- The save side-effect (when applicable for Premium Free) is automatic — the game saves on scene transition out of gameplay.

**AOB anchor:** Same as mod #16 Quick Fail — see §16 below.

---

### 10. Special SSQ Importing

A complete rewrite of `FUN_18019e8d0` (`get_ssq_path` — given a song code and difficulty index, returns the path to the SSQ file, with `_<digit>` suffix for special charts).

Stock implementation (~933 bytes): hardcoded chains of `if (strcmp(songcode, "acef") == 0 && diff_idx == 4) suffix='5'; else if ...` for each special song. Adding a new song required releasing a new game DLL.

Mod implementation (~150 bytes): walks a data table starting at `0x1802da50e` (well, actually using the stock `"acef"` string at `0x18035f508` and the modder's table follows). Each entry is 8 bytes: 5-char song code (NUL-padded if shorter), 3 bytes packed (4-bit nibbles) — high nibble of byte N is the SSQ suffix for difficulty 2N, low nibble for 2N+1. A 0 nibble means "use default SSQ".

Patches:

| File offset | VA | What changed | Why |
|---|---|---|---|
| `0x0019dcda` (R23) | `0x18019e8da` | 933 bytes of stock string-compare chain → 933 bytes of compact table-walk | Complete function rewrite. |
| `0x0035e504` (R38) | `0x18035f504` | Stock had ASCII song-code constants `"acef\0\0\0\0chao2\0..."` (148 bytes) used as immediates inside the chain → all zeros | The constants table is no longer referenced because the new function walks a different table at `0x18035f508`. |

The relevant table starts at `0x18035f508` and continues for 18 entries (matching the README's pre-existing modded songs). The entries are 5-byte song codes followed by the bytes `00 00 00`. The mod at `0x1802da400` keeps the same song-codes list at `0x18035f508` (zeroed out since no longer needed) but writes the NEW 8-byte-per-entry table somewhere else — embedded inside the new function body at `0x18019e8da` plus some offset. The function just walks it inline.

**Port strategy:**

- This mod can be implemented entirely as a Rust hook on `get_ssq_path`. Take a config-driven `Vec<SpecialSsqEntry>` from `mod-config.json` and override the SSQ filename on match.
- The hook is at `FUN_18019e8d0` — anchor the function entry by its prologue.

**AOB anchor for `FUN_18019e8d0`:**
```
Pattern: 4C 8B D1 48 8D 3D ?? ?? ?? ?? 48 8B F2 B9 05 00 00 00 F3 A6
            ^-- mov r10, rcx (param_1 saved)
                  ^-- lea rdi, "acef" string
                                 ^-- mov rsi, rdx (param_2)
                                       ^-- mov ecx, 5
                                                  ^-- repe cmpsb
```
Verified unique on 20250805 and unique on 20260421 (`0x1801b2d6a`).

---

### 11. "NOW LOADING" Screen Optimization

Speeds up the song-metadata load during the boot "NOW LOADING" screen by ~90% by draining the entire ready queue per frame instead of one item per frame. Equivalent to this codebase's existing **Fast Bootup** mod.

Implementation lives in two patches inside `FUN_180032200` (which is `CheckStepDataActor::onUpdate`, the per-frame song-data loader; the same function this codebase's Fast Bootup hooks via `vtable[6]`):

| File offset | VA | What changed | Why |
|---|---|---|---|
| `0x00031eda` (R11) | `0x180032ada` | `ret` (function epilogue) → `jmp 0x181270898` | Replaces the function's terminal `ret` with a jump into the cave-3 stub. The stub peeks the next entry in the step-data queue (`r12`/`rbx` is the actor; `[rbx+0x58]` is the index, `[rbx+0x88]` is the 12-byte-stride array, the global table at `[rip-0xbbabe6]` resolves to the entries with `0x40` stride and status byte at `+0x20`). If the next entry's status is one of `{0, 5, 6, 8}` (= ready), it tail-calls back into `FUN_180032200` to process the next item in the same frame. Otherwise, returns normally. The `{0, 5, 6, 8}` status set is identical to the `READY_STATUSES` used by this codebase's Fast Bootup mod. |
| `0x00031d78` (R10) | `0x180032978` | `add rdx, rax; cmp rdx, rdi` (6 bytes) → `call 0x1802b8ba8` + nop | Inside the same function, just before a `ja 0x180032a7f` branch that would normally cap the per-frame iteration. Trampoline at `0x1802b8ba8` does the original `add rdx, rax`, then calls `0x181270248` (Start-held check on either player). If Start IS held, replaces the cmp with `cmp edx, edx` (forcing ZF=1 → the subsequent `ja` is NOT taken → the cap-bypass branch runs). This implements **the README's "Optional Skip NOW LOADING on boot"**: hold Start during the loading screen to drain the queue maximum-fast. |

**Two-part mod:** R11 is the *automatic* optimization (always active, no input needed). R10 is the *interactive boost* (Start-held = skip cap = even faster). Together they implement both mod #11 and the optional Skip-NOW-LOADING-on-boot feature that the README describes as replacing a separate skip-loading DLL.

**Port strategy:**

- The existing `Fast Bootup` mod in this codebase already hooks `CheckStepDataActor::vtable[6]` and drains items per frame using the same status set. Mod #11 is therefore **already implemented** in this codebase. No port needed.
- For the optional Start-held boost, extend Fast Bootup to also process more aggressively while Start is held. The codebase's `services/input_manager` already has `arkMDXGetStart` integration — the gating logic is straightforward.

**AOB anchor for R11 site (function epilogue ret):**

The function entry of `FUN_180032200` is anchored via `CheckStepDataActor` RTTI vtable[6] (this codebase already does this — see `core/signatures.rs::find_check_step_data_actor`). Once the function entry is known, scan forward for the standard 5-callee-saved epilogue `5F 41 5E 41 5D 41 5C 5D C3 CC CC CC CC` and patch the `c3` ret with `e9 ?? ?? ?? ??`.

**AOB anchor for R10 site (cap-bypass):**
```
Pattern: 48 8B C2 48 C1 E8 3F 48 03 D0 48 3B D7 0F 87
            ^-- mov rax, rdx       (within FUN_180032200)
                  ^-- shr rax, 0x3f
                            ^-- add rdx, rax
                                  ^-- cmp rdx, rdi
                                        ^-- ja  0x180032a7f
```
Verified unique on 20250805 stock (`0x180032971`) and 20260421 (`0x180032be1`). R10 patch site is anchor + 7 (the `add rdx, rax`).

---

### 12. Export Step Data (CSV)

Writes per-song step-by-step error data to `/dev/nvram/<date>_<time>_<songcode>_<player>.csv`.

Implementation lives entirely in cave 3:

- `0x1812702a0`: per-step ring-buffer commit. Calculates the ring index from `r12` (player ID) — `r13 + 0x11d0000 + r12*0x2000` is the per-player ring base. Reads the current step's expected timestamp from `[rax+0xb0]+ (some_index*0x40) + 8` and the actual hit ms-error, packs as two `int32`s into the ring slot.

- `0x181270500` (R17 trampoline): on every judgment opcode (called from `case 0x1028..0x102f` of the score-render function), records the step into the ring buffer and updates per-judgment counter and abs/sum/max/mean stats.

- `0x181270920` (called from R19/end-of-stage save): on stage exit, opens `"/dev/nvram/<date>_<time>_<songcode>_<player>.csv"`, writes header `"Expected,Actual,Delta (Ms Error)\r\n"` (string at `0x1802da410`), then dumps the ring buffer with row format `"%d,%d,%d\r\n"` (string at `0x1802da400`). The format string at `0x1802da400` and the path template at `0x1802da43c` (`"/dev/nvram/%s_%uP_%u-%u-%u_%02u%02u.csv"`) are pre-baked.

Patches that wire this in:

| File offset | VA | What changed | Why |
|---|---|---|---|
| `0x00076f36` (R13) | `0x180077b36` | `mov rdx, [rdi+0xb0]` (7 bytes) → `call 0x1812706e0` + 2 nops | Trampoline at `0x1812706e0` checks pacemaker→ms-error setting; if active, overwrites `param_3[1]` (score delta) with the step's ms-error read from the ring buffer. Then restores the original `mov rdx, [rdi+0xb0]`. |
| `0x0007716f` (R17) | `0x180077d6f` | `mov [rdi+0x98], eax` (6 bytes) → `call 0x181270500` + nop | Trampoline performs the original write (judgment value) and additionally calls `0x181270a80` (per-judgment counter bump) and `0x1812702a0` (ring-buffer commit) when the judgment is one that should be exported (M/P/G/Goo/Boo/Miss). |

**Port strategy:**

- The `judge_hook` service already exposes per-step ms-error to subscribers. Build a `StepDataExportMod` that subscribes, accumulates per-song, and on song-end writes a CSV file (this codebase has filesystem access via standard Rust + `data_mods/` config).
- For the per-frame stats (Mod #2 above), the same subscription drives the widget update.

**AOB anchor for R13:**
```
Pattern: 48 8B 97 B0 00 00 00
            ^-- mov rdx, [rdi+0xb0]
```
Verified unique on 20250805 stock (`0x180077b36`) and 20260421 (`0x18007bba6`). Note the subsequent `movd xmm0, esi` is encoded with `0F 6E C6` in 20250805 but `66 0F 6E C6` in 20260421 (with operand-size prefix) — so do NOT extend the anchor past byte 7 if you want cross-version compatibility.

**AOB anchor for R17:**
```
Pattern: 89 87 98 00 00 00 83 F9 06
            ^-- mov [rdi+0x98], eax
                          ^-- cmp ecx, 6
```
Verified unique on 20250805 stock (`0x180077d6f`) and 20260421 (`0x18007bddf`).

---

### 13. Updated Speed Toggle (0.05× / 0.50× quantum)

Stock speed toggle moves in 0.25× steps. Mod moves in 0.05× steps, or 0.50× when Start is held.

Patches:

| File offset | VA | What changed | Why |
|---|---|---|---|
| `0x001c9547` (R27) | `0x1801ca147` | `add edx, 0x19; lea r8, [rip+...]; lea rcx, [rsp+0x30]` (15 bytes) → `push 0; pop rcx; lea r8, [rip+...]; call 0x181270a58` (15 bytes) | "Speed up" path: instead of adding 0x19 (= 25, i.e. +0.25× in some unit), passes direction=0 (positive) to the modder's quantum dispatcher. |
| `0x001c9594` (R28) | `0x1801ca194` | `add edx, -0x19; lea r8, ...; lea rcx, ...` → `push 1; pop rcx; lea r8, ...; call 0x181270a58` | "Speed down" path: passes direction=1 (negate result). |

Cave stub `0x181270a58`:
- Default = `0x32` (= 50, i.e. ±0.50×).
- Calls `0x181270260` (`arkMDXGetStart` for ONE player slot) with the **OTHER player's** index (`xor ecx, 1` flips player_idx), since the player triggering speed-toggle naturally has Start held themselves.
- If the other player's Start is held, keep the default `0x32`. Otherwise use `5` (= ±0.05×).
- Negate based on direction param, then `add edx, eax` to apply.

**Port strategy:**

- Hook the speed-toggle function at the same site (anchor the unique pre-pattern: `83 C2 19 4C 8D 05` for the +0x19 site, `83 C2 E7 4C 8D 05` for the -0x19 site).
- Replace the +/- 25 with a config-driven quantum, optionally larger when start is held.

**AOB anchor for R27 (speed-up site):**
```
Pattern: 83 C2 19 4C 8D 05 ?? ?? ?? ?? 48 8D 4C 24 30
            ^-- add edx, 0x19         (the "+25" speed-up step)
                     ^-- lea r8, [rip+...]   (label string ptr)
                                       ^-- lea rcx, [rsp+0x30]
```
Verified unique on 20250805 stock (`0x1801ca147`) and 20260421 (`0x1801e01b7`).

**AOB anchor for R28 (speed-down site):**
```
Pattern: 83 C2 E7 4C 8D 05 ?? ?? ?? ?? 48 8D 4C 24 30
            ^-- add edx, -0x19        (the "-25" speed-down step)
```
Verified unique on 20250805 stock (`0x1801ca194`) and 20260421 (`0x1801e0204`).

---

### 14. Modified Options Menu Controls

**Not applicable to this codebase.** Mod #14 is a quality-of-life enhancement specifically for **SD cabinets** — repurposes the Start button (which on an SD cab has limited reach) to move the option-row cursor down instead of advancing tabs. This codebase targets HD cabinets exclusively, so this mod isn't needed.

The R22 patch at `0x18016abd4` (file `0x169fd4`) modifies the option-row cursor function `FUN_18016abd0` (`Cursor::move`) to redirect through cave stub `0x181270b50`, which inserts a call to `FUN_18004b6d0` (an alternative cursor-advance routine with different bounds/wraparound behavior) for normal-call cases (4th param `r8 == null`). Documented here for completeness but **not portable as a feature** for this codebase.

**Port strategy:** Skip.

---

### 15. Replace Flare Clear Banner With Clear/Combo Lamps

When enabled, the flare-clear banner shows clear-lamp colors (MFC = white FLARE EX, PFC = gold FLARE IX, etc.) instead of stock flare grade.

Patch:

| File offset | VA | What changed | Why |
|---|---|---|---|
| `0x001453a2` (R21) | `0x180145fa2` | `call 0x1800f2700; ... cmovne esi, eax` (~10 bytes) → `call 0x181270b88; ... mov esi, eax; nop` | Inside `FUN_1801452e0` (results-screen banner setup). Stock calls `FUN_1800f2700` (get flare-clear level) and conditionally accepts the result. Mod calls a wrapper that branches on operator-menu `"flare"` (cached at `0x1802c2199`); when set, calls `FUN_1800f3c00` (get clear-lamp level) and remaps via a 32-byte table at `0x18034c5e0+...`. |

**Port strategy:**

- Hook the flare-banner setup function. Read mod config; if `flare_to_clear_lamps` is on, substitute the clear-lamp value for the flare-grade value.
- The two source functions (`FUN_1800f2700` and `FUN_1800f3c00`) are stable across versions — anchor them by xrefs from the results-screen function.

**AOB anchor:** `48 8B 11 83 3A 01 0F 45 F0` — `mov rdx, [rcx]; cmp [rdx], 1; cmovne esi, eax`. Verified unique in 20260421 (`0x18015a9ad`).

---

### 16. Quick Fail (hold Start during gameplay)

Hold Start on either side to instantly fail both players, transitioning to the fail/results scene immediately.

Patch:

| File offset | VA | What changed | Why |
|---|---|---|---|
| `0x000b4ce1` (R19) | `0x1800b58e1` | `mov ecx, 0x20` (5 bytes) → `jmp 0x1802b8b80` (5 bytes) | Inside `FUN_1800b3a80` (player-select / stage-result state machine), case `0x1c`. Trampoline at `0x1802b8b80` performs two operations: (1) calls `0x1812708e8` which checks the operator-menu "save" key and, if set, dumps Step Export CSVs for both players (mod #12 logic — see below); (2) calls `0x181270248` (Start-held check on either player). If Start IS held, jumps to `0x1800b5883` which selects the alternate state pair `0x21`/`0x38` (the failed/quit-out scene transition) instead of the original `0x20`/`0x37`. Otherwise restores the original `mov ecx, 0x20` and continues normally. |

The Start-held Quick Fail and the operator-menu-driven Step Export are **dual-purposed in the same trampoline** because both fire at the same state-transition point (case 0x1c is the post-stage waiting state).

This is also the **"force logout button"** described under mod #3 Premium Free — same Start-held mechanism, same patch site. When Premium Free's MAX STAGE = ∞ keeps the player looping, this gives them a way to force a logout/save-and-exit by holding Start at the natural-stage-end transition.

State-pair semantics:
- `0x20` / `0x37` — normal post-stage transition (default; `0x37` when "save" flag is set)
- `0x21` / `0x38` — failed/quit-out post-stage transition (default; `0x38` when "save" flag is set)

In 20260421 the state IDs shifted to `0x21`/`0x39` and `0x22`/`0x3a` — the structural pattern is preserved but the immediates changed.

**Port strategy:**

- Hook the same site as R19 to monitor the case-0x1c transition for held-Start input. When held, force the alternate state pair.
- Cleaner approach: detect held-Start during gameplay scene IDs (28..32, the song-play range) via this codebase's `services/input_manager` callback system, then trigger the fail path through the existing scene-transition hook (`services/scene_manager`). No state-machine hijack required.
- The scene-transition is what FUN_1802056d0 dispatches; this codebase already hooks `TransitionSequence::createNextSequence` for similar redirects.

**AOB anchor for R19 site:**

The original 12-byte pattern with the literal state IDs (`B0 01 B9 20 00 00 00 BA 37 00 00 00`) does NOT match 20260421 — the state IDs shifted from `0x20`/`0x37` to `0x21`/`0x39`. The version-agnostic pattern wildcards both immediates:
```
Pattern: 32 C0 EB 02 B0 01 B9 ?? 00 00 00 BA ?? 00 00 00 84 C0 0F 45 CA
            ^-- xor al, al        (flag = false branch — "save" not set)
                  ^-- jmp +2
                       ^-- mov al, 1   (flag = true branch — "save" set)
                              ^-- mov ecx, <state_when_flag_clear>  (0x20 in 20250805, 0x21 in 20260421)
                                          ^-- mov edx, <state_when_flag_set>    (0x37 in 20250805, 0x39 in 20260421)
                                                          ^-- test al, al
                                                               ^-- cmovne ecx, edx
```
Verified unique on 20250805 stock (`0x1800b58db`) and 20260421 (`0x1800bf099`). The R19 hook site is the `mov ecx, imm32` instruction at anchor + 6 bytes.

---

### 17. Default Player Options/Filters

Sets new defaults for new profiles / Local Mode (Real Speed: 600, Music Folder: All, Filter Mode: Normal, Difficulty: Expert, Lane Visibility: 100%, Guideline: Off, Fast/Slow: On, Primary Pane: Detailed, etc.).

Implementation: pure data writes, no code patches. Stock initializer functions write hardcoded defaults to a per-player profile struct on profile creation. Mod replaces those constants in the `.rdata` initializer-template.

The exact patch sites are not in this diff — the README's rev. 2/28/26 says these affect "new profiles or Local Mode", which means they're written into a different DLL (likely `ess.dll` or by a separate config file) rather than `gamemdx.dll`. The byte-level diff against gamemdx.dll only shows 44 changes total, none of which match a "default options table" rewrite.

**Port strategy:**

- This codebase has a `WebUIOptions` and `CustomPlayerOptions` framework (`services/custom_options/`). Default values are config-driven via `mod-config.json`.
- Just expose the same defaults in `mod-config.json` — no game patching needed.

---

### Patches that didn't fit the user's "core functionality" filter

These are operator-menu plumbing and PE-structure changes — not portable as features:

| File offset | VA | Notes |
|---|---|---|
| `0x000001b4` (R0) | PE timestamp | Build date stamp — irrelevant. |
| `0x0000020c` (R1) | PE optional-header | Address-of-import-table or similar — supports the IAT extension. |
| `0x000abfc4` (R18) | `0x1800acbc4` | `e8 47 fb f6 ff` (call `FUN_18001c710`) → `c6 44 25 e7 01` (`mov byte [rbp-0x19], 1`). Inside `FUN_1800ac6a0` (network common-event-load state machine, case 5). Forces the network response to be `1` (= got response, advance to state 7) without actually polling the server. **This is offline-mode infrastructure**: it lets the operator-menu mod boot the game without network access. Not a user-facing mod. |
| `0x00112691` (R20) | `0x180113291` | `lea rdx, [rip+0x1aee6f]` → `lea rdx, [rip+0x1c7407]`. Redirects a `std::string::assign` from the static `.rdata` "S" string at `0x1802c2104` to the modder's relocatable copy at `0x1802da6a0`. Part of the Mod #2 banner-text override mechanism — once the widget-based replacement is in this codebase, this patch isn't needed. |
| `0x001d585e` (R29) | `0x1801d645e` | `lea rax, [rip+0xebca6]` → `lea rax, [rip+0x10423e]`. Redirects a default-sentinel return value in `std::map::find` (`FUN_1801d63f0`). Same banner-text override family. |
| `0x002b7e60` (R30) | `0x1802b8a60` | Cave 1 contents (loader). |
| `0x002c1190` (R31) | `0x1802c2190` | Cleared the `"PASELI: NOT AVAILABLE"` string to make room for the modder's BSS slot. |
| `0x002c1214` (R32) | `0x1802c2214` | Cleared the `"ONLINE"` string to make room for the BSS slot. The modder's loader now writes the timing-window label here. |
| `0x002d9400` (R33) | `0x1802da400` | Mod data table: CSV format strings, range tables. |
| `0x002d94d8` (R34) | `0x1802da4d8` | Mod data table: operator-menu key strings. |
| `0x002d95e8` (R35) | `0x1802da5e8` | Mod data table: timing-window range pairs (24×8 bytes = 6 entries × 4 thresholds). |
| `0x002d9700` (R36) | `0x1802da700` | Mod data: 0xE00 bytes — the runtime payload that gets `rep movsb`'d to `0x181270200`. |
| `0x00422bf8` (R39) | `0x180423bf8` | IAT entry table extension — moves an IAT entry pointer to make room for `VirtualProtect`. |
| `0x00422c98` (R40) | `0x180423c98` | More IAT extension. |
| `0x0042456c` (R41) | `0x18042556c` | Adds the ASCII string `"VirtualProtect"` and `"gameMain"` to the import-name table. |
| `0x0042bd70` (R42), `0x0047ca30` (R43) | `0x18042d770`, `0x18047e430` | Two `.data` 8-byte pointer-table entries redirected from `0x1802c2104` to `0x1802da69c` — extends the banner-text override across additional reference sites. |

## Recommended porting order

If implementing these as DDR World Universal Modpack mods, build them in this order (least dependent first):

**Already covered by this codebase / not needed:**

- **NOW LOADING Optimization (#11)** — already implemented as `Fast Bootup` (`mods/fast_bootup.rs`), which hooks `CheckStepDataActor::vtable[6]` (= `FUN_180032200`) and drains the queue per frame using the same `READY_STATUSES = {0, 5, 6, 8}` set. R10's hold-Start cap-bypass is purely a UX flourish — Fast Bootup already runs at full speed without it.
- **Force Event Mode (#6)** — implemented in the pre-modded build only as a banner-string hack to support mod #2's overwrite. This codebase has its own widget system, so the banner doesn't need to be repurposed. **Skip.**
- **Modified Options Menu Controls (#14)** — SD cabinet quality-of-life only; this codebase targets HD cabinets exclusively. **Skip.**
- **Default Player Options/Filters (#17)** — config-only, this codebase already supports defaults via `mod-config.json` and `services/custom_options`. **Already covered.**

**To port (least dependent first):**

1. **Updated Speed Toggle (#13)** — two hooks, purely additive UX. Replace the ±0x19 stride at the speed-up/down sites with ±0x05 (default) or ±0x32 (when the OTHER player's Start is held).
2. **Force Platinum Pass Features (#5)** — two byte writes after profile init: force `Work + 0x1d` (`isSubscribed`) to 1 and `Work + 0x1f` (`isDispPopupPlatinumMemberDisable`) to 0.
3. **"Real Speed" Calculation Fix (#4)** — one hook on the BPM-divisor instruction (use Core BPM instead of Max BPM).
4. **Replace Flare Clear Banner (#15)** — single hook on the banner setup function.
5. **Premium Free (#3)** — hook stage-counter increment.
6. **Internal Offset Adjustments (#8)** — write to frame_offset array on config change.
7. **Special SSQ Importing (#10)** — hook `get_ssq_path` and consult config-driven table.
8. **Quick Fail (#16) + Skip Results Screen (#9) + Premium Free force-logout (#3 sub-feature)** — these are all the same mechanic: detect held-Start during a gameplay or post-stage scene and trigger an early scene transition. Implement once via `services/input_manager` + `services/scene_manager`.
9. **Pacemaker → MsError Switch (#7)** — overlay step-error onto pacemaker via judge_hook subscription.
10. **Timing Statistics During Gameplay (#2)** — text widgets fed by judge_hook subscription.
11. **Export Step Data (#12)** — judge_hook subscription + file writer.
12. **Change Timing Windows (#1)** — config-driven write to threshold BSS slot at `0x18033c710` (or via the same judge_hook context).

Mods 9–11 share a single judgment-data subscription and could be one mod with optional sub-features.

## Gotchas

- **The pre-modded build's loader is fragile.** The `VirtualProtect` IAT extension repurposes existing IAT padding. If a future game build has a different IAT layout, the loader needs updating. Our universal hook DLL, by contrast, never touches the IAT — it loads as a separate module.

- **Timing-window thresholds use signed int32 milliseconds.** The pre-modded build's table at `0x1802da5e8` matches the units of the in-game judgment thresholds at `0x18033c710`. Pre-validate any custom timing window before writing.

- **The pre-modded operator-menu mirror at `0x1802c2199..0x1802c221c` is in `.rdata`.** That's a read-only section, but the loader marks the page RW via `VirtualProtect` at boot. If we don't run the same `VirtualProtect`, we can't write to mirror those configs. Our codebase doesn't need this mirror — it has its own config service — but be aware of this if cross-referencing.

- **`FUN_180077a00` is the central pacemaker/score render function.** Five separate mods (#2 timing stats, #4 Real Speed, #7 Pacemaker→MS, #12 Step Export, indirectly #15 Flare→Lamps for the banner-portion of results) all overlap on this function. Plan a single shared dispatcher when porting (analogous to `services/judge_hook`).

- **`FUN_18002e140` is the operator-menu config-refresh entry point in the pre-modded build, but only fires on state == 0 (boot).** R7+R8 inject all operator-menu reads here. This codebase's `mod-config.json` reader runs at DLL init and doesn't need this hook — none of the ported mods need to re-hook this point.

- **R23 (Special SSQ) and R38 (zeroed song-codes table) are paired.** The 148-byte zero-fill at R38 only makes sense in the context of R23's rewrite. Don't import either patch without the other.

- **`0x181270248` is "is either player's Start button held?"** — it calls `arkMDXGetStart` for both player slots. Several patches use this trampoline (R7's Force Event Mode trigger, R10's NOW LOADING boost, R19's Quick Fail / Skip Results / Premium Free force-logout). When porting, use `services/input_manager`'s held-Start detection rather than re-implementing this.

- **R10's "Optional Skip NOW LOADING on boot" boost vs. the always-on optimization in R11.** R11 (the function-epilogue tail-recurse) is the ~90% loading speedup that runs unconditionally. R10 is an additional cap-bypass triggered by holding Start. The existing Fast Bootup mod doesn't currently expose the held-Start boost — but it's a small extension if desired.

## Cross-Version Anchor Verification Summary

All anchors below were verified by Ghidra byte-pattern search against `gamemdx_20250805_STOCK.dll` and `gamemdx_20260421.dll`. The "matches" column is the count and known address(es) on each side.

| Mod | Anchor pattern | 20250805 stock | 20260421 stock | Notes |
|---|---|---|---|---|
| #2 (timing stats — banner LEA) | `48 8D 3D ?? ?? ?? ?? 48 8D 1D ?? ?? ?? ??` | unique @ `0x18000932b` | unique @ `0x18000980b` | Patch site is anchor + 3 (the disp32 of the LEA RDI). |
| #2 (timing stats — precision fmt) | `25 30 2E 31 6C 66` (`%0.1lf`) | unique @ `0x18034c5e8` | unique (same string) | Single ASCII byte change. |
| #2/etc DllMain entry (R2) | `48 83 EC 28 4C 8B C1 48 8D 15 ?? ?? ?? ?? 48 8D 0D ?? ?? ?? ?? FF 15 ?? ?? ?? ?? 33 C0 48 83 C4 28 C3` | unique @ `0x180003ec0` | unique @ `0x180003f30` | gameInit prologue. |
| #3 (Premium Free — stage inc) | `48 8B 08 FF 41 0C` | unique @ `0x1800300f0` | unique @ `0x180030593` | R9 site is anchor + 2 (the `inc dword` instruction). |
| #16 (Quick Fail / Premium Free force-logout) | `32 C0 EB 02 B0 01 B9 ?? 00 00 00 BA ?? 00 00 00 84 C0 0F 45 CA` | unique @ `0x1800b58db` | unique @ `0x1800bf099` | State IDs (immediates) wildcarded — they shifted from `0x20`/`0x37` to `0x21`/`0x39`. R19 site is anchor + 6. Same patch site implements both behaviors (Start-held during stage transition). |
| #11 (NOW LOADING — cap bypass when Start held) | `48 8B C2 48 C1 E8 3F 48 03 D0 48 3B D7 0F 87` | unique @ `0x180032971` | unique @ `0x180032be1` | R10 site is anchor + 7 (the `48 03 D0`). Inside `FUN_180032200` (CheckStepDataActor::onUpdate). |
| #4 (Real Speed — BPM divisor) | `F2 0F 5E 01 48 8D 4C 24 40` | unique @ `0x1801c98d8` | unique @ `0x1801df948` | R25 patch is anchor + 3 (the `01` ModR/M of `divsd xmm0, [rcx]`). |
| #4 (Real Speed — logf guard) | `0F 28 C7 E8 ?? ?? ?? ?? F3 0F 58 C6` | unique @ `0x180077be6` | unique @ `0x18007bc56` | R16 site is anchor + 4 (the rel32 of the `call`). |
| #5 (Force Platinum Pass) | `42 0F B6 BC 36 ?? ?? ?? ?? 44 8B CF 4C 8B 03 49 81 C0 ?? ?? 00 00` | 3 ordered hits: `0x18001426e`, `0x18001429f`, `0x1800142e2` | 3 ordered hits: `0x1800140ad`, `0x1800140de`, `0x180014121` | Use ordinal 1 and 3 to identify R5 and R6 patch sites. |
| #6 (Force Event Mode — state machine entry, **not portable**) | `48 8B C4 55 57 41 54 41 55 41 56 48 8D A8 ?? ?? FF FF 48 81 EC ?? 03 00 00` | unique @ `0x18002e140` | unique @ `0x18002e470` | FUN_18002e140 prologue. R7 patches the int3 pad just before this and the first 7 bytes of the prologue. Only relevant if reproducing the pre-modded banner-overwrite trick. |
| #6 (operator-menu refresh dispatcher, **not portable**) | `8B 8C 82 ?? ?? 03 00 48 03 CA FF E1` | unique @ `0x18002e240` | unique @ `0x18002e574` | R8 site. This codebase's `mod-config.json` reader replaces this entirely. |
| #7 (Pacemaker → MS) | `48 8B 01 85 F6 75 ?? F3 0F 10 0D` | unique @ `0x180077b88` | unique @ `0x18007bbf8` | R14 site. JNE displacement wildcarded (changed `+0x22` → `+0x20`). |
| Network bypass infrastructure (R18) | `48 8D 55 E7 48 8D 4D EF E8 ?? ?? ?? ?? 83 7D E7 02` | unique @ `0x1800acbbc` | unique @ `0x1800b508c` | R18 site is anchor + 8 (the `e8` rel32 call). Forces network common-event-load to skip and fall through to local-mode state. Not user-facing — supports the modder's offline operation. |
| #10 (Special SSQ) | `4C 8B D1 48 8D 3D ?? ?? ?? ?? 48 8B F2 B9 05 00 00 00 F3 A6` | unique @ `0x18019e8d0` | unique @ `0x1801b2d6a` | FUN_18019e8d0 prologue. R23 rewrites the entire function. |
| #12/#7 (Pacemaker→MS render hook R13) | `48 8B 97 B0 00 00 00` | unique @ `0x180077b36` | unique @ `0x18007bba6` | R13 site. Subsequent `movd xmm0, esi` encoding differs (with vs without operand-size prefix), don't extend pattern past byte 7. |
| #12 (Step Export R17) | `89 87 98 00 00 00 83 F9 06` | unique @ `0x180077d6f` | unique @ `0x18007bddf` | R17 site is anchor + 0 (the `mov [rdi+0x98], eax`). |
| #13 (Speed Toggle — up) | `83 C2 19 4C 8D 05 ?? ?? ?? ?? 48 8D 4C 24 30` | unique @ `0x1801ca147` | unique @ `0x1801e01b7` | R27 site. |
| #13 (Speed Toggle — down) | `83 C2 E7 4C 8D 05 ?? ?? ?? ?? 48 8D 4C 24 30` | unique @ `0x1801ca194` | unique @ `0x1801e0204` | R28 site. |
| #14 (Options Menu Controls — **not portable**, SD-cab QoL only) | `80 79 18 00 8B C2 4C 8B C1` | unique @ `0x18016abd4` | unique @ `0x18017fad4` | R22 site (Cursor::move). This codebase is HD-cab only, so not relevant. |
| #15 (Flare → Clear Lamps) | `48 8B 11 83 3A 01 0F 45 F0` | unique @ `0x180145fad` | unique @ `0x18015a9ad` | R21 site is anchor - 12 (the `call FUN_1800f2700`). |
| #11 (NOW LOADING — recursive tail-call) | derived from `CheckStepDataActor` RTTI vtable[6] | derivable | derivable | R11 site is the function epilogue. The function entry is what this codebase's `find_check_step_data_actor` already resolves; scan forward for the standard 5-callee-saved epilogue and patch the `c3` ret. |

**Pattern hygiene rules used:**
1. RIP-relative displacements (`?? ?? ?? ??`) wildcarded — they always change between versions because section layouts shift.
2. Branch displacements (Jcc, JMP rel8/rel32) wildcarded when the surrounding basic-block size is sensitive to compiler tuning.
3. State-id immediates wildcarded when the state-machine encoding could change.
4. Frame-allocation immediates (`sub rsp, ?`, `lea rbp, [rax+?]`) wildcarded when the function's frame size could change.
5. **Never** wildcarded: opcodes, ModR/M bytes, SIB bytes, register-encoding bytes, or any constant that's a structural game value (judgment-set IDs `0x4c1`/`0x4c2`, struct field offsets like `+0x1d`/`+0x1e`/`+0x1f`/`+0x88`/`+0xb0`/`+0x98`).
